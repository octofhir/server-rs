//! Token endpoint types and handlers.
//!
//! This module provides types for the OAuth 2.0 token endpoint,
//! including request parsing, response generation, and error handling.
//!
//! # Supported Grant Types
//!
//! - `authorization_code` - Exchange authorization code for tokens
//! - `refresh_token` - Refresh an access token
//! - `client_credentials` - Machine-to-machine authentication
//!
//! # SMART on FHIR Extensions
//!
//! Token responses include SMART on FHIR context fields:
//! - `patient` - Current patient context
//! - `encounter` - Current encounter context
//! - `fhirContext` - Additional FHIR context (SMART v2)
//! - `need_patient_banner` - Whether to show patient banner
//! - `smart_style_url` - URL to SMART styling info

use serde::{Deserialize, Serialize};
use std::fmt;

use crate::oauth::session::FhirContextItem;

/// Token request parameters.
///
/// This structure handles all OAuth 2.0 grant types. Different fields
/// are required depending on the `grant_type`:
///
/// - `authorization_code`: code, redirect_uri, code_verifier, client_id
/// - `refresh_token`: refresh_token, (optional) scope
/// - `client_credentials`: (optional) scope
///
/// # Client Authentication
///
/// Clients authenticate using one of:
/// - HTTP Basic Auth header (not in this struct)
/// - `client_id` + `client_secret` in body
/// - `client_assertion_type` + `client_assertion` (JWT)
/// - `client_id` only (public clients)
#[derive(Debug, Clone, Deserialize)]
pub struct TokenRequest {
    /// OAuth 2.0 grant type.
    /// Required. One of: "authorization_code", "refresh_token", "client_credentials"
    pub grant_type: String,

    /// Authorization code (for authorization_code grant).
    #[serde(default)]
    pub code: Option<String>,

    /// Redirect URI (must match authorization request).
    #[serde(default)]
    pub redirect_uri: Option<String>,

    /// PKCE code verifier (for authorization_code grant).
    #[serde(default)]
    pub code_verifier: Option<String>,

    /// Client ID (for public clients or client_secret_post).
    #[serde(default)]
    pub client_id: Option<String>,

    /// Client secret (for client_secret_post authentication).
    #[serde(default)]
    pub client_secret: Option<String>,

    /// Client assertion type (for private_key_jwt).
    /// Must be "urn:ietf:params:oauth:client-assertion-type:jwt-bearer"
    #[serde(default)]
    pub client_assertion_type: Option<String>,

    /// Client assertion JWT (for private_key_jwt authentication).
    #[serde(default)]
    pub client_assertion: Option<String>,

    /// Refresh token (for refresh_token grant).
    #[serde(default)]
    pub refresh_token: Option<String>,

    /// Requested scope (for refresh_token grant, must be subset of original).
    #[serde(default)]
    pub scope: Option<String>,

    /// Username (for password grant - Resource Owner Password Credentials).
    #[serde(default)]
    pub username: Option<String>,

    /// Password (for password grant - Resource Owner Password Credentials).
    #[serde(default)]
    pub password: Option<String>,
}

/// Successful token response.
///
/// Returned when a token request succeeds. Contains the access token
/// and optionally refresh token, ID token, and SMART context.
///
/// # Example Response
///
/// ```json
/// {
///   "access_token": "eyJhbG...",
///   "token_type": "Bearer",
///   "expires_in": 3600,
///   "scope": "openid patient/*.read",
///   "refresh_token": "abc123...",
///   "patient": "Patient/123",
///   "need_patient_banner": true
/// }
/// ```
#[derive(Debug, Clone, Serialize)]
pub struct TokenResponse {
    /// The access token (JWT).
    pub access_token: String,

    /// Token type, always "Bearer".
    pub token_type: String,

    /// Access token lifetime in seconds.
    pub expires_in: u64,

    /// Granted scopes (space-separated).
    pub scope: String,

    /// Refresh token (if offline_access scope was granted).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,

    /// ID token (if openid scope was granted).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id_token: Option<String>,

