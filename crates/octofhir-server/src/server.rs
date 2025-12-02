use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::{Router, middleware, routing::get};
use octofhir_fhir_model::provider::{FhirVersion, ModelProvider};
use octofhir_fhirpath::FhirPathEngine;
use octofhir_fhirschema::embedded::{FhirVersion as SchemaFhirVersion, get_schemas};
use octofhir_fhirschema::model_provider::DynamicSchemaProvider;
use octofhir_fhirschema::types::StructureDefinition;
use tower_http::{compression::CompressionLayer, cors::CorsLayer, trace::TraceLayer};

use crate::operations::{DynOperationHandler, OperationRegistry, register_core_operations};
use crate::validation::ValidationService;

use crate::{
    config::AppConfig, handlers, middleware as app_middleware,
    storage_adapter::PostgresStorageAdapter,
};
use octofhir_db_postgres::{PostgresConfig, PostgresStorage};
use octofhir_search::SearchConfig as EngineSearchConfig;
use octofhir_storage::legacy::DynStorage;

/// Shared model provider type for FHIRPath evaluation
pub type SharedModelProvider = Arc<dyn ModelProvider + Send + Sync>;

#[derive(Clone)]
pub struct AppState {
    pub storage: DynStorage,
    pub search_cfg: EngineSearchConfig,
    pub fhir_version: String,
    /// Base URL for the server, used in links and responses
    pub base_url: String,
    /// FHIRPath engine for FHIRPath Patch support
    pub fhirpath_engine: Arc<FhirPathEngine>,
    /// Model provider for FHIRPath evaluation (schema-aware)
    pub model_provider: SharedModelProvider,
    /// Registry of available FHIR operations loaded from packages
    pub operation_registry: Arc<OperationRegistry>,
    /// Map of operation handlers by operation code
    pub operation_handlers: Arc<HashMap<String, DynOperationHandler>>,
    /// Validation service for FHIR resource validation with FHIRPath constraints
    pub validation_service: ValidationService,
    /// Gateway router for dynamic API endpoints
    pub gateway_router: crate::gateway::GatewayRouter,
    /// PostgreSQL connection pool for SQL handler
    pub db_pool: Arc<sqlx_postgres::PgPool>,
    /// Custom handler registry for gateway operations
    pub handler_registry: Arc<crate::gateway::HandlerRegistry>,
    /// Compartment registry for compartment-based search
    pub compartment_registry: Arc<crate::compartments::CompartmentRegistry>,
    /// Async job manager for FHIR asynchronous request pattern
    pub async_job_manager: Arc<crate::async_jobs::AsyncJobManager>,
}

pub struct OctofhirServer {
    addr: SocketAddr,
    app: Router,
}

