//! Token introspection (RFC 7662)
//!
//! This module implements OAuth 2.0 Token Introspection per RFC 7662,
//! allowing resource servers to validate tokens and retrieve metadata.
//!
//! # Security Considerations
//!
//! - Introspection endpoint requires client authentication
//! - Never reveal why a token is inactive (expired vs revoked vs invalid)
//! - Always return valid JSON response
//!
//! # Example
//!
//! ```ignore
//! use octofhir_auth::token::introspection::{IntrospectionRequest, IntrospectionResponse};
//!
//! let request = IntrospectionRequest {
//!     token: "access_token_value".to_string(),
//!     token_type_hint: Some(TokenTypeHint::AccessToken),
//! };
//!
//! // Introspect the token
//! let response = token_service.introspect(&request).await;
//! ```
//!
//! # References
//!
//! - [RFC 7662 - OAuth 2.0 Token Introspection](https://tools.ietf.org/html/rfc7662)

use serde::{Deserialize, Serialize};

use super::revocation::TokenTypeHint;

// =============================================================================
// Request Types
// =============================================================================

/// Token introspection request per RFC 7662.
///
/// The client sends this request to the introspection endpoint to
/// determine the active state of a token and retrieve metadata.
#[derive(Debug, Clone, Deserialize)]
pub struct IntrospectionRequest {
    /// The token to introspect.
    pub token: String,

    /// Optional hint about the token type.
    ///
    /// Per RFC 7662, this is a hint to the authorization server about what
    /// type of token is being introspected. The server may attempt to identify
    /// the token type even without this hint.
    #[serde(default)]
    pub token_type_hint: Option<TokenTypeHint>,
}

// =============================================================================
// Response Types
// =============================================================================

/// Token introspection response per RFC 7662.
///
/// Contains the token's active state and metadata if active.
/// If the token is invalid, expired, revoked, or unknown, the response
/// will only contain `active: false` with no additional claims.
#[derive(Debug, Clone, Serialize, Default)]
pub struct IntrospectionResponse {
    /// Boolean indicator of whether the token is currently active.
    ///
    /// Per RFC 7662, this is the ONLY required field.
    pub active: bool,

    /// A space-separated list of scope values granted to the token.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,

    /// Client identifier for the OAuth 2.0 client that requested this token.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,

    /// Human-readable identifier for the resource owner who authorized this token.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,

    /// Type of the token (e.g., "Bearer").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_type: Option<String>,

    /// Expiration time (Unix timestamp).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exp: Option<i64>,

    /// Issued at time (Unix timestamp).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iat: Option<i64>,

    /// Not before time (Unix timestamp).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nbf: Option<i64>,

    /// Subject identifier (user or client ID).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sub: Option<String>,

    /// Intended audience(s) for this token.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aud: Option<Vec<String>>,

    /// Issuer of the token.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iss: Option<String>,

    /// JWT ID (unique identifier for the token).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jti: Option<String>,

    // =========================================================================
    // SMART on FHIR Extensions
    // =========================================================================
    /// Patient context (SMART on FHIR).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub patient: Option<String>,

    /// Encounter context (SMART on FHIR).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encounter: Option<String>,

    /// User's FHIR resource reference (SMART on FHIR).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fhir_user: Option<String>,
}

impl IntrospectionResponse {
    /// Creates an inactive response (used for invalid/expired/revoked tokens).
    ///
    /// Per RFC 7662, we should not reveal why a token is inactive.
    #[must_use]
    pub fn inactive() -> Self {
        Self {
            active: false,
            ..Default::default()
        }
    }

    /// Creates an active response with the provided claims.
    #[must_use]
    pub fn active() -> Self {
        Self {
            active: true,
            ..Default::default()
        }
    }

    /// Sets the scope.
    #[must_use]
    pub fn with_scope(mut self, scope: impl Into<String>) -> Self {
        self.scope = Some(scope.into());
        self
    }

    /// Sets the client ID.
    #[must_use]
    pub fn with_client_id(mut self, client_id: impl Into<String>) -> Self {
        self.client_id = Some(client_id.into());
        self
    }

