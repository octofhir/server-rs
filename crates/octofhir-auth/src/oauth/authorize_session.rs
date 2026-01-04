//! OAuth authorize flow session management.
//!
//! This module provides types for managing the multi-step OAuth 2.0 authorization
//! flow before the authorization code is issued. The session tracks user authentication
//! state during the login and consent screens.
//!
//! # Lifecycle
//!
//! 1. Session created when GET /oauth/authorize is received
//! 2. User authenticates on login form (session updated with user_id)
//! 3. User provides consent on consent form
//! 4. Authorization code is issued and session is deleted
//!
//! # Distinction from AuthorizationSession
//!
//! - `AuthorizeSession` - Tracks the UI flow (login → consent) BEFORE code issuance
//! - `AuthorizationSession` - Stores the authorization CODE after code issuance

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

use super::authorize::AuthorizationRequest;

/// Default session expiry in seconds (10 minutes).
pub const DEFAULT_SESSION_EXPIRY_SECS: i64 = 600;

/// OAuth authorize flow session stored in the database.
///
/// Represents the state of an authorization request during the login/consent UI flow.
/// This session is temporary and deleted after the authorization code is issued.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthorizeSession {
    /// Unique session identifier (stored in cookie).
    pub id: Uuid,

    /// User ID after successful authentication.
    /// None until user authenticates via login form.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,

    /// Original authorization request parameters.
    /// Contains client_id, redirect_uri, scope, state, PKCE params, etc.
    pub authorization_request: AuthorizationRequest,

    /// Timestamp when the session was created.
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,

    /// Timestamp when the session expires.
    #[serde(with = "time::serde::rfc3339")]
    pub expires_at: OffsetDateTime,
}

impl AuthorizeSession {
    /// Creates a new authorize session from an authorization request.
    ///
    /// The session is created with a new UUID and default expiry.
    #[must_use]
    pub fn new(authorization_request: AuthorizationRequest) -> Self {
        let now = OffsetDateTime::now_utc();
        Self {
            id: Uuid::new_v4(),
            user_id: None,
            authorization_request,
            created_at: now,
            expires_at: now + time::Duration::seconds(DEFAULT_SESSION_EXPIRY_SECS),
        }
    }

    /// Creates a new authorize session with custom expiry.
    #[must_use]
    pub fn with_expiry(authorization_request: AuthorizationRequest, expiry_secs: i64) -> Self {
        let now = OffsetDateTime::now_utc();
        Self {
            id: Uuid::new_v4(),
            user_id: None,
            authorization_request,
            created_at: now,
            expires_at: now + time::Duration::seconds(expiry_secs),
        }
    }

    /// Checks if the session has expired.
    #[must_use]
    pub fn is_expired(&self) -> bool {
        OffsetDateTime::now_utc() > self.expires_at
    }

    /// Checks if the user has authenticated.
    #[must_use]
    pub fn is_authenticated(&self) -> bool {
        self.user_id.is_some()
    }

    /// Returns the client ID from the authorization request.
    #[must_use]
    pub fn client_id(&self) -> &str {
        &self.authorization_request.client_id
    }

    /// Returns the requested scopes as a slice of strings.
    #[must_use]
    pub fn scopes(&self) -> Vec<&str> {
        self.authorization_request
            .scope
            .split_whitespace()
            .collect()
    }

    /// Returns the redirect URI from the authorization request.
    #[must_use]
    pub fn redirect_uri(&self) -> &str {
        &self.authorization_request.redirect_uri
    }

    /// Returns the state parameter from the authorization request.
    #[must_use]
    pub fn state(&self) -> &str {
        &self.authorization_request.state
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_request() -> AuthorizationRequest {
        AuthorizationRequest {
            response_type: "code".to_string(),
            client_id: "test-client".to_string(),
            redirect_uri: "https://example.com/callback".to_string(),
            scope: "openid patient/*.read".to_string(),
            state: "test-state-123".to_string(),
            code_challenge: Some("E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM".to_string()),
            code_challenge_method: Some("S256".to_string()),
            aud: "https://fhir.example.com".to_string(),
            launch: None,
            nonce: None,
        }
    }

    #[test]
    fn test_new_session() {
        let request = create_test_request();
        let session = AuthorizeSession::new(request);

        assert!(!session.is_expired());
        assert!(!session.is_authenticated());
        assert!(session.user_id.is_none());
        assert_eq!(session.client_id(), "test-client");
        assert_eq!(session.redirect_uri(), "https://example.com/callback");
        assert_eq!(session.state(), "test-state-123");
    }

    #[test]
    fn test_scopes() {
        let request = create_test_request();
        let session = AuthorizeSession::new(request);

        let scopes = session.scopes();
        assert_eq!(scopes.len(), 2);
        assert!(scopes.contains(&"openid"));
        assert!(scopes.contains(&"patient/*.read"));
    }

    #[test]
    fn test_expired_session() {
        let request = create_test_request();
        let mut session = AuthorizeSession::new(request);

        // Set expires_at to the past
        session.expires_at = OffsetDateTime::now_utc() - time::Duration::seconds(1);

        assert!(session.is_expired());
    }

    #[test]
    fn test_authenticated_session() {
        let request = create_test_request();
        let mut session = AuthorizeSession::new(request);

        assert!(!session.is_authenticated());

        session.user_id = Some(Uuid::new_v4().to_string());

        assert!(session.is_authenticated());
    }

    #[test]
    fn test_serialization() {
        let request = create_test_request();
        let session = AuthorizeSession::new(request);

        let json = serde_json::to_string(&session).unwrap();
        let deserialized: AuthorizeSession = serde_json::from_str(&json).unwrap();

        assert_eq!(session.id, deserialized.id);
        assert_eq!(session.client_id(), deserialized.client_id());
    }
}
