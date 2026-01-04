//! Gateway-specific error types.

use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;
use std::fmt;

/// Gateway-specific errors.
#[derive(Debug)]
pub enum GatewayError {
    /// Route not found for the given method and path.
    RouteNotFound { method: String, path: String },

    /// Invalid gateway configuration.
    InvalidConfig(String),

    /// Error loading routes from storage.
    StorageError(String),

    /// Error executing proxy request.
    ProxyError(String),

    /// Error executing SQL query.
    SqlError(String),

    /// Error evaluating FHIRPath expression.
    FhirPathError(String),

    /// Handler not found for the operation.
    HandlerNotFound(String),

    /// Bad request (400).
    BadRequest(String),

    /// Authentication required (401 Unauthorized).
    Unauthorized(String),

    /// Access forbidden (403 Forbidden).
    Forbidden(String),

    /// Generic internal error.
    InternalError(String),
}

impl fmt::Display for GatewayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RouteNotFound { method, path } => {
                write!(f, "No route found for {} {}", method, path)
            }
            Self::InvalidConfig(msg) => write!(f, "Invalid gateway configuration: {}", msg),
            Self::StorageError(msg) => write!(f, "Storage error: {}", msg),
            Self::ProxyError(msg) => write!(f, "Proxy error: {}", msg),
            Self::SqlError(msg) => write!(f, "SQL error: {}", msg),
            Self::FhirPathError(msg) => write!(f, "FHIRPath error: {}", msg),
            Self::HandlerNotFound(msg) => write!(f, "Handler not found: {}", msg),
            Self::BadRequest(msg) => write!(f, "Bad request: {}", msg),
            Self::Unauthorized(msg) => write!(f, "Authentication required: {}", msg),
            Self::Forbidden(msg) => write!(f, "Access forbidden: {}", msg),
            Self::InternalError(msg) => write!(f, "Internal error: {}", msg),
        }
    }
}

impl std::error::Error for GatewayError {}

impl IntoResponse for GatewayError {
    fn into_response(self) -> Response {
        let (status, severity, code) = match &self {
            Self::RouteNotFound { .. } => (StatusCode::NOT_FOUND, "error", "not-found"),
            Self::InvalidConfig(_) => (StatusCode::INTERNAL_SERVER_ERROR, "error", "invalid"),
            Self::StorageError(_) => (StatusCode::INTERNAL_SERVER_ERROR, "error", "exception"),
            Self::ProxyError(_) => (StatusCode::BAD_GATEWAY, "error", "exception"),
            Self::SqlError(_) => (StatusCode::INTERNAL_SERVER_ERROR, "error", "exception"),
            Self::FhirPathError(_) => (StatusCode::BAD_REQUEST, "error", "invalid"),
            Self::BadRequest(_) => (StatusCode::BAD_REQUEST, "error", "invalid"),
            Self::HandlerNotFound(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, "error", "not-supported")
            }
            Self::Unauthorized(_) => (StatusCode::UNAUTHORIZED, "error", "login"),
            Self::Forbidden(_) => (StatusCode::FORBIDDEN, "error", "forbidden"),
            Self::InternalError(_) => (StatusCode::INTERNAL_SERVER_ERROR, "error", "exception"),
        };

        let operation_outcome = json!({
            "resourceType": "OperationOutcome",
            "issue": [{
                "severity": severity,
                "code": code,
                "diagnostics": self.to_string()
            }]
        });

        (status, Json(operation_outcome)).into_response()
    }
}
