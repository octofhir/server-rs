//! GraphQL subscription hook.
//!
//! This hook forwards resource events to the GraphQL subscription broadcaster,
//! enabling real-time updates to GraphQL subscription clients.

use std::sync::Arc;

use async_trait::async_trait;
use octofhir_core::events::{HookError, ResourceEvent, ResourceEventType, ResourceHook};
use octofhir_graphql::subscriptions::{
    ResourceChangeEvent, ResourceEventBroadcaster, ResourceEventType as GraphQLEventType,
};
use tracing::debug;

/// Hook that forwards resource events to GraphQL subscription broadcaster.
///
/// When a resource is created, updated, or deleted, this hook:
/// 1. Converts the core event to GraphQL subscription format
/// 2. Sends it to the GraphQL broadcaster
/// 3. Connected WebSocket/SSE clients receive the update
///
/// # Example
///
/// ```ignore
/// let hook = GraphQLSubscriptionHook::new(graphql_broadcaster.clone());
/// registry.register(Arc::new(hook));
/// ```
pub struct GraphQLSubscriptionHook {
    broadcaster: Arc<ResourceEventBroadcaster>,
}

impl GraphQLSubscriptionHook {
    /// Create a new GraphQL subscription hook.
    ///
    /// # Arguments
    ///
    /// * `broadcaster` - The GraphQL subscription broadcaster
    pub fn new(broadcaster: Arc<ResourceEventBroadcaster>) -> Self {
        Self { broadcaster }
    }
}

#[async_trait]
impl ResourceHook for GraphQLSubscriptionHook {
    fn name(&self) -> &str {
        "graphql_subscription"
    }

    fn resource_types(&self) -> &[&str] {
        // Subscribe to all resource types for GraphQL subscriptions
        &[]
    }

    async fn handle(&self, event: &ResourceEvent) -> Result<(), HookError> {
        // Convert core event type to GraphQL event type
        let graphql_event_type = match event.event_type {
            ResourceEventType::Created => GraphQLEventType::Created,
            ResourceEventType::Updated => GraphQLEventType::Updated,
            ResourceEventType::Deleted => GraphQLEventType::Deleted,
        };

        // Create GraphQL subscription event
        let graphql_event = ResourceChangeEvent::new(
            graphql_event_type,
            &event.resource_type,
            &event.resource_id,
            event.resource.clone(),
        );

        // Send to broadcaster
        let count = self.broadcaster.send(graphql_event);

        debug!(
            resource_type = %event.resource_type,
            resource_id = %event.resource_id,
            event_type = %event.event_type,
            subscriber_count = count,
            "Forwarded event to GraphQL subscription broadcaster"
        );

        Ok(())
    }
}

impl std::fmt::Debug for GraphQLSubscriptionHook {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GraphQLSubscriptionHook")
            .field("subscriber_count", &self.broadcaster.subscriber_count())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use octofhir_core::events::ResourceEvent;
    use serde_json::json;

    #[test]
    fn test_resource_type_matching() {
        // GraphQL subscription hook should match all resource types
        let broadcaster = ResourceEventBroadcaster::new_shared();
        let hook = GraphQLSubscriptionHook::new(broadcaster);

        assert!(hook.resource_types().is_empty());
    }

    #[tokio::test]
    async fn test_event_forwarding() {
        let broadcaster = ResourceEventBroadcaster::new_shared();
        let mut receiver = broadcaster.subscribe();

        let hook = GraphQLSubscriptionHook::new(broadcaster);

        // Create a test event
        let event = ResourceEvent::created("Patient", "test-123", json!({"id": "test-123"}));

        // Handle the event
        hook.handle(&event).await.unwrap();

        // Verify the event was forwarded
        let received = receiver.recv().await.unwrap();
        assert_eq!(received.resource_type, "Patient");
        assert_eq!(received.resource_id, "test-123");
    }
}
