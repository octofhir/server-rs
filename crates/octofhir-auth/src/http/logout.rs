//! Logout endpoint handlers for browser-based authentication.
//!
//! This module provides Axum handlers for the `/auth/logout` endpoint supporting both:
//! - POST requests (API-style logout with JSON response)
//! - GET requests (OIDC RP-Initiated Logout 1.0 with redirect)
//!
//! # OIDC RP-Initiated Logout 1.0
//!
//! Per [OpenID Connect RP-Initiated Logout 1.0](https://openid.net/specs/openid-connect-rpinitiated-1_0.html),
//! the logout endpoint MUST support both GET and POST methods. The GET handler accepts
//! the following query parameters:
//!
//! - `id_token_hint` (RECOMMENDED): ID Token previously issued to identify the session
//! - `post_logout_redirect_uri` (OPTIONAL): URI to redirect after logout
//! - `client_id` (OPTIONAL): Client identifier (required if no id_token_hint)
//! - `state` (OPTIONAL): Opaque value passed back in redirect
//! - `ui_locales` (OPTIONAL): Preferred languages for UI (not implemented)
//!
//! # Usage
//!
//! ```ignore
//! use axum::{Router, routing::{get, post}};
//! use octofhir_auth::http::{logout_handler, oidc_logout_handler};
//!
//! let app = Router::new()
//!     .route("/auth/logout", get(oidc_logout_handler).post(logout_handler))
//!     .with_state(logout_state);
//! ```
//!
//! # Security Considerations
//!
//! Per the spec:
//! - The OP MUST NOT redirect to `post_logout_redirect_uri` unless it matches a registered value
//! - If `id_token_hint` is not provided with `post_logout_redirect_uri`, the OP must have
//!   other means of confirming the legitimacy of the redirect target
//! - When `client_id` and `id_token_hint` are both present, client_id must match the token's audience

use std::sync::Arc;

use axum::{
    Json,
    extract::{Query, State},
    http::{HeaderMap, StatusCode, header::AUTHORIZATION, header::COOKIE},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};
use url::form_urlencoded;

use crate::config::{CookieConfig, SessionConfig};
use crate::storage::{ClientStorage, RevokedTokenStorage, SsoSessionStorage};
use crate::token::jwt::{AccessTokenClaims, IdTokenClaims, JwtService};

// =============================================================================
// State Types
// =============================================================================

/// Callback for JWT cache invalidation when a token is revoked.
///
/// This allows the server to hook in cache invalidation without
/// creating a dependency from octofhir-auth to octofhir-server.
pub type TokenRevokedCallback = Arc<dyn Fn(&str) + Send + Sync>;

/// State required for the logout endpoint.
#[derive(Clone)]
pub struct LogoutState {
    /// JWT service for token validation.
    pub jwt_service: Arc<JwtService>,
    /// Revoked token storage for marking tokens as revoked.
    pub revoked_token_storage: Arc<dyn RevokedTokenStorage>,
    /// Cookie configuration for clearing the auth cookie.
    pub cookie_config: CookieConfig,
    /// SSO session storage for revoking authentication sessions.
    pub sso_session_storage: Arc<dyn SsoSessionStorage>,
    /// Session configuration for SSO cookie settings.
    pub session_config: SessionConfig,
    /// Client storage for validating post_logout_redirect_uri.
    pub client_storage: Arc<dyn ClientStorage>,
    /// Optional callback invoked when a token is revoked (for cache invalidation).
    pub on_token_revoked: Option<TokenRevokedCallback>,
}

impl LogoutState {
    /// Creates a new logout state.
    pub fn new(
        jwt_service: Arc<JwtService>,
        revoked_token_storage: Arc<dyn RevokedTokenStorage>,
        cookie_config: CookieConfig,
        sso_session_storage: Arc<dyn SsoSessionStorage>,
        session_config: SessionConfig,
        client_storage: Arc<dyn ClientStorage>,
    ) -> Self {
        Self {
            jwt_service,
            revoked_token_storage,
            cookie_config,
            sso_session_storage,
            session_config,
            client_storage,
            on_token_revoked: None,
        }
    }

