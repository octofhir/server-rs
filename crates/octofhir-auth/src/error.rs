//! Authentication and authorization error types.
//!
//! This module defines all error types that can occur during authentication
//! and authorization operations.

use std::fmt;

/// Errors that can occur during authentication and authorization operations.
#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    /// The client credentials are invalid or the client is not registered.
    #[error("Invalid client: {message}")]
    InvalidClient {
        /// Description of why the client is invalid.
        message: String,
    },

    /// The authorization grant or refresh token is invalid, expired, or revoked.
    #[error("Invalid grant: {message}")]
    InvalidGrant {
        /// Description of why the grant is invalid.
        message: String,
    },

    /// The requested scope is invalid, unknown, or malformed.
    #[error("Invalid scope: {message}")]
    InvalidScope {
        /// Description of why the scope is invalid.
        message: String,
    },

    /// The access token is invalid, malformed, or cannot be parsed.
    #[error("Invalid token: {message}")]
    InvalidToken {
        /// Description of why the token is invalid.
        message: String,
    },

    /// The request lacks valid authentication credentials.
    #[error("Unauthorized: {message}")]
    Unauthorized {
        /// Description of why the request is unauthorized.
        message: String,
    },

    /// The authenticated user does not have permission to perform the action.
    #[error("Forbidden: {message}")]
    Forbidden {
        /// Description of why access is forbidden.
        message: String,
    },

    /// The access token has expired.
    #[error("Token expired")]
    TokenExpired,

    /// The token has been explicitly revoked.
    #[error("Token revoked")]
    TokenRevoked,

    /// PKCE code verifier does not match the code challenge.
    #[error("PKCE verification failed")]
    PkceVerificationFailed,

    /// The authorization request is invalid or malformed.
    #[error("Invalid request: {message}")]
    InvalidRequest {
        /// Description of why the request is invalid.
        message: String,
    },

    /// The resource owner denied the authorization request.
    #[error("Access denied: {message}")]
    AccessDenied {
        /// Description of why access was denied.
        message: String,
    },

    /// The authorization server does not support the requested response type.
    #[error("Unsupported response type: {response_type}")]
    UnsupportedResponseType {
        /// The unsupported response type.
        response_type: String,
    },

    /// The authorization server does not support the requested grant type.
    #[error("Unsupported grant type: {grant_type}")]
    UnsupportedGrantType {
        /// The unsupported grant type.
        grant_type: String,
    },

    /// An error occurred while storing or retrieving auth data.
    #[error("Storage error: {message}")]
    Storage {
        /// Description of the storage error.
        message: String,
    },

    /// The auth configuration is invalid.
    #[error("Configuration error: {message}")]
    Configuration {
        /// Description of the configuration error.
        message: String,
    },

    /// An unexpected internal error occurred.
    #[error("Internal error: {message}")]
    Internal {
        /// Description of the internal error.
        message: String,
    },

    /// The identity provider returned an error.
    #[error("Identity provider error: {provider} - {message}")]
    IdentityProvider {
        /// The identity provider name.
        provider: String,
        /// Description of the error.
        message: String,
    },

    /// The policy evaluation failed or denied access.
    #[error("Policy error: {message}")]
    Policy {
        /// Description of the policy error.
        message: String,
    },
}

impl AuthError {
    /// Creates a new `InvalidClient` error.
    #[must_use]
    pub fn invalid_client(message: impl Into<String>) -> Self {
        Self::InvalidClient {
            message: message.into(),
        }
    }

    /// Creates a new `InvalidGrant` error.
    #[must_use]
    pub fn invalid_grant(message: impl Into<String>) -> Self {
        Self::InvalidGrant {
            message: message.into(),
        }
    }

    /// Creates a new `InvalidScope` error.
    #[must_use]
    pub fn invalid_scope(message: impl Into<String>) -> Self {
        Self::InvalidScope {
            message: message.into(),
        }
    }

    /// Creates a new `InvalidToken` error.
    #[must_use]
    pub fn invalid_token(message: impl Into<String>) -> Self {
        Self::InvalidToken {
            message: message.into(),
        }
    }

