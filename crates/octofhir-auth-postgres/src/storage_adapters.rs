//! Arc-owning storage adapters for use with authentication middleware.
//!
//! These adapters wrap the lifetime-based storage types and own an Arc<PgPool>,
//! allowing them to be used as `Arc<dyn Storage>` in middleware.

use std::sync::Arc;

use async_trait::async_trait;
use time::OffsetDateTime;
use uuid::Uuid;

use octofhir_auth::storage::{
    ClientStorage as ClientStorageTrait, RevokedTokenStorage as RevokedTokenStorageTrait,
    User, UserStorage as UserStorageTrait,
};
use octofhir_auth::types::Client;
use octofhir_auth::{AuthError, AuthResult};

use crate::client::PostgresClientStorage;
use crate::revoked_token::RevokedTokenStorage;
use crate::user::{UserRow, UserStorage};
use crate::PgPool;

// =============================================================================
// Arc-Owning Client Storage
// =============================================================================

/// Arc-owning PostgreSQL client storage adapter.
///
/// This wrapper owns an `Arc<PgPool>` instead of borrowing, allowing it
/// to be used as `Arc<dyn ClientStorage>` in middleware.
#[derive(Clone)]
pub struct ArcClientStorage {
    pool: Arc<PgPool>,
}

impl ArcClientStorage {
    /// Create a new Arc-owning client storage.
    #[must_use]
    pub fn new(pool: Arc<PgPool>) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl ClientStorageTrait for ArcClientStorage {
    async fn find_by_client_id(&self, client_id: &str) -> AuthResult<Option<Client>> {
        let storage = PostgresClientStorage::new(&self.pool);
        storage.find_by_client_id(client_id).await
    }

    async fn create(&self, client: &Client) -> AuthResult<Client> {
        let storage = PostgresClientStorage::new(&self.pool);
        storage.create(client).await
    }

    async fn update(&self, client_id: &str, client: &Client) -> AuthResult<Client> {
        let storage = PostgresClientStorage::new(&self.pool);
        storage.update(client_id, client).await
    }

    async fn delete(&self, client_id: &str) -> AuthResult<()> {
        let storage = PostgresClientStorage::new(&self.pool);
        storage.delete(client_id).await
    }

    async fn list(&self, limit: i64, offset: i64) -> AuthResult<Vec<Client>> {
        let storage = PostgresClientStorage::new(&self.pool);
        storage.list(limit, offset).await
    }

    async fn verify_secret(&self, client_id: &str, secret: &str) -> AuthResult<bool> {
        let storage = PostgresClientStorage::new(&self.pool);
        storage.verify_secret(client_id, secret).await
    }
}

// =============================================================================
// Arc-Owning Revoked Token Storage
// =============================================================================

/// Arc-owning PostgreSQL revoked token storage adapter.
#[derive(Clone)]
pub struct ArcRevokedTokenStorage {
    pool: Arc<PgPool>,
}

impl ArcRevokedTokenStorage {
    /// Create a new Arc-owning revoked token storage.
    #[must_use]
    pub fn new(pool: Arc<PgPool>) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl RevokedTokenStorageTrait for ArcRevokedTokenStorage {
    async fn revoke(&self, jti: &str, expires_at: OffsetDateTime) -> AuthResult<()> {
        let storage = RevokedTokenStorage::new(&self.pool);
        storage
            .revoke(jti, expires_at)
            .await
            .map_err(|e| AuthError::storage(e.to_string()))
    }

    async fn is_revoked(&self, jti: &str) -> AuthResult<bool> {
        let storage = RevokedTokenStorage::new(&self.pool);
        storage
            .is_revoked(jti)
            .await
            .map_err(|e| AuthError::storage(e.to_string()))
    }

    async fn cleanup_expired(&self) -> AuthResult<u64> {
        let storage = RevokedTokenStorage::new(&self.pool);
        storage
            .cleanup_expired()
            .await
            .map_err(|e| AuthError::storage(e.to_string()))
    }
}

// =============================================================================
// Arc-Owning User Storage
// =============================================================================

/// Arc-owning PostgreSQL user storage adapter.
#[derive(Clone)]
pub struct ArcUserStorage {
    pool: Arc<PgPool>,
}

impl ArcUserStorage {
    /// Create a new Arc-owning user storage.
    #[must_use]
    pub fn new(pool: Arc<PgPool>) -> Self {
        Self { pool }
    }