    /// Sets the callback invoked when a token is revoked.
    ///
    /// This is called with the JTI of the revoked token, allowing
    /// the server to invalidate its JWT verification cache immediately
    /// instead of waiting for cache TTL expiration.
    pub fn with_token_revoked_callback(mut self, callback: TokenRevokedCallback) -> Self {
        self.on_token_revoked = Some(callback);
        self
    }
}

// =============================================================================
// Request/Response Types
// =============================================================================

/// OIDC RP-Initiated Logout query parameters.
///
/// Per OpenID Connect RP-Initiated Logout 1.0 specification.
#[derive(Debug, Deserialize)]
pub struct OidcLogoutParams {
    /// ID Token previously issued by the OP to the RP.
    /// Used as a hint about the End-User's current authenticated session.
    #[serde(default)]
    pub id_token_hint: Option<String>,

    /// Hint about the End-User that is logging out.
    /// Could be email, username, or session identifier.
    #[serde(default)]
    pub logout_hint: Option<String>,

    /// OAuth 2.0 Client Identifier.
    /// When both client_id and id_token_hint are present, must match the token's audience.
    #[serde(default)]
    pub client_id: Option<String>,

    /// URI to redirect the User Agent after logout.
    /// Must exactly match a registered post_logout_redirect_uri for the client.
    #[serde(default)]
    pub post_logout_redirect_uri: Option<String>,

    /// Opaque value used to maintain state between the logout request and callback.
    #[serde(default)]
    pub state: Option<String>,

    /// End-User's preferred languages for the user interface (BCP47 tags).
    #[serde(default)]
    pub ui_locales: Option<String>,
}

/// Response from the POST logout endpoint.
#[derive(Debug, Serialize)]
pub struct LogoutResponse {
    /// Whether the logout was successful.
    pub success: bool,
    /// Human-readable message.
    pub message: String,
}

// =============================================================================
// OIDC GET Handler (RP-Initiated Logout)
// =============================================================================

