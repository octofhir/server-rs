//! Client storage trait.
//!
//! Defines the interface for OAuth client persistence operations.
//! Implementations are provided by storage backends (e.g., PostgreSQL).

use async_trait::async_trait;

use crate::AuthResult;
use crate::types::Client;

// =============================================================================
// Client Storage Trait
// =============================================================================

/// Storage operations for OAuth 2.0 clients.
///
/// This trait defines the interface for persisting and retrieving OAuth client
/// registrations. Implementations handle the actual database operations.
///
/// # Example
///
/// ```ignore
/// use octofhir_auth::storage::ClientStorage;
///
/// async fn example(storage: &impl ClientStorage) {
///     // Find a client by its OAuth client_id
///     if let Some(client) = storage.find_by_client_id("my-app").await? {
///         println!("Found client: {}", client.name);
///     }
/// }
/// ```
#[async_trait]
pub trait ClientStorage: Send + Sync {
    /// Find a client by its OAuth client_id.
    ///
    /// Returns `None` if the client doesn't exist or is not active.
    ///
    /// # Errors
    ///
    /// Returns an error if the storage operation fails.
    async fn find_by_client_id(&self, client_id: &str) -> AuthResult<Option<Client>>;

    /// Create a new client.
    ///
    /// The client is validated before creation.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The client validation fails
    /// - A client with the same client_id already exists
    /// - The storage operation fails
    async fn create(&self, client: &Client) -> AuthResult<Client>;

    /// Update an existing client.
    ///
    /// The client is validated before update.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The client doesn't exist
    /// - The client validation fails
    /// - The storage operation fails
    async fn update(&self, client_id: &str, client: &Client) -> AuthResult<Client>;

    /// Delete a client.
    ///
    /// Implementations should perform a soft delete to preserve audit trail.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The client doesn't exist
    /// - The storage operation fails
    async fn delete(&self, client_id: &str) -> AuthResult<()>;

    /// List all active clients.
    ///
    /// # Arguments
    ///
    /// * `limit` - Maximum number of clients to return
    /// * `offset` - Number of clients to skip for pagination
    ///
    /// # Errors
    ///
    /// Returns an error if the storage operation fails.
    async fn list(&self, limit: i64, offset: i64) -> AuthResult<Vec<Client>>;

    /// Verify a client secret.
    ///
    /// Compares the provided secret against the stored BCrypt hash.
    ///
    /// # Arguments
    ///
    /// * `client_id` - The OAuth client_id
    /// * `secret` - The plaintext secret to verify
    ///
    /// # Returns
    ///
    /// - `Ok(true)` if the secret matches
    /// - `Ok(false)` if the secret doesn't match or client has no secret
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The client doesn't exist
    /// - The storage operation fails
    async fn verify_secret(&self, client_id: &str, secret: &str) -> AuthResult<bool>;
}
