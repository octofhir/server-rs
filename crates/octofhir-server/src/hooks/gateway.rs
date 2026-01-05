//! Gateway reload hook.
//!
//! This hook triggers gateway route reload when App or CustomOperation resources are changed.
//! It replaces the PostgreSQL LISTEN/NOTIFY based GatewayReloadListener.

use std::sync::Arc;

use async_trait::async_trait;
use octofhir_core::events::{HookError, ResourceEvent, ResourceHook};
use octofhir_storage::DynStorage;
use tracing::{debug, error, info, warn};

use crate::gateway::GatewayRouter;
use crate::operation_registry::OperationRegistryService;

/// Hook that triggers gateway route reload on App/CustomOperation changes.
///
/// When an App or CustomOperation resource is created, updated, or deleted,
/// this hook triggers a full reload of gateway routes.
///
/// # Example
///
/// ```ignore
/// let hook = GatewayReloadHook::new(gateway_router, storage)
///     .with_operation_registry(operation_registry);
/// registry.register(Arc::new(hook));
/// ```
pub struct GatewayReloadHook {
    gateway_router: Arc<GatewayRouter>,
    storage: DynStorage,
    operation_registry: Option<Arc<OperationRegistryService>>,
}

impl GatewayReloadHook {
    /// Create a new gateway reload hook.
    ///
    /// # Arguments
    ///
    /// * `gateway_router` - The gateway router to reload
    /// * `storage` - Storage for loading routes from database
    pub fn new(gateway_router: Arc<GatewayRouter>, storage: DynStorage) -> Self {
        Self {
            gateway_router,
            storage,
            operation_registry: None,
        }
    }

    /// Add operation registry for updating when routes change.
    pub fn with_operation_registry(mut self, registry: Arc<OperationRegistryService>) -> Self {
        self.operation_registry = Some(registry);
        self
    }
}

#[async_trait]
impl ResourceHook for GatewayReloadHook {
    fn name(&self) -> &str {
        "gateway_reload"
    }

    fn resource_types(&self) -> &[&str] {
        &["App", "CustomOperation"]
    }

    async fn handle(&self, event: &ResourceEvent) -> Result<(), HookError> {
        debug!(
            resource_type = %event.resource_type,
            resource_id = %event.resource_id,
            event_type = %event.event_type,
            "GatewayReloadHook: triggering route reload"
        );

        // Reload gateway routes
        match self.gateway_router.reload_routes(&self.storage).await {
            Ok(count) => {
                info!(
                    count = count,
                    resource_type = %event.resource_type,
                    resource_id = %event.resource_id,
                    "Gateway routes reloaded successfully"
                );

                // Re-sync operations to update in-memory indexes
                if let Some(ref registry) = self.operation_registry {
                    match registry.sync_operations(false).await {
                        Ok(_) => {
                            debug!("Operation registry indexes updated after gateway reload");
                        }
                        Err(e) => {
                            warn!(error = %e, "Failed to update operation registry indexes");
                            // Don't fail the hook - route reload was successful
                        }
                    }
                }

                Ok(())
            }
            Err(e) => {
                error!(
                    error = %e,
                    resource_type = %event.resource_type,
                    resource_id = %event.resource_id,
                    "Failed to reload gateway routes"
                );
                Err(HookError::execution(format!("Gateway reload failed: {}", e)))
            }
        }
    }
}

impl std::fmt::Debug for GatewayReloadHook {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GatewayReloadHook")
            .field("has_operation_registry", &self.operation_registry.is_some())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use octofhir_core::events::ResourceEvent;
    use serde_json::json;

    // Note: Full tests require mock implementations of GatewayRouter and storage.
    // These are basic tests for matching logic.

    #[test]
    fn test_resource_type_matching() {
        // Create a dummy event
        let app_event = ResourceEvent::created("App", "test-id", json!({}));
        let custom_op_event = ResourceEvent::created("CustomOperation", "test-id", json!({}));
        let patient_event = ResourceEvent::created("Patient", "test-id", json!({}));

        // Check matching resource types
        assert!(["App", "CustomOperation"].contains(&app_event.resource_type.as_str()));
        assert!(["App", "CustomOperation"].contains(&custom_op_event.resource_type.as_str()));
        assert!(!["App", "CustomOperation"].contains(&patient_event.resource_type.as_str()));
    }
}
