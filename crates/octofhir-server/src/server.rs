use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::{Router, extract::FromRef, middleware, routing::get};
use octofhir_auth::config::CookieConfig;
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
use octofhir_fhirschema::embedded::{FhirVersion as SchemaFhirVersion, get_schemas};
use octofhir_fhirschema::model_provider::FhirSchemaModelProvider;
use octofhir_fhirschema::types::StructureDefinition;
use time::Duration;
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
}

// =============================================================================
// FromRef Implementations for Middleware States
// =============================================================================

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
/// 4. Bootstraps auth resources (admin user, default UI client)
/// 5. Starts hot-reload listener
async fn bootstrap_conformance_if_postgres(cfg: &AppConfig) -> Result<(), anyhow::Error> {
    use octofhir_auth_postgres::{ArcClientStorage, ArcUserStorage};
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
    tracing::debug!("Attempting to load StructureDefinitions from canonical manager");
    if let Some(manager) = crate::canonical::get_manager() {
        tracing::debug!("Canonical manager is available, querying for StructureDefinitions");
        // Query for ALL StructureDefinitions in loaded packages
        // IMPORTANT: Set high limit to get all SDs (FHIR has ~140 resources + ~200 types)
        let query = SearchQuery {
            resource_types: vec!["StructureDefinition".to_string()],
            limit: Some(10000), // High limit to ensure we get ALL StructureDefinitions
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
                } else {
                    tracing::warn!("Query returned 0 StructureDefinitions from canonical manager");
                }
            }
            Err(e) => {
                tracing::warn!(
                    "Failed to query StructureDefinitions from canonical manager: {}",
                    e
                );
            }
        }
    } else {
        tracing::debug!("Canonical manager not available");
    }

    tracing::debug!("Returning empty StructureDefinition list");
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

    let octofhir_provider = if has_packages_configured {
        // Load StructureDefinitions from canonical manager packages
        let structure_definitions = load_structure_definitions_from_packages().await;

        if !structure_definitions.is_empty() {
            // Convert StructureDefinitions to FhirSchemas
            tracing::info!(
                "Converting {} StructureDefinitions to FhirSchema format",
                structure_definitions.len()
            );

            let mut schemas = std::collections::HashMap::new();
            for sd in structure_definitions {
                match octofhir_fhirschema::translate(sd.clone(), None) {
                    Ok(schema) => {
                        schemas.insert(schema.name.clone(), schema);
                    }
                    Err(e) => {
                        tracing::warn!(
                            "Failed to convert StructureDefinition '{}' to FhirSchema: {}",
                            &sd.url,
                            e
                        );
                    }
                }
            }

            let schema_count = schemas.len();
            tracing::info!(
                "Converted to {} FhirSchemas (FHIR version: {:?})",
                schema_count,
                cfg.fhir.version
            );

            if schema_count == 0 {
                return Err(anyhow::anyhow!(
                    "Failed to convert StructureDefinitions to FhirSchemas - conversion resulted in 0 schemas. Cannot start server without schemas."
                ));
            }

            tracing::info!(
                "✓ Model provider initialized successfully with {} schemas from packages",
                schema_count
            );
            let provider = FhirSchemaModelProvider::new(schemas, fhir_version);
            let octofhir_provider = Arc::new(OctoFhirModelProvider::new(provider));
            octofhir_provider.clone()
        } else {
            // Packages configured but failed to load - fallback to embedded
            tracing::warn!(
                "Packages configured but no StructureDefinitions loaded, falling back to embedded schemas"
            );
            let schemas = get_schemas(schema_version).clone();
            let schema_count = schemas.len();

            if schema_count == 0 {
                return Err(anyhow::anyhow!(
                    "Embedded schemas are empty for version {:?}. Cannot start server without schemas.",
                    schema_version
                ));
            }

            let provider = FhirSchemaModelProvider::new(schemas, fhir_version);
            tracing::info!(
                "✓ Model provider initialized with {} embedded schemas (fallback, FHIR version: {:?})",
                schema_count,
                cfg.fhir.version
            );
            let octofhir_provider = Arc::new(OctoFhirModelProvider::new(provider));
            octofhir_provider.clone()
        }
    } else {
        // No packages configured - use embedded schemas for the configured FHIR version
        tracing::info!("No packages configured, using embedded schemas");
        let schemas = get_schemas(schema_version).clone();
        let schema_count = schemas.len();

        if schema_count == 0 {
            return Err(anyhow::anyhow!(
                "Embedded schemas are empty for version {:?}. Cannot start server without schemas.",
                schema_version
            ));
        }

        let provider = FhirSchemaModelProvider::new(schemas, fhir_version);
        tracing::info!(
            "✓ Model provider initialized with {} embedded schemas (FHIR version: {:?})",
            schema_count,
            cfg.fhir.version
        );
        let octofhir_provider = Arc::new(OctoFhirModelProvider::new(provider));
        octofhir_provider.clone()
    };

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
    };

    Ok(build_router(state, body_limit))
}