    /// Patient context (FHIR Patient resource ID).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub patient: Option<String>,

    /// Encounter context (FHIR Encounter resource ID).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encounter: Option<String>,

    /// Additional FHIR context items (SMART v2).
    #[serde(rename = "fhirContext", skip_serializing_if = "Option::is_none")]
    pub fhir_context: Option<Vec<FhirContextItem>>,

    /// Whether to display patient banner in the app.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub need_patient_banner: Option<bool>,

    /// URL to SMART styling information.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub smart_style_url: Option<String>,
}

impl TokenResponse {
    /// Creates a new token response with required fields.
    #[must_use]
    pub fn new(access_token: String, expires_in: u64, scope: String) -> Self {
        Self {
            access_token,
            token_type: "Bearer".to_string(),
            expires_in,
            scope,
            refresh_token: None,
            id_token: None,
            patient: None,
            encounter: None,
            fhir_context: None,
            need_patient_banner: None,
            smart_style_url: None,
        }
    }

    /// Sets the refresh token.
    #[must_use]
    pub fn with_refresh_token(mut self, token: String) -> Self {
        self.refresh_token = Some(token);
        self
    }

    /// Sets the ID token.
    #[must_use]
    pub fn with_id_token(mut self, token: String) -> Self {
        self.id_token = Some(token);
        self
    }

    /// Sets the patient context.
    #[must_use]
    pub fn with_patient(mut self, patient: String) -> Self {
        self.patient = Some(patient);
        self
    }

    /// Sets the encounter context.
    #[must_use]
    pub fn with_encounter(mut self, encounter: String) -> Self {
        self.encounter = Some(encounter);
        self
    }

    /// Sets the FHIR context items.
    #[must_use]
    pub fn with_fhir_context(mut self, context: Vec<FhirContextItem>) -> Self {
        self.fhir_context = Some(context);
        self
    }

    /// Sets the patient banner flag.
    #[must_use]
    pub fn with_patient_banner(mut self, need_banner: bool) -> Self {
        self.need_patient_banner = Some(need_banner);
        self
    }

    /// Sets the SMART style URL.
    #[must_use]
    pub fn with_smart_style_url(mut self, url: String) -> Self {
        self.smart_style_url = Some(url);
        self
    }
}

/// Token error response.
///
/// Returned when a token request fails. Contains an error code and
/// optional description.
///
/// # Example Response
///
/// ```json
/// {
///   "error": "invalid_grant",
///   "error_description": "Authorization code expired"
/// }
/// ```
#[derive(Debug, Clone, Serialize)]
pub struct TokenError {
    /// OAuth 2.0 error code.
    pub error: TokenErrorCode,

    /// Human-readable error description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_description: Option<String>,
}

impl TokenError {
    /// Creates a new token error.
    #[must_use]
    pub fn new(error: TokenErrorCode) -> Self {
        Self {
            error,
            error_description: None,
        }
    }

    /// Creates a new token error with description.
    #[must_use]
    pub fn with_description(error: TokenErrorCode, description: impl Into<String>) -> Self {
        Self {
            error,
            error_description: Some(description.into()),
        }
    }

    /// Creates an invalid_request error.
    #[must_use]
    pub fn invalid_request(description: impl Into<String>) -> Self {
        Self::with_description(TokenErrorCode::InvalidRequest, description)
    }

    /// Creates an invalid_client error.
    #[must_use]
    pub fn invalid_client(description: impl Into<String>) -> Self {
        Self::with_description(TokenErrorCode::InvalidClient, description)
    }

    /// Creates an invalid_grant error.
    #[must_use]
    pub fn invalid_grant(description: impl Into<String>) -> Self {
        Self::with_description(TokenErrorCode::InvalidGrant, description)
    }

    /// Creates an unauthorized_client error.
    #[must_use]
    pub fn unauthorized_client(description: impl Into<String>) -> Self {
        Self::with_description(TokenErrorCode::UnauthorizedClient, description)
    }

