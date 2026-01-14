//! Basic Auth storage trait for unified Client/App authentication.

use async_trait::async_trait;

use crate::AuthResult;
use crate::types::BasicAuthEntity;

/// Storage operations for Basic Auth (Client or App).
///
/// This trait provides a unified interface for authenticating entities
/// via HTTP Basic Auth. It searches both Client and App storage to find
/// the entity by ID and verify the secret.
#[async_trait]
pub trait BasicAuthStorage: Send + Sync {
    /// Authenticate an entity by ID and secret.
    ///
    /// This method:
    /// 1. Searches for a Client with the given ID
    /// 2. If not found, searches for an App with the given ID
    /// 3. Verifies the provided secret against the stored hash
    ///
    /// # Arguments
    ///
    /// * `entity_id` - The client_id or app_id
    /// * `secret` - The plain-text secret to verify
    ///
    /// # Returns
    ///
    /// - `Ok(Some(entity))` if authentication succeeds
    /// - `Ok(None)` if entity doesn't exist or secret doesn't match
    ///
    /// # Errors
    ///
    /// Returns an error if the storage operation fails.
    async fn authenticate(
        &self,
        entity_id: &str,
        secret: &str,
    ) -> AuthResult<Option<BasicAuthEntity>>;
}