    /// Sets the username.
    #[must_use]
    pub fn with_username(mut self, username: impl Into<String>) -> Self {
        self.username = Some(username.into());
        self
    }

    /// Sets the token type.
    #[must_use]
    pub fn with_token_type(mut self, token_type: impl Into<String>) -> Self {
        self.token_type = Some(token_type.into());
        self
    }

    /// Sets the expiration time.
    #[must_use]
    pub fn with_exp(mut self, exp: i64) -> Self {
        self.exp = Some(exp);
        self
    }

    /// Sets the issued at time.
    #[must_use]
    pub fn with_iat(mut self, iat: i64) -> Self {
        self.iat = Some(iat);
        self
    }

    /// Sets the not before time.
    #[must_use]
    pub fn with_nbf(mut self, nbf: i64) -> Self {
        self.nbf = Some(nbf);
        self
    }

    /// Sets the subject.
    #[must_use]
    pub fn with_sub(mut self, sub: impl Into<String>) -> Self {
        self.sub = Some(sub.into());
        self
    }

    /// Sets the audience.
    #[must_use]
    pub fn with_aud(mut self, aud: Vec<String>) -> Self {
        self.aud = Some(aud);
        self
    }

    /// Sets the issuer.
    #[must_use]
    pub fn with_iss(mut self, iss: impl Into<String>) -> Self {
        self.iss = Some(iss.into());
        self
    }

    /// Sets the JWT ID.
    #[must_use]
    pub fn with_jti(mut self, jti: impl Into<String>) -> Self {
        self.jti = Some(jti.into());
        self
    }

    /// Sets the patient context.
    #[must_use]
    pub fn with_patient(mut self, patient: impl Into<String>) -> Self {
        self.patient = Some(patient.into());
        self
    }

    /// Sets the encounter context.
    #[must_use]
    pub fn with_encounter(mut self, encounter: impl Into<String>) -> Self {
        self.encounter = Some(encounter.into());
        self
    }

    /// Sets the FHIR user.
    #[must_use]
    pub fn with_fhir_user(mut self, fhir_user: impl Into<String>) -> Self {
        self.fhir_user = Some(fhir_user.into());
        self
    }
}

// =============================================================================
// Error Types
// =============================================================================

/// Introspection error response.
///
/// Per RFC 7662, most errors should return `{"active": false}`.
/// These errors are only for client authentication failures.
#[derive(Debug, Clone, Serialize)]
pub struct IntrospectionError {
    /// The error code.
    pub error: IntrospectionErrorCode,

    /// Optional human-readable error description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_description: Option<String>,
}

impl IntrospectionError {
    /// Creates a new introspection error.
    #[must_use]
    pub fn new(error: IntrospectionErrorCode) -> Self {
        Self {
            error,
            error_description: None,
        }
    }

    /// Creates a new introspection error with a description.
    #[must_use]
    pub fn with_description(error: IntrospectionErrorCode, description: impl Into<String>) -> Self {
        Self {
            error,
            error_description: Some(description.into()),
        }
    }

    /// Creates an `invalid_request` error.
    #[must_use]
    pub fn invalid_request(description: impl Into<String>) -> Self {
        Self::with_description(IntrospectionErrorCode::InvalidRequest, description)
    }

    /// Creates an `invalid_client` error.
    #[must_use]
    pub fn invalid_client(description: impl Into<String>) -> Self {
        Self::with_description(IntrospectionErrorCode::InvalidClient, description)
    }
}

impl std::fmt::Display for IntrospectionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.error.as_str())?;
        if let Some(ref desc) = self.error_description {
            write!(f, ": {}", desc)?;
        }
        Ok(())
    }
}

impl std::error::Error for IntrospectionError {}

/// Introspection error codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IntrospectionErrorCode {
    /// The request is missing a required parameter or is otherwise malformed.
    InvalidRequest,

    /// Client authentication failed.
    InvalidClient,
}

