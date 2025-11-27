//! Token service for generating and validating tokens.
//!
//! This module provides the token service that handles OAuth 2.0 token operations:
//!
//! - Authorization code exchange
//! - Refresh token handling
//! - Access token generation
//! - ID token generation (OpenID Connect)
//!
//! # Usage
//!
//! ```ignore
//! use octofhir_auth::token::{TokenService, TokenConfig};
//!
//! let config = TokenConfig::new("https://auth.example.com", "https://fhir.example.com/r4");
//! let service = TokenService::new(jwt_service, session_storage, refresh_storage, config);
//!
//! let response = service.exchange_code(&request, &client).await?;
//! ```

use std::sync::Arc;

use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::AuthResult;
use crate::error::AuthError;
use crate::oauth::pkce::{PkceChallenge, PkceVerifier};
use crate::oauth::session::AuthorizationSession;
use crate::oauth::token::{TokenRequest, TokenResponse};
use crate::storage::refresh_token::RefreshTokenStorage;
use crate::storage::session::SessionStorage;
use crate::token::jwt::{AccessTokenClaims, IdTokenClaims, JwtService};
use crate::types::Client;
use crate::types::refresh_token::RefreshToken;

/// Token service for generating and managing OAuth tokens.
pub struct TokenService {
    /// JWT service for encoding/decoding tokens.
    jwt_service: Arc<JwtService>,

    /// Session storage for authorization codes.
    session_storage: Arc<dyn SessionStorage>,

    /// Refresh token storage.
    refresh_token_storage: Arc<dyn RefreshTokenStorage>,

    /// Service configuration.
    config: TokenConfig,
}

/// Configuration for the token service.
#[derive(Debug, Clone)]
pub struct TokenConfig {
    /// Server issuer URL (included in tokens as `iss`).
    pub issuer: String,

    /// Default FHIR server audience URL (included in tokens as `aud`).
    pub audience: String,

    /// Default access token lifetime.
    /// Can be overridden per-client.
    pub access_token_lifetime: Duration,

    /// Default refresh token lifetime.
    /// Can be overridden per-client.
    pub refresh_token_lifetime: Duration,

    /// ID token lifetime (typically shorter than access token).
    pub id_token_lifetime: Duration,

    /// Whether to rotate refresh tokens on use.
    /// When true, the old token is revoked and a new one is issued.
    /// When false, the same token is reused.
    pub rotate_refresh_tokens: bool,
}

impl TokenConfig {
    /// Creates a new token configuration with defaults.
    ///
    /// # Arguments
    ///
    /// * `issuer` - The authorization server's issuer URL
    /// * `audience` - The default FHIR server audience URL
    #[must_use]
    pub fn new(issuer: impl Into<String>, audience: impl Into<String>) -> Self {
        Self {
            issuer: issuer.into(),
            audience: audience.into(),
            access_token_lifetime: Duration::hours(1),
            refresh_token_lifetime: Duration::days(90),
            id_token_lifetime: Duration::hours(1),
            rotate_refresh_tokens: true, // Default to rotating for security
        }
    }

    /// Sets the access token lifetime.
    #[must_use]
    pub fn with_access_token_lifetime(mut self, lifetime: Duration) -> Self {
        self.access_token_lifetime = lifetime;
        self
    }

    /// Sets the refresh token lifetime.
    #[must_use]
    pub fn with_refresh_token_lifetime(mut self, lifetime: Duration) -> Self {
        self.refresh_token_lifetime = lifetime;
        self
    }

    /// Sets the ID token lifetime.
    #[must_use]
    pub fn with_id_token_lifetime(mut self, lifetime: Duration) -> Self {
        self.id_token_lifetime = lifetime;
        self
    }

    /// Sets whether to rotate refresh tokens on use.
    #[must_use]
    pub fn with_rotate_refresh_tokens(mut self, rotate: bool) -> Self {
        self.rotate_refresh_tokens = rotate;
        self
    }
}

impl TokenService {
    /// Creates a new token service.
    ///
    /// # Arguments
    ///
    /// * `jwt_service` - Service for JWT encoding/decoding
    /// * `session_storage` - Storage for authorization sessions
    /// * `refresh_token_storage` - Storage for refresh tokens
    /// * `config` - Service configuration
    #[must_use]
    pub fn new(
        jwt_service: Arc<JwtService>,
        session_storage: Arc<dyn SessionStorage>,
        refresh_token_storage: Arc<dyn RefreshTokenStorage>,
        config: TokenConfig,
    ) -> Self {
        Self {
            jwt_service,
            session_storage,
            refresh_token_storage,
            config,
        }
    }

    /// Exchanges an authorization code for tokens.
    ///
    /// This method validates the token request, verifies PKCE, consumes
    /// the authorization code, and generates tokens.
    ///
    /// # Arguments
    ///
    /// * `request` - The token request
    /// * `client` - The authenticated client
    ///
    /// # Returns
    ///
    /// Returns a token response containing access token and optionally
    /// refresh token, ID token, and SMART context.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - `grant_type` is not "authorization_code"
    /// - Required fields are missing
    /// - Authorization code is invalid, expired, or consumed
    /// - Client ID doesn't match
    /// - Redirect URI doesn't match
    /// - PKCE verification fails
    ///
    /// # Security
    ///
    /// - Authorization codes are consumed atomically (one-time use)
    /// - PKCE is always verified (no fallback)
    /// - Tokens are never logged
    pub async fn exchange_code(
        &self,
        request: &TokenRequest,
        client: &Client,
    ) -> AuthResult<TokenResponse> {
        // 1. Validate grant type
        if request.grant_type != "authorization_code" {
            return Err(AuthError::unsupported_grant_type(&request.grant_type));
        }

        // 2. Extract required fields
        let code = request
            .code
            .as_ref()
            .ok_or_else(|| AuthError::invalid_grant("Missing code parameter"))?;

        let redirect_uri = request
            .redirect_uri
            .as_ref()
            .ok_or_else(|| AuthError::invalid_grant("Missing redirect_uri parameter"))?;

        let code_verifier = request
            .code_verifier
            .as_ref()
            .ok_or_else(|| AuthError::invalid_grant("Missing code_verifier parameter"))?;

        // 3. Find and consume session (atomic one-time use)
        let session = self.session_storage.consume(code).await.map_err(|e| {
            // Map storage errors to appropriate grant errors
            match e {
                AuthError::InvalidGrant { .. } => e,
                _ => AuthError::invalid_grant("Invalid authorization code"),
            }
        })?;

        // 4. Validate session not expired (double-check after consume)
        if session.is_expired() {
            return Err(AuthError::invalid_grant("Authorization code expired"));
        }

        // 5. Validate client ID matches
        if session.client_id != client.client_id {
            return Err(AuthError::invalid_grant(
                "Authorization code was issued to a different client",
            ));
        }

        // 6. Validate redirect URI matches exactly
        if session.redirect_uri != *redirect_uri {
            return Err(AuthError::invalid_grant(
                "Redirect URI does not match authorization request",
            ));
        }

        // 7. Verify PKCE
        let challenge = PkceChallenge::new(session.code_challenge.clone())
            .map_err(|e| AuthError::invalid_grant(format!("Invalid PKCE challenge: {}", e)))?;

        let verifier = PkceVerifier::new(code_verifier.clone())
            .map_err(|e| AuthError::invalid_grant(format!("Invalid PKCE verifier: {}", e)))?;

        challenge
            .verify(&verifier)
            .map_err(|_| AuthError::PkceVerificationFailed)?;

        // 8. Generate tokens
        self.generate_tokens(&session, client).await
    }

