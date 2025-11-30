//! Error types for external identity provider operations.
//!
//! This module provides error types for IdP authentication flows including
//! authorization, token exchange, and token validation.

use super::discovery::DiscoveryError;
use super::jwks::JwksError;

/// Errors that can occur during IdP authentication operations.
#[derive(Debug, thiserror::Error)]
pub enum IdpError {
    /// The requested provider was not found.
    #[error("Identity provider not found: {0}")]
    ProviderNotFound(String),

    /// The provider is disabled.
    #[error("Identity provider is disabled: {0}")]
    ProviderDisabled(String),

    /// Failed to fetch OIDC discovery document.
    #[error("Discovery failed: {0}")]
    DiscoveryFailed(#[from] DiscoveryError),

    /// Failed to fetch or use JWKS.
    #[error("JWKS error: {0}")]
    JwksFailed(#[from] JwksError),

    /// Token exchange with the IdP failed.
    #[error("Token exchange failed: {0}")]
    TokenExchangeFailed(String),

    /// ID token validation failed.
    #[error("Token validation failed: {0}")]
    TokenValidationFailed(String),

    /// The ID token is missing the required `kid` header.
    #[error("ID token is missing key ID (kid) header")]
    MissingKeyId,

    /// The nonce in the ID token doesn't match the expected nonce.
    #[error("Nonce mismatch: ID token nonce does not match expected nonce")]
    NonceMismatch,

    /// The audience in the ID token doesn't match our client ID.
    #[error("Audience mismatch: ID token audience does not include our client ID")]
    AudienceMismatch,

    /// The issuer in the ID token doesn't match the expected issuer.
    #[error("Issuer mismatch: expected {expected}, got {actual}")]
    IssuerMismatch {
        /// The expected issuer URL.
        expected: String,
        /// The actual issuer from the ID token.
        actual: String,
    },

    /// The ID token has expired.
    #[error("ID token has expired")]
    TokenExpired,

    /// The ID token is not yet valid (iat in the future).
    #[error("ID token is not yet valid")]
    TokenNotYetValid,

    /// Failed to map user claims from the ID token.
    #[error("User mapping failed: {0}")]
    UserMappingFailed(String),

    /// A network error occurred.
    #[error("Network error: {0}")]
    NetworkError(#[from] reqwest::Error),

    /// Failed to parse a URL.
    #[error("URL error: {0}")]
    UrlError(#[from] url::ParseError),

    /// JWT decoding or validation error.
    #[error("JWT error: {0}")]
    JwtError(#[from] jsonwebtoken::errors::Error),

    /// The IdP returned an OAuth error.
    #[error("OAuth error from IdP: {error} - {description}")]
    OAuthError {
        /// The OAuth error code.
        error: String,
        /// Optional error description.
        description: String,
    },

    /// Missing required field in configuration or token.
    #[error("Missing required field: {0}")]
    MissingField(String),
}

impl IdpError {
    /// Creates an `IssuerMismatch` error.
    #[must_use]
    pub fn issuer_mismatch(expected: impl Into<String>, actual: impl Into<String>) -> Self {
        Self::IssuerMismatch {
            expected: expected.into(),
            actual: actual.into(),
        }
    }

    /// Creates an `OAuthError` from IdP response.
    #[must_use]
    pub fn oauth_error(error: impl Into<String>, description: impl Into<String>) -> Self {
        Self::OAuthError {
            error: error.into(),
            description: description.into(),
        }
    }

    /// Returns `true` if this is a provider configuration error.
    #[must_use]
    pub fn is_provider_error(&self) -> bool {
        matches!(self, Self::ProviderNotFound(_) | Self::ProviderDisabled(_))
    }

    /// Returns `true` if this is a token validation error.
    #[must_use]
    pub fn is_validation_error(&self) -> bool {
        matches!(
            self,
            Self::TokenValidationFailed(_)
                | Self::MissingKeyId
                | Self::NonceMismatch
                | Self::AudienceMismatch
                | Self::IssuerMismatch { .. }
                | Self::TokenExpired
                | Self::TokenNotYetValid
                | Self::JwtError(_)
        )
    }

    /// Returns `true` if this is a network or external service error.
    #[must_use]
    pub fn is_external_error(&self) -> bool {
        matches!(
            self,
            Self::DiscoveryFailed(_)
                | Self::JwksFailed(_)
                | Self::TokenExchangeFailed(_)
                | Self::NetworkError(_)
                | Self::OAuthError { .. }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = IdpError::ProviderNotFound("google".to_string());
        assert_eq!(err.to_string(), "Identity provider not found: google");

        let err = IdpError::ProviderDisabled("github".to_string());
        assert_eq!(err.to_string(), "Identity provider is disabled: github");

        let err = IdpError::issuer_mismatch("https://a.com", "https://b.com");
        assert!(err.to_string().contains("https://a.com"));
        assert!(err.to_string().contains("https://b.com"));

        let err = IdpError::oauth_error("invalid_grant", "Token expired");
        assert!(err.to_string().contains("invalid_grant"));
        assert!(err.to_string().contains("Token expired"));
    }

    #[test]
    fn test_error_predicates() {
        assert!(IdpError::ProviderNotFound("x".to_string()).is_provider_error());
        assert!(IdpError::ProviderDisabled("x".to_string()).is_provider_error());
        assert!(!IdpError::NonceMismatch.is_provider_error());

        assert!(IdpError::NonceMismatch.is_validation_error());
        assert!(IdpError::AudienceMismatch.is_validation_error());
        assert!(IdpError::TokenExpired.is_validation_error());
        assert!(!IdpError::TokenExchangeFailed("x".to_string()).is_validation_error());

        assert!(IdpError::TokenExchangeFailed("x".to_string()).is_external_error());
        assert!(IdpError::oauth_error("err", "desc").is_external_error());
        assert!(!IdpError::NonceMismatch.is_external_error());
    }
}
