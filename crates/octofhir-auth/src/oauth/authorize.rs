//! Authorization endpoint types and handlers.
//!
//! This module provides types for the OAuth 2.0 authorization endpoint,
//! including request parsing, response generation, and error handling.
//!
//! # OAuth 2.0 Authorization Code Flow
//!
//! The authorization endpoint is the first step in the authorization code flow:
//!
//! 1. Client redirects user to authorization endpoint with request parameters
//! 2. User authenticates and authorizes the request
//! 3. Server redirects back to client with authorization code
//! 4. Client exchanges code for tokens at token endpoint
//!
//! # SMART on FHIR Extensions
//!
//! This implementation supports SMART on FHIR parameters:
//! - `aud`: FHIR server base URL (required for SMART)
//! - `launch`: EHR launch parameter for launch context
//!
//! # Security Requirements
//!
//! - PKCE is required (code_challenge and code_challenge_method)
//! - Only S256 challenge method is supported (plain is forbidden)
//! - State parameter must have at least 122 bits of entropy

use serde::{Deserialize, Serialize};
use std::fmt;

/// Authorization request parameters.
///
/// These parameters are received as query string parameters on the
/// authorization endpoint. All required parameters must be present
/// for a valid authorization request.
///
/// # Example
///
/// ```ignore
/// GET /authorize?
///   response_type=code
///   &client_id=my-app
///   &redirect_uri=https://app.example.com/callback
///   &scope=openid fhirUser patient/*.read
///   &state=abc123xyz
///   &code_challenge=E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM
///   &code_challenge_method=S256
///   &aud=https://fhir.example.com/r4
/// ```
#[derive(Debug, Clone, Deserialize)]
pub struct AuthorizationRequest {
    /// Must be "code" for authorization code flow.
    pub response_type: String,

    /// Client identifier issued during registration.
    pub client_id: String,

    /// Redirect URI where the response will be sent.
    /// Must exactly match one of the registered redirect URIs.
    pub redirect_uri: String,

    /// Requested scopes (space-separated).
    /// For SMART on FHIR, includes scopes like `patient/*.read`, `openid`, etc.
    pub scope: String,

    /// CSRF protection state parameter.
    /// Must have at least 122 bits of entropy (approximately 21 characters
    /// of base64-encoded random data).
    pub state: String,

    /// PKCE code challenge.
    /// Base64url-encoded SHA-256 hash of the code verifier.
    pub code_challenge: String,

    /// PKCE code challenge method.
    /// Must be "S256" (plain is not supported per SMART on FHIR requirements).
    pub code_challenge_method: String,

    /// FHIR server base URL (audience).
    /// Required for SMART on FHIR to identify which FHIR server the
    /// authorization is for.
    pub aud: String,

    /// EHR launch parameter (optional).
    /// Present when launching from an EHR context, contains an opaque
    /// launch context identifier.
    #[serde(default)]
    pub launch: Option<String>,

    /// OpenID Connect nonce (optional).
    /// Used to associate a client session with an ID token for replay protection.
    #[serde(default)]
    pub nonce: Option<String>,
}

/// Authorization response parameters.
///
/// These parameters are returned as query string parameters on the
/// redirect URI after successful authorization.
///
/// # Example
///
/// ```ignore
/// HTTP/1.1 302 Found
/// Location: https://app.example.com/callback?
///   code=SplxlOBeZQQYbYS6WxSbIA
///   &state=abc123xyz
/// ```
#[derive(Debug, Clone, Serialize)]
pub struct AuthorizationResponse {
    /// Authorization code to be exchanged for tokens.
    /// This code is single-use and expires after a short time (typically 10 minutes).
    pub code: String,

    /// Echoed state parameter for CSRF validation.
    /// The client must verify this matches the state sent in the request.
    pub state: String,
}

impl AuthorizationResponse {
    /// Creates a new authorization response.
    #[must_use]
    pub fn new(code: String, state: String) -> Self {
        Self { code, state }
    }

    /// Builds the redirect URL with response parameters.
    ///
    /// # Arguments
    ///
    /// * `redirect_uri` - The base redirect URI
    ///
    /// # Returns
    ///
    /// The complete redirect URL with query parameters, or an error if
    /// the redirect URI is invalid.
    pub fn to_redirect_url(&self, redirect_uri: &str) -> Result<String, url::ParseError> {
        let mut url = url::Url::parse(redirect_uri)?;
        url.query_pairs_mut()
            .append_pair("code", &self.code)
            .append_pair("state", &self.state);
        Ok(url.to_string())
    }
}

