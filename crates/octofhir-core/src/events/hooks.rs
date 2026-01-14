//! Hook traits for the unified resource event system.
//!
//! Hooks are asynchronous handlers that react to system events.
//! They are designed to be:
//! - **Async**: Non-blocking, run in separate tokio tasks
//! - **Isolated**: Errors in one hook don't affect others
//! - **Composable**: Multiple hooks can react to the same event

use async_trait::async_trait;
use std::sync::Arc;

use super::types::{AuthEvent, AuthEventType, ResourceEvent, ResourceEventType, SystemEvent};

/// Error type for hook operations.
#[derive(Debug, thiserror::Error)]
pub enum HookError {
    /// Hook execution failed with a message.
    #[error("Hook execution failed: {0}")]
    Execution(String),

    /// Hook failed to send to an internal channel.
    #[error("Channel send failed: {0}")]
    Channel(String),

    /// Hook failed due to a database error.
    #[error("Database error: {0}")]
    Database(String),

    /// Hook failed due to a network error.
    #[error("Network error: {0}")]
    Network(String),

    /// Hook failed due to serialization error.
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// Generic error with source.
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl HookError {
    /// Create an execution error from a string.
    pub fn execution(msg: impl Into<String>) -> Self {
        HookError::Execution(msg.into())
    }

    /// Create a channel error from a string.
    pub fn channel(msg: impl Into<String>) -> Self {
        HookError::Channel(msg.into())
    }

    /// Create a database error from a string.
    pub fn database(msg: impl Into<String>) -> Self {
        HookError::Database(msg.into())
    }
}

// ============================================================================
// Hook Traits
// ============================================================================

/// Trait for system event hooks.
///
/// This is the most general hook trait that can handle any system event.
/// For more specific hooks, use `ResourceHook` or `AuthHook`.
///
/// # Implementation Notes
///
/// - Hooks should be quick and non-blocking
/// - For heavy work, send to an internal channel and return immediately
/// - Errors are logged but don't propagate to the event source
/// - Hooks run in isolated tokio tasks with panic protection
#[async_trait]
pub trait SystemHook: Send + Sync {
    /// Unique name for this hook (for logging and metrics).
    fn name(&self) -> &str;

    /// Handle a system event.
    ///
    /// This method should be quick and non-blocking.
    /// For debouncing or heavy work, send to an internal channel.
    async fn handle(&self, event: &SystemEvent) -> Result<(), HookError>;

    /// Check if this hook should handle the given event.
    ///
    /// Default implementation returns true for all events.
    /// Override to filter events before handling.
    fn matches(&self, _event: &SystemEvent) -> bool {
        true
    }

    /// Called when the hook system starts.
    /// Use for initialization or startup tasks.
    async fn on_start(&self) -> Result<(), HookError> {
        Ok(())
    }

    /// Called when the hook system shuts down.
    async fn on_shutdown(&self) -> Result<(), HookError> {
        Ok(())
    }
}

/// Trait for FHIR resource event hooks.
///
/// This trait provides a more specific interface for handling resource events.
///
/// # Example
///
/// ```ignore
/// struct PolicyReloadHook {
///     notifier: Arc<PolicyChangeNotifier>,
/// }
///
/// #[async_trait]
/// impl ResourceHook for PolicyReloadHook {
///     fn name(&self) -> &str { "policy_reload" }
///     fn resource_types(&self) -> &[&str] { &["AccessPolicy"] }
///
///     async fn handle(&self, event: &ResourceEvent) -> Result<(), HookError> {
///         self.notifier.notify(PolicyChange::from(event));
///         Ok(())
///     }
/// }
/// ```
#[async_trait]
pub trait ResourceHook: Send + Sync {
    /// Unique name for this hook (for logging and metrics).
    fn name(&self) -> &str;

    /// Resource types this hook is interested in.
    ///
    /// Return an empty slice to match all resource types.
    fn resource_types(&self) -> &[&str];

    /// Event types this hook handles.
    ///
    /// Return an empty slice to match all event types (Created, Updated, Deleted).
    fn event_types(&self) -> &[ResourceEventType] {
        &[] // default: all event types
    }

    /// Handle a resource change event.
    ///
    /// This method should be quick and non-blocking.
    async fn handle(&self, event: &ResourceEvent) -> Result<(), HookError>;

    /// Called when the hook system starts.
    async fn on_start(&self) -> Result<(), HookError> {
        Ok(())
    }

    /// Called when the hook system shuts down.
    async fn on_shutdown(&self) -> Result<(), HookError> {
        Ok(())
    }

