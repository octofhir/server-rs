//! OAuth 2.0 Token endpoint handler.
//!
//! This module provides the HTTP handler for the token endpoint (`/auth/token`).
//! It supports the following grant types:
//!
//! - `authorization_code` - Exchange authorization code for tokens
//! - `refresh_token` - Refresh an access token
//! - `client_credentials` - Machine-to-machine authentication
//! - `password` - Resource Owner Password Credentials (ROPC)
//!
//! # Example
//!
//! ```ignore
//! // Authorization code grant
//! POST /auth/token
//! Content-Type: application/x-www-form-urlencoded
//!
//! grant_type=authorization_code
//! &code=SplxlOBeZQQYbYS6WxSbIA
//! &redirect_uri=https://app.example.com/callback
//! &code_verifier=dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk
//! &client_id=my-app
//!
//! // Client credentials grant
//! POST /auth/token
//! Content-Type: application/x-www-form-urlencoded
//! Authorization: Basic <base64(client_id:client_secret)>
//!
//! grant_type=client_credentials
//! &scope=system/*.read
//! ```

use std::sync::Arc;

use axum::{
    Form, Json,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use base64::Engine;
use serde_json::json;
use tracing::{debug, info, warn};

use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::config::CookieConfig;
use crate::error::AuthError;
use crate::federation::ClientJwksCache;
use crate::oauth::client_assertion::ClientAssertionConfig;
use crate::oauth::token::{TokenError, TokenErrorCode, TokenRequest, TokenResponse};
use crate::storage::session::SessionStorage;
use crate::storage::{
    ClientStorage, JtiStorage, RefreshTokenStorage, RevokedTokenStorage, UserStorage,
};
use crate::token::jwt::{AccessTokenClaims, JwtService};
use crate::token::service::{TokenConfig, TokenService};
use crate::types::{Client, GrantType};

/// State required for the token endpoint.
#[derive(Clone)]
pub struct TokenState {
    /// Token service for generating tokens.
    token_service: Arc<TokenService>,
    /// Client storage for authenticating clients.
    client_storage: Arc<dyn ClientStorage>,
    /// User storage for password grant (optional).
    user_storage: Option<Arc<dyn UserStorage>>,
    /// Cookie configuration for browser-based auth.
    cookie_config: CookieConfig,
    /// JTI storage for client assertion replay prevention (optional).
    jti_storage: Option<Arc<dyn JtiStorage>>,
    /// JWKS cache for client assertion validation (optional).
    jwks_cache: Option<Arc<ClientJwksCache>>,
    /// Token endpoint URL for audience validation.
    token_endpoint_url: Option<String>,
    /// FHIR storage for creating AuthSession resources (optional).
    fhir_storage: Option<Arc<dyn octofhir_storage::FhirStorage>>,
}

impl TokenState {
    /// Creates a new token state.
    pub fn new(
        jwt_service: Arc<JwtService>,
        session_storage: Arc<dyn SessionStorage>,
        refresh_token_storage: Arc<dyn RefreshTokenStorage>,
        revoked_token_storage: Arc<dyn RevokedTokenStorage>,
        client_storage: Arc<dyn ClientStorage>,
        config: TokenConfig,
    ) -> Self {
        let token_service = Arc::new(TokenService::new(
            jwt_service,
            session_storage,
            refresh_token_storage,
            revoked_token_storage,
            config,
        ));

        Self {
            token_service,
            client_storage,
            user_storage: None,
            cookie_config: CookieConfig::default(),
            jti_storage: None,
            jwks_cache: None,
            token_endpoint_url: None,
            fhir_storage: None,
        }
    }

    /// Creates token state from existing token service.
    pub fn from_service(
        token_service: Arc<TokenService>,
        client_storage: Arc<dyn ClientStorage>,
    ) -> Self {
        Self {
            token_service,
            client_storage,
            user_storage: None,
            cookie_config: CookieConfig::default(),
            fhir_storage: None,
            jti_storage: None,
            jwks_cache: None,
            token_endpoint_url: None,
        }
    }

    /// Sets user storage for password grant support.
    #[must_use]
    pub fn with_user_storage(mut self, user_storage: Arc<dyn UserStorage>) -> Self {
        self.user_storage = Some(user_storage);
        self
    }

    /// Sets cookie configuration for browser-based authentication.
    #[must_use]
    pub fn with_cookie_config(mut self, cookie_config: CookieConfig) -> Self {
        self.cookie_config = cookie_config;
        self
    }

    /// Sets JTI storage for client assertion replay prevention.
    #[must_use]
    pub fn with_jti_storage(mut self, jti_storage: Arc<dyn JtiStorage>) -> Self {
        self.jti_storage = Some(jti_storage);
        self
    }

    /// Sets JWKS cache for client assertion validation.
    #[must_use]
    pub fn with_jwks_cache(mut self, jwks_cache: Arc<ClientJwksCache>) -> Self {
        self.jwks_cache = Some(jwks_cache);
        self
    }

    /// Sets the token endpoint URL for client assertion audience validation.
    #[must_use]
    pub fn with_token_endpoint_url(mut self, url: impl Into<String>) -> Self {
        self.token_endpoint_url = Some(url.into());
        self
    }

    /// Sets FHIR storage for creating AuthSession resources during password grant.
    #[must_use]
    pub fn with_fhir_storage(
        mut self,
        fhir_storage: Arc<dyn octofhir_storage::FhirStorage>,
    ) -> Self {
        self.fhir_storage = Some(fhir_storage);
        self
    }
}

/// OAuth 2.0 token endpoint handler.
///
/// Handles POST requests to `/auth/token` with `application/x-www-form-urlencoded` body.
///
/// # Client Authentication
///
/// Clients can authenticate using:
/// - HTTP Basic Auth header: `Authorization: Basic <base64(client_id:client_secret)>`
/// - Request body: `client_id` and `client_secret` parameters
/// - Client assertion (JWT): `client_assertion_type` and `client_assertion` parameters
/// - Public client: Just `client_id` parameter (for authorization_code with PKCE)
///
/// # Grant Types
///
/// - `authorization_code`: Requires `code`, `redirect_uri`, `code_verifier`
/// - `refresh_token`: Requires `refresh_token`
/// - `client_credentials`: Requires valid client credentials
pub async fn token_handler(
    State(state): State<TokenState>,
    headers: HeaderMap,
    Form(mut request): Form<TokenRequest>,
) -> Response {
    debug!(
        grant_type = %request.grant_type,
        client_id = ?request.client_id,
        "Processing token request"
    );

    // Extract client credentials from Authorization header or request body
    let client_auth = extract_client_auth(&headers, &request);

    // Look up and authenticate the client
    let client = match authenticate_client(&state, client_auth, &mut request).await {
        Ok(client) => client,
        Err(e) => {
            warn!(error = %e, "Client authentication failed");
            return token_error_response(e);
        }
    };

    info!(
        client_id = %client.client_id,
        grant_type = %request.grant_type,
        "Client authenticated, processing grant"
    );

    // Process the grant based on grant_type
    let result = match request.grant_type.as_str() {
        "authorization_code" => state.token_service.exchange_code(&request, &client).await,
        "client_credentials" => {
            state
                .token_service
                .client_credentials(&request, &client)
                .await
        }
        "refresh_token" => state.token_service.refresh(&request, &client).await,
        "password" => password_grant(&state, &request, &client).await,
        other => {
            warn!(grant_type = other, "Unsupported grant type");
            Err(AuthError::unsupported_grant_type(other))
        }
    };

    match result {
        Ok(response) => {
            info!(
                client_id = %client.client_id,
                grant_type = %request.grant_type,
                "Token issued successfully"
            );
            token_success_response(response, &state.cookie_config)
        }
        Err(e) => {
            warn!(
                client_id = %client.client_id,
                grant_type = %request.grant_type,
                error = %e,
                "Token request failed"
            );
            token_error_response(e)
        }
    }
}

/// Client authentication credentials extracted from the request.
enum ClientAuth {
    /// HTTP Basic authentication.
    Basic {
        client_id: String,
        client_secret: String,
    },
    /// Client credentials in request body.
    Body {
        client_id: String,
        client_secret: String,
    },
    /// Client assertion (JWT-based).
    Assertion {
        client_id: String,
        assertion_type: String,
        assertion: String,
    },
    /// Public client (no secret).
    Public { client_id: String },
    /// No client credentials provided.
    None,
}

/// Extract client authentication from headers and request.
fn extract_client_auth(headers: &HeaderMap, request: &TokenRequest) -> ClientAuth {
    // Try HTTP Basic Auth first
    if let Some(auth_header) = headers.get("authorization")
        && let Ok(auth_str) = auth_header.to_str()
        && let Some(basic_creds) = auth_str.strip_prefix("Basic ")
        && let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(basic_creds.trim())
        && let Ok(creds_str) = String::from_utf8(decoded)
        && let Some((client_id, client_secret)) = creds_str.split_once(':')
    {
        return ClientAuth::Basic {
            client_id: client_id.to_string(),
            client_secret: client_secret.to_string(),
        };
    }

    // Try client assertion (JWT)
    if let (Some(assertion_type), Some(assertion)) = (
        request.client_assertion_type.as_ref(),
        request.client_assertion.as_ref(),
    ) && let Some(client_id) = request.client_id.as_ref()
    {
        return ClientAuth::Assertion {
            client_id: client_id.clone(),
            assertion_type: assertion_type.clone(),
            assertion: assertion.clone(),
        };
    }

    // Try client_id + client_secret in body
    if let (Some(client_id), Some(client_secret)) =
        (request.client_id.as_ref(), request.client_secret.as_ref())
    {
        return ClientAuth::Body {
            client_id: client_id.clone(),
            client_secret: client_secret.clone(),
        };
    }

    // Public client (client_id only)
    if let Some(client_id) = request.client_id.as_ref() {
        return ClientAuth::Public {
            client_id: client_id.clone(),
        };
    }

    ClientAuth::None
}

/// Authenticate the client based on provided credentials.
async fn authenticate_client(
    state: &TokenState,
    auth: ClientAuth,
    request: &mut TokenRequest,
) -> Result<crate::types::Client, AuthError> {
    match auth {
        ClientAuth::Basic {
            client_id,
            client_secret,
        } => {
            // Set client_id on request for downstream processing
            request.client_id = Some(client_id.clone());
            authenticate_with_secret(state, &client_id, &client_secret).await
        }
        ClientAuth::Body {
            client_id,
            client_secret,
        } => authenticate_with_secret(state, &client_id, &client_secret).await,
        ClientAuth::Assertion {
            client_id,
            assertion_type,
            assertion,
        } => authenticate_with_assertion(state, &client_id, &assertion_type, &assertion).await,
        ClientAuth::Public { client_id } => authenticate_public_client(state, &client_id).await,
        ClientAuth::None => Err(AuthError::invalid_client("No client credentials provided")),
    }
}

/// Authenticate a client using client_id and client_secret.
async fn authenticate_with_secret(
    state: &TokenState,
    client_id: &str,
    client_secret: &str,
) -> Result<Client, AuthError> {
    let client = state
        .client_storage
        .find_by_client_id(client_id)
        .await?
        .ok_or_else(|| AuthError::invalid_client("Unknown client"))?;

    if !client.active {
        return Err(AuthError::invalid_client("Client is inactive"));
    }

    if !client.confidential {
        return Err(AuthError::invalid_client(
            "Public clients cannot use client secret authentication",
        ));
    }

    let valid = state
        .client_storage
        .verify_secret(client_id, client_secret)
        .await?;

    if !valid {
        return Err(AuthError::invalid_client("Invalid client secret"));
    }

    Ok(client)
}

/// Authenticate a public client (no secret required).
async fn authenticate_public_client(
    state: &TokenState,
    client_id: &str,
) -> Result<Client, AuthError> {
    let client = state
        .client_storage
        .find_by_client_id(client_id)
        .await?
        .ok_or_else(|| AuthError::invalid_client("Unknown client"))?;

    if !client.active {
        return Err(AuthError::invalid_client("Client is inactive"));
    }

    if client.confidential {
        return Err(AuthError::invalid_client(
            "Confidential clients must provide client credentials",
        ));
    }

    Ok(client)
}

/// Authenticate a client using JWT client assertion (private_key_jwt).
///
/// This validates the JWT assertion per RFC 7523:
/// - Verifies the assertion type is jwt-bearer
/// - Validates JWT signature against client's JWKS
/// - Checks iss, sub, aud, exp claims
/// - Prevents replay attacks via JTI tracking
async fn authenticate_with_assertion(
    state: &TokenState,
    client_id: &str,
    assertion_type: &str,
    assertion: &str,
) -> Result<Client, AuthError> {
    use crate::oauth::client_assertion::{
        ClientAssertionValidator, extract_algorithm, extract_key_id,
    };

    // 1. Validate assertion type
    if assertion_type != "urn:ietf:params:oauth:client-assertion-type:jwt-bearer" {
        return Err(AuthError::invalid_request(format!(
            "Unsupported client_assertion_type: {}",
            assertion_type
        )));
    }

    // 2. Check required dependencies are configured
    let token_endpoint = state.token_endpoint_url.as_ref().ok_or_else(|| {
        AuthError::internal("Token endpoint URL not configured for assertion validation")
    })?;

    let jwks_cache = state
        .jwks_cache
        .as_ref()
        .ok_or_else(|| AuthError::internal("JWKS cache not configured for assertion validation"))?;

    // 3. Look up client
    let client = state
        .client_storage
        .find_by_client_id(client_id)
        .await?
        .ok_or_else(|| AuthError::invalid_client("Unknown client"))?;

    if !client.active {
        return Err(AuthError::invalid_client("Client is inactive"));
    }

    // 4. Extract algorithm and key ID from JWT header
    let algorithm = extract_algorithm(assertion)?;
    let kid = extract_key_id(assertion)?;

    // 5. Get decoding key from client's JWKS
    let decoding_key = if let Some(ref jwks) = client.jwks {
        // Use inline JWKS
        jwks_cache.get_decoding_key_from_inline(jwks, kid.as_deref(), algorithm)?
    } else if let Some(ref jwks_uri) = client.jwks_uri {
        // Fetch from JWKS URI
        jwks_cache
            .get_decoding_key(jwks_uri, kid.as_deref(), algorithm)
            .await?
    } else {
        return Err(AuthError::invalid_client(
            "Client has no JWKS or JWKS URI configured for private_key_jwt authentication",
        ));
    };

    // 6. Create validator and validate claims (without JTI check)
    let config = ClientAssertionConfig::new(token_endpoint);

    // Use a dummy JTI storage just for the validator type - we'll check JTI separately
    struct DummyJtiStorage;
    #[async_trait::async_trait]
    impl crate::storage::JtiStorage for DummyJtiStorage {
        async fn mark_used(
            &self,
            _jti: &str,
            _expires_at: OffsetDateTime,
        ) -> crate::AuthResult<bool> {
            Ok(true) // Dummy - we handle JTI separately
        }
        async fn is_used(&self, _jti: &str) -> crate::AuthResult<bool> {
            Ok(false)
        }
        async fn cleanup_expired(&self) -> crate::AuthResult<u64> {
            Ok(0)
        }
    }

    let validator = ClientAssertionValidator::new(config, Arc::new(DummyJtiStorage));
    let claims = validator.validate_without_jti(assertion, client_id, &decoding_key, algorithm)?;

    // 7. Check JTI for replay prevention (if JTI storage is configured)
    if let Some(jti_storage) = &state.jti_storage {
        let expires_at = OffsetDateTime::from_unix_timestamp(claims.exp)
            .map_err(|_| AuthError::invalid_client("Invalid exp timestamp"))?;

        let is_new = jti_storage.mark_used(&claims.jti, expires_at).await?;
        if !is_new {
            return Err(AuthError::invalid_client(
                "Assertion jti already used (possible replay attack)",
            ));
        }
    } else {
        // JTI storage not configured - log warning but continue
        // This is less secure but allows assertion validation without JTI tracking
        warn!(
            client_id = %client_id,
            jti = %claims.jti,
            "JTI storage not configured - replay prevention disabled"
        );
    }

    info!(
        client_id = %client_id,
        "Client authenticated via private_key_jwt"
    );

    Ok(client)
}

/// Handle Resource Owner Password Credentials (ROPC) grant.
///
/// This authenticates the user directly with username/password and issues tokens.
///
/// # Security Warning
///
/// ROPC is considered a legacy grant type and should only be used for:
/// - Trusted first-party applications
/// - Migration scenarios
/// - Testing and development
///
/// For user-facing applications, prefer the authorization code flow with PKCE.
async fn password_grant(
    state: &TokenState,
    request: &TokenRequest,
    client: &Client,
) -> Result<TokenResponse, AuthError> {
    // 1. Validate client is allowed password grant
    if !client.is_grant_type_allowed(GrantType::Password) {
        return Err(AuthError::unauthorized(
            "Client not authorized for password grant",
        ));
    }

    // 2. Check we have user storage configured
    let user_storage = state.user_storage.as_ref().ok_or_else(|| {
        AuthError::invalid_request("Password grant not supported (no user storage configured)")
    })?;

    // 3. Extract and validate required parameters
    let username = request
        .username
        .as_ref()
        .ok_or_else(|| AuthError::invalid_request("Missing username parameter"))?;

    let password = request
        .password
        .as_ref()
        .ok_or_else(|| AuthError::invalid_request("Missing password parameter"))?;

    // 4. Find user by username
    let user = user_storage
        .find_by_username(username)
        .await?
        .ok_or_else(|| AuthError::invalid_grant("Invalid username or password"))?;

    // 5. Verify user is active
    if !user.is_active() {
        return Err(AuthError::invalid_grant("User account is inactive"));
    }

    // 6. Verify password
    let password_valid = user_storage.verify_password(&user.id, password).await?;
    if !password_valid {
        return Err(AuthError::invalid_grant("Invalid username or password"));
    }

    info!(
        user_id = %user.id,
        username = %user.username,
        client_id = %client.client_id,
        "Password authentication successful"
    );

    // Update last login timestamp
    if let Err(e) = user_storage.update_last_login(&user.id).await {
        tracing::warn!(
            user_id = %user.id,
            error = %e,
            "Failed to update last login timestamp"
        );
    }

    // 7. Validate scope (use requested or default)
    let scope = request.scope.as_deref().unwrap_or("openid");

    // Validate each scope against client's allowed scopes
    for s in scope.split_whitespace() {
        if !client.is_scope_allowed(s) {
            return Err(AuthError::invalid_scope(format!(
                "Scope '{}' not allowed for this client",
                s
            )));
        }
    }

    // 8. Create AuthSession if FHIR storage is available
    let now = OffsetDateTime::now_utc();
    let access_lifetime = Duration::hours(1); // Default 1 hour for password grant
    let expires_at = now + access_lifetime;

    let session_id = if let Some(fhir_storage) = &state.fhir_storage {
        // Create AuthSession resource
        let auth_session = json!({
            "resourceType": "AuthSession",
            "status": "active",
            "subject": {
                "reference": format!("User/{}", user.id)
            },
            "client": {
                "reference": format!("Client/{}", client.client_id),
                "display": client.name.clone()
            },
            "createdAt": now.format(&time::format_description::well_known::Rfc3339).unwrap(),
            "lastActivityAt": now.format(&time::format_description::well_known::Rfc3339).unwrap(),
            "expiresAt": expires_at.format(&time::format_description::well_known::Rfc3339).unwrap(),
        });

        match fhir_storage.create(&auth_session).await {
            Ok(stored) => {
                info!(
                    user_id = %user.id,
                    session_id = %stored.id,
                    client_id = %client.client_id,
                    "Created AuthSession for password grant"
                );
                Some(stored.id)
            }
            Err(e) => {
                warn!(
                    user_id = %user.id,
                    error = %e,
                    "Failed to create AuthSession - continuing without session tracking"
                );
                None
            }
        }
    } else {
        None
    };

    // 9. Generate access token
    let access_claims = AccessTokenClaims {
        iss: state.token_service.issuer().to_string(),
        sub: user.id.to_string(),
        aud: vec![state.token_service.audience().to_string()],
        exp: expires_at.unix_timestamp(),
        iat: now.unix_timestamp(),
        jti: Uuid::new_v4().to_string(),
        scope: scope.to_string(),
        client_id: client.client_id.clone(),
        patient: None,
        encounter: None,
        fhir_user: user.fhir_user.clone(),
        sid: session_id.clone(),
    };

    // Encode access token using the token service's JWT service
    let access_token = state
        .token_service
        .encode_access_token(&access_claims)
        .map_err(|e| AuthError::internal(format!("Failed to encode access token: {}", e)))?;

    // 10. Build response
    let mut response = TokenResponse::new(
        access_token,
        access_lifetime.whole_seconds() as u64,
        scope.to_string(),
    );

    // 11. Issue refresh token if:
    //     - offline_access scope is requested, OR
    //     - client has refresh_token grant type allowed
    let has_offline_access = scope.split_whitespace().any(|s| s == "offline_access");
    let client_supports_refresh = client.is_grant_type_allowed(GrantType::RefreshToken);

    if has_offline_access || client_supports_refresh {
        let refresh_token = state
            .token_service
            .issue_refresh_token_for_user(&user.id, client, scope)
            .await?;

        info!(
            user_id = %user.id,
            client_id = %client.client_id,
            "Refresh token issued for password grant"
        );

        response = response.with_refresh_token(refresh_token);
    }

    Ok(response)
}

/// Build a successful token response.
///
/// If cookie configuration is enabled, includes a Set-Cookie header with
/// the access token for browser-based authentication.
fn token_success_response(response: TokenResponse, cookie_config: &CookieConfig) -> Response {
    // Build the Set-Cookie header if enabled
    let cookie_header =
        cookie_config.build_cookie(&response.access_token, response.expires_in as i64);

    let mut headers = vec![
        ("Content-Type".to_string(), "application/json".to_string()),
        ("Cache-Control".to_string(), "no-store".to_string()),
        ("Pragma".to_string(), "no-cache".to_string()),
    ];

    if let Some(cookie) = cookie_header {
        debug!(cookie_name = %cookie_config.name, "Setting auth cookie");
        headers.push(("Set-Cookie".to_string(), cookie));
    }

    let mut res = Response::builder().status(StatusCode::OK);

    for (name, value) in &headers {
        res = res.header(name.as_str(), value.as_str());
    }

    res.body(axum::body::Body::from(
        serde_json::to_string(&response).unwrap_or_default(),
    ))
    .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

/// Build an error response for token endpoint.
fn token_error_response(error: AuthError) -> Response {
    let (code, description) = match &error {
        AuthError::InvalidClient { message, .. } => {
            (TokenErrorCode::InvalidClient, message.clone())
        }
        AuthError::InvalidGrant { message, .. } => (TokenErrorCode::InvalidGrant, message.clone()),
        AuthError::InvalidScope { message, .. } => (TokenErrorCode::InvalidScope, message.clone()),
        AuthError::InvalidRequest { message, .. } => {
            (TokenErrorCode::InvalidRequest, message.clone())
        }
        AuthError::UnsupportedGrantType { grant_type, .. } => (
            TokenErrorCode::UnsupportedGrantType,
            format!("Grant type '{}' is not supported", grant_type),
        ),
        AuthError::PkceVerificationFailed => (
            TokenErrorCode::InvalidGrant,
            "PKCE verification failed".to_string(),
        ),
        _ => (TokenErrorCode::InvalidRequest, error.to_string()),
    };

    let token_error = TokenError::with_description(code, description);
    let status = match code.http_status() {
        401 => StatusCode::UNAUTHORIZED,
        _ => StatusCode::BAD_REQUEST,
    };

    (
        status,
        [
            ("Content-Type", "application/json"),
            ("Cache-Control", "no-store"),
            ("Pragma", "no-cache"),
        ],
        Json(token_error),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_basic_auth() {
        let mut headers = HeaderMap::new();
        // Basic auth for "client_id:client_secret"
        let encoded = base64::engine::general_purpose::STANDARD.encode("test-client:test-secret");
        headers.insert(
            "authorization",
            format!("Basic {}", encoded).parse().unwrap(),
        );

        let request = TokenRequest {
            grant_type: "client_credentials".to_string(),
            code: None,
            redirect_uri: None,
            code_verifier: None,
            client_id: None,
            client_secret: None,
            client_assertion_type: None,
            client_assertion: None,
            refresh_token: None,
            scope: None,
            username: None,
            password: None,
        };

        let auth = extract_client_auth(&headers, &request);
        match auth {
            ClientAuth::Basic {
                client_id,
                client_secret,
            } => {
                assert_eq!(client_id, "test-client");
                assert_eq!(client_secret, "test-secret");
            }
            _ => panic!("Expected Basic auth"),
        }
    }

    #[test]
    fn test_extract_body_auth() {
        let headers = HeaderMap::new();
        let request = TokenRequest {
            grant_type: "client_credentials".to_string(),
            code: None,
            redirect_uri: None,
            code_verifier: None,
            client_id: Some("test-client".to_string()),
            client_secret: Some("test-secret".to_string()),
            client_assertion_type: None,
            client_assertion: None,
            refresh_token: None,
            scope: None,
            username: None,
            password: None,
        };

        let auth = extract_client_auth(&headers, &request);
        match auth {
            ClientAuth::Body {
                client_id,
                client_secret,
            } => {
                assert_eq!(client_id, "test-client");
                assert_eq!(client_secret, "test-secret");
            }
            _ => panic!("Expected Body auth"),
        }
    }

    #[test]
    fn test_extract_public_client() {
        let headers = HeaderMap::new();
        let request = TokenRequest {
            grant_type: "authorization_code".to_string(),
            code: Some("test-code".to_string()),
            redirect_uri: Some("https://app.example.com/callback".to_string()),
            code_verifier: Some("verifier".to_string()),
            client_id: Some("public-client".to_string()),
            client_secret: None,
            client_assertion_type: None,
            client_assertion: None,
            refresh_token: None,
            scope: None,
            username: None,
            password: None,
        };

        let auth = extract_client_auth(&headers, &request);
        match auth {
            ClientAuth::Public { client_id } => {
                assert_eq!(client_id, "public-client");
            }
            _ => panic!("Expected Public auth"),
        }
    }
}
