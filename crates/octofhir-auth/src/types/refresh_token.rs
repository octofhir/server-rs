//! Refresh token domain type.
//!
//! This module defines the refresh token structure used for persisting
//! and managing OAuth 2.0 refresh tokens.
//!
//! # Security
//!
//! - Refresh tokens are stored as SHA-256 hashes, never plaintext
//! - Tokens can be revoked individually or by client/user
//! - Expired tokens are cleaned up periodically

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::oauth::session::LaunchContext;

/// Refresh token stored in the database.
///
/// Refresh tokens allow clients to obtain new access tokens without
/// requiring user re-authentication. They are long-lived and must
/// be stored securely.
///
/// # Storage Security
///
/// The token itself is never stored. Only a SHA-256 hash is persisted,
/// similar to password storage. When validating a refresh token:
///
/// 1. Hash the incoming token
/// 2. Look up by hash
/// 3. Validate expiration and revocation status
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshToken {
    /// Unique identifier for this refresh token record.
    pub id: Uuid,

    /// SHA-256 hash of the actual token value.
    /// The plaintext token is returned to the client but never stored.
    pub token_hash: String,

    /// Client ID that this token was issued to.
    pub client_id: String,

    /// User ID that authorized this token (None for client credentials).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<Uuid>,

    /// Granted scopes (space-separated).
    pub scope: String,

    /// SMART on FHIR launch context.
    /// Preserved from the original authorization for subsequent token refreshes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub launch_context: Option<LaunchContext>,

    /// When this token was created.
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,

    /// When this token expires (None = no expiration).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "time::serde::rfc3339::option"
    )]
    pub expires_at: Option<OffsetDateTime>,

    /// When this token was revoked (None = not revoked).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "time::serde::rfc3339::option"
    )]
    pub revoked_at: Option<OffsetDateTime>,
}

impl RefreshToken {
    /// Returns `true` if this token has expired.
    #[must_use]
    pub fn is_expired(&self) -> bool {
        self.expires_at
            .map(|exp| OffsetDateTime::now_utc() > exp)
            .unwrap_or(false)
    }

    /// Returns `true` if this token has been revoked.
    #[must_use]
    pub fn is_revoked(&self) -> bool {
        self.revoked_at.is_some()
    }

    /// Returns `true` if this token is valid (not expired and not revoked).
    #[must_use]
    pub fn is_valid(&self) -> bool {
        !self.is_expired() && !self.is_revoked()
    }

    /// Hash a token value using SHA-256.
    ///
    /// This is used both when storing new tokens and when looking up
    /// tokens for validation.
    #[must_use]
    pub fn hash_token(token: &str) -> String {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(token.as_bytes());
        hex::encode(hasher.finalize())
    }

    /// Generate a cryptographically secure random token.
    ///
    /// Returns a 256-bit random value encoded as base64url (43 characters).
    #[must_use]
    pub fn generate_token() -> String {
        use base64::Engine;
        use base64::engine::general_purpose::URL_SAFE_NO_PAD;

        let mut bytes = [0u8; 32];
        rand::Rng::fill(&mut rand::thread_rng(), &mut bytes);
        URL_SAFE_NO_PAD.encode(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::Duration;

    #[test]
    fn test_hash_token() {
        let token = "test-token-value";
        let hash = RefreshToken::hash_token(token);

        // SHA-256 produces 64 hex characters
        assert_eq!(hash.len(), 64);

        // Same input produces same hash
        assert_eq!(hash, RefreshToken::hash_token(token));

        // Different input produces different hash
        assert_ne!(hash, RefreshToken::hash_token("different-token"));
    }

    #[test]
    fn test_generate_token() {
        let token = RefreshToken::generate_token();

        // 32 bytes base64url encoded = 43 characters
        assert_eq!(token.len(), 43);

        // Should be URL-safe base64
        assert!(
            token
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        );
    }

    #[test]
    fn test_generate_token_uniqueness() {
        let tokens: Vec<String> = (0..100).map(|_| RefreshToken::generate_token()).collect();

        let mut unique = tokens.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(tokens.len(), unique.len());
    }

    #[test]
    fn test_is_expired() {
        let now = OffsetDateTime::now_utc();

        // Not expired (no expiration)
        let token = create_test_token(None, None);
        assert!(!token.is_expired());

        // Not expired (future expiration)
        let token = create_test_token(Some(now + Duration::hours(1)), None);
        assert!(!token.is_expired());

        // Expired
        let token = create_test_token(Some(now - Duration::minutes(1)), None);
        assert!(token.is_expired());
    }

    #[test]
    fn test_is_revoked() {
        let now = OffsetDateTime::now_utc();

        // Not revoked
        let token = create_test_token(None, None);
        assert!(!token.is_revoked());

        // Revoked
        let token = create_test_token(None, Some(now));
        assert!(token.is_revoked());
    }

    #[test]
    fn test_is_valid() {
        let now = OffsetDateTime::now_utc();

        // Valid (not expired, not revoked)
        let token = create_test_token(Some(now + Duration::hours(1)), None);
        assert!(token.is_valid());

        // Invalid (expired)
        let token = create_test_token(Some(now - Duration::minutes(1)), None);
        assert!(!token.is_valid());

        // Invalid (revoked)
        let token = create_test_token(Some(now + Duration::hours(1)), Some(now));
        assert!(!token.is_valid());
    }

    #[test]
    fn test_serialization() {
        let now = OffsetDateTime::now_utc();
        let token = create_test_token(Some(now + Duration::hours(1)), None);

        let json = serde_json::to_string(&token).unwrap();
        let deserialized: RefreshToken = serde_json::from_str(&json).unwrap();

        assert_eq!(token.id, deserialized.id);
        assert_eq!(token.token_hash, deserialized.token_hash);
        assert_eq!(token.client_id, deserialized.client_id);
        assert_eq!(token.scope, deserialized.scope);
    }

    fn create_test_token(
        expires_at: Option<OffsetDateTime>,
        revoked_at: Option<OffsetDateTime>,
    ) -> RefreshToken {
        RefreshToken {
            id: Uuid::new_v4(),
            token_hash: RefreshToken::hash_token("test-token"),
            client_id: "test-client".to_string(),
            user_id: Some(Uuid::new_v4()),
            scope: "openid offline_access".to_string(),
            launch_context: None,
            created_at: OffsetDateTime::now_utc(),
            expires_at,
            revoked_at,
        }
    }
}