/// Handler for GET /auth/logout (OIDC RP-Initiated Logout).
///
/// This endpoint implements OpenID Connect RP-Initiated Logout 1.0:
/// 1. Validates id_token_hint if present (extracts client_id from audience)
/// 2. Validates post_logout_redirect_uri against client's registered URIs
/// 3. Revokes the SSO session
/// 4. Redirects to post_logout_redirect_uri with optional state parameter
///
/// If no valid redirect URI is provided, returns a simple HTML page confirming logout.
pub async fn oidc_logout_handler(
    State(state): State<LogoutState>,
    Query(params): Query<OidcLogoutParams>,
    headers: HeaderMap,
) -> Response {
    info!("OIDC RP-Initiated Logout request received");

    // Step 1: Determine client_id from id_token_hint or explicit parameter
    let client_id = match extract_client_id(&state, &params).await {
        Ok(id) => id,
        Err(error_response) => return error_response,
    };

    // Step 2: Validate post_logout_redirect_uri if provided
    let redirect_uri = match validate_post_logout_redirect_uri(&state, &params, client_id.as_deref()).await {
        Ok(uri) => uri,
        Err(error_response) => return error_response,
    };

    // Step 3: Revoke SSO session (from cookie)
    if let Some(session_token) = extract_sso_cookie(&headers, &state.session_config) {
        match revoke_sso_session(&state, &session_token).await {
            Ok(session_id) => {
                info!(session_id = %session_id, "SSO session revoked during OIDC logout");
            }
            Err(e) => {
                debug!(error = %e, "Failed to revoke SSO session during OIDC logout");
            }
        }
    }

    // Step 4: Also try to revoke access token if present
    let token = extract_token_from_header(&headers)
        .or_else(|| extract_token_from_cookie(&headers, &state.cookie_config));

    if let Some(token) = token {
        if let Err(e) = revoke_token(&state, &token).await {
            debug!(error = %e, "Failed to revoke access token during OIDC logout");
        }
    }

    // Build response with cookies cleared
    let clear_auth_cookie = state.cookie_config.build_clear_cookie();
    let clear_sso_cookie = build_clear_sso_cookie(&state.session_config);
    let combined_cookies = format!("{}, {}", clear_auth_cookie, clear_sso_cookie);

    // Step 5: Redirect or show confirmation page
    match redirect_uri {
        Some(uri) => {
            // Build redirect URL with optional state
            let redirect_url = if let Some(state_param) = params.state {
                // URL-encode the state parameter
                let encoded_state: String = form_urlencoded::byte_serialize(state_param.as_bytes()).collect();
                if uri.contains('?') {
                    format!("{}&state={}", uri, encoded_state)
                } else {
                    format!("{}?state={}", uri, encoded_state)
                }
            } else {
                uri
            };

            info!(redirect_uri = %redirect_url, "Redirecting after OIDC logout");

            (
                StatusCode::SEE_OTHER,
                [
                    ("Location", redirect_url.as_str()),
                    ("Set-Cookie", combined_cookies.as_str()),
                    ("Cache-Control", "no-store"),
                ],
            )
                .into_response()
        }
        None => {
            // No valid redirect URI - show simple confirmation page
            let html = r#"<!DOCTYPE html>
<html>
<head>
    <title>Logged Out</title>
    <style>
        body { font-family: system-ui, sans-serif; display: flex; justify-content: center; align-items: center; height: 100vh; margin: 0; background: #f5f5f5; }
        .container { text-align: center; padding: 2rem; background: white; border-radius: 8px; box-shadow: 0 2px 4px rgba(0,0,0,0.1); }
        h1 { color: #333; margin-bottom: 0.5rem; }
        p { color: #666; }
    </style>
</head>
<body>
    <div class="container">
        <h1>Logged Out</h1>
        <p>You have been successfully logged out.</p>
    </div>
</body>
</html>"#;

            (
                StatusCode::OK,
                [
                    ("Content-Type", "text/html; charset=utf-8"),
                    ("Set-Cookie", combined_cookies.as_str()),
                    ("Cache-Control", "no-store"),
                ],
                html,
            )
                .into_response()
        }
    }
}

/// Extract client_id from id_token_hint or explicit parameter.
///
/// Per the spec, when both client_id and id_token_hint are present,
/// the OP MUST verify that the client_id matches the token's audience.
async fn extract_client_id(
    state: &LogoutState,
    params: &OidcLogoutParams,
) -> Result<Option<String>, Response> {
    // If id_token_hint is provided, decode it to get the audience (client_id)
    let token_client_id = if let Some(ref id_token) = params.id_token_hint {
        match decode_id_token_hint(state, id_token).await {
            Ok(claims) => Some(claims.aud),
            Err(e) => {
                // Per spec, invalid id_token_hint should prompt for confirmation
                // For now, we log and continue without client validation
                warn!(error = %e, "Invalid id_token_hint provided, continuing without client validation");
                None
            }
        }
    } else {
        None
    };

    // If explicit client_id is provided, verify it matches the token
    if let Some(ref explicit_client_id) = params.client_id {
        if let Some(ref token_aud) = token_client_id {
            if explicit_client_id != token_aud {
                warn!(
                    explicit_client_id = %explicit_client_id,
                    token_audience = %token_aud,
                    "client_id does not match id_token_hint audience"
                );
                return Err(logout_error_response(
                    "invalid_request",
                    "client_id does not match id_token_hint audience",
                ));
            }
        }
        return Ok(Some(explicit_client_id.clone()));
    }

    Ok(token_client_id)
}

/// Validate post_logout_redirect_uri against client's registered URIs.
///
/// Per the spec, the OP MUST NOT redirect if the URI doesn't match a registered value.
async fn validate_post_logout_redirect_uri(
    state: &LogoutState,
    params: &OidcLogoutParams,
    client_id: Option<&str>,
) -> Result<Option<String>, Response> {
    let Some(ref redirect_uri) = params.post_logout_redirect_uri else {
        // No redirect URI requested - that's fine
        return Ok(None);
    };

    let Some(client_id) = client_id else {
        // No client_id means we can't validate the redirect URI
        // Per spec, we MUST NOT redirect without validation
        warn!(
            redirect_uri = %redirect_uri,
            "post_logout_redirect_uri provided but no client_id to validate against"
        );
        // Don't error - just don't redirect
        return Ok(None);
    };

    // Look up the client
    let client = match state.client_storage.find_by_client_id(client_id).await {
        Ok(Some(client)) => client,
        Ok(None) => {
            warn!(client_id = %client_id, "Client not found for logout redirect validation");
            return Ok(None);
        }
        Err(e) => {
            warn!(error = %e, client_id = %client_id, "Failed to lookup client for logout");
            return Ok(None);
        }
    };

    // Validate the redirect URI
    if client.is_post_logout_redirect_uri_allowed(redirect_uri) {
        info!(
            client_id = %client_id,
            redirect_uri = %redirect_uri,
            "post_logout_redirect_uri validated successfully"
        );
        Ok(Some(redirect_uri.clone()))
    } else {
        warn!(
            client_id = %client_id,
            redirect_uri = %redirect_uri,
            registered_uris = ?client.post_logout_redirect_uris,
            "post_logout_redirect_uri not registered for client"
        );
        // Per spec, we MUST NOT redirect to unregistered URIs
        Ok(None)
    }
}

/// Decode id_token_hint without full validation.
///
/// We use relaxed validation since the token may be expired (user logging out
/// after being away). We only need to extract the audience (client_id).
async fn decode_id_token_hint(
    state: &LogoutState,
    id_token: &str,
) -> Result<IdTokenClaims, String> {
    // Decode with signature verification but allow expired tokens
    // The id_token_hint is just a hint about the session, so expiry doesn't matter
    state
        .jwt_service
        .decode_allow_expired_async::<IdTokenClaims>(id_token.to_string())
        .await
        .map(|data| data.claims)
        .map_err(|e| format!("Failed to decode id_token_hint: {}", e))
}

/// Build an error response for OIDC logout.
fn logout_error_response(error: &str, description: &str) -> Response {
    let html = format!(
        r#"<!DOCTYPE html>
<html>
<head>
    <title>Logout Error</title>
    <style>
        body {{ font-family: system-ui, sans-serif; display: flex; justify-content: center; align-items: center; height: 100vh; margin: 0; background: #f5f5f5; }}
        .container {{ text-align: center; padding: 2rem; background: white; border-radius: 8px; box-shadow: 0 2px 4px rgba(0,0,0,0.1); max-width: 400px; }}
        h1 {{ color: #d32f2f; margin-bottom: 0.5rem; }}
        p {{ color: #666; }}
        code {{ background: #f5f5f5; padding: 0.2rem 0.4rem; border-radius: 4px; }}
    </style>
</head>
<body>
    <div class="container">
        <h1>Logout Error</h1>
        <p><code>{}</code></p>
        <p>{}</p>
    </div>
</body>
</html>"#,
        error, description
    );

    (
        StatusCode::BAD_REQUEST,
        [
            ("Content-Type", "text/html; charset=utf-8"),
            ("Cache-Control", "no-store"),
        ],
        html,
    )
        .into_response()
}

// =============================================================================
// POST Handler (API-style logout)
// =============================================================================

/// Handler for POST /auth/logout.
///
/// This endpoint:
/// 1. Extracts the access token from Authorization header or cookie
/// 2. Revokes the token (if valid)
/// 3. Extracts the SSO session cookie and revokes the session
/// 4. Returns Set-Cookie headers to clear both auth and SSO cookies
///
/// The endpoint is lenient - it returns 200 OK even if no token was found,
/// to ensure the cookies are always cleared on the client.
pub async fn logout_handler(State(state): State<LogoutState>, headers: HeaderMap) -> Response {
    // Try to extract token from Authorization header first
    let token = extract_token_from_header(&headers)
        .or_else(|| extract_token_from_cookie(&headers, &state.cookie_config));

    // If we have a token, try to revoke it
    if let Some(token) = token {
        match revoke_token(&state, &token).await {
            Ok(jti) => {
                info!(jti = %jti, "Access token revoked during logout");
            }
            Err(e) => {
                // Log but don't fail - we still want to clear the cookie
                debug!(error = %e, "Failed to revoke access token during logout (may be expired or invalid)");
            }
        }
    } else {
        debug!("No access token found during logout - clearing cookies only");
    }

    // Try to extract and revoke SSO session
    if let Some(session_token) = extract_sso_cookie(&headers, &state.session_config) {
        match revoke_sso_session(&state, &session_token).await {
            Ok(session_id) => {
                info!(session_id = %session_id, "SSO session revoked during logout");
            }
            Err(e) => {
                // Log but don't fail - we still want to clear the cookie
                debug!(error = %e, "Failed to revoke SSO session during logout (may be expired or not found)");
            }
        }
    }

    // Build response with Set-Cookie headers to clear both cookies
    let clear_auth_cookie = state.cookie_config.build_clear_cookie();
    let clear_sso_cookie = build_clear_sso_cookie(&state.session_config);

    let response = LogoutResponse {
        success: true,
        message: "Logged out successfully".to_string(),
    };

    (
        StatusCode::OK,
        [
            ("Content-Type", "application/json"),
            ("Set-Cookie", &format!("{}, {}", clear_auth_cookie, clear_sso_cookie)),
            ("Cache-Control", "no-store"),
        ],
        Json(response),
    )
        .into_response()
}

// =============================================================================
// Helper Functions
// =============================================================================

/// Extract Bearer token from Authorization header.
fn extract_token_from_header(headers: &HeaderMap) -> Option<String> {
    headers
        .get(AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
        .filter(|t| !t.is_empty())
        .map(ToString::to_string)
}

/// Extract token from cookie.
fn extract_token_from_cookie(headers: &HeaderMap, cookie_config: &CookieConfig) -> Option<String> {
    if !cookie_config.enabled {
        return None;
    }

    let cookie_header = headers.get(COOKIE)?.to_str().ok()?;
    let cookie_name = &cookie_config.name;

    for cookie in cookie_header.split(';') {
        let cookie = cookie.trim();
        if let Some((name, value)) = cookie.split_once('=')
            && name.trim() == cookie_name
        {
            let value = value.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }

    None
}

/// Revoke the token and return its JTI.
async fn revoke_token(state: &LogoutState, token: &str) -> Result<String, String> {
    // Decode the token to get its JTI (using spawn_blocking to avoid blocking async runtime)
    let claims = state
        .jwt_service
        .decode_async::<AccessTokenClaims>(token.to_string())
        .await
        .map_err(|e| format!("Failed to decode token: {}", e))?
        .claims;

    let jti = claims.jti.clone();

    // CRITICAL: Invalidate JWT verification cache FIRST (sync, fast, <1ms)
    // This ensures the revoked token is immediately rejected for new requests
    // even before the DB write completes.
    if let Some(ref callback) = state.on_token_revoked {
        debug!(jti = %jti, "Invalidating JWT cache for revoked token");
        callback(&jti);
    }

    // Calculate expiration time as OffsetDateTime
    let expires_at = time::OffsetDateTime::from_unix_timestamp(claims.exp)
        .map_err(|e| format!("Invalid expiration timestamp: {}", e))?;

    // Revoke the token in storage (persists revocation for future restarts)
    state
        .revoked_token_storage
        .revoke(&jti, expires_at)
        .await
        .map_err(|e| format!("Failed to revoke token: {}", e))?;

    Ok(jti)
}

/// Extract SSO session token from cookie.
fn extract_sso_cookie(headers: &HeaderMap, session_config: &SessionConfig) -> Option<String> {
    let cookie_header = headers.get(COOKIE)?.to_str().ok()?;
    let cookie_name = &session_config.cookie_name;

    for cookie in cookie_header.split(';') {
        let cookie = cookie.trim();
        if let Some((name, value)) = cookie.split_once('=')
            && name.trim() == cookie_name
        {
            let value = value.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }

    None
}

/// Revoke the SSO session and return its resource ID.
async fn revoke_sso_session(state: &LogoutState, session_token: &str) -> Result<String, String> {
    // Lookup AuthSession by token
    let resource_id = state
        .sso_session_storage
        .find_session_by_token(session_token)
        .await
        .map_err(|e| format!("Failed to lookup session: {}", e))?
        .ok_or_else(|| "Session not found".to_string())?;

    // Revoke the session (updates status and removes from index)
    state
        .sso_session_storage
        .revoke_session(&resource_id)
        .await
        .map_err(|e| format!("Failed to revoke session: {}", e))?;

    Ok(resource_id)
}

/// Build a Set-Cookie header to clear the SSO cookie.
fn build_clear_sso_cookie(session_config: &SessionConfig) -> String {
    let secure = if session_config.cookie_secure {
        "; Secure"
    } else {
        ""
    };

    format!(
        "{}=; Path=/; Max-Age=0; HttpOnly; SameSite={}{}",
        session_config.cookie_name, session_config.cookie_same_site, secure
    )
}
