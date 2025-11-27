//! PostgreSQL storage backend for OctoFHIR Auth
//!
//! Provides persistent storage for:
//!
//! - OAuth clients (Client resource)
//! - Authorization sessions (Session resource)
//! - Refresh tokens (RefreshToken resource)
//! - Revoked access token JTIs (for token revocation per RFC 7009)
//! - Access policies (AccessPolicy resource)
//! - Users (User resource)
//!
//! All auth resources are stored as standard FHIR resources in the public schema.
//! Tables are created dynamically by the server's SchemaManager when resources
//! are first stored.
//!
//! # Example
//!
//! ```ignore
//! use octofhir_auth_postgres::PostgresAuthStorage;
//!
//! // Create storage with connection pool
//! let storage = PostgresAuthStorage::connect("postgres://localhost/octofhir").await?;
//!
//! // Use client storage
//! let client_storage = storage.clients();
//! let client = client_storage.find_by_client_id("my-app").await?;
//! ```

pub mod client;
pub mod policy;
pub mod revoked_token;
pub mod session;
pub mod token;
pub mod user;

use std::sync::Arc;

use sqlx_core::pool::Pool;
use sqlx_postgres::Postgres;

/// PostgreSQL connection pool type alias.
pub type PgPool = Pool<Postgres>;

pub use client::{ClientStorage, PostgresClientStorage};
pub use policy::PolicyStorage;
pub use revoked_token::RevokedTokenStorage;
pub use session::SessionStorage;
pub use token::TokenStorage;
pub use user::UserStorage;

// =============================================================================
// Error Types
// =============================================================================

/// Errors that can occur during auth storage operations.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    /// Database operation failed.
    #[error("Database error: {0}")]
    Database(#[from] sqlx_core::Error),

    /// Requested resource was not found.
    #[error("Not found: {0}")]
    NotFound(String),

    /// Resource already exists (conflict).
    #[error("Conflict: {0}")]
    Conflict(String),

    /// Serialization/deserialization failed.
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// Invalid input data.
    #[error("Invalid input: {0}")]
    InvalidInput(String),
}

impl StorageError {
    // -------------------------------------------------------------------------
    // Constructor Methods
    // -------------------------------------------------------------------------

    /// Create a `NotFound` error.
    #[must_use]
    pub fn not_found(resource: impl Into<String>) -> Self {
        Self::NotFound(resource.into())
    }

    /// Create a `Conflict` error.
    #[must_use]
    pub fn conflict(message: impl Into<String>) -> Self {
        Self::Conflict(message.into())
    }

    /// Create an `InvalidInput` error.
    #[must_use]
    pub fn invalid_input(message: impl Into<String>) -> Self {
        Self::InvalidInput(message.into())
    }

    // -------------------------------------------------------------------------
    // Predicate Methods
    // -------------------------------------------------------------------------

    /// Returns `true` if this is a `NotFound` error.
    #[must_use]
    pub fn is_not_found(&self) -> bool {
        matches!(self, Self::NotFound(_))
    }

    /// Returns `true` if this is a `Conflict` error.
    #[must_use]
    pub fn is_conflict(&self) -> bool {
        matches!(self, Self::Conflict(_))
    }

    /// Returns `true` if this is a database error.
    #[must_use]
    pub fn is_database_error(&self) -> bool {
        matches!(self, Self::Database(_))
    }

    /// Returns `true` if this is a serialization error.
    #[must_use]
    pub fn is_serialization_error(&self) -> bool {
        matches!(self, Self::Serialization(_))
    }

    /// Returns `true` if this is an invalid input error.
    #[must_use]
    pub fn is_invalid_input(&self) -> bool {
        matches!(self, Self::InvalidInput(_))
    }

    /// Returns `true` if this is a client error (4xx equivalent).
    #[must_use]
    pub fn is_client_error(&self) -> bool {
        matches!(
            self,
            Self::NotFound(_) | Self::Conflict(_) | Self::InvalidInput(_)
        )
    }

    /// Returns `true` if this is a server error (5xx equivalent).
    #[must_use]
    pub fn is_server_error(&self) -> bool {
        matches!(self, Self::Database(_) | Self::Serialization(_))
    }
}

/// Result type for storage operations.
pub type StorageResult<T> = Result<T, StorageError>;

// =============================================================================
// PostgreSQL Auth Storage
// =============================================================================

/// PostgreSQL storage backend for authentication data.
///
/// This struct holds a connection pool and provides access to specialized
/// storage types for different auth entities (clients, sessions, tokens, etc.).
#[derive(Debug, Clone)]
pub struct PostgresAuthStorage {
    pool: Arc<PgPool>,
}

impl PostgresAuthStorage {
    /// Create new storage with an existing connection pool.
    #[must_use]
    pub fn new(pool: Arc<PgPool>) -> Self {
        Self { pool }
    }

    /// Create new storage by connecting to the database.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection fails.
    pub async fn connect(database_url: &str) -> Result<Self, StorageError> {
        use sqlx_core::pool::PoolOptions;
        let pool = PoolOptions::<Postgres>::new().connect(database_url).await?;
        Ok(Self::new(Arc::new(pool)))
    }

    /// Get a reference to the connection pool.
    #[must_use]
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Get a reference to the Arc-wrapped pool.
    #[must_use]
    pub fn pool_arc(&self) -> Arc<PgPool> {
        Arc::clone(&self.pool)
    }

    // -------------------------------------------------------------------------
    // Storage Accessors
    // -------------------------------------------------------------------------

    /// Get client storage operations.
    #[must_use]
    pub fn clients(&self) -> ClientStorage<'_> {
        ClientStorage::new(&self.pool)
    }

    /// Get session storage operations.
    #[must_use]
    pub fn sessions(&self) -> SessionStorage<'_> {
        SessionStorage::new(&self.pool)
    }

    /// Get token storage operations.
    #[must_use]
    pub fn tokens(&self) -> TokenStorage<'_> {
        TokenStorage::new(&self.pool)
    }

    /// Get policy storage operations.
    #[must_use]
    pub fn policies(&self) -> PolicyStorage<'_> {
        PolicyStorage::new(&self.pool)
    }

    /// Get user storage operations.
    #[must_use]
    pub fn users(&self) -> UserStorage<'_> {
        UserStorage::new(&self.pool)
    }

    /// Get revoked token storage operations.
    #[must_use]
    pub fn revoked_tokens(&self) -> RevokedTokenStorage<'_> {
        RevokedTokenStorage::new(&self.pool)
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_storage_error_not_found() {
        let err = StorageError::not_found("Client abc123");
        assert!(err.is_not_found());
        assert!(err.is_client_error());
        assert!(!err.is_server_error());
        assert_eq!(err.to_string(), "Not found: Client abc123");
    }

    #[test]
    fn test_storage_error_conflict() {
        let err = StorageError::conflict("Client already exists");
        assert!(err.is_conflict());
        assert!(err.is_client_error());
        assert!(!err.is_server_error());
    }

    #[test]
    fn test_storage_error_invalid_input() {
        let err = StorageError::invalid_input("Invalid client_id format");
        assert!(err.is_invalid_input());
        assert!(err.is_client_error());
        assert!(!err.is_server_error());
    }

    #[test]
    fn test_storage_error_serialization() {
        let json_err = serde_json::from_str::<serde_json::Value>("invalid json").unwrap_err();
        let err = StorageError::from(json_err);
        assert!(err.is_serialization_error());
        assert!(err.is_server_error());
        assert!(!err.is_client_error());
    }
}
