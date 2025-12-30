use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Context;
use arc_swap::ArcSwap;
use axum::{
    Router,
    extract::FromRef,
    middleware,
    routing::{get, post},
};
use octofhir_auth::middleware::AuthState;
use octofhir_auth::policy::{
    PolicyCache, PolicyChangeNotifier, PolicyEvaluator, PolicyEvaluatorConfig, PolicyReloadService,
    ReloadConfig,
};
use octofhir_auth::token::jwt::{JwtService, SigningAlgorithm, SigningKeyPair};
use octofhir_auth_postgres::{
    ArcClientStorage, ArcRevokedTokenStorage, ArcUserStorage, PolicyListener,
    PostgresPolicyStorageAdapter,
};

use crate::middleware::AuthorizationState;
use crate::model_provider::OctoFhirModelProvider;
use octofhir_fhir_model::provider::{FhirVersion, ModelProvider};
use octofhir_fhirpath::FhirPathEngine;
// Note: FhirSchemas are now loaded on-demand from the database
// The model provider uses an LRU cache to avoid repeated DB queries
use octofhir_db_postgres::PostgresPackageStore;
use octofhir_graphql::handler::{GraphQLContextTemplate, GraphQLState};
use octofhir_graphql::{FhirSchemaBuilder, InMemoryModelProvider, LazySchema, SchemaBuilderConfig};
use time::Duration;
use tower_http::{compression::CompressionLayer, trace::TraceLayer};

use crate::operation_registry::OperationRegistryService;
use crate::operations::{DynOperationHandler, OperationRegistry, register_core_operations_full};
use crate::reference_resolver::StorageReferenceResolver;
use crate::validation::ValidationService;

use crate::audit::AuditService;
use crate::cache::AuthContextCache;
use crate::{config::AppConfig, handlers, middleware as app_middleware};
use octofhir_db_postgres::PostgresConfig;
use octofhir_db_postgres::PostgresStorage;
use octofhir_fhir_model::terminology::TerminologyProvider;
use octofhir_fhirschema::TerminologyProviderAdapter;
use octofhir_search::{HybridTerminologyProvider, SearchConfig as EngineSearchConfig};
use octofhir_storage::DynStorage;

/// Shared model provider type for FHIRPath evaluation
pub type SharedModelProvider = Arc<dyn ModelProvider + Send + Sync>;

/// Application state shared across all requests.
///
/// Wrapped in Arc for cheap cloning - a single Arc::clone instead of
/// cloning 25+ individual Arc fields on every request.
#[derive(Clone)]
pub struct AppState(pub Arc<AppStateInner>);

