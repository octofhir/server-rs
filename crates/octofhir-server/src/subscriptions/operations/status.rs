//! $status operation for Subscription resources.
//!
//! Returns the current status of a subscription, including delivery statistics
//! and error information.

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::operations::{OperationError, OperationHandler};
use crate::server::AppState;

/// The $status operation for Subscription resources.
///
/// Returns a `SubscriptionStatus` resource (or `Parameters` resource containing
/// status information) for a specific subscription.
///
/// Endpoint: `GET /fhir/Subscription/{id}/$status`
pub struct StatusOperation;

#[async_trait]
impl OperationHandler for StatusOperation {
    fn code(&self) -> &str {
        "status"
    }

    async fn handle_system(
        &self,
        _state: &AppState,
        _params: &Value,
    ) -> Result<Value, OperationError> {
        Err(OperationError::NotSupported(
            "Operation $status is not supported at system level. Use /Subscription/{id}/$status instead.".into(),
        ))
    }

    async fn handle_type(
        &self,
        _state: &AppState,
        resource_type: &str,
        _params: &Value,
    ) -> Result<Value, OperationError> {
        if resource_type != "Subscription" {
            return Err(OperationError::NotSupported(format!(
                "Operation $status is only supported for Subscription resources, not {}",
                resource_type
            )));
        }

        // Type-level status is not implemented - would return all subscription statuses
        Err(OperationError::NotSupported(
            "Operation $status at type level is not yet implemented. Use /Subscription/{id}/$status instead.".into(),
        ))
    }

    async fn handle_instance(
        &self,
        state: &AppState,
        resource_type: &str,
        id: &str,
        _params: &Value,
    ) -> Result<Value, OperationError> {
        if resource_type != "Subscription" {
            return Err(OperationError::NotSupported(format!(
                "Operation $status is only supported for Subscription resources, not {}",
                resource_type
            )));
        }

        // Get the subscription from storage
        let stored = state
            .storage
            .read("Subscription", id)
            .await
            .map_err(|e| OperationError::Internal(e.to_string()))?
            .ok_or_else(|| OperationError::NotFound(format!("Subscription/{} not found", id)))?;

        let subscription = &stored.resource;

        // Extract current status from subscription
        let status = subscription
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("off");

        // Get topic URL
        let topic = subscription
            .get("topic")
            .and_then(|v| v.as_str())
            .or_else(|| subscription.get("criteria").and_then(|v| v.as_str()))
            .unwrap_or("");

        // Build SubscriptionStatus resource
        // Note: SubscriptionStatus is an R5 resource. For R4 with Backport IG,
        // we return a Parameters resource with equivalent information.
        let status_resource = json!({
            "resourceType": "SubscriptionStatus",
            "id": format!("{}-status", id),
            "status": status,
            "type": "query-status",
            "subscription": {
                "reference": format!("Subscription/{}", id)
            },
            "topic": topic,
            "notificationEvent": []
        });

        // TODO: In R5, we would return SubscriptionStatus directly.
        // For R4 compatibility, we could wrap in Parameters if needed.
        // For now, we return the SubscriptionStatus-like structure.

        Ok(status_resource)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_operation_code() {
        assert_eq!(StatusOperation.code(), "status");
    }
}
