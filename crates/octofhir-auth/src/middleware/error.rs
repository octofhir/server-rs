//! Error response handling for authentication middleware.
//!
//! This module implements `IntoResponse` for `AuthError` to provide
//! FHIR-compliant error responses (OperationOutcome format).

use axum::{
    Json,
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde_json::json;

use crate::error::AuthError;

// =============================================================================
// IntoResponse Implementation
// =============================================================================

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        let (status, oauth_error, issue_code, message) = error_details(&self);

        // Build FHIR OperationOutcome response
        let body = json!({
            "resourceType": "OperationOutcome",
            "issue": [{
                "severity": "error",
                "code": issue_code,
                "details": {
                    "coding": [{
                        "system": "https://tools.ietf.org/html/rfc6749",
                        "code": oauth_error
                    }]
                },
                "diagnostics": message
            }]
        });

        // Build headers
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/fhir+json"),
        );

        // Add WWW-Authenticate header for 401 responses
        if status == StatusCode::UNAUTHORIZED {
            let www_auth = build_www_authenticate_header(oauth_error, &message);
            if let Ok(value) = HeaderValue::from_str(&www_auth) {
                headers.insert(header::WWW_AUTHENTICATE, value);
            }
        }

        (status, headers, Json(body)).into_response()
    }
}

/// Extracts error details from an AuthError.
///
/// Returns (HTTP status, OAuth error code, FHIR issue code, message).
fn error_details(error: &AuthError) -> (StatusCode, &'static str, &'static str, String) {
    match error {
        AuthError::InvalidClient { message } => (
            StatusCode::UNAUTHORIZED,
            "invalid_client",
            "security",
            message.clone(),
        ),
        AuthError::InvalidGrant { message } => (
            StatusCode::BAD_REQUEST,
            "invalid_grant",
            "invalid",
            message.clone(),
        ),
        AuthError::InvalidScope { message } => (
            StatusCode::FORBIDDEN,
            "insufficient_scope",
            "forbidden",
            message.clone(),
        ),
        AuthError::InvalidToken { message } => (
            StatusCode::UNAUTHORIZED,
            "invalid_token",
            "security",
            message.clone(),
        ),
        AuthError::Unauthorized { message } => (
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "security",
            message.clone(),
        ),
        AuthError::Forbidden { message } => (
            StatusCode::FORBIDDEN,
            "access_denied",
            "forbidden",
            message.clone(),
        ),
        AuthError::TokenExpired => (
            StatusCode::UNAUTHORIZED,
            "invalid_token",
            "expired",
            "Token has expired".to_string(),
        ),
        AuthError::TokenRevoked => (
            StatusCode::UNAUTHORIZED,
            "invalid_token",
            "security",
            "Token has been revoked".to_string(),
        ),
        AuthError::PkceVerificationFailed => (
            StatusCode::BAD_REQUEST,
            "invalid_grant",
            "invalid",
            "PKCE verification failed".to_string(),
        ),
        AuthError::InvalidRequest { message } => (
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "invalid",
            message.clone(),
        ),
        AuthError::AccessDenied { message } => (
            StatusCode::FORBIDDEN,
            "access_denied",
            "forbidden",
            message.clone(),
        ),
        AuthError::UnsupportedResponseType { response_type } => (
            StatusCode::BAD_REQUEST,
            "unsupported_response_type",
            "not-supported",
            format!("Unsupported response type: {}", response_type),
        ),
        AuthError::UnsupportedGrantType { grant_type } => (
            StatusCode::BAD_REQUEST,
            "unsupported_grant_type",
            "not-supported",
            format!("Unsupported grant type: {}", grant_type),
        ),
        AuthError::Storage { message } => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "server_error",
            "exception",
            message.clone(),
        ),
        AuthError::Configuration { message } => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "server_error",
            "exception",
            message.clone(),
        ),
        AuthError::Internal { message } => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "server_error",
            "exception",
            message.clone(),
        ),
        AuthError::IdentityProvider { provider, message } => (
            StatusCode::BAD_GATEWAY,
            "server_error",
            "exception",
            format!("Identity provider '{}' error: {}", provider, message),
        ),
        AuthError::Policy { message } => (
            StatusCode::FORBIDDEN,
            "access_denied",
            "forbidden",
            message.clone(),
        ),
    }
}