    /// Generates tokens for a validated session.
    async fn generate_tokens(
        &self,
        session: &AuthorizationSession,
        client: &Client,
    ) -> AuthResult<TokenResponse> {
        let now = OffsetDateTime::now_utc();

        // Determine token lifetimes (client-specific or default)
        let access_lifetime = client
            .access_token_lifetime
            .map(Duration::seconds)
            .unwrap_or(self.config.access_token_lifetime);

        let refresh_lifetime = client
            .refresh_token_lifetime
            .map(Duration::seconds)
            .unwrap_or(self.config.refresh_token_lifetime);

        // Build access token claims
        let access_claims = AccessTokenClaims {
            iss: self.config.issuer.clone(),
            sub: session.user_id.map(|u| u.to_string()).unwrap_or_default(),
            aud: vec![session.aud.clone()],
            exp: (now + access_lifetime).unix_timestamp(),
            iat: now.unix_timestamp(),
            jti: Uuid::new_v4().to_string(),
            scope: session.scope.clone(),
            client_id: client.client_id.clone(),
            patient: session
                .launch_context
                .as_ref()
                .and_then(|c| c.patient.clone()),
            encounter: session
                .launch_context
                .as_ref()
                .and_then(|c| c.encounter.clone()),
            fhir_user: None, // TODO: Load from user store
        };

        // Encode access token
        let access_token = self
            .jwt_service
            .encode(&access_claims)
            .map_err(|e| AuthError::internal(format!("Failed to encode access token: {}", e)))?;

        // Build response
        let mut response = TokenResponse::new(
            access_token,
            access_lifetime.whole_seconds() as u64,
            session.scope.clone(),
        );

        // Add SMART context from launch
        if let Some(ref ctx) = session.launch_context {
            if let Some(ref patient) = ctx.patient {
                response = response.with_patient(patient.clone());
            }
            if let Some(ref encounter) = ctx.encounter {
                response = response.with_encounter(encounter.clone());
            }
            if !ctx.fhir_context.is_empty() {
                response = response.with_fhir_context(ctx.fhir_context.clone());
            }
            if ctx.need_patient_banner {
                response = response.with_patient_banner(true);
            }
            if let Some(ref url) = ctx.smart_style_url {
                response = response.with_smart_style_url(url.clone());
            }
        }

        // Generate refresh token if offline_access scope
        if session
            .scope
            .split_whitespace()
            .any(|s| s == "offline_access")
        {
            let refresh_token = self
                .generate_refresh_token(session, client, refresh_lifetime)
                .await?;
            response = response.with_refresh_token(refresh_token);
        }

        // Generate ID token if openid scope
        if session.scope.split_whitespace().any(|s| s == "openid") {
            let id_token = self.generate_id_token(session, client)?;
            response = response.with_id_token(id_token);
        }

        Ok(response)
    }

    /// Generates and stores a refresh token.
    async fn generate_refresh_token(
        &self,
        session: &AuthorizationSession,
        client: &Client,
        lifetime: Duration,
    ) -> AuthResult<String> {
        let now = OffsetDateTime::now_utc();

        // Generate random token
        let token_value = RefreshToken::generate_token();
        let token_hash = RefreshToken::hash_token(&token_value);

        // Create refresh token record
        let refresh_token = RefreshToken {
            id: Uuid::new_v4(),
            token_hash,
            client_id: client.client_id.clone(),
            user_id: session.user_id,
            scope: session.scope.clone(),
            launch_context: session.launch_context.clone(),
            created_at: now,
            expires_at: Some(now + lifetime),
            revoked_at: None,
        };

        // Store token
        self.refresh_token_storage.create(&refresh_token).await?;

        // Return plaintext token to client
        Ok(token_value)
    }

    /// Generates an ID token (OpenID Connect).
    fn generate_id_token(
        &self,
        session: &AuthorizationSession,
        client: &Client,
    ) -> AuthResult<String> {
        let now = OffsetDateTime::now_utc();

        let claims = IdTokenClaims {
            iss: self.config.issuer.clone(),
            sub: session.user_id.map(|u| u.to_string()).unwrap_or_default(),
            aud: client.client_id.clone(),
            exp: (now + self.config.id_token_lifetime).unix_timestamp(),
            iat: now.unix_timestamp(),
            nonce: session.nonce.clone(),
            fhir_user: None, // TODO: Load from user store
        };

        self.jwt_service
            .encode(&claims)
            .map_err(|e| AuthError::internal(format!("Failed to encode ID token: {}", e)))
    }

