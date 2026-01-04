use async_trait::async_trait;
use lettre::{
    message::header::ContentType,
    transport::smtp::authentication::Credentials,
    AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor,
};
use reqwest::Client;
use serde_json::json;

use super::{NotificationAdapter, RenderedContent, SendResult};
use crate::error::NotificationError;
use crate::provider::ProviderConfig;
use crate::types::Notification;

pub struct EmailAdapter {
    http_client: Client,
}

impl EmailAdapter {
    pub fn new() -> Self {
        Self {
            http_client: Client::new(),
        }
    }

    async fn send_sendgrid(
        &self,
        config: &ProviderConfig,
        notification: &Notification,
        content: &RenderedContent,
    ) -> Result<SendResult, NotificationError> {
        let api_key = config
            .api_key
            .as_ref()
            .ok_or(NotificationError::InvalidConfig("Missing api_key".into()))?;

        let from = config
            .from
            .as_ref()
            .ok_or(NotificationError::InvalidConfig("Missing from".into()))?;

        let to = notification
            .recipient
            .email
            .as_ref()
            .ok_or(NotificationError::RecipientNotFound)?;

        let subject = content.subject.as_deref().unwrap_or("Notification");

        let body = json!({
            "personalizations": [{
                "to": [{"email": to}]
            }],
            "from": {"email": from},
            "subject": subject,
            "content": [{
                "type": if content.html_body.is_some() { "text/html" } else { "text/plain" },
                "value": content.html_body.as_ref().unwrap_or(&content.body)
            }]
        });

        let response = self
            .http_client
            .post("https://api.sendgrid.com/v3/mail/send")
            .bearer_auth(api_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| NotificationError::SendFailed(e.to_string()))?;

        if response.status().is_success() {
            let message_id = response
                .headers()
                .get("x-message-id")
                .and_then(|v| v.to_str().ok())
                .map(String::from);

            Ok(SendResult {
                success: true,
                external_id: message_id,
                error: None,
            })
        } else {
            let error = response.text().await.unwrap_or_default();
            Ok(SendResult {
                success: false,
                external_id: None,
                error: Some(error),
            })
        }
    }

    async fn send_smtp(
        &self,
        config: &ProviderConfig,
        notification: &Notification,
        content: &RenderedContent,
    ) -> Result<SendResult, NotificationError> {
        let host = config
            .smtp_host
            .as_ref()
            .ok_or(NotificationError::InvalidConfig("Missing smtp_host".into()))?;

        let port = config.smtp_port.unwrap_or(587);

        let from = config
            .from
            .as_ref()
            .ok_or(NotificationError::InvalidConfig("Missing from".into()))?;

        let to = notification
            .recipient
            .email
            .as_ref()
            .ok_or(NotificationError::RecipientNotFound)?;

        let subject = content.subject.as_deref().unwrap_or("Notification");

        let email = Message::builder()
            .from(
                from.parse()
                    .map_err(|e| NotificationError::InvalidConfig(format!("Invalid from: {}", e)))?,
            )
            .to(to
                .parse()
                .map_err(|e| NotificationError::InvalidConfig(format!("Invalid to: {}", e)))?)
            .subject(subject)
            .header(ContentType::TEXT_PLAIN)
            .body(content.body.clone())
            .map_err(|e| NotificationError::SendFailed(e.to_string()))?;

        let mut mailer_builder = AsyncSmtpTransport::<Tokio1Executor>::relay(host)
            .map_err(|e| NotificationError::InvalidConfig(e.to_string()))?
            .port(port);

        if let (Some(username), Some(password)) = (&config.smtp_username, &config.smtp_password) {
            mailer_builder =
                mailer_builder.credentials(Credentials::new(username.clone(), password.clone()));
        }

        let mailer = mailer_builder.build();

        match mailer.send(email).await {
            Ok(response) => Ok(SendResult {
                success: true,
                external_id: Some(response.message().map(|m| m.to_string()).collect()),
                error: None,
            }),
            Err(e) => Ok(SendResult {
                success: false,
                external_id: None,
                error: Some(e.to_string()),
            }),
        }
    }
}

impl Default for EmailAdapter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl NotificationAdapter for EmailAdapter {
    async fn send(
        &self,
        config: &ProviderConfig,
        notification: &Notification,
        content: &RenderedContent,
    ) -> Result<SendResult, NotificationError> {
        // Determine provider by available config
        if config.api_key.is_some() {
            self.send_sendgrid(config, notification, content).await
        } else if config.smtp_host.is_some() {
            self.send_smtp(config, notification, content).await
        } else {
            Err(NotificationError::InvalidConfig(
                "No email provider configured".into(),
            ))
        }
    }

    fn supports(&self, provider: &str) -> bool {
        matches!(provider, "sendgrid" | "smtp" | "email")
    }
}
