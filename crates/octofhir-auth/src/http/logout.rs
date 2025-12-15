//! Logout endpoint handler for browser-based authentication.
//!
//! This module provides the Axum handler for the `/auth/logout` endpoint.
//! It revokes the current access token and clears the authentication cookie.
//!
//! # Usage
//!
//! ```ignore
//! use axum::{Router, routing::post};
//! use octofhir_auth::http::logout_handler;
//!
//! let app = Router::new()
//!     .route("/auth/logout", post(logout_handler))
//!     .with_state(logout_state);
//! ```
//!
//! # Request Format
//!
//! The endpoint accepts POST requests with either:
//! - `Authorization: Bearer <token>` header
//! - HttpOnly cookie (as configured)
//!
//! # Response
//!
//! Returns 200 OK with a Set-Cookie header to clear the auth cookie.
//! The response includes a JSON body with logout status.

use std::sync::Arc;

use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode, header::AUTHORIZATION, header::COOKIE},
    response::{IntoResponse, Response},
};
use serde::Serialize;
use tracing::{debug, info};

use crate::config::CookieConfig;
use crate::storage::RevokedTokenStorage;
use crate::token::jwt::{AccessTokenClaims, JwtService};

// =============================================================================
// State Types
// =============================================================================

/// State required for the logout endpoint.
#[derive(Clone)]
pub struct LogoutState {
    /// JWT service for token validation.
    pub jwt_service: Arc<JwtService>,
    /// Revoked token storage for marking tokens as revoked.
    pub revoked_token_storage: Arc<dyn RevokedTokenStorage>,
    /// Cookie configuration for clearing the auth cookie.
    pub cookie_config: CookieConfig,
}

impl LogoutState {
    /// Creates a new logout state.
    pub fn new(
        jwt_service: Arc<JwtService>,
        revoked_token_storage: Arc<dyn RevokedTokenStorage>,
        cookie_config: CookieConfig,
    ) -> Self {
        Self {
            jwt_service,
            revoked_token_storage,
            cookie_config,
        }
    }
}

// =============================================================================
// Response Types
// =============================================================================

/// Response from the logout endpoint.
#[derive(Debug, Serialize)]
pub struct LogoutResponse {
    /// Whether the logout was successful.
    pub success: bool,
    /// Human-readable message.
    pub message: String,
}

// =============================================================================
// Handler
// =============================================================================

/// Handler for POST /auth/logout.
///
/// This endpoint:
/// 1. Extracts the access token from Authorization header or cookie
/// 2. Revokes the token (if valid)
/// 3. Returns a Set-Cookie header to clear the auth cookie
///
/// The endpoint is lenient - it returns 200 OK even if no token was found,
/// to ensure the cookie is always cleared on the client.
pub async fn logout_handler(State(state): State<LogoutState>, headers: HeaderMap) -> Response {
    // Try to extract token from Authorization header first
    let token = extract_token_from_header(&headers)
        .or_else(|| extract_token_from_cookie(&headers, &state.cookie_config));

    // If we have a token, try to revoke it
    if let Some(token) = token {
        match revoke_token(&state, &token).await {
            Ok(jti) => {
                info!(jti = %jti, "Token revoked during logout");
            }
            Err(e) => {
                // Log but don't fail - we still want to clear the cookie
                debug!(error = %e, "Failed to revoke token during logout (may be expired or invalid)");
            }
        }
    } else {
        debug!("No token found during logout - clearing cookie only");
    }

    // Build response with Set-Cookie to clear the auth cookie
    let clear_cookie = state.cookie_config.build_clear_cookie();

    let response = LogoutResponse {
        success: true,
        message: "Logged out successfully".to_string(),
    };

    (
        StatusCode::OK,
        [
            ("Content-Type", "application/json"),
            ("Set-Cookie", clear_cookie.as_str()),
            ("Cache-Control", "no-store"),
        ],
        Json(response),
    )
        .into_response()
}

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
    // Decode the token to get its JTI
    let claims = state
        .jwt_service
        .decode::<AccessTokenClaims>(token)
        .map_err(|e| format!("Failed to decode token: {}", e))?
        .claims;

    // Calculate expiration time as OffsetDateTime
    let expires_at = time::OffsetDateTime::from_unix_timestamp(claims.exp)
        .map_err(|e| format!("Invalid expiration timestamp: {}", e))?;

    // Revoke the token
    state
        .revoked_token_storage
        .revoke(&claims.jti, expires_at)
        .await
        .map_err(|e| format!("Failed to revoke token: {}", e))?;

    Ok(claims.jti)
}
