//! Hook registry and dispatcher for the unified resource event system.
//!
//! The registry manages hook registration and the dispatcher handles
//! event routing to hooks with proper isolation and error handling.

use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use std::time::Duration;

use futures_util::FutureExt;
use tokio::sync::{RwLock, broadcast};
use tracing::{debug, error, info, warn};

use super::hooks::{
    AuthHook, AuthHookAdapter, HookError, ResourceHook, ResourceHookAdapter, SystemHook,
};
use super::types::SystemEvent;

/// Default timeout for hook execution.
const DEFAULT_HOOK_TIMEOUT: Duration = Duration::from_secs(30);

// ============================================================================
// Hook Registry
// ============================================================================

/// Registry for system hooks with lifecycle management.
///
/// The registry maintains a list of registered hooks and provides
/// methods for dispatching events and managing hook lifecycles.
pub struct HookRegistry {
    /// All registered hooks.
    hooks: RwLock<Vec<Arc<dyn SystemHook>>>,
    /// Hook execution timeout.
    timeout: Duration,
}

impl HookRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self {
            hooks: RwLock::new(Vec::new()),
            timeout: DEFAULT_HOOK_TIMEOUT,
        }
    }

    /// Create a new registry with custom timeout.
    pub fn with_timeout(timeout: Duration) -> Self {
        Self {
            hooks: RwLock::new(Vec::new()),
            timeout,
        }
    }

    /// Register a system hook.
    pub async fn register(&self, hook: Arc<dyn SystemHook>) {
        let name = hook.name().to_string();
        self.hooks.write().await.push(hook);
        debug!(hook = %name, "Registered system hook");
    }

    /// Register a resource hook.
    ///
    /// The hook is wrapped in an adapter to convert it to a SystemHook.
    pub async fn register_resource<H: ResourceHook + 'static>(&self, hook: Arc<H>) {
        let name = hook.name().to_string();
        let adapter = Arc::new(ResourceHookAdapter(hook));
        self.hooks.write().await.push(adapter);
        debug!(hook = %name, "Registered resource hook");
    }

    /// Register an auth hook.
    ///
    /// The hook is wrapped in an adapter to convert it to a SystemHook.
    pub async fn register_auth<H: AuthHook + 'static>(&self, hook: Arc<H>) {
        let name = hook.name().to_string();
        let adapter = Arc::new(AuthHookAdapter(hook));
        self.hooks.write().await.push(adapter);
        debug!(hook = %name, "Registered auth hook");
    }

    /// Get the number of registered hooks.
    pub async fn hook_count(&self) -> usize {
        self.hooks.read().await.len()
    }

    /// Get hooks that match an event.
    pub async fn get_matching_hooks(&self, event: &SystemEvent) -> Vec<Arc<dyn SystemHook>> {
        let hooks = self.hooks.read().await;
        hooks.iter().filter(|h| h.matches(event)).cloned().collect()
    }

    /// Dispatch an event to all matching hooks.
    ///
    /// Each hook runs in an isolated tokio task with:
    /// - Timeout protection
    /// - Panic recovery
    /// - Error isolation (errors don't propagate)
    pub async fn dispatch(&self, event: &SystemEvent) {
        let hooks = self.get_matching_hooks(event).await;

        if hooks.is_empty() {
            debug!("No hooks matched event");
            return;
        }

        let timeout = self.timeout;

        for hook in hooks {
            let hook = hook.clone();
            let event = event.clone();

            // Each hook runs in an isolated task
            tokio::spawn(async move {
                let hook_name = hook.name().to_string();

                // Wrap in timeout
                let result = tokio::time::timeout(timeout, async {
                    // Wrap in catch_unwind for panic protection
                    AssertUnwindSafe(hook.handle(&event)).catch_unwind().await
                })
                .await;

                match result {
                    Ok(Ok(Ok(()))) => {
                        // Success
                        debug!(hook = %hook_name, "Hook executed successfully");
                    }
                    Ok(Ok(Err(e))) => {
                        // Hook returned an error
                        warn!(
                            hook = %hook_name,
                            error = %e,
                            "Hook execution failed"
                        );
                    }
                    Ok(Err(panic)) => {
                        // Hook panicked
                        let panic_msg = if let Some(s) = panic.downcast_ref::<&str>() {
                            s.to_string()
                        } else if let Some(s) = panic.downcast_ref::<String>() {
                            s.clone()
                        } else {
                            "Unknown panic".to_string()
                        };
                        error!(
                            hook = %hook_name,
                            panic = %panic_msg,
                            "Hook panicked!"
                        );
                    }
                    Err(_) => {
                        // Timeout
                        error!(
                            hook = %hook_name,
                            timeout_secs = timeout.as_secs(),
                            "Hook timed out"
                        );
                    }
                }
            });
        }
    }

    /// Call on_start for all hooks.
    pub async fn on_start(&self) -> Result<(), HookError> {
        let hooks = self.hooks.read().await;
        for hook in hooks.iter() {
            let hook_name = hook.name().to_string();
            if let Err(e) = hook.on_start().await {
                warn!(hook = %hook_name, error = %e, "Hook on_start failed");
            }
        }
        Ok(())
    }

    /// Call on_shutdown for all hooks.
    pub async fn on_shutdown(&self) -> Result<(), HookError> {
        let hooks = self.hooks.read().await;
        for hook in hooks.iter() {
            let hook_name = hook.name().to_string();
            if let Err(e) = hook.on_shutdown().await {
                warn!(hook = %hook_name, error = %e, "Hook on_shutdown failed");
            }
        }
        Ok(())
    }
}

