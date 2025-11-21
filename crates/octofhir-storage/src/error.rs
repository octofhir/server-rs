//! Storage error types for the FHIR storage abstraction layer.
//!
//! This module defines all error types that can occur during storage operations.

use std::fmt;

/// Errors that can occur during storage operations.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    /// The requested resource was not found.
    #[error("Resource not found: {resource_type}/{id}")]
    NotFound {
        /// The type of resource that was not found.
        resource_type: String,
        /// The ID of the resource that was not found.
        id: String,
    },

    /// A version conflict occurred during an update operation.
    #[error("Version conflict: expected {expected}, found {actual}")]
    VersionConflict {
        /// The expected version ID.
        expected: String,
        /// The actual version ID found.
        actual: String,
    },

    /// Attempted to create a resource that already exists.
    #[error("Resource already exists: {resource_type}/{id}")]
    AlreadyExists {
        /// The type of resource that already exists.
        resource_type: String,
        /// The ID of the resource that already exists.
        id: String,
    },

    /// The resource data is invalid.
    #[error("Invalid resource: {message}")]
    InvalidResource {
        /// Description of why the resource is invalid.
        message: String,
    },

    /// An error occurred during a transaction.
    #[error("Transaction error: {message}")]
    TransactionError {
        /// Description of the transaction error.
        message: String,
    },

    /// Failed to connect to the storage backend.
    #[error("Connection error: {message}")]
    ConnectionError {
        /// Description of the connection error.
        message: String,
    },

    /// An internal storage error occurred.
    #[error("Internal error: {message}")]
    Internal {
        /// Description of the internal error.
        message: String,
    },
}

impl StorageError {
    /// Creates a new `NotFound` error.
    #[must_use]
    pub fn not_found(resource_type: impl Into<String>, id: impl Into<String>) -> Self {
        Self::NotFound {
            resource_type: resource_type.into(),
            id: id.into(),
        }
    }

    /// Creates a new `VersionConflict` error.
    #[must_use]
    pub fn version_conflict(expected: impl Into<String>, actual: impl Into<String>) -> Self {
        Self::VersionConflict {
            expected: expected.into(),
            actual: actual.into(),
        }
    }

    /// Creates a new `AlreadyExists` error.
    #[must_use]
    pub fn already_exists(resource_type: impl Into<String>, id: impl Into<String>) -> Self {
        Self::AlreadyExists {
            resource_type: resource_type.into(),
            id: id.into(),
        }
    }

    /// Creates a new `InvalidResource` error.
    #[must_use]
    pub fn invalid_resource(message: impl Into<String>) -> Self {
        Self::InvalidResource {
            message: message.into(),
        }
    }

    /// Creates a new `TransactionError` error.
    #[must_use]
    pub fn transaction_error(message: impl Into<String>) -> Self {
        Self::TransactionError {
            message: message.into(),
        }
    }

    /// Creates a new `ConnectionError` error.
    #[must_use]
    pub fn connection_error(message: impl Into<String>) -> Self {
        Self::ConnectionError {
            message: message.into(),
        }
    }

    /// Creates a new `Internal` error.
    #[must_use]
    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal {
            message: message.into(),
        }
    }

    /// Returns `true` if this is a not found error.
    #[must_use]
    pub fn is_not_found(&self) -> bool {
        matches!(self, Self::NotFound { .. })
    }

    /// Returns `true` if this is a version conflict error.
    #[must_use]
    pub fn is_version_conflict(&self) -> bool {
        matches!(self, Self::VersionConflict { .. })
    }

    /// Returns `true` if this is an already exists error.
    #[must_use]
    pub fn is_already_exists(&self) -> bool {
        matches!(self, Self::AlreadyExists { .. })
    }

    /// Returns the error category for logging/monitoring purposes.
    #[must_use]
    pub fn category(&self) -> ErrorCategory {
        match self {
            Self::NotFound { .. } => ErrorCategory::NotFound,
            Self::VersionConflict { .. } => ErrorCategory::Conflict,
            Self::AlreadyExists { .. } => ErrorCategory::Conflict,
            Self::InvalidResource { .. } => ErrorCategory::Validation,
            Self::TransactionError { .. } => ErrorCategory::Transaction,
            Self::ConnectionError { .. } => ErrorCategory::Infrastructure,
            Self::Internal { .. } => ErrorCategory::Internal,
        }
    }
}

/// Categories of storage errors for logging and monitoring.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorCategory {
    /// Resource not found.
    NotFound,
    /// Conflict (version or existence).
    Conflict,
    /// Validation error.
    Validation,
    /// Transaction-related error.
    Transaction,
    /// Infrastructure/connection error.
    Infrastructure,
    /// Internal error.
    Internal,
}

impl fmt::Display for ErrorCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => write!(f, "not_found"),
            Self::Conflict => write!(f, "conflict"),
            Self::Validation => write!(f, "validation"),
            Self::Transaction => write!(f, "transaction"),
            Self::Infrastructure => write!(f, "infrastructure"),
            Self::Internal => write!(f, "internal"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = StorageError::not_found("Patient", "123");
        assert_eq!(err.to_string(), "Resource not found: Patient/123");

        let err = StorageError::version_conflict("1", "2");
        assert_eq!(err.to_string(), "Version conflict: expected 1, found 2");

        let err = StorageError::already_exists("Patient", "456");
        assert_eq!(err.to_string(), "Resource already exists: Patient/456");
    }

    #[test]
    fn test_error_predicates() {
        let err = StorageError::not_found("Patient", "123");
        assert!(err.is_not_found());
        assert!(!err.is_version_conflict());
        assert!(!err.is_already_exists());

        let err = StorageError::version_conflict("1", "2");
        assert!(!err.is_not_found());
        assert!(err.is_version_conflict());
    }

    #[test]
    fn test_error_category() {
        assert_eq!(
            StorageError::not_found("Patient", "123").category(),
            ErrorCategory::NotFound
        );
        assert_eq!(
            StorageError::version_conflict("1", "2").category(),
            ErrorCategory::Conflict
        );
        assert_eq!(
            StorageError::already_exists("Patient", "456").category(),
            ErrorCategory::Conflict
        );
        assert_eq!(
            StorageError::invalid_resource("bad data").category(),
            ErrorCategory::Validation
        );
    }
}
