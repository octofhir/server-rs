use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Context;
use arc_swap::ArcSwap;
use axum::{
    Router,
    extract::{FromRef, Path, State, WebSocketUpgrade},
    middleware,
    response::IntoResponse,
    routing::{get, post},
};
use octofhir_auth::extractors::BasicAuthState;
use octofhir_auth::middleware::AuthState;
use octofhir_auth::policy::{
    PolicyCache, PolicyChangeNotifier, PolicyEvaluator, PolicyEvaluatorConfig, PolicyReloadService,
    ReloadConfig,
};
use octofhir_auth::storage::BasicAuthStorage;
use octofhir_auth::token::jwt::{JwtService, SigningAlgorithm, SigningKeyPair};
use octofhir_auth_postgres::{
    ArcBasicAuthStorage, ArcClientStorage, ArcRevokedTokenStorage, ArcUserStorage,
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
use tower_http::compression::CompressionLayer;

use crate::events::{RedisEventSyncBuilder, RedisPublishHook};
use crate::hooks::{
    AsyncAuditHook, GatewayReloadHook, GraphQLSubscriptionHook, HookRegistry, PolicyReloadHook,
    SearchParamHook,
};
use crate::operation_registry::OperationRegistryService;
use crate::operations::{DynOperationHandler, OperationRegistry, register_core_operations_all};
use crate::reference_resolver::StorageReferenceResolver;
use crate::subscriptions::{SubscriptionHook, SubscriptionState};
use crate::validation::ValidationService;

use crate::audit::AuditService;
use crate::cache::AuthContextCache;
use crate::{config::AppConfig, handlers, middleware as app_middleware};
use octofhir_core::events::EventBroadcaster;
use octofhir_db_postgres::PostgresConfig;
use octofhir_db_postgres::PostgresStorage;
use octofhir_fhir_model::terminology::TerminologyProvider;
use octofhir_fhirschema::TerminologyProviderAdapter;
use octofhir_search::{HybridTerminologyProvider, ReloadableSearchConfig};
use octofhir_storage::{DynStorage, EventedStorage};

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
    pub search_config: ReloadableSearchConfig,
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
    /// PostgreSQL connection pool for SQL handler (primary / write)
    pub db_pool: Arc<sqlx_postgres::PgPool>,
    /// Read replica pool (falls back to db_pool if no replica configured)
    pub read_db_pool: Arc<sqlx_postgres::PgPool>,
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
    /// CQL evaluation service (optional, feature-gated)
    pub cql_service: Option<Arc<octofhir_cql_service::CqlService>>,
    /// Operation registry (source of truth for public paths, middleware lookups)
    pub operation_registry: Arc<OperationRegistryService>,
    /// Cache for authenticated contexts (reduces DB queries per request)
    pub auth_cache: Arc<dyn AuthContextCache>,
    /// Cache for JWT verification (reduces signature verification overhead)
    pub jwt_cache: Arc<crate::cache::JwtVerificationCache>,
    /// Cache for JSON Schema conversions (FhirSchema -> JSON Schema)
    pub json_schema_cache: Arc<dashmap::DashMap<String, serde_json::Value>>,
    /// Cache for FHIR resource reads (reduces DB queries for read-heavy workloads)
    pub resource_cache: Option<Arc<crate::cache::ResourceCache>>,
    /// Query cache for search SQL template reuse
    pub query_cache: Option<Arc<octofhir_search::QueryCache>>,
    /// Cached resource types for fast validation
    pub resource_type_set: Arc<ArcSwap<HashSet<String>>>,
    /// Cached CapabilityStatement (built at startup)
    pub capability_statement: Arc<serde_json::Value>,
    /// Configuration manager for runtime config and feature flags
    pub config_manager: Option<Arc<octofhir_config::ConfigurationManager>>,
    /// Audit service for creating FHIR AuditEvent resources
    pub audit_service: Arc<AuditService>,
    /// Basic authentication storage (Client and App authentication via HTTP Basic Auth)
    pub basic_auth_storage: Arc<dyn BasicAuthStorage>,
    /// Notification queue storage (optional, for notification operations)
    pub notification_queue: Option<Arc<dyn octofhir_notifications::NotificationQueueStorage>>,
    /// PostgreSQL package store for FHIR Implementation Guide resources
    pub package_store: Arc<octofhir_db_postgres::PostgresPackageStore>,
    /// Subscription state for FHIR R5 subscriptions
    pub subscription_state: SubscriptionState,
    /// Terminology provider for $expand, $validate-code, $subsumes, $translate, $lookup
    /// This is the HybridTerminologyProvider with local + cached remote support
    pub terminology_provider: Option<Arc<dyn TerminologyProvider>>,
    // pub automation_state: Option<crate::automations::AutomationState>,
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

impl FromRef<AppState> for crate::middleware::CombinedAuthState {
    fn from_ref(state: &AppState) -> Self {
        crate::middleware::CombinedAuthState {
            auth_state: state.auth_state.clone(),
            operation_registry: state.operation_registry.clone(),
            auth_cache: state.auth_cache.clone(),
            jwt_cache: state.jwt_cache.clone(),
            policy_evaluator: state.policy_evaluator.clone(),
        }
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

impl FromRef<AppState> for BasicAuthState {
    fn from_ref(state: &AppState) -> Self {
        BasicAuthState::new(state.basic_auth_storage.clone())
    }
}

/*
impl FromRef<AppState> for crate::automations::AutomationState {
    fn from_ref(state: &AppState) -> Self {
        state
            .automation_state
            .clone()
            .expect("AutomationState not initialized in AppState")
    }
}
*/

pub struct OctofhirServer {
    addr: SocketAddr,
    app: Router,
}

/// Handler for WebSocket subscription events endpoint.
///
/// Upgrades HTTP connection to WebSocket for real-time subscription notifications.
/// Endpoint: `/fhir/Subscription/{id}/$events`
async fn subscription_events_ws_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    use crate::subscriptions::types::SubscriptionChannel;

    use axum::http::StatusCode;

    // Get the subscription
    let subscription = match state
        .subscription_state
        .subscription_manager
        .get_subscription(&id)
        .await
    {
        Ok(Some(sub)) => sub,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                format!("Subscription/{} not found", id),
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!(error = %e, subscription_id = %id, "Failed to get subscription");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to get subscription",
            )
                .into_response();
        }
    };

    // Verify subscription uses WebSocket channel
    if !matches!(subscription.channel, SubscriptionChannel::WebSocket { .. }) {
        return (
            StatusCode::BAD_REQUEST,
            "Subscription does not use WebSocket channel",
        )
            .into_response();
    }

    // Verify subscription is active
    if subscription.status != crate::subscriptions::types::SubscriptionStatus::Active {
        return (
            StatusCode::BAD_REQUEST,
            format!(
                "Subscription is not active (status: {:?})",
                subscription.status
            ),
        )
            .into_response();
    }

    let registry = state.subscription_state.websocket_registry.clone();

    // Upgrade to WebSocket
    ws.on_upgrade(move |socket| async move {
        crate::subscriptions::handle_subscription_websocket(socket, subscription, registry).await;
    })
    .into_response()
}

