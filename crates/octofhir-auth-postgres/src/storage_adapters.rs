//! Arc-owning storage adapters for use with authentication middleware.
//!
//! These adapters wrap the lifetime-based storage types and own an Arc<PgPool>,
//! allowing them to be used as `Arc<dyn Storage>` in middleware.

use std::sync::Arc;

use async_trait::async_trait;
use time::OffsetDateTime;
use uuid::Uuid;

use octofhir_auth::oauth::session::{AuthorizationSession, LaunchContext};
use octofhir_auth::storage::{
    ClientStorage as ClientStorageTrait, RefreshTokenStorage as RefreshTokenStorageTrait,
    RevokedTokenStorage as RevokedTokenStorageTrait, SessionStorage as SessionStorageTrait, User,
    UserStorage as UserStorageTrait,
};
use octofhir_auth::types::{Client, RefreshToken};
use octofhir_auth::{AuthError, AuthResult};

use crate::PgPool;
use crate::client::PostgresClientStorage;
use crate::revoked_token::RevokedTokenStorage;
use crate::session::SessionStorage;
use crate::token::TokenStorage;
use crate::user::{UserRow, UserStorage};

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

    async fn regenerate_secret(&self, client_id: &str) -> AuthResult<(Client, String)> {
        let storage = PostgresClientStorage::new(&self.pool);
        storage.regenerate_secret(client_id).await
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

        // Ensure the ID matches the row ID (parse from String to Uuid)
        user.id = Uuid::parse_str(&row.id)
            .map_err(|e| AuthError::storage(format!("Invalid user ID: {}", e)))?;

        // Set timestamps from row metadata
        user.updated_at = row.updated_at;

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
                        bcrypt::verify(password, &hash).map_err(|e| {
                            AuthError::storage(format!("Password verification failed: {}", e))
                        })
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

    async fn update_last_login(&self, user_id: Uuid) -> AuthResult<()> {
        let storage = UserStorage::new(&self.pool);
        storage
            .update_last_login(user_id)
            .await
            .map(|_| ()) // Discard the returned UserRow
            .map_err(|e| AuthError::storage(e.to_string()))
    }
}

// =============================================================================
// Arc-Owning Session Storage
// =============================================================================

/// Arc-owning PostgreSQL session storage adapter.
#[derive(Clone)]
pub struct ArcSessionStorage {
    pool: Arc<PgPool>,
}

impl ArcSessionStorage {
    /// Create a new Arc-owning session storage.
    #[must_use]
    pub fn new(pool: Arc<PgPool>) -> Self {
        Self { pool }
    }

    /// Convert a session row to AuthorizationSession.
    fn row_to_session(row: crate::session::SessionRow) -> AuthResult<AuthorizationSession> {
        serde_json::from_value(row.resource)
            .map_err(|e| AuthError::storage(format!("Failed to deserialize session: {}", e)))
    }
}

#[async_trait]
impl SessionStorageTrait for ArcSessionStorage {
    async fn create(&self, session: &AuthorizationSession) -> AuthResult<()> {
        let storage = SessionStorage::new(&self.pool);
        let resource = serde_json::to_value(session)
            .map_err(|e| AuthError::storage(format!("Failed to serialize session: {}", e)))?;
        storage
            .create(session.id, resource)
            .await
            .map_err(|e| AuthError::storage(e.to_string()))?;
        Ok(())
    }

    async fn find_by_code(&self, code: &str) -> AuthResult<Option<AuthorizationSession>> {
        let storage = SessionStorage::new(&self.pool);
        let row = storage
            .find_by_code(code)
            .await
            .map_err(|e| AuthError::storage(e.to_string()))?;

        match row {
            Some(r) => Ok(Some(Self::row_to_session(r)?)),
            None => Ok(None),
        }
    }

    async fn find_by_id(&self, id: Uuid) -> AuthResult<Option<AuthorizationSession>> {
        let storage = SessionStorage::new(&self.pool);
        let row = storage
            .find_by_id(id)
            .await
            .map_err(|e| AuthError::storage(e.to_string()))?;

        match row {
            Some(r) => Ok(Some(Self::row_to_session(r)?)),
            None => Ok(None),
        }
    }

    async fn consume(&self, code: &str) -> AuthResult<AuthorizationSession> {
        let storage = SessionStorage::new(&self.pool);

        // First find the session by code
        let row = storage
            .find_by_code(code)
            .await
            .map_err(|e| AuthError::storage(e.to_string()))?
            .ok_or_else(|| AuthError::invalid_grant("Authorization code not found"))?;

        // Check if already consumed
        let session: AuthorizationSession = serde_json::from_value(row.resource.clone())
            .map_err(|e| AuthError::storage(format!("Failed to deserialize session: {}", e)))?;

        if session.consumed_at.is_some() {
            return Err(AuthError::invalid_grant("Authorization code already used"));
        }

        // Mark as used
        let id = Uuid::parse_str(&row.id)
            .map_err(|e| AuthError::storage(format!("Invalid session ID: {}", e)))?;
        let updated_row = storage
            .mark_used(id)
            .await
            .map_err(|e| AuthError::storage(e.to_string()))?;

        // Return the updated session with consumed_at set
        let mut consumed_session: AuthorizationSession =
            serde_json::from_value(updated_row.resource)
                .map_err(|e| AuthError::storage(format!("Failed to deserialize session: {}", e)))?;
        consumed_session.consumed_at = Some(OffsetDateTime::now_utc());
        Ok(consumed_session)
    }

