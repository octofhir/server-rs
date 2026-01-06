//! Error types for the PostgreSQL storage backend.

use octofhir_storage::StorageError;
use sqlx_core::error::Error as SqlxError;

/// PostgreSQL error code for undefined table (42P01).
pub const PG_UNDEFINED_TABLE: &str = "42P01";

/// PostgreSQL error code for undefined function/operator (42883).
pub const PG_UNDEFINED_FUNCTION: &str = "42883";

/// Checks if a sqlx error has a specific PostgreSQL error code.
pub fn has_pg_error_code(err: &SqlxError, code: &str) -> bool {
    if let SqlxError::Database(db_err) = err {
        db_err.code().as_deref() == Some(code)
    } else {
        false
    }
}

/// Checks if a sqlx error is "undefined table" (42P01).
pub fn is_undefined_table(err: &SqlxError) -> bool {
    has_pg_error_code(err, PG_UNDEFINED_TABLE)
}

/// Errors specific to the PostgreSQL storage backend.
#[derive(Debug, thiserror::Error)]
pub enum PostgresError {
    /// Database connection error.
    #[error("Database connection error: {0}")]
    Connection(#[from] sqlx_core::error::Error),

    /// Migration error (generic string for compatibility with different migration tools).
    #[error("Migration error: {0}")]
    Migration(String),

    /// Configuration error.
    #[error("Configuration error: {message}")]
    Config { message: String },

    /// Pool error.
    #[error("Pool error: {message}")]
    Pool { message: String },
}

impl PostgresError {
    /// Creates a new configuration error.
    #[must_use]
    pub fn config(message: impl Into<String>) -> Self {
        Self::Config {
            message: message.into(),
        }
    }

    /// Creates a new pool error.
    #[must_use]
    pub fn pool(message: impl Into<String>) -> Self {
        Self::Pool {
            message: message.into(),
        }
    }
}

impl From<PostgresError> for StorageError {
    fn from(err: PostgresError) -> Self {
        match err {
            PostgresError::Connection(e) => StorageError::connection_error(e.to_string()),
            PostgresError::Migration(e) => StorageError::internal(format!("Migration error: {e}")),
            PostgresError::Config { message } => {
                StorageError::internal(format!("Configuration error: {message}"))
            }
            PostgresError::Pool { message } => {
                StorageError::connection_error(format!("Pool error: {message}"))
            }
        }
    }
}

/// Result type alias for PostgreSQL operations.
pub type Result<T> = std::result::Result<T, PostgresError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = PostgresError::config("invalid URL");
        assert!(err.to_string().contains("Configuration error"));

        let err = PostgresError::pool("pool exhausted");
        assert!(err.to_string().contains("Pool error"));
    }

    #[test]
    fn test_conversion_to_storage_error() {
        let pg_err = PostgresError::config("test error");
        let storage_err: StorageError = pg_err.into();
        assert!(matches!(storage_err, StorageError::Internal { .. }));
    }
}
