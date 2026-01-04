pub mod email;
pub mod telegram;
pub mod webhook;

use async_trait::async_trait;

use crate::error::NotificationError;
use crate::provider::ProviderConfig;
use crate::types::Notification;

/// Result of sending a notification
#[derive(Debug)]
pub struct SendResult {
    pub success: bool,
    pub external_id: Option<String>,
    pub error: Option<String>,
}

/// Rendered notification content
#[derive(Debug, Clone)]
pub struct RenderedContent {
    pub subject: Option<String>,
    pub body: String,
    pub html_body: Option<String>,
}

/// Adapter for sending notifications
#[async_trait]
pub trait NotificationAdapter: Send + Sync {
    /// Send a notification
    async fn send(
        &self,
        config: &ProviderConfig,
        notification: &Notification,
        rendered_content: &RenderedContent,
    ) -> Result<SendResult, NotificationError>;

    /// Check if adapter can handle this provider
    fn supports(&self, provider: &str) -> bool;
}

pub use email::EmailAdapter;
pub use telegram::TelegramAdapter;
pub use webhook::WebhookAdapter;