    /// Creates a new `Unauthorized` error.
    #[must_use]
    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self::Unauthorized {
            message: message.into(),
        }
    }

    /// Creates a new `Forbidden` error.
    #[must_use]
    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::Forbidden {
            message: message.into(),
        }
    }

    /// Creates a new `InvalidRequest` error.
    #[must_use]
    pub fn invalid_request(message: impl Into<String>) -> Self {
        Self::InvalidRequest {
            message: message.into(),
        }
    }

    /// Creates a new `AccessDenied` error.
    #[must_use]
    pub fn access_denied(message: impl Into<String>) -> Self {
        Self::AccessDenied {
            message: message.into(),
        }
    }

    /// Creates a new `UnsupportedResponseType` error.
    #[must_use]
    pub fn unsupported_response_type(response_type: impl Into<String>) -> Self {
        Self::UnsupportedResponseType {
            response_type: response_type.into(),
        }
    }

    /// Creates a new `UnsupportedGrantType` error.
    #[must_use]
    pub fn unsupported_grant_type(grant_type: impl Into<String>) -> Self {
        Self::UnsupportedGrantType {
            grant_type: grant_type.into(),
        }
    }

    /// Creates a new `Storage` error.
    #[must_use]
    pub fn storage(message: impl Into<String>) -> Self {
        Self::Storage {
            message: message.into(),
        }
    }

    /// Creates a new `Configuration` error.
    #[must_use]
    pub fn configuration(message: impl Into<String>) -> Self {
        Self::Configuration {
            message: message.into(),
        }
    }

    /// Creates a new `Internal` error.
    #[must_use]
    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal {
            message: message.into(),
        }
    }

    /// Creates a new `IdentityProvider` error.
    #[must_use]
    pub fn identity_provider(provider: impl Into<String>, message: impl Into<String>) -> Self {
        Self::IdentityProvider {
            provider: provider.into(),
            message: message.into(),
        }
    }

    /// Creates a new `Policy` error.
    #[must_use]
    pub fn policy(message: impl Into<String>) -> Self {
        Self::Policy {
            message: message.into(),
        }
    }

    /// Returns `true` if this is a client error (4xx category).
    #[must_use]
    pub fn is_client_error(&self) -> bool {
        matches!(
            self,
            Self::InvalidClient { .. }
                | Self::InvalidGrant { .. }
                | Self::InvalidScope { .. }
                | Self::InvalidToken { .. }
                | Self::Unauthorized { .. }
                | Self::Forbidden { .. }
                | Self::TokenExpired
                | Self::TokenRevoked
                | Self::PkceVerificationFailed
                | Self::InvalidRequest { .. }
                | Self::AccessDenied { .. }
                | Self::UnsupportedResponseType { .. }
                | Self::UnsupportedGrantType { .. }
        )
    }

    /// Returns `true` if this is a server error (5xx category).
    #[must_use]
    pub fn is_server_error(&self) -> bool {
        matches!(
            self,
            Self::Storage { .. }
                | Self::Configuration { .. }
                | Self::Internal { .. }
                | Self::IdentityProvider { .. }
        )
    }

    /// Returns `true` if this is an authentication error.
    #[must_use]
    pub fn is_authentication_error(&self) -> bool {
        matches!(
            self,
            Self::InvalidClient { .. }
                | Self::InvalidGrant { .. }
                | Self::InvalidToken { .. }
                | Self::Unauthorized { .. }
                | Self::TokenExpired
                | Self::TokenRevoked
                | Self::PkceVerificationFailed
        )
    }

    /// Returns `true` if this is an authorization error.
    #[must_use]
    pub fn is_authorization_error(&self) -> bool {
        matches!(
            self,
            Self::InvalidScope { .. }
                | Self::Forbidden { .. }
                | Self::AccessDenied { .. }
                | Self::Policy { .. }
        )
    }

    /// Returns `true` if this is a token-related error.
    #[must_use]
    pub fn is_token_error(&self) -> bool {
        matches!(
            self,
            Self::InvalidToken { .. } | Self::TokenExpired | Self::TokenRevoked
        )
    }

    /// Returns the error category for logging/monitoring purposes.
    #[must_use]
    pub fn category(&self) -> ErrorCategory {
        match self {
            Self::InvalidClient { .. } => ErrorCategory::Authentication,
            Self::InvalidGrant { .. } => ErrorCategory::Authentication,
            Self::InvalidScope { .. } => ErrorCategory::Authorization,
            Self::InvalidToken { .. } => ErrorCategory::Token,
            Self::Unauthorized { .. } => ErrorCategory::Authentication,
            Self::Forbidden { .. } => ErrorCategory::Authorization,
            Self::TokenExpired => ErrorCategory::Token,
            Self::TokenRevoked => ErrorCategory::Token,
            Self::PkceVerificationFailed => ErrorCategory::Authentication,
            Self::InvalidRequest { .. } => ErrorCategory::Validation,
            Self::AccessDenied { .. } => ErrorCategory::Authorization,
            Self::UnsupportedResponseType { .. } => ErrorCategory::Validation,
            Self::UnsupportedGrantType { .. } => ErrorCategory::Validation,
            Self::Storage { .. } => ErrorCategory::Infrastructure,
            Self::Configuration { .. } => ErrorCategory::Configuration,
            Self::Internal { .. } => ErrorCategory::Internal,
            Self::IdentityProvider { .. } => ErrorCategory::Federation,
            Self::Policy { .. } => ErrorCategory::Authorization,
        }
    }

    /// Returns the OAuth 2.0 error code for this error.
    #[must_use]
    pub fn oauth_error_code(&self) -> &'static str {
        match self {
            Self::InvalidClient { .. } => "invalid_client",
            Self::InvalidGrant { .. } => "invalid_grant",
            Self::InvalidScope { .. } => "invalid_scope",
            Self::InvalidToken { .. } => "invalid_token",
            Self::Unauthorized { .. } => "unauthorized",
            Self::Forbidden { .. } => "access_denied",
            Self::TokenExpired => "invalid_token",
            Self::TokenRevoked => "invalid_token",
            Self::PkceVerificationFailed => "invalid_grant",
            Self::InvalidRequest { .. } => "invalid_request",
            Self::AccessDenied { .. } => "access_denied",
            Self::UnsupportedResponseType { .. } => "unsupported_response_type",
            Self::UnsupportedGrantType { .. } => "unsupported_grant_type",
            Self::Storage { .. } => "server_error",
            Self::Configuration { .. } => "server_error",
            Self::Internal { .. } => "server_error",
            Self::IdentityProvider { .. } => "server_error",
            Self::Policy { .. } => "access_denied",
        }
    }
}

