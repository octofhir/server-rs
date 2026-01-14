//! Event synchronization for multi-instance deployments.
//!
//! This module provides infrastructure for synchronizing resource events
//! across multiple server instances using Redis pub/sub.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────┐     ┌─────────────────────┐
//! │  Server Instance 1  │     │  Server Instance 2  │
//! │                     │     │                     │
//! │  EventBroadcaster   │     │  EventBroadcaster   │
//! │        │            │     │        ▲            │
//! │        ▼            │     │        │            │
//! │  RedisPublishHook ──┼────►│  RedisEventSync ────┘
//! │                     │     │                     │
//! └─────────────────────┘     └─────────────────────┘
//!              │                      ▲
//!              └──────► Redis ────────┘
//!                    (pub/sub)
//! ```
//!
//! # Usage
//!
//! ```ignore
//! // Create Redis event sync
//! let redis_sync = RedisEventSyncBuilder::new()
//!     .with_pool(redis_pool)
//!     .with_broadcaster(event_broadcaster)
//!     .with_redis_url(redis_url)
//!     .start()?;
//!
//! // Create publish hook
//! let publish_hook = RedisPublishHook::new(redis_pool);
//! hook_registry.register(Arc::new(publish_hook));
//! ```

mod redis;

pub use redis::{RedisEventError, RedisEventSync, RedisEventSyncBuilder};

use async_trait::async_trait;
use deadpool_redis::{Pool, redis::AsyncCommands};
use octofhir_core::events::{HookError, ResourceEvent, ResourceHook};
use tracing::{debug, warn};

/// Redis channel for resource events.
pub const REDIS_CHANNEL: &str = "octofhir:resource_events";

/// Hook that publishes resource events to Redis for cross-instance synchronization.
///
/// This hook should be registered when Redis is enabled to ensure that
/// resource changes are propagated to all server instances.
///
/// # Example
///
/// ```ignore
/// let publish_hook = RedisPublishHook::new(redis_pool.clone());
/// hook_registry.register(Arc::new(publish_hook));
/// ```
pub struct RedisPublishHook {
    pool: Pool,
}

impl RedisPublishHook {
    /// Create a new Redis publish hook.
    pub fn new(pool: Pool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl ResourceHook for RedisPublishHook {
    fn name(&self) -> &str {
        "redis_publish"
    }

    fn resource_types(&self) -> &[&str] {
        &[] // All resource types
    }

    async fn handle(&self, event: &ResourceEvent) -> Result<(), HookError> {
        let mut conn = match self.pool.get().await {
            Ok(conn) => conn,
            Err(e) => {
                warn!(error = %e, "Failed to get Redis connection for event publish");
                return Err(HookError::Execution(format!("Redis pool error: {}", e)));
            }
        };

        // Serialize event
        let message = match serde_json::to_string(&SerializableEvent::from(event)) {
            Ok(msg) => msg,
            Err(e) => {
                warn!(error = %e, "Failed to serialize event for Redis");
                return Err(HookError::Execution(format!("Serialization error: {}", e)));
            }
        };

        // Publish to Redis
        let result: Result<(), _> = conn.publish(REDIS_CHANNEL, &message).await;
        if let Err(e) = result {
            warn!(error = %e, "Failed to publish event to Redis");
            return Err(HookError::Execution(format!("Redis publish error: {}", e)));
        }

        debug!(
            resource_type = %event.resource_type,
            resource_id = %event.resource_id,
            "Published event to Redis"
        );

        Ok(())
    }
}

/// Serializable event format for Redis transmission.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct SerializableEvent {
    pub event_type: String,
    pub resource_type: String,
    pub resource_id: String,
    pub version_id: Option<i64>,
    pub resource: Option<serde_json::Value>,
    pub timestamp: i64, // Unix timestamp
}

impl From<&ResourceEvent> for SerializableEvent {
    fn from(event: &ResourceEvent) -> Self {
        use octofhir_core::events::ResourceEventType;

        Self {
            event_type: match event.event_type {
                ResourceEventType::Created => "created".to_string(),
                ResourceEventType::Updated => "updated".to_string(),
                ResourceEventType::Deleted => "deleted".to_string(),
            },
            resource_type: event.resource_type.clone(),
            resource_id: event.resource_id.clone(),
            version_id: event.version_id,
            resource: event.resource.clone(),
            timestamp: event.timestamp.unix_timestamp(),
        }
    }
}

impl SerializableEvent {
    pub fn into_resource_event(self) -> ResourceEvent {
        use octofhir_core::events::ResourceEventType;

        let event_type = match self.event_type.as_str() {
            "created" => ResourceEventType::Created,
            "updated" => ResourceEventType::Updated,
            "deleted" => ResourceEventType::Deleted,
            _ => ResourceEventType::Updated, // Default fallback
        };

        let timestamp = time::OffsetDateTime::from_unix_timestamp(self.timestamp)
            .unwrap_or_else(|_| time::OffsetDateTime::now_utc());

        ResourceEvent {
            event_type,
            resource_type: self.resource_type,
            resource_id: self.resource_id,
            version_id: self.version_id,
            resource: self.resource,
            timestamp,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use octofhir_core::events::ResourceEventType;

    #[test]
    fn test_serializable_event_roundtrip() {
        let event = ResourceEvent {
            event_type: ResourceEventType::Created,
            resource_type: "Patient".to_string(),
            resource_id: "123".to_string(),
            version_id: Some(1),
            resource: Some(serde_json::json!({"id": "123"})),
            timestamp: time::OffsetDateTime::now_utc(),
        };

        let serializable = SerializableEvent::from(&event);
        let json = serde_json::to_string(&serializable).unwrap();
        let deserialized: SerializableEvent = serde_json::from_str(&json).unwrap();
        let roundtrip = deserialized.into_resource_event();

        assert_eq!(roundtrip.resource_type, "Patient");
        assert_eq!(roundtrip.resource_id, "123");
        assert!(matches!(roundtrip.event_type, ResourceEventType::Created));
    }
}