/// Bootstraps conformance resources for PostgreSQL backend.
///
/// This function:
/// 1. Creates conformance storage
/// 2. Loads bootstrap resources from igs/octofhir-internal/
/// 3. Syncs to canonical manager
/// 4. Starts hot-reload listener
async fn bootstrap_conformance_if_postgres(cfg: &AppConfig) -> Result<(), anyhow::Error> {
    use octofhir_db_postgres::{HotReloadBuilder, PostgresConformanceStorage, sync_and_load};
    use std::path::PathBuf;

    let pg_cfg = cfg.storage.postgres.as_ref().ok_or_else(|| {
        anyhow::anyhow!("PostgreSQL config is required for conformance bootstrap")
    })?;

    // Create PostgresStorage first to get access to the pool
    let postgres_config = octofhir_db_postgres::PostgresConfig::new(pg_cfg.connection_url())
        .with_pool_size(pg_cfg.pool_size)
        .with_connect_timeout_ms(pg_cfg.connect_timeout_ms)
        .with_idle_timeout_ms(pg_cfg.idle_timeout_ms)
        .with_run_migrations(true);

    let pg_storage = octofhir_db_postgres::PostgresStorage::new(postgres_config).await?;
    let conformance_storage = Arc::new(PostgresConformanceStorage::new(pg_storage.pool().clone()));

    // Bootstrap conformance resources
    match crate::bootstrap::bootstrap_conformance_resources(&conformance_storage).await {
        Ok(stats) if stats.total() > 0 => {
            tracing::info!(
                structure_definitions = stats.structure_definitions,
                value_sets = stats.value_sets,
                code_systems = stats.code_systems,
                "Bootstrapped conformance resources"
            );
        }
        Ok(_) => {
            tracing::debug!("Conformance resources already bootstrapped");
        }
        Err(e) => {
            tracing::warn!(error = %e, "Failed to bootstrap conformance resources");
        }
    }

    // Sync to canonical manager
    let base_dir = PathBuf::from(cfg.packages.path.as_deref().unwrap_or(".fhir"));

    let canonical_manager = crate::canonical::get_manager();

    match sync_and_load(&conformance_storage, &base_dir, canonical_manager.as_ref()).await {
        Ok(package_dir) => {
            tracing::info!(
                package_dir = %package_dir.display(),
                "Synced internal conformance resources to canonical manager"
            );
        }
        Err(e) => {
            tracing::warn!(
                error = %e,
                "Failed to sync conformance resources to canonical manager"
            );
        }
    }

    // Ensure we have a canonical manager (required)
    let canonical_manager = if let Some(manager) = canonical_manager {
        manager
    } else {
        // Create a default FcmConfig
        let config = octofhir_canonical_manager::FcmConfig::default();
        let manager = octofhir_canonical_manager::CanonicalManager::new(config)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to create canonical manager: {}", e))?;
        Arc::new(manager)
    };

    // Start hot-reload listener to monitor conformance changes
    match HotReloadBuilder::new(pg_storage.pool().clone())
        .with_conformance_storage(conformance_storage)
        .with_canonical_manager(canonical_manager)
        .with_base_dir(base_dir)
        .start()
    {
        Ok(_handle) => {
            tracing::info!("Hot-reload listener started for conformance resources");
            // Handle is dropped here, but the task continues running in the background
        }
        Err(e) => {
            tracing::warn!(
                error = %e,
                "Failed to start hot-reload listener"
            );
        }
    }

    Ok(())
}

/// Creates PostgreSQL storage.
///
/// Returns (storage, PostgreSQL pool for SQL handler).
async fn create_storage(
    cfg: &AppConfig,
) -> Result<(DynStorage, Arc<sqlx_postgres::PgPool>), anyhow::Error> {
    let pg_cfg = cfg
        .storage
        .postgres
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("PostgreSQL config is required"))?;

    let postgres_config = PostgresConfig::new(pg_cfg.connection_url())
        .with_pool_size(pg_cfg.pool_size)
        .with_connect_timeout_ms(pg_cfg.connect_timeout_ms)
        .with_idle_timeout_ms(pg_cfg.idle_timeout_ms)
        .with_run_migrations(true);

    let pg_storage = PostgresStorage::new(postgres_config)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to create PostgreSQL storage: {e}"))?;

    // Get the pool reference for SQL handler
    let pool = pg_storage.pool().clone();

    let adapter = PostgresStorageAdapter::new(pg_storage);
    Ok((Arc::new(adapter), Arc::new(pool)))
}

/// Loads FHIR StructureDefinitions from the canonical manager's packages.
/// Returns StructureDefinitions if packages are loaded, empty Vec otherwise.
async fn load_structure_definitions_from_packages() -> Vec<StructureDefinition> {
    use octofhir_canonical_manager::search::SearchQuery;

    // Try to get StructureDefinitions from canonical manager
    if let Some(manager) = crate::canonical::get_manager() {
        // Query for all StructureDefinitions in loaded packages
        let query = SearchQuery {
            resource_types: vec!["StructureDefinition".to_string()],
            ..Default::default()
        };

        match manager.search_engine().search(&query).await {
            Ok(results) => {
                let mut structure_definitions = Vec::new();

                for resource_match in results.resources {
                    // Parse as StructureDefinition
                    if let Ok(sd) = serde_json::from_value::<StructureDefinition>(
                        resource_match.resource.content,
                    ) {
                        structure_definitions.push(sd);
                    }
                }

                if !structure_definitions.is_empty() {
                    tracing::info!(
                        "Loaded {} StructureDefinitions from canonical manager packages",
                        structure_definitions.len()
                    );
                    return structure_definitions;
                }
            }
            Err(e) => {
                tracing::warn!(
                    "Failed to query StructureDefinitions from canonical manager: {}",
                    e
                );
            }
        }
    }

    Vec::new()
}