/// Builds the WWW-Authenticate header value for 401 responses.
///
/// Format: `Bearer realm="octofhir", error="invalid_token", error_description="..."`
fn build_www_authenticate_header(error: &str, description: &str) -> String {
    // Escape quotes in description
    let escaped_desc = description.replace('\"', "\\\"");
    format!(
        "Bearer realm=\"octofhir\", error=\"{}\", error_description=\"{}\"",
        error, escaped_desc
    )
}

// =============================================================================
// Helper Functions
// =============================================================================

/// Creates a FHIR OperationOutcome JSON for an error.
///
/// This can be used when you need the JSON body without the full response.
#[must_use]
pub fn operation_outcome_json(
    severity: &str,
    code: &str,
    oauth_error: &str,
    diagnostics: &str,
) -> serde_json::Value {
    json!({
        "resourceType": "OperationOutcome",
        "issue": [{
            "severity": severity,
            "code": code,
            "details": {
                "coding": [{
                    "system": "https://tools.ietf.org/html/rfc6749",
                    "code": oauth_error
                }]
            },
            "diagnostics": diagnostics
        }]
    })
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;

    #[tokio::test]
    async fn test_unauthorized_response() {
        let error = AuthError::unauthorized("Missing Authorization header");
        let response = error.into_response();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        // Check headers
        let headers = response.headers();
        assert_eq!(
            headers.get(header::CONTENT_TYPE).unwrap(),
            "application/fhir+json"
        );
        assert!(headers.contains_key(header::WWW_AUTHENTICATE));

        let www_auth = headers
            .get(header::WWW_AUTHENTICATE)
            .unwrap()
            .to_str()
            .unwrap();
        assert!(www_auth.contains("Bearer"));
        assert!(www_auth.contains("realm=\"octofhir\""));
        assert!(www_auth.contains("error=\"unauthorized\""));
    }

    #[tokio::test]
    async fn test_forbidden_response() {
        let error = AuthError::forbidden("Insufficient permissions");
        let response = error.into_response();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        // Check headers - no WWW-Authenticate for 403
        let headers = response.headers();
        assert_eq!(
            headers.get(header::CONTENT_TYPE).unwrap(),
            "application/fhir+json"
        );
        assert!(!headers.contains_key(header::WWW_AUTHENTICATE));
    }

    #[tokio::test]
    async fn test_token_expired_response() {
        let error = AuthError::TokenExpired;
        let response = error.into_response();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        let headers = response.headers();
        let www_auth = headers
            .get(header::WWW_AUTHENTICATE)
            .unwrap()
            .to_str()
            .unwrap();
        assert!(www_auth.contains("error=\"invalid_token\""));
    }

    #[tokio::test]
    async fn test_token_revoked_response() {
        let error = AuthError::TokenRevoked;
        let response = error.into_response();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_invalid_scope_response() {
        let error = AuthError::invalid_scope("Missing required scope: patient/Patient.read");
        let response = error.into_response();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_server_error_response() {
        let error = AuthError::internal("Database connection failed");
        let response = error.into_response();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn test_response_body_is_operation_outcome() {
        let error = AuthError::invalid_token("Malformed JWT");
        let response = error.into_response();

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["resourceType"], "OperationOutcome");
        assert!(json["issue"].is_array());
        assert_eq!(json["issue"][0]["severity"], "error");
        assert_eq!(json["issue"][0]["code"], "security");
        assert_eq!(
            json["issue"][0]["details"]["coding"][0]["code"],
            "invalid_token"
        );
        assert_eq!(json["issue"][0]["diagnostics"], "Malformed JWT");
    }

    #[test]
    fn test_www_authenticate_header_escaping() {
        let header = build_www_authenticate_header("invalid_token", "Token contains \"quotes\"");
        assert!(header.contains("\\\"quotes\\\""));
    }

    #[test]
    fn test_operation_outcome_json() {
        let json = operation_outcome_json("error", "security", "invalid_token", "Test error");

        assert_eq!(json["resourceType"], "OperationOutcome");
        assert_eq!(json["issue"][0]["severity"], "error");
        assert_eq!(json["issue"][0]["code"], "security");
        assert_eq!(json["issue"][0]["diagnostics"], "Test error");
    }
}
