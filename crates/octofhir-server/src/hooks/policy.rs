//! Policy reload hook.
//!
//! This hook triggers policy cache reload when AccessPolicy resources are changed.
//! It replaces the PostgreSQL LISTEN/NOTIFY based PolicyListener.

use std::sync::Arc;

use async_trait::async_trait;
use octofhir_auth::policy::reload::{PolicyChange, PolicyChangeNotifier};
use octofhir_core::events::{HookError, ResourceEvent, ResourceEventType, ResourceHook};
use tracing::debug;

/// Hook that triggers policy cache reload on AccessPolicy changes.
///
/// When an AccessPolicy resource is created, updated, or deleted, this hook
/// sends a notification to the [`PolicyChangeNotifier`], which triggers the
/// [`PolicyReloadService`] to refresh the policy cache.
///
/// # Example
///
/// ```ignore
/// let notifier = Arc::new(PolicyChangeNotifier::new(64));
/// let hook = PolicyReloadHook::new(notifier.clone());
/// registry.register(Arc::new(hook));
/// ```
pub struct PolicyReloadHook {
    notifier: Arc<PolicyChangeNotifier>,
}

impl PolicyReloadHook {
    /// Create a new policy reload hook.
    ///
    /// # Arguments
    ///
    /// * `notifier` - The policy change notifier to send events to
    pub fn new(notifier: Arc<PolicyChangeNotifier>) -> Self {
        Self { notifier }
    }
}

#[async_trait]
impl ResourceHook for PolicyReloadHook {
    fn name(&self) -> &str {
        "policy_reload"
    }

    fn resource_types(&self) -> &[&str] {
        &["AccessPolicy"]
    }

    async fn handle(&self, event: &ResourceEvent) -> Result<(), HookError> {
        let policy_id = event.resource_id.clone();

        let change = match event.event_type {
            ResourceEventType::Created => PolicyChange::Created { policy_id },
            ResourceEventType::Updated => PolicyChange::Updated { policy_id },
            ResourceEventType::Deleted => PolicyChange::Deleted { policy_id },
        };

        debug!(
            policy_id = %event.resource_id,
            event_type = %event.event_type,
            "PolicyReloadHook: notifying policy change"
        );

        self.notifier.notify(change);
        Ok(())
    }
}

impl std::fmt::Debug for PolicyReloadHook {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PolicyReloadHook")
            .field("subscribers", &self.notifier.subscriber_count())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn test_policy_hook_created() {
        let notifier = Arc::new(PolicyChangeNotifier::new(16));
        let mut receiver = notifier.subscribe();
        let hook = PolicyReloadHook::new(notifier.clone());

        // Check it matches AccessPolicy
        assert!(hook.matches(&ResourceEvent::created(
            "AccessPolicy",
            "test-id",
            json!({})
        )));

        // Check it doesn't match other types
        assert!(!hook.matches(&ResourceEvent::created("Patient", "test-id", json!({}))));

        // Handle event
        let event = ResourceEvent::created("AccessPolicy", "policy-123", json!({"id": "policy-123"}));
        hook.handle(&event).await.unwrap();

        // Check notification was sent
        let change = receiver.recv().await.unwrap();
        assert!(matches!(
            change,
            PolicyChange::Created { policy_id } if policy_id == "policy-123"
        ));
    }

    #[tokio::test]
    async fn test_policy_hook_updated() {
        let notifier = Arc::new(PolicyChangeNotifier::new(16));
        let mut receiver = notifier.subscribe();
        let hook = PolicyReloadHook::new(notifier.clone());

        let event = ResourceEvent::updated("AccessPolicy", "policy-456", json!({}));
        hook.handle(&event).await.unwrap();

        let change = receiver.recv().await.unwrap();
        assert!(matches!(
            change,
            PolicyChange::Updated { policy_id } if policy_id == "policy-456"
        ));
    }

    #[tokio::test]
    async fn test_policy_hook_deleted() {
        let notifier = Arc::new(PolicyChangeNotifier::new(16));
        let mut receiver = notifier.subscribe();
        let hook = PolicyReloadHook::new(notifier.clone());

        let event = ResourceEvent::deleted("AccessPolicy", "policy-789");
        hook.handle(&event).await.unwrap();

        let change = receiver.recv().await.unwrap();
        assert!(matches!(
            change,
            PolicyChange::Deleted { policy_id } if policy_id == "policy-789"
        ));
    }
}
