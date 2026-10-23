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

use super::{REDIS_CHANNEL, SerializableEvent};

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
    resource_cache: Option<Arc<crate::cache::ResourceCache>>,
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
            resource_cache: None,
        }
    }

    /// Publish an event to Redis for other instances.
    ///
    /// This should be called after local event processing to sync across instances.
    pub async fn publish(&self, event: &ResourceEvent) -> Result<(), RedisEventError> {
        if event.remote {
            return Ok(());
        }
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
                    if let Some(cache) = &self.resource_cache {
                        cache.suspend();
                    }
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

        if let Some(cache) = &self.resource_cache {
            cache.resume();
        }
        info!(channel = REDIS_CHANNEL, "Subscribed to Redis event channel");

        let mut stream = pubsub.on_message();

        loop {
            match stream.next().await {
                Some(msg) => {
                    let payload: String =
                        msg.get_payload()
                            .map_err(|e: deadpool_redis::redis::RedisError| {
                                RedisEventError::Message(e.to_string())
                            })?;

                    match serde_json::from_str::<SerializableEvent>(&payload) {
                        Ok(event) => {
                            if event.source_instance_id.as_deref()
                                == Some(super::INSTANCE_ID.as_str())
                            {
                                continue;
                            }
                            let resource_event = match event.into_resource_event() {
                                Ok(event) => event,
                                Err(error) => {
                                    warn!(%error, "Rejected invalid Redis resource event");
                                    continue;
                                }
                            };

                            debug!(
                                resource_type = %resource_event.resource_type,
                                resource_id = %resource_event.resource_id,
                                "Received event from Redis"
                            );

                            if let Some(cache) = &self.resource_cache {
                                cache
                                    .invalidate(
                                        &resource_event.resource_type,
                                        &resource_event.resource_id,
                                    )
                                    .await;
                            }
                            // Forward to local broadcaster
                            self.broadcaster.send_resource(resource_event);
                        }
                        Err(e) => {
                            warn!(
                                error = %e,
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
    resource_cache: Option<Arc<crate::cache::ResourceCache>>,
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

    /// Keep resource caching disabled across subscription gaps.
    pub fn with_resource_cache(mut self, cache: Option<Arc<crate::cache::ResourceCache>>) -> Self {
        self.resource_cache = cache;
        self
    }

    /// Build and start the Redis event sync.
    ///
    /// Returns a handle to the spawned task.
    pub fn start(self) -> Result<tokio::task::JoinHandle<()>, RedisEventError> {
        Ok(tokio::spawn(self.build()?.run()))
    }

    pub(crate) fn start_tracked(
        self,
        background: &crate::lifecycle::BackgroundRuntime,
    ) -> Result<tokio::task::JoinHandle<()>, RedisEventError> {
        Ok(background.spawn(self.build()?.run()))
    }

    fn build(self) -> Result<Arc<RedisEventSync>, RedisEventError> {
        let pool = self
            .pool
            .ok_or_else(|| RedisEventError::Pool("Redis pool is required".to_string()))?;

        let broadcaster = self
            .broadcaster
            .ok_or_else(|| RedisEventError::Pool("Event broadcaster is required".to_string()))?;

        let redis_url = self
            .redis_url
            .ok_or_else(|| RedisEventError::Connection("Redis URL is required".to_string()))?;

        let mut sync = RedisEventSync::new(pool, broadcaster, redis_url);
        sync.resource_cache = self.resource_cache;
        Ok(Arc::new(sync))
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
        let roundtrip = serializable.into_resource_event().unwrap();

        assert_eq!(roundtrip.resource_type, "Patient");
        assert_eq!(roundtrip.resource_id, "123");
        assert!(matches!(roundtrip.event_type, ResourceEventType::Created));
    }

    #[tokio::test]
    async fn redis_transport_rejects_corruption_and_does_not_echo_events() {
        use octofhir_core::events::ResourceHook;
        use testcontainers::{ImageExt, runners::AsyncRunner};
        let container = testcontainers_modules::redis::Redis::default()
            .with_tag("7-alpine")
            .start()
            .await
            .unwrap();
        let port = container.get_host_port_ipv4(6379).await.unwrap();
        let url = format!("redis://127.0.0.1:{port}");
        let pool = deadpool_redis::Config::from_url(&url)
            .create_pool(Some(deadpool_redis::Runtime::Tokio1))
            .unwrap();
        let broadcaster = EventBroadcaster::new_shared();
        let mut receiver = broadcaster.subscribe();
        let cache = Arc::new(crate::cache::ResourceCache::new(
            100,
            Duration::from_secs(60),
        ));
        cache.suspend();
        let mut sync = RedisEventSync::new(pool.clone(), broadcaster, url.clone());
        sync.resource_cache = Some(cache.clone());
        let sync = Arc::new(sync);
        let subscriber = {
            let sync = sync.clone();
            tokio::spawn(async move { sync.subscribe_loop().await })
        };
        let mut connection = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                match pool.get().await {
                    Ok(connection) => break connection,
                    Err(_) => sleep(Duration::from_millis(10)).await,
                }
            }
        })
        .await
        .expect("Redis must become reachable after container startup");
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let subscribers: Vec<(String, i64)> = deadpool_redis::redis::cmd("PUBSUB")
                    .arg("NUMSUB")
                    .arg(REDIS_CHANNEL)
                    .query_async(&mut *connection)
                    .await
                    .unwrap();
                if subscribers[0].1 == 1 {
                    break;
                }
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        let local = ResourceEvent::updated(
            "Patient",
            "transport-probe",
            serde_json::json!({"resourceType":"Patient","id":"transport-probe","active":false}),
        )
        .with_previous(
            serde_json::json!({"resourceType":"Patient","id":"transport-probe","active":true}),
        )
        .with_version(2);
        sync.publish(&local).await.unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(100), receiver.recv())
                .await
                .is_err(),
            "the originating replica must not process its own publication twice"
        );
        for malformed in ["unknown-type", "invalid-time"] {
            let mut invalid = SerializableEvent::from(&local);
            invalid.source_instance_id = Some("external-replica".into());
            if malformed == "unknown-type" {
                invalid.event_type = "unknown".into();
            } else {
                invalid.timestamp_subsec_nanos = 1_000_000_000;
            }
            let _: () = connection
                .publish(REDIS_CHANNEL, serde_json::to_string(&invalid).unwrap())
                .await
                .unwrap();
        }
        let stored = octofhir_storage::RawStoredResource {
            id: local.resource_id.clone(),
            version_id: "1".into(),
            resource_type: local.resource_type.clone(),
            resource_json: local.previous_resource.as_ref().unwrap().to_string(),
            last_updated: local.timestamp,
            created_at: local.timestamp,
        };
        let in_flight = cache.begin_fill(&stored.resource_type, &stored.id);
        cache.set(&stored, in_flight).await;
        assert!(cache.get(&stored.resource_type, &stored.id).await.is_some());
        let mut external = SerializableEvent::from(&local);
        external.source_instance_id = Some("external-replica".into());
        let _: () = connection
            .publish(REDIS_CHANNEL, serde_json::to_string(&external).unwrap())
            .await
            .unwrap();
        let received = tokio::time::timeout(Duration::from_secs(5), receiver.recv())
            .await
            .unwrap()
            .unwrap();
        let received = received.as_resource().unwrap();
        assert!(received.remote);
        cache.set(&stored, in_flight).await;
        assert!(
            cache.get(&stored.resource_type, &stored.id).await.is_none(),
            "remote invalidation must precede forwarding and reject old in-flight reads"
        );
        assert_eq!(received.timestamp, local.timestamp);
        assert_eq!(received.resource, local.resource);
        assert_eq!(received.previous_resource, local.previous_resource);
        let mut monitor = deadpool_redis::redis::Client::open(url.as_str())
            .unwrap()
            .get_async_pubsub()
            .await
            .unwrap();
        monitor.subscribe(REDIS_CHANNEL).await.unwrap();
        // Both publication APIs must suppress imported events. A healthy
        // subscriber would otherwise rebroadcast them into an unbounded loop.
        sync.publish(received).await.unwrap();
        super::super::RedisPublishHook::new(pool)
            .handle(received)
            .await
            .unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(100), monitor.on_message().next())
                .await
                .is_err(),
            "imported events must not be republished even when the origin listener skips self messages"
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(100), receiver.recv())
                .await
                .is_err()
        );
        subscriber.abort();
        assert!(subscriber.await.unwrap_err().is_cancelled());
    }
}
