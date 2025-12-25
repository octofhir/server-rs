use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

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

use crate::operation_registry::OperationStorage;
use crate::operations::{DynOperationHandler, OperationRegistry, register_core_operations};
use crate::reference_resolver::StorageReferenceResolver;
use crate::validation::ValidationService;

use crate::{config::AppConfig, handlers, middleware as app_middleware};
use octofhir_db_postgres::PostgresConfig;
use octofhir_db_postgres::PostgresStorage;
use octofhir_search::SearchConfig as EngineSearchConfig;
use octofhir_storage::DynStorage;

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
    /// Model provider for validation, FHIRPath, LSP, and all server features
    pub model_provider: Arc<OctoFhirModelProvider>,
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
    /// Application configuration for runtime access
    pub config: Arc<AppConfig>,
    /// Policy evaluator for access control
    pub policy_evaluator: Arc<PolicyEvaluator>,
    /// Policy cache for hot-reload
    pub policy_cache: Arc<PolicyCache>,
    /// Policy reload service for hot-reload
    pub policy_reload_service: Arc<PolicyReloadService>,
    /// Authentication state for token validation
    pub auth_state: Option<AuthState>,
    /// GraphQL state for GraphQL handlers
    pub graphql_state: Option<GraphQLState>,
    /// Cache of public operation paths for authentication middleware
    pub public_paths_cache: crate::middleware::PublicPathsCache,
    /// Cache for JSON Schema conversions (FhirSchema -> JSON Schema)
    pub json_schema_cache: Arc<dashmap::DashMap<String, serde_json::Value>>,
    /// Configuration manager for runtime config and feature flags
    pub config_manager: Option<Arc<octofhir_config::ConfigurationManager>>,
}

// =============================================================================
// FromRef Implementations for Middleware States
// =============================================================================

impl FromRef<AppState> for crate::middleware::ExtendedAuthState {
    fn from_ref(state: &AppState) -> Self {
        let auth_state = state
            .auth_state
            .clone()
            .expect("AuthState not initialized in AppState");
        crate::middleware::ExtendedAuthState::new(auth_state, state.public_paths_cache.clone())
    }
}

impl FromRef<AppState> for AuthState {
    fn from_ref(state: &AppState) -> Self {
        state
            .auth_state
            .clone()
            .expect("AuthState not initialized in AppState")
    }
}

