//! Event broadcaster for the unified resource event system.
//!
//! The `EventBroadcaster` is the central event bus that all modules use to
//! publish and subscribe to events. It uses tokio's broadcast channel for
//! efficient multi-producer, multi-consumer messaging.

use std::sync::Arc;
use tokio::sync::broadcast;

use super::types::{AuthEvent, ResourceEvent, SystemEvent};

/// Default buffer size for the broadcast channel.
/// Events beyond this limit will cause older events to be dropped for slow receivers.
const DEFAULT_BUFFER_SIZE: usize = 1024;

/// Broadcaster for system events.
///
/// This is a thread-safe broadcaster that can be cloned and shared across the application.
/// Multiple subscribers can receive events from a single sender.
///
/// # Example
///
/// ```
/// use octofhir_core::events::{EventBroadcaster, ResourceEvent};
///
/// let broadcaster = EventBroadcaster::new();
/// let mut receiver = broadcaster.subscribe();
///
/// // Send an event
/// broadcaster.send_resource(ResourceEvent::created("Patient", "123", serde_json::json!({})));
///
/// // Receive in another task
/// // let event = receiver.recv().await.unwrap();
/// ```
#[derive(Clone)]
pub struct EventBroadcaster {
    sender: broadcast::Sender<SystemEvent>,
}

impl EventBroadcaster {
    /// Create a new broadcaster with default buffer size.
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_BUFFER_SIZE)
    }

    /// Create a new broadcaster with custom buffer size.
    pub fn with_capacity(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self { sender }
    }

    /// Create a new broadcaster wrapped in an Arc for sharing.
    pub fn new_shared() -> Arc<Self> {
        Arc::new(Self::new())
    }

    /// Send a system event to all subscribers.
    ///
    /// Returns the number of subscribers that received the event.
    /// Returns 0 if there are no active subscribers.
    pub fn send(&self, event: SystemEvent) -> usize {
        self.sender.send(event).unwrap_or_default()
    }

    /// Send a resource event to all subscribers.
    ///
    /// Convenience method that wraps the event in `SystemEvent::Resource`.
    pub fn send_resource(&self, event: ResourceEvent) -> usize {
        self.send(SystemEvent::Resource(event))
    }

    /// Send an auth event to all subscribers.
    ///
    /// Convenience method that wraps the event in `SystemEvent::Auth`.
    pub fn send_auth(&self, event: AuthEvent) -> usize {
        self.send(SystemEvent::Auth(event))
    }

    /// Send a "resource created" event.
    pub fn send_created(
        &self,
        resource_type: impl Into<String>,
        resource_id: impl Into<String>,
        resource: serde_json::Value,
    ) -> usize {
        self.send_resource(ResourceEvent::created(resource_type, resource_id, resource))
    }

    /// Send a "resource updated" event.
    pub fn send_updated(
        &self,
        resource_type: impl Into<String>,
        resource_id: impl Into<String>,
        resource: serde_json::Value,
    ) -> usize {
        self.send_resource(ResourceEvent::updated(resource_type, resource_id, resource))
    }

    /// Send a "resource deleted" event.
    pub fn send_deleted(
        &self,
        resource_type: impl Into<String>,
        resource_id: impl Into<String>,
    ) -> usize {
        self.send_resource(ResourceEvent::deleted(resource_type, resource_id))
    }

    /// Subscribe to events.
    ///
    /// Returns a receiver that will receive all events broadcast after subscription.
    /// Note: Events sent before subscription are not received.
    pub fn subscribe(&self) -> broadcast::Receiver<SystemEvent> {
        self.sender.subscribe()
    }

    /// Get the number of active subscribers.
    pub fn subscriber_count(&self) -> usize {
        self.sender.receiver_count()
    }

    /// Check if there are any active subscribers.
    pub fn has_subscribers(&self) -> bool {
        self.sender.receiver_count() > 0
    }
}

impl Default for EventBroadcaster {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for EventBroadcaster {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EventBroadcaster")
            .field("subscriber_count", &self.subscriber_count())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_broadcaster_creation() {
        let broadcaster = EventBroadcaster::new();
        assert_eq!(broadcaster.subscriber_count(), 0);
        assert!(!broadcaster.has_subscribers());
    }

    #[test]
    fn test_broadcaster_subscribe() {
        let broadcaster = EventBroadcaster::new();
        let _receiver = broadcaster.subscribe();
        assert_eq!(broadcaster.subscriber_count(), 1);
        assert!(broadcaster.has_subscribers());
    }

    #[test]
    fn test_broadcaster_no_subscribers() {
        let broadcaster = EventBroadcaster::new();
        let count = broadcaster.send_created("Patient", "123", serde_json::json!({}));
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn test_broadcaster_send_receive() {
        let broadcaster = EventBroadcaster::new();
        let mut receiver = broadcaster.subscribe();

        broadcaster.send_created("Patient", "123", serde_json::json!({"id": "123"}));

        let event = receiver.recv().await.unwrap();
        if let SystemEvent::Resource(re) = event {
            assert_eq!(re.resource_type, "Patient");
            assert_eq!(re.resource_id, "123");
        } else {
            panic!("Expected ResourceEvent");
        }
    }

    #[tokio::test]
    async fn test_broadcaster_multiple_subscribers() {
        let broadcaster = EventBroadcaster::new();
        let mut receiver1 = broadcaster.subscribe();
        let mut receiver2 = broadcaster.subscribe();

        assert_eq!(broadcaster.subscriber_count(), 2);

        let count = broadcaster.send_created("Patient", "123", serde_json::json!({}));
        assert_eq!(count, 2);

        let event1 = receiver1.recv().await.unwrap();
        let event2 = receiver2.recv().await.unwrap();

        assert!(matches!(event1, SystemEvent::Resource(_)));
        assert!(matches!(event2, SystemEvent::Resource(_)));
    }

    #[tokio::test]
    async fn test_broadcaster_auth_event() {
        let broadcaster = EventBroadcaster::new();
        let mut receiver = broadcaster.subscribe();

        broadcaster.send_auth(AuthEvent::login_succeeded("user-1", "client-1"));

        let event = receiver.recv().await.unwrap();
        if let SystemEvent::Auth(ae) = event {
            assert_eq!(ae.user_id, Some("user-1".to_string()));
            assert_eq!(ae.client_id, "client-1");
        } else {
            panic!("Expected AuthEvent");
        }
    }

    #[test]
    fn test_broadcaster_shared() {
        let broadcaster = EventBroadcaster::new_shared();
        let broadcaster2 = broadcaster.clone();

        let _receiver = broadcaster.subscribe();
        assert_eq!(broadcaster2.subscriber_count(), 1);
    }
}
