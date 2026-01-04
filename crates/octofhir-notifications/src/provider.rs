use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// NotificationProvider resource
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationProvider {
    pub resource_type: String, // "NotificationProvider"
    pub id: String,
    pub name: String,

    /// Provider type: email, telegram, webhook
    #[serde(rename = "type")]
    pub provider_type: ProviderType,

    /// Specific provider: sendgrid, smtp, telegram, generic
    pub provider: String,

    /// Provider-specific configuration
    pub config: ProviderConfig,

    /// Is provider active
    #[serde(default = "default_true")]
    pub active: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderType {
    Email,
    Sms,
    Telegram,
    Webhook,
    Push,
}

/// Provider configuration (credentials are encrypted in DB)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderConfig {
    // Email (SendGrid)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,

    // Email (SMTP)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub smtp_host: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub smtp_port: Option<u16>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub smtp_username: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub smtp_password: Option<String>,

    // Telegram
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bot_token: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub bot_username: Option<String>,

    // Webhook
    #[serde(skip_serializing_if = "Option::is_none")]
    pub webhook_url: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub webhook_secret: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub webhook_headers: Option<HashMap<String, String>>,
}

/// Fields that should be encrypted
pub const ENCRYPTED_FIELDS: &[&str] = &["api_key", "smtp_password", "bot_token", "webhook_secret"];

/// Preprocess provider before saving (encrypt secrets)
pub fn preprocess_provider<E>(
    provider: &mut NotificationProvider,
    encrypt_fn: impl Fn(&str) -> Result<String, E>,
) -> Result<(), E> {
    if let Some(ref api_key) = provider.config.api_key
        && !api_key.starts_with("$encrypted$")
    {
        provider.config.api_key = Some(encrypt_fn(api_key)?);
    }
    if let Some(ref password) = provider.config.smtp_password
        && !password.starts_with("$encrypted$")
    {
        provider.config.smtp_password = Some(encrypt_fn(password)?);
    }
    if let Some(ref token) = provider.config.bot_token
        && !token.starts_with("$encrypted$")
    {
        provider.config.bot_token = Some(encrypt_fn(token)?);
    }
    if let Some(ref secret) = provider.config.webhook_secret
        && !secret.starts_with("$encrypted$")
    {
        provider.config.webhook_secret = Some(encrypt_fn(secret)?);
    }
    Ok(())
}

/// Mask secrets in response
pub fn mask_secrets(mut provider: NotificationProvider) -> NotificationProvider {
    if provider.config.api_key.is_some() {
        provider.config.api_key = Some("***".to_string());
    }
    if provider.config.smtp_password.is_some() {
        provider.config.smtp_password = Some("***".to_string());
    }
    if provider.config.bot_token.is_some() {
        provider.config.bot_token = Some("***".to_string());
    }
    if provider.config.webhook_secret.is_some() {
        provider.config.webhook_secret = Some("***".to_string());
    }
    provider
}
