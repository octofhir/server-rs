//! Token introspection endpoint handler (RFC 7662).
//!
//! This module provides the Axum handler for the `/auth/introspect` endpoint.
//!
//! # Usage
//!
//! ```ignore
//! use axum::{Router, routing::post};
//! use octofhir_auth::http::introspect_handler;
//!
//! let app = Router::new()
//!     .route("/auth/introspect", post(introspect_handler))
//!     .with_state(auth_state);
//! ```
//!
//! # Request Format
//!
//! ```text
//! POST /auth/introspect
//! Content-Type: application/x-www-form-urlencoded
//! Authorization: Basic <client_credentials>
//!
//! token=<token_to_introspect>&token_type_hint=access_token
//! ```
//!
//! # Response
//!
//! Returns JSON with `active: true/false` and metadata if active.
//!
//! # Security
//!
//! - Client authentication is required
//! - Never reveals why a token is inactive
//! - Always returns valid JSON

use std::sync::Arc;

use axum::{
    Form, Json,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
use serde::Deserialize;

use crate::oauth::client_auth::{authenticate_client, parse_basic_auth};
use crate::oauth::token::TokenRequest;
use crate::storage::ClientStorage;
use crate::token::introspection::{IntrospectionError, IntrospectionRequest};
use crate::token::revocation::TokenTypeHint;
use crate::token::service::TokenService;

// =============================================================================
// State Types
// =============================================================================

/// State required for the introspection endpoint.
///
/// This struct should be provided via Axum's `State` extractor.
#[derive(Clone)]
pub struct IntrospectionState {
    /// Token service for performing introspection.
    pub token_service: Arc<TokenService>,
    /// Client storage for authentication.
    pub client_storage: Arc<dyn ClientStorage>,
}

impl IntrospectionState {
    /// Creates a new introspection state.
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

/// Form parameters for the introspection endpoint.
///
/// These match the RFC 7662 specification.
#[derive(Debug, Deserialize)]
pub struct IntrospectionForm {
    /// The token to introspect.
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

impl IntrospectionForm {
    /// Converts to an IntrospectionRequest.
    fn to_introspection_request(&self) -> IntrospectionRequest {
        IntrospectionRequest {
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

/// Token introspection endpoint handler.
///
/// Implements RFC 7662 token introspection.
///
/// # Security
///
/// - Requires client authentication (same as token endpoint)
/// - Returns `{"active": false}` for invalid/expired/revoked tokens
/// - Returns 401 Unauthorized for invalid client credentials
/// - Returns 400 Bad Request for missing token parameter
///
/// # Request
///
/// - Method: POST
/// - Content-Type: application/x-www-form-urlencoded
/// - Body: `token=<token>&token_type_hint=<hint>`
///
/// # Response
///
/// - 200 OK with JSON body: `{"active": true/false, ...metadata}`
/// - 400 Bad Request: Missing token parameter
/// - 401 Unauthorized: Invalid client credentials
pub async fn introspect_handler(
    State(state): State<IntrospectionState>,
    headers: HeaderMap,
    Form(form): Form<IntrospectionForm>,
) -> impl IntoResponse {
    // Validate required token parameter
    if form.token.is_empty() {
        let error = IntrospectionError::invalid_request("Missing required 'token' parameter");
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
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

    match auth_result {
        Ok(_authenticated) => {
            // Perform introspection
            let introspection_request = form.to_introspection_request();
            let response = state.token_service.introspect(&introspection_request).await;

            tracing::debug!(active = response.active, "Token introspection completed");

            (StatusCode::OK, Json(response)).into_response()
        }
        Err(e) => {
            tracing::debug!(error = %e, "Introspection: client authentication failed");
            let error = IntrospectionError::invalid_client(e.to_string());
            (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({
                    "error": error.error.as_str(),
                    "error_description": error.error_description,
                })),
            )
                .into_response()
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
    fn test_introspection_form_to_request() {
        let form = IntrospectionForm {
            token: "test-token".to_string(),
            token_type_hint: Some("access_token".to_string()),
            client_id: Some("client123".to_string()),
            client_secret: None,
        };

        let request = form.to_introspection_request();
        assert_eq!(request.token, "test-token");
        assert_eq!(request.token_type_hint, Some(TokenTypeHint::AccessToken));
    }

    #[test]
    fn test_introspection_form_to_token_request() {
        let form = IntrospectionForm {
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
