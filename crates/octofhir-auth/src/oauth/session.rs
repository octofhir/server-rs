//! Authorization session management.
//!
//! This module provides types for managing authorization sessions during
//! the OAuth 2.0 authorization code flow. Sessions track the state of an
//! authorization request from initial creation through code exchange.
//!
//! # Lifecycle
//!
//! 1. Session created when authorization request is validated
//! 2. User authenticates and session is updated with user ID
//! 3. Authorization code is issued to client
//! 4. Client exchanges code for tokens (session consumed)
//! 5. Session cleaned up after expiration
//!
//! # Security
//!
//! - Authorization codes are cryptographically random (256 bits)
//! - Sessions expire after a short time (default 10 minutes)
//! - Codes are single-use (consumed on exchange)
//! - PKCE challenge is stored for verification at token exchange

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

/// Authorization session stored in the database.
///
/// Represents the state of an authorization request from creation
/// through code exchange. The session stores all information needed
/// to validate the token request and issue tokens.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthorizationSession {
    /// Unique session identifier.
    pub id: Uuid,

    /// Authorization code (one-time use).
    /// 256-bit random value, base64url-encoded.
    pub code: String,

    /// Client identifier that initiated the request.
    pub client_id: String,

    /// Redirect URI from the authorization request.
    /// Must match the redirect_uri in the token request.
    pub redirect_uri: String,

    /// Granted scopes (space-separated).
    pub scope: String,

    /// State parameter from the authorization request.
    /// Stored for audit/debugging purposes.
    pub state: String,

    /// PKCE code challenge from the authorization request.
    pub code_challenge: String,

    /// PKCE challenge method (always "S256").
    pub code_challenge_method: String,

    /// User ID after successful authentication.
    /// None until user authenticates.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<Uuid>,

    /// SMART on FHIR launch context.
    /// Contains patient, encounter, and other context from EHR launch.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub launch_context: Option<LaunchContext>,

    /// OpenID Connect nonce for ID token binding.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nonce: Option<String>,

    /// Audience (FHIR server base URL).
    pub aud: String,

    /// Timestamp when the session was created.
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,

    /// Timestamp when the session expires.
    #[serde(with = "time::serde::rfc3339")]
    pub expires_at: OffsetDateTime,

    /// Timestamp when the code was exchanged (consumed).
    /// None until the code is used.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "time::serde::rfc3339::option"
    )]
    pub consumed_at: Option<OffsetDateTime>,
}

impl AuthorizationSession {
    /// Generates a new cryptographically secure authorization code.
    ///
    /// The code is 256 bits (32 bytes) of random data, encoded as
    /// base64url without padding (43 characters).
    ///
    /// # Security
    ///
    /// Uses the system's cryptographically secure random number generator.
    /// The resulting code has 256 bits of entropy, exceeding the OAuth 2.0
    /// recommendation of at least 128 bits.
    #[must_use]
    pub fn generate_code() -> String {
        let mut bytes = [0u8; 32];
        rand::Rng::fill(&mut rand::thread_rng(), &mut bytes);
        URL_SAFE_NO_PAD.encode(bytes)
    }

    /// Returns `true` if the session has expired.
    ///
    /// Expired sessions should not be used for code exchange.
    #[must_use]
    pub fn is_expired(&self) -> bool {
        OffsetDateTime::now_utc() > self.expires_at
    }

    /// Returns `true` if the authorization code has been consumed.
    ///
    /// Consumed codes cannot be used again (single-use requirement).
    #[must_use]
    pub fn is_consumed(&self) -> bool {
        self.consumed_at.is_some()
    }

    /// Returns `true` if the session is valid for code exchange.
    ///
    /// A session is valid if it is not expired and not consumed.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        !self.is_expired() && !self.is_consumed()
    }

    /// Returns `true` if the user has authenticated.
    #[must_use]
    pub fn is_authenticated(&self) -> bool {
        self.user_id.is_some()
    }
}