    /// Convert a UserRow to the User trait type.
    fn row_to_user(row: UserRow) -> AuthResult<User> {
        // The resource field contains the user data as JSON
        // We need to deserialize it, but also add the id from the row
        let mut user: User = serde_json::from_value(row.resource.clone())
            .map_err(|e| AuthError::storage(format!("Failed to deserialize user: {}", e)))?;

        // Ensure the ID matches the row ID
        user.id = row.id;

        // Set timestamps from row metadata
        user.updated_at = row.ts;

        Ok(user)
    }

    /// Convert a User to JSON for storage.
    fn user_to_json(user: &User) -> serde_json::Value {
        serde_json::to_value(user).unwrap_or_default()
    }
}

#[async_trait]
impl UserStorageTrait for ArcUserStorage {
    async fn find_by_id(&self, id: Uuid) -> AuthResult<Option<User>> {
        let storage = UserStorage::new(&self.pool);
        let row = storage
            .find_by_id(id)
            .await
            .map_err(|e| AuthError::storage(e.to_string()))?;

        match row {
            Some(r) => Ok(Some(Self::row_to_user(r)?)),
            None => Ok(None),
        }
    }

    async fn find_by_username(&self, username: &str) -> AuthResult<Option<User>> {
        let storage = UserStorage::new(&self.pool);
        let row = storage
            .find_by_username(username)
            .await
            .map_err(|e| AuthError::storage(e.to_string()))?;

        match row {
            Some(r) => Ok(Some(Self::row_to_user(r)?)),
            None => Ok(None),
        }
    }

    async fn find_by_email(&self, email: &str) -> AuthResult<Option<User>> {
        let storage = UserStorage::new(&self.pool);
        let row = storage
            .find_by_email(email)
            .await
            .map_err(|e| AuthError::storage(e.to_string()))?;

        match row {
            Some(r) => Ok(Some(Self::row_to_user(r)?)),
            None => Ok(None),
        }
    }

    async fn find_by_external_identity(
        &self,
        provider_id: &str,
        external_subject: &str,
    ) -> AuthResult<Option<User>> {
        let storage = UserStorage::new(&self.pool);
        let row = storage
            .find_by_external_identity(provider_id, external_subject)
            .await
            .map_err(|e| AuthError::storage(e.to_string()))?;

        match row {
            Some(r) => Ok(Some(Self::row_to_user(r)?)),
            None => Ok(None),
        }
    }

    async fn create(&self, user: &User) -> AuthResult<()> {
        let storage = UserStorage::new(&self.pool);
        let resource = Self::user_to_json(user);
        storage
            .create(user.id, resource)
            .await
            .map_err(|e| AuthError::storage(e.to_string()))?;
        Ok(())
    }

    async fn update(&self, user: &User) -> AuthResult<()> {
        let storage = UserStorage::new(&self.pool);
        let resource = Self::user_to_json(user);
        storage
            .update(user.id, resource)
            .await
            .map_err(|e| AuthError::storage(e.to_string()))?;
        Ok(())
    }

    async fn delete(&self, id: Uuid) -> AuthResult<()> {
        let storage = UserStorage::new(&self.pool);
        storage
            .delete(id)
            .await
            .map_err(|e| AuthError::storage(e.to_string()))
    }

    async fn verify_password(&self, user_id: Uuid, password: &str) -> AuthResult<bool> {
        // First, get the user to access their password hash
        let user = self.find_by_id(user_id).await?;

        match user {
            Some(u) => {
                match u.password_hash {
                    Some(hash) => {
                        // Verify using bcrypt
                        bcrypt::verify(password, &hash)
                            .map_err(|e| AuthError::storage(format!("Password verification failed: {}", e)))
                    }
                    None => {
                        // User has no password (federated/SSO user)
                        Ok(false)
                    }
                }
            }
            None => Err(AuthError::storage(format!("User {} not found", user_id))),
        }
    }

    async fn list(&self, limit: i64, offset: i64) -> AuthResult<Vec<User>> {
        let storage = UserStorage::new(&self.pool);
        let rows = storage
            .list(limit, offset)
            .await
            .map_err(|e| AuthError::storage(e.to_string()))?;

        rows.into_iter().map(Self::row_to_user).collect()
    }
}
