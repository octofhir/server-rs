//! Notification Operations
//!
//! FHIR operations for the Notification resource:
//! - `$resend` - Resend a failed notification
//! - `$resend-all` - Resend all failed notifications
//! - `$stats` - Get notification statistics
//! - `$cancel` - Cancel a pending notification

use async_trait::async_trait;
use serde_json::{json, Value};

use super::handler::{OperationError, OperationHandler};
use crate::server::AppState;

/// $resend operation - Resend a single failed notification
pub struct ResendOperation;

#[async_trait]
impl OperationHandler for ResendOperation {
    fn code(&self) -> &str {
        "resend"
    }

    async fn handle_instance(
        &self,
        state: &AppState,
        resource_type: &str,
        id: &str,
        _params: &Value,
    ) -> Result<Value, OperationError> {
        if resource_type != "Notification" {
            return Err(OperationError::NotSupported(
                "$resend is only supported for Notification resources".to_string(),
            ));
        }

        // Get the notification queue storage
        let notification_queue = state.notification_queue.as_ref().ok_or_else(|| {
            OperationError::NotSupported("Notifications are not enabled".to_string())
        })?;

        // Restart the notification
        let success = notification_queue
            .restart(id)
            .await
            .map_err(|e| OperationError::Internal(e.to_string()))?;

        if success {
            Ok(json!({
                "resourceType": "OperationOutcome",
                "issue": [{
                    "severity": "information",
                    "code": "informational",
                    "diagnostics": format!("Notification {} has been queued for resend", id)
                }]
            }))
        } else {
            Err(OperationError::NotFound(format!(
                "Notification {} not found or not in failed state",
                id
            )))
        }
    }
}

/// $resend-all operation - Resend all failed notifications
pub struct ResendAllOperation;

#[async_trait]
impl OperationHandler for ResendAllOperation {
    fn code(&self) -> &str {
        "resend-all"
    }

    async fn handle_type(
        &self,
        state: &AppState,
        resource_type: &str,
        _params: &Value,
    ) -> Result<Value, OperationError> {
        if resource_type != "Notification" {
            return Err(OperationError::NotSupported(
                "$resend-all is only supported for Notification resources".to_string(),
            ));
        }

        // Get the notification queue storage
        let notification_queue = state.notification_queue.as_ref().ok_or_else(|| {
            OperationError::NotSupported("Notifications are not enabled".to_string())
        })?;

        // Restart all failed notifications
        let count = notification_queue
            .restart_all_failed()
            .await
            .map_err(|e| OperationError::Internal(e.to_string()))?;

        Ok(json!({
            "resourceType": "OperationOutcome",
            "issue": [{
                "severity": "information",
                "code": "informational",
                "diagnostics": format!("{} failed notifications have been queued for resend", count)
            }]
        }))
    }
}

/// $stats operation - Get notification statistics
pub struct StatsOperation;

#[async_trait]
impl OperationHandler for StatsOperation {
    fn code(&self) -> &str {
        "stats"
    }

    async fn handle_type(
        &self,
        state: &AppState,
        resource_type: &str,
        _params: &Value,
    ) -> Result<Value, OperationError> {
        if resource_type != "Notification" {
            return Err(OperationError::NotSupported(
                "$stats is only supported for Notification resources".to_string(),
            ));
        }

        // Get the notification queue storage
        let notification_queue = state.notification_queue.as_ref().ok_or_else(|| {
            OperationError::NotSupported("Notifications are not enabled".to_string())
        })?;

        // Get statistics
        let stats = notification_queue
            .get_stats()
            .await
            .map_err(|e| OperationError::Internal(e.to_string()))?;

        Ok(json!({
            "resourceType": "Parameters",
            "parameter": [
                {"name": "pending", "valueInteger": stats.pending},
                {"name": "sending", "valueInteger": stats.sending},
                {"name": "sent", "valueInteger": stats.sent},
                {"name": "failed", "valueInteger": stats.failed},
                {"name": "cancelled", "valueInteger": stats.cancelled},
                {"name": "total", "valueInteger": stats.pending + stats.sending + stats.sent + stats.failed + stats.cancelled}
            ]
        }))
    }
}

/// $cancel operation - Cancel a pending notification
pub struct CancelOperation;

#[async_trait]
impl OperationHandler for CancelOperation {
    fn code(&self) -> &str {
        "cancel"
    }

    async fn handle_instance(
        &self,
        state: &AppState,
        resource_type: &str,
        id: &str,
        _params: &Value,
    ) -> Result<Value, OperationError> {
        if resource_type != "Notification" {
            return Err(OperationError::NotSupported(
                "$cancel is only supported for Notification resources".to_string(),
            ));
        }

        // Get the notification queue storage
        let notification_queue = state.notification_queue.as_ref().ok_or_else(|| {
            OperationError::NotSupported("Notifications are not enabled".to_string())
        })?;

        // Cancel the notification
        let success = notification_queue
            .cancel(id)
            .await
            .map_err(|e| OperationError::Internal(e.to_string()))?;

        if success {
            Ok(json!({
                "resourceType": "OperationOutcome",
                "issue": [{
                    "severity": "information",
                    "code": "informational",
                    "diagnostics": format!("Notification {} has been cancelled", id)
                }]
            }))
        } else {
            Err(OperationError::NotFound(format!(
                "Notification {} not found or not in pending state",
                id
            )))
        }
    }
}