    /// Handles client_credentials grant for Backend Services.
    ///
    /// This method generates an access token for machine-to-machine
    /// authentication without user involvement.
    ///
    /// # Arguments
    ///
    /// * `request` - The token request
    /// * `client` - The authenticated client
    ///
    /// # Returns
    ///
    /// Returns a token response containing only an access token.
    /// No refresh token or ID token is issued for client_credentials.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - `grant_type` is not "client_credentials"
    /// - Client is not authorized for this grant type
    /// - Requested scope is invalid or not allowed
    ///
    /// # Security
    ///
    /// - Only `system/*` scopes are allowed for backend services
    /// - No refresh tokens are issued
    /// - Access token lifetime is typically shorter (5 minutes default)
    pub async fn client_credentials(
        &self,
        request: &TokenRequest,
        client: &Client,
    ) -> AuthResult<TokenResponse> {
        use crate::types::GrantType;

        // 1. Validate grant type
        if request.grant_type != "client_credentials" {
            return Err(AuthError::unsupported_grant_type(&request.grant_type));
        }

        // 2. Validate client is allowed this grant
        if !client.is_grant_type_allowed(GrantType::ClientCredentials) {
            return Err(AuthError::unauthorized(
                "Client not authorized for client_credentials grant",
            ));
        }

        // 3. Validate and normalize scope
        let scope = request.scope.as_deref().unwrap_or("");
        self.validate_backend_service_scopes(scope, client)?;

        // 4. Generate access token (no user context)
        let now = OffsetDateTime::now_utc();

        // Use client-specific or default lifetime
        let access_lifetime = client
            .access_token_lifetime
            .map(Duration::seconds)
            .unwrap_or(self.config.access_token_lifetime);

        // Build access token claims
        // For client_credentials, the subject is the client itself
        let access_claims = AccessTokenClaims {
            iss: self.config.issuer.clone(),
            sub: client.client_id.clone(), // Client is the subject
            aud: vec![self.config.audience.clone()],
            exp: (now + access_lifetime).unix_timestamp(),
            iat: now.unix_timestamp(),
            jti: Uuid::new_v4().to_string(),
            scope: scope.to_string(),
            client_id: client.client_id.clone(),
            patient: None,   // No patient context for backend services
            encounter: None, // No encounter context
            fhir_user: None, // No user context
        };

        // Encode access token
        let access_token = self
            .jwt_service
            .encode(&access_claims)
            .map_err(|e| AuthError::internal(format!("Failed to encode access token: {}", e)))?;

        // Build response (no refresh token, no ID token)
        Ok(TokenResponse::new(
            access_token,
            access_lifetime.whole_seconds() as u64,
            scope.to_string(),
        ))
    }

    /// Validates scopes for backend service requests.
    ///
    /// Backend services can only request `system/*` scopes as they
    /// operate without user context.
    fn validate_backend_service_scopes(&self, scope: &str, client: &Client) -> AuthResult<()> {
        if scope.is_empty() {
            return Err(AuthError::invalid_scope(
                "Scope is required for client_credentials",
            ));
        }

        for s in scope.split_whitespace() {
            // Backend services can only use system/* scopes
            if !s.starts_with("system/") {
                return Err(AuthError::invalid_scope(format!(
                    "Backend services can only request system/* scopes, got: {}",
                    s
                )));
            }

            // Check against client's allowed scopes
            if !client.is_scope_allowed(s) {
                return Err(AuthError::invalid_scope(format!(
                    "Scope '{}' not allowed for this client",
                    s
                )));
            }
        }

        Ok(())
    }

    /// Exchanges a refresh token for a new access token.
    ///
    /// This method validates the refresh token, optionally rotates it,
    /// and generates a new access token with preserved launch context.
    ///
    /// # Arguments
    ///
    /// * `request` - The token request with `grant_type=refresh_token`
    /// * `client` - The authenticated client
    ///
    /// # Returns
    ///
    /// Returns a token response containing a new access token and
    /// optionally a new refresh token (if rotation is enabled).
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - `grant_type` is not "refresh_token"
    /// - Refresh token is missing or invalid
    /// - Token is expired or revoked
    /// - Client doesn't match the token
    /// - Requested scope exceeds original grant
    ///
    /// # Security
    ///
    /// - Token rotation is recommended to limit replay window
    /// - ID tokens are NOT reissued on refresh per spec
    /// - Launch context is preserved from original authorization
    pub async fn refresh(
        &self,
        request: &TokenRequest,
        client: &Client,
    ) -> AuthResult<TokenResponse> {
        use crate::types::GrantType;

        // 1. Validate grant type
        if request.grant_type != "refresh_token" {
            return Err(AuthError::unsupported_grant_type(&request.grant_type));
        }

        // 2. Validate client is allowed refresh_token grant
        if !client.is_grant_type_allowed(GrantType::RefreshToken) {
            return Err(AuthError::unauthorized(
                "Client not authorized for refresh_token grant",
            ));
        }

        // 3. Get refresh token from request
        let refresh_token_value = request
            .refresh_token
            .as_ref()
            .ok_or_else(|| AuthError::invalid_grant("Missing refresh_token parameter"))?;

        // 4. Hash and lookup token
        let token_hash = RefreshToken::hash_token(refresh_token_value);
        let stored_token = self
            .refresh_token_storage
            .find_by_hash(&token_hash)
            .await?
            .ok_or_else(|| AuthError::invalid_grant("Invalid refresh token"))?;

        // 5. Validate token
        self.validate_refresh_token(&stored_token, client)?;

        // 6. Determine scope (can be narrowed, not expanded)
        let scope = self.determine_refresh_scope(request, &stored_token)?;

        // 7. Generate new access token
        let now = OffsetDateTime::now_utc();

        // Use client-specific or default lifetime
        let access_lifetime = client
            .access_token_lifetime
            .map(Duration::seconds)
            .unwrap_or(self.config.access_token_lifetime);

        // Build access token claims with preserved launch context
        let access_claims = AccessTokenClaims {
            iss: self.config.issuer.clone(),
            sub: stored_token
                .user_id
                .map(|u| u.to_string())
                .unwrap_or_else(|| stored_token.client_id.clone()),
            aud: vec![self.config.audience.clone()],
            exp: (now + access_lifetime).unix_timestamp(),
            iat: now.unix_timestamp(),
            jti: Uuid::new_v4().to_string(),
            scope: scope.clone(),
            client_id: client.client_id.clone(),
            patient: stored_token
                .launch_context
                .as_ref()
                .and_then(|c| c.patient.clone()),
            encounter: stored_token
                .launch_context
                .as_ref()
                .and_then(|c| c.encounter.clone()),
            fhir_user: None, // TODO: Load from user store
        };

        // Encode access token
        let access_token = self
            .jwt_service
            .encode(&access_claims)
            .map_err(|e| AuthError::internal(format!("Failed to encode access token: {}", e)))?;

        // 8. Handle refresh token rotation
        let new_refresh_token = if self.config.rotate_refresh_tokens {
            // Revoke old token
            self.refresh_token_storage.revoke(&token_hash).await?;

            // Generate new token with same metadata
            let new_token_value = RefreshToken::generate_token();
            let new_token_hash = RefreshToken::hash_token(&new_token_value);

            let new_token = RefreshToken {
                id: Uuid::new_v4(),
                token_hash: new_token_hash,
                client_id: client.client_id.clone(),
                user_id: stored_token.user_id,
                scope: scope.clone(),
                launch_context: stored_token.launch_context.clone(),
                created_at: now,
                expires_at: stored_token.expires_at, // Keep original expiration
                revoked_at: None,
            };

            self.refresh_token_storage.create(&new_token).await?;
            Some(new_token_value)
        } else {
            // No rotation - token is reused
            None
        };

        // 9. Build response
        let mut response =
            TokenResponse::new(access_token, access_lifetime.whole_seconds() as u64, scope);

        // Add new refresh token if rotated
        if let Some(token) = new_refresh_token {
            response = response.with_refresh_token(token);
        }

        // Add preserved SMART context from launch
        if let Some(ref ctx) = stored_token.launch_context {
            if let Some(ref patient) = ctx.patient {
                response = response.with_patient(patient.clone());
            }
            if let Some(ref encounter) = ctx.encounter {
                response = response.with_encounter(encounter.clone());
            }
            if !ctx.fhir_context.is_empty() {
                response = response.with_fhir_context(ctx.fhir_context.clone());
            }
            if ctx.need_patient_banner {
                response = response.with_patient_banner(true);
            }
            if let Some(ref url) = ctx.smart_style_url {
                response = response.with_smart_style_url(url.clone());
            }
        }

        // Note: ID token is NOT reissued on refresh per OpenID Connect spec
        Ok(response)
    }

