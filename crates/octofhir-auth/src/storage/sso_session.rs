//! SSO session storage trait.
//!
//! This module defines the storage interface for managing SSO (Single Sign-On)
//! authentication sessions (AuthSession resources).

use async_trait::async_trait;

/// Storage trait for SSO authentication sessions.
///
/// Provides operations for managing persistent login sessions that survive
/// across multiple OAuth authorization flows.
#[async_trait]
pub trait SsoSessionStorage: Send + Sync {
    /// Find an AuthSession resource ID by session token.
    ///
    /// Returns `None` if the session is not found or has expired.
    async fn find_session_by_token(&self, token: &str) -> Result<Option<String>, StorageError>;

    /// Revoke an SSO session by updating its status to "revoked".
    ///
    /// This also removes the session from the token index.
    async fn revoke_session(&self, session_id: &str) -> Result<(), StorageError>;

    /// Count active sessions for a user.
    ///
    /// Used to enforce concurrent session limits.
    async fn count_active_sessions(&self, user_id: &str) -> Result<u32, StorageError>;

    /// Clean up expired sessions.
    ///
    /// Returns the number of sessions cleaned up.
    async fn cleanup_expired_sessions(&self) -> Result<u64, StorageError>;
}

/// Error type for SSO session storage operations.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    /// Database operation failed.
    #[error("Database error: {0}")]
    Database(String),

    /// Session not found.
    #[error("Session not found")]
    NotFound,

    /// Invalid input data.
    #[error("Invalid input: {0}")]
    InvalidInput(String),

    /// Serialization/deserialization failed.
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// Internal error.
    #[error("Internal error: {0}")]
    Internal(String),
}