/// Bootstraps auth resources and database tables.
///
/// This function:
/// 1. Creates database tables for all FHIR resource types from FCM packages
/// 2. Bootstraps auth resources (admin user, default UI client, access policy)
async fn bootstrap_conformance_if_postgres(
    cfg: &AppConfig,
    pool: Arc<sqlx_postgres::PgPool>,
) -> Result<(), anyhow::Error> {
    use octofhir_auth_postgres::{ArcClientStorage, ArcUserStorage};

    // Create database tables for all FHIR resource types from FCM packages
    // This includes all resource-kind and logical-kind StructureDefinitions
    let fcm_storage = octofhir_db_postgres::PostgresPackageStore::new((*pool).clone());
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

    // Bootstrap backend service client if configured
    if let Some(ref backend_config) = cfg.bootstrap.backend_client {
        let client_storage = ArcClientStorage::new(pool.clone());
        match crate::bootstrap::bootstrap_backend_client(&client_storage, backend_config).await {
            Ok((true, Some(plaintext_secret))) => {
                // Auto-generated secret - log it prominently
                tracing::warn!(
                    client_id = %backend_config.client_id,
                    "================================================"
                );
                tracing::warn!(
                    client_id = %backend_config.client_id,
                    "Backend service client created with AUTO-GENERATED secret"
                );
                tracing::warn!(
                    client_id = %backend_config.client_id,
                    "Client ID: {}",
                    backend_config.client_id
                );
                tracing::warn!(
                    client_id = %backend_config.client_id,
                    "Client Secret: {}",
                    plaintext_secret
                );
                tracing::warn!(
                    client_id = %backend_config.client_id,
                    "SAVE THIS SECRET - IT WILL NOT BE SHOWN AGAIN"
                );
                tracing::warn!(
                    client_id = %backend_config.client_id,
                    "================================================"
                );
            }
            Ok((true, None)) => {
                tracing::info!(
                    client_id = %backend_config.client_id,
                    "Backend service client bootstrapped with configured secret"
                );
            }
            Ok((false, _)) => {
                tracing::debug!(
                    client_id = %backend_config.client_id,
                    "Backend service client already exists"
                );
            }
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    client_id = %backend_config.client_id,
                    "Failed to bootstrap backend service client"
                );
            }
        }

        // Bootstrap backend access policy for the backend client
        let policy_storage = PostgresPolicyStorageAdapter::new(pool.clone());
        match crate::bootstrap::bootstrap_backend_access_policy(
            &policy_storage,
            &backend_config.client_id,
        )
        .await
        {
            Ok(true) => {
                tracing::info!(
                    client_id = %backend_config.client_id,
                    "Backend access policy bootstrapped successfully"
                );
            }
            Ok(false) => {
                tracing::debug!(
                    client_id = %backend_config.client_id,
                    "Backend access policy already exists"
                );
            }
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    client_id = %backend_config.client_id,
                    "Failed to bootstrap backend access policy"
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

/// Creates Redis connection pool for event synchronization.
///
/// Returns a deadpool_redis pool configured for pub/sub operations.
async fn create_redis_pool(
    config: &crate::config::RedisConfig,
) -> Result<deadpool_redis::Pool, anyhow::Error> {
    use std::time::Duration as StdDuration;

    let mut redis_config = deadpool_redis::Config::from_url(&config.url);
    if let Some(ref mut pool_config) = redis_config.pool {
        pool_config.max_size = config.pool_size;
        pool_config.timeouts.wait = Some(StdDuration::from_millis(config.timeout_ms));
        pool_config.timeouts.create = Some(StdDuration::from_millis(config.timeout_ms));
        pool_config.timeouts.recycle = Some(StdDuration::from_millis(config.timeout_ms));
    }

    let pool = redis_config
        .create_pool(Some(deadpool_redis::Runtime::Tokio1))
        .map_err(|e| anyhow::anyhow!("Failed to create Redis pool: {e}"))?;

    // Test connection
    let _conn = pool
        .get()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to connect to Redis: {e}"))?;

    tracing::info!(url = %config.url, "Redis pool created for event sync");
    Ok(pool)
}

/// Creates PostgreSQL storage.
///
/// Returns (PostgresStorage, primary pool, read pool).
/// The caller is responsible for wrapping with EventedStorage and Arc if needed.
async fn create_storage(
    cfg: &AppConfig,
) -> Result<
    (
        PostgresStorage,
        Arc<sqlx_postgres::PgPool>,
        Arc<sqlx_postgres::PgPool>,
    ),
    anyhow::Error,
> {
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

    let mut pg_storage = PostgresStorage::new(postgres_config)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to create PostgreSQL storage: {e}"))?;

    let primary_pool = Arc::new(pg_storage.pool().clone());

    // Create read replica pool if configured
    let read_pool = if let Some(ref replica_cfg) = pg_cfg.read_replica {
        tracing::info!(url = %octofhir_db_postgres::pool::mask_password(&replica_cfg.url), "Creating read replica pool");
        let replica_config = PostgresConfig::new(&replica_cfg.url)
            .with_pool_size(replica_cfg.pool_size.unwrap_or(pg_cfg.pool_size))
            .with_connect_timeout_ms(
                replica_cfg
                    .connect_timeout_ms
                    .unwrap_or(pg_cfg.connect_timeout_ms),
            )
            .with_idle_timeout_ms(replica_cfg.idle_timeout_ms.or(pg_cfg.idle_timeout_ms))
            .with_run_migrations(false);

        let replica_pool = octofhir_db_postgres::pool::create_pool(&replica_config)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to create read replica pool: {e}"))?;

        pg_storage.set_read_pool(replica_pool.clone());
        Arc::new(replica_pool)
    } else {
        primary_pool.clone()
    };

    Ok((pg_storage, primary_pool, read_pool))
}

/// Builds the application router with the given configuration.
pub async fn build_app(
    cfg: &AppConfig,
    config_manager: Arc<octofhir_config::ConfigurationManager>,
) -> Result<Router, anyhow::Error> {
    let body_limit = cfg.server.body_limit_bytes;
    let (pg_storage, db_pool, read_db_pool) = create_storage(cfg).await?;

    // Create event broadcaster for unified event system
    let event_broadcaster = EventBroadcaster::new_shared();

    // Save the search registry slot for late initialization (after parallel init)
    let search_registry_slot = pg_storage.search_registry_slot().clone();

    // Wrap storage with EventedStorage to emit events on CRUD operations
    let evented_storage = EventedStorage::new(pg_storage, event_broadcaster.clone());
    let storage: DynStorage = Arc::new(evented_storage);
    tracing::info!("Event broadcaster initialized, storage wrapped with EventedStorage");

    // Create PostgreSQL package store for FHIR package management
    let package_store = Arc::new(octofhir_db_postgres::PostgresPackageStore::new(
        (*db_pool).clone(),
    ));

    // Parse FHIR version early - used by both model provider and GraphQL
    let fhir_version = match cfg.fhir.version.as_str() {
        "R4" | "4.0" | "4.0.1" => FhirVersion::R4,
        "R4B" | "4.3" | "4.3.0" => FhirVersion::R4B,
        "R5" | "5.0" | "5.0.0" => FhirVersion::R5,
        "R6" | "6.0" => FhirVersion::R6,
        _ => FhirVersion::R4, // Default to R4
    };

    // ── Phase 1: Create lightweight components (instant, no I/O) ──

    // On-demand model provider with LRU cache (schemas loaded lazily from DB)
    let octofhir_provider = Arc::new(OctoFhirModelProvider::new(
        db_pool.as_ref().clone(),
        fhir_version.clone(),
        500, // LRU cache size
    ));
    let model_provider = octofhir_provider;
    tracing::info!(
        "Model provider initialized with on-demand schema loading (FHIR version: {:?})",
        cfg.fhir.version
    );

    // HybridTerminologyProvider for terminology operations
    let terminology_provider: Option<Arc<dyn TerminologyProvider>> =
        match crate::canonical::get_manager() {
            Some(manager) => match HybridTerminologyProvider::new(manager, &cfg.terminology) {
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
            },
            None => {
                tracing::warn!(
                    "Canonical manager not available, terminology operations will be limited"
                );
                None
            }
        };

    // FHIRPath engine (needs model_provider, fast init)
    let registry = Arc::new(octofhir_fhirpath::create_function_registry());
    let mut fhirpath_engine = FhirPathEngine::new(registry, model_provider.clone())
        .await
        .map_err(|e| anyhow::anyhow!("Failed to initialize FHIRPath engine: {}", e))?;
    if let Some(ref provider) = terminology_provider {
        fhirpath_engine = fhirpath_engine.with_terminology_provider(provider.clone());
        tracing::info!("FHIRPath engine configured with terminology provider");
    }
    let fhirpath_engine = Arc::new(fhirpath_engine);
    tracing::info!("FHIRPath engine initialized successfully with schema-aware model provider");

    // Policy storage and cache (creation is instant, refresh is async — done in Phase 2)
    let policy_storage = Arc::new(PostgresPolicyStorageAdapter::new(db_pool.clone()));
    let policy_cache = Arc::new(PolicyCache::new(policy_storage, Duration::minutes(5)));

    // Async job manager (instant)
    let async_job_config = crate::async_jobs::AsyncJobConfig::default();
    let async_job_manager = Arc::new(crate::async_jobs::AsyncJobManager::new(
        db_pool.clone(),
        async_job_config,
    ));
    tracing::info!("Async job manager initialized");

    // Gateway router (creation is instant, route loading is async — done in Phase 2)
    let gateway_router = Arc::new(crate::gateway::GatewayRouter::new());

    // ── Phase 2: Parallel heavy I/O operations ──
    // These are all independent: they need only db_pool, cfg, or canonical_manager

    use octofhir_search::SearchOptions;
    let search_options = SearchOptions {
        default_count: cfg.search.default_count,
        max_count: cfg.search.max_count,
        cache_capacity: cfg.search.cache_capacity,
    };

    let canonical_manager = crate::canonical::get_manager()
        .ok_or_else(|| anyhow::anyhow!("Canonical manager required for initialization"))?;

    // Launch all heavy init operations concurrently
    let parallel_start = std::time::Instant::now();
    tracing::info!("Starting parallel initialization of 8 components...");
    let bootstrap_fut = bootstrap_conformance_if_postgres(cfg, db_pool.clone());
    let search_config_fut = ReloadableSearchConfig::new(
        &canonical_manager,
        search_options,
        Some(model_provider.as_ref()),
    );
    let auth_state_fut = initialize_auth_state(cfg, db_pool.clone());
    let policy_refresh_fut = policy_cache.refresh();
    let operations_fut = crate::operations::load_operations();
    let compartment_fut =
        crate::compartments::CompartmentRegistry::from_canonical_manager(&canonical_manager);
    let gateway_routes_fut = gateway_router.reload_routes(&storage);
    let resource_types_fut = model_provider.get_resource_types();

    let (
        bootstrap_result,
        search_config_result,
        auth_state_result,
        policy_refresh_result,
        operations_result,
        compartment_result,
        gateway_routes_result,
        resource_types_result,
    ) = tokio::join!(
        bootstrap_fut,
        search_config_fut,
        auth_state_fut,
        policy_refresh_fut,
        operations_fut,
        compartment_fut,
        gateway_routes_fut,
        resource_types_fut,
    );

    tracing::info!(
        elapsed_ms = parallel_start.elapsed().as_millis(),
        "Parallel initialization completed"
    );

    // Process results from parallel operations
    if let Err(e) = bootstrap_result {
        tracing::warn!(error = %e, "Failed to bootstrap database tables and auth resources");
    }

    let search_config = search_config_result
        .map_err(|e| anyhow::anyhow!("Failed to create reloadable search config: {}", e))?;

    // Late-initialize the search registry on the storage so CRUD operations
    // can write search indexes (references, dates, strings).
    let cfg_snapshot = search_config.config();
    if search_registry_slot
        .set(cfg_snapshot.registry.clone())
        .is_err()
    {
        tracing::warn!("Search registry slot was already set (should not happen)");
    } else {
        tracing::info!("Search registry initialized on storage for index writing");
    }

    let auth_state = auth_state_result.context("Failed to initialize authentication")?;
    tracing::info!("Authentication initialized");

    if let Err(e) = policy_refresh_result {
        tracing::warn!(error = %e, "Failed to load initial policies, continuing with empty cache");
    } else {
        let stats = policy_cache.stats().await;
        tracing::info!(
            policy_count = stats.policy_count,
            "Policy cache initialized"
        );
    }

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
            let search_registry = search_config.config().registry.clone();
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

    // ── Phase 3: Process parallel results and build dependent components ──

    // Process operation definitions from parallel Phase 2
    let fhir_operations = match operations_result {
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
                url: "http://hl7.org/fhir/uv/sql-on-fhir/OperationDefinition/ViewDefinition-run"
                    .to_string(),
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
                url: "http://hl7.org/fhir/uv/sql-on-fhir/OperationDefinition/ViewDefinition-sql"
                    .to_string(),
                kind: crate::operations::OperationKind::Operation,
                system: false,
                type_level: true,
                instance: true,
                resource: vec!["ViewDefinition".to_string()],
                parameters: vec![],
                affects_state: false,
            });
            tracing::info!("Registered $sql for ViewDefinition");
            // Register $status for Subscription (R5 subscription status)
            registry.register(crate::operations::OperationDefinition {
                code: "status".to_string(),
                url: "http://hl7.org/fhir/OperationDefinition/Subscription-status".to_string(),
                kind: crate::operations::OperationKind::Operation,
                system: false,
                type_level: false,
                instance: true,
                resource: vec!["Subscription".to_string()],
                parameters: vec![],
                affects_state: false,
            });
            tracing::info!("Registered $status for Subscription");
            // Register $fhirpath operation
            registry.register(crate::operations::OperationDefinition {
                code: "fhirpath".to_string(),
                url: "http://octofhir.org/OperationDefinition/fhirpath".to_string(),
                kind: crate::operations::OperationKind::Operation,
                system: true,
                type_level: true,
                instance: true,
                resource: vec![], // All resource types
                parameters: vec![],
                affects_state: false,
            });
            tracing::info!("Registered $fhirpath operation");

            // Register CQL operations if enabled
            if cfg.cql.enabled {
                registry.register(crate::operations::OperationDefinition {
                    code: "cql".to_string(),
                    url: "http://octofhir.org/OperationDefinition/cql".to_string(),
                    kind: crate::operations::OperationKind::Operation,
                    system: true,
                    type_level: true,
                    instance: true,
                    resource: vec![], // All resource types
                    parameters: vec![],
                    affects_state: false,
                });
                tracing::info!("Registered $cql operation");

                registry.register(crate::operations::OperationDefinition {
                    code: "evaluate-measure".to_string(),
                    url: "http://hl7.org/fhir/OperationDefinition/Measure-evaluate-measure"
                        .to_string(),
                    kind: crate::operations::OperationKind::Operation,
                    system: false,
                    type_level: false,
                    instance: true,
                    resource: vec!["Measure".to_string()],
                    parameters: vec![],
                    affects_state: false,
                });
                tracing::info!("Registered $evaluate-measure operation");
            }

            // Register $reindex operation
            registry.register(crate::operations::OperationDefinition {
                code: "reindex".to_string(),
                url: "http://octofhir.org/OperationDefinition/reindex".to_string(),
                kind: crate::operations::OperationKind::Operation,
                system: true,
                type_level: true,
                instance: true,
                resource: vec![], // All resource types
                parameters: vec![],
                affects_state: true,
            });
            tracing::info!("Registered $reindex operation");

            // Register $import operation
            registry.register(crate::operations::OperationDefinition {
                code: "import".to_string(),
                url: "http://octofhir.org/OperationDefinition/import".to_string(),
                kind: crate::operations::OperationKind::Operation,
                system: true,
                type_level: false,
                instance: false,
                resource: vec![],
                parameters: vec![],
                affects_state: true,
            });
            tracing::info!("Registered $import operation");

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
                url: "http://hl7.org/fhir/uv/sql-on-fhir/OperationDefinition/ViewDefinition-run"
                    .to_string(),
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
                url: "http://hl7.org/fhir/uv/sql-on-fhir/OperationDefinition/ViewDefinition-sql"
                    .to_string(),
                kind: crate::operations::OperationKind::Operation,
                system: false,
                type_level: true,
                instance: true,
                resource: vec!["ViewDefinition".to_string()],
                parameters: vec![],
                affects_state: false,
            });
            // Register $status for Subscription (R5 subscription status)
            registry.register(crate::operations::OperationDefinition {
                code: "status".to_string(),
                url: "http://hl7.org/fhir/OperationDefinition/Subscription-status".to_string(),
                kind: crate::operations::OperationKind::Operation,
                system: false,
                type_level: false,
                instance: true,
                resource: vec!["Subscription".to_string()],
                parameters: vec![],
                affects_state: false,
            });
            // Register $fhirpath operation
            registry.register(crate::operations::OperationDefinition {
                code: "fhirpath".to_string(),
                url: "http://octofhir.org/OperationDefinition/fhirpath".to_string(),
                kind: crate::operations::OperationKind::Operation,
                system: true,
                type_level: true,
                instance: true,
                resource: vec![], // All resource types
                parameters: vec![],
                affects_state: false,
            });

            // Register CQL operations if enabled
            if cfg.cql.enabled {
                registry.register(crate::operations::OperationDefinition {
                    code: "cql".to_string(),
                    url: "http://octofhir.org/OperationDefinition/cql".to_string(),
                    kind: crate::operations::OperationKind::Operation,
                    system: true,
                    type_level: true,
                    instance: true,
                    resource: vec![], // All resource types
                    parameters: vec![],
                    affects_state: false,
                });

                registry.register(crate::operations::OperationDefinition {
                    code: "evaluate-measure".to_string(),
                    url: "http://hl7.org/fhir/OperationDefinition/Measure-evaluate-measure"
                        .to_string(),
                    kind: crate::operations::OperationKind::Operation,
                    system: false,
                    type_level: false,
                    instance: true,
                    resource: vec!["Measure".to_string()],
                    parameters: vec![],
                    affects_state: false,
                });
            }

            // Register $reindex operation
            registry.register(crate::operations::OperationDefinition {
                code: "reindex".to_string(),
                url: "http://octofhir.org/OperationDefinition/reindex".to_string(),
                kind: crate::operations::OperationKind::Operation,
                system: true,
                type_level: true,
                instance: true,
                resource: vec![], // All resource types
                parameters: vec![],
                affects_state: true,
            });

            // Register $import operation
            registry.register(crate::operations::OperationDefinition {
                code: "import".to_string(),
                url: "http://octofhir.org/OperationDefinition/import".to_string(),
                kind: crate::operations::OperationKind::Operation,
                system: true,
                type_level: false,
                instance: false,
                resource: vec![],
                parameters: vec![],
                affects_state: true,
            });

            Arc::new(registry)
        }
    };

    // Register core operation handlers
    tracing::info!(
        cql_enabled = cfg.cql.enabled,
        "Registering operation handlers with CQL enabled flag"
    );
    let operation_handlers: Arc<HashMap<String, DynOperationHandler>> =
        Arc::new(register_core_operations_all(
            fhirpath_engine.clone(),
            model_provider.clone(),
            cfg.bulk_export.clone(),
            cfg.sql_on_fhir.clone(),
            cfg.cql.enabled,
            cfg.reindex.clone(),
            cfg.bulk_import.clone(),
        ));
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

    // Process gateway routes from parallel Phase 2
    match gateway_routes_result {
        Ok(count) => {
            tracing::info!(count = count, "Loaded initial gateway routes");
        }
        Err(e) => {
            tracing::warn!(error = %e, "Failed to load initial gateway routes");
        }
    }

    // Initialize custom handler registry
    let handler_registry = Arc::new(crate::gateway::HandlerRegistry::new());
    tracing::info!("Initialized custom handler registry");

    // Process compartment registry from parallel Phase 2
    let compartment_registry = match compartment_result {
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
    };

    // Start background cleanup task for expired jobs
    let _cleanup_handle = async_job_manager.clone().start_cleanup_task();
    tracing::info!("Async job cleanup task started");

    // Start background cleanup task for expired bulk export files
    if cfg.bulk_export.enabled {
        let export_path = cfg.bulk_export.export_path.clone();
        let retention_hours = cfg.bulk_export.retention_hours;
        let _export_cleanup_handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(3600));
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

    // Create policy evaluator with policies evaluated FIRST
    let policy_evaluator = Arc::new(PolicyEvaluator::new(
        policy_cache.clone(),
        PolicyEvaluatorConfig {
            evaluate_scopes_first: false,
            quickjs_enabled: cfg.auth.policy.quickjs_enabled,
            quickjs_config: cfg.auth.policy.quickjs.clone(),
            ..PolicyEvaluatorConfig::default()
        },
    ));
    tracing::info!("Policy evaluator initialized");

    // Bootstrap operations registry (source of truth for public paths)
    let (operation_registry, gateway_provider) =
        crate::bootstrap::bootstrap_operations(db_pool.as_ref(), &storage, cfg)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to bootstrap operations registry: {}", e))?;

    tracing::info!("Operations registry bootstrapped with in-memory indexes");

    // Create GraphQL state if enabled (schema build was started early)
    let (graphql_state, graphql_subscription_broadcaster) = if let Some(lazy_schema) = lazy_schema {
        // Create PostgresStorage from the pool for GraphQL
        let graphql_storage: octofhir_storage::DynStorage =
            Arc::new(PostgresStorage::from_pool(db_pool.as_ref().clone()));

        // Create subscription broadcaster for real-time updates
        let subscription_broadcaster =
            octofhir_graphql::subscriptions::ResourceEventBroadcaster::new_shared();

        // Create context template with shared dependencies
        let context_template = GraphQLContextTemplate {
            storage: graphql_storage,
            search_config: (*search_config.config()).clone(),
            policy_evaluator: policy_evaluator.clone(),
            subscription_broadcaster: Some(subscription_broadcaster.clone()),
        };

        let state = GraphQLState {
            lazy_schema,
            context_template,
        };

        // Schema build was already started early, no need to trigger again
        tracing::info!("GraphQL state initialized (schema build started early)");

        (Some(state), Some(subscription_broadcaster))
    } else {
        (None, None)
    };

    // Initialize CQL service if enabled
    let cql_service = if cfg.cql.enabled {
        tracing::info!("Initializing CQL service...");

        // Create FHIR data provider with FHIRPath engine for property navigation
        let data_provider = Arc::new(
            octofhir_cql_service::data_provider::FhirServerDataProvider::new(
                storage.clone(),
                fhirpath_engine.clone(),
                cfg.cql.max_retrieve_size,
            ),
        );

        // Create terminology adapter
        let cql_terminology =
            Arc::new(octofhir_cql_service::terminology_provider::CqlTerminologyProvider::new());

        // Create library cache (with optional Redis L2 cache)
        let library_cache = Arc::new(octofhir_cql_service::library_cache::LibraryCache::new(
            cfg.cql.cache_capacity,
        ));

        // Create CQL service
        let service = octofhir_cql_service::CqlService::new(
            data_provider,
            cql_terminology,
            library_cache,
            storage.clone(),
            cfg.cql.clone(),
        );

        tracing::info!("CQL service initialized");
        Some(Arc::new(service))
    } else {
        tracing::info!("CQL service disabled");
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
                    tracing::debug!(auth_removed, jwt_removed, "Cache cleanup completed");
                }
            }
        });
        tracing::info!("Background cache cleanup task started (60s interval)");
    }

    // Spawn background SSO session cleanup task (runs every hour)
    {
        let storage_for_cleanup = storage.clone();
        tokio::spawn(async move {
            use octofhir_auth::storage::sso_session::SsoSessionStorage;
            use octofhir_auth_postgres::PostgresSsoSessionStorage;

            let sso_storage = PostgresSsoSessionStorage::new(storage_for_cleanup);

            // Run every hour (3600 seconds)
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(3600));
            loop {
                interval.tick().await;

                match sso_storage.cleanup_expired_sessions().await {
                    Ok(removed) => {
                        if removed > 0 {
                            tracing::info!(
                                sessions_removed = removed,
                                "SSO session cleanup completed"
                            );
                        } else {
                            tracing::debug!("SSO session cleanup: no expired sessions found");
                        }
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "Failed to cleanup expired SSO sessions");
                    }
                }
            }
        });
        tracing::info!("Background SSO session cleanup task started (1h interval)");
    }

    // Process resource types from parallel Phase 2
    let resource_types = resource_types_result.unwrap_or_default();
    let resource_type_set = Arc::new(ArcSwap::from_pointee(
        resource_types.iter().cloned().collect::<HashSet<String>>(),
    ));
    tracing::info!(
        resource_types = resource_type_set.load().len(),
        "Resource types loaded for validation"
    );

    // Build CapabilityStatement at startup (cached for /metadata requests)
    // Reuses the search registry that was already loaded by ReloadableSearchConfig
    let mut capability_statement = handlers::build_capability_statement(
        &cfg.fhir.version,
        &cfg.base_url(),
        &db_pool,
        &resource_types,
        &search_config.config().registry,
    )
    .await;

    // Add SMART on FHIR security extensions (oauth-uris) to CapabilityStatement
    if let Ok(base_url) = url::Url::parse(&cfg.base_url()) {
        if let Err(e) =
            octofhir_auth::add_smart_security(&mut capability_statement, &cfg.auth.smart, &base_url)
        {
            tracing::warn!(error = %e, "Failed to add SMART security to CapabilityStatement");
        }
    }

    let capability_statement = Arc::new(capability_statement);
    tracing::info!("CapabilityStatement built and cached");

    // Register hooks for unified event system
    // Hooks run asynchronously with isolation (timeout + panic recovery)
    let hook_registry = HookRegistry::new();

    // PolicyReloadHook: triggers policy cache reload on AccessPolicy changes
    let policy_hook = PolicyReloadHook::new(policy_notifier.clone());
    hook_registry.register_resource(Arc::new(policy_hook)).await;

    // GatewayReloadHook: triggers gateway route reload on App/CustomOperation changes
    let gateway_hook = GatewayReloadHook::new(gateway_router.clone(), storage.clone())
        .with_operation_registry(operation_registry.clone())
        .with_gateway_provider(gateway_provider.clone());

    hook_registry
        .register_resource(Arc::new(gateway_hook))
        .await;

    // SearchParamHook: updates search parameter registry on SearchParameter changes
    let search_hook = SearchParamHook::new(search_config.clone());
    hook_registry.register_resource(Arc::new(search_hook)).await;

    // GraphQLSubscriptionHook: forwards events to GraphQL subscription clients
    if let Some(ref graphql_broadcaster) = graphql_subscription_broadcaster {
        let graphql_hook = GraphQLSubscriptionHook::new(graphql_broadcaster.clone());
        hook_registry
            .register_resource(Arc::new(graphql_hook))
            .await;
        tracing::debug!("GraphQL subscription hook registered");
    }

    // AsyncAuditHook: logs resource changes asynchronously as FHIR AuditEvents
    if audit_service.is_enabled() {
        let audit_hook = AsyncAuditHook::new(audit_service.clone());
        hook_registry.register_resource(Arc::new(audit_hook)).await;
        tracing::debug!("Async audit hook registered");
    }

    // SubscriptionHook: dispatches resource events to matching FHIR subscriptions
    // This will be fully initialized later once AppState is ready
    let mut subscription_state = {
        // Create event storage
        let event_storage = Arc::new(crate::subscriptions::SubscriptionEventStorage::new(
            db_pool.as_ref().clone(),
        ));

        // Create topic registry
        let topic_registry = Arc::new(crate::subscriptions::TopicRegistry::new(storage.clone()));

        // Create subscription manager
        let subscription_manager = Arc::new(crate::subscriptions::SubscriptionManager::new(
            storage.clone(),
            db_pool.as_ref().clone(),
        ));

        // Create event matcher with FHIRPath engine
        let event_matcher = Arc::new(crate::subscriptions::EventMatcher::new(
            fhirpath_engine.clone(),
        ));

        // Create subscription hook
        let subscription_hook = SubscriptionHook::new(
            topic_registry.clone(),
            subscription_manager.clone(),
            event_matcher.clone(),
            event_storage.clone(),
            true, // enabled
        );
        hook_registry
            .register_resource(Arc::new(subscription_hook))
            .await;
        tracing::debug!("Subscription hook registered");

        // Return state for later use
        // Create WebSocket registry for subscription connections
        let websocket_registry = Arc::new(crate::subscriptions::WebSocketRegistry::new());

        SubscriptionState {
            topic_registry,
            subscription_manager,
            event_matcher,
            event_storage,
            websocket_registry,
            enabled: true,
            delivery_shutdown: None,
        }
    };

    // Load subscription topics into cache
    // Note: SubscriptionTopic table is created via octofhir-subscription IG for R4/R4B,
    // or via the native R5+ core package
    if let Err(e) = subscription_state.topic_registry.reload().await {
        tracing::warn!(error = %e, "Failed to load subscription topics on startup");
    } else {
        tracing::info!(
            topics = subscription_state.topic_registry.topic_count(),
            "Subscription topics loaded"
        );
    }

    // Start subscription delivery processor in background
    let delivery_shutdown = {
        let processor = crate::subscriptions::delivery::DeliveryProcessor::new(
            subscription_state.event_storage.clone(),
            subscription_state.subscription_manager.clone(),
            subscription_state.websocket_registry.clone(),
        );
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        let shutdown_handle = Arc::new(shutdown_tx);

        tokio::spawn(async move {
            if let Err(e) = processor.run(shutdown_rx).await {
                tracing::error!(error = %e, "Subscription delivery processor failed");
            }
        });
        tracing::info!("Subscription delivery processor started");

        shutdown_handle
    };

    // Store shutdown handle in subscription state (keeps it alive for server lifetime)
    subscription_state.delivery_shutdown = Some(delivery_shutdown);

    // RedisPublishHook + RedisEventSync: multi-instance event synchronization
    // When Redis is enabled:
    // - RedisPublishHook publishes local events to Redis for other instances
    // - RedisEventSync subscribes to Redis and forwards events to local broadcaster
    if cfg.redis.enabled {
        match create_redis_pool(&cfg.redis).await {
            Ok(redis_pool) => {
                // Register hook to publish events to Redis
                let publish_hook = RedisPublishHook::new(redis_pool.clone());
                hook_registry
                    .register_resource(Arc::new(publish_hook))
                    .await;

                // Start Redis subscriber to receive events from other instances
                match RedisEventSyncBuilder::new()
                    .with_pool(redis_pool)
                    .with_broadcaster(event_broadcaster.clone())
                    .with_redis_url(&cfg.redis.url)
                    .start()
                {
                    Ok(_handle) => {
                        tracing::info!("Redis event sync enabled for multi-instance deployment");
                    }
                    Err(e) => {
                        tracing::warn!(
                            error = %e,
                            "Failed to start Redis event sync, continuing without multi-instance sync"
                        );
                    }
                }
            }
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    "Failed to create Redis pool for event sync, continuing without multi-instance sync"
                );
            }
        }
    }

    // Start the event dispatcher - subscribes to broadcaster and dispatches to hooks
    let hook_registry = Arc::new(hook_registry);
    let dispatcher = octofhir_core::events::HookDispatcher::new(hook_registry.clone());
    tokio::spawn(dispatcher.run(event_broadcaster.subscribe()));
    tracing::info!(
        "Unified event system: {} hooks registered, dispatcher started",
        hook_registry.hook_count().await
    );

    // Create basic auth storage (unified Client and App authentication)
    let basic_auth_storage = Arc::new(ArcBasicAuthStorage::new(db_pool.clone()));

    // Create notification queue storage
    let notification_queue: Arc<dyn octofhir_notifications::NotificationQueueStorage> = Arc::new(
        octofhir_db_postgres::PostgresNotificationStorage::new(db_pool.as_ref().clone()),
    );

    // Create AppState wrapped in Arc for cheap cloning across all middleware/handlers
    // This is a single Arc::clone per request instead of cloning 25+ individual fields
    let state = AppState(Arc::new(AppStateInner {
        storage,
        search_config,
        fhir_version: cfg.fhir.version.clone(),
        base_url: cfg.base_url(),
        fhirpath_engine,
        model_provider,
        fhir_operations,
        operation_handlers,
        validation_service,
        gateway_router: (*gateway_router).clone(),
        db_pool,
        read_db_pool,
        handler_registry,
        compartment_registry,
        async_job_manager,
        config: Arc::new(cfg.clone()),
        policy_evaluator,
        policy_cache,
        policy_reload_service,
        auth_state,
        graphql_state,
        cql_service,
        operation_registry,
        auth_cache,
        jwt_cache,
        json_schema_cache: Arc::new(dashmap::DashMap::new()),
        resource_cache: if cfg.cache.resource_ttl_secs > 0 {
            Some(Arc::new(crate::cache::ResourceCache::new(
                crate::cache::CacheBackend::new_local(),
                std::time::Duration::from_secs(cfg.cache.resource_ttl_secs),
            )))
        } else {
            None
        },
        query_cache: Some(Arc::new(octofhir_search::QueryCache::new(
            cfg.search.cache_capacity,
        ))),
        resource_type_set,
        capability_statement,
        config_manager: Some(config_manager),
        audit_service,
        basic_auth_storage,
        notification_queue: Some(notification_queue),
        package_store,
        subscription_state,
        terminology_provider,
        // automation_state,
    }));

    // Configure async job executor to handle bulk export and ViewDefinition export jobs
    let state_for_executor = state.clone();
    let executor: crate::async_jobs::JobExecutor = Arc::new(
        move |job_id: uuid::Uuid,
              request_type: String,
              _method: String,
              _url: String,
              body: Option<serde_json::Value>| {
            let state = state_for_executor.clone();
            Box::pin(async move {
                match request_type.as_str() {
                    "bulk_export" => {
                        // Extract job parameters from body
                        let body = body.ok_or_else(|| "Missing job parameters".to_string())?;

                        // Execute the bulk export
                        crate::operations::execute_bulk_export(state, job_id, body).await
                    }
                    "viewdefinition_export" => {
                        // Extract job parameters from body
                        let body = body.ok_or_else(|| "Missing job parameters".to_string())?;

                        // Execute the ViewDefinition export
                        crate::operations::execute_viewdefinition_export(state, job_id, body).await
                    }
                    "reindex" => {
                        let body = body.ok_or_else(|| "Missing job parameters".to_string())?;
                        crate::operations::execute_reindex(state, job_id, body).await
                    }
                    "bulk_import" => {
                        let body = body.ok_or_else(|| "Missing job parameters".to_string())?;
                        crate::operations::execute_bulk_import(state, job_id, body).await
                    }
                    _ => Err(format!("Unknown job type: {}", request_type)),
                }
            })
                as std::pin::Pin<
                    Box<dyn std::future::Future<Output = Result<serde_json::Value, String>> + Send>,
                >
        },
    );

    state.async_job_manager.set_executor(executor);
    tracing::info!("Async job executor configured for bulk export and ViewDefinition export");

    Ok(build_router(state, body_limit, cfg.server.compression))
}

