pub mod admin;
pub mod async_jobs;
pub mod bootstrap;
pub mod cache;
pub mod canonical;
pub mod compartments;
pub mod config;
pub mod config_manager;
pub mod gateway;
pub mod handlers;
pub mod mapping;
pub mod middleware;
pub mod observability;
pub mod operations;
pub mod patch;
pub mod server;
pub mod storage_adapter;
pub mod validation;

pub use admin::{AdminState, CombinedAdminState, admin_routes};
pub use async_jobs::{AsyncJob, AsyncJobConfig, AsyncJobError, AsyncJobManager, AsyncJobStatus};
pub use cache::{CacheBackend, CachedEntry};
pub use compartments::{CompartmentDefinition, CompartmentRegistry};
pub use config::{
    AppConfig, CacheConfig, OtelConfig, PostgresStorageConfig, RedisConfig, ServerConfig,
};
pub use config_manager::{ServerConfigManager, ServerConfigManagerBuilder};
pub use observability::{init_tracing, shutdown_tracing};
pub use server::{AppState, OctofhirServer, ServerBuilder, build_app};

/// Create a cache backend based on configuration.
///
/// ## Cache Modes
///
/// - **Redis disabled**: Returns local-only cache (DashMap)
/// - **Redis enabled**: Attempts to connect to Redis, falls back to local on failure
///
/// ## Graceful Degradation
///
/// If Redis connection fails, the system automatically falls back to local-only mode.
/// This allows the server to start and run even if Redis is unavailable.
pub async fn create_cache_backend(config: &RedisConfig) -> CacheBackend {
    use std::time::Duration;

    if !config.enabled {
        tracing::info!("Redis disabled, using local cache only");
        return CacheBackend::new_local();
    }

    tracing::info!(url = %config.url, "Connecting to Redis");

    // Create Redis pool configuration
    let mut redis_config = deadpool_redis::Config::from_url(&config.url);
    if let Some(ref mut pool_config) = redis_config.pool {
        pool_config.max_size = config.pool_size;
        pool_config.timeouts.wait = Some(Duration::from_millis(config.timeout_ms));
        pool_config.timeouts.create = Some(Duration::from_millis(config.timeout_ms));
        pool_config.timeouts.recycle = Some(Duration::from_millis(config.timeout_ms));
    }

    // Create pool
    let pool = match redis_config.create_pool(Some(deadpool_redis::Runtime::Tokio1)) {
        Ok(pool) => pool,
        Err(e) => {
            tracing::warn!(
                error = %e,
                "Failed to create Redis pool. Falling back to local cache."
            );
            return CacheBackend::new_local();
        }
    };

    // Test connection
    match pool.get().await {
        Ok(_) => {
            tracing::info!("✓ Connected to Redis successfully");

            // Create cache backend with Redis
            let backend = CacheBackend::new_redis(pool.clone());

            // Start cache invalidation listener
            if let Some(local) = backend.local_cache() {
                cache::pubsub::CacheInvalidationListener {
                    redis_pool: pool,
                    redis_url: config.url.clone(),
                    local_cache: local.clone(),
                }
                .start()
                .await;
            }

            backend
        }
        Err(e) => {
            tracing::warn!(
                error = %e,
                "Failed to connect to Redis. Falling back to local cache."
            );
            CacheBackend::new_local()
        }
    }
}
