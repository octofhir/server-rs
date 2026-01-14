//! Subscription error types.

use std::fmt;

use octofhir_core::error::CoreError;

/// Result type for subscription operations.
pub type SubscriptionResult<T> = Result<T, SubscriptionError>;

/// Errors that can occur during subscription operations.
#[derive(Debug)]
pub enum SubscriptionError {
    /// Database error
    Database(sqlx_core::Error),

    /// Storage error from FHIR storage layer
    Storage(String),

    /// Topic not found
    TopicNotFound(String),

    /// Subscription not found
    SubscriptionNotFound(String),

    /// Invalid subscription configuration
    InvalidSubscription(String),

    /// Invalid topic configuration
    InvalidTopic(String),

    /// FHIRPath evaluation error
    FhirPathError(String),

    /// Channel delivery error
    DeliveryError(String),

    /// Validation error
    ValidationError(String),

    /// Resource parsing error
    ParseError(String),

    /// Internal error
    Internal(String),
}

impl fmt::Display for SubscriptionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Database(e) => write!(f, "Database error: {e}"),
            Self::Storage(msg) => write!(f, "Storage error: {msg}"),
            Self::TopicNotFound(url) => write!(f, "Subscription topic not found: {url}"),
            Self::SubscriptionNotFound(id) => write!(f, "Subscription not found: {id}"),
            Self::InvalidSubscription(msg) => write!(f, "Invalid subscription: {msg}"),
            Self::InvalidTopic(msg) => write!(f, "Invalid topic: {msg}"),
            Self::FhirPathError(msg) => write!(f, "FHIRPath error: {msg}"),
            Self::DeliveryError(msg) => write!(f, "Delivery error: {msg}"),
            Self::ValidationError(msg) => write!(f, "Validation error: {msg}"),
            Self::ParseError(msg) => write!(f, "Parse error: {msg}"),
            Self::Internal(msg) => write!(f, "Internal error: {msg}"),
        }
    }
}

impl std::error::Error for SubscriptionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Database(e) => Some(e),
            _ => None,
        }
    }
}

impl From<sqlx_core::Error> for SubscriptionError {
    fn from(e: sqlx_core::Error) -> Self {
        Self::Database(e)
    }
}

impl From<CoreError> for SubscriptionError {
    fn from(e: CoreError) -> Self {
        Self::Storage(e.to_string())
    }
}

impl From<serde_json::Error> for SubscriptionError {
    fn from(e: serde_json::Error) -> Self {
        Self::ParseError(e.to_string())
    }
}