fn build_router(state: AppState, body_limit: usize) -> Router {
    // Build OAuth routes if auth is enabled
    let oauth_routes = build_oauth_routes(&state);

    let mut router = Router::new()
        // Health and info endpoints
        .route("/", get(handlers::root).post(handlers::transaction_handler))
        .route("/healthz", get(handlers::healthz))
        .route("/readyz", get(handlers::readyz))
        .route("/metadata", get(handlers::metadata))
        // Browser favicon shortcut
        .route("/favicon.ico", get(handlers::favicon))
        // API endpoints for UI (before gateway fallback)
        .route("/api/health", get(handlers::api_health))
        .route("/api/build-info", get(handlers::api_build_info))
        .route("/api/resource-types", get(handlers::api_resource_types))
        // DB Console SQL execution endpoint
        .route(
            "/api/$sql",
            axum::routing::post(crate::operations::sql::sql_operation),
        )
        // PostgreSQL LSP WebSocket endpoint (authenticated)
        .route("/api/pg-lsp", get(crate::lsp::lsp_websocket_handler))
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
        // Gateway fallback - only called when no explicit route matches.
        // This allows users to define custom operations on any path.
        .fallback(crate::gateway::router::gateway_fallback_handler)
        // Middleware stack (outer to inner: body limit -> trace -> compression/cors -> content negotiation -> authz -> auth -> request id -> handler)
        // Note: Layers wrap from outside-in, so first .layer() is closest to handler
        .layer(middleware::from_fn(app_middleware::request_id))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            app_middleware::authorization_middleware,
        ))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            app_middleware::authentication_middleware,
        ))
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
    if let Some(oauth) = oauth_routes {
        router = router.merge(oauth);
    }

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

/// Initialize the authentication state if auth is enabled.
///
/// Build CORS layer with appropriate settings for cookie-based auth.
///
/// When cookies are enabled:
/// - Allow credentials
/// - Mirror the Origin header (for development compatibility)
/// - Allow common HTTP methods
///
/// When cookies are disabled:
/// - Use permissive CORS (any origin, any method)
fn build_cors_layer(cookie_config: &CookieConfig) -> CorsLayer {
    use axum::http::{Method, header};

    if cookie_config.enabled {
        // When cookie auth is enabled, we need to:
        // 1. Allow credentials
        // 2. Use specific origin (not wildcard) - mirror the Origin header for development
        CorsLayer::new()
            .allow_credentials(true)
            .allow_origin(tower_http::cors::AllowOrigin::mirror_request())
            .allow_methods([
                Method::GET,
                Method::POST,
                Method::PUT,
                Method::PATCH,
                Method::DELETE,
                Method::OPTIONS,
            ])
            .allow_headers([
                header::AUTHORIZATION,
                header::CONTENT_TYPE,
                header::ACCEPT,
                header::ORIGIN,
                header::COOKIE,
            ])
            .expose_headers([header::SET_COOKIE])
    } else {
        // Permissive CORS when cookie auth is disabled
        CorsLayer::permissive()
    }
}

/// Returns `Some(AuthState)` if auth is enabled and initialization succeeds,
/// or `None` if auth is disabled.
async fn initialize_auth_state(
    cfg: &AppConfig,
    db_pool: Arc<sqlx_postgres::PgPool>,
) -> Option<AuthState> {
    // Check if auth is enabled
    if !cfg.auth.enabled {
        return None;
    }

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
