//! Operation handler trait and error types.
//!
//! This module defines the trait that must be implemented by concrete
//! operation handlers, as well as the error types for operation failures.

use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;

use crate::server::AppState;

/// Error type for FHIR operation failures.
#[derive(Debug, thiserror::Error)]
pub enum OperationError {
    /// Invalid or missing parameters
    #[error("Invalid parameters: {0}")]
    InvalidParameters(String),

    /// Resource not found
    #[error("Resource not found: {0}")]
    NotFound(String),

    /// Operation not supported at this level
    #[error("Operation not supported: {0}")]
    NotSupported(String),

    /// Internal server error
    #[error("Internal error: {0}")]
    Internal(String),

    /// Validation failed
    #[error("Validation failed")]
    ValidationFailed(Value),
}

/// Trait for implementing FHIR operations.
///
/// Each operation handler implements this trait to provide the logic for
/// executing the operation at different levels (system, type, instance).
///
/// # Implementation Notes
///
/// - Implement only the methods that are relevant for your operation's level(s)
/// - Default implementations return `NotSupported` errors
/// - The `code` method returns the operation code without the `$` prefix
#[async_trait]
pub trait OperationHandler: Send + Sync {
    /// Returns the operation code (without the `$` prefix).
    fn code(&self) -> &str;

    /// Handles the operation at system level (e.g., `GET /$operation`).
    ///
    /// Override this method if your operation supports system-level invocation.
    async fn handle_system(
        &self,
        _state: &AppState,
        _params: &Value,
    ) -> Result<Value, OperationError> {
        Err(OperationError::NotSupported(format!(
            "Operation ${} is not supported at system level",
            self.code()
        )))
    }

    /// Handles the operation at type level (e.g., `GET /Patient/$operation`).
    ///
    /// Override this method if your operation supports type-level invocation.
    async fn handle_type(
        &self,
        _state: &AppState,
        _resource_type: &str,
        _params: &Value,
    ) -> Result<Value, OperationError> {
        Err(OperationError::NotSupported(format!(
            "Operation ${} is not supported at type level",
            self.code()
        )))
    }

    /// Handles the operation at instance level (e.g., `GET /Patient/123/$operation`).
    ///
    /// Override this method if your operation supports instance-level invocation.
    async fn handle_instance(
        &self,
        _state: &AppState,
        _resource_type: &str,
        _id: &str,
        _params: &Value,
    ) -> Result<Value, OperationError> {
        Err(OperationError::NotSupported(format!(
            "Operation ${} is not supported at instance level",
            self.code()
        )))
    }
}

/// Type alias for a boxed operation handler.
pub type DynOperationHandler = Arc<dyn OperationHandler>;

impl From<OperationError> for octofhir_api::ApiError {
    fn from(err: OperationError) -> Self {
        match err {
            OperationError::InvalidParameters(msg) => octofhir_api::ApiError::bad_request(msg),
            OperationError::NotFound(msg) => octofhir_api::ApiError::not_found(msg),
            OperationError::NotSupported(msg) => octofhir_api::ApiError::bad_request(msg),
            OperationError::Internal(msg) => octofhir_api::ApiError::internal(msg),
            OperationError::ValidationFailed(_outcome) => {
                // ValidationFailed contains an OperationOutcome - convert to BadRequest
                // The outcome will be returned as the error message
                octofhir_api::ApiError::bad_request("Validation failed")
            }
        }
    }
}