    /// Validates that a refresh token can be used.
    fn validate_refresh_token(&self, token: &RefreshToken, client: &Client) -> AuthResult<()> {
        // Check client match
        if token.client_id != client.client_id {
            return Err(AuthError::invalid_grant(
                "Refresh token was issued to a different client",
            ));
        }

        // Check revocation
        if token.is_revoked() {
            return Err(AuthError::invalid_grant("Refresh token has been revoked"));
        }

        // Check expiration
        if token.is_expired() {
            return Err(AuthError::invalid_grant("Refresh token has expired"));
        }

        Ok(())
    }

    /// Determines the scope to use for a refreshed token.
    ///
    /// Per OAuth 2.0 spec, the scope can be narrowed but not expanded.
    fn determine_refresh_scope(
        &self,
        request: &TokenRequest,
        stored_token: &RefreshToken,
    ) -> AuthResult<String> {
        match request.scope.as_deref() {
            None => {
                // No scope requested - use original
                Ok(stored_token.scope.clone())
            }
            Some(requested) => {
                // Requested scope must be subset of original
                let original_scopes: std::collections::HashSet<&str> =
                    stored_token.scope.split_whitespace().collect();
                let requested_scopes: std::collections::HashSet<&str> =
                    requested.split_whitespace().collect();

                if !requested_scopes.is_subset(&original_scopes) {
                    return Err(AuthError::invalid_scope(
                        "Requested scope exceeds original grant",
                    ));
                }

                Ok(requested.to_string())
            }
        }
    }

    /// Gets the JWT service reference.
    #[must_use]
    pub fn jwt_service(&self) -> &Arc<JwtService> {
        &self.jwt_service
    }

    /// Gets the service configuration.
    #[must_use]
    pub fn config(&self) -> &TokenConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oauth::pkce::PkceChallenge;
    use crate::oauth::session::LaunchContext;
    use crate::token::jwt::{SigningAlgorithm, SigningKeyPair};
    use crate::types::GrantType;
    use std::collections::HashMap;
    use std::sync::RwLock;

    /// Mock session storage for testing.
    struct MockSessionStorage {
        sessions: RwLock<HashMap<String, AuthorizationSession>>,
    }

    impl MockSessionStorage {
        fn new() -> Self {
            Self {
                sessions: RwLock::new(HashMap::new()),
            }
        }

        fn add_session(&self, session: AuthorizationSession) {
            self.sessions
                .write()
                .unwrap()
                .insert(session.code.clone(), session);
        }
    }

    #[async_trait::async_trait]
    impl SessionStorage for MockSessionStorage {
        async fn create(&self, session: &AuthorizationSession) -> AuthResult<()> {
            self.add_session(session.clone());
            Ok(())
        }

        async fn find_by_code(&self, code: &str) -> AuthResult<Option<AuthorizationSession>> {
            Ok(self.sessions.read().unwrap().get(code).cloned())
        }

        async fn find_by_id(&self, id: Uuid) -> AuthResult<Option<AuthorizationSession>> {
            Ok(self
                .sessions
                .read()
                .unwrap()
                .values()
                .find(|s| s.id == id)
                .cloned())
        }

        async fn consume(&self, code: &str) -> AuthResult<AuthorizationSession> {
            let mut sessions = self.sessions.write().unwrap();
            let session = sessions
                .get_mut(code)
                .ok_or_else(|| AuthError::invalid_grant("Code not found"))?;

            if session.is_consumed() {
                return Err(AuthError::invalid_grant("Code already consumed"));
            }

            session.consumed_at = Some(OffsetDateTime::now_utc());
            Ok(session.clone())
        }

        async fn update_user(&self, id: Uuid, user_id: Uuid) -> AuthResult<()> {
            let mut sessions = self.sessions.write().unwrap();
            for session in sessions.values_mut() {
                if session.id == id {
                    session.user_id = Some(user_id);
                    return Ok(());
                }
            }
            Err(AuthError::invalid_grant("Session not found"))
        }

        async fn update_launch_context(
            &self,
            id: Uuid,
            launch_context: LaunchContext,
        ) -> AuthResult<()> {
            let mut sessions = self.sessions.write().unwrap();
            for session in sessions.values_mut() {
                if session.id == id {
                    session.launch_context = Some(launch_context);
                    return Ok(());
                }
            }
            Err(AuthError::invalid_grant("Session not found"))
        }

        async fn cleanup_expired(&self) -> AuthResult<u64> {
            let mut sessions = self.sessions.write().unwrap();
            let before = sessions.len();
            sessions.retain(|_, s| !s.is_expired());
            Ok((before - sessions.len()) as u64)
        }

        async fn delete_by_client(&self, client_id: &str) -> AuthResult<u64> {
            let mut sessions = self.sessions.write().unwrap();
            let before = sessions.len();
            sessions.retain(|_, s| s.client_id != client_id);
            Ok((before - sessions.len()) as u64)
        }
    }

    /// Mock refresh token storage for testing.
    struct MockRefreshTokenStorage {
        tokens: RwLock<HashMap<String, RefreshToken>>,
    }

    impl MockRefreshTokenStorage {
        fn new() -> Self {
            Self {
                tokens: RwLock::new(HashMap::new()),
            }
        }
    }

    #[async_trait::async_trait]
    impl RefreshTokenStorage for MockRefreshTokenStorage {
        async fn create(&self, token: &RefreshToken) -> AuthResult<()> {
            self.tokens
                .write()
                .unwrap()
                .insert(token.token_hash.clone(), token.clone());
            Ok(())
        }

