//! Authorize session storage trait.
//!
//! This module defines the storage interface for authorize sessions
//! used during the OAuth 2.0 authorization flow UI (login/consent screens).
//!
//! # Implementation Notes
//!
//! Implementations should:
//!
//! - Store sessions with short TTL (10 minutes max)
//! - Support efficient lookup by session ID
//! - Clean up expired sessions periodically
//!
//! # Security Considerations
//!
//! - Sessions contain sensitive authorization request data
//! - Implement proper access controls on the storage backend
//! - Delete sessions after authorization code is issued

use async_trait::async_trait;
use uuid::Uuid;

use crate::AuthResult;
use crate::oauth::authorize_session::AuthorizeSession;

/// Storage trait for authorize flow sessions.
///
/// This trait defines the interface for persisting authorize sessions
/// during the OAuth 2.0 login/consent UI flow. Sessions track the
/// multi-step authorization process before the code is issued.
///
/// # Implementations
///
/// Implementations are provided for:
/// - PostgreSQL (in `octofhir-auth-postgres` crate)
#[async_trait]
pub trait AuthorizeSessionStorage: Send + Sync {
    /// Creates a new authorize session.
    ///
    /// # Arguments
    ///
    /// * `session` - The authorize session to store
    ///
    /// # Errors
    ///
    /// Returns an error if the session cannot be stored.
    async fn create(&self, session: &AuthorizeSession) -> AuthResult<()>;

    /// Finds a session by its ID.
    ///
    /// # Arguments
    ///
    /// * `id` - The session UUID to look up
    ///
    /// # Returns
    ///
    /// Returns `Some(session)` if found and not expired, `None` otherwise.
    ///
    /// # Errors
    ///
    /// Returns an error if the storage operation fails.
    async fn find_by_id(&self, id: Uuid) -> AuthResult<Option<AuthorizeSession>>;

    /// Updates the session with user ID after successful authentication.
    ///
    /// Called after the user successfully authenticates on the login form.
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

    /// Deletes a session by ID.
    ///
    /// Called after the authorization code is issued to clean up the session.
    ///
    /// # Arguments
    ///
    /// * `id` - The session ID to delete
    ///
    /// # Errors
    ///
    /// Returns an error if the deletion fails.
    async fn delete(&self, id: Uuid) -> AuthResult<()>;

    /// Deletes expired sessions.
    ///
    /// Should be called periodically to clean up old sessions.
    ///
    /// # Returns
    ///
    /// Returns the number of sessions deleted.
    ///
    /// # Errors
    ///
    /// Returns an error if the cleanup operation fails.
    async fn cleanup_expired(&self) -> AuthResult<u64>;
}
