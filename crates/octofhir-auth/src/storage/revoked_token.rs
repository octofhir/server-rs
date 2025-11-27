//! Revoked access token storage trait.
//!
//! This module defines the storage interface for tracking revoked access token JTIs.
//! When an access token is revoked, its JTI is stored until the token would have
//! naturally expired, allowing validation to check for revocation.
//!
//! # Security Considerations
//!
//! - Revoked JTIs must be stored with their original expiration time
//! - Lookups must be fast to not impact token validation performance
//! - Expired revocation records should be cleaned up periodically
//! - Storage should be persistent across server restarts
//!
//! # Implementation Notes
//!
//! Unlike refresh tokens which are stored with their full metadata, access token
//! revocation only needs to track JTIs because:
//!
//! 1. Access tokens are JWTs that are validated by signature
//! 2. We only need to know IF a token is revoked, not its full contents
//! 3. JTIs are unique and sufficient for identification
//! 4. Storage can be cleaned up once the token's exp time passes

use async_trait::async_trait;
use time::OffsetDateTime;

use crate::AuthResult;

/// Storage trait for revoked access token JTIs.
///
/// This trait defines the interface for tracking which access tokens have been
/// explicitly revoked. Since access tokens are JWTs, we track their JTI (JWT ID)
/// claims rather than storing the full token.
///
/// # Implementations
///
/// Implementations are provided in separate crates:
/// - `octofhir-auth-postgres` - PostgreSQL storage backend
///
/// # Example Implementation
///
/// ```ignore
/// use octofhir_auth::storage::RevokedTokenStorage;
/// use octofhir_auth::AuthResult;
/// use time::OffsetDateTime;
///
/// struct InMemoryRevokedTokenStorage {
///     revoked: std::sync::RwLock<std::collections::HashMap<String, OffsetDateTime>>,
/// }
///
/// #[async_trait::async_trait]
/// impl RevokedTokenStorage for InMemoryRevokedTokenStorage {
///     async fn revoke(&self, jti: &str, expires_at: OffsetDateTime) -> AuthResult<()> {
///         let mut revoked = self.revoked.write().unwrap();
///         revoked.insert(jti.to_string(), expires_at);
///         Ok(())
///     }
///     // ... other methods
/// }
/// ```
#[async_trait]
pub trait RevokedTokenStorage: Send + Sync {
    /// Marks an access token JTI as revoked.
    ///
    /// The `expires_at` parameter should be the token's original expiration time.
    /// This allows cleanup of revocation records once they're no longer needed.
    ///
    /// # Arguments
    ///
    /// * `jti` - The JWT ID of the access token to revoke
    /// * `expires_at` - When the token would have naturally expired
    ///
    /// # Errors
    ///
    /// Returns an error if the storage operation fails.
    ///
    /// # Idempotency
    ///
    /// This operation should be idempotent - revoking an already-revoked JTI
    /// should succeed without error.
    async fn revoke(&self, jti: &str, expires_at: OffsetDateTime) -> AuthResult<()>;

    /// Checks if an access token JTI has been revoked.
    ///
    /// This method is called during token validation to check if a token
    /// that is otherwise valid (signature OK, not expired) has been revoked.
    ///
    /// # Arguments
    ///
    /// * `jti` - The JWT ID to check
    ///
    /// # Returns
    ///
    /// Returns `true` if the JTI has been revoked, `false` otherwise.
    ///
    /// # Performance
    ///
    /// This method is called on every token validation, so implementations
    /// should ensure it's fast (consider caching, indexes, etc.).
    ///
    /// # Errors
    ///
    /// Returns an error if the storage operation fails.
    async fn is_revoked(&self, jti: &str) -> AuthResult<bool>;

    /// Deletes expired revocation records.
    ///
    /// Should be called periodically to clean up revocation records for tokens
    /// that have naturally expired. Once a token would have expired anyway,
    /// there's no need to track its revocation.
    ///
    /// # Returns
    ///
    /// Returns the number of records deleted.
    ///
    /// # Errors
    ///
    /// Returns an error if the cleanup operation fails.
    async fn cleanup_expired(&self) -> AuthResult<u64>;
}
