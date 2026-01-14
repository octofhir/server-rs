//! Email delivery channel for FHIR Subscriptions.
//!
//! Sends subscription notifications via email using configured SMTP or API providers.

use async_trait::async_trait;
use serde_json::json;

use super::{DeliveryChannel, SubscriptionResult};
use crate::subscriptions::types::{ActiveSubscription, DeliveryResult, SubscriptionEvent};

/// Email delivery channel.
///
/// Uses the configured email provider (SMTP or SendGrid) to send
/// subscription notifications.
pub struct EmailChannel {
    /// SMTP host for email delivery
    smtp_host: Option<String>,
    /// SMTP port (default 587)
    smtp_port: u16,
    /// SMTP username
    smtp_username: Option<String>,
    /// SMTP password
    smtp_password: Option<String>,
    /// From email address
    from_email: String,
    /// HTTP client for API-based providers
    http_client: reqwest::Client,
    /// SendGrid API key (optional, alternative to SMTP)
    sendgrid_api_key: Option<String>,
}

impl EmailChannel {
    /// Create a new email channel with SMTP configuration.
    pub fn with_smtp(
        host: String,
        port: u16,
        username: Option<String>,
        password: Option<String>,
        from_email: String,
    ) -> Self {
        Self {
            smtp_host: Some(host),
            smtp_port: port,
            smtp_username: username,
            smtp_password: password,
            from_email,
            http_client: reqwest::Client::new(),
            sendgrid_api_key: None,
        }
    }

    /// Create a new email channel with SendGrid configuration.
    pub fn with_sendgrid(api_key: String, from_email: String) -> Self {
        Self {
            smtp_host: None,
            smtp_port: 587,
            smtp_username: None,
            smtp_password: None,
            from_email,
            http_client: reqwest::Client::new(),
            sendgrid_api_key: Some(api_key),
        }
    }

    /// Build the email subject line.
    fn build_subject(_subscription: &ActiveSubscription, event: &SubscriptionEvent) -> String {
        let resource_type = event.focus_resource_type.as_deref().unwrap_or("Resource");
        let trigger = event
            .focus_event
            .as_ref()
            .map(|e| e.as_str())
            .unwrap_or("event");
        format!(
            "FHIR Subscription Notification: {} {}",
            resource_type, trigger
        )
    }

    /// Build the email body (plain text).
    fn build_body(subscription: &ActiveSubscription, event: &SubscriptionEvent) -> String {
        let resource_type = event.focus_resource_type.as_deref().unwrap_or("Resource");
        let resource_id = event.focus_resource_id.as_deref().unwrap_or("unknown");
        let trigger = event
            .focus_event
            .as_ref()
            .map(|e| e.as_str())
            .unwrap_or("event");
        let timestamp = event
            .created_at
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_else(|_| "unknown".to_string());

        format!(
            r#"FHIR Subscription Notification

Subscription ID: {}
Topic: {}

Event Details:
- Resource Type: {}
- Resource ID: {}
- Trigger: {}
- Event Number: {}
- Timestamp: {}

This is an automated notification from your FHIR server subscription.
"#,
            subscription.id,
            subscription.topic_url,
            resource_type,
            resource_id,
            trigger,
            event.event_number,
            timestamp
        )
    }

