//! Redis pub/sub synchronization for multi-instance deployments.
//!
//! This module provides cross-instance event synchronization using Redis pub/sub.
//! When a resource change occurs on one server instance, the event is:
//! 1. Published to a Redis channel
//! 2. Received by all other instances subscribed to the channel
//! 3. Forwarded to local hooks for processing
//!
//! This enables features like cache invalidation across all instances.

use std::sync::Arc;
use std::time::Duration;

use deadpool_redis::{Pool, redis::AsyncCommands};
use futures_util::StreamExt;
use octofhir_core::events::{EventBroadcaster, ResourceEvent};
use tokio::time::sleep;
use tracing::{debug, error, info, warn};

use super::{SerializableEvent, REDIS_CHANNEL};

/// Redis event synchronization for multi-instance deployments.
///
/// This component:
/// - Publishes local events to Redis pub/sub
/// - Subscribes to Redis and forwards events to the local broadcaster
///
/// # Example
///
/// ```ignore
/// let redis_sync = RedisEventSync::new(redis_pool, event_broadcaster);
/// tokio::spawn(redis_sync.run());
/// ```
pub struct RedisEventSync {
    pool: Pool,
    broadcaster: Arc<EventBroadcaster>,
    redis_url: String,
}

impl RedisEventSync {
    /// Create a new Redis event sync.
    ///
    /// # Arguments
    ///
    /// * `pool` - Redis connection pool
    /// * `broadcaster` - Event broadcaster for forwarding received events
    /// * `redis_url` - Redis connection URL for pub/sub subscription
    pub fn new(pool: Pool, broadcaster: Arc<EventBroadcaster>, redis_url: String) -> Self {
        Self {
            pool,
            broadcaster,
            redis_url,
        }
    }

    /// Publish an event to Redis for other instances.
    ///
    /// This should be called after local event processing to sync across instances.
    pub async fn publish(&self, event: &ResourceEvent) -> Result<(), RedisEventError> {
        let mut conn = self
            .pool
            .get()
            .await
            .map_err(|e| RedisEventError::Pool(e.to_string()))?;

        let message = serde_json::to_string(&SerializableEvent::from(event))
            .map_err(|e| RedisEventError::Serialization(e.to_string()))?;

        let _: () = conn
            .publish(REDIS_CHANNEL, &message)
            .await
            .map_err(|e| RedisEventError::Publish(e.to_string()))?;

        debug!(
            resource_type = %event.resource_type,
            resource_id = %event.resource_id,
            "Published event to Redis"
        );

        Ok(())
    }

    /// Start the Redis subscription loop.
    ///
    /// This spawns a background task that:
    /// 1. Connects to Redis with SUBSCRIBE
    /// 2. Receives events from other instances
    /// 3. Forwards them to the local broadcaster
    ///
    /// The task automatically reconnects on connection failures.
    pub async fn run(self: Arc<Self>) {
        info!(channel = REDIS_CHANNEL, "Starting Redis event sync");

        loop {
            match self.subscribe_loop().await {
                Ok(()) => {
                    info!("Redis event sync stopped gracefully");
                    break;
                }
                Err(e) => {
                    error!(error = %e, "Redis event sync error, reconnecting in 5s");
                    sleep(Duration::from_secs(5)).await;
                }
            }
        }
    }

    /// Main subscription loop.
    async fn subscribe_loop(&self) -> Result<(), RedisEventError> {
        use deadpool_redis::redis::Client;

        // Create a separate client for pub/sub (can't use pooled connections for SUBSCRIBE)
        let client = Client::open(self.redis_url.as_str())
            .map_err(|e| RedisEventError::Connection(e.to_string()))?;

        let mut pubsub = client
            .get_async_pubsub()
            .await
            .map_err(|e| RedisEventError::Connection(e.to_string()))?;

        pubsub
            .subscribe(REDIS_CHANNEL)
            .await
            .map_err(|e| RedisEventError::Subscribe(e.to_string()))?;

        info!(
            channel = REDIS_CHANNEL,
            "Subscribed to Redis event channel"
        );

        let mut stream = pubsub.on_message();

        loop {
            match stream.next().await {
                Some(msg) => {
                    let payload: String = msg
                        .get_payload()
                        .map_err(|e: deadpool_redis::redis::RedisError| RedisEventError::Message(e.to_string()))?;

                    match serde_json::from_str::<SerializableEvent>(&payload) {
                        Ok(event) => {
                            let resource_event = event.into_resource_event();

                            debug!(
                                resource_type = %resource_event.resource_type,
                                resource_id = %resource_event.resource_id,
                                "Received event from Redis"
                            );

                            // Forward to local broadcaster
                            self.broadcaster.send_resource(resource_event);
                        }
                        Err(e) => {
                            warn!(
                                error = %e,
                                payload = %payload,
                                "Failed to deserialize Redis event"
                            );
                        }
                    }
                }
                None => {
                    warn!("Redis pub/sub stream ended");
                    return Err(RedisEventError::StreamEnded);
                }
            }
        }
    }
}

/// Errors that can occur during Redis event sync.
#[derive(Debug, thiserror::Error)]
pub enum RedisEventError {
    #[error("Redis pool error: {0}")]
    Pool(String),

    #[error("Redis connection error: {0}")]
    Connection(String),

    #[error("Redis subscribe error: {0}")]
    Subscribe(String),

    #[error("Redis publish error: {0}")]
    Publish(String),

    #[error("Redis message error: {0}")]
    Message(String),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Redis pub/sub stream ended")]
    StreamEnded,
}

/// Builder for Redis event sync.
#[derive(Default)]
pub struct RedisEventSyncBuilder {
    pool: Option<Pool>,
    broadcaster: Option<Arc<EventBroadcaster>>,
    redis_url: Option<String>,
}

impl RedisEventSyncBuilder {
    /// Create a new builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the Redis pool.
    pub fn with_pool(mut self, pool: Pool) -> Self {
        self.pool = Some(pool);
        self
    }

    /// Set the event broadcaster.
    pub fn with_broadcaster(mut self, broadcaster: Arc<EventBroadcaster>) -> Self {
        self.broadcaster = Some(broadcaster);
        self
    }

    /// Set the Redis URL.
    pub fn with_redis_url(mut self, url: impl Into<String>) -> Self {
        self.redis_url = Some(url.into());
        self
    }

    /// Build and start the Redis event sync.
    ///
    /// Returns a handle to the spawned task.
    pub fn start(self) -> Result<tokio::task::JoinHandle<()>, RedisEventError> {
        let pool = self.pool.ok_or_else(|| {
            RedisEventError::Pool("Redis pool is required".to_string())
        })?;

        let broadcaster = self.broadcaster.ok_or_else(|| {
            RedisEventError::Pool("Event broadcaster is required".to_string())
        })?;

        let redis_url = self.redis_url.ok_or_else(|| {
            RedisEventError::Connection("Redis URL is required".to_string())
        })?;

        let sync = Arc::new(RedisEventSync::new(pool, broadcaster, redis_url));
        Ok(tokio::spawn(sync.run()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use octofhir_core::events::ResourceEventType;

    #[test]
    fn test_serializable_event_roundtrip() {
        let event = ResourceEvent::created("Patient", "123", serde_json::json!({"id": "123"}));
        let serializable = SerializableEvent::from(&event);
        let roundtrip = serializable.into_resource_event();

        assert_eq!(roundtrip.resource_type, "Patient");
        assert_eq!(roundtrip.resource_id, "123");
        assert!(matches!(roundtrip.event_type, ResourceEventType::Created));
    }
}
