//! Event types and broadcasting for FHIR resource subscriptions.

use std::sync::Arc;
use tokio::sync::broadcast;

/// Maximum number of events to buffer in the broadcast channel.
const EVENT_BUFFER_SIZE: usize = 1024;

/// Type of resource change event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceEventType {
    /// Resource was created
    Created,
    /// Resource was updated
    Updated,
    /// Resource was deleted
    Deleted,
}

impl ResourceEventType {
    /// Returns the string representation of the event type.
    pub fn as_str(&self) -> &'static str {
        match self {
            ResourceEventType::Created => "created",
            ResourceEventType::Updated => "updated",
            ResourceEventType::Deleted => "deleted",
        }
    }
}

impl std::fmt::Display for ResourceEventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Event representing a change to a FHIR resource.
#[derive(Debug, Clone)]
pub struct ResourceChangeEvent {
    /// Type of change (created, updated, deleted)
    pub event_type: ResourceEventType,
    /// FHIR resource type (e.g., "Patient", "Observation")
    pub resource_type: String,
    /// Resource ID
    pub resource_id: String,
    /// The resource data as JSON (None for deletions).
    /// Wrapped in Arc to avoid deep clones through the subscription pipeline.
    pub resource: Option<Arc<serde_json::Value>>,
    /// Timestamp of the event
    pub timestamp: time::OffsetDateTime,
}

impl ResourceChangeEvent {
    /// Create a new resource change event.
    pub fn new(
        event_type: ResourceEventType,
        resource_type: impl Into<String>,
        resource_id: impl Into<String>,
        resource: Option<Arc<serde_json::Value>>,
    ) -> Self {
        Self {
            event_type,
            resource_type: resource_type.into(),
            resource_id: resource_id.into(),
            resource,
            timestamp: time::OffsetDateTime::now_utc(),
        }
    }

    /// Create a "created" event.
    pub fn created(
        resource_type: impl Into<String>,
        resource_id: impl Into<String>,
        resource: serde_json::Value,
    ) -> Self {
        Self::new(
            ResourceEventType::Created,
            resource_type,
            resource_id,
            Some(Arc::new(resource)),
        )
    }

    /// Create an "updated" event.
    pub fn updated(
        resource_type: impl Into<String>,
        resource_id: impl Into<String>,
        resource: serde_json::Value,
    ) -> Self {
        Self::new(
            ResourceEventType::Updated,
            resource_type,
            resource_id,
            Some(Arc::new(resource)),
        )
    }

    /// Create a "deleted" event.
    pub fn deleted(resource_type: impl Into<String>, resource_id: impl Into<String>) -> Self {
        Self::new(ResourceEventType::Deleted, resource_type, resource_id, None)
    }

    /// Check if this event matches a filter by resource type.
    pub fn matches_type(&self, filter_type: Option<&str>) -> bool {
        match filter_type {
            Some(t) => self.resource_type == t,
            None => true, // No filter means match all
        }
    }

    /// Check if this event matches a filter by event type.
    pub fn matches_event_type(&self, filter: Option<ResourceEventType>) -> bool {
        match filter {
            Some(t) => self.event_type == t,
            None => true, // No filter means match all
        }
    }
}

/// Broadcaster for resource change events.
///
/// This is a thread-safe broadcaster that can be shared across the application.
/// Multiple subscribers can receive events from a single sender.
#[derive(Clone)]
pub struct ResourceEventBroadcaster {
    sender: broadcast::Sender<ResourceChangeEvent>,
}

impl ResourceEventBroadcaster {
    /// Create a new broadcaster.
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(EVENT_BUFFER_SIZE);
        Self { sender }
    }

    /// Create a new broadcaster wrapped in an Arc for sharing.
    pub fn new_shared() -> Arc<Self> {
        Arc::new(Self::new())
    }

    /// Send an event to all subscribers.
    ///
    /// Returns the number of subscribers that received the event.
    /// Returns 0 if there are no active subscribers.
    pub fn send(&self, event: ResourceChangeEvent) -> usize {
        match self.sender.send(event) {
            Ok(count) => count,
            Err(_) => 0, // No active receivers
        }
    }

    /// Send a "created" event.
    pub fn send_created(
        &self,
        resource_type: impl Into<String>,
        resource_id: impl Into<String>,
        resource: serde_json::Value,
    ) -> usize {
        self.send(ResourceChangeEvent::new(
            ResourceEventType::Created,
            resource_type,
            resource_id,
            Some(Arc::new(resource)),
        ))
    }

    /// Send an "updated" event.
    pub fn send_updated(
        &self,
        resource_type: impl Into<String>,
        resource_id: impl Into<String>,
        resource: serde_json::Value,
    ) -> usize {
        self.send(ResourceChangeEvent::new(
            ResourceEventType::Updated,
            resource_type,
            resource_id,
            Some(Arc::new(resource)),
        ))
    }

    /// Send a "deleted" event.
    pub fn send_deleted(
        &self,
        resource_type: impl Into<String>,
        resource_id: impl Into<String>,
    ) -> usize {
        self.send(ResourceChangeEvent::deleted(resource_type, resource_id))
    }

    /// Subscribe to events.
    ///
    /// Returns a receiver that will receive all events broadcast after subscription.
    pub fn subscribe(&self) -> broadcast::Receiver<ResourceChangeEvent> {
        self.sender.subscribe()
    }

    /// Get the number of active subscribers.
    pub fn subscriber_count(&self) -> usize {
        self.sender.receiver_count()
    }
}

impl Default for ResourceEventBroadcaster {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for ResourceEventBroadcaster {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResourceEventBroadcaster")
            .field("subscriber_count", &self.subscriber_count())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_type_display() {
        assert_eq!(ResourceEventType::Created.to_string(), "created");
        assert_eq!(ResourceEventType::Updated.to_string(), "updated");
        assert_eq!(ResourceEventType::Deleted.to_string(), "deleted");
    }

    #[test]
    fn test_event_matches_type() {
        let event = ResourceChangeEvent::created("Patient", "123", serde_json::json!({}));

        assert!(event.matches_type(Some("Patient")));
        assert!(!event.matches_type(Some("Observation")));
        assert!(event.matches_type(None));
    }

    #[test]
    fn test_event_matches_event_type() {
        let event = ResourceChangeEvent::created("Patient", "123", serde_json::json!({}));

        assert!(event.matches_event_type(Some(ResourceEventType::Created)));
        assert!(!event.matches_event_type(Some(ResourceEventType::Updated)));
        assert!(event.matches_event_type(None));
    }

    #[tokio::test]
    async fn test_broadcaster_send_receive() {
        let broadcaster = ResourceEventBroadcaster::new();
        let mut receiver = broadcaster.subscribe();

        let event =
            ResourceChangeEvent::created("Patient", "123", serde_json::json!({"id": "123"}));
        broadcaster.send(event.clone());

        let received = receiver.recv().await.unwrap();
        assert_eq!(received.resource_type, "Patient");
        assert_eq!(received.resource_id, "123");
    }

    #[test]
    fn test_broadcaster_no_subscribers() {
        let broadcaster = ResourceEventBroadcaster::new();
        let count = broadcaster.send_created("Patient", "123", serde_json::json!({}));
        assert_eq!(count, 0);
    }
}