    /// Build the email body (HTML).
    fn build_html_body(subscription: &ActiveSubscription, event: &SubscriptionEvent) -> String {
        let resource_type = event.focus_resource_type.as_deref().unwrap_or("Resource");
        let resource_id = event.focus_resource_id.as_deref().unwrap_or("unknown");
        let trigger = event
            .focus_event
            .as_ref()
            .map(|e| e.as_str())
            .unwrap_or("event");
        let timestamp = event
            .created_at
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_else(|_| "unknown".to_string());

        format!(
            r#"<!DOCTYPE html>
<html>
<head>
    <style>
        body {{ font-family: Arial, sans-serif; max-width: 600px; margin: 0 auto; padding: 20px; }}
        .header {{ background-color: #4a90d9; color: white; padding: 20px; border-radius: 8px 8px 0 0; }}
        .content {{ background-color: #f5f5f5; padding: 20px; border-radius: 0 0 8px 8px; }}
        .field {{ margin-bottom: 10px; }}
        .label {{ font-weight: bold; color: #333; }}
        .value {{ color: #666; }}
        .footer {{ margin-top: 20px; font-size: 12px; color: #999; }}
    </style>
</head>
<body>
    <div class="header">
        <h2>FHIR Subscription Notification</h2>
    </div>
    <div class="content">
        <div class="field">
            <span class="label">Subscription ID:</span>
            <span class="value">{}</span>
        </div>
        <div class="field">
            <span class="label">Topic:</span>
            <span class="value">{}</span>
        </div>
        <h3>Event Details</h3>
        <div class="field">
            <span class="label">Resource Type:</span>
            <span class="value">{}</span>
        </div>
        <div class="field">
            <span class="label">Resource ID:</span>
            <span class="value">{}</span>
        </div>
        <div class="field">
            <span class="label">Trigger:</span>
            <span class="value">{}</span>
        </div>
        <div class="field">
            <span class="label">Event Number:</span>
            <span class="value">{}</span>
        </div>
        <div class="field">
            <span class="label">Timestamp:</span>
            <span class="value">{}</span>
        </div>
        <div class="footer">
            This is an automated notification from your FHIR server subscription.
        </div>
    </div>
</body>
</html>"#,
            subscription.id,
            subscription.topic_url,
            resource_type,
            resource_id,
            trigger,
            event.event_number,
            timestamp
        )
    }

    /// Send email via SendGrid API.
    async fn send_via_sendgrid(
        &self,
        to_email: &str,
        subject: &str,
        body: &str,
        html_body: &str,
    ) -> SubscriptionResult<DeliveryResult> {
        let api_key = self.sendgrid_api_key.as_ref().ok_or_else(|| {
            crate::subscriptions::error::SubscriptionError::DeliveryError(
                "SendGrid API key not configured".to_string(),
            )
        })?;

        let start = std::time::Instant::now();

        let payload = json!({
            "personalizations": [{
                "to": [{"email": to_email}]
            }],
            "from": {"email": &self.from_email},
            "subject": subject,
            "content": [
                {"type": "text/plain", "value": body},
                {"type": "text/html", "value": html_body}
            ]
        });

        let response = self
            .http_client
            .post("https://api.sendgrid.com/v3/mail/send")
            .bearer_auth(api_key)
            .json(&payload)
            .send()
            .await;

        let elapsed = start.elapsed().as_millis() as u32;

        match response {
            Ok(resp) => {
                if resp.status().is_success() {
                    Ok(DeliveryResult {
                        success: true,
                        http_status: Some(resp.status().as_u16()),
                        response_time_ms: elapsed,
                        error: None,
                        error_code: None,
                    })
                } else {
                    let status = resp.status().as_u16();
                    let error = resp.text().await.unwrap_or_default();
                    Ok(DeliveryResult {
                        success: false,
                        http_status: Some(status),
                        response_time_ms: elapsed,
                        error: Some(error),
                        error_code: Some(format!("HTTP_{}", status)),
                    })
                }
            }
            Err(e) => Ok(DeliveryResult {
                success: false,
                http_status: None,
                response_time_ms: elapsed,
                error: Some(e.to_string()),
                error_code: Some("NETWORK_ERROR".to_string()),
            }),
        }
    }

    /// Send email via SMTP.
    async fn send_via_smtp(
        &self,
        to_email: &str,
        subject: &str,
        body: &str,
    ) -> SubscriptionResult<DeliveryResult> {
        use lettre::{
            AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor,
            message::header::ContentType, transport::smtp::authentication::Credentials,
        };

        let host = self.smtp_host.as_ref().ok_or_else(|| {
            crate::subscriptions::error::SubscriptionError::DeliveryError(
                "SMTP host not configured".to_string(),
            )
        })?;

        let start = std::time::Instant::now();

        let email = Message::builder()
            .from(self.from_email.parse().map_err(|e| {
                crate::subscriptions::error::SubscriptionError::DeliveryError(format!(
                    "Invalid from email: {}",
                    e
                ))
            })?)
            .to(to_email.parse().map_err(|e| {
                crate::subscriptions::error::SubscriptionError::DeliveryError(format!(
                    "Invalid to email: {}",
                    e
                ))
            })?)
            .subject(subject)
            .header(ContentType::TEXT_PLAIN)
            .body(body.to_string())
            .map_err(|e| {
                crate::subscriptions::error::SubscriptionError::DeliveryError(e.to_string())
            })?;

        let mut mailer_builder =
            AsyncSmtpTransport::<Tokio1Executor>::relay(host).map_err(|e| {
                crate::subscriptions::error::SubscriptionError::DeliveryError(e.to_string())
            })?;

        mailer_builder = mailer_builder.port(self.smtp_port);

        if let (Some(username), Some(password)) = (&self.smtp_username, &self.smtp_password) {
            mailer_builder =
                mailer_builder.credentials(Credentials::new(username.clone(), password.clone()));
        }

        let mailer = mailer_builder.build();
        let elapsed = start.elapsed().as_millis() as u32;

        match mailer.send(email).await {
            Ok(_) => Ok(DeliveryResult {
                success: true,
                http_status: None,
                response_time_ms: elapsed,
                error: None,
                error_code: None,
            }),
            Err(e) => Ok(DeliveryResult {
                success: false,
                http_status: None,
                response_time_ms: elapsed,
                error: Some(e.to_string()),
                error_code: Some("SMTP_ERROR".to_string()),
            }),
        }
    }
}

#[async_trait]
impl DeliveryChannel for EmailChannel {
    fn name(&self) -> &str {
        "email"
    }

    async fn deliver(
        &self,
        subscription: &ActiveSubscription,
        event: &SubscriptionEvent,
    ) -> SubscriptionResult<DeliveryResult> {
        // Get the email address from subscription contact or channel endpoint
        let to_email = subscription.contact.as_ref().ok_or_else(|| {
            crate::subscriptions::error::SubscriptionError::DeliveryError(
                "No email address configured for subscription".to_string(),
            )
        })?;

        let subject = Self::build_subject(subscription, event);
        let body = Self::build_body(subscription, event);
        let html_body = Self::build_html_body(subscription, event);

        if self.sendgrid_api_key.is_some() {
            self.send_via_sendgrid(to_email, &subject, &body, &html_body)
                .await
        } else if self.smtp_host.is_some() {
            self.send_via_smtp(to_email, &subject, &body).await
        } else {
            Err(
                crate::subscriptions::error::SubscriptionError::DeliveryError(
                    "No email provider configured".to_string(),
                ),
            )
        }
    }
}