/// Creates routes for internal administrative resources.
///
/// These routes handle FHIR CRUD operations for internal resources like
/// User, Role, Client, AccessPolicy, IdentityProvider, and CustomOperation.
/// They are served at the root level (e.g., /User, /Role) rather than under /fhir.
fn internal_resource_routes() -> Router<AppState> {
    Router::new()
        // User-specific operations
        .route(
            "/User/{id}/$reset-password",
            post(crate::admin::reset_user_password),
        )
        // Generic CRUD for internal resources
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

fn build_router(state: AppState, body_limit: usize, compression: bool) -> Router {
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
        // DB Console API endpoints
        .route(
            "/api/db-console/history",
            get(crate::operations::db_console_api::list_history)
                .post(crate::operations::db_console_api::save_history)
                .delete(crate::operations::db_console_api::clear_history),
        )
        .route(
            "/api/db-console/tables",
            get(crate::operations::db_console_api::list_tables),
        )
        .route(
            "/api/db-console/tables/{schema}/{table}",
            get(crate::operations::db_console_api::get_table_detail),
        )
        .route(
            "/api/db-console/active-queries",
            get(crate::operations::db_console_api::list_active_queries),
        )
        .route(
            "/api/db-console/terminate-query",
            axum::routing::post(crate::operations::db_console_api::terminate_query),
        )
        .route(
            "/api/db-console/indexes/{schema}/{index_name}",
            axum::routing::delete(crate::operations::db_console_api::drop_index),
        )
        // LSP WebSocket endpoints (authenticated)
        .route("/api/lsp/pg", get(crate::lsp::pg_lsp_websocket_handler))
        .route(
            "/api/lsp/fhirpath",
            get(crate::lsp::fhirpath_lsp_websocket_handler),
        )
        // Log stream WebSocket endpoint (authenticated, admin scope required)
        .route(
            "/api/logs/stream",
            get(crate::log_stream::log_stream_handler),
        )
        // System API (for App integrations)
        .nest("/api/system", crate::routes::system::system_routes())
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
        // API routes (nested under /api)
        .nest("/api", {
            let api_router = Router::new().merge(crate::routes::canonical::canonical_routes());

            /*
            // Add automation routes only if automation is enabled
            let api_router = if state.automation_state.is_some() {
                api_router.merge(crate::automations::automation_routes())
            } else {
                api_router
            };
            */
            let api_router = api_router;

            api_router
                // Allow larger uploads (50MB) for canonical package upload
                .layer(axum::extract::DefaultBodyLimit::max(50 * 1024 * 1024))
        })
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
        // Compartment search + instance operation routes (must come before generic instance routes)
        // GET /Patient/123/Observation - compartment search
        // POST /Patient/123/$reindex - instance-level operation
        .route(
            "/Patient/{id}/{resource_type}",
            get(handlers::compartment_search).post(crate::operations::compartment_post_handler),
        )
        .route(
            "/Encounter/{id}/{resource_type}",
            get(handlers::compartment_search).post(crate::operations::compartment_post_handler),
        )
        .route(
            "/Practitioner/{id}/{resource_type}",
            get(handlers::compartment_search).post(crate::operations::compartment_post_handler),
        )
        .route(
            "/RelatedPerson/{id}/{resource_type}",
            get(handlers::compartment_search).post(crate::operations::compartment_post_handler),
        )
        .route(
            "/Device/{id}/{resource_type}",
            get(handlers::compartment_search).post(crate::operations::compartment_post_handler),
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
        // Subscription WebSocket endpoint: /Subscription/{id}/$events
        // Must be before generic operation handler since $events requires WebSocket upgrade
        .route(
            "/Subscription/{id}/$events",
            get(subscription_events_ws_handler),
        )
        // Vread: GET /[type]/[id]/_history/[vid]
        .route(
            "/{resource_type}/{id}/_history/{version_id}",
            get(handlers::vread_resource),
        )
        // Instance-level operations and history: GET/POST /{type}/{id}/{operation_or_history}
        // Note: /{type}/{id}/_history and /{type}/{id}/{operation} cannot coexist as separate
        // routes in matchit, so we use a single catch-all and dispatch inside the handler.
        .route(
            "/{resource_type}/{id}/{operation}",
            get(crate::operations::instance_operation_or_history_handler)
                .post(crate::operations::instance_operation_handler),
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
        // Combined root/system-search at base /fhir
        // GET /fhir - returns service info
        // GET /fhir?_type=Patient,Observation - system-level search
        // POST /fhir - transaction/batch
        .route(
            "/",
            get(handlers::fhir_root).post(handlers::transaction_handler),
        )
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

    // Add root-level gateway fallback for custom App operations.
    // This handles paths not matched by explicit routes (e.g., /myapp/users/123/profile).
    // The internal_*_resource handlers already check gateway for 1-2 segment paths,
    // but we need this fallback for paths with 3+ segments.
    router = router.fallback(crate::gateway::router::gateway_fallback_handler);

    // Apply middleware stack (outer to inner):
    // DefaultBodyLimit → trace_metrics(+request_id) → cors → compression →
    //   auth_combined(+content_negotiation) → audit → handler
    // 6 layers total (down from 10), reducing Tower BoxCloneSyncService clone overhead.
    // Note: Layers wrap from outside-in, so first .layer() is closest to handler.

    router = router
        // Audit middleware runs closest to handler, after auth context is set
        .layer(middleware::from_fn_with_state(
            state.clone(),
            app_middleware::audit_middleware,
        ))
        // Combined auth (authn + authz) — single layer instead of two
        .layer(middleware::from_fn_with_state(
            state.clone(),
            app_middleware::auth_middleware,
        ));

    let router = if compression {
        use tower_http::compression::predicate::{DefaultPredicate, Predicate, SizeAbove};
        router.layer(
            CompressionLayer::new()
                .compress_when(DefaultPredicate::new().and(SizeAbove::new(1024))),
        )
    } else {
        router
    };
    let router: Router = router
        .layer(middleware::from_fn(app_middleware::dynamic_cors_middleware))
        // Combined tracing + metrics + request ID (replaces separate TraceLayer + metrics_middleware)
        .layer(middleware::from_fn(
            app_middleware::trace_metrics_middleware,
        ))
        .with_state(state)
        .layer(axum::extract::DefaultBodyLimit::max(body_limit));

    // Merge OAuth routes if auth is enabled
    // OAuth routes have their own state and are not subject to FHIR content negotiation
    // Note: OAuth routes intentionally bypass auth middleware as they're public

    if let Some(oauth) = oauth_routes {
        router.merge(oauth)
    } else {
        router
    }
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
    let smart_config_router =
        crate::oauth::smart_config_route(oauth_state.smart_config_state.clone());
    let userinfo_router = crate::oauth::userinfo_route(auth_state.clone());
    let authorize_router = crate::oauth::authorize_route(oauth_state.authorize_state);
    let launch_router = crate::oauth::launch_route(oauth_state.launch_state);

    // Duplicate discovery routes under /fhir so that
    // /fhir/.well-known/smart-configuration works when Inferno uses /fhir as base URL
    let fhir_discovery = crate::oauth::smart_config_route(oauth_state.smart_config_state);

    tracing::info!(
        "OAuth routes enabled: /auth/token, /auth/logout, /auth/authorize, /auth/launch, /auth/userinfo, /auth/jwks, /.well-known/smart-configuration, /fhir/.well-known/smart-configuration"
    );

    Some(
        Router::new()
            .merge(oauth_router)
            .merge(jwks_router)
            .merge(smart_config_router)
            .merge(userinfo_router)
            .merge(authorize_router)
            .merge(launch_router)
            .nest("/fhir", fhir_discovery)
            // Apply CORS middleware to OAuth routes to allow cross-origin requests
            .layer(middleware::from_fn(app_middleware::dynamic_cors_middleware)),
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
            .map_err(|e| {
                anyhow::anyhow!("Failed to load JWT signing key from configuration: {}", e)
            })?;
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
            SigningAlgorithm::ES384 => SigningKeyPair::generate_ec()
                .map_err(|e| anyhow::anyhow!("Failed to generate EC signing key: {}", e))?,
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

    Ok(AuthState::new(
        jwt_service,
        client_storage,
        revoked_token_storage,
        user_storage,
    )
    .with_cookie_config(cfg.auth.cookie.clone()))
}