impl Default for HookRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for HookRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HookRegistry")
            .field("timeout", &self.timeout)
            .finish_non_exhaustive()
    }
}

// ============================================================================
// Hook Dispatcher
// ============================================================================

/// Dispatcher that consumes events from a broadcast channel and routes them to hooks.
///
/// The dispatcher runs as a background task and forwards events to the registry.
pub struct HookDispatcher {
    registry: Arc<HookRegistry>,
}

impl HookDispatcher {
    /// Create a new dispatcher.
    pub fn new(registry: Arc<HookRegistry>) -> Self {
        Self { registry }
    }

    /// Run the dispatcher, consuming events from the receiver.
    ///
    /// This method runs indefinitely until the channel is closed.
    pub async fn run(self, mut receiver: broadcast::Receiver<SystemEvent>) {
        info!("Starting hook dispatcher");

        loop {
            match receiver.recv().await {
                Ok(event) => {
                    self.registry.dispatch(&event).await;
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    warn!(missed = n, "Dispatcher lagged, missed events");
                }
                Err(broadcast::error::RecvError::Closed) => {
                    info!("Hook dispatcher channel closed, stopping");
                    break;
                }
            }
        }

        // Graceful shutdown
        if let Err(e) = self.registry.on_shutdown().await {
            warn!(error = %e, "Error during hook shutdown");
        }
    }

    /// Get the registry.
    pub fn registry(&self) -> &Arc<HookRegistry> {
        &self.registry
    }
}

impl std::fmt::Debug for HookDispatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HookDispatcher")
            .field("registry", &self.registry)
            .finish()
    }
}

// ============================================================================
// Builder
// ============================================================================

/// Builder for creating the hook system.
pub struct HookSystemBuilder {
    registry: HookRegistry,
}

impl HookSystemBuilder {
    /// Create a new builder.
    pub fn new() -> Self {
        Self {
            registry: HookRegistry::new(),
        }
    }

    /// Create a new builder with custom timeout.
    pub fn with_timeout(timeout: Duration) -> Self {
        Self {
            registry: HookRegistry::with_timeout(timeout),
        }
    }

    /// Register a system hook.
    pub async fn register(self, hook: Arc<dyn SystemHook>) -> Self {
        self.registry.register(hook).await;
        self
    }