    /// Check if this hook should handle the given event.
    fn matches(&self, event: &ResourceEvent) -> bool {
        // Check resource type filter
        let types = self.resource_types();
        if !types.is_empty() && !types.contains(&event.resource_type.as_str()) {
            return false;
        }

        // Check event type filter
        let event_types = self.event_types();
        if !event_types.is_empty() && !event_types.contains(&event.event_type) {
            return false;
        }

        true
    }
}

/// Trait for authentication event hooks.
///
/// This trait provides a specific interface for handling auth events.
#[async_trait]
pub trait AuthHook: Send + Sync {
    /// Unique name for this hook (for logging and metrics).
    fn name(&self) -> &str;

    /// Event types this hook handles.
    ///
    /// Return an empty slice to match all auth event types.
    fn event_types(&self) -> &[AuthEventType] {
        &[] // default: all event types
    }

    /// Handle an auth event.
    ///
    /// This method should be quick and non-blocking.
    async fn handle(&self, event: &AuthEvent) -> Result<(), HookError>;

    /// Called when the hook system starts.
    async fn on_start(&self) -> Result<(), HookError> {
        Ok(())
    }

    /// Called when the hook system shuts down.
    async fn on_shutdown(&self) -> Result<(), HookError> {
        Ok(())
    }

    /// Check if this hook should handle the given event.
    fn matches(&self, event: &AuthEvent) -> bool {
        let event_types = self.event_types();
        event_types.is_empty() || event_types.contains(&event.event_type)
    }
}

// ============================================================================
// Wrapper implementations
// ============================================================================

/// Wrapper to adapt a ResourceHook to a SystemHook.
pub struct ResourceHookAdapter<H: ResourceHook>(pub Arc<H>);

#[async_trait]
impl<H: ResourceHook + 'static> SystemHook for ResourceHookAdapter<H> {
    fn name(&self) -> &str {
        self.0.name()
    }

    async fn handle(&self, event: &SystemEvent) -> Result<(), HookError> {
        if let SystemEvent::Resource(re) = event {
            self.0.handle(re).await
        } else {
            Ok(()) // Not a resource event, skip
        }
    }

    fn matches(&self, event: &SystemEvent) -> bool {
        match event {
            SystemEvent::Resource(re) => self.0.matches(re),
            _ => false,
        }
    }

    async fn on_start(&self) -> Result<(), HookError> {
        self.0.on_start().await
    }

    async fn on_shutdown(&self) -> Result<(), HookError> {
        self.0.on_shutdown().await
    }
}

/// Wrapper to adapt an AuthHook to a SystemHook.
pub struct AuthHookAdapter<H: AuthHook>(pub Arc<H>);

#[async_trait]
impl<H: AuthHook + 'static> SystemHook for AuthHookAdapter<H> {
    fn name(&self) -> &str {
        self.0.name()
    }

    async fn handle(&self, event: &SystemEvent) -> Result<(), HookError> {
        if let SystemEvent::Auth(ae) = event {
            self.0.handle(ae).await
        } else {
            Ok(()) // Not an auth event, skip
        }
    }

    fn matches(&self, event: &SystemEvent) -> bool {
        match event {
            SystemEvent::Auth(ae) => self.0.matches(ae),
            _ => false,
        }
    }

    async fn on_start(&self) -> Result<(), HookError> {
        self.0.on_start().await
    }

    async fn on_shutdown(&self) -> Result<(), HookError> {
        self.0.on_shutdown().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestResourceHook {
        name: &'static str,
        resource_types: Vec<&'static str>,
    }

    #[async_trait]
    impl ResourceHook for TestResourceHook {
        fn name(&self) -> &str {
            self.name
        }

        fn resource_types(&self) -> &[&str] {
            &self.resource_types
        }

        async fn handle(&self, _event: &ResourceEvent) -> Result<(), HookError> {
            Ok(())
        }
    }

    #[test]
    fn test_resource_hook_matches_type() {
        let hook = TestResourceHook {
            name: "test",
            resource_types: vec!["Patient", "Observation"],
        };

        let patient_event = ResourceEvent::created("Patient", "1", serde_json::json!({}));
        let observation_event = ResourceEvent::created("Observation", "2", serde_json::json!({}));
        let encounter_event = ResourceEvent::created("Encounter", "3", serde_json::json!({}));

        assert!(hook.matches(&patient_event));
        assert!(hook.matches(&observation_event));
        assert!(!hook.matches(&encounter_event));
    }

    #[test]
    fn test_resource_hook_matches_all() {
        let hook = TestResourceHook {
            name: "test",
            resource_types: vec![], // Empty = match all
        };

        let patient_event = ResourceEvent::created("Patient", "1", serde_json::json!({}));
        let encounter_event = ResourceEvent::created("Encounter", "3", serde_json::json!({}));

        assert!(hook.matches(&patient_event));
        assert!(hook.matches(&encounter_event));
    }

    #[test]
    fn test_hook_error_display() {
        let err = HookError::execution("something went wrong");
        assert_eq!(
            err.to_string(),
            "Hook execution failed: something went wrong"
        );

        let err = HookError::database("connection failed");
        assert_eq!(err.to_string(), "Database error: connection failed");
    }
}