    async fn update_user(&self, id: Uuid, user_id: Uuid) -> AuthResult<()> {
        let storage = SessionStorage::new(&self.pool);

        // Get current session
        let row = storage
            .find_by_id(id)
            .await
            .map_err(|e| AuthError::storage(e.to_string()))?
            .ok_or_else(|| AuthError::storage("Session not found"))?;

        // Update with user_id
        let mut session: AuthorizationSession = serde_json::from_value(row.resource)
            .map_err(|e| AuthError::storage(format!("Failed to deserialize session: {}", e)))?;
        session.user_id = Some(user_id);

        let resource = serde_json::to_value(&session)
            .map_err(|e| AuthError::storage(format!("Failed to serialize session: {}", e)))?;

        storage
            .update(id, resource)
            .await
            .map_err(|e| AuthError::storage(e.to_string()))?;

        Ok(())
    }

    async fn update_launch_context(
        &self,
        id: Uuid,
        launch_context: LaunchContext,
    ) -> AuthResult<()> {
        let storage = SessionStorage::new(&self.pool);

        // Get current session
        let row = storage
            .find_by_id(id)
            .await
            .map_err(|e| AuthError::storage(e.to_string()))?
            .ok_or_else(|| AuthError::storage("Session not found"))?;

        // Update with launch context
        let mut session: AuthorizationSession = serde_json::from_value(row.resource)
            .map_err(|e| AuthError::storage(format!("Failed to deserialize session: {}", e)))?;
        session.launch_context = Some(launch_context);

        let resource = serde_json::to_value(&session)
            .map_err(|e| AuthError::storage(format!("Failed to serialize session: {}", e)))?;

        storage
            .update(id, resource)
            .await
            .map_err(|e| AuthError::storage(e.to_string()))?;

        Ok(())
    }

    async fn cleanup_expired(&self) -> AuthResult<u64> {
        let storage = SessionStorage::new(&self.pool);
        storage
            .cleanup_expired()
            .await
            .map_err(|e| AuthError::storage(e.to_string()))
    }

    async fn delete_by_client(&self, client_id: &str) -> AuthResult<u64> {
        let storage = SessionStorage::new(&self.pool);
        storage
            .delete_by_client(client_id)
            .await
            .map_err(|e| AuthError::storage(e.to_string()))
    }
}

// =============================================================================
// Arc-Owning Refresh Token Storage
// =============================================================================

/// Arc-owning PostgreSQL refresh token storage adapter.
#[derive(Clone)]
pub struct ArcRefreshTokenStorage {
    pool: Arc<PgPool>,
}

impl ArcRefreshTokenStorage {
    /// Create a new Arc-owning refresh token storage.
    #[must_use]
    pub fn new(pool: Arc<PgPool>) -> Self {
        Self { pool }
    }

    /// Convert a token row to RefreshToken.
    fn row_to_token(row: crate::token::TokenRow) -> AuthResult<RefreshToken> {
        serde_json::from_value(row.resource)
            .map_err(|e| AuthError::storage(format!("Failed to deserialize refresh token: {}", e)))
    }
}

#[async_trait]
impl RefreshTokenStorageTrait for ArcRefreshTokenStorage {
    async fn create(&self, token: &RefreshToken) -> AuthResult<()> {
        let storage = TokenStorage::new(&self.pool);
        let resource = serde_json::to_value(token)
            .map_err(|e| AuthError::storage(format!("Failed to serialize refresh token: {}", e)))?;
        storage
            .create(token.id, resource)
            .await
            .map_err(|e| AuthError::storage(e.to_string()))?;
        Ok(())
    }

    async fn find_by_hash(&self, token_hash: &str) -> AuthResult<Option<RefreshToken>> {
        let storage = TokenStorage::new(&self.pool);
        let row = storage
            .find_by_hash(token_hash)
            .await
            .map_err(|e| AuthError::storage(e.to_string()))?;

        match row {
            Some(r) => Ok(Some(Self::row_to_token(r)?)),
            None => Ok(None),
        }
    }

    async fn find_by_id(&self, id: Uuid) -> AuthResult<Option<RefreshToken>> {
        let storage = TokenStorage::new(&self.pool);
        let row = storage
            .find_by_id(id)
            .await
            .map_err(|e| AuthError::storage(e.to_string()))?;

        match row {
            Some(r) => Ok(Some(Self::row_to_token(r)?)),
            None => Ok(None),
        }
    }

    async fn revoke(&self, token_hash: &str) -> AuthResult<()> {
        let storage = TokenStorage::new(&self.pool);
        storage
            .revoke_by_hash(token_hash)
            .await
            .map_err(|e| AuthError::storage(e.to_string()))
    }

    async fn revoke_by_client(&self, client_id: &str) -> AuthResult<u64> {
        let storage = TokenStorage::new(&self.pool);
        storage
            .revoke_by_client(client_id)
            .await
            .map_err(|e| AuthError::storage(e.to_string()))
    }

    async fn revoke_by_user(&self, user_id: Uuid) -> AuthResult<u64> {
        let storage = TokenStorage::new(&self.pool);
        storage
            .revoke_by_user(user_id)
            .await
            .map_err(|e| AuthError::storage(e.to_string()))
    }

    async fn cleanup_expired(&self) -> AuthResult<u64> {
        let storage = TokenStorage::new(&self.pool);
        storage
            .cleanup_expired()
            .await
            .map_err(|e| AuthError::storage(e.to_string()))
    }

    async fn list_by_user(&self, user_id: Uuid) -> AuthResult<Vec<RefreshToken>> {
        let storage = TokenStorage::new(&self.pool);
        let rows = storage
            .list_by_user(user_id)
            .await
            .map_err(|e| AuthError::storage(e.to_string()))?;

        rows.into_iter().map(Self::row_to_token).collect()
    }
}
