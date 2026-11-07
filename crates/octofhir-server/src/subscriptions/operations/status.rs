//! $status operation for Subscription resources.
//!
//! Returns the current status of a subscription, including delivery statistics
//! and error information.

use crate::operations::handler::OperationLevel;

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
    fn supports_level(&self, level: OperationLevel, resource_type: Option<&str>) -> bool {
        level == OperationLevel::Instance && resource_type == Some("Subscription")
    }

    fn code(&self) -> &str {
        "status"
    }

    async fn handle_system(
        &self,
        _state: &AppState,
        _context: &crate::operations::OperationContext,
        _params: &Value,
    ) -> Result<Value, OperationError> {
        Err(OperationError::NotSupported(
            "Operation $status is not supported at system level. Use /Subscription/{id}/$status instead.".into(),
        ))
    }

    async fn handle_type(
        &self,
        _state: &AppState,
        _context: &crate::operations::OperationContext,
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
        _context: &crate::operations::OperationContext,
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
            // Backport filters ride on the topic URL after `?`.
            .map(|url| url.split_once('?').map_or(url, |(topic, _)| topic))
            .unwrap_or("");

        let stats = state
            .subscription_state
            .event_storage
            .get_subscription_stats(id)
            .await
            .map_err(|e| OperationError::Internal(e.to_string()))?;

        // Built as an R5 SubscriptionStatus; `backport::status` gives the R4 form.
        let mut status_resource = json!({
            "resourceType": "SubscriptionStatus",
            "id": format!("{}-status", id),
            "status": status,
            "type": "query-status",
            // integer64 is a JSON string in FHIR R5.
            "eventsSinceSubscriptionStart": stats.events_since_subscription_start.to_string(),
            "subscription": {
                "reference": format!("Subscription/{}", id)
            },
            "topic": topic,
            "notificationEvent": []
        });
        // The latest delivery failure, or why the server put the subscription in error.
        let error = stats.last_error_message.or_else(|| {
            subscription
                .get("error")
                .and_then(|v| v.as_str())
                .map(str::to_owned)
        });
        if let Some(error) = error {
            status_resource["error"] = json!([{ "text": error }]);
        }

        // The operation returns a Bundle of statuses; R4 carries each as backport Parameters.
        Ok(json!({
            "resourceType": "Bundle",
            "type": "searchset",
            "total": 1,
            "entry": [{
                "fullUrl": format!("urn:uuid:{}", uuid::Uuid::new_v4()),
                "resource": crate::subscriptions::backport::status(status_resource),
                "search": {"mode": "match"},
            }],
        }))
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