        async fn find_by_hash(&self, token_hash: &str) -> AuthResult<Option<RefreshToken>> {
            Ok(self.tokens.read().unwrap().get(token_hash).cloned())
        }

        async fn find_by_id(&self, id: Uuid) -> AuthResult<Option<RefreshToken>> {
            Ok(self
                .tokens
                .read()
                .unwrap()
                .values()
                .find(|t| t.id == id)
                .cloned())
        }

        async fn revoke(&self, token_hash: &str) -> AuthResult<()> {
            let mut tokens = self.tokens.write().unwrap();
            if let Some(token) = tokens.get_mut(token_hash) {
                token.revoked_at = Some(OffsetDateTime::now_utc());
            }
            Ok(())
        }

        async fn revoke_by_client(&self, client_id: &str) -> AuthResult<u64> {
            let mut tokens = self.tokens.write().unwrap();
            let mut count = 0u64;
            for token in tokens.values_mut() {
                if token.client_id == client_id && token.revoked_at.is_none() {
                    token.revoked_at = Some(OffsetDateTime::now_utc());
                    count += 1;
                }
            }
            Ok(count)
        }

        async fn revoke_by_user(&self, user_id: Uuid) -> AuthResult<u64> {
            let mut tokens = self.tokens.write().unwrap();
            let mut count = 0u64;
            for token in tokens.values_mut() {
                if token.user_id == Some(user_id) && token.revoked_at.is_none() {
                    token.revoked_at = Some(OffsetDateTime::now_utc());
                    count += 1;
                }
            }
            Ok(count)
        }

        async fn cleanup_expired(&self) -> AuthResult<u64> {
            let mut tokens = self.tokens.write().unwrap();
            let before = tokens.len();
            tokens.retain(|_, t| !t.is_expired());
            Ok((before - tokens.len()) as u64)
        }

        async fn list_by_user(&self, user_id: Uuid) -> AuthResult<Vec<RefreshToken>> {
            Ok(self
                .tokens
                .read()
                .unwrap()
                .values()
                .filter(|t| t.user_id == Some(user_id) && t.is_valid())
                .cloned()
                .collect())
        }
    }

    fn create_test_client() -> Client {
        Client {
            client_id: "test-client".to_string(),
            client_secret: None,
            name: "Test Client".to_string(),
            description: None,
            grant_types: vec![GrantType::AuthorizationCode],
            redirect_uris: vec!["https://app.example.com/callback".to_string()],
            scopes: vec![],
            confidential: false,
            active: true,
            access_token_lifetime: None,
            refresh_token_lifetime: None,
            pkce_required: None,
            allowed_origins: vec![],
            jwks: None,
            jwks_uri: None,
        }
    }

    fn create_test_session(code_verifier: &str) -> AuthorizationSession {
        let verifier = PkceVerifier::new(code_verifier.to_string()).unwrap();
        let challenge = PkceChallenge::from_verifier(&verifier);
        let now = OffsetDateTime::now_utc();

        AuthorizationSession {
            id: Uuid::new_v4(),
            code: "test-auth-code".to_string(),
            client_id: "test-client".to_string(),
            redirect_uri: "https://app.example.com/callback".to_string(),
            scope: "openid patient/*.read".to_string(),
            state: "test-state".to_string(),
            code_challenge: challenge.into_inner(),
            code_challenge_method: "S256".to_string(),
            user_id: Some(Uuid::new_v4()),
            launch_context: None,
            nonce: Some("test-nonce".to_string()),
            aud: "https://fhir.example.com/r4".to_string(),
            created_at: now,
            expires_at: now + Duration::minutes(10),
            consumed_at: None,
        }
    }

    fn create_test_service() -> (
        TokenService,
        Arc<MockSessionStorage>,
        Arc<MockRefreshTokenStorage>,
    ) {
        let key_pair = SigningKeyPair::generate_rsa(SigningAlgorithm::RS256).unwrap();
        let jwt_service = Arc::new(JwtService::new(key_pair, "https://auth.example.com"));

        let session_storage = Arc::new(MockSessionStorage::new());
        let refresh_storage = Arc::new(MockRefreshTokenStorage::new());

        let config = TokenConfig::new("https://auth.example.com", "https://fhir.example.com/r4");

        let service = TokenService::new(
            jwt_service,
            session_storage.clone(),
            refresh_storage.clone(),
            config,
        );

        (service, session_storage, refresh_storage)
    }

    #[tokio::test]
    async fn test_exchange_code_success() {
        let (service, session_storage, _) = create_test_service();
        let client = create_test_client();

        // Use a valid PKCE verifier (43-128 chars)
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        let session = create_test_session(verifier);
        session_storage.add_session(session);

        let request = TokenRequest {
            grant_type: "authorization_code".to_string(),
            code: Some("test-auth-code".to_string()),
            redirect_uri: Some("https://app.example.com/callback".to_string()),
            code_verifier: Some(verifier.to_string()),
            client_id: Some("test-client".to_string()),
            client_secret: None,
            client_assertion_type: None,
            client_assertion: None,
            refresh_token: None,
            scope: None,
        };

        let result = service.exchange_code(&request, &client).await;
        assert!(result.is_ok());

        let response = result.unwrap();
        assert!(!response.access_token.is_empty());
        assert_eq!(response.token_type, "Bearer");
        assert!(response.expires_in > 0);
        assert_eq!(response.scope, "openid patient/*.read");
        assert!(response.id_token.is_some()); // openid scope
    }

    #[tokio::test]
    async fn test_exchange_code_invalid_grant_type() {
        let (service, _, _) = create_test_service();
        let client = create_test_client();

        let request = TokenRequest {
            grant_type: "refresh_token".to_string(),
            code: Some("test-auth-code".to_string()),
            redirect_uri: None,
            code_verifier: None,
            client_id: None,
            client_secret: None,
            client_assertion_type: None,
            client_assertion: None,
            refresh_token: None,
            scope: None,
        };

        let result = service.exchange_code(&request, &client).await;
        assert!(matches!(
            result,
            Err(AuthError::UnsupportedGrantType { .. })
        ));
    }

    #[tokio::test]
    async fn test_exchange_code_missing_code() {
        let (service, _, _) = create_test_service();
        let client = create_test_client();

        let request = TokenRequest {
            grant_type: "authorization_code".to_string(),
            code: None,
            redirect_uri: Some("https://app.example.com/callback".to_string()),
            code_verifier: Some("verifier".to_string()),
            client_id: None,
            client_secret: None,
            client_assertion_type: None,
            client_assertion: None,
            refresh_token: None,
            scope: None,
        };

        let result = service.exchange_code(&request, &client).await;
        assert!(matches!(result, Err(AuthError::InvalidGrant { .. })));
    }