    /// Creates an unsupported_grant_type error.
    #[must_use]
    pub fn unsupported_grant_type(description: impl Into<String>) -> Self {
        Self::with_description(TokenErrorCode::UnsupportedGrantType, description)
    }

    /// Creates an invalid_scope error.
    #[must_use]
    pub fn invalid_scope(description: impl Into<String>) -> Self {
        Self::with_description(TokenErrorCode::InvalidScope, description)
    }
}

/// OAuth 2.0 token error codes.
///
/// Defined in RFC 6749 Section 5.2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TokenErrorCode {
    /// The request is missing a required parameter, includes an unsupported
    /// parameter value, includes a parameter more than once, or is otherwise
    /// malformed.
    InvalidRequest,

    /// Client authentication failed (unknown client, no client authentication
    /// included, or unsupported authentication method).
    InvalidClient,

    /// The provided authorization grant or refresh token is invalid, expired,
    /// revoked, or was issued to another client.
    InvalidGrant,

    /// The authenticated client is not authorized to use this authorization
    /// grant type.
    UnauthorizedClient,

    /// The authorization grant type is not supported by the authorization server.
    UnsupportedGrantType,

    /// The requested scope is invalid, unknown, malformed, or exceeds the scope
    /// granted by the resource owner.
    InvalidScope,
}

impl TokenErrorCode {
    /// Returns the string representation of the error code.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::InvalidRequest => "invalid_request",
            Self::InvalidClient => "invalid_client",
            Self::InvalidGrant => "invalid_grant",
            Self::UnauthorizedClient => "unauthorized_client",
            Self::UnsupportedGrantType => "unsupported_grant_type",
            Self::InvalidScope => "invalid_scope",
        }
    }

    /// Returns the HTTP status code for this error.
    #[must_use]
    pub fn http_status(&self) -> u16 {
        match self {
            Self::InvalidClient => 401,
            Self::InvalidRequest
            | Self::InvalidGrant
            | Self::UnauthorizedClient
            | Self::UnsupportedGrantType
            | Self::InvalidScope => 400,
        }
    }
}

