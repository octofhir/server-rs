use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use time::OffsetDateTime;

/// Notification channel type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NotificationChannel {
    Email,
    Telegram,
    Webhook,
}

/// Notification status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NotificationStatus {
    Pending,
    Sending,
    Sent,
    Failed,
    Cancelled,
}

/// A notification to be sent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notification {
    pub id: String,
    pub channel: NotificationChannel,
    pub provider_id: String,
    pub status: NotificationStatus,

    /// Recipient info
    pub recipient: NotificationRecipient,

    /// Template ID
    pub template_id: String,

    /// Template data for rendering
    pub template_data: HashMap<String, serde_json::Value>,

    /// When to send (None = immediately)
    #[serde(with = "time::serde::rfc3339::option")]
    pub scheduled_at: Option<OffsetDateTime>,

    /// Timestamps
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,

    #[serde(with = "time::serde::rfc3339::option")]
    pub sent_at: Option<OffsetDateTime>,

    /// Error if failed
    pub error: Option<String>,

    /// Retry count
    pub retry_count: u32,
}

/// Notification recipient
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationRecipient {
    /// FHIR reference (e.g., "Patient/123")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,

    /// Email address
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,

    /// Telegram chat ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub telegram_chat_id: Option<String>,

    /// Webhook URL (for webhook channel)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub webhook_url: Option<String>,
}

/// Request to send notification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendNotificationRequest {
    pub channel: NotificationChannel,
    pub provider_id: String,
    pub recipient: NotificationRecipient,
    pub template_id: String,
    pub template_data: HashMap<String, serde_json::Value>,
    #[serde(with = "time::serde::rfc3339::option", default)]
    pub scheduled_at: Option<OffsetDateTime>,
}

/// Notification statistics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NotificationStats {
    pub pending: u32,
    pub sending: u32,
    pub sent: u32,
    pub failed: u32,
    pub cancelled: u32,
}
