//! Error types for the PostgreSQL storage backend.

use octofhir_storage::StorageError;

/// Errors specific to the PostgreSQL storage backend.
#[derive(Debug, thiserror::Error)]
pub enum PostgresError {
    /// Database connection error.
    #[error("Database connection error: {0}")]
    Connection(#[from] sqlx_core::error::Error),

    /// Migration error.
    #[error("Migration error: {0}")]
    Migration(#[from] sqlx_core::migrate::MigrateError),

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
