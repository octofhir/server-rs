use async_trait::async_trait;
use reqwest::Client;
use serde_json::json;

use super::{NotificationAdapter, RenderedContent, SendResult};
use crate::error::NotificationError;
use crate::provider::ProviderConfig;
use crate::types::Notification;

pub struct TelegramAdapter {
    http_client: Client,
}

impl TelegramAdapter {
    pub fn new() -> Self {
        Self {
            http_client: Client::new(),
        }
    }
}

impl Default for TelegramAdapter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl NotificationAdapter for TelegramAdapter {
    async fn send(
        &self,
        config: &ProviderConfig,
        notification: &Notification,
        content: &RenderedContent,
    ) -> Result<SendResult, NotificationError> {
        let bot_token = config
            .bot_token
            .as_ref()
            .ok_or(NotificationError::InvalidConfig("Missing bot_token".into()))?;

        let chat_id = notification
            .recipient
            .telegram_chat_id
            .as_ref()
            .ok_or(NotificationError::RecipientNotFound)?;

        let url = format!("https://api.telegram.org/bot{}/sendMessage", bot_token);

        let body = json!({
            "chat_id": chat_id,
            "text": &content.body,
            "parse_mode": "Markdown",
            "disable_web_page_preview": true
        });

        let response = self
            .http_client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| NotificationError::SendFailed(e.to_string()))?;

        let status = response.status();
        let response_body: serde_json::Value = response
            .json()
            .await
            .map_err(|e| NotificationError::SendFailed(e.to_string()))?;

        if status.is_success() && response_body["ok"].as_bool() == Some(true) {
            let message_id = response_body["result"]["message_id"]
                .as_i64()
                .map(|id| id.to_string());

            Ok(SendResult {
                success: true,
                external_id: message_id,
                error: None,
            })
        } else {
            let error = response_body["description"]
                .as_str()
                .unwrap_or("Unknown error")
                .to_string();

            Ok(SendResult {
                success: false,
                external_id: None,
                error: Some(error),
            })
        }
    }

    fn supports(&self, provider: &str) -> bool {
        provider == "telegram"
    }
}
