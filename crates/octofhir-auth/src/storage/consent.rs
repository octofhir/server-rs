//! User consent storage trait.
//!
//! This module defines the storage interface for user consent records.
//! Consents are stored persistently and used to skip the consent screen
//! on repeat authorizations for the same client+scopes.
//!
//! # Implementation Notes
//!
//! Implementations should:
//!
//! - Support efficient lookup by user_id + client_id
//! - Store scopes as an array for subset checking
//! - Support upsert (update on conflict) for save operations
//!
//! # Security Considerations
//!
//! - Consent records are tied to user identity
//! - Users should be able to revoke consents via UI

use async_trait::async_trait;

use crate::AuthResult;

/// Storage trait for user consent records.
///
/// This trait defines the interface for persisting user consent decisions.
/// Consents allow skipping the consent screen when a user has previously
/// authorized an application with the same or subset of scopes.
///
/// # Implementations
///
/// Implementations are provided for:
/// - PostgreSQL (in `octofhir-auth-postgres` crate)
#[async_trait]
pub trait ConsentStorage: Send + Sync {
    /// Checks if user has granted consent for client with all requested scopes.
    ///
    /// Returns true if a consent record exists for the user+client combination
    /// AND the stored scopes include ALL the requested scopes.
    ///
    /// # Arguments
    ///
    /// * `user_id` - The user's ID
    /// * `client_id` - The OAuth client ID
    /// * `scopes` - The requested scopes to check
    ///
    /// # Returns
    ///
    /// Returns `true` if consent exists and covers all requested scopes.
    ///
    /// # Errors
    ///
    /// Returns an error if the storage operation fails.
    async fn has_consent(&self, user_id: &str, client_id: &str, scopes: &[&str]) -> AuthResult<bool>;

    /// Saves or updates consent for a user+client combination.
    ///
    /// If consent already exists, the scopes are updated (upsert).
    /// The `updated_at` timestamp is refreshed.
    ///
    /// # Arguments
    ///
    /// * `user_id` - The user's ID
    /// * `client_id` - The OAuth client ID
    /// * `scopes` - The granted scopes
    ///
    /// # Errors
    ///
    /// Returns an error if the save operation fails.
    async fn save_consent(
        &self,
        user_id: &str,
        client_id: &str,
        scopes: &[String],
    ) -> AuthResult<()>;

    /// Revokes consent for a user+client combination.
    ///
    /// Deletes the consent record, forcing the user to re-consent
    /// on next authorization.
    ///
    /// # Arguments
    ///
    /// * `user_id` - The user's ID
    /// * `client_id` - The OAuth client ID
    ///
    /// # Errors
    ///
    /// Returns an error if the revocation fails.
    async fn revoke_consent(&self, user_id: &str, client_id: &str) -> AuthResult<()>;

    /// Lists all consents for a user.
    ///
    /// Used for displaying consents in user settings UI.
    ///
    /// # Arguments
    ///
    /// * `user_id` - The user's ID
    ///
    /// # Returns
    ///
    /// Returns a list of (client_id, scopes) tuples.
    ///
    /// # Errors
    ///
    /// Returns an error if the list operation fails.
    async fn list_consents(&self, user_id: &str) -> AuthResult<Vec<UserConsent>>;
}

/// Represents a user's consent record.
#[derive(Debug, Clone)]
pub struct UserConsent {
    /// The OAuth client ID.
    pub client_id: String,
    /// The granted scopes.
    pub scopes: Vec<String>,
    /// When the consent was first granted.
    pub created_at: time::OffsetDateTime,
    /// When the consent was last updated.
    pub updated_at: time::OffsetDateTime,
}
