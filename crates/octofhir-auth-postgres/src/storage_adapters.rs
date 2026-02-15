//! Arc-owning storage adapters for use with authentication middleware.
//!
//! These adapters wrap the lifetime-based storage types and own an Arc<PgPool>,
//! allowing them to be used as `Arc<dyn Storage>` in middleware.

use std::sync::Arc;

use async_trait::async_trait;
use time::OffsetDateTime;
use uuid::Uuid;

use octofhir_auth::oauth::authorize_session::AuthorizeSession;
use octofhir_auth::oauth::session::{AuthorizationSession, LaunchContext};
use octofhir_auth::smart::launch::StoredLaunchContext;
use octofhir_auth::storage::{
    AuthorizeSessionStorage as AuthorizeSessionStorageTrait, BasicAuthStorage,
    ClientStorage as ClientStorageTrait, ConsentStorage as ConsentStorageTrait,
    LaunchContextStorage as LaunchContextStorageTrait,
    RefreshTokenStorage as RefreshTokenStorageTrait,
    RevokedTokenStorage as RevokedTokenStorageTrait, SessionStorage as SessionStorageTrait, User,
    UserConsent, UserStorage as UserStorageTrait,
};
use octofhir_auth::types::{BasicAuthEntity, Client, RefreshToken};
use octofhir_auth::{AuthError, AuthResult, verify_app_secret};