/// Authorization error response.
///
/// Returned when the authorization request fails. The error is communicated
/// via redirect to the client's redirect URI (if valid) or displayed to
/// the user (if redirect URI is invalid).
///
/// # Example
///
/// ```ignore
/// HTTP/1.1 302 Found
/// Location: https://app.example.com/callback?
///   error=invalid_request
///   &error_description=Missing+required+parameter:+scope
///   &state=abc123xyz
/// ```
#[derive(Debug, Clone, Serialize)]
pub struct AuthorizationError {
    /// OAuth 2.0 error code.
    pub error: AuthorizationErrorCode,

    /// Human-readable error description (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_description: Option<String>,

    /// Echoed state parameter for CSRF validation.
    pub state: String,
}

impl AuthorizationError {
    /// Creates a new authorization error.
    #[must_use]
    pub fn new(error: AuthorizationErrorCode, state: String) -> Self {
        Self {
            error,
            error_description: None,
            state,
        }
    }

    /// Creates a new authorization error with description.
    #[must_use]
    pub fn with_description(
        error: AuthorizationErrorCode,
        description: impl Into<String>,
        state: String,
    ) -> Self {
        Self {
            error,
            error_description: Some(description.into()),
            state,
        }
    }

    /// Builds the redirect URL with error parameters.
    ///
    /// # Arguments
    ///
    /// * `redirect_uri` - The base redirect URI
    ///
    /// # Returns
    ///
    /// The complete redirect URL with error query parameters, or an error
    /// if the redirect URI is invalid.
    pub fn to_redirect_url(&self, redirect_uri: &str) -> Result<String, url::ParseError> {
        let mut url = url::Url::parse(redirect_uri)?;
        {
            let mut pairs = url.query_pairs_mut();
            pairs.append_pair("error", self.error.as_str());
            if let Some(ref desc) = self.error_description {
                pairs.append_pair("error_description", desc);
            }
            pairs.append_pair("state", &self.state);
        }
        Ok(url.to_string())
    }
}

/// OAuth 2.0 authorization error codes.
///
/// These error codes are defined in RFC 6749 Section 4.1.2.1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthorizationErrorCode {
    /// The request is missing a required parameter, includes an invalid
    /// parameter value, includes a parameter more than once, or is
    /// otherwise malformed.
    InvalidRequest,

    /// The client is not authorized to request an authorization code
    /// using this method.
    UnauthorizedClient,

    /// The resource owner or authorization server denied the request.
    AccessDenied,

    /// The authorization server does not support obtaining an authorization
    /// code using this method.
    UnsupportedResponseType,

    /// The requested scope is invalid, unknown, or malformed.
    InvalidScope,

    /// The authorization server encountered an unexpected condition that
    /// prevented it from fulfilling the request.
    ServerError,

    /// The authorization server is currently unable to handle the request
    /// due to temporary overloading or maintenance.
    TemporarilyUnavailable,
}

impl AuthorizationErrorCode {
    /// Returns the string representation of the error code.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::InvalidRequest => "invalid_request",
            Self::UnauthorizedClient => "unauthorized_client",
            Self::AccessDenied => "access_denied",
            Self::UnsupportedResponseType => "unsupported_response_type",
            Self::InvalidScope => "invalid_scope",
            Self::ServerError => "server_error",
            Self::TemporarilyUnavailable => "temporarily_unavailable",
        }
    }
}