impl std::ops::Deref for AppState {
    type Target = AppStateInner;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Inner application state containing all shared server resources.
pub struct AppStateInner {
    pub storage: DynStorage,
    pub search_cfg: EngineSearchConfig,
    pub fhir_version: String,
    /// Base URL for the server, used in links and responses
    pub base_url: String,
    /// FHIRPath engine for FHIRPath Patch support
    pub fhirpath_engine: Arc<FhirPathEngine>,
    /// Model provider for validation, FHIRPath, LSP, and all server features
    pub model_provider: Arc<OctoFhirModelProvider>,
    /// Registry of available FHIR operations loaded from packages
    pub fhir_operations: Arc<OperationRegistry>,
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
    /// Application configuration for runtime access
    pub config: Arc<AppConfig>,
    /// Policy evaluator for access control
    pub policy_evaluator: Arc<PolicyEvaluator>,
    /// Policy cache for hot-reload
    pub policy_cache: Arc<PolicyCache>,
    /// Policy reload service for hot-reload
    pub policy_reload_service: Arc<PolicyReloadService>,
    /// Authentication state for token validation
    pub auth_state: AuthState,
    /// GraphQL state for GraphQL handlers
    pub graphql_state: Option<GraphQLState>,
    /// Operation registry (source of truth for public paths, middleware lookups)
    pub operation_registry: Arc<OperationRegistryService>,
    /// Cache for authenticated contexts (reduces DB queries per request)
    pub auth_cache: Arc<dyn AuthContextCache>,
    /// Cache for JWT verification (reduces signature verification overhead)
    pub jwt_cache: Arc<crate::cache::JwtVerificationCache>,
    /// Cache for JSON Schema conversions (FhirSchema -> JSON Schema)
    pub json_schema_cache: Arc<dashmap::DashMap<String, serde_json::Value>>,
    /// Cached resource types for fast validation
    pub resource_type_set: Arc<ArcSwap<HashSet<String>>>,
    /// Cached CapabilityStatement (built at startup)
    pub capability_statement: Arc<serde_json::Value>,
    /// Configuration manager for runtime config and feature flags
    pub config_manager: Option<Arc<octofhir_config::ConfigurationManager>>,
    /// Audit service for creating FHIR AuditEvent resources
    pub audit_service: Arc<AuditService>,
}

// =============================================================================
// FromRef Implementations for Middleware States
// =============================================================================

impl FromRef<AppState> for crate::middleware::ExtendedAuthState {
    fn from_ref(state: &AppState) -> Self {
        crate::middleware::ExtendedAuthState::new(
            state.auth_state.clone(),
            state.operation_registry.clone(),
            state.auth_cache.clone(),
            state.jwt_cache.clone(),
        )
    }
}

impl FromRef<AppState> for AuthState {
    fn from_ref(state: &AppState) -> Self {
        state.auth_state.clone()
    }
}

impl FromRef<AppState> for AuthorizationState {
    fn from_ref(state: &AppState) -> Self {
        AuthorizationState::new(
            state.policy_evaluator.clone(),
            state.operation_registry.clone(),
        )
    }
}

impl FromRef<AppState> for GraphQLState {
    fn from_ref(state: &AppState) -> Self {
        state
            .graphql_state
            .clone()
            .expect("GraphQLState not initialized in AppState")
    }
}

impl FromRef<AppState> for crate::admin::AdminState {
    fn from_ref(state: &AppState) -> Self {
        crate::admin::AdminState::new(state.db_pool.clone())
    }
}

impl FromRef<AppState> for crate::admin::ConfigState {
    fn from_ref(state: &AppState) -> Self {
        crate::admin::ConfigState::new(
            state
                .config_manager
                .clone()
                .expect("ConfigurationManager not initialized in AppState"),
        )
    }
}

pub struct OctofhirServer {
    addr: SocketAddr,
    app: Router,
}

/// Bootstraps auth resources and database tables.
///
/// This function:
/// 1. Creates database tables for all FHIR resource types from FCM packages
/// 2. Bootstraps auth resources (admin user, default UI client, access policy)
async fn bootstrap_conformance_if_postgres(cfg: &AppConfig) -> Result<(), anyhow::Error> {
    use octofhir_auth_postgres::{ArcClientStorage, ArcUserStorage};

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

    // Create database tables for all FHIR resource types from FCM packages
    // This includes all resource-kind and logical-kind StructureDefinitions
    let fcm_storage = octofhir_db_postgres::PostgresPackageStore::new(pg_storage.pool().clone());
    match fcm_storage.ensure_resource_tables().await {
        Ok(count) => {
            tracing::info!(
                tables_created = count,
                "Ensured database tables for FHIR resource types from FCM"
            );
        }
        Err(e) => {
            tracing::warn!(
                error = %e,
                "Failed to ensure database tables for FHIR resource types"
            );
        }
    }

    // Bootstrap auth resources (admin user, default UI client)
    // Tables are automatically created when StructureDefinitions are loaded above
    let pool = Arc::new(pg_storage.pool().clone());

    // Bootstrap default UI client
    let client_storage = ArcClientStorage::new(pool.clone());
    let issuer = &cfg.auth.issuer;
    match crate::bootstrap::bootstrap_default_ui_client(&client_storage, issuer).await {
        Ok(true) => {
            tracing::info!("Default UI client bootstrapped successfully");
        }
        Ok(false) => {
            tracing::debug!("Default UI client already exists");
        }
        Err(e) => {
            tracing::warn!(error = %e, "Failed to bootstrap default UI client");
        }
    }

    // Bootstrap admin user if configured
    if let Some(ref admin_config) = cfg.bootstrap.admin_user {
        let user_storage = ArcUserStorage::new(pool.clone());
        match crate::bootstrap::bootstrap_admin_user(&user_storage, admin_config).await {
            Ok(true) => {
                tracing::info!(
                    username = %admin_config.username,
                    "Admin user bootstrapped successfully"
                );
            }
            Ok(false) => {
                tracing::debug!(
                    username = %admin_config.username,
                    "Admin user already exists"
                );
            }
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    username = %admin_config.username,
                    "Failed to bootstrap admin user"
                );
            }
        }
    }

    // Bootstrap admin access policy
    let policy_storage = PostgresPolicyStorageAdapter::new(pool);
    match crate::bootstrap::bootstrap_admin_access_policy(&policy_storage).await {
        Ok(true) => {
            tracing::info!("Admin access policy bootstrapped successfully");
        }
        Ok(false) => {
            tracing::debug!("Admin access policy already exists");
        }
        Err(e) => {
            tracing::warn!(
                error = %e,
                "Failed to bootstrap admin access policy"
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

    // Return PostgresStorage directly - it implements FhirStorage
    Ok((Arc::new(pg_storage), Arc::new(pool)))
}

/// Builds the application router with the given configuration.
pub async fn build_app(
    cfg: &AppConfig,
    config_manager: Arc<octofhir_config::ConfigurationManager>,
) -> Result<Router, anyhow::Error> {
    let body_limit = cfg.server.body_limit_bytes;
    let (storage, db_pool) = create_storage(cfg).await?;

    // Bootstrap database tables and auth resources
    if let Err(e) = bootstrap_conformance_if_postgres(cfg).await {
        tracing::warn!(error = %e, "Failed to bootstrap database tables and auth resources");
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

    // Parse FHIR version early - used by both model provider and GraphQL
    let fhir_version = match cfg.fhir.version.as_str() {
        "R4" | "4.0" | "4.0.1" => FhirVersion::R4,
        "R4B" | "4.3" | "4.3.0" => FhirVersion::R4B,
        "R5" | "5.0" | "5.0.0" => FhirVersion::R5,
        "R6" | "6.0" => FhirVersion::R6,
        _ => FhirVersion::R4, // Default to R4
    };

    // Start GraphQL schema build EARLY in background to improve startup time
    // The schema will be building while other components initialize
    let lazy_schema = if cfg.graphql.enabled {
        let schema_builder_config = SchemaBuilderConfig {
            max_depth: cfg.graphql.max_depth,
            max_complexity: cfg.graphql.max_complexity,
            introspection_enabled: cfg.graphql.introspection,
            subscriptions_enabled: cfg.graphql.subscriptions,
        };

        // Create lazy schema holder - will be populated by background task
        let lazy_schema = Arc::new(LazySchema::new_empty());

        // Start background task to bulk-load schemas and build GraphQL schema
        {
            let lazy_schema_clone = lazy_schema.clone();
            let pool = db_pool.as_ref().clone();
            let fhir_version_str = cfg.fhir.version.clone();
            let search_registry = search_cfg.registry.clone();
            let config = schema_builder_config;
            let fhir_version_enum = fhir_version.clone();

            tokio::spawn(async move {
                let total_start = std::time::Instant::now();
                tracing::info!("Starting early background GraphQL schema build...");

                // Step 1: Bulk load all schemas from database
                let step_start = std::time::Instant::now();
                let store = PostgresPackageStore::new(pool);
                let records = match store
                    .bulk_load_fhirschemas_for_graphql(&fhir_version_str)
                    .await
                {
                    Ok(records) => {
                        tracing::info!(
                            schema_count = records.len(),
                            elapsed_ms = step_start.elapsed().as_millis(),
                            "Step 1/4: Bulk loaded schemas from database"
                        );
                        records
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "Failed to bulk load schemas for GraphQL");
                        return;
                    }
                };

                // Step 2: Deserialize schemas
                let step_start = std::time::Instant::now();
                let mut schemas = Vec::with_capacity(records.len());
                let mut deserialize_errors = 0;
                for record in records {
                    match serde_json::from_value(record.content) {
                        Ok(schema) => schemas.push(schema),
                        Err(e) => {
                            deserialize_errors += 1;
                            tracing::warn!(
                                url = %record.url,
                                error = %e,
                                "Failed to deserialize schema, skipping"
                            );
                        }
                    }
                }
                tracing::info!(
                    schema_count = schemas.len(),
                    errors = deserialize_errors,
                    elapsed_ms = step_start.elapsed().as_millis(),
                    "Step 2/4: Deserialized schemas"
                );

                // Step 3: Create in-memory provider
                let step_start = std::time::Instant::now();
                let memory_provider =
                    Arc::new(InMemoryModelProvider::new(schemas, fhir_version_enum));
                tracing::info!(
                    schema_count = memory_provider.schema_count(),
                    elapsed_ms = step_start.elapsed().as_millis(),
                    "Step 3/4: Created in-memory model provider"
                );

                // Step 4: Build GraphQL schema
                let step_start = std::time::Instant::now();
                let schema_builder =
                    FhirSchemaBuilder::new(search_registry, memory_provider, config);

                match schema_builder.build().await {
                    Ok(schema) => {
                        let build_elapsed = step_start.elapsed();
                        lazy_schema_clone.set_schema(Arc::new(schema)).await;
                        tracing::info!(
                            elapsed_ms = build_elapsed.as_millis(),
                            "Step 4/4: Built GraphQL schema"
                        );
                        tracing::info!(
                            total_elapsed_ms = total_start.elapsed().as_millis(),
                            "GraphQL schema build completed successfully"
                        );
                    }
                    Err(e) => {
                        let error_msg = e.to_string();
                        tracing::error!(
                            error = %error_msg,
                            elapsed_ms = step_start.elapsed().as_millis(),
                            "GraphQL schema build failed"
                        );
                        lazy_schema_clone.set_error(error_msg).await;
                    }
                }

                // memory_provider is dropped here, freeing the bulk-loaded schema memory
                tracing::debug!("Released in-memory model provider (schema data freed)");
            });
        }

        tracing::info!(
            introspection = cfg.graphql.introspection,
            max_depth = cfg.graphql.max_depth,
            max_complexity = cfg.graphql.max_complexity,
            "GraphQL schema build started early in background"
        );

        Some(lazy_schema)
    } else {
        tracing::info!("GraphQL disabled");
        None
    };

    // Create on-demand model provider - schemas loaded from database as needed
    // The database stores pre-converted FHIRSchemas which are loaded on-demand
    // with an LRU cache for frequently accessed schemas.
    let octofhir_provider = Arc::new(OctoFhirModelProvider::new(
        db_pool.as_ref().clone(),
        fhir_version,
        500, // LRU cache size
    ));

    tracing::info!(
        "✓ Model provider initialized with on-demand schema loading (FHIR version: {:?})",
        cfg.fhir.version
    );

    // Use Arc<OctoFhirModelProvider> directly for all components
    let model_provider = octofhir_provider;

    // Create HybridTerminologyProvider for terminology operations
    // This is shared across FhirPath, validation, $expand, and $validate-code
    // Terminology is always enabled - uses local packages first, then remote server with caching
    let terminology_provider: Option<Arc<dyn TerminologyProvider>> =
        match crate::canonical::get_manager() {
            Some(manager) => {
                match HybridTerminologyProvider::new(manager, &cfg.terminology) {
                    Ok(provider) => {
                        tracing::info!(
                            server_url = %cfg.terminology.server_url,
                            cache_ttl = cfg.terminology.cache_ttl_secs,
                            "Terminology service initialized (local packages + remote server with caching)"
                        );
                        Some(Arc::new(provider))
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "Failed to create terminology provider, terminology operations will be limited");
                        None
                    }
                }
            }
            None => {
                tracing::warn!("Canonical manager not available, terminology operations will be limited");
                None
            }
        };

    // Create FHIRPath function registry and engine
    let registry = Arc::new(octofhir_fhirpath::create_function_registry());
    let mut fhirpath_engine =
        FhirPathEngine::new(registry, model_provider.clone())
            .await
            .map_err(|e| anyhow::anyhow!("Failed to initialize FHIRPath engine: {}", e))?;

    // Add terminology provider to FhirPath engine for memberOf, validateVS, etc.
    if let Some(ref provider) = terminology_provider {
        fhirpath_engine = fhirpath_engine.with_terminology_provider(provider.clone());
        tracing::info!("FHIRPath engine configured with terminology provider");
    }

    let fhirpath_engine = Arc::new(fhirpath_engine);
    tracing::info!("FHIRPath engine initialized successfully with schema-aware model provider");

    // Load operation definitions from canonical manager
    let fhir_operations = match crate::operations::load_operations().await {
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
            // Register $run for ViewDefinition (SQL on FHIR)
            registry.register(crate::operations::OperationDefinition {
                code: "run".to_string(),
                url: "http://hl7.org/fhir/uv/sql-on-fhir/OperationDefinition/ViewDefinition-run".to_string(),
                kind: crate::operations::OperationKind::Operation,
                system: false,
                type_level: true,
                instance: true,
                resource: vec!["ViewDefinition".to_string()],
                parameters: vec![],
                affects_state: false,
            });
            tracing::info!("Registered $run for ViewDefinition");
            // Register $sql for ViewDefinition (SQL generation)
            registry.register(crate::operations::OperationDefinition {
                code: "sql".to_string(),
                url: "http://hl7.org/fhir/uv/sql-on-fhir/OperationDefinition/ViewDefinition-sql".to_string(),
                kind: crate::operations::OperationKind::Operation,
                system: false,
                type_level: true,
                instance: true,
                resource: vec!["ViewDefinition".to_string()],
                parameters: vec![],
                affects_state: false,
            });
            tracing::info!("Registered $sql for ViewDefinition");
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
            // Register $run for ViewDefinition (SQL on FHIR)
            registry.register(crate::operations::OperationDefinition {
                code: "run".to_string(),
                url: "http://hl7.org/fhir/uv/sql-on-fhir/OperationDefinition/ViewDefinition-run".to_string(),
                kind: crate::operations::OperationKind::Operation,
                system: false,
                type_level: true,
                instance: true,
                resource: vec!["ViewDefinition".to_string()],
                parameters: vec![],
                affects_state: false,
            });
            // Register $sql for ViewDefinition (SQL generation)
            registry.register(crate::operations::OperationDefinition {
                code: "sql".to_string(),
                url: "http://hl7.org/fhir/uv/sql-on-fhir/OperationDefinition/ViewDefinition-sql".to_string(),
                kind: crate::operations::OperationKind::Operation,
                system: false,
                type_level: true,
                instance: true,
                resource: vec!["ViewDefinition".to_string()],
                parameters: vec![],
                affects_state: false,
            });
            Arc::new(registry)
        }
    };

    // Register core operation handlers
    let operation_handlers: Arc<HashMap<String, DynOperationHandler>> = Arc::new(
        register_core_operations_full(
            fhirpath_engine.clone(),
            model_provider.clone(),
            cfg.bulk_export.clone(),
            cfg.sql_on_fhir.clone(),
        ),
    );
    tracing::info!(
        count = operation_handlers.len(),
        "Registered operation handlers"
    );

    // Initialize ValidationService with shared validator (single instance per server)
    // Prepare optional reference resolver
    let reference_resolver: Option<Arc<dyn octofhir_fhirschema::reference::ReferenceResolver>> =
        if !cfg.validation.skip_reference_validation {
            tracing::info!("Validation service with reference existence validation");
            Some(Arc::new(StorageReferenceResolver::new(
                storage.clone(),
                cfg.base_url(),
            )))
        } else {
            tracing::info!("Validation service (reference validation disabled)");
            None
        };

    // Prepare optional terminology service
    let terminology_service: Option<Arc<dyn octofhir_fhirschema::terminology::TerminologyService>> =
        if let Some(ref provider) = terminology_provider {
            tracing::info!("Validation service with terminology binding validation");
            Some(Arc::new(TerminologyProviderAdapter::new(provider.clone())))
        } else {
            None
        };

    // Create validation service with shared validator
    let validation_service = ValidationService::with_options(
        model_provider.clone(),
        fhirpath_engine.clone(),
        reference_resolver,
        terminology_service,
    );

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
    // Note: Gateway hot-reload listener is started later after operation_registry is initialized

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

    // Start background cleanup task for expired bulk export files
    if cfg.bulk_export.enabled {
        let export_path = cfg.bulk_export.export_path.clone();
        let retention_hours = cfg.bulk_export.retention_hours;
        let _export_cleanup_handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(3600)); // Run every hour
            loop {
                interval.tick().await;
                match crate::operations::cleanup_expired_exports(&export_path, retention_hours)
                    .await
                {
                    Ok(deleted) if deleted > 0 => {
                        tracing::info!(
                            deleted = deleted,
                            retention_hours = retention_hours,
                            "Cleaned up expired bulk export directories"
                        );
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "Failed to clean up expired bulk exports");
                    }
                    _ => {}
                }
            }
        });
        tracing::info!(
            export_path = %cfg.bulk_export.export_path,
            retention_hours = cfg.bulk_export.retention_hours,
            "Bulk export cleanup task started"
        );
    }

    // Initialize policy evaluation components
    let policy_storage = Arc::new(PostgresPolicyStorageAdapter::new(db_pool.clone()));
    let policy_cache = Arc::new(PolicyCache::new(policy_storage, Duration::minutes(5)));

    // Perform initial cache load
    if let Err(e) = policy_cache.refresh().await {
        tracing::warn!(error = %e, "Failed to load initial policies, continuing with empty cache");
    } else {
        let stats = policy_cache.stats().await;
        tracing::info!(
            policy_count = stats.policy_count,
            "Policy cache initialized"
        );
    }

    // Create policy change notifier and reload service
    let policy_notifier = Arc::new(PolicyChangeNotifier::new(64));
    let reload_config = ReloadConfig::default();
    let policy_reload_service = Arc::new(PolicyReloadService::new(
        policy_cache.clone(),
        policy_notifier.clone(),
        reload_config,
    ));

    // Start the reload service
    let reload_service_clone = policy_reload_service.clone();
    tokio::spawn(async move {
        reload_service_clone.run().await;
    });
    tracing::info!("Policy reload service started");

    // Create and start policy listener (PostgreSQL LISTEN/NOTIFY)
    let policy_listener = Arc::new(PolicyListener::new(db_pool.as_ref().clone()));
    let policy_notifier_for_listener = policy_notifier.clone();

    // Wire listener to notifier
    let mut policy_rx = policy_listener.subscribe();
    tokio::spawn(async move {
        use octofhir_auth::policy::PolicyChange;
        use octofhir_auth_postgres::PolicyChangeOp;

        while let Ok(event) = policy_rx.recv().await {
            let change = match event.operation {
                PolicyChangeOp::Insert => PolicyChange::Created {
                    policy_id: event.policy_id,
                },
                PolicyChangeOp::Update => PolicyChange::Updated {
                    policy_id: event.policy_id,
                },
                PolicyChangeOp::Delete => PolicyChange::Deleted {
                    policy_id: event.policy_id,
                },
            };
            policy_notifier_for_listener.notify(change);
        }
    });

    // Start the listener
    let _listener_handle = policy_listener.start();
    tracing::info!("Policy LISTEN/NOTIFY listener started");

    // Create policy evaluator with policies evaluated FIRST
    // This allows explicit Allow policies (like admin policy) to grant access
    // without requiring SMART scopes for non-FHIR endpoints like /api/*
    let policy_evaluator = Arc::new(PolicyEvaluator::new(
        policy_cache.clone(),
        PolicyEvaluatorConfig {
            evaluate_scopes_first: false, // Policies first, then scope check
            quickjs_enabled: cfg.auth.policy.quickjs_enabled,
            quickjs_config: cfg.auth.policy.quickjs.clone().into(),
            ..PolicyEvaluatorConfig::default()
        },
    ));
    tracing::info!("Policy evaluator initialized");

    // Initialize auth state (mandatory)
    let auth_state = initialize_auth_state(&cfg, db_pool.clone())
        .await
        .context("Failed to initialize authentication")?;
    tracing::info!("Authentication initialized");

    // Bootstrap admin user if configured
    if let Some(ref admin_config) = cfg.bootstrap.admin_user {
        let user_storage = octofhir_auth_postgres::ArcUserStorage::new(db_pool.clone());
        match crate::bootstrap::bootstrap_admin_user(&user_storage, admin_config).await {
            Ok(true) => {
                tracing::info!(
                    username = %admin_config.username,
                    "Admin user bootstrapped successfully"
                );
            }
            Ok(false) => {
                tracing::debug!(
                    username = %admin_config.username,
                    "Admin user already exists, skipping bootstrap"
                );
            }
            Err(e) => {
                tracing::error!(
                    error = %e,
                    username = %admin_config.username,
                    "Failed to bootstrap admin user"
                );
            }
        }
    }

    // Bootstrap operations registry (source of truth for public paths)
    let operation_registry = match crate::bootstrap::bootstrap_operations(db_pool.as_ref(), &cfg).await {
        Ok(registry) => {
            tracing::info!("Operations registry bootstrapped with in-memory indexes");
            registry
        }
        Err(e) => {
            tracing::warn!(error = %e, "Failed to bootstrap operations registry, using empty registry");
            // Create an empty registry as fallback
            let op_storage = Arc::new(crate::operation_registry::PostgresOperationStorage::new((*db_pool).clone()));
            Arc::new(OperationRegistryService::new(op_storage))
        }
    };

    // Create GraphQL state if enabled (schema build was started early)
    let graphql_state = if let Some(lazy_schema) = lazy_schema {
        // Create PostgresStorage from the pool for GraphQL
        let graphql_storage: octofhir_storage::DynStorage =
            Arc::new(PostgresStorage::from_pool(db_pool.as_ref().clone()));

        // Create context template with shared dependencies
        let context_template = GraphQLContextTemplate {
            storage: graphql_storage,
            search_config: search_cfg.clone(),
            policy_evaluator: policy_evaluator.clone(),
        };

        let state = GraphQLState {
            lazy_schema,
            context_template,
        };

        // Schema build was already started early, no need to trigger again
        tracing::info!("GraphQL state initialized (schema build started early)");

        Some(state)
    } else {
        None
    };

    // Initialize audit service
    let audit_service = Arc::new(AuditService::new(
        storage.clone(),
        cfg.audit.enabled,
        cfg.audit.clone(),
    ));
    if cfg.audit.enabled {
        tracing::info!(
            log_fhir = cfg.audit.log_fhir_operations,
            log_auth = cfg.audit.log_auth_events,
            log_reads = cfg.audit.log_read_operations,
            log_searches = cfg.audit.log_search_operations,
            "Audit service initialized"
        );
    } else {
        tracing::info!("Audit service disabled");
    }

    // Initialize auth context cache (60 second TTL)
    let auth_cache = crate::cache::create_auth_cache(std::time::Duration::from_secs(60));
    tracing::info!("Auth context cache initialized with 60s TTL");

    // Initialize JWT verification cache (30 second TTL, 10k max entries)
    // Short TTL for security, max size prevents DoS via token flooding
    let jwt_cache = Arc::new(crate::cache::JwtVerificationCache::default_ttl());
    tracing::info!("JWT verification cache initialized with 30s TTL, 10k max entries");

    // Spawn background cache cleanup task (runs every 60 seconds)
    {
        let auth_cache_for_cleanup = auth_cache.clone();
        let jwt_cache_for_cleanup = jwt_cache.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
            loop {
                interval.tick().await;
                let auth_removed = auth_cache_for_cleanup.cleanup_expired();
                let jwt_removed = jwt_cache_for_cleanup.cleanup_expired();
                if auth_removed > 0 || jwt_removed > 0 {
                    tracing::debug!(
                        auth_removed,
                        jwt_removed,
                        "Cache cleanup completed"
                    );
                }
            }
        });
        tracing::info!("Background cache cleanup task started (60s interval)");
    }

    let resource_types = model_provider.get_resource_types().await.unwrap_or_default();
    let resource_type_set = Arc::new(ArcSwap::from_pointee(
        resource_types.iter().cloned().collect::<HashSet<String>>(),
    ));
    tracing::info!(
        resource_types = resource_type_set.load().len(),
        "Resource types loaded for validation"
    );

    // Build CapabilityStatement at startup (cached for /metadata requests)
    let capability_statement = Arc::new(
        handlers::build_capability_statement(
            &cfg.fhir.version,
            &cfg.base_url(),
            &db_pool,
            &resource_types,
        )
        .await,
    );
    tracing::info!("CapabilityStatement built and cached");

    // Start gateway hot-reload listener now that operation_registry is available
    let gateway_reload_pool = db_pool.as_ref().clone();
    let gateway_reload_router = gateway_router.clone();
    let gateway_reload_storage = storage.clone();
    let gateway_reload_ops = operation_registry.clone();
    tokio::spawn(async move {
        use crate::gateway::GatewayReloadBuilder;
        match GatewayReloadBuilder::new()
            .with_pool(gateway_reload_pool)
            .with_gateway_router(gateway_reload_router)
            .with_storage(gateway_reload_storage)
            .with_operation_registry(gateway_reload_ops)
            .start()
        {
            Ok(_handle) => {
                tracing::info!("Gateway hot-reload listener started");
            }
            Err(e) => {
                tracing::warn!(error = %e, "Failed to start gateway hot-reload listener");
            }
        }
    });

    // Create AppState wrapped in Arc for cheap cloning across all middleware/handlers
    // This is a single Arc::clone per request instead of cloning 25+ individual fields
    let state = AppState(Arc::new(AppStateInner {
        storage,
        search_cfg,
        fhir_version: cfg.fhir.version.clone(),
        base_url: cfg.base_url(),
        fhirpath_engine,
        model_provider,
        fhir_operations,
        operation_handlers,
        validation_service,
        gateway_router: (*gateway_router).clone(),
        db_pool,
        handler_registry,
        compartment_registry,
        async_job_manager,
        config: Arc::new(cfg.clone()),
        policy_evaluator,
        policy_cache,
        policy_reload_service,
        auth_state,
        graphql_state,
        operation_registry,
        auth_cache,
        jwt_cache,
        json_schema_cache: Arc::new(dashmap::DashMap::new()),
        resource_type_set,
        capability_statement,
        config_manager: Some(config_manager),
        audit_service,
    }));

    // Configure async job executor to handle bulk export and ViewDefinition export jobs
    let state_for_executor = state.clone();
    let executor: crate::async_jobs::JobExecutor = Arc::new(move |job_id: uuid::Uuid, request_type: String, _method: String, _url: String, body: Option<serde_json::Value>| {
        let state = state_for_executor.clone();
        Box::pin(async move {
            match request_type.as_str() {
                "bulk_export" => {
                    // Extract job parameters from body
                    let body = body.ok_or_else(|| "Missing job parameters".to_string())?;

                    // Execute the bulk export
                    crate::operations::execute_bulk_export(state, job_id, body).await
                },
                "viewdefinition_export" => {
                    // Extract job parameters from body
                    let body = body.ok_or_else(|| "Missing job parameters".to_string())?;

                    // Execute the ViewDefinition export
                    crate::operations::execute_viewdefinition_export(state, job_id, body).await
                },
                _ => Err(format!("Unknown job type: {}", request_type))
            }
        }) as std::pin::Pin<Box<dyn std::future::Future<Output = Result<serde_json::Value, String>> + Send>>
    });

    state.async_job_manager.set_executor(executor);
    tracing::info!("Async job executor configured for bulk export and ViewDefinition export");

    Ok(build_router(state, body_limit))
}