impl fmt::Display for TokenErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_request_deserialization() {
        let json = r#"{
            "grant_type": "authorization_code",
            "code": "SplxlOBeZQQYbYS6WxSbIA",
            "redirect_uri": "https://app.example.com/callback",
            "code_verifier": "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk",
            "client_id": "my-app"
        }"#;

        let request: TokenRequest = serde_json::from_str(json).unwrap();
        assert_eq!(request.grant_type, "authorization_code");
        assert_eq!(request.code, Some("SplxlOBeZQQYbYS6WxSbIA".to_string()));
        assert_eq!(
            request.redirect_uri,
            Some("https://app.example.com/callback".to_string())
        );
        assert_eq!(
            request.code_verifier,
            Some("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk".to_string())
        );
        assert_eq!(request.client_id, Some("my-app".to_string()));
        assert!(request.client_secret.is_none());
        assert!(request.refresh_token.is_none());
    }

    #[test]
    fn test_token_request_refresh_grant() {
        let json = r#"{
            "grant_type": "refresh_token",
            "refresh_token": "tGzv3JOkF0XG5Qx2TlKWIA",
            "client_id": "my-app",
            "scope": "openid"
        }"#;

        let request: TokenRequest = serde_json::from_str(json).unwrap();
        assert_eq!(request.grant_type, "refresh_token");
        assert_eq!(
            request.refresh_token,
            Some("tGzv3JOkF0XG5Qx2TlKWIA".to_string())
        );
        assert_eq!(request.scope, Some("openid".to_string()));
    }

    #[test]
    fn test_token_request_client_credentials() {
        let json = r#"{
            "grant_type": "client_credentials",
            "client_id": "backend-service",
            "client_secret": "secret123",
            "scope": "system/*.read"
        }"#;

        let request: TokenRequest = serde_json::from_str(json).unwrap();
        assert_eq!(request.grant_type, "client_credentials");
        assert_eq!(request.client_id, Some("backend-service".to_string()));
        assert_eq!(request.client_secret, Some("secret123".to_string()));
    }

    #[test]
    fn test_token_response_serialization() {
        let response = TokenResponse::new(
            "eyJhbGciOiJSUzI1NiIsInR5cCI6IkpXVCJ9...".to_string(),
            3600,
            "openid patient/*.read".to_string(),
        );

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains(r#""access_token":"eyJhbGciOiJSUzI1NiIsInR5cCI6IkpXVCJ9..."#));
        assert!(json.contains(r#""token_type":"Bearer""#));
        assert!(json.contains(r#""expires_in":3600"#));
        assert!(json.contains(r#""scope":"openid patient/*.read""#));
        // Optional fields should not be present (use field name format to avoid matching scope content)
        assert!(!json.contains(r#""refresh_token":"#));
        assert!(!json.contains(r#""id_token":"#));
        assert!(!json.contains(r#""patient":"#));
    }

    #[test]
    fn test_token_response_with_all_fields() {
        let response = TokenResponse::new("access-token".to_string(), 3600, "openid".to_string())
            .with_refresh_token("refresh-token".to_string())
            .with_id_token("id-token".to_string())
            .with_patient("Patient/123".to_string())
            .with_encounter("Encounter/456".to_string())
            .with_patient_banner(true)
            .with_smart_style_url("https://style.example.com".to_string());

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains(r#""refresh_token":"refresh-token""#));
        assert!(json.contains(r#""id_token":"id-token""#));
        assert!(json.contains(r#""patient":"Patient/123""#));
        assert!(json.contains(r#""encounter":"Encounter/456""#));
        assert!(json.contains(r#""need_patient_banner":true"#));
        assert!(json.contains(r#""smart_style_url":"https://style.example.com""#));
    }

    #[test]
    fn test_token_error_serialization() {
        let error = TokenError::invalid_grant("Authorization code expired");

        let json = serde_json::to_string(&error).unwrap();
        assert!(json.contains(r#""error":"invalid_grant""#));
        assert!(json.contains(r#""error_description":"Authorization code expired""#));
    }

    #[test]
    fn test_token_error_without_description() {
        let error = TokenError::new(TokenErrorCode::InvalidClient);

        let json = serde_json::to_string(&error).unwrap();
        assert!(json.contains(r#""error":"invalid_client""#));
        assert!(!json.contains("error_description"));
    }

    #[test]
    fn test_error_code_as_str() {
        assert_eq!(TokenErrorCode::InvalidRequest.as_str(), "invalid_request");
        assert_eq!(TokenErrorCode::InvalidClient.as_str(), "invalid_client");
        assert_eq!(TokenErrorCode::InvalidGrant.as_str(), "invalid_grant");
        assert_eq!(
            TokenErrorCode::UnauthorizedClient.as_str(),
            "unauthorized_client"
        );
        assert_eq!(
            TokenErrorCode::UnsupportedGrantType.as_str(),
            "unsupported_grant_type"
        );
        assert_eq!(TokenErrorCode::InvalidScope.as_str(), "invalid_scope");
    }

    #[test]
    fn test_error_code_http_status() {
        assert_eq!(TokenErrorCode::InvalidRequest.http_status(), 400);
        assert_eq!(TokenErrorCode::InvalidClient.http_status(), 401);
        assert_eq!(TokenErrorCode::InvalidGrant.http_status(), 400);
        assert_eq!(TokenErrorCode::UnsupportedGrantType.http_status(), 400);
    }

    #[test]
    fn test_error_code_serde_roundtrip() {
        let codes = vec![
            TokenErrorCode::InvalidRequest,
            TokenErrorCode::InvalidClient,
            TokenErrorCode::InvalidGrant,
            TokenErrorCode::UnauthorizedClient,
            TokenErrorCode::UnsupportedGrantType,
            TokenErrorCode::InvalidScope,
        ];

        for code in codes {
            let json = serde_json::to_string(&code).unwrap();
            let deserialized: TokenErrorCode = serde_json::from_str(&json).unwrap();
            assert_eq!(code, deserialized);
        }
    }
}