/// SMART on FHIR launch context.
///
/// Contains context information from an EHR launch, including
/// the current patient, encounter, and other FHIR resources.
///
/// # SMART App Launch Framework
///
/// When an app is launched from an EHR, the launch context provides
/// information about the clinical context (patient, encounter, etc.)
/// that the app should use.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LaunchContext {
    /// Current patient ID (FHIR resource ID).
    /// Included in token response as `patient` claim.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub patient: Option<String>,

    /// Current encounter ID (FHIR resource ID).
    /// Included in token response as `encounter` claim.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encounter: Option<String>,

    /// Additional FHIR context items.
    /// Included in token response as `fhirContext` claim (SMART v2).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fhir_context: Vec<FhirContextItem>,

    /// Whether to display patient banner in the app.
    /// When true, app should show patient demographics.
    #[serde(default)]
    pub need_patient_banner: bool,

    /// URL to SMART styling information.
    /// Apps can use this to match the EHR's look and feel.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub smart_style_url: Option<String>,

    /// Launch intent describing why the app was launched.
    /// Examples: "reconcile-medications", "review-results"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intent: Option<String>,
}

impl LaunchContext {
    /// Creates a new empty launch context.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a launch context with a patient.
    #[must_use]
    pub fn with_patient(patient_id: impl Into<String>) -> Self {
        Self {
            patient: Some(patient_id.into()),
            ..Self::default()
        }
    }

    /// Returns `true` if this is an empty launch context.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.patient.is_none()
            && self.encounter.is_none()
            && self.fhir_context.is_empty()
            && self.smart_style_url.is_none()
            && self.intent.is_none()
    }
}

/// A FHIR context item in the launch context.
///
/// Represents a FHIR resource reference with an optional role
/// describing how the resource relates to the launch context.
///
/// # SMART v2 fhirContext
///
/// This corresponds to items in the `fhirContext` array in SMART v2
/// token responses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FhirContextItem {
    /// FHIR resource reference (e.g., "Patient/123", "Encounter/456").
    pub reference: String,

    /// Optional role describing the resource's relationship.
    /// Examples: "current-patient", "current-encounter"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
}

impl FhirContextItem {
    /// Creates a new FHIR context item.
    #[must_use]
    pub fn new(reference: impl Into<String>) -> Self {
        Self {
            reference: reference.into(),
            role: None,
        }
    }