    #[tokio::test]
    async fn test_exchange_code_invalid_code() {
        let (service, _, _) = create_test_service();
        let client = create_test_client();

        let request = TokenRequest {
            grant_type: "authorization_code".to_string(),
            code: Some("invalid-code".to_string()),
            redirect_uri: Some("https://app.example.com/callback".to_string()),
            code_verifier: Some("verifier".to_string()),
            client_id: None,
            client_secret: None,
            client_assertion_type: None,
            client_assertion: None,
            refresh_token: None,
            scope: None,
        };

        let result = service.exchange_code(&request, &client).await;
        assert!(matches!(result, Err(AuthError::InvalidGrant { .. })));
    }

    #[tokio::test]
    async fn test_exchange_code_client_mismatch() {
        let (service, session_storage, _) = create_test_service();

        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        let session = create_test_session(verifier);
        session_storage.add_session(session);

        // Different client
        let mut client = create_test_client();
        client.client_id = "different-client".to_string();

        let request = TokenRequest {
            grant_type: "authorization_code".to_string(),
            code: Some("test-auth-code".to_string()),
            redirect_uri: Some("https://app.example.com/callback".to_string()),
            code_verifier: Some(verifier.to_string()),
            client_id: None,
            client_secret: None,
            client_assertion_type: None,
            client_assertion: None,
            refresh_token: None,
            scope: None,
        };

        let result = service.exchange_code(&request, &client).await;
        assert!(matches!(result, Err(AuthError::InvalidGrant { .. })));
    }

    #[tokio::test]
    async fn test_exchange_code_redirect_uri_mismatch() {
        let (service, session_storage, _) = create_test_service();
        let client = create_test_client();

        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        let session = create_test_session(verifier);
        session_storage.add_session(session);

        let request = TokenRequest {
            grant_type: "authorization_code".to_string(),
            code: Some("test-auth-code".to_string()),
            redirect_uri: Some("https://evil.example.com/callback".to_string()), // Wrong URI
            code_verifier: Some(verifier.to_string()),
            client_id: None,
            client_secret: None,
            client_assertion_type: None,
            client_assertion: None,
            refresh_token: None,
            scope: None,
        };

        let result = service.exchange_code(&request, &client).await;
        assert!(matches!(result, Err(AuthError::InvalidGrant { .. })));
    }

    #[tokio::test]
    async fn test_exchange_code_pkce_failure() {
        let (service, session_storage, _) = create_test_service();
        let client = create_test_client();

        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        let session = create_test_session(verifier);
        session_storage.add_session(session);

        let request = TokenRequest {
            grant_type: "authorization_code".to_string(),
            code: Some("test-auth-code".to_string()),
            redirect_uri: Some("https://app.example.com/callback".to_string()),
            code_verifier: Some("wrong-verifier-that-is-long-enough-for-pkce".to_string()), // Wrong verifier
            client_id: None,
            client_secret: None,
            client_assertion_type: None,
            client_assertion: None,
            refresh_token: None,
            scope: None,
        };

        let result = service.exchange_code(&request, &client).await;
        assert!(matches!(result, Err(AuthError::PkceVerificationFailed)));
    }

    #[tokio::test]
    async fn test_exchange_code_with_offline_access() {
        let (service, session_storage, refresh_storage) = create_test_service();
        let client = create_test_client();

        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        let mut session = create_test_session(verifier);
        session.scope = "openid offline_access patient/*.read".to_string();
        session_storage.add_session(session);

        let request = TokenRequest {
            grant_type: "authorization_code".to_string(),
            code: Some("test-auth-code".to_string()),
            redirect_uri: Some("https://app.example.com/callback".to_string()),
            code_verifier: Some(verifier.to_string()),
            client_id: None,
            client_secret: None,
            client_assertion_type: None,
            client_assertion: None,
            refresh_token: None,
            scope: None,
        };

        let result = service.exchange_code(&request, &client).await;
        assert!(result.is_ok());

        let response = result.unwrap();
        assert!(response.refresh_token.is_some());

        // Verify refresh token was stored
        let stored_tokens = refresh_storage.tokens.read().unwrap();
        assert_eq!(stored_tokens.len(), 1);
    }

    #[tokio::test]
    async fn test_exchange_code_with_launch_context() {
        let (service, session_storage, _) = create_test_service();
        let client = create_test_client();

        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        let mut session = create_test_session(verifier);
        session.launch_context = Some(LaunchContext {
            patient: Some("Patient/123".to_string()),
            encounter: Some("Encounter/456".to_string()),
            fhir_context: vec![],
            need_patient_banner: true,
            smart_style_url: Some("https://style.example.com".to_string()),
            intent: None,
        });
        session_storage.add_session(session);

        let request = TokenRequest {
            grant_type: "authorization_code".to_string(),
            code: Some("test-auth-code".to_string()),
            redirect_uri: Some("https://app.example.com/callback".to_string()),
            code_verifier: Some(verifier.to_string()),
            client_id: None,
            client_secret: None,
            client_assertion_type: None,
            client_assertion: None,
            refresh_token: None,
            scope: None,
        };

        let result = service.exchange_code(&request, &client).await;
        assert!(result.is_ok());

        let response = result.unwrap();
        assert_eq!(response.patient, Some("Patient/123".to_string()));
        assert_eq!(response.encounter, Some("Encounter/456".to_string()));
        assert_eq!(response.need_patient_banner, Some(true));
        assert_eq!(
            response.smart_style_url,
            Some("https://style.example.com".to_string())
        );
    }

    #[test]
    fn test_token_config_defaults() {
        let config = TokenConfig::new("https://auth.example.com", "https://fhir.example.com");

        assert_eq!(config.issuer, "https://auth.example.com");
        assert_eq!(config.audience, "https://fhir.example.com");
        assert_eq!(config.access_token_lifetime, Duration::hours(1));
        assert_eq!(config.refresh_token_lifetime, Duration::days(90));
        assert!(config.rotate_refresh_tokens);
    }

    #[test]
    fn test_token_config_builder() {
        let config = TokenConfig::new("https://auth.example.com", "https://fhir.example.com")
            .with_access_token_lifetime(Duration::minutes(30))
            .with_refresh_token_lifetime(Duration::days(30))
            .with_id_token_lifetime(Duration::minutes(15))
            .with_rotate_refresh_tokens(false);

        assert_eq!(config.access_token_lifetime, Duration::minutes(30));
        assert_eq!(config.refresh_token_lifetime, Duration::days(30));
        assert_eq!(config.id_token_lifetime, Duration::minutes(15));
        assert!(!config.rotate_refresh_tokens);
    }

