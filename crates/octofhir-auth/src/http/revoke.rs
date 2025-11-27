//! Token revocation endpoint handler (RFC 7009).
//!
//! This module provides the Axum handler for the `/auth/revoke` endpoint.
//!
//! # Usage
//!
//! ```ignore
//! use axum::{Router, routing::post};
//! use octofhir_auth::http::revoke_handler;
//!
//! let app = Router::new()
//!     .route("/auth/revoke", post(revoke_handler))
//!     .with_state(auth_state);
//! ```
//!
//! # Request Format
//!
//! ```text
//! POST /auth/revoke
//! Content-Type: application/x-www-form-urlencoded
//! Authorization: Basic <client_credentials>
//!
//! token=<token_to_revoke>&token_type_hint=access_token
//! ```
//!
//! # Response
//!
//! Per RFC 7009, the endpoint always returns 200 OK (even for invalid tokens)
//! unless there's a client authentication error.

use std::sync::Arc;

use axum::{
    Form,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
use serde::Deserialize;

use crate::oauth::client_auth::{authenticate_client, parse_basic_auth};
use crate::oauth::token::TokenRequest;
use crate::storage::ClientStorage;
use crate::token::revocation::{RevocationError, RevocationRequest, TokenTypeHint};
use crate::token::service::TokenService;

// =============================================================================
// State Types
// =============================================================================

/// State required for the revocation endpoint.
///
/// This struct should be provided via Axum's `State` extractor.
#[derive(Clone)]
pub struct RevocationState {
    /// Token service for performing revocation.
    pub token_service: Arc<TokenService>,
    /// Client storage for authentication.
    pub client_storage: Arc<dyn ClientStorage>,
}

impl RevocationState {
    /// Creates a new revocation state.
    pub fn new(token_service: Arc<TokenService>, client_storage: Arc<dyn ClientStorage>) -> Self {
        Self {
            token_service,
            client_storage,
        }
    }
}

// =============================================================================
// Request Types
// =============================================================================

/// Form parameters for the revocation endpoint.
///
/// These match the RFC 7009 specification.
#[derive(Debug, Deserialize)]
pub struct RevocationForm {
    /// The token to revoke.
    pub token: String,

    /// Optional hint about the token type.
    #[serde(default)]
    pub token_type_hint: Option<String>,

    /// Client ID (for public clients or when not using Basic auth).
    #[serde(default)]
    pub client_id: Option<String>,

    /// Client secret (for client_secret_post authentication).
    #[serde(default)]
    pub client_secret: Option<String>,
}

impl RevocationForm {
    /// Converts to a RevocationRequest.
    fn to_revocation_request(&self) -> RevocationRequest {
        RevocationRequest {
            token: self.token.clone(),
            token_type_hint: self
                .token_type_hint
                .as_deref()
                .and_then(parse_token_type_hint),
        }
    }

    /// Converts to a TokenRequest for client authentication.
    fn to_token_request(&self) -> TokenRequest {
        TokenRequest {
            grant_type: String::new(), // Not used for authentication
            code: None,
            redirect_uri: None,
            code_verifier: None,
            client_id: self.client_id.clone(),
            client_secret: self.client_secret.clone(),
            client_assertion_type: None,
            client_assertion: None,
            refresh_token: None,
            scope: None,
        }
    }
}

/// Parses a token type hint string.
fn parse_token_type_hint(hint: &str) -> Option<TokenTypeHint> {
    match hint {
        "access_token" => Some(TokenTypeHint::AccessToken),
        "refresh_token" => Some(TokenTypeHint::RefreshToken),
        _ => None,
    }
}

// =============================================================================
// Handler
// =============================================================================

/// Token revocation endpoint handler.
///
/// Implements RFC 7009 token revocation.
///
/// # Security
///
/// - Requires client authentication (same as token endpoint)
/// - Always returns 200 OK for valid client auth (even if token is invalid)
/// - Returns 401 Unauthorized for invalid client credentials
///
/// # Request
///
/// - Method: POST
/// - Content-Type: application/x-www-form-urlencoded
/// - Body: `token=<token>&token_type_hint=<hint>`
///
/// # Response
///
/// - 200 OK: Token revoked (or was already invalid/revoked)
/// - 400 Bad Request: Missing token parameter
/// - 401 Unauthorized: Invalid client credentials
pub async fn revoke_handler(
    State(state): State<RevocationState>,
    headers: HeaderMap,
    Form(form): Form<RevocationForm>,
) -> impl IntoResponse {
    // Validate required token parameter
    if form.token.is_empty() {
        let error = RevocationError::invalid_request("Missing required 'token' parameter");
        return (
            StatusCode::BAD_REQUEST,
            axum::Json(serde_json::json!({
                "error": error.error.as_str(),
                "error_description": error.error_description,
            })),
        )
            .into_response();
    }

    // Extract Basic auth credentials if present
    let basic_auth = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(parse_basic_auth);

    let basic_auth_ref = basic_auth
        .as_ref()
        .map(|(id, secret)| (id.as_str(), secret.as_str()));

    // Authenticate client (same as token endpoint)
    let token_request = form.to_token_request();
    let auth_result = authenticate_client(
        &token_request,
        basic_auth_ref,
        state.client_storage.as_ref(),
    )
    .await;

    let client = match auth_result {
        Ok(authenticated) => authenticated.client,
        Err(e) => {
            tracing::debug!(error = %e, "Revocation: client authentication failed");
            let error = RevocationError::invalid_client(e.to_string());
            return (
                StatusCode::UNAUTHORIZED,
                axum::Json(serde_json::json!({
                    "error": error.error.as_str(),
                    "error_description": error.error_description,
                })),
            )
                .into_response();
        }
    };

    // Perform revocation
    let revocation_request = form.to_revocation_request();
    match state
        .token_service
        .revoke(&revocation_request, &client)
        .await
    {
        Ok(()) => {
            tracing::info!(
                client_id = %client.client_id,
                "Token revocation successful"
            );
            // RFC 7009: Return 200 OK with empty body
            StatusCode::OK.into_response()
        }
        Err(e) => {
            // Per RFC 7009, we should still return 200 OK for most errors
            // to avoid revealing information about token existence.
            // Only return error for unexpected server errors.
            tracing::warn!(
                client_id = %client.client_id,
                error = %e,
                "Token revocation encountered error (returning 200 OK per RFC 7009)"
            );
            StatusCode::OK.into_response()
        }
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_token_type_hint() {
        assert_eq!(
            parse_token_type_hint("access_token"),
            Some(TokenTypeHint::AccessToken)
        );
        assert_eq!(
            parse_token_type_hint("refresh_token"),
            Some(TokenTypeHint::RefreshToken)
        );
        assert_eq!(parse_token_type_hint("unknown"), None);
        assert_eq!(parse_token_type_hint(""), None);
    }

    #[test]
    fn test_revocation_form_to_request() {
        let form = RevocationForm {
            token: "test-token".to_string(),
            token_type_hint: Some("access_token".to_string()),
            client_id: Some("client123".to_string()),
            client_secret: None,
        };

        let request = form.to_revocation_request();
        assert_eq!(request.token, "test-token");
        assert_eq!(request.token_type_hint, Some(TokenTypeHint::AccessToken));
    }

    #[test]
    fn test_revocation_form_to_token_request() {
        let form = RevocationForm {
            token: "test-token".to_string(),
            token_type_hint: None,
            client_id: Some("client123".to_string()),
            client_secret: Some("secret".to_string()),
        };

        let token_request = form.to_token_request();
        assert_eq!(token_request.client_id, Some("client123".to_string()));
        assert_eq!(token_request.client_secret, Some("secret".to_string()));
    }
}
