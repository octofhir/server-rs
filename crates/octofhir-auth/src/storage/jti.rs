//! JWT ID (JTI) storage trait for replay prevention.
//!
//! This module defines the storage interface for tracking used JTI values
//! to prevent replay attacks in JWT client assertions.
//!
//! # Security Considerations
//!
//! - JTIs must be stored with their expiration time
//! - Lookups must be atomic to prevent race conditions
//! - Expired JTIs should be cleaned up periodically
//! - Storage should be persistent across server restarts
//!
//! # Implementation Notes
//!
//! The `mark_used` method should atomically check and mark a JTI as used.
//! This prevents replay attacks where the same assertion is submitted
//! concurrently to multiple server instances.

use async_trait::async_trait;
use time::OffsetDateTime;

use crate::AuthResult;

/// Storage trait for JWT ID (JTI) tracking.
///
/// This trait defines the interface for preventing JWT replay attacks
/// by tracking which JTI values have already been used.
///
/// # Implementations
///
/// Implementations are provided in separate crates:
/// - `octofhir-auth-postgres` - PostgreSQL storage backend
///
/// # Example Implementation
///
/// ```ignore
/// use octofhir_auth::storage::JtiStorage;
/// use octofhir_auth::AuthResult;
/// use time::OffsetDateTime;
///
/// struct InMemoryJtiStorage {
///     used: std::sync::RwLock<std::collections::HashMap<String, OffsetDateTime>>,
/// }
///
/// #[async_trait::async_trait]
/// impl JtiStorage for InMemoryJtiStorage {
///     async fn mark_used(&self, jti: &str, expires_at: OffsetDateTime) -> AuthResult<bool> {
///         let mut used = self.used.write().unwrap();
///         if used.contains_key(jti) {
///             return Ok(false); // Already used
///         }
///         used.insert(jti.to_string(), expires_at);
///         Ok(true) // Successfully marked
///     }
///     // ... other methods
/// }
/// ```
#[async_trait]
pub trait JtiStorage: Send + Sync {
    /// Atomically marks a JTI as used if not already used.
    ///
    /// This operation must be atomic to prevent race conditions where
    /// the same JWT assertion is submitted concurrently.
    ///
    /// # Arguments
    ///
    /// * `jti` - The JWT ID to mark as used
    /// * `expires_at` - When this JTI entry can be cleaned up (matches JWT exp)
    ///
    /// # Returns
    ///
    /// Returns `true` if the JTI was successfully marked as used (first use),
    /// or `false` if the JTI was already used (replay attack detected).
    ///
    /// # Errors
    ///
    /// Returns an error if the storage operation fails.
    ///
    /// # Atomicity
    ///
    /// Implementations must ensure this operation is atomic. A common approach
    /// is to use a conditional insert:
    ///
    /// ```sql
    /// INSERT INTO used_jtis (jti, expires_at)
    /// VALUES ($1, $2)
    /// ON CONFLICT (jti) DO NOTHING
    /// RETURNING jti
    /// ```
    async fn mark_used(&self, jti: &str, expires_at: OffsetDateTime) -> AuthResult<bool>;

    /// Checks if a JTI has already been used.
    ///
    /// Note: For security, prefer `mark_used` which atomically checks and marks.
    /// This method is provided for cases where you need to check without marking.
    ///
    /// # Arguments
    ///
    /// * `jti` - The JWT ID to check
    ///
    /// # Returns
    ///
    /// Returns `true` if the JTI has been used, `false` otherwise.
    ///
    /// # Errors
    ///
    /// Returns an error if the storage operation fails.
    async fn is_used(&self, jti: &str) -> AuthResult<bool>;

    /// Deletes expired JTI entries.
    ///
    /// Should be called periodically to clean up old entries and prevent
    /// storage growth. Entries can be deleted once their `expires_at`
    /// timestamp has passed.
    ///
    /// # Returns
    ///
    /// Returns the number of entries deleted.
    ///
    /// # Errors
    ///
    /// Returns an error if the cleanup operation fails.
    async fn cleanup_expired(&self) -> AuthResult<u64>;
}