impl IntrospectionErrorCode {
    /// Returns the error code as a string.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::InvalidRequest => "invalid_request",
            Self::InvalidClient => "invalid_client",
        }
    }
}

impl std::fmt::Display for IntrospectionErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_introspection_request_deserialization() {
        let json = r#"{"token": "abc123"}"#;
        let request: IntrospectionRequest = serde_json::from_str(json).unwrap();
        assert_eq!(request.token, "abc123");
        assert!(request.token_type_hint.is_none());

        let json = r#"{"token": "abc123", "token_type_hint": "access_token"}"#;
        let request: IntrospectionRequest = serde_json::from_str(json).unwrap();
        assert_eq!(request.token, "abc123");
        assert_eq!(request.token_type_hint, Some(TokenTypeHint::AccessToken));

        let json = r#"{"token": "abc123", "token_type_hint": "refresh_token"}"#;
        let request: IntrospectionRequest = serde_json::from_str(json).unwrap();
        assert_eq!(request.token_type_hint, Some(TokenTypeHint::RefreshToken));
    }

    #[test]
    fn test_introspection_response_inactive() {
        let response = IntrospectionResponse::inactive();
        assert!(!response.active);
        assert!(response.scope.is_none());
        assert!(response.client_id.is_none());

        let json = serde_json::to_string(&response).unwrap();
        assert_eq!(json, r#"{"active":false}"#);
    }

    #[test]
    fn test_introspection_response_active() {
        let response = IntrospectionResponse::active()
            .with_scope("openid patient/*.read")
            .with_client_id("test-client")
            .with_sub("user123")
            .with_exp(1700000000)
            .with_iat(1699996400)
            .with_iss("https://auth.example.com")
            .with_jti("unique-token-id")
            .with_token_type("Bearer")
            .with_aud(vec!["https://fhir.example.com".to_string()])
            .with_patient("Patient/123")
            .with_encounter("Encounter/456")
            .with_fhir_user("Practitioner/789");

        assert!(response.active);
        assert_eq!(response.scope, Some("openid patient/*.read".to_string()));
        assert_eq!(response.client_id, Some("test-client".to_string()));
        assert_eq!(response.sub, Some("user123".to_string()));
        assert_eq!(response.exp, Some(1700000000));
        assert_eq!(response.patient, Some("Patient/123".to_string()));

        // Verify serialization
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"active\":true"));
        assert!(json.contains("\"scope\":\"openid patient/*.read\""));
        assert!(json.contains("\"patient\":\"Patient/123\""));
    }

    #[test]
    fn test_introspection_response_builder_chain() {
        let response = IntrospectionResponse::active()
            .with_scope("openid")
            .with_nbf(1699990000)
            .with_username("john.doe");

        assert!(response.active);
        assert_eq!(response.scope, Some("openid".to_string()));
        assert_eq!(response.nbf, Some(1699990000));
        assert_eq!(response.username, Some("john.doe".to_string()));
    }

    #[test]
    fn test_introspection_error_serialization() {
        let error = IntrospectionError::invalid_request("Missing token parameter");
        let json = serde_json::to_string(&error).unwrap();
        assert!(json.contains("invalid_request"));
        assert!(json.contains("Missing token parameter"));

        let error = IntrospectionError::new(IntrospectionErrorCode::InvalidClient);
        let json = serde_json::to_string(&error).unwrap();
        assert!(json.contains("invalid_client"));
        assert!(!json.contains("error_description"));
    }

    #[test]
    fn test_introspection_error_display() {
        let error = IntrospectionError::invalid_client("Authentication failed");
        assert_eq!(error.to_string(), "invalid_client: Authentication failed");

        let error = IntrospectionError::new(IntrospectionErrorCode::InvalidRequest);
        assert_eq!(error.to_string(), "invalid_request");
    }

    #[test]
    fn test_error_code_as_str() {
        assert_eq!(
            IntrospectionErrorCode::InvalidRequest.as_str(),
            "invalid_request"
        );
        assert_eq!(
            IntrospectionErrorCode::InvalidClient.as_str(),
            "invalid_client"
        );
    }
}
