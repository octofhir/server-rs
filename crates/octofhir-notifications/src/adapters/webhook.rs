use async_trait::async_trait;
use hmac::{Hmac, KeyInit, Mac};
use reqwest::Client;
use serde_json::json;
use sha2::Sha256;
use time::OffsetDateTime;

use super::{NotificationAdapter, RenderedContent, SendResult};
use crate::error::NotificationError;
use crate::provider::ProviderConfig;
use crate::types::Notification;

type HmacSha256 = Hmac<Sha256>;

pub struct WebhookAdapter {
    http_client: Client,
}

impl WebhookAdapter {
    pub fn new() -> Self {
        Self::with_client(Client::new())
    }

    /// Send through `http_client`, which can enforce where requests may go.
    pub fn with_client(http_client: Client) -> Self {
        Self { http_client }
    }

    fn sign_payload(&self, payload: &str, secret: &str) -> String {
        let mut mac =
            HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC can take key of any size");
        mac.update(payload.as_bytes());
        let result = mac.finalize();
        hex::encode(result.into_bytes())
    }
}

impl Default for WebhookAdapter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl NotificationAdapter for WebhookAdapter {
    async fn send(
        &self,
        config: &ProviderConfig,
        notification: &Notification,
        content: &RenderedContent,
    ) -> Result<SendResult, NotificationError> {
        let url = notification
            .recipient
            .webhook_url
            .as_ref()
            .or(config.webhook_url.as_ref())
            .ok_or(NotificationError::InvalidConfig(
                "Missing webhook_url".into(),
            ))?;

        let now = OffsetDateTime::now_utc();
        let timestamp = now
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default();

        let payload = json!({
            "notification_id": notification.id,
            "channel": notification.channel,
            "template_id": notification.template_id,
            "recipient": notification.recipient,
            "content": {
                "subject": content.subject,
                "body": content.body
            },
            "data": notification.template_data,
            "timestamp": timestamp
        });

        let payload_str = serde_json::to_string(&payload)
            .map_err(|e| NotificationError::SendFailed(e.to_string()))?;

        let mut request = self
            .http_client
            .post(url)
            .header("Content-Type", "application/json");

        for header in &config.webhook_header {
            request = request.header(&header.name, &header.value);
        }

        // Add HMAC signature if secret is configured
        if let Some(secret) = &config.webhook_secret {
            let signature = self.sign_payload(&payload_str, secret);
            request = request.header("X-Signature-256", format!("sha256={}", signature));
        }

        let response = request
            .body(payload_str)
            .send()
            .await
            .map_err(|e| NotificationError::SendFailed(e.to_string()))?;

        if response.status().is_success() {
            Ok(SendResult {
                success: true,
                external_id: None,
                error: None,
            })
        } else {
            let error = response.text().await.unwrap_or_default();
            Ok(SendResult {
                success: false,
                external_id: None,
                error: Some(format!("Webhook failed: {}", error)),
            })
        }
    }

    fn supports(&self, provider: &str) -> bool {
        matches!(provider, "webhook" | "generic")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // RFC 4231, test case 1: guards interoperability across the HMAC/SHA-2 update.
    #[test]
    fn webhook_signature_matches_rfc_4231() {
        let adapter = WebhookAdapter::new();
        let key = String::from_utf8(vec![0x0b; 20]).unwrap();
        assert_eq!(
            adapter.sign_payload("Hi There", &key),
            "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
        );
    }

    #[test]
    fn test_webhook_signature() {
        let adapter = WebhookAdapter::new();
        let signature = adapter.sign_payload(r#"{"test": "data"}"#, "secret123");
        assert!(!signature.is_empty());
        // Signature should be consistent
        let signature2 = adapter.sign_payload(r#"{"test": "data"}"#, "secret123");
        assert_eq!(signature, signature2);
    }
}