    // =========================================================================
    // Refresh Token Flow Tests
    // =========================================================================

    fn create_refresh_client() -> Client {
        Client {
            client_id: "test-client".to_string(),
            client_secret: None,
            name: "Test Client".to_string(),
            description: None,
            grant_types: vec![GrantType::AuthorizationCode, GrantType::RefreshToken],
            redirect_uris: vec!["https://app.example.com/callback".to_string()],
            scopes: vec![],
            confidential: false,
            active: true,
            access_token_lifetime: None,
            refresh_token_lifetime: None,
            pkce_required: None,
            allowed_origins: vec![],
            jwks: None,
            jwks_uri: None,
        }
    }

    fn create_stored_refresh_token(
        client_id: &str,
        scope: &str,
        token_value: &str,
    ) -> RefreshToken {
        let now = OffsetDateTime::now_utc();
        RefreshToken {
            id: Uuid::new_v4(),
            token_hash: RefreshToken::hash_token(token_value),
            client_id: client_id.to_string(),
            user_id: Some(Uuid::new_v4()),
            scope: scope.to_string(),
            launch_context: None,
            created_at: now,
            expires_at: Some(now + Duration::days(90)),
            revoked_at: None,
        }
    }

    #[tokio::test]
    async fn test_refresh_success() {
        let (service, _, refresh_storage) = create_test_service();
        let client = create_refresh_client();

        // Store a refresh token
        let token_value = "test-refresh-token-value-1234567890abc";
        let stored_token =
            create_stored_refresh_token(&client.client_id, "openid patient/*.read", token_value);
        refresh_storage.create(&stored_token).await.unwrap();

        let request = TokenRequest {
            grant_type: "refresh_token".to_string(),
            code: None,
            redirect_uri: None,
            code_verifier: None,
            client_id: None,
            client_secret: None,
            client_assertion_type: None,
            client_assertion: None,
            refresh_token: Some(token_value.to_string()),
            scope: None,
        };

        let result = service.refresh(&request, &client).await;
        assert!(result.is_ok());

        let response = result.unwrap();
        assert!(!response.access_token.is_empty());
        assert_eq!(response.token_type, "Bearer");
        assert!(response.expires_in > 0);
        assert_eq!(response.scope, "openid patient/*.read");
        // Token rotation is on by default, so new refresh token should be issued
        assert!(response.refresh_token.is_some());
        // ID token NOT reissued on refresh
        assert!(response.id_token.is_none());
    }

    #[tokio::test]
    async fn test_refresh_invalid_grant_type() {
        let (service, _, _) = create_test_service();
        let client = create_refresh_client();

        let request = TokenRequest {
            grant_type: "authorization_code".to_string(),
            code: None,
            redirect_uri: None,
            code_verifier: None,
            client_id: None,
            client_secret: None,
            client_assertion_type: None,
            client_assertion: None,
            refresh_token: Some("token".to_string()),
            scope: None,
        };

        let result = service.refresh(&request, &client).await;
        assert!(matches!(
            result,
            Err(AuthError::UnsupportedGrantType { .. })
        ));
    }

    #[tokio::test]
    async fn test_refresh_missing_token() {
        let (service, _, _) = create_test_service();
        let client = create_refresh_client();

        let request = TokenRequest {
            grant_type: "refresh_token".to_string(),
            code: None,
            redirect_uri: None,
            code_verifier: None,
            client_id: None,
            client_secret: None,
            client_assertion_type: None,
            client_assertion: None,
            refresh_token: None,
            scope: None,
        };

        let result = service.refresh(&request, &client).await;
        assert!(matches!(result, Err(AuthError::InvalidGrant { .. })));
    }

    #[tokio::test]
    async fn test_refresh_unknown_token() {
        let (service, _, _) = create_test_service();
        let client = create_refresh_client();

        let request = TokenRequest {
            grant_type: "refresh_token".to_string(),
            code: None,
            redirect_uri: None,
            code_verifier: None,
            client_id: None,
            client_secret: None,
            client_assertion_type: None,
            client_assertion: None,
            refresh_token: Some("unknown-token".to_string()),
            scope: None,
        };

        let result = service.refresh(&request, &client).await;
        assert!(matches!(result, Err(AuthError::InvalidGrant { .. })));
    }

    #[tokio::test]
    async fn test_refresh_expired_token() {
        let (service, _, refresh_storage) = create_test_service();
        let client = create_refresh_client();

        // Create an expired token
        let token_value = "expired-refresh-token-value";
        let now = OffsetDateTime::now_utc();
        let expired_token = RefreshToken {
            id: Uuid::new_v4(),
            token_hash: RefreshToken::hash_token(token_value),
            client_id: client.client_id.clone(),
            user_id: Some(Uuid::new_v4()),
            scope: "openid".to_string(),
            launch_context: None,
            created_at: now - Duration::days(100),
            expires_at: Some(now - Duration::days(1)), // Expired yesterday
            revoked_at: None,
        };
        refresh_storage.create(&expired_token).await.unwrap();

        let request = TokenRequest {
            grant_type: "refresh_token".to_string(),
            code: None,
            redirect_uri: None,
            code_verifier: None,
            client_id: None,
            client_secret: None,
            client_assertion_type: None,
            client_assertion: None,
            refresh_token: Some(token_value.to_string()),
            scope: None,
        };

        let result = service.refresh(&request, &client).await;
        assert!(matches!(result, Err(AuthError::InvalidGrant { .. })));
    }

    #[tokio::test]
    async fn test_refresh_revoked_token() {
        let (service, _, refresh_storage) = create_test_service();
        let client = create_refresh_client();

        // Create a revoked token
        let token_value = "revoked-refresh-token-value";
        let now = OffsetDateTime::now_utc();
        let revoked_token = RefreshToken {
            id: Uuid::new_v4(),
            token_hash: RefreshToken::hash_token(token_value),
            client_id: client.client_id.clone(),
            user_id: Some(Uuid::new_v4()),
            scope: "openid".to_string(),
            launch_context: None,
            created_at: now,
            expires_at: Some(now + Duration::days(90)),
            revoked_at: Some(now), // Revoked now
        };
        refresh_storage.create(&revoked_token).await.unwrap();

        let request = TokenRequest {
            grant_type: "refresh_token".to_string(),
            code: None,
            redirect_uri: None,
            code_verifier: None,
            client_id: None,
            client_secret: None,
            client_assertion_type: None,
            client_assertion: None,
            refresh_token: Some(token_value.to_string()),
            scope: None,
        };

        let result = service.refresh(&request, &client).await;
        assert!(matches!(result, Err(AuthError::InvalidGrant { .. })));
    }