    /// Creates a new FHIR context item with a role.
    #[must_use]
    pub fn with_role(reference: impl Into<String>, role: impl Into<String>) -> Self {
        Self {
            reference: reference.into(),
            role: Some(role.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::Duration;

    #[test]
    fn test_generate_code_length() {
        let code = AuthorizationSession::generate_code();
        // 32 bytes = 256 bits, base64url encoded = 43 characters (no padding)
        assert_eq!(code.len(), 43);
    }

    #[test]
    fn test_generate_code_is_base64url() {
        let code = AuthorizationSession::generate_code();
        // Should only contain URL-safe base64 characters
        assert!(
            code.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        );
    }

    #[test]
    fn test_generate_code_uniqueness() {
        let codes: Vec<String> = (0..100)
            .map(|_| AuthorizationSession::generate_code())
            .collect();

        // All codes should be unique
        let mut unique_codes = codes.clone();
        unique_codes.sort();
        unique_codes.dedup();
        assert_eq!(codes.len(), unique_codes.len());
    }

    #[test]
    fn test_session_is_expired() {
        let now = OffsetDateTime::now_utc();

        // Not expired
        let session = create_test_session(now + Duration::minutes(10), None);
        assert!(!session.is_expired());

        // Expired
        let session = create_test_session(now - Duration::minutes(1), None);
        assert!(session.is_expired());
    }

    #[test]
    fn test_session_is_consumed() {
        let now = OffsetDateTime::now_utc();

        // Not consumed
        let session = create_test_session(now + Duration::minutes(10), None);
        assert!(!session.is_consumed());

        // Consumed
        let session = create_test_session(now + Duration::minutes(10), Some(now));
        assert!(session.is_consumed());
    }

    #[test]
    fn test_session_is_valid() {
        let now = OffsetDateTime::now_utc();

        // Valid: not expired, not consumed
        let session = create_test_session(now + Duration::minutes(10), None);
        assert!(session.is_valid());

        // Invalid: expired
        let session = create_test_session(now - Duration::minutes(1), None);
        assert!(!session.is_valid());

        // Invalid: consumed
        let session = create_test_session(now + Duration::minutes(10), Some(now));
        assert!(!session.is_valid());

        // Invalid: both expired and consumed
        let session = create_test_session(now - Duration::minutes(1), Some(now));
        assert!(!session.is_valid());
    }

    #[test]
    fn test_session_is_authenticated() {
        let now = OffsetDateTime::now_utc();

        let mut session = create_test_session(now + Duration::minutes(10), None);
        assert!(!session.is_authenticated());

        session.user_id = Some(Uuid::new_v4());
        assert!(session.is_authenticated());
    }

    #[test]
    fn test_launch_context_is_empty() {
        let ctx = LaunchContext::new();
        assert!(ctx.is_empty());

        let ctx = LaunchContext::with_patient("Patient/123");
        assert!(!ctx.is_empty());

        let mut ctx = LaunchContext::new();
        ctx.encounter = Some("Encounter/456".to_string());
        assert!(!ctx.is_empty());

        let mut ctx = LaunchContext::new();
        ctx.fhir_context.push(FhirContextItem::new("Patient/123"));
        assert!(!ctx.is_empty());
    }

    #[test]
    fn test_fhir_context_item() {
        let item = FhirContextItem::new("Patient/123");
        assert_eq!(item.reference, "Patient/123");
        assert!(item.role.is_none());

        let item = FhirContextItem::with_role("Encounter/456", "current-encounter");
        assert_eq!(item.reference, "Encounter/456");
        assert_eq!(item.role, Some("current-encounter".to_string()));
    }

    #[test]
    fn test_session_serialization() {
        let now = OffsetDateTime::now_utc();
        let session = create_test_session(now + Duration::minutes(10), None);

        let json = serde_json::to_string(&session).unwrap();
        let deserialized: AuthorizationSession = serde_json::from_str(&json).unwrap();

        assert_eq!(session.id, deserialized.id);
        assert_eq!(session.code, deserialized.code);
        assert_eq!(session.client_id, deserialized.client_id);
        assert_eq!(session.scope, deserialized.scope);
    }

    #[test]
    fn test_launch_context_serialization() {
        let mut ctx = LaunchContext::with_patient("Patient/123");
        ctx.encounter = Some("Encounter/456".to_string());
        ctx.need_patient_banner = true;
        ctx.fhir_context.push(FhirContextItem::with_role(
            "Observation/789",
            "launch-context",
        ));

        let json = serde_json::to_string(&ctx).unwrap();
        let deserialized: LaunchContext = serde_json::from_str(&json).unwrap();

        assert_eq!(ctx.patient, deserialized.patient);
        assert_eq!(ctx.encounter, deserialized.encounter);
        assert_eq!(ctx.need_patient_banner, deserialized.need_patient_banner);
        assert_eq!(ctx.fhir_context.len(), deserialized.fhir_context.len());
    }

    /// Helper function to create a test session.
    fn create_test_session(
        expires_at: OffsetDateTime,
        consumed_at: Option<OffsetDateTime>,
    ) -> AuthorizationSession {
        let now = OffsetDateTime::now_utc();
        AuthorizationSession {
            id: Uuid::new_v4(),
            code: AuthorizationSession::generate_code(),
            client_id: "test-client".to_string(),
            redirect_uri: "https://app.example.com/callback".to_string(),
            scope: "openid patient/*.read".to_string(),
            state: "test-state".to_string(),
            code_challenge: "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM".to_string(),
            code_challenge_method: "S256".to_string(),
            user_id: None,
            launch_context: None,
            nonce: None,
            aud: "https://fhir.example.com/r4".to_string(),
            created_at: now,
            expires_at,
            consumed_at,
        }
    }
}
