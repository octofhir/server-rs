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

static INSTANCE_ID: std::sync::LazyLock<String> =
    std::sync::LazyLock::new(|| uuid::Uuid::new_v4().to_string());

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
        if event.remote {
            return Ok(());
        }
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
    #[serde(default)]
    pub source_instance_id: Option<String>,
    pub event_type: String,
    pub resource_type: String,
    pub resource_id: String,
    pub version_id: Option<i64>,
    pub resource: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_resource: Option<serde_json::Value>,
    pub timestamp: i64, // Unix timestamp
    #[serde(default)]
    pub timestamp_subsec_nanos: u32,
}

impl From<&ResourceEvent> for SerializableEvent {
    fn from(event: &ResourceEvent) -> Self {
        use octofhir_core::events::ResourceEventType;

        Self {
            source_instance_id: Some(INSTANCE_ID.clone()),
            event_type: match event.event_type {
                ResourceEventType::Created => "created".to_string(),
                ResourceEventType::Updated => "updated".to_string(),
                ResourceEventType::Deleted => "deleted".to_string(),
            },
            resource_type: event.resource_type.clone(),
            resource_id: event.resource_id.clone(),
            version_id: event.version_id,
            resource: event.resource.as_deref().cloned(),
            previous_resource: event.previous_resource.as_deref().cloned(),
            timestamp: event.timestamp.unix_timestamp(),
            timestamp_subsec_nanos: event.timestamp.nanosecond(),
        }
    }
}

impl SerializableEvent {
    pub fn into_resource_event(self) -> Result<ResourceEvent, RedisEventError> {
        use octofhir_core::events::ResourceEventType;

        let event_type = match self.event_type.as_str() {
            "created" => ResourceEventType::Created,
            "updated" => ResourceEventType::Updated,
            "deleted" => ResourceEventType::Deleted,
            _ => {
                return Err(RedisEventError::Message(
                    "Unknown resource event type".into(),
                ));
            }
        };

        let timestamp = time::OffsetDateTime::from_unix_timestamp(self.timestamp)
            .and_then(|timestamp| timestamp.replace_nanosecond(self.timestamp_subsec_nanos))
            .map_err(|error| {
                RedisEventError::Message(format!("Invalid event timestamp: {error}"))
            })?;

        Ok(ResourceEvent {
            event_type,
            resource_type: self.resource_type,
            resource_id: self.resource_id,
            version_id: self.version_id,
            resource: self.resource.map(std::sync::Arc::new),
            previous_resource: self.previous_resource.map(std::sync::Arc::new),
            remote: true,
            timestamp,
        })
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
            resource: Some(std::sync::Arc::new(serde_json::json!({"id": "123"}))),
            previous_resource: None,
            remote: false,
            timestamp: time::OffsetDateTime::now_utc(),
        };

        let serializable = SerializableEvent::from(&event);
        let json = serde_json::to_string(&serializable).unwrap();
        let deserialized: SerializableEvent = serde_json::from_str(&json).unwrap();
        let roundtrip = deserialized.into_resource_event().unwrap();

        assert_eq!(roundtrip.resource_type, "Patient");
        assert_eq!(roundtrip.resource_id, "123");
        assert_eq!(roundtrip.timestamp, event.timestamp);
        assert!(matches!(roundtrip.event_type, ResourceEventType::Created));
    }

    #[test]
    fn malformed_events_never_become_updates_or_receive_a_new_timestamp() {
        let event = ResourceEvent::updated("Patient", "123", serde_json::json!({"id":"123"}));
        let mut invalid = SerializableEvent::from(&event);
        invalid.event_type = "unknown".into();
        assert!(invalid.into_resource_event().is_err());
        let mut invalid = SerializableEvent::from(&event);
        invalid.timestamp_subsec_nanos = 1_000_000_000;
        assert!(invalid.into_resource_event().is_err());
        let mut invalid = SerializableEvent::from(&event);
        invalid.timestamp = i64::MAX;
        assert!(invalid.into_resource_event().is_err());
        let wire = serde_json::to_value(&event).unwrap();
        assert!(wire.get("remote").is_none());
    }
}
