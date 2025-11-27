//! Refresh token storage trait.
//!
//! This module defines the storage interface for OAuth 2.0 refresh tokens.
//!
//! # Security Considerations
//!
//! - Tokens are stored as SHA-256 hashes only
//! - Revocation must be atomic and immediate
//! - Expired tokens should be cleaned up periodically
//! - Access to this storage should be restricted

use async_trait::async_trait;
use uuid::Uuid;

use crate::AuthResult;
use crate::types::refresh_token::RefreshToken;

/// Storage trait for refresh tokens.
///
/// This trait defines the interface for persisting and managing refresh
/// tokens. Implementations must ensure security properties like atomic
/// revocation and secure hash storage.
///
/// # Implementations
///
/// Implementations are provided in separate crates:
/// - `octofhir-auth-postgres` - PostgreSQL storage backend
///
/// # Example Implementation
///
/// ```ignore
/// use octofhir_auth::storage::RefreshTokenStorage;
/// use octofhir_auth::types::RefreshToken;
/// use octofhir_auth::AuthResult;
///
/// struct InMemoryRefreshTokenStorage {
///     tokens: std::sync::RwLock<std::collections::HashMap<String, RefreshToken>>,
/// }
///
/// #[async_trait::async_trait]
/// impl RefreshTokenStorage for InMemoryRefreshTokenStorage {
///     async fn create(&self, token: &RefreshToken) -> AuthResult<()> {
///         let mut tokens = self.tokens.write().unwrap();
///         tokens.insert(token.token_hash.clone(), token.clone());
///         Ok(())
///     }
///     // ... other methods
/// }
/// ```
#[async_trait]
pub trait RefreshTokenStorage: Send + Sync {
    /// Stores a new refresh token.
    ///
    /// # Arguments
    ///
    /// * `token` - The refresh token to store (with hashed token value)
    ///
    /// # Errors
    ///
    /// Returns an error if the token cannot be stored (e.g., duplicate hash,
    /// storage unavailable).
    async fn create(&self, token: &RefreshToken) -> AuthResult<()>;

    /// Finds a refresh token by its hash.
    ///
    /// # Arguments
    ///
    /// * `token_hash` - SHA-256 hash of the token to find
    ///
    /// # Returns
    ///
    /// Returns `Some(token)` if found, `None` if not found.
    /// This returns tokens regardless of expiration/revocation status;
    /// callers should check `is_valid()` before using.
    ///
    /// # Errors
    ///
    /// Returns an error if the storage operation fails.
    async fn find_by_hash(&self, token_hash: &str) -> AuthResult<Option<RefreshToken>>;

    /// Finds a refresh token by its ID.
    ///
    /// # Arguments
    ///
    /// * `id` - The token's UUID
    ///
    /// # Returns
    ///
    /// Returns `Some(token)` if found, `None` if not found.
    ///
    /// # Errors
    ///
    /// Returns an error if the storage operation fails.
    async fn find_by_id(&self, id: Uuid) -> AuthResult<Option<RefreshToken>>;

    /// Revokes a refresh token.
    ///
    /// Sets the `revoked_at` timestamp to the current time. This operation
    /// must be atomic - once revoked, the token cannot be used.
    ///
    /// # Arguments
    ///
    /// * `token_hash` - SHA-256 hash of the token to revoke
    ///
    /// # Errors
    ///
    /// Returns an error if the token is not found or the operation fails.
    async fn revoke(&self, token_hash: &str) -> AuthResult<()>;

    /// Revokes all refresh tokens for a client.
    ///
    /// Used when a client is compromised or deleted to invalidate all
    /// outstanding tokens.
    ///
    /// # Arguments
    ///
    /// * `client_id` - The client ID whose tokens should be revoked
    ///
    /// # Returns
    ///
    /// Returns the number of tokens revoked.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    async fn revoke_by_client(&self, client_id: &str) -> AuthResult<u64>;

    /// Revokes all refresh tokens for a user.
    ///
    /// Used when a user's session is invalidated (logout, password change,
    /// account compromise) to invalidate all outstanding tokens.
    ///
    /// # Arguments
    ///
    /// * `user_id` - The user ID whose tokens should be revoked
    ///
    /// # Returns
    ///
    /// Returns the number of tokens revoked.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    async fn revoke_by_user(&self, user_id: Uuid) -> AuthResult<u64>;

    /// Deletes expired and revoked tokens.
    ///
    /// Should be called periodically to clean up old tokens and prevent
    /// storage growth. This can delete both expired tokens and tokens
    /// that have been revoked for a certain period.
    ///
    /// # Returns
    ///
    /// Returns the number of tokens deleted.
    ///
    /// # Errors
    ///
    /// Returns an error if the cleanup operation fails.
    async fn cleanup_expired(&self) -> AuthResult<u64>;

    /// Lists all active (non-revoked, non-expired) tokens for a user.
    ///
    /// Useful for user session management UI.
    ///
    /// # Arguments
    ///
    /// * `user_id` - The user ID whose tokens to list
    ///
    /// # Returns
    ///
    /// Returns a list of active refresh tokens.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    async fn list_by_user(&self, user_id: Uuid) -> AuthResult<Vec<RefreshToken>>;
}