/// Creates routes for internal administrative resources.
///
/// These routes handle FHIR CRUD operations for internal resources like
/// User, Role, Client, AccessPolicy, IdentityProvider, and CustomOperation.
/// They are served at the root level (e.g., /User, /Role) rather than under /fhir.
fn internal_resource_routes() -> Router<AppState> {
    Router::new()
        .route(
            "/{resource_type}",
            get(handlers::internal_search_resource).post(handlers::internal_create_resource),
        )
        .route(
            "/{resource_type}/{id}",
            get(handlers::internal_read_resource)
                .put(handlers::internal_update_resource)
                .delete(handlers::internal_delete_resource),
        )
}

fn build_router(state: AppState, body_limit: usize) -> Router {
    // Build OAuth routes if auth is enabled (these are merged AFTER middleware, as they're public)
    let oauth_routes = build_oauth_routes(&state);

    // NOTE: GraphQL routes are now added directly to the main router BEFORE middleware
    // is applied, so they go through authentication. Previously they were merged after
    // middleware, bypassing auth, which caused "Missing AuthContext extension" errors.

    let mut router = Router::new()
        // Health and info endpoints
        .route("/", get(handlers::root))
        .route("/healthz", get(handlers::healthz))
        .route("/readyz", get(handlers::readyz))
        .route("/metrics", get(handlers::metrics))
        // Browser favicon shortcut
        .route("/favicon.ico", get(handlers::favicon))
        // API endpoints for UI (before gateway fallback)
        .route("/api/health", get(handlers::api_health))
        .route("/api/build-info", get(handlers::api_build_info))
        .route("/api/settings", get(handlers::api_settings))
        .route("/api/resource-types", get(handlers::api_resource_types))
        .route(
            "/api/resource-types-categorized",
            get(handlers::api_resource_types_categorized),
        )
        .route(
            "/api/json-schema/{resource_type}",
            get(handlers::api_json_schema),
        )
        .route(
            "/api/__introspect/rest-console",
            get(crate::rest_console::introspect),
        )
        // Operations registry API
        .route("/api/operations", get(handlers::api_operations))
        .route(
            "/api/operations/{id}",
            get(handlers::api_operation_get).patch(handlers::api_operation_patch),
        )
        // DB Console SQL execution endpoint
        .route(
            "/api/$sql",
            axum::routing::post(crate::operations::sql::sql_operation),
        )
        // LSP WebSocket endpoints (authenticated)
        .route("/api/lsp/pg", get(crate::lsp::pg_lsp_websocket_handler))
        .route("/api/lsp/fhirpath", get(crate::lsp::fhirpath_lsp_websocket_handler))
        // Log stream WebSocket endpoint (authenticated, admin scope required)
        .route("/api/logs/stream", get(crate::log_stream::log_stream_handler))
        // Package Management API
        .route("/api/packages", get(handlers::api_packages_list))
        .route(
            "/api/packages/{name}/{version}",
            get(handlers::api_packages_get),
        )
        .route(
            "/api/packages/{name}/{version}/resources",
            get(handlers::api_packages_resources),
        )
        .route(
            "/api/packages/{name}/{version}/resources/{url}",
            get(handlers::api_packages_resource_content),
        )
        .route(
            "/api/packages/{name}/{version}/fhirschema/{url}",
            get(handlers::api_packages_fhirschema),
        )
        .route(
            "/api/packages/lookup/{name}",
            get(handlers::api_packages_lookup),
        )
        .route("/api/packages/search", get(handlers::api_packages_search))
        .route(
            "/api/packages/install",
            axum::routing::post(handlers::api_packages_install),
        )
        .route(
            "/api/packages/install/stream",
            axum::routing::post(handlers::api_packages_install_stream),
        )
        // Internal resource routes (User, Role, Client, etc.) without /fhir prefix
        // These routes handle FHIR CRUD operations for internal administrative resources
        // Routes are merged at root level to use parameterized handlers
        .merge(internal_resource_routes())
        // Admin routes (nested under /admin)
        .nest(
            "/admin",
            if state.config_manager.is_some() {
                Router::new()
                    .merge(crate::admin::admin_routes())
                    .merge(crate::admin::config_routes())
                    .merge(crate::admin::audit_routes())
            } else {
                Router::new()
                    .merge(crate::admin::admin_routes())
                    .merge(crate::admin::audit_routes())
            },
        )
        // Embedded UI under /ui
        .route("/ui", get(handlers::ui_index))
        .route("/ui/{*path}", get(handlers::ui_static));

    let mut fhir_router = Router::new()
        // Async job status endpoints (FHIR asynchronous request pattern)
        .route(
            "/_async-status/{job_id}",
            get(handlers::async_job_status).delete(handlers::async_job_cancel),
        )
        .route(
            "/_async-status/{job_id}/result",
            get(handlers::async_job_result),
        )
        // Bulk export file serving (Bulk Data Access IG)
        .route(
            "/_bulk-files/{job_id}/{filename}",
            get(handlers::bulk_export_file),
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
        // GET /Patient/123/Observation - resources in compartment (specific routes first)
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
        // Metadata endpoint scoped to /fhir base
        .route("/metadata", get(handlers::metadata))
        // Transaction endpoint at base /fhir
        .route("/", get(handlers::root).post(handlers::transaction_handler))
        // Gateway fallback - only called when no explicit route matches.
        // This allows users to define custom operations on any path.
        .fallback(crate::gateway::router::gateway_fallback_handler);

    // Add GraphQL route conditionally (before middleware so it goes through auth)
    // GraphQL handlers use State<GraphQLState> which is extracted from AppState via FromRef
    if state.graphql_state.is_some() {
        router = router.route(
            "/$graphql",
            get(octofhir_graphql::graphql_handler_get).post(octofhir_graphql::graphql_handler),
        );
        fhir_router = fhir_router.route(
            "/{resource_type}/{id}/$graphql",
            post(octofhir_graphql::instance_graphql_handler),
        );
        tracing::info!(
            "GraphQL endpoints enabled: /$graphql and /fhir/{{resourceType}}/{{id}}/$graphql"
        );
    }

    router = router.nest("/fhir", fhir_router);

    // Apply middleware stack (outer to inner: body limit -> trace -> compression/cors -> content negotiation -> authz -> auth -> request id -> handler)
    // Note: Layers wrap from outside-in, so first .layer() is closest to handler
    // Note: `.with_state(state)` consumes the AppState and returns Router<()>

    // Only apply auth/authz middleware if auth is enabled
    // Apply middleware stack (auth is mandatory)
    router = router
        // Audit middleware runs closest to handler, after auth context is set
        .layer(middleware::from_fn_with_state(
            state.clone(),
            app_middleware::audit_middleware,
        ))
        .layer(middleware::from_fn(app_middleware::request_id))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            app_middleware::authorization_middleware,
        ))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            app_middleware::authentication_middleware,
        ));

    let router: Router = router
        .layer(middleware::from_fn(app_middleware::content_negotiation))
        .layer(CompressionLayer::new())
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(|req: &axum::http::Request<_>| {
                    use tracing::field::Empty;
                    // Skip creating a span for noisy infrastructure endpoints
                    let path = req.uri().path();
                    if matches!(
                        path,
                        "/healthz"
                            | "/readyz"
                            | "/livez"
                            | "/metrics"
                            | "/favicon.ico"
                            | "/api/health"
                    ) {
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
        // Metrics middleware - tracks all requests before any other processing
        .layer(middleware::from_fn(app_middleware::metrics_middleware))
        .with_state(state)
        .layer(axum::extract::DefaultBodyLimit::max(body_limit));

    // Merge OAuth routes if auth is enabled
    // OAuth routes have their own state and are not subject to FHIR content negotiation
    // Note: OAuth routes intentionally bypass auth middleware as they're public
    let router = if let Some(oauth) = oauth_routes {
        router.merge(oauth)
    } else {
        router
    };

    router
}

/// Build OAuth routes.
fn build_oauth_routes(state: &AppState) -> Option<Router> {
    let auth_state = &state.auth_state;

    // Create OAuth state using the JWT service from auth state
    let oauth_state =
        crate::oauth::OAuthState::from_app_state(state, auth_state.jwt_service.clone())?;

    // Build OAuth routes
    let oauth_router = crate::oauth::oauth_routes(oauth_state.clone());
    let jwks_router = crate::oauth::jwks_route(oauth_state.jwks_state.clone());
    let smart_config_router = crate::oauth::smart_config_route(oauth_state.smart_config_state);
    let userinfo_router = crate::oauth::userinfo_route(auth_state.clone());

    tracing::info!(
        "OAuth routes enabled: /auth/token, /auth/logout, /auth/userinfo, /auth/jwks, /.well-known/smart-configuration"
    );

    Some(
        Router::new()
            .merge(oauth_router)
            .merge(jwks_router)
            .merge(smart_config_router)
            .merge(userinfo_router),
    )
}

pub struct ServerBuilder {
    addr: SocketAddr,
    config: AppConfig,
    config_manager: Arc<octofhir_config::ConfigurationManager>,
}

impl ServerBuilder {
    pub fn new(config_manager: Arc<octofhir_config::ConfigurationManager>) -> Self {
        let cfg = AppConfig::default();
        Self {
            addr: cfg.addr(),
            config: cfg,
            config_manager,
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
        let app = build_app(&self.config, self.config_manager).await?;

        Ok(OctofhirServer {
            addr: self.addr,
            app,
        })
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

/// Initializes authentication state.
///
/// Returns an error if auth initialization fails.
async fn initialize_auth_state(
    cfg: &AppConfig,
    db_pool: Arc<sqlx_postgres::PgPool>,
) -> anyhow::Result<AuthState> {
    tracing::info!(
        algorithm = %cfg.auth.signing.algorithm,
        has_private_key = cfg.auth.signing.private_key_pem.is_some(),
        has_public_key = cfg.auth.signing.public_key_pem.is_some(),
        "Initializing authentication module"
    );

    // Parse signing algorithm
    let algorithm = match cfg.auth.signing.algorithm.as_str() {
        "RS256" => SigningAlgorithm::RS256,
        "RS384" => SigningAlgorithm::RS384,
        "ES384" => SigningAlgorithm::ES384,
        other => {
            anyhow::bail!("Unsupported signing algorithm: {}", other);
        }
    };

    // Load or generate signing key pair
    let signing_key = if let (Some(private_pem), Some(public_pem)) = (
        cfg.auth.signing.private_key_pem.as_ref(),
        cfg.auth.signing.public_key_pem.as_ref(),
    ) {
        // Load key from configuration
        let kid = cfg
            .auth
            .signing
            .kid
            .clone()
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

        let key = SigningKeyPair::from_pem(kid.clone(), algorithm, private_pem, public_pem)
            .map_err(|e| anyhow::anyhow!("Failed to load JWT signing key from configuration: {}", e))?;
        tracing::info!(
            algorithm = %algorithm,
            kid = %kid,
            "Loaded JWT signing key from configuration"
        );
        key
    } else {
        // Generate new signing key pair
        let key = match algorithm {
            SigningAlgorithm::RS256 | SigningAlgorithm::RS384 => {
                SigningKeyPair::generate_rsa(algorithm)
                    .map_err(|e| anyhow::anyhow!("Failed to generate RSA signing key: {}", e))?
            }
            SigningAlgorithm::ES384 => {
                SigningKeyPair::generate_ec()
                    .map_err(|e| anyhow::anyhow!("Failed to generate EC signing key: {}", e))?
            }
        };

        tracing::warn!(
            algorithm = %algorithm,
            kid = %key.kid,
            "Generated new JWT signing key - tokens will be invalidated on server restart. \
             Consider setting auth.signing.private_key_pem in configuration for production."
        );

        key
    };

    // Create JWT service
    let jwt_service = Arc::new(JwtService::new(signing_key, cfg.auth.issuer.clone()));

    // Create Arc-owning storage adapters
    let client_storage = Arc::new(ArcClientStorage::new(db_pool.clone()));
    let revoked_token_storage = Arc::new(ArcRevokedTokenStorage::new(db_pool.clone()));
    let user_storage = Arc::new(ArcUserStorage::new(db_pool));

    Ok(
        AuthState::new(
            jwt_service,
            client_storage,
            revoked_token_storage,
            user_storage,
        )
        .with_cookie_config(cfg.auth.cookie.clone()),
    )
}
