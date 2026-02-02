//! Error types for CQL service

use thiserror::Error;

/// Result type for CQL operations
pub type CqlResult<T> = Result<T, CqlError>;

/// Errors that can occur during CQL operations
#[derive(Debug, Error)]
pub enum CqlError {
    /// CQL parsing error
    #[error("CQL parse error: {0}")]
    ParseError(String),

    /// CQL evaluation error
    #[error("CQL evaluation error: {0}")]
    EvaluationError(String),

    /// Library not found
    #[error("Library not found: {0}")]
    LibraryNotFound(String),

    /// Library compilation error
    #[error("Library compilation error: {0}")]
    CompilationError(String),

    /// Operation timeout
    #[error("Operation timeout: {0}")]
    Timeout(String),

    /// Data provider error
    #[error("Data provider error: {0}")]
    DataProviderError(String),

    /// Terminology provider error
    #[error("Terminology provider error: {0}")]
    TerminologyError(String),

    /// Storage error
    #[error("Storage error: {0}")]
    StorageError(#[from] octofhir_storage::StorageError),

    /// Serialization error
    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),

    /// Invalid parameter
    #[error("Invalid parameter: {0}")]
    InvalidParameter(String),

    /// Cache error
    #[error("Cache error: {0}")]
    CacheError(String),

    /// Generic error
    #[error("{0}")]
    Other(String),
}

impl From<anyhow::Error> for CqlError {
    fn from(err: anyhow::Error) -> Self {
        CqlError::Other(err.to_string())
    }
}

impl From<String> for CqlError {
    fn from(err: String) -> Self {
        CqlError::Other(err)
    }
}

impl From<&str> for CqlError {
    fn from(err: &str) -> Self {
        CqlError::Other(err.to_string())
    }
}
