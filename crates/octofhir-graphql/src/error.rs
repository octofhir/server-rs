//! Error types for GraphQL operations.
//!
//! This module defines the error types used throughout the GraphQL layer.
//! Errors are designed to be converted to appropriate HTTP responses and
//! GraphQL error extensions.

use std::fmt;

/// Errors that can occur during GraphQL operations.
#[derive(Debug)]
pub enum GraphQLError {
    /// Schema is still being built - client should retry.
    SchemaInitializing,

    /// Schema build failed.
    SchemaBuildFailed(String),

    /// Invalid query syntax.
    InvalidQuery(String),

    /// Query complexity exceeded.
    ComplexityExceeded {
        /// Actual complexity of the query.
        actual: usize,
        /// Maximum allowed complexity.
        max: usize,
    },

    /// Query depth exceeded.
    DepthExceeded {
        /// Actual depth of the query.
        actual: usize,
        /// Maximum allowed depth.
        max: usize,
    },

    /// Authentication required.
    Unauthorized(String),

    /// Permission denied.
    Forbidden(String),

    /// Resource not found.
    NotFound {
        /// Resource type.
        resource_type: String,
        /// Resource ID.
        resource_id: String,
    },

    /// Storage error.
    Storage(String),

    /// Validation error.
    Validation(String),

    /// Internal server error.
    Internal(String),
}

impl fmt::Display for GraphQLError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SchemaInitializing => {
                write!(f, "GraphQL schema is initializing, please retry")
            }
            Self::SchemaBuildFailed(msg) => {
                write!(f, "Failed to build GraphQL schema: {msg}")
            }
            Self::InvalidQuery(msg) => {
                write!(f, "Invalid GraphQL query: {msg}")
            }
            Self::ComplexityExceeded { actual, max } => {
                write!(f, "Query complexity {actual} exceeds maximum allowed {max}")
            }
            Self::DepthExceeded { actual, max } => {
                write!(f, "Query depth {actual} exceeds maximum allowed {max}")
            }
            Self::Unauthorized(msg) => {
                write!(f, "Unauthorized: {msg}")
            }
            Self::Forbidden(msg) => {
                write!(f, "Forbidden: {msg}")
            }
            Self::NotFound {
                resource_type,
                resource_id,
            } => {
                write!(f, "{resource_type}/{resource_id} not found")
            }
            Self::Storage(msg) => {
                write!(f, "Storage error: {msg}")
            }
            Self::Validation(msg) => {
                write!(f, "Validation error: {msg}")
            }
            Self::Internal(msg) => {
                write!(f, "Internal error: {msg}")
            }
        }
    }
}

impl std::error::Error for GraphQLError {}

impl GraphQLError {
    /// Returns the HTTP status code for this error.
    #[must_use]
    pub fn status_code(&self) -> u16 {
        match self {
            Self::SchemaInitializing => 503,
            Self::SchemaBuildFailed(_) => 500,
            Self::InvalidQuery(_) => 400,
            Self::ComplexityExceeded { .. } | Self::DepthExceeded { .. } => 400,
            Self::Unauthorized(_) => 401,
            Self::Forbidden(_) => 403,
            Self::NotFound { .. } => 404,
            Self::Storage(_) => 500,
            Self::Validation(_) => 400,
            Self::Internal(_) => 500,
        }
    }

    /// Returns the error code for GraphQL error extensions.
    #[must_use]
    pub fn error_code(&self) -> &'static str {
        match self {
            Self::SchemaInitializing => "SCHEMA_INITIALIZING",
            Self::SchemaBuildFailed(_) => "SCHEMA_BUILD_FAILED",
            Self::InvalidQuery(_) => "INVALID_QUERY",
            Self::ComplexityExceeded { .. } => "COMPLEXITY_EXCEEDED",
            Self::DepthExceeded { .. } => "DEPTH_EXCEEDED",
            Self::Unauthorized(_) => "UNAUTHORIZED",
            Self::Forbidden(_) => "FORBIDDEN",
            Self::NotFound { .. } => "NOT_FOUND",
            Self::Storage(_) => "STORAGE_ERROR",
            Self::Validation(_) => "VALIDATION_ERROR",
            Self::Internal(_) => "INTERNAL_ERROR",
        }
    }

    /// Returns the Retry-After header value in seconds, if applicable.
    #[must_use]
    pub fn retry_after(&self) -> Option<u32> {
        match self {
            Self::SchemaInitializing => Some(5),
            _ => None,
        }
    }

    /// Converts the error to a FHIR OperationOutcome JSON.
    #[must_use]
    pub fn to_operation_outcome(&self) -> serde_json::Value {
        let severity = match self {
            Self::SchemaInitializing => "information",
            Self::NotFound { .. } => "warning",
            _ => "error",
        };

        let code = match self {
            Self::SchemaInitializing => "transient",
            Self::InvalidQuery(_)
            | Self::ComplexityExceeded { .. }
            | Self::DepthExceeded { .. } => "invalid",
            Self::Unauthorized(_) => "login",
            Self::Forbidden(_) => "forbidden",
            Self::NotFound { .. } => "not-found",
            Self::Storage(_) | Self::SchemaBuildFailed(_) | Self::Internal(_) => "exception",
            Self::Validation(_) => "structure",
        };

        serde_json::json!({
            "resourceType": "OperationOutcome",
            "issue": [{
                "severity": severity,
                "code": code,
                "diagnostics": self.to_string()
            }]
        })
    }
}

impl From<octofhir_storage::StorageError> for GraphQLError {
    fn from(err: octofhir_storage::StorageError) -> Self {
        match err {
            octofhir_storage::StorageError::NotFound { resource_type, id } => Self::NotFound {
                resource_type,
                resource_id: id,
            },
            other => Self::Storage(other.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_status_codes() {
        assert_eq!(GraphQLError::SchemaInitializing.status_code(), 503);
        assert_eq!(GraphQLError::InvalidQuery("test".into()).status_code(), 400);
        assert_eq!(GraphQLError::Unauthorized("test".into()).status_code(), 401);
        assert_eq!(GraphQLError::Forbidden("test".into()).status_code(), 403);
        assert_eq!(
            GraphQLError::NotFound {
                resource_type: "Patient".into(),
                resource_id: "123".into()
            }
            .status_code(),
            404
        );
    }

    #[test]
    fn test_error_codes() {
        assert_eq!(
            GraphQLError::SchemaInitializing.error_code(),
            "SCHEMA_INITIALIZING"
        );
        assert_eq!(
            GraphQLError::ComplexityExceeded {
                actual: 100,
                max: 50
            }
            .error_code(),
            "COMPLEXITY_EXCEEDED"
        );
    }

    #[test]
    fn test_retry_after() {
        assert_eq!(GraphQLError::SchemaInitializing.retry_after(), Some(5));
        assert_eq!(
            GraphQLError::InvalidQuery("test".into()).retry_after(),
            None
        );
    }

    #[test]
    fn test_operation_outcome() {
        let err = GraphQLError::NotFound {
            resource_type: "Patient".into(),
            resource_id: "123".into(),
        };
        let outcome = err.to_operation_outcome();

        assert_eq!(outcome["resourceType"], "OperationOutcome");
        assert_eq!(outcome["issue"][0]["severity"], "warning");
        assert_eq!(outcome["issue"][0]["code"], "not-found");
    }
}