/// Categories of authentication/authorization errors for logging and monitoring.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorCategory {
    /// Authentication-related errors (identity verification).
    Authentication,
    /// Authorization-related errors (permission checks).
    Authorization,
    /// Token-related errors (validation, expiration).
    Token,
    /// Request validation errors.
    Validation,
    /// Infrastructure/storage errors.
    Infrastructure,
    /// Configuration errors.
    Configuration,
    /// Internal server errors.
    Internal,
    /// Identity provider federation errors.
    Federation,
}

impl fmt::Display for ErrorCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Authentication => write!(f, "authentication"),
            Self::Authorization => write!(f, "authorization"),
            Self::Token => write!(f, "token"),
            Self::Validation => write!(f, "validation"),
            Self::Infrastructure => write!(f, "infrastructure"),
            Self::Configuration => write!(f, "configuration"),
            Self::Internal => write!(f, "internal"),
            Self::Federation => write!(f, "federation"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = AuthError::invalid_client("client not found");
        assert_eq!(err.to_string(), "Invalid client: client not found");

        let err = AuthError::invalid_grant("expired authorization code");
        assert_eq!(err.to_string(), "Invalid grant: expired authorization code");

        let err = AuthError::TokenExpired;
        assert_eq!(err.to_string(), "Token expired");

        let err = AuthError::identity_provider("google", "connection failed");
        assert_eq!(
            err.to_string(),
            "Identity provider error: google - connection failed"
        );
    }

    #[test]
    fn test_error_predicates() {
        let err = AuthError::invalid_client("test");
        assert!(err.is_client_error());
        assert!(!err.is_server_error());
        assert!(err.is_authentication_error());
        assert!(!err.is_authorization_error());

        let err = AuthError::forbidden("no access");
        assert!(err.is_client_error());
        assert!(!err.is_authentication_error());
        assert!(err.is_authorization_error());

        let err = AuthError::TokenExpired;
        assert!(err.is_client_error());
        assert!(err.is_token_error());

        let err = AuthError::storage("database down");
        assert!(!err.is_client_error());
        assert!(err.is_server_error());
    }

    #[test]
    fn test_error_category() {
        assert_eq!(
            AuthError::invalid_client("test").category(),
            ErrorCategory::Authentication
        );
        assert_eq!(
            AuthError::forbidden("test").category(),
            ErrorCategory::Authorization
        );
        assert_eq!(AuthError::TokenExpired.category(), ErrorCategory::Token);
        assert_eq!(
            AuthError::storage("test").category(),
            ErrorCategory::Infrastructure
        );
        assert_eq!(
            AuthError::configuration("test").category(),
            ErrorCategory::Configuration
        );
        assert_eq!(
            AuthError::identity_provider("google", "test").category(),
            ErrorCategory::Federation
        );
    }

    #[test]
    fn test_oauth_error_code() {
        assert_eq!(
            AuthError::invalid_client("test").oauth_error_code(),
            "invalid_client"
        );
        assert_eq!(
            AuthError::invalid_grant("test").oauth_error_code(),
            "invalid_grant"
        );
        assert_eq!(
            AuthError::invalid_scope("test").oauth_error_code(),
            "invalid_scope"
        );
        assert_eq!(AuthError::TokenExpired.oauth_error_code(), "invalid_token");
        assert_eq!(
            AuthError::PkceVerificationFailed.oauth_error_code(),
            "invalid_grant"
        );
        assert_eq!(
            AuthError::unsupported_grant_type("test").oauth_error_code(),
            "unsupported_grant_type"
        );
    }

    #[test]
    fn test_error_category_display() {
        assert_eq!(ErrorCategory::Authentication.to_string(), "authentication");
        assert_eq!(ErrorCategory::Authorization.to_string(), "authorization");
        assert_eq!(ErrorCategory::Token.to_string(), "token");
        assert_eq!(ErrorCategory::Federation.to_string(), "federation");
    }
}
