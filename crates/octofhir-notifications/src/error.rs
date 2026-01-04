use thiserror::Error;

#[derive(Debug, Error)]
pub enum NotificationError {
    #[error("Provider not found: {0}")]
    ProviderNotFound(String),

    #[error("Provider disabled: {0}")]
    ProviderDisabled(String),

    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    #[error("Send failed: {0}")]
    SendFailed(String),

    #[error("Template not found: {0}")]
    TemplateNotFound(String),

    #[error("Recipient not found")]
    RecipientNotFound,

    #[error("Internal error: {0}")]
    Internal(String),
}