/// Builds the application router with the given configuration.
pub async fn build_app(cfg: &AppConfig) -> Result<Router, anyhow::Error> {
    let body_limit = cfg.server.body_limit_bytes;
    let (storage, db_pool) = create_storage(cfg).await?;

    // Bootstrap conformance resources
    if let Err(e) = bootstrap_conformance_if_postgres(cfg).await {
        tracing::warn!(error = %e, "Failed to bootstrap conformance resources");
    }

    // Build search parameter registry from canonical manager (REQUIRED)
    let search_registry = match crate::canonical::get_manager() {
        Some(manager) => match crate::canonical::build_search_registry(&manager).await {
            Ok(registry) => {
                tracing::info!(
                    params_loaded = registry.len(),
                    "Search parameter registry built from canonical manager"
                );
                Arc::new(registry)
            }
            Err(e) => {
                return Err(anyhow::anyhow!(
                    "Failed to build search parameter registry: {}. Server cannot start without search parameters.",
                    e
                ));
            }
        },
        None => {
            return Err(anyhow::anyhow!(
                "Canonical manager not available. Server cannot start without search parameters."
            ));
        }
    };

    // Build search engine config using counts from AppConfig and registry
    let search_cfg = EngineSearchConfig::new(search_registry)
        .with_counts(cfg.search.default_count, cfg.search.max_count);

    // Initialize FHIRPath engine with schema-aware model provider
    let fhir_version = match cfg.fhir.version.as_str() {
        "R4" | "4.0" | "4.0.1" => FhirVersion::R4,
        "R4B" | "4.3" | "4.3.0" => FhirVersion::R4B,
        "R5" | "5.0" | "5.0.0" => FhirVersion::R5,
        "R6" | "6.0" => FhirVersion::R6,
        _ => FhirVersion::R4, // Default to R4
    };

    // Convert to schema version for embedded schemas (fallback)
    let schema_version = match fhir_version {
        FhirVersion::R4 => SchemaFhirVersion::R4,
        FhirVersion::R4B => SchemaFhirVersion::R4B,
        FhirVersion::R5 => SchemaFhirVersion::R5,
        FhirVersion::R6 => SchemaFhirVersion::R6,
        _ => SchemaFhirVersion::R4,
    };

    // Decide schema source based on config:
    // - If packages are configured -> use DynamicSchemaProvider with StructureDefinitions from packages
    // - If only FHIR version is set -> use embedded schemas
    let has_packages_configured = !cfg.packages.load.is_empty();

    let model_provider: SharedModelProvider = if has_packages_configured {
        // Load StructureDefinitions from canonical manager packages
        let structure_definitions = load_structure_definitions_from_packages().await;

        if !structure_definitions.is_empty() {
            // DynamicSchemaProvider handles StructureDefinition -> FhirSchema translation internally
            let provider = DynamicSchemaProvider::from_structure_definitions(
                structure_definitions,
                fhir_version,
            );
            tracing::info!(
                "Initialized dynamic FHIR model provider for version {:?} with {} schemas from configured packages",
                cfg.fhir.version,
                provider.schema_count()
            );
            Arc::new(provider)
        } else {
            // Packages configured but failed to load - fallback to embedded
            tracing::warn!(
                "Packages configured but no StructureDefinitions loaded, falling back to embedded schemas"
            );
            let schemas = get_schemas(schema_version).clone();
            let schema_count = schemas.len();
            let provider = DynamicSchemaProvider::new(schemas, fhir_version);
            tracing::info!(
                "Initialized dynamic FHIR model provider for version {:?} with {} embedded schemas (fallback)",
                cfg.fhir.version,
                schema_count
            );
            Arc::new(provider)
        }
    } else {
        // No packages configured - use embedded schemas for the configured FHIR version
        let schemas = get_schemas(schema_version).clone();
        let schema_count = schemas.len();
        let provider = DynamicSchemaProvider::new(schemas, fhir_version);
        tracing::info!(
            "Initialized FHIR model provider for version {:?} with {} embedded schemas",
            cfg.fhir.version,
            schema_count
        );
        Arc::new(provider)
    };

    // Create FHIRPath function registry and engine
    let registry = Arc::new(octofhir_fhirpath::create_function_registry());
    let fhirpath_engine = Arc::new(
        FhirPathEngine::new(registry, model_provider.clone())
            .await
            .map_err(|e| anyhow::anyhow!("Failed to initialize FHIRPath engine: {}", e))?,
    );

    tracing::info!("FHIRPath engine initialized successfully with schema-aware model provider");

    // Load operation definitions from canonical manager
    let operation_registry = match crate::operations::load_operations().await {
        Ok(mut registry) => {
            tracing::info!(
                count = registry.len(),
                "Loaded operation definitions from packages"
            );
            // Manually register $validate at system level (FHIR spec supports it but package may not)
            registry.register(crate::operations::OperationDefinition {
                code: "validate".to_string(),
                url: "http://hl7.org/fhir/OperationDefinition/Resource-validate".to_string(),
                kind: crate::operations::OperationKind::Operation,
                system: true,
                type_level: true,
                instance: true,
                resource: vec![], // All resource types
                parameters: vec![],
                affects_state: false,
            });
            tracing::info!("Registered $validate at system level");
            Arc::new(registry)
        }
        Err(e) => {
            tracing::warn!(error = %e, "Failed to load operation definitions, using empty registry");
            let mut registry = OperationRegistry::new();
            // Register $validate even when packages fail to load
            registry.register(crate::operations::OperationDefinition {
                code: "validate".to_string(),
                url: "http://hl7.org/fhir/OperationDefinition/Resource-validate".to_string(),
                kind: crate::operations::OperationKind::Operation,
                system: true,
                type_level: true,
                instance: true,
                resource: vec![],
                parameters: vec![],
                affects_state: false,
            });
            Arc::new(registry)
        }
    };

    // Register core operation handlers
    let operation_handlers: Arc<HashMap<String, DynOperationHandler>> = Arc::new(
        register_core_operations(fhirpath_engine.clone(), model_provider.clone()),
    );
    tracing::info!(
        count = operation_handlers.len(),
        "Registered operation handlers"
    );

    // Initialize ValidationService with FHIRPath constraint support
    let validation_service =
        ValidationService::new(model_provider.clone(), fhirpath_engine.clone())
            .await
            .map_err(|e| anyhow::anyhow!("Failed to initialize validation service: {}", e))?;
    tracing::info!("Validation service initialized with FHIRPath constraint support");

    // Initialize gateway router and load initial routes
    let gateway_router = Arc::new(crate::gateway::GatewayRouter::new());
    match gateway_router.reload_routes(&storage).await {
        Ok(count) => {
            tracing::info!(count = count, "Loaded initial gateway routes");
        }
        Err(e) => {
            tracing::warn!(error = %e, "Failed to load initial gateway routes");
        }
    }

    // Start gateway hot-reload listener
    match crate::gateway::GatewayReloadBuilder::new()
        .with_pool(db_pool.as_ref().clone())
        .with_gateway_router(gateway_router.clone())
        .with_storage(storage.clone())
        .start()
    {
        Ok(_handle) => {
            tracing::info!("Gateway hot-reload listener started");
            // Handle is dropped here, but the task continues running
        }
        Err(e) => {
            tracing::warn!(
                error = %e,
                "Failed to start gateway hot-reload listener"
            );
        }
    }

    // Initialize custom handler registry
    let handler_registry = Arc::new(crate::gateway::HandlerRegistry::new());
    tracing::info!("Initialized custom handler registry");

    // Initialize compartment registry from canonical manager
    let compartment_registry = match crate::canonical::get_manager() {
        Some(manager) => {
            match crate::compartments::CompartmentRegistry::from_canonical_manager(&manager).await {
                Ok(registry) => {
                    tracing::info!(
                        compartments = %registry.list_compartments().join(", "),
                        "Compartment registry initialized"
                    );
                    Arc::new(registry)
                }
                Err(e) => {
                    tracing::warn!(error = %e, "Failed to load compartment definitions, using empty registry");
                    Arc::new(crate::compartments::CompartmentRegistry::new())
                }
            }
        }
        None => {
            tracing::warn!(
                "Canonical manager not available, compartment search will not be available"
            );
            Arc::new(crate::compartments::CompartmentRegistry::new())
        }
    };

    // Initialize async job manager
    let async_job_config = crate::async_jobs::AsyncJobConfig::default();
    let async_job_manager = Arc::new(crate::async_jobs::AsyncJobManager::new(
        db_pool.clone(),
        async_job_config,
    ));
    tracing::info!("Async job manager initialized");

    // Start background cleanup task for expired jobs
    let _cleanup_handle = async_job_manager.clone().start_cleanup_task();
    tracing::info!("Async job cleanup task started");
    // Note: _cleanup_handle is dropped here but the task continues running in the background

    let state = AppState {
        storage,
        search_cfg,
        fhir_version: cfg.fhir.version.clone(),
        base_url: cfg.base_url(),
        fhirpath_engine,
        model_provider,
        operation_registry,
        operation_handlers,
        validation_service,
        gateway_router: (*gateway_router).clone(),
        db_pool,
        handler_registry,
        compartment_registry,
        async_job_manager,
    };

    Ok(build_router(state, body_limit))
}

