//! Async audit hook.
//!
//! This hook logs resource changes asynchronously without blocking the API response.
//! It creates FHIR AuditEvent resources for tracking system activity.

use std::sync::Arc;

use async_trait::async_trait;
use octofhir_core::events::{HookError, ResourceEvent, ResourceEventType, ResourceHook};
use tracing::{debug, warn};

use crate::audit::{AuditAction, AuditEventBuilder, AuditOutcome, AuditService};

/// Hook that creates audit events asynchronously for resource changes.
///
/// When a resource is created, updated, or deleted, this hook:
/// 1. Checks if audit logging is enabled for this resource type
/// 2. Builds a FHIR AuditEvent resource
/// 3. Stores it asynchronously (fire-and-forget)
///
/// # Example
///
/// ```ignore
/// let hook = AsyncAuditHook::new(audit_service.clone());
/// registry.register(Arc::new(hook));
/// ```
pub struct AsyncAuditHook {
    audit_service: Arc<AuditService>,
}

impl AsyncAuditHook {
    /// Create a new async audit hook.
    ///
    /// # Arguments
    ///
    /// * `audit_service` - The audit service for creating audit events
    pub fn new(audit_service: Arc<AuditService>) -> Self {
        Self { audit_service }
    }
}

#[async_trait]
impl ResourceHook for AsyncAuditHook {
    fn name(&self) -> &str {
        "async_audit"
    }

    fn resource_types(&self) -> &[&str] {
        // Subscribe to all resource types - filtering is done by audit config
        &[]
    }

    async fn handle(&self, event: &ResourceEvent) -> Result<(), HookError> {
        // Map event type to audit action
        let action = match event.event_type {
            ResourceEventType::Created => AuditAction::ResourceCreate,
            ResourceEventType::Updated => AuditAction::ResourceUpdate,
            ResourceEventType::Deleted => AuditAction::ResourceDelete,
        };

        // Check if this action should be logged based on config
        if !self
            .audit_service
            .should_log(&action, Some(&event.resource_type))
        {
            return Ok(());
        }

        // Build the audit event
        let audit_builder = AuditEventBuilder::new(action)
            .outcome(AuditOutcome::Success)
            .system()
            .entity(
                Some(event.resource_type.clone()),
                Some(event.resource_id.clone()),
                None,
            );

        // Log the event asynchronously (fire-and-forget)
        let audit_service = self.audit_service.clone();
        tokio::spawn(async move {
            if let Err(e) = audit_service.log(audit_builder).await {
                warn!(error = %e, "Failed to log audit event");
            }
        });

        debug!(
            resource_type = %event.resource_type,
            resource_id = %event.resource_id,
            action = ?action,
            "Audit event queued"
        );

        Ok(())
    }
}

impl std::fmt::Debug for AsyncAuditHook {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AsyncAuditHook")
            .field("enabled", &self.audit_service.is_enabled())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: Full tests require mock AuditService.
    // Basic tests for matching logic.

    #[test]
    fn test_resource_type_matching() {
        // Async audit hook should match all resource types
        // (actual filtering done by audit config)
    }
}