impl FromRef<AppState> for AuthorizationState {
    fn from_ref(state: &AppState) -> Self {
        AuthorizationState::new(state.policy_evaluator.clone())
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
    if cfg.auth.enabled {
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
    let mut validation_service =
        ValidationService::new(model_provider.clone(), fhirpath_engine.clone())
            .await
            .map_err(|e| anyhow::anyhow!("Failed to initialize validation service: {}", e))?;

    // Add reference resolver for existence validation unless disabled in config
    if !cfg.validation.skip_reference_validation {
        let reference_resolver = Arc::new(StorageReferenceResolver::new(
            storage.clone(),
            cfg.base_url(),
        ));
        validation_service = validation_service.with_reference_resolver(reference_resolver);
        tracing::info!("Validation service initialized with reference existence validation");
    } else {
        tracing::info!("Validation service initialized (reference validation disabled)");
    }

    // Create public paths cache for authentication middleware
    // This is created early so it can be shared with the gateway reload listener
    let public_paths_cache = crate::middleware::PublicPathsCache::new();

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

    // Start gateway hot-reload listener with public paths cache
    // so it can update the cache when CustomOperations change
    match crate::gateway::GatewayReloadBuilder::new()
        .with_pool(db_pool.as_ref().clone())
        .with_gateway_router(gateway_router.clone())
        .with_storage(storage.clone())
        .with_public_paths_cache(public_paths_cache.clone())
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

    // Initialize auth state if auth is enabled
    let auth_state = initialize_auth_state(&cfg, db_pool.clone()).await;
    if auth_state.is_some() {
        tracing::info!("Authentication enabled");

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
    } else {
        tracing::warn!("Authentication disabled - no auth configuration found");
    }

    // Bootstrap operations registry
    match crate::bootstrap::bootstrap_operations(db_pool.as_ref(), &cfg).await {
        Ok(count) => {
            tracing::info!(count, "Operations registry bootstrapped");

            // Populate public paths cache from operations registry (static operations)
            let op_storage =
                crate::operation_registry::PostgresOperationStorage::new((*db_pool).clone());
            if let Ok(all_operations) = op_storage.list_all().await {
                public_paths_cache.update_from_operations(&all_operations);
            }

            // Also update cache with gateway CustomOperations
            let gateway_ops = crate::handlers::load_gateway_operations(&storage).await;
            public_paths_cache.update_from_operations(&gateway_ops);

            let (exact, prefix) = public_paths_cache.len();
            tracing::info!(
                exact_paths = exact,
                prefix_paths = prefix,
                "Public paths cache populated (static + gateway operations)"
            );
        }
        Err(e) => {
            tracing::warn!(error = %e, "Failed to bootstrap operations registry");
        }
    }

    // Create GraphQL state if enabled (schema build was started early)
    let graphql_state = if let Some(lazy_schema) = lazy_schema {
        // Create PostgresStorage from the pool for GraphQL
        // GraphQL uses the new FhirStorage trait, not the legacy Storage trait
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
        config: Arc::new(cfg.clone()),
        policy_evaluator,
        policy_cache,
        policy_reload_service,
        auth_state,
        graphql_state,
        public_paths_cache,
        json_schema_cache: Arc::new(dashmap::DashMap::new()),
        config_manager: Some(config_manager),
    };

    Ok(build_router(state, body_limit))
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
        .route("/metadata", get(handlers::metadata))
        // Browser favicon shortcut
        .route("/favicon.ico", get(handlers::favicon))
        // API endpoints for UI (before gateway fallback)
        .route("/api/health", get(handlers::api_health))
        .route("/api/build-info", get(handlers::api_build_info))
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
        // PostgreSQL LSP WebSocket endpoint (authenticated)
        .route("/api/pg-lsp", get(crate::lsp::lsp_websocket_handler))
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
        // Admin routes (nested under /admin)
        .nest(
            "/admin",
            if state.auth_state.is_some() && state.config_manager.is_some() {
                Router::new()
                    .merge(crate::admin::admin_routes())
                    .merge(crate::admin::config_routes())
            } else if state.auth_state.is_some() {
                crate::admin::admin_routes()
            } else {
                Router::new()
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
    router = if state.auth_state.is_some() {
        router
            .layer(middleware::from_fn(app_middleware::request_id))
            .layer(middleware::from_fn_with_state(
                state.clone(),
                app_middleware::authorization_middleware,
            ))
            .layer(middleware::from_fn_with_state(
                state.clone(),
                app_middleware::authentication_middleware,
            ))
    } else {
        router.layer(middleware::from_fn(app_middleware::request_id))
    };

    let router: Router = router
        .layer(middleware::from_fn(app_middleware::content_negotiation))
        .layer(CompressionLayer::new())
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(|req: &axum::http::Request<_>| {
                    use tracing::field::Empty;
                    // Skip creating a span for browser favicon and health check requests to avoid noisy logs
                    let path = req.uri().path();
                    if path == "/favicon.ico" || path == "/api/health" || path == "/healthz" {
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

/// Build OAuth routes if auth is enabled.
fn build_oauth_routes(state: &AppState) -> Option<Router> {
    let auth_state = state.auth_state.as_ref()?;

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

/// Returns `Some(AuthState)` if auth is enabled and initialization succeeds,
/// or `None` if auth is disabled.
async fn initialize_auth_state(
    cfg: &AppConfig,
    db_pool: Arc<sqlx_postgres::PgPool>,
) -> Option<AuthState> {
    // Check if auth is enabled
    if !cfg.auth.enabled {
        tracing::info!("Authentication is disabled in configuration");
        return None;
    }

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
            tracing::error!(algorithm = other, "Unsupported signing algorithm");
            return None;
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

        match SigningKeyPair::from_pem(kid.clone(), algorithm, private_pem, public_pem) {
            Ok(key) => {
                tracing::info!(
                    algorithm = %algorithm,
                    kid = %kid,
                    "Loaded JWT signing key from configuration"
                );
                key
            }
            Err(e) => {
                tracing::error!(
                    error = %e,
                    "Failed to load JWT signing key from configuration"
                );
                return None;
            }
        }
    } else {
        // Generate new signing key pair
        let key = match algorithm {
            SigningAlgorithm::RS256 | SigningAlgorithm::RS384 => {
                match SigningKeyPair::generate_rsa(algorithm) {
                    Ok(key) => key,
                    Err(e) => {
                        tracing::error!(error = %e, "Failed to generate RSA signing key");
                        return None;
                    }
                }
            }
            SigningAlgorithm::ES384 => match SigningKeyPair::generate_ec() {
                Ok(key) => key,
                Err(e) => {
                    tracing::error!(error = %e, "Failed to generate EC signing key");
                    return None;
                }
            },
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

    Some(
        AuthState::new(
            jwt_service,
            client_storage,
            revoked_token_storage,
            user_storage,
        )
        .with_cookie_config(cfg.auth.cookie.clone()),
    )
}