fn build_router(state: AppState, body_limit: usize) -> Router {
    // Create gateway router for dynamic API endpoints
    let gateway_routes = crate::gateway::GatewayRouter::create_router();

    Router::new()
        // Health and info endpoints
        .route("/", get(handlers::root).post(handlers::transaction_handler))
        .route("/healthz", get(handlers::healthz))
        .route("/readyz", get(handlers::readyz))
        .route("/metadata", get(handlers::metadata))
        // Browser favicon shortcut
        .route("/favicon.ico", get(handlers::favicon))
        // Merge gateway routes (handles /api/* dynamically)
        .merge(gateway_routes)
        // New API endpoints for UI
        .route("/api/health", get(handlers::api_health))
        .route("/api/build-info", get(handlers::api_build_info))
        .route("/api/resource-types", get(handlers::api_resource_types))
        // Embedded UI under /ui
        .route("/ui", get(handlers::ui_index))
        .route("/ui/{*path}", get(handlers::ui_static))
        // Async job status endpoints (FHIR asynchronous request pattern)
        .route(
            "/_async-status/{job_id}",
            get(handlers::async_job_status).delete(handlers::async_job_cancel),
        )
        .route(
            "/_async-status/{job_id}/result",
            get(handlers::async_job_result),
        )
        // System search: GET /?_type=... or POST /_search
        .route("/_search", axum::routing::post(handlers::system_search))
        // System history: GET /_history
        .route("/_history", get(handlers::system_history))
        // Type history: GET /{type}/_history (before CRUD route)
        .route("/{resource_type}/_history", get(handlers::type_history))
        // POST search: /{type}/_search
        .route(
            "/{resource_type}/_search",
            axum::routing::post(handlers::search_resource_post),
        )
        // Compartment search routes (must come before instance-level routes)
        // GET /Patient/123/* - all resources in Patient/123 compartment
        .route("/Patient/{id}/*", get(handlers::compartment_search_all))
        .route("/Encounter/{id}/*", get(handlers::compartment_search_all))
        .route(
            "/Practitioner/{id}/*",
            get(handlers::compartment_search_all),
        )
        .route(
            "/RelatedPerson/{id}/*",
            get(handlers::compartment_search_all),
        )
        .route("/Device/{id}/*", get(handlers::compartment_search_all))
        // GET /Patient/123/Observation - resources in compartment
        .route(
            "/Patient/{id}/{resource_type}",
            get(handlers::compartment_search),
        )
        .route(
            "/Encounter/{id}/{resource_type}",
            get(handlers::compartment_search),
        )
        .route(
            "/Practitioner/{id}/{resource_type}",
            get(handlers::compartment_search),
        )
        .route(
            "/RelatedPerson/{id}/{resource_type}",
            get(handlers::compartment_search),
        )
        .route(
            "/Device/{id}/{resource_type}",
            get(handlers::compartment_search),
        )
        // CRUD, search, and system operations
        // This merged route handles both /$operation and /ResourceType
        .route(
            "/{resource_type}",
            get(crate::operations::merged_root_get_handler)
                .post(crate::operations::merged_root_post_handler)
                .put(handlers::conditional_update_resource)
                .patch(handlers::conditional_patch_resource)
                .delete(handlers::conditional_delete_resource),
        )
        // Instance-level operations: GET/POST /{type}/{id}/$operation
        .route(
            "/{resource_type}/{id}/{operation}",
            get(crate::operations::instance_operation_handler)
                .post(crate::operations::instance_operation_handler),
        )
        // Instance history: GET /{type}/{id}/_history (before vread)
        .route(
            "/{resource_type}/{id}/_history",
            get(handlers::instance_history),
        )
        // Vread: GET /[type]/[id]/_history/[vid]
        .route(
            "/{resource_type}/{id}/_history/{version_id}",
            get(handlers::vread_resource),
        )
        // Type operations and resource CRUD
        // This merged route handles both /{type}/$operation and /{type}/{id}
        .route(
            "/{resource_type}/{id}",
            get(crate::operations::merged_type_get_handler)
                .post(crate::operations::merged_type_post_handler)
                .put(handlers::update_resource)
                .patch(handlers::patch_resource)
                .delete(handlers::delete_resource),
        )
        // Middleware stack (order: request id -> content negotiation -> compression/cors/trace -> body limit)
        .layer(middleware::from_fn(app_middleware::request_id))
        .layer(middleware::from_fn(app_middleware::content_negotiation))
        .layer(CorsLayer::permissive())
        .layer(CompressionLayer::new())
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(|req: &axum::http::Request<_>| {
                    use tracing::field::Empty;
                    // Skip creating a span for browser favicon requests to avoid noisy logs
                    if req.uri().path() == "/favicon.ico" {
                        return tracing::span!(tracing::Level::TRACE, "noop");
                    }
                    let method = req.method().clone();
                    let uri = req.uri().clone();
                    let req_id = req
                        .extensions()
                        .get::<axum::http::HeaderValue>()
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or("")
                        .to_string();
                    tracing::info_span!(
                        "http.request",
                        http.method = %method,
                        http.target = %uri,
                        http.route = Empty,
                        http.status_code = Empty,
                        request_id = %req_id
                    )
                })
                .on_response(
                    |res: &axum::http::Response<_>,
                     latency: std::time::Duration,
                     span: &tracing::Span| {
                        // Record status on the span; access log emission is handled only for non-favicon paths via the span field presence
                        span.record(
                            "http.status_code",
                            tracing::field::display(res.status().as_u16()),
                        );
                        // Determine if this span is our real request span by checking that it has the http.method field recorded (noop span won't)
                        // Unfortunately Span API doesn't expose field inspection, so we conservatively avoid extra logic and instead rely on make_span_with to avoid logging favicon.
                        // Thus, only emit the access log if the span's metadata target matches our request span name.
                        if let Some(meta) = span.metadata()
                            && meta.name() != "noop"
                        {
                            tracing::info!(
                                http.status = %res.status().as_u16(),
                                elapsed_ms = %latency.as_millis(),
                                "request handled"
                            );
                        }
                    },
                ),
        )
        .with_state(state)
        .layer(axum::extract::DefaultBodyLimit::max(body_limit))
}