impl fmt::Display for AuthorizationErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_authorization_request_deserialize() {
        let json = r#"{
            "response_type": "code",
            "client_id": "my-app",
            "redirect_uri": "https://app.example.com/callback",
            "scope": "openid patient/*.read",
            "state": "abc123xyz",
            "code_challenge": "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM",
            "code_challenge_method": "S256",
            "aud": "https://fhir.example.com/r4"
        }"#;

        let request: AuthorizationRequest = serde_json::from_str(json).unwrap();
        assert_eq!(request.response_type, "code");
        assert_eq!(request.client_id, "my-app");
        assert_eq!(request.redirect_uri, "https://app.example.com/callback");
        assert_eq!(request.scope, "openid patient/*.read");
        assert_eq!(request.state, "abc123xyz");
        assert_eq!(
            request.code_challenge,
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        assert_eq!(request.code_challenge_method, "S256");
        assert_eq!(request.aud, "https://fhir.example.com/r4");
        assert!(request.launch.is_none());
        assert!(request.nonce.is_none());
    }

    #[test]
    fn test_authorization_request_with_optional_fields() {
        let json = r#"{
            "response_type": "code",
            "client_id": "my-app",
            "redirect_uri": "https://app.example.com/callback",
            "scope": "openid launch",
            "state": "xyz789",
            "code_challenge": "challenge123",
            "code_challenge_method": "S256",
            "aud": "https://fhir.example.com/r4",
            "launch": "launch-context-123",
            "nonce": "nonce-456"
        }"#;

        let request: AuthorizationRequest = serde_json::from_str(json).unwrap();
        assert_eq!(request.launch, Some("launch-context-123".to_string()));
        assert_eq!(request.nonce, Some("nonce-456".to_string()));
    }

    #[test]
    fn test_authorization_response_serialize() {
        let response = AuthorizationResponse::new(
            "SplxlOBeZQQYbYS6WxSbIA".to_string(),
            "abc123xyz".to_string(),
        );

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains(r#""code":"SplxlOBeZQQYbYS6WxSbIA""#));
        assert!(json.contains(r#""state":"abc123xyz""#));
    }

    #[test]
    fn test_authorization_response_to_redirect_url() {
        let response = AuthorizationResponse::new("code123".to_string(), "state456".to_string());

        let url = response
            .to_redirect_url("https://app.example.com/callback")
            .unwrap();

        assert!(url.starts_with("https://app.example.com/callback?"));
        assert!(url.contains("code=code123"));
        assert!(url.contains("state=state456"));
    }

    #[test]
    fn test_authorization_error_serialize() {
        let error = AuthorizationError::with_description(
            AuthorizationErrorCode::InvalidRequest,
            "Missing required parameter: scope",
            "abc123".to_string(),
        );

        let json = serde_json::to_string(&error).unwrap();
        assert!(json.contains(r#""error":"invalid_request""#));
        assert!(json.contains(r#""error_description":"Missing required parameter: scope""#));
        assert!(json.contains(r#""state":"abc123""#));
    }

    #[test]
    fn test_authorization_error_without_description() {
        let error =
            AuthorizationError::new(AuthorizationErrorCode::AccessDenied, "xyz".to_string());

        let json = serde_json::to_string(&error).unwrap();
        assert!(json.contains(r#""error":"access_denied""#));
        assert!(!json.contains("error_description"));
    }

    #[test]
    fn test_authorization_error_to_redirect_url() {
        let error = AuthorizationError::with_description(
            AuthorizationErrorCode::InvalidScope,
            "Unknown scope",
            "state123".to_string(),
        );

        let url = error
            .to_redirect_url("https://app.example.com/callback")
            .unwrap();

        assert!(url.starts_with("https://app.example.com/callback?"));
        assert!(url.contains("error=invalid_scope"));
        assert!(url.contains("error_description=Unknown+scope"));
        assert!(url.contains("state=state123"));
    }

    #[test]
    fn test_error_code_as_str() {
        assert_eq!(
            AuthorizationErrorCode::InvalidRequest.as_str(),
            "invalid_request"
        );
        assert_eq!(
            AuthorizationErrorCode::UnauthorizedClient.as_str(),
            "unauthorized_client"
        );
        assert_eq!(
            AuthorizationErrorCode::AccessDenied.as_str(),
            "access_denied"
        );
        assert_eq!(
            AuthorizationErrorCode::UnsupportedResponseType.as_str(),
            "unsupported_response_type"
        );
        assert_eq!(
            AuthorizationErrorCode::InvalidScope.as_str(),
            "invalid_scope"
        );
        assert_eq!(AuthorizationErrorCode::ServerError.as_str(), "server_error");
        assert_eq!(
            AuthorizationErrorCode::TemporarilyUnavailable.as_str(),
            "temporarily_unavailable"
        );
    }

    #[test]
    fn test_error_code_display() {
        assert_eq!(
            AuthorizationErrorCode::InvalidRequest.to_string(),
            "invalid_request"
        );
        assert_eq!(
            AuthorizationErrorCode::ServerError.to_string(),
            "server_error"
        );
    }

    #[test]
    fn test_error_code_serde_roundtrip() {
        let codes = vec![
            AuthorizationErrorCode::InvalidRequest,
            AuthorizationErrorCode::UnauthorizedClient,
            AuthorizationErrorCode::AccessDenied,
            AuthorizationErrorCode::UnsupportedResponseType,
            AuthorizationErrorCode::InvalidScope,
            AuthorizationErrorCode::ServerError,
            AuthorizationErrorCode::TemporarilyUnavailable,
        ];

        for code in codes {
            let json = serde_json::to_string(&code).unwrap();
            let deserialized: AuthorizationErrorCode = serde_json::from_str(&json).unwrap();
            assert_eq!(code, deserialized);
        }
    }
}
