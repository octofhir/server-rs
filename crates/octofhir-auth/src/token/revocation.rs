//! Token revocation (RFC 7009)
//!
//! This module implements OAuth 2.0 Token Revocation per RFC 7009,
//! supporting revocation of both access tokens and refresh tokens.
//!
//! # Security Considerations
//!
//! - Revocation endpoint always returns 200 OK for security (don't reveal token existence)
//! - Client must own the token to revoke it
//! - Revoked access tokens are tracked by JTI until expiration
//! - Revoked JTIs should be cleaned up periodically
//!
//! # Example
//!
//! ```ignore
//! use octofhir_auth::token::revocation::{RevocationRequest, TokenTypeHint};
//!
//! let request = RevocationRequest {
//!     token: "access_token_value".to_string(),
//!     token_type_hint: Some(TokenTypeHint::AccessToken),
//! };
//!
//! // Revoke the token
//! token_service.revoke(&request, &client).await?;
//! ```
//!
//! # References
//!
//! - [RFC 7009 - OAuth 2.0 Token Revocation](https://tools.ietf.org/html/rfc7009)

use serde::{Deserialize, Serialize};

// =============================================================================
// Request Types
// =============================================================================

/// Token revocation request per RFC 7009.
///
/// The client sends this request to the revocation endpoint to invalidate
/// a previously issued access token or refresh token.
#[derive(Debug, Clone, Deserialize)]
pub struct RevocationRequest {
    /// The token to revoke.
    ///
    /// This is the token string that was previously issued to the client.
    pub token: String,

    /// Optional hint about the token type.
    ///
    /// Per RFC 7009, this is a hint to the authorization server about what
    /// type of token is being revoked. The server may attempt to identify
    /// the token type even without this hint.
    #[serde(default)]
    pub token_type_hint: Option<TokenTypeHint>,
}

/// Token type hint for revocation requests.
///
/// Indicates whether the token being revoked is an access token or refresh token.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TokenTypeHint {
    /// The token is an access token.
    AccessToken,
    /// The token is a refresh token.
    RefreshToken,
}

impl TokenTypeHint {
    /// Returns the token type hint as a string.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::AccessToken => "access_token",
            Self::RefreshToken => "refresh_token",
        }
    }
}

impl std::fmt::Display for TokenTypeHint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

// =============================================================================
// Error Types
// =============================================================================

/// Revocation error response per RFC 7009.
///
/// Note: Per RFC 7009 Section 2.2, the revocation endpoint should return 200 OK
/// even for invalid tokens. This error type is primarily for internal use or
/// for cases where the server chooses to return an error (e.g., invalid client).
#[derive(Debug, Clone, Serialize)]
pub struct RevocationError {
    /// The error code.
    pub error: RevocationErrorCode,

    /// Optional human-readable error description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_description: Option<String>,
}

impl RevocationError {
    /// Creates a new revocation error.
    #[must_use]
    pub fn new(error: RevocationErrorCode) -> Self {
        Self {
            error,
            error_description: None,
        }
    }

    /// Creates a new revocation error with a description.
    #[must_use]
    pub fn with_description(error: RevocationErrorCode, description: impl Into<String>) -> Self {
        Self {
            error,
            error_description: Some(description.into()),
        }
    }

    /// Creates an `invalid_request` error.
    #[must_use]
    pub fn invalid_request(description: impl Into<String>) -> Self {
        Self::with_description(RevocationErrorCode::InvalidRequest, description)
    }

    /// Creates an `invalid_client` error.
    #[must_use]
    pub fn invalid_client(description: impl Into<String>) -> Self {
        Self::with_description(RevocationErrorCode::InvalidClient, description)
    }

    /// Creates an `unsupported_token_type` error.
    #[must_use]
    pub fn unsupported_token_type(description: impl Into<String>) -> Self {
        Self::with_description(RevocationErrorCode::UnsupportedTokenType, description)
    }
}

impl std::fmt::Display for RevocationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.error.as_str())?;
        if let Some(ref desc) = self.error_description {
            write!(f, ": {}", desc)?;
        }
        Ok(())
    }
}

impl std::error::Error for RevocationError {}

/// Revocation error codes per RFC 7009.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RevocationErrorCode {
    /// The request is missing a required parameter or is otherwise malformed.
    InvalidRequest,

    /// Client authentication failed.
    InvalidClient,

    /// The provided token is invalid (optional, per RFC 7009 Section 2.2.1).
    ///
    /// Note: Servers typically should NOT return this error to avoid
    /// revealing token existence. Return 200 OK instead.
    InvalidToken,

    /// The authorization server does not support the revocation of the
    /// presented token type.
    UnsupportedTokenType,
}

impl RevocationErrorCode {
    /// Returns the error code as a string.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::InvalidRequest => "invalid_request",
            Self::InvalidClient => "invalid_client",
            Self::InvalidToken => "invalid_token",
            Self::UnsupportedTokenType => "unsupported_token_type",
        }
    }
}

impl std::fmt::Display for RevocationErrorCode {
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
    fn test_revocation_request_deserialization() {
        let json = r#"{"token": "abc123"}"#;
        let request: RevocationRequest = serde_json::from_str(json).unwrap();
        assert_eq!(request.token, "abc123");
        assert!(request.token_type_hint.is_none());

        let json = r#"{"token": "abc123", "token_type_hint": "access_token"}"#;
        let request: RevocationRequest = serde_json::from_str(json).unwrap();
        assert_eq!(request.token, "abc123");
        assert_eq!(request.token_type_hint, Some(TokenTypeHint::AccessToken));

        let json = r#"{"token": "abc123", "token_type_hint": "refresh_token"}"#;
        let request: RevocationRequest = serde_json::from_str(json).unwrap();
        assert_eq!(request.token_type_hint, Some(TokenTypeHint::RefreshToken));
    }

    #[test]
    fn test_token_type_hint_display() {
        assert_eq!(TokenTypeHint::AccessToken.to_string(), "access_token");
        assert_eq!(TokenTypeHint::RefreshToken.to_string(), "refresh_token");
    }

    #[test]
    fn test_revocation_error_serialization() {
        let error = RevocationError::invalid_request("Missing token parameter");
        let json = serde_json::to_string(&error).unwrap();
        assert!(json.contains("invalid_request"));
        assert!(json.contains("Missing token parameter"));

        let error = RevocationError::new(RevocationErrorCode::InvalidClient);
        let json = serde_json::to_string(&error).unwrap();
        assert!(json.contains("invalid_client"));
        assert!(!json.contains("error_description"));
    }

    #[test]
    fn test_revocation_error_display() {
        let error = RevocationError::invalid_client("Authentication failed");
        assert_eq!(error.to_string(), "invalid_client: Authentication failed");

        let error = RevocationError::new(RevocationErrorCode::UnsupportedTokenType);
        assert_eq!(error.to_string(), "unsupported_token_type");
    }

    #[test]
    fn test_error_code_as_str() {
        assert_eq!(
            RevocationErrorCode::InvalidRequest.as_str(),
            "invalid_request"
        );
        assert_eq!(
            RevocationErrorCode::InvalidClient.as_str(),
            "invalid_client"
        );
        assert_eq!(RevocationErrorCode::InvalidToken.as_str(), "invalid_token");
        assert_eq!(
            RevocationErrorCode::UnsupportedTokenType.as_str(),
            "unsupported_token_type"
        );
    }
}