use crate::PgPool;
use crate::app::AppStorage;
use crate::authorize_session::AuthorizeSessionStorage;
use crate::client::PostgresClientStorage;
use crate::consent::ConsentStorage;
use crate::launch_context::LaunchContextStorage;
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

        // Use the ID from the row directly (it's already a String)
        user.id = row.id;

        // Set timestamps from row metadata (not stored in JSON)
        user.created_at = row.created_at;
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
    async fn find_by_id(&self, id: &str) -> AuthResult<Option<User>> {
        let storage = UserStorage::new(&self.pool);
        let row = storage
            .find_by_id_str(id)
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
            .create_str(&user.id, resource)
            .await
            .map_err(|e| AuthError::storage(e.to_string()))?;
        Ok(())
    }

    async fn update(&self, user: &User) -> AuthResult<()> {
        let storage = UserStorage::new(&self.pool);
        let resource = Self::user_to_json(user);
        storage
            .update_str(&user.id, resource)
            .await
            .map_err(|e| AuthError::storage(e.to_string()))?;
        Ok(())
    }

    async fn delete(&self, id: &str) -> AuthResult<()> {
        let storage = UserStorage::new(&self.pool);
        storage
            .delete_str(id)
            .await
            .map_err(|e| AuthError::storage(e.to_string()))
    }

    async fn verify_password(&self, user_id: &str, password: &str) -> AuthResult<bool> {
        // First, get the user to access their password hash
        tracing::debug!(user_id = %user_id, "verify_password: fetching user");
        let user = self.find_by_id(user_id).await?;

        match user {
            Some(u) => {
                tracing::debug!(
                    user_id = %user_id,
                    username = %u.username,
                    has_password_hash = u.password_hash.is_some(),
                    hash_len = u.password_hash.as_ref().map(|h| h.len()),
                    hash_prefix = ?u.password_hash.as_ref().map(|h| h.chars().take(30).collect::<String>()),
                    "verify_password: user found"
                );
                match u.password_hash {
                    Some(hash) => {
                        // Verify using Argon2
                        use argon2::{
                            Argon2,
                            password_hash::{PasswordHash, PasswordVerifier},
                        };

                        let parsed_hash = match PasswordHash::new(&hash) {
                            Ok(h) => h,
                            Err(e) => {
                                tracing::error!(
                                    user_id = %user_id,
                                    error = %e,
                                    hash_preview = ?hash.chars().take(50).collect::<String>(),
                                    "verify_password: invalid hash format"
                                );
                                return Err(AuthError::storage(format!(
                                    "Invalid password hash format: {}",
                                    e
                                )));
                            }
                        };

                        let result =
                            Argon2::default().verify_password(password.as_bytes(), &parsed_hash);
                        let verified = result.is_ok();
                        tracing::debug!(
                            user_id = %user_id,
                            verified = verified,
                            "verify_password: argon2 verification complete"
                        );
                        Ok(verified)
                    }
                    None => {
                        // User has no password (federated/SSO user)
                        tracing::warn!(user_id = %user_id, "verify_password: user has no password hash (SSO user?)");
                        Ok(false)
                    }
                }
            }
            None => {
                tracing::error!(user_id = %user_id, "verify_password: user not found");
                Err(AuthError::storage(format!("User {} not found", user_id)))
            }
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

    async fn update_last_login(&self, user_id: &str) -> AuthResult<()> {
        let storage = UserStorage::new(&self.pool);
        storage
            .update_last_login_str(user_id)
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

    async fn update_user(&self, id: Uuid, user_id: &str) -> AuthResult<()> {
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
        session.user_id = Some(user_id.to_string());

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

    async fn revoke_by_user(&self, user_id: &str) -> AuthResult<u64> {
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

    async fn list_by_user(&self, user_id: &str) -> AuthResult<Vec<RefreshToken>> {
        let storage = TokenStorage::new(&self.pool);
        let rows = storage
            .list_by_user(user_id)
            .await
            .map_err(|e| AuthError::storage(e.to_string()))?;

        rows.into_iter().map(Self::row_to_token).collect()
    }
}

// =============================================================================
// Arc-Owning Basic Auth Storage (Universal Client/App)
// =============================================================================

/// Arc-owning PostgreSQL basic auth storage adapter.
///
/// This adapter provides unified authentication for both Clients and Apps.
/// It searches both storages and verifies the provided secret.
#[derive(Clone)]
pub struct ArcBasicAuthStorage {
    pool: Arc<PgPool>,
}

impl ArcBasicAuthStorage {
    /// Create a new Arc-owning basic auth storage.
    #[must_use]
    pub fn new(pool: Arc<PgPool>) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl BasicAuthStorage for ArcBasicAuthStorage {
    async fn authenticate(
        &self,
        entity_id: &str,
        secret: &str,
    ) -> AuthResult<Option<BasicAuthEntity>> {
        // Try to find Client first
        let client_storage = PostgresClientStorage::new(&self.pool);
        if let Some(client) = client_storage.find_by_client_id(entity_id).await? {
            // Verify client secret
            let is_valid = client_storage.verify_secret(entity_id, secret).await?;
            if is_valid {
                return Ok(Some(BasicAuthEntity::Client(client)));
            }
        }

        // Try to find App
        let app_storage = AppStorage::new(&self.pool);
        if let Some((app, secret_hash)) = app_storage
            .find_by_id_with_secret(entity_id)
            .await
            .map_err(|e| AuthError::storage(e.to_string()))?
            && let Some(hash) = secret_hash
        {
            // Verify app secret
            let is_valid = verify_app_secret(secret, &hash)
                .map_err(|e| AuthError::internal(format!("Failed to verify app secret: {}", e)))?;
            if is_valid {
                return Ok(Some(BasicAuthEntity::App(app)));
            }
        }

        // Not found or invalid credentials
        Ok(None)
    }
}

// =============================================================================
// Arc-Owning Authorize Session Storage
// =============================================================================

/// Arc-owning PostgreSQL authorize session storage adapter.
///
/// This wrapper owns an `Arc<PgPool>` instead of borrowing, allowing it
/// to be used as `Arc<dyn AuthorizeSessionStorage>` in handlers.
#[derive(Clone)]
pub struct ArcAuthorizeSessionStorage {
    pool: Arc<PgPool>,
}

impl ArcAuthorizeSessionStorage {
    /// Create a new Arc-owning authorize session storage.
    #[must_use]
    pub fn new(pool: Arc<PgPool>) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl AuthorizeSessionStorageTrait for ArcAuthorizeSessionStorage {
    async fn create(&self, session: &AuthorizeSession) -> AuthResult<()> {
        let storage = AuthorizeSessionStorage::new(&self.pool);
        let authorization_request = serde_json::to_value(&session.authorization_request)
            .map_err(|e| AuthError::storage(format!("Failed to serialize request: {}", e)))?;
        storage
            .create(session.id, authorization_request, session.expires_at)
            .await
            .map_err(|e| AuthError::storage(e.to_string()))
    }

    async fn find_by_id(&self, id: Uuid) -> AuthResult<Option<AuthorizeSession>> {
        let storage = AuthorizeSessionStorage::new(&self.pool);
        let row = storage
            .find_by_id(id)
            .await
            .map_err(|e| AuthError::storage(e.to_string()))?;

        match row {
            Some(r) => {
                let authorization_request = serde_json::from_value(r.authorization_request)
                    .map_err(|e| {
                        AuthError::storage(format!("Failed to deserialize request: {}", e))
                    })?;
                Ok(Some(AuthorizeSession {
                    id: r.id,
                    user_id: r.user_id,
                    authorization_request,
                    created_at: r.created_at,
                    expires_at: r.expires_at,
                }))
            }
            None => Ok(None),
        }
    }

    async fn update_user(&self, id: Uuid, user_id: &str) -> AuthResult<()> {
        let storage = AuthorizeSessionStorage::new(&self.pool);
        storage
            .update_user(id, user_id)
            .await
            .map_err(|e| AuthError::storage(e.to_string()))
    }

    async fn delete(&self, id: Uuid) -> AuthResult<()> {
        let storage = AuthorizeSessionStorage::new(&self.pool);
        storage
            .delete(id)
            .await
            .map_err(|e| AuthError::storage(e.to_string()))
    }

    async fn cleanup_expired(&self) -> AuthResult<u64> {
        let storage = AuthorizeSessionStorage::new(&self.pool);
        storage
            .cleanup_expired()
            .await
            .map_err(|e| AuthError::storage(e.to_string()))
    }
}

// =============================================================================
// Arc-Owning Consent Storage
// =============================================================================

/// Arc-owning PostgreSQL consent storage adapter.
///
/// This wrapper owns an `Arc<PgPool>` instead of borrowing, allowing it
/// to be used as `Arc<dyn ConsentStorage>` in handlers.
#[derive(Clone)]
pub struct ArcConsentStorage {
    pool: Arc<PgPool>,
}

impl ArcConsentStorage {
    /// Create a new Arc-owning consent storage.
    #[must_use]
    pub fn new(pool: Arc<PgPool>) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl ConsentStorageTrait for ArcConsentStorage {
    async fn has_consent(
        &self,
        user_id: &str,
        client_id: &str,
        scopes: &[&str],
    ) -> AuthResult<bool> {
        let storage = ConsentStorage::new(&self.pool);
        storage
            .has_consent(user_id, client_id, scopes)
            .await
            .map_err(|e| AuthError::storage(e.to_string()))
    }

    async fn save_consent(
        &self,
        user_id: &str,
        client_id: &str,
        scopes: &[String],
    ) -> AuthResult<()> {
        let storage = ConsentStorage::new(&self.pool);
        storage
            .save_consent(user_id, client_id, scopes)
            .await
            .map_err(|e| AuthError::storage(e.to_string()))
    }

    async fn revoke_consent(&self, user_id: &str, client_id: &str) -> AuthResult<()> {
        let storage = ConsentStorage::new(&self.pool);
        storage
            .revoke_consent(user_id, client_id)
            .await
            .map_err(|e| AuthError::storage(e.to_string()))
    }

    async fn list_consents(&self, user_id: &str) -> AuthResult<Vec<UserConsent>> {
        let storage = ConsentStorage::new(&self.pool);
        let rows = storage
            .list_consents(user_id)
            .await
            .map_err(|e| AuthError::storage(e.to_string()))?;

        Ok(rows
            .into_iter()
            .map(|r| UserConsent {
                client_id: r.client_id,
                scopes: r.scopes,
                created_at: r.created_at,
                updated_at: r.updated_at,
            })
            .collect())
    }
}

// =============================================================================
// Arc-Owning Launch Context Storage
// =============================================================================

/// Arc-owning PostgreSQL launch context storage adapter.
#[derive(Clone)]
pub struct ArcLaunchContextStorage {
    pool: Arc<PgPool>,
}

impl ArcLaunchContextStorage {
    /// Create a new Arc-owning launch context storage.
    #[must_use]
    pub fn new(pool: Arc<PgPool>) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl LaunchContextStorageTrait for ArcLaunchContextStorage {
    async fn store(&self, context: &StoredLaunchContext, ttl_seconds: u64) -> AuthResult<()> {
        let storage = LaunchContextStorage::new(&self.pool);
        storage
            .store(context, ttl_seconds)
            .await
            .map_err(|e| AuthError::storage(e.to_string()))
    }

    async fn get(&self, launch_id: &str) -> AuthResult<Option<StoredLaunchContext>> {
        let storage = LaunchContextStorage::new(&self.pool);
        storage
            .get(launch_id)
            .await
            .map_err(|e| AuthError::storage(e.to_string()))
    }

    async fn consume(&self, launch_id: &str) -> AuthResult<Option<StoredLaunchContext>> {
        let storage = LaunchContextStorage::new(&self.pool);
        storage
            .consume(launch_id)
            .await
            .map_err(|e| AuthError::storage(e.to_string()))
    }

    async fn delete(&self, launch_id: &str) -> AuthResult<()> {
        let storage = LaunchContextStorage::new(&self.pool);
        storage
            .delete(launch_id)
            .await
            .map_err(|e| AuthError::storage(e.to_string()))
    }

    async fn cleanup_expired(&self) -> AuthResult<u64> {
        let storage = LaunchContextStorage::new(&self.pool);
        storage
            .cleanup_expired()
            .await
            .map_err(|e| AuthError::storage(e.to_string()))
    }
}
