use std::net::SocketAddr;
use std::sync::Arc;

use axum::{Router, middleware, routing::get};
use octofhir_fhir_model::provider::{FhirVersion, ModelProvider};
use octofhir_fhirpath::FhirPathEngine;
use octofhir_fhirschema::embedded::{FhirVersion as SchemaFhirVersion, get_schemas};
use octofhir_fhirschema::model_provider::DynamicSchemaProvider;
use octofhir_fhirschema::types::StructureDefinition;
use tower_http::{compression::CompressionLayer, cors::CorsLayer, trace::TraceLayer};

use crate::{
    config::{AppConfig, StorageBackend as ConfigBackend},
    handlers, middleware as app_middleware,
    storage_adapter::PostgresStorageAdapter,
};
use octofhir_db_memory::{
    DynStorage, StorageBackend as DbBackend, StorageConfig as DbStorageConfig,
    StorageOptions as DbStorageOptions, create_storage as create_memory_storage,
};
use octofhir_db_postgres::{PostgresConfig, PostgresStorage};
use octofhir_search::SearchConfig as EngineSearchConfig;

/// Shared model provider type for FHIRPath evaluation
pub type SharedModelProvider = Arc<dyn ModelProvider + Send + Sync>;

#[derive(Clone)]
pub struct AppState {
    pub storage: DynStorage,
    pub search_cfg: EngineSearchConfig,
    pub fhir_version: String,
    /// FHIRPath engine for FHIRPath Patch support
    pub fhirpath_engine: Option<Arc<FhirPathEngine>>,
    /// Model provider for FHIRPath evaluation (schema-aware)
    pub model_provider: Option<SharedModelProvider>,
}

pub struct OctofhirServer {
    addr: SocketAddr,
    app: Router,
}

/// Creates storage based on the configured backend.
///
/// For in-memory backend, this is synchronous.
/// For PostgreSQL, this must be called in an async context.
async fn create_storage(cfg: &AppConfig) -> Result<DynStorage, anyhow::Error> {
    match cfg.storage.backend {
        ConfigBackend::InMemoryPapaya => {
            let db_cfg = DbStorageConfig {
                backend: DbBackend::InMemoryPapaya,
                options: DbStorageOptions {
                    memory_limit_bytes: cfg.storage.memory_limit_bytes,
                    preallocate_items: cfg.storage.preallocate_items,
                },
            };
            Ok(create_memory_storage(&db_cfg))
        }
        ConfigBackend::Postgres => {
            let pg_cfg = cfg.storage.postgres.as_ref().ok_or_else(|| {
                anyhow::anyhow!("PostgreSQL config is required when backend is 'postgres'")
            })?;

            let postgres_config = PostgresConfig::new(&pg_cfg.url)
                .with_pool_size(pg_cfg.pool_size)
                .with_connect_timeout_ms(pg_cfg.connect_timeout_ms)
                .with_idle_timeout_ms(pg_cfg.idle_timeout_ms)
                .with_run_migrations(pg_cfg.run_migrations);

            let pg_storage = PostgresStorage::new(postgres_config)
                .await
                .map_err(|e| anyhow::anyhow!("Failed to create PostgreSQL storage: {e}"))?;

            let adapter = PostgresStorageAdapter::new(pg_storage);
            Ok(Arc::new(adapter))
        }
    }
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

/// Builds the application router with the given configuration (async version).
///
/// Use this when PostgreSQL backend may be configured.
pub async fn build_app_async(cfg: &AppConfig) -> Result<Router, anyhow::Error> {
    let body_limit = cfg.server.body_limit_bytes;
    let storage = create_storage(cfg).await?;

    // Build search engine config using counts from AppConfig
    let search_cfg = EngineSearchConfig {
        default_count: cfg.search.default_count,
        max_count: cfg.search.max_count,
        ..Default::default()
    };

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
    let fhirpath_engine = match FhirPathEngine::new(registry, model_provider.clone()).await {
        Ok(engine) => {
            tracing::info!(
                "FHIRPath engine initialized successfully with schema-aware model provider"
            );
            Some(Arc::new(engine))
        }
        Err(e) => {
            tracing::warn!(
                "Failed to initialize FHIRPath engine: {}. FHIRPath Patch will not be available.",
                e
            );
            None
        }
    };

    let state = AppState {
        storage,
        search_cfg,
        fhir_version: cfg.fhir.version.clone(),
        fhirpath_engine,
        model_provider: Some(model_provider),
    };

    Ok(build_router(state, body_limit))
}

/// Builds the application router with the given configuration (sync version).
///
/// Note: This only works with in-memory backend. Use `build_app_async` for PostgreSQL.
/// FHIRPath Patch is not available in sync mode - use `build_app_async` for full feature support.
pub fn build_app(cfg: &AppConfig) -> Router {
    let body_limit = cfg.server.body_limit_bytes;

    // Build storage from server config (in-memory backend only)
    let db_cfg = DbStorageConfig {
        backend: DbBackend::InMemoryPapaya,
        options: DbStorageOptions {
            memory_limit_bytes: cfg.storage.memory_limit_bytes,
            preallocate_items: cfg.storage.preallocate_items,
        },
    };
    let storage = create_memory_storage(&db_cfg);

    // Build search engine config using counts from AppConfig
    let search_cfg = EngineSearchConfig {
        default_count: cfg.search.default_count,
        max_count: cfg.search.max_count,
        ..Default::default()
    };

    let state = AppState {
        storage,
        search_cfg,
        fhir_version: cfg.fhir.version.clone(),
        // FHIRPath engine requires async initialization, not available in sync mode
        fhirpath_engine: None,
        model_provider: None,
    };

    build_router(state, body_limit)
}

fn build_router(state: AppState, body_limit: usize) -> Router {
    Router::new()
        // Health and info endpoints
        .route("/", get(handlers::root))
        .route("/healthz", get(handlers::healthz))
        .route("/readyz", get(handlers::readyz))
        .route("/metadata", get(handlers::metadata))
        // Browser favicon shortcut
        .route("/favicon.ico", get(handlers::favicon))
        // New API endpoints for UI
        .route("/api/health", get(handlers::api_health))
        .route("/api/build-info", get(handlers::api_build_info))
        .route("/api/resource-types", get(handlers::api_resource_types))
        // Embedded UI under /ui
        .route("/ui", get(handlers::ui_index))
        .route("/ui/{*path}", get(handlers::ui_static))
        // CRUD, search, and versioned read endpoints
        .route(
            "/{resource_type}",
            get(handlers::search_resource)
                .post(handlers::create_resource)
                .put(handlers::conditional_update_resource)
                .patch(handlers::conditional_patch_resource)
                .delete(handlers::conditional_delete_resource),
        )
        // Vread: GET /[type]/[id]/_history/[vid]
        .route(
            "/{resource_type}/{id}/_history/{version_id}",
            get(handlers::vread_resource),
        )
        .route(
            "/{resource_type}/{id}",
            get(handlers::read_resource)
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

    /// Builds the server synchronously (in-memory storage only).
    ///
    /// Use `build_async` for PostgreSQL backend.
    pub fn build(self) -> OctofhirServer {
        let app = build_app(&self.config);

        OctofhirServer {
            addr: self.addr,
            app,
        }
    }

    /// Builds the server asynchronously (supports all backends).
    ///
    /// This is the recommended method when PostgreSQL backend may be configured.
    pub async fn build_async(self) -> Result<OctofhirServer, anyhow::Error> {
        let app = build_app_async(&self.config).await?;

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
