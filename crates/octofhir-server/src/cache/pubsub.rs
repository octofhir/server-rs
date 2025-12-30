//! Redis Pub/Sub for cross-instance cache invalidation.

use dashmap::DashMap;
use deadpool_redis::Pool;
use redis::AsyncCommands;
use std::sync::Arc;
use std::time::Duration;

use super::backend::CachedEntry;

/// Cache invalidation listener that subscribes to Redis Pub/Sub.
///
/// ## How It Works
///
/// 1. Subscribe to "cache:invalidate" channel
/// 2. When a message is received, invalidate the key in L1 cache
/// 3. This keeps L1 caches synchronized across multiple server instances
///
/// ## Example Flow
///
/// ```text
/// Instance 1: cache.invalidate("key1")
///   ↓
/// Redis Pub/Sub: PUBLISH cache:invalidate "key1"
///   ↓
/// Instance 2: Listener receives "key1" → removes from L1
/// Instance 3: Listener receives "key1" → removes from L1
/// ```
pub struct CacheInvalidationListener {
    pub redis_pool: Pool,
    pub redis_url: String,
    pub local_cache: Arc<DashMap<String, CachedEntry>>,
}

impl CacheInvalidationListener {
    /// Start listening for cache invalidation events.
    ///
    /// This spawns a background task that:
    /// 1. Subscribes to the "cache:invalidate" channel
    /// 2. Removes keys from L1 cache when invalidation events are received
    /// 3. Automatically reconnects with exponential backoff if the connection is lost
    pub async fn start(self) {
        tokio::spawn(async move {
            let mut backoff = Duration::from_secs(1);
            const MAX_BACKOFF: Duration = Duration::from_secs(300); // 5 minutes max

            loop {
                match self.run().await {
                    Ok(()) => {
                        // Connection closed gracefully, reset backoff
                        backoff = Duration::from_secs(1);
                    }
                    Err(e) => {
                        tracing::error!(
                            error = %e,
                            backoff_secs = backoff.as_secs(),
                            "Cache invalidation listener error, reconnecting..."
                        );
                        tokio::time::sleep(backoff).await;
                        // Exponential backoff with max limit
                        backoff = (backoff * 2).min(MAX_BACKOFF);
                    }
                }
            }
        });
    }

    async fn run(&self) -> Result<(), String> {
        use futures_util::StreamExt;

        // Create a dedicated Redis client for pub/sub
        let client = redis::Client::open(self.redis_url.clone())
            .map_err(|e| format!("failed to create Redis client: {e}"))?;

        // Get async connection and create pub/sub
        let conn = client
            .get_async_pubsub()
            .await
            .map_err(|e| format!("failed to get pub/sub connection: {e}"))?;

        let mut pubsub = conn;

        // Subscribe to invalidation channel
        pubsub
            .subscribe("cache:invalidate")
            .await
            .map_err(|e| format!("failed to subscribe: {e}"))?;

        tracing::info!("Subscribed to cache:invalidate channel");

        // Process messages
        let mut stream = pubsub.on_message();
        loop {
            match stream.next().await {
                Some(msg) => {
                    if let Ok(key) = msg.get_payload::<String>() {
                        tracing::debug!(key = %key, "received cache invalidation");
                        self.local_cache.remove(&key);
                    } else {
                        tracing::warn!("failed to parse invalidation message payload");
                    }
                }
                None => {
                    return Err("pub/sub connection closed".to_string());
                }
            }
        }
    }
}

/// Publish a cache invalidation event to other instances.
///
/// This is called automatically by `CacheBackend::invalidate()`,
/// but can also be called directly if needed.
pub async fn publish_invalidation(redis: &Pool, key: &str) -> Result<(), String> {
    let mut conn = redis
        .get()
        .await
        .map_err(|e| format!("failed to get Redis connection: {e}"))?;

    conn.publish::<_, _, ()>("cache:invalidate", key)
        .await
        .map_err(|e| format!("failed to publish invalidation: {e}"))?;

    tracing::debug!(key = %key, "published cache invalidation");
    Ok(())
}
