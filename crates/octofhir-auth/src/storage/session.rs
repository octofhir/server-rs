//! Authorization session storage trait.
//!
//! This module defines the storage interface for authorization sessions
//! used during the OAuth 2.0 authorization code flow.
//!
//! # Implementation Notes
//!
//! Implementations should:
//!
//! - Store sessions securely (authorization codes are sensitive)
//! - Support efficient lookup by authorization code
//! - Ensure atomicity for consume operations (prevent replay attacks)
//! - Clean up expired sessions periodically
//!
//! # Security Considerations
//!
//! - Never log authorization codes
//! - Ensure consume is atomic to prevent race conditions
//! - Sessions should be stored encrypted at rest if possible
//! - Implement proper access controls on the storage backend

use async_trait::async_trait;
use uuid::Uuid;

use crate::AuthResult;
use crate::oauth::session::AuthorizationSession;

/// Storage trait for authorization sessions.
///
/// This trait defines the interface for persisting authorization sessions
/// during the OAuth 2.0 authorization code flow. Sessions are created when
/// an authorization request is validated and consumed when the code is
/// exchanged for tokens.
///
/// # Implementations
///
/// Implementations are provided for:
/// - PostgreSQL (in `octofhir-auth-postgres` crate)
///
/// # Example Implementation
///
/// ```ignore
/// use octofhir_auth::storage::SessionStorage;
/// use octofhir_auth::oauth::AuthorizationSession;
/// use octofhir_auth::AuthResult;
///
/// struct InMemorySessionStorage {
///     sessions: std::sync::RwLock<std::collections::HashMap<String, AuthorizationSession>>,
/// }
///
/// #[async_trait::async_trait]
/// impl SessionStorage for InMemorySessionStorage {
///     async fn create(&self, session: &AuthorizationSession) -> AuthResult<()> {
///         let mut sessions = self.sessions.write().unwrap();
///         sessions.insert(session.code.clone(), session.clone());
///         Ok(())
///     }
///     // ... other methods
/// }
/// ```
#[async_trait]
pub trait SessionStorage: Send + Sync {
    /// Creates a new authorization session.
    ///
    /// # Arguments
    ///
    /// * `session` - The authorization session to store
    ///
    /// # Errors
    ///
    /// Returns an error if the session cannot be stored (e.g., duplicate code,
    /// storage unavailable).
    async fn create(&self, session: &AuthorizationSession) -> AuthResult<()>;

    /// Finds a session by authorization code.
    ///
    /// # Arguments
    ///
    /// * `code` - The authorization code to look up
    ///
    /// # Returns
    ///
    /// Returns `Some(session)` if found, `None` if not found.
    /// This method returns sessions regardless of their consumed/expired status;
    /// callers should check `is_valid()` before using.
    ///
    /// # Errors
    ///
    /// Returns an error if the storage operation fails.
    async fn find_by_code(&self, code: &str) -> AuthResult<Option<AuthorizationSession>>;

    /// Finds a session by its ID.
    ///
    /// # Arguments
    ///
    /// * `id` - The session UUID to look up
    ///
    /// # Returns
    ///
    /// Returns `Some(session)` if found, `None` if not found.
    ///
    /// # Errors
    ///
    /// Returns an error if the storage operation fails.
    async fn find_by_id(&self, id: Uuid) -> AuthResult<Option<AuthorizationSession>>;

    /// Consumes an authorization code (marks as used).
    ///
    /// This operation must be atomic to prevent replay attacks where the same
    /// code is used multiple times concurrently.
    ///
    /// # Arguments
    ///
    /// * `code` - The authorization code to consume
    ///
    /// # Returns
    ///
    /// Returns the consumed session on success. The session's `consumed_at`
    /// field will be set to the current timestamp.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The code is not found (`InvalidGrant`)
    /// - The code is already consumed (`InvalidGrant`)
    /// - The code is expired (`InvalidGrant`)
    /// - The storage operation fails
    ///
    /// # Atomicity
    ///
    /// Implementations must ensure this operation is atomic. A common approach
    /// is to use a conditional update:
    ///
    /// ```sql
    /// UPDATE sessions
    /// SET consumed_at = NOW()
    /// WHERE code = $1 AND consumed_at IS NULL AND expires_at > NOW()
    /// RETURNING *
    /// ```
    async fn consume(&self, code: &str) -> AuthResult<AuthorizationSession>;

    /// Updates the session with user information after authentication.
    ///
    /// Called after the user successfully authenticates to associate
    /// the session with a user ID.
    ///
    /// # Arguments
    ///
    /// * `id` - The session ID to update
    /// * `user_id` - The authenticated user's ID
    ///
    /// # Errors
    ///
    /// Returns an error if the session is not found or the update fails.
    async fn update_user(&self, id: Uuid, user_id: &str) -> AuthResult<()>;

    /// Updates the session with launch context.
    ///
    /// Called during EHR launch to add context information (patient,
    /// encounter, etc.) to the session.
    ///
    /// # Arguments
    ///
    /// * `id` - The session ID to update
    /// * `launch_context` - The launch context to set
    ///
    /// # Errors
    ///
    /// Returns an error if the session is not found or the update fails.
    async fn update_launch_context(
        &self,
        id: Uuid,
        launch_context: crate::oauth::session::LaunchContext,
    ) -> AuthResult<()>;

    /// Deletes expired sessions.
    ///
    /// Should be called periodically to clean up old sessions and
    /// prevent storage growth.
    ///
    /// # Returns
    ///
    /// Returns the number of sessions deleted.
    ///
    /// # Errors
    ///
    /// Returns an error if the cleanup operation fails.
    async fn cleanup_expired(&self) -> AuthResult<u64>;

    /// Deletes all sessions for a specific client.
    ///
    /// Used when a client is deleted or compromised to invalidate
    /// all pending authorizations.
    ///
    /// # Arguments
    ///
    /// * `client_id` - The client ID whose sessions should be deleted
    ///
    /// # Returns
    ///
    /// Returns the number of sessions deleted.
    ///
    /// # Errors
    ///
    /// Returns an error if the deletion fails.
    async fn delete_by_client(&self, client_id: &str) -> AuthResult<u64>;
}
