//! Notification Operations
//!
//! FHIR operations for the Notification resource:
//! - `$resend` - Resend a failed notification
//! - `$resend-all` - Resend all failed notifications
//! - `$stats` - Get notification statistics
//! - `$cancel` - Cancel a pending notification
//! - `$send` - Queue a notification

use std::collections::HashMap;

use async_trait::async_trait;
use octofhir_notifications::{
    Notification, NotificationChannel, NotificationRecipient, NotificationStatus,
};
use serde_json::{Value, json};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

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
        _context: &crate::operations::OperationContext,
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
        _context: &crate::operations::OperationContext,
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
        _context: &crate::operations::OperationContext,
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
        _context: &crate::operations::OperationContext,
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

/// `$send`: queue a notification through a `NotificationProvider` with a `NotificationTemplate`.
///
/// Parameters: `provider` and `template` (ids), one recipient (`email`, `telegramChatId`,
/// `webhookUrl`; `reference` may come along), `data` parts whose names fill `{{name}}`, and
/// `scheduledAt` to send later.
pub struct SendOperation;

/// The value of a parameter of a `Parameters` resource, whatever its type.
fn value_of(parameter: &Value) -> Option<Value> {
    let object = parameter.as_object()?;
    object
        .iter()
        .find(|(key, _)| key.starts_with("value"))
        .map(|(_, value)| value.clone())
}

fn parameter<'a>(params: &'a Value, name: &str) -> Option<&'a Value> {
    params["parameter"]
        .as_array()?
        .iter()
        .find(|parameter| parameter["name"] == name)
}

fn text(params: &Value, name: &str) -> Option<String> {
    parameter(params, name)
        .and_then(value_of)
        .and_then(|value| value.as_str().map(str::to_owned))
}

#[async_trait]
impl OperationHandler for SendOperation {
    fn code(&self) -> &str {
        "send"
    }

    async fn handle_type(
        &self,
        state: &AppState,
        _context: &crate::operations::OperationContext,
        resource_type: &str,
        params: &Value,
    ) -> Result<Value, OperationError> {
        if resource_type != "Notification" {
            return Err(OperationError::NotSupported(
                "$send is only supported for Notification".to_string(),
            ));
        }
        let queue = state.notification_queue.as_ref().ok_or_else(|| {
            OperationError::NotSupported("Notifications are not enabled".to_string())
        })?;
        let required = |name: &str| {
            text(params, name)
                .ok_or_else(|| OperationError::InvalidParameters(format!("{name} is required")))
        };
        let provider_id = required("provider")?;
        let template_id = required("template")?;

        let provider = read(state, "NotificationProvider", &provider_id).await?;
        if provider["active"] == Value::Bool(false) {
            return Err(OperationError::InvalidParameters(format!(
                "NotificationProvider/{provider_id} is not active"
            )));
        }
        let channel = match provider["type"].as_str() {
            Some("email") => NotificationChannel::Email,
            Some("telegram") => NotificationChannel::Telegram,
            Some("webhook") => NotificationChannel::Webhook,
            other => {
                return Err(OperationError::InvalidParameters(format!(
                    "providers of type {} cannot send",
                    other.unwrap_or("none")
                )));
            }
        };
        let template = read(state, "NotificationTemplate", &template_id).await?;
        if template["active"] == Value::Bool(false) {
            return Err(OperationError::InvalidParameters(format!(
                "NotificationTemplate/{template_id} is not active"
            )));
        }

        let recipient = NotificationRecipient {
            reference: text(params, "reference"),
            email: text(params, "email"),
            telegram_chat_id: text(params, "telegramChatId"),
            webhook_url: text(params, "webhookUrl"),
        };
        let has_recipient = match channel {
            NotificationChannel::Email => recipient.email.is_some(),
            NotificationChannel::Telegram => recipient.telegram_chat_id.is_some(),
            NotificationChannel::Webhook => {
                recipient.webhook_url.is_some() || provider["config"]["webhookUrl"].is_string()
            }
        };
        if !has_recipient {
            return Err(OperationError::InvalidParameters(
                "no recipient for this provider's channel".to_string(),
            ));
        }
        // A webhook may only go where other outbound requests may.
        for url in [
            recipient.webhook_url.as_deref(),
            provider["config"]["webhookUrl"].as_str(),
        ]
        .into_iter()
        .flatten()
        {
            let parsed = url::Url::parse(url).map_err(|error| {
                OperationError::InvalidParameters(format!("webhook URL: {error}"))
            })?;
            state.egress.check_url(&parsed).map_err(|error| {
                OperationError::InvalidParameters(format!("webhook URL: {error}"))
            })?;
        }

        let mut data = HashMap::new();
        for part in params["parameter"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|parameter| parameter["name"] == "data")
            .flat_map(|parameter| parameter["part"].as_array().into_iter().flatten())
        {
            if let (Some(name), Some(value)) = (part["name"].as_str(), value_of(part)) {
                data.insert(name.to_owned(), value);
            }
        }
        let scheduled_at = match text(params, "scheduledAt") {
            Some(at) => Some(OffsetDateTime::parse(&at, &Rfc3339).map_err(|error| {
                OperationError::InvalidParameters(format!("scheduledAt: {error}"))
            })?),
            None => None,
        };

        let notification = Notification {
            id: octofhir_core::generate_id(),
            channel,
            provider_id,
            status: NotificationStatus::Pending,
            recipient,
            template_id,
            template_data: data,
            scheduled_at,
            created_at: OffsetDateTime::now_utc(),
            sent_at: None,
            error: None,
            retry_count: 0,
        };
        queue
            .enqueue(&notification)
            .await
            .map_err(|error| OperationError::Internal(error.to_string()))?;
        Ok(json!({
            "resourceType": "Parameters",
            "parameter": [
                {"name": "id", "valueString": notification.id},
                {"name": "status", "valueCode": "pending"},
            ]
        }))
    }
}

/// A resource `$send` names, or why it cannot be used.
async fn read(state: &AppState, resource_type: &str, id: &str) -> Result<Value, OperationError> {
    state
        .storage
        .read(resource_type, id)
        .await
        .map_err(|error| OperationError::Internal(error.to_string()))?
        .map(|stored| stored.resource)
        .ok_or_else(|| OperationError::InvalidParameters(format!("{resource_type}/{id} not found")))
}