    /// Register a resource hook.
    pub async fn register_resource<H: ResourceHook + 'static>(self, hook: Arc<H>) -> Self {
        self.registry.register_resource(hook).await;
        self
    }

    /// Register an auth hook.
    pub async fn register_auth<H: AuthHook + 'static>(self, hook: Arc<H>) -> Self {
        self.registry.register_auth(hook).await;
        self
    }

    /// Build the registry.
    pub fn build(self) -> Arc<HookRegistry> {
        Arc::new(self.registry)
    }

    /// Build and start the dispatcher.
    ///
    /// Returns the registry and spawns the dispatcher as a background task.
    pub fn start(self, receiver: broadcast::Receiver<SystemEvent>) -> Arc<HookRegistry> {
        let registry = Arc::new(self.registry);
        let dispatcher = HookDispatcher::new(registry.clone());
        tokio::spawn(dispatcher.run(receiver));
        registry
    }
}

impl Default for HookSystemBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::types::ResourceEvent;
    use std::sync::atomic::{AtomicU32, Ordering};

    struct CountingHook {
        name: &'static str,
        count: AtomicU32,
    }

    impl CountingHook {
        fn new(name: &'static str) -> Self {
            Self {
                name,
                count: AtomicU32::new(0),
            }
        }

        fn count(&self) -> u32 {
            self.count.load(Ordering::SeqCst)
        }
    }

    #[async_trait::async_trait]
    impl ResourceHook for CountingHook {
        fn name(&self) -> &str {
            self.name
        }

        fn resource_types(&self) -> &[&str] {
            &[]
        }

        async fn handle(&self, _event: &ResourceEvent) -> Result<(), HookError> {
            self.count.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_registry_register() {
        let registry = HookRegistry::new();
        assert_eq!(registry.hook_count().await, 0);

        let hook = Arc::new(CountingHook::new("test"));
        registry.register_resource(hook).await;
        assert_eq!(registry.hook_count().await, 1);
    }

    #[tokio::test]
    async fn test_registry_dispatch() {
        let registry = HookRegistry::new();
        let hook = Arc::new(CountingHook::new("test"));
        registry.register_resource(hook.clone()).await;

        let event = SystemEvent::Resource(ResourceEvent::created(
            "Patient",
            "123",
            serde_json::json!({}),
        ));

        registry.dispatch(&event).await;

        // Give the spawned task time to run
        tokio::time::sleep(Duration::from_millis(50)).await;

        assert_eq!(hook.count(), 1);
    }

    #[tokio::test]
    async fn test_dispatcher_run() {
        use crate::events::EventBroadcaster;

        let broadcaster = EventBroadcaster::new();
        let registry = Arc::new(HookRegistry::new());
        let hook = Arc::new(CountingHook::new("test"));
        registry.register_resource(hook.clone()).await;

        let dispatcher = HookDispatcher::new(registry.clone());
        let receiver = broadcaster.subscribe();

        // Start dispatcher in background
        tokio::spawn(dispatcher.run(receiver));

        // Send events
        broadcaster.send_created("Patient", "1", serde_json::json!({}));
        broadcaster.send_created("Patient", "2", serde_json::json!({}));

        // Give time for processing
        tokio::time::sleep(Duration::from_millis(100)).await;

        assert_eq!(hook.count(), 2);
    }

    struct PanicHook;

    #[async_trait::async_trait]
    impl ResourceHook for PanicHook {
        fn name(&self) -> &str {
            "panic_hook"
        }

        fn resource_types(&self) -> &[&str] {
            &[]
        }

        async fn handle(&self, _event: &ResourceEvent) -> Result<(), HookError> {
            panic!("This hook panics!");
        }
    }

    #[tokio::test]
    async fn test_panic_isolation() {
        let registry = HookRegistry::new();

        let panic_hook = Arc::new(PanicHook);
        let counting_hook = Arc::new(CountingHook::new("counter"));

        registry.register_resource(panic_hook).await;
        registry.register_resource(counting_hook.clone()).await;

        let event = SystemEvent::Resource(ResourceEvent::created(
            "Patient",
            "123",
            serde_json::json!({}),
        ));

        // This should not panic the test, even though one hook panics
        registry.dispatch(&event).await;

        // Give time for processing
        tokio::time::sleep(Duration::from_millis(100)).await;

        // The counting hook should still have run
        assert_eq!(counting_hook.count(), 1);
    }
}
