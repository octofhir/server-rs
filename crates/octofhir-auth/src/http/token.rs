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
use tracing::{debug, info, warn};

use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::error::AuthError;
use crate::oauth::token::{TokenError, TokenErrorCode, TokenRequest, TokenResponse};
use crate::storage::{ClientStorage, RefreshTokenStorage, RevokedTokenStorage, UserStorage};
use crate::storage::session::SessionStorage;
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
        }
    }

    /// Sets user storage for password grant support.
    #[must_use]
    pub fn with_user_storage(mut self, user_storage: Arc<dyn UserStorage>) -> Self {
        self.user_storage = Some(user_storage);
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
    let client = match authenticate_client(&state.client_storage, client_auth, &mut request).await {
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
        "client_credentials" => state.token_service.client_credentials(&request, &client).await,
        "refresh_token" => state.token_service.refresh(&request, &client).await,
        "password" => {
            password_grant(&state, &request, &client).await
        }
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
            token_success_response(response)
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
    Basic { client_id: String, client_secret: String },
    /// Client credentials in request body.
    Body { client_id: String, client_secret: String },
    /// Client assertion (JWT-based).
    Assertion { client_id: String, assertion_type: String, assertion: String },
    /// Public client (no secret).
    Public { client_id: String },
    /// No client credentials provided.
    None,
}

/// Extract client authentication from headers and request.
fn extract_client_auth(headers: &HeaderMap, request: &TokenRequest) -> ClientAuth {
    // Try HTTP Basic Auth first
    if let Some(auth_header) = headers.get("authorization") {
        if let Ok(auth_str) = auth_header.to_str() {
            if let Some(basic_creds) = auth_str.strip_prefix("Basic ") {
                if let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(basic_creds.trim()) {
                    if let Ok(creds_str) = String::from_utf8(decoded) {
                        if let Some((client_id, client_secret)) = creds_str.split_once(':') {
                            return ClientAuth::Basic {
                                client_id: client_id.to_string(),
                                client_secret: client_secret.to_string(),
                            };
                        }
                    }
                }
            }
        }
    }

    // Try client assertion (JWT)
    if let (Some(assertion_type), Some(assertion)) = (
        request.client_assertion_type.as_ref(),
        request.client_assertion.as_ref(),
    ) {
        if let Some(client_id) = request.client_id.as_ref() {
            return ClientAuth::Assertion {
                client_id: client_id.clone(),
                assertion_type: assertion_type.clone(),
                assertion: assertion.clone(),
            };
        }
    }

    // Try client_id + client_secret in body
    if let (Some(client_id), Some(client_secret)) = (
        request.client_id.as_ref(),
        request.client_secret.as_ref(),
    ) {
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
    client_storage: &Arc<dyn ClientStorage>,
    auth: ClientAuth,
    request: &mut TokenRequest,
) -> Result<crate::types::Client, AuthError> {
    let (client_id, secret) = match auth {
        ClientAuth::Basic { client_id, client_secret } => {
            // Set client_id on request for downstream processing
            request.client_id = Some(client_id.clone());
            (client_id, Some(client_secret))
        }
        ClientAuth::Body { client_id, client_secret } => {
            (client_id, Some(client_secret))
        }
        ClientAuth::Assertion { client_id, assertion_type, assertion } => {
            // TODO: Implement client assertion validation
            // For now, just look up the client
            debug!(
                client_id = %client_id,
                assertion_type = %assertion_type,
                "Client assertion authentication not fully implemented"
            );
            let _ = assertion; // Suppress unused warning
            (client_id, None)
        }
        ClientAuth::Public { client_id } => {
            (client_id, None)
        }
        ClientAuth::None => {
            return Err(AuthError::invalid_client("No client credentials provided"));
        }
    };

    // Look up the client
    let client = client_storage
        .find_by_client_id(&client_id)
        .await?
        .ok_or_else(|| AuthError::invalid_client("Unknown client"))?;

    // Check if client is active
    if !client.active {
        return Err(AuthError::invalid_client("Client is inactive"));
    }

    // Verify secret for confidential clients
    if client.confidential {
        let provided_secret = secret.ok_or_else(|| {
            AuthError::invalid_client("Client secret required for confidential client")
        })?;

        // Verify using storage (allows for hashed secrets)
        let valid = client_storage
            .verify_secret(&client_id, &provided_secret)
            .await?;

        if !valid {
            return Err(AuthError::invalid_client("Invalid client secret"));
        }
    }

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
    let username = request.username.as_ref().ok_or_else(|| {
        AuthError::invalid_request("Missing username parameter")
    })?;

    let password = request.password.as_ref().ok_or_else(|| {
        AuthError::invalid_request("Missing password parameter")
    })?;

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
    let password_valid = user_storage.verify_password(user.id, password).await?;
    if !password_valid {
        return Err(AuthError::invalid_grant("Invalid username or password"));
    }

    info!(
        user_id = %user.id,
        username = %user.username,
        client_id = %client.client_id,
        "Password authentication successful"
    );

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

    // 8. Generate access token
    let now = OffsetDateTime::now_utc();
    let access_lifetime = Duration::hours(1); // Default 1 hour for password grant

    let access_claims = AccessTokenClaims {
        iss: state.token_service.issuer().to_string(),
        sub: user.id.to_string(),
        aud: vec![state.token_service.audience().to_string()],
        exp: (now + access_lifetime).unix_timestamp(),
        iat: now.unix_timestamp(),
        jti: Uuid::new_v4().to_string(),
        scope: scope.to_string(),
        client_id: client.client_id.clone(),
        patient: None,
        encounter: None,
        fhir_user: user.fhir_user.clone(),
    };

    // Encode access token using the token service's JWT service
    let access_token = state
        .token_service
        .encode_access_token(&access_claims)
        .map_err(|e| AuthError::internal(format!("Failed to encode access token: {}", e)))?;

    // 9. Build response
    let mut response = TokenResponse::new(
        access_token,
        access_lifetime.whole_seconds() as u64,
        scope.to_string(),
    );

    // 10. Issue refresh token if:
    //     - offline_access scope is requested, OR
    //     - client has refresh_token grant type allowed
    let has_offline_access = scope.split_whitespace().any(|s| s == "offline_access");
    let client_supports_refresh = client.is_grant_type_allowed(GrantType::RefreshToken);

    if has_offline_access || client_supports_refresh {
        let refresh_token = state
            .token_service
            .issue_refresh_token_for_user(user.id, client, scope)
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
fn token_success_response(response: TokenResponse) -> Response {
    (
        StatusCode::OK,
        [
            ("Content-Type", "application/json"),
            ("Cache-Control", "no-store"),
            ("Pragma", "no-cache"),
        ],
        Json(response),
    )
        .into_response()
}

/// Build an error response for token endpoint.
fn token_error_response(error: AuthError) -> Response {
    let (code, description) = match &error {
        AuthError::InvalidClient { message, .. } => {
            (TokenErrorCode::InvalidClient, message.clone())
        }
        AuthError::InvalidGrant { message, .. } => {
            (TokenErrorCode::InvalidGrant, message.clone())
        }
        AuthError::InvalidScope { message, .. } => {
            (TokenErrorCode::InvalidScope, message.clone())
        }
        AuthError::InvalidRequest { message, .. } => {
            (TokenErrorCode::InvalidRequest, message.clone())
        }
        AuthError::UnsupportedGrantType { grant_type, .. } => {
            (
                TokenErrorCode::UnsupportedGrantType,
                format!("Grant type '{}' is not supported", grant_type),
            )
        }
        AuthError::PkceVerificationFailed => {
            (TokenErrorCode::InvalidGrant, "PKCE verification failed".to_string())
        }
        _ => {
            (TokenErrorCode::InvalidRequest, error.to_string())
        }
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
        };

        let auth = extract_client_auth(&headers, &request);
        match auth {
            ClientAuth::Basic { client_id, client_secret } => {
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
        };

        let auth = extract_client_auth(&headers, &request);
        match auth {
            ClientAuth::Body { client_id, client_secret } => {
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