    #[tokio::test]
    async fn test_refresh_client_mismatch() {
        let (service, _, refresh_storage) = create_test_service();
        let client = create_refresh_client();

        // Token issued to a different client
        let token_value = "other-client-token-value";
        let token = create_stored_refresh_token("other-client", "openid", token_value);
        refresh_storage.create(&token).await.unwrap();

        let request = TokenRequest {
            grant_type: "refresh_token".to_string(),
            code: None,
            redirect_uri: None,
            code_verifier: None,
            client_id: None,
            client_secret: None,
            client_assertion_type: None,
            client_assertion: None,
            refresh_token: Some(token_value.to_string()),
            scope: None,
        };

        let result = service.refresh(&request, &client).await;
        assert!(matches!(result, Err(AuthError::InvalidGrant { .. })));
    }

    #[tokio::test]
    async fn test_refresh_scope_narrowing() {
        let (service, _, refresh_storage) = create_test_service();
        let client = create_refresh_client();

        let token_value = "scope-narrowing-token";
        let token = create_stored_refresh_token(
            &client.client_id,
            "openid patient/*.read patient/*.write",
            token_value,
        );
        refresh_storage.create(&token).await.unwrap();

        // Request a narrower scope
        let request = TokenRequest {
            grant_type: "refresh_token".to_string(),
            code: None,
            redirect_uri: None,
            code_verifier: None,
            client_id: None,
            client_secret: None,
            client_assertion_type: None,
            client_assertion: None,
            refresh_token: Some(token_value.to_string()),
            scope: Some("openid patient/*.read".to_string()), // Narrower
        };

        let result = service.refresh(&request, &client).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().scope, "openid patient/*.read");
    }

    #[tokio::test]
    async fn test_refresh_scope_expansion_rejected() {
        let (service, _, refresh_storage) = create_test_service();
        let client = create_refresh_client();

        let token_value = "scope-expansion-token";
        let token =
            create_stored_refresh_token(&client.client_id, "openid patient/*.read", token_value);
        refresh_storage.create(&token).await.unwrap();

        // Request a broader scope
        let request = TokenRequest {
            grant_type: "refresh_token".to_string(),
            code: None,
            redirect_uri: None,
            code_verifier: None,
            client_id: None,
            client_secret: None,
            client_assertion_type: None,
            client_assertion: None,
            refresh_token: Some(token_value.to_string()),
            scope: Some("openid patient/*.read patient/*.write".to_string()), // Broader
        };

        let result = service.refresh(&request, &client).await;
        assert!(matches!(result, Err(AuthError::InvalidScope { .. })));
    }

    #[tokio::test]
    async fn test_refresh_with_launch_context() {
        let (service, _, refresh_storage) = create_test_service();
        let client = create_refresh_client();

        let token_value = "launch-context-token";
        let now = OffsetDateTime::now_utc();
        let token_with_context = RefreshToken {
            id: Uuid::new_v4(),
            token_hash: RefreshToken::hash_token(token_value),
            client_id: client.client_id.clone(),
            user_id: Some(Uuid::new_v4()),
            scope: "openid launch/patient patient/*.read".to_string(),
            launch_context: Some(LaunchContext {
                patient: Some("Patient/123".to_string()),
                encounter: Some("Encounter/456".to_string()),
                fhir_context: vec![],
                need_patient_banner: true,
                smart_style_url: Some("https://style.example.com".to_string()),
                intent: None,
            }),
            created_at: now,
            expires_at: Some(now + Duration::days(90)),
            revoked_at: None,
        };
        refresh_storage.create(&token_with_context).await.unwrap();

        let request = TokenRequest {
            grant_type: "refresh_token".to_string(),
            code: None,
            redirect_uri: None,
            code_verifier: None,
            client_id: None,
            client_secret: None,
            client_assertion_type: None,
            client_assertion: None,
            refresh_token: Some(token_value.to_string()),
            scope: None,
        };

        let result = service.refresh(&request, &client).await;
        assert!(result.is_ok());

        let response = result.unwrap();
        // Launch context should be preserved
        assert_eq!(response.patient, Some("Patient/123".to_string()));
        assert_eq!(response.encounter, Some("Encounter/456".to_string()));
        assert_eq!(response.need_patient_banner, Some(true));
        assert_eq!(
            response.smart_style_url,
            Some("https://style.example.com".to_string())
        );
    }

    #[tokio::test]
    async fn test_refresh_token_rotation() {
        let (service, _, refresh_storage) = create_test_service();
        let client = create_refresh_client();

        let token_value = "rotation-test-token";
        let token = create_stored_refresh_token(&client.client_id, "openid", token_value);
        let original_hash = token.token_hash.clone();
        refresh_storage.create(&token).await.unwrap();

        let request = TokenRequest {
            grant_type: "refresh_token".to_string(),
            code: None,
            redirect_uri: None,
            code_verifier: None,
            client_id: None,
            client_secret: None,
            client_assertion_type: None,
            client_assertion: None,
            refresh_token: Some(token_value.to_string()),
            scope: None,
        };

        let result = service.refresh(&request, &client).await;
        assert!(result.is_ok());

        let response = result.unwrap();
        // New refresh token should be issued
        assert!(response.refresh_token.is_some());
        let new_token = response.refresh_token.unwrap();
        assert_ne!(new_token, token_value); // Different value

        // Old token should be revoked
        let old_token = refresh_storage.find_by_hash(&original_hash).await.unwrap();
        assert!(old_token.unwrap().is_revoked());

        // New token should be stored
        let new_hash = RefreshToken::hash_token(&new_token);
        let stored_new = refresh_storage.find_by_hash(&new_hash).await.unwrap();
        assert!(stored_new.is_some());
        assert!(stored_new.unwrap().is_valid());
    }

    #[tokio::test]
    async fn test_refresh_client_not_authorized() {
        let (service, _, refresh_storage) = create_test_service();

        // Client without refresh_token grant
        let client = create_test_client(); // Only has authorization_code

        let token_value = "unauthorized-client-token";
        let token = create_stored_refresh_token(&client.client_id, "openid", token_value);
        refresh_storage.create(&token).await.unwrap();

        let request = TokenRequest {
            grant_type: "refresh_token".to_string(),
            code: None,
            redirect_uri: None,
            code_verifier: None,
            client_id: None,
            client_secret: None,
            client_assertion_type: None,
            client_assertion: None,
            refresh_token: Some(token_value.to_string()),
            scope: None,
        };

        let result = service.refresh(&request, &client).await;
        assert!(matches!(result, Err(AuthError::Unauthorized { .. })));
    }
}