pub struct ServerBuilder {
    addr: SocketAddr,
    config: AppConfig,
}

impl ServerBuilder {
    pub fn new() -> Self {
        let cfg = AppConfig::default();
        Self {
            addr: cfg.addr(),
            config: cfg,
        }
    }

    pub fn with_addr(mut self, addr: SocketAddr) -> Self {
        self.addr = addr;
        self
    }

    pub fn with_config(mut self, cfg: AppConfig) -> Self {
        self.addr = cfg.addr();
        self.config = cfg;
        self
    }

    /// Builds the server asynchronously.
    pub async fn build(self) -> Result<OctofhirServer, anyhow::Error> {
        let app = build_app(&self.config).await?;

        Ok(OctofhirServer {
            addr: self.addr,
            app,
        })
    }
}

impl Default for ServerBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl OctofhirServer {
    pub async fn run(self) -> anyhow::Result<()> {
        let listener = tokio::net::TcpListener::bind(self.addr).await?;
        tracing::info!("listening on {}", self.addr);
        axum::serve(listener, self.app)
            .with_graceful_shutdown(shutdown_signal())
            .await?;
        Ok(())
    }
}

async fn shutdown_signal() {
    // Wait for Ctrl+C
    let _ = tokio::signal::ctrl_c().await;
    tracing::info!("shutdown signal received");
}
