//! PKCE (Proof Key for Code Exchange) implementation
//!
//! Implements RFC 7636 with S256 method only.
//! The "plain" method is explicitly forbidden per SMART on FHIR.
//!
//! # Example
//!
//! ```
//! use octofhir_auth::oauth::{PkceVerifier, PkceChallenge, PkceChallengeMethod};
//!
//! // Client generates a verifier and challenge
//! let verifier = PkceVerifier::generate();
//! let challenge = PkceChallenge::from_verifier(&verifier);
//!
//! // Client sends challenge in authorization request
//! let challenge_str = challenge.as_str();
//! let method = PkceChallengeMethod::S256;
//!
//! // Server stores challenge, later verifies with verifier from token request
//! let stored_challenge = PkceChallenge::new(challenge_str.to_string()).unwrap();
//! assert!(stored_challenge.verify(&verifier).is_ok());
//! ```

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};

// =============================================================================
// Error Types
// =============================================================================

/// Errors that can occur during PKCE operations.
#[derive(Debug, thiserror::Error)]
pub enum PkceError {
    /// Verifier length is outside the valid range (43-128 characters).
    #[error("Invalid verifier length: must be 43-128 characters, got {0}")]
    InvalidVerifierLength(usize),

    /// Verifier contains invalid characters.
    #[error("Invalid verifier characters: must be URL-safe base64 ([A-Za-z0-9-._~])")]
    InvalidVerifierCharacters,

    /// Challenge format is invalid.
    #[error("Invalid challenge format: must be valid base64url")]
    InvalidChallengeFormat,

    /// Unsupported challenge method (only S256 is supported).
    #[error("Unsupported challenge method: {0}. Only S256 is supported.")]
    UnsupportedMethod(String),

    /// PKCE verification failed (verifier doesn't match challenge).
    #[error("PKCE verification failed: verifier does not match challenge")]
    VerificationFailed,
}

impl PkceError {
    // -------------------------------------------------------------------------
    // Constructor Methods
    // -------------------------------------------------------------------------

    /// Create an `InvalidVerifierLength` error.
    #[must_use]
    pub fn invalid_verifier_length(len: usize) -> Self {
        Self::InvalidVerifierLength(len)
    }

    /// Create an `InvalidVerifierCharacters` error.
    #[must_use]
    pub fn invalid_verifier_characters() -> Self {
        Self::InvalidVerifierCharacters
    }

    /// Create an `InvalidChallengeFormat` error.
    #[must_use]
    pub fn invalid_challenge_format() -> Self {
        Self::InvalidChallengeFormat
    }

    /// Create an `UnsupportedMethod` error.
    #[must_use]
    pub fn unsupported_method(method: impl Into<String>) -> Self {
        Self::UnsupportedMethod(method.into())
    }

    /// Create a `VerificationFailed` error.
    #[must_use]
    pub fn verification_failed() -> Self {
        Self::VerificationFailed
    }

    // -------------------------------------------------------------------------
    // Predicate Methods
    // -------------------------------------------------------------------------

    /// Returns `true` if this is a verifier validation error.
    #[must_use]
    pub fn is_verifier_error(&self) -> bool {
        matches!(
            self,
            Self::InvalidVerifierLength(_) | Self::InvalidVerifierCharacters
        )
    }

    /// Returns `true` if this is a challenge validation error.
    #[must_use]
    pub fn is_challenge_error(&self) -> bool {
        matches!(
            self,
            Self::InvalidChallengeFormat | Self::UnsupportedMethod(_)
        )
    }

    /// Returns `true` if this is a verification failure.
    #[must_use]
    pub fn is_verification_error(&self) -> bool {
        matches!(self, Self::VerificationFailed)
    }

    /// Get the OAuth 2.0 error code for this error.
    #[must_use]
    pub fn oauth_error_code(&self) -> &'static str {
        match self {
            Self::InvalidVerifierLength(_)
            | Self::InvalidVerifierCharacters
            | Self::InvalidChallengeFormat
            | Self::UnsupportedMethod(_) => "invalid_request",
            Self::VerificationFailed => "invalid_grant",
        }
    }
}

// =============================================================================
// PKCE Challenge Method
// =============================================================================

/// PKCE challenge method.
///
/// Only S256 (SHA-256) is supported. The "plain" method is explicitly
/// forbidden per SMART on FHIR requirements.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PkceChallengeMethod {
    /// SHA-256 hash (the only supported method).
    S256,
}

impl PkceChallengeMethod {
    /// Parse challenge method from string.
    ///
    /// # Errors
    ///
    /// Returns `PkceError::UnsupportedMethod` if the method is not "S256".
    /// The "plain" method is explicitly forbidden per SMART on FHIR.
    pub fn parse(method: &str) -> Result<Self, PkceError> {
        match method {
            "S256" => Ok(Self::S256),
            "plain" => Err(PkceError::unsupported_method(
                "plain (forbidden by SMART on FHIR)",
            )),
            other => Err(PkceError::unsupported_method(other)),
        }
    }

    /// Get the method as a string.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::S256 => "S256",
        }
    }
}

impl std::fmt::Display for PkceChallengeMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl Default for PkceChallengeMethod {
    fn default() -> Self {
        Self::S256
    }
}

// =============================================================================
// PKCE Verifier
// =============================================================================

/// PKCE code verifier.
///
/// A high-entropy cryptographic random string using the unreserved characters
/// `[A-Z] / [a-z] / [0-9] / "-" / "." / "_" / "~"`, with a minimum length of
/// 43 characters and a maximum length of 128 characters.
///
/// # RFC 7636 Specification
///
/// From Section 4.1:
/// > code_verifier = high-entropy cryptographic random STRING using the
/// > unreserved characters [A-Z] / [a-z] / [0-9] / "-" / "." / "_" / "~"
/// > from Section 2.3 of [RFC3986], with a minimum length of 43 characters
/// > and a maximum length of 128 characters.
#[derive(Debug, Clone)]
pub struct PkceVerifier(String);

impl PkceVerifier {
    /// Create a new verifier from a string.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Length is not between 43 and 128 characters
    /// - Contains characters other than `[A-Za-z0-9-._~]`
    pub fn new(verifier: String) -> Result<Self, PkceError> {
        let len = verifier.len();

        // RFC 7636: verifier must be 43-128 characters
        if !(43..=128).contains(&len) {
            return Err(PkceError::invalid_verifier_length(len));
        }

        // Must be URL-safe unreserved characters: [A-Z], [a-z], [0-9], '-', '.', '_', '~'
        if !verifier
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.' || c == '_' || c == '~')
        {
            return Err(PkceError::invalid_verifier_characters());
        }

        Ok(Self(verifier))
    }

    /// Generate a cryptographically random verifier.
    ///
    /// Generates 32 random bytes and encodes them as base64url (43 characters).
    #[must_use]
    pub fn generate() -> Self {
        use rand::Rng;
        let mut rng = rand::thread_rng();
        // `gen` is a reserved keyword in Rust 2024, so we use r#gen
        let bytes: [u8; 32] = rng.r#gen();
        let verifier = URL_SAFE_NO_PAD.encode(bytes);
        Self(verifier)
    }

    /// Get the verifier as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consume the verifier and return the inner string.
    #[must_use]
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl AsRef<str> for PkceVerifier {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

// =============================================================================
// PKCE Challenge
// =============================================================================

/// PKCE code challenge.
///
/// The S256 challenge is the base64url-encoded SHA-256 hash of the verifier.
///
/// # RFC 7636 Specification
///
/// From Section 4.2:
/// > S256
/// >    code_challenge = BASE64URL(SHA256(ASCII(code_verifier)))
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PkceChallenge(String);

impl PkceChallenge {
    /// Create a challenge from a verifier using the S256 method.
    ///
    /// Computes `BASE64URL(SHA256(ASCII(code_verifier)))`.
    #[must_use]
    pub fn from_verifier(verifier: &PkceVerifier) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(verifier.0.as_bytes());
        let hash = hasher.finalize();
        Self(URL_SAFE_NO_PAD.encode(hash))
    }

    /// Create a challenge from a raw string (received from client).
    ///
    /// # Errors
    ///
    /// Returns `PkceError::InvalidChallengeFormat` if the string is not valid base64url.
    pub fn new(challenge: String) -> Result<Self, PkceError> {
        // Validate it's valid base64url
        if URL_SAFE_NO_PAD.decode(&challenge).is_err() {
            return Err(PkceError::invalid_challenge_format());
        }
        Ok(Self(challenge))
    }

    /// Verify that a verifier matches this challenge.
    ///
    /// Computes the S256 hash of the verifier and compares it to this challenge.
    ///
    /// # Errors
    ///
    /// Returns `PkceError::VerificationFailed` if the verifier doesn't match.
    pub fn verify(&self, verifier: &PkceVerifier) -> Result<(), PkceError> {
        let expected = Self::from_verifier(verifier);
        if self.0 == expected.0 {
            Ok(())
        } else {
            Err(PkceError::verification_failed())
        }
    }

    /// Get the challenge as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consume the challenge and return the inner string.
    #[must_use]
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl AsRef<str> for PkceChallenge {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -------------------------------------------------------------------------
    // Verifier Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_verifier_generation() {
        let verifier = PkceVerifier::generate();
        let len = verifier.as_str().len();
        assert!(
            (43..=128).contains(&len),
            "Generated verifier length {} should be 43-128",
            len
        );

        // Verify all characters are valid
        assert!(
            verifier
                .as_str()
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
            "Generated verifier should only contain base64url characters"
        );
    }

    #[test]
    fn test_verifier_generation_uniqueness() {
        // Generate multiple verifiers and ensure they're unique
        let v1 = PkceVerifier::generate();
        let v2 = PkceVerifier::generate();
        let v3 = PkceVerifier::generate();

        assert_ne!(v1.as_str(), v2.as_str());
        assert_ne!(v2.as_str(), v3.as_str());
        assert_ne!(v1.as_str(), v3.as_str());
    }

    #[test]
    fn test_verifier_validation_length_too_short() {
        let short = "a".repeat(42);
        let result = PkceVerifier::new(short);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            PkceError::InvalidVerifierLength(42)
        ));
    }

    #[test]
    fn test_verifier_validation_length_minimum() {
        let min = "a".repeat(43);
        assert!(PkceVerifier::new(min).is_ok());
    }

    #[test]
    fn test_verifier_validation_length_maximum() {
        let max = "a".repeat(128);
        assert!(PkceVerifier::new(max).is_ok());
    }

    #[test]
    fn test_verifier_validation_length_too_long() {
        let long = "a".repeat(129);
        let result = PkceVerifier::new(long);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            PkceError::InvalidVerifierLength(129)
        ));
    }

    #[test]
    fn test_verifier_validation_characters_valid() {
        // All valid unreserved characters from RFC 3986
        let valid = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-._~"
            .chars()
            .cycle()
            .take(64)
            .collect::<String>();
        assert!(PkceVerifier::new(valid).is_ok());
    }

    #[test]
    fn test_verifier_validation_characters_invalid() {
        // Contains invalid characters (!, @, #, etc.)
        let invalid = "abcdefghijklmnopqrstuvwxyz0123456789!@#$%^&*()".to_string();
        let result = PkceVerifier::new(invalid);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            PkceError::InvalidVerifierCharacters
        ));
    }

    #[test]
    fn test_verifier_into_inner() {
        let verifier = PkceVerifier::generate();
        let original = verifier.as_str().to_string();
        let inner = verifier.into_inner();
        assert_eq!(original, inner);
    }

    // -------------------------------------------------------------------------
    // Challenge Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_challenge_from_verifier() {
        let verifier = PkceVerifier::generate();
        let challenge = PkceChallenge::from_verifier(&verifier);

        // SHA-256 produces 32 bytes, base64url encoded = 43 characters
        assert_eq!(
            challenge.as_str().len(),
            43,
            "S256 challenge should be 43 characters"
        );
    }

    #[test]
    fn test_challenge_verification_success() {
        let verifier = PkceVerifier::generate();
        let challenge = PkceChallenge::from_verifier(&verifier);

        assert!(challenge.verify(&verifier).is_ok());
    }

    #[test]
    fn test_challenge_verification_failure() {
        let verifier1 = PkceVerifier::generate();
        let verifier2 = PkceVerifier::generate();
        let challenge = PkceChallenge::from_verifier(&verifier1);

        let result = challenge.verify(&verifier2);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), PkceError::VerificationFailed));
    }

    #[test]
    fn test_challenge_new_valid() {
        // Valid base64url string
        let valid = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";
        assert!(PkceChallenge::new(valid.to_string()).is_ok());
    }

    #[test]
    fn test_challenge_new_invalid() {
        // Invalid base64url (contains invalid characters)
        let invalid = "not valid base64url!!!";
        let result = PkceChallenge::new(invalid.to_string());
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            PkceError::InvalidChallengeFormat
        ));
    }

    // -------------------------------------------------------------------------
    // Challenge Method Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_challenge_method_s256() {
        let method = PkceChallengeMethod::parse("S256");
        assert!(method.is_ok());
        assert_eq!(method.unwrap(), PkceChallengeMethod::S256);
    }

    #[test]
    fn test_challenge_method_plain_rejected() {
        let result = PkceChallengeMethod::parse("plain");
        assert!(result.is_err());

        let err = result.unwrap_err();
        assert!(matches!(err, PkceError::UnsupportedMethod(_)));
        assert!(err.to_string().contains("plain"));
        assert!(err.to_string().contains("SMART on FHIR"));
    }

    #[test]
    fn test_challenge_method_unknown_rejected() {
        let result = PkceChallengeMethod::parse("unknown");
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            PkceError::UnsupportedMethod(_)
        ));
    }

    #[test]
    fn test_challenge_method_as_str() {
        assert_eq!(PkceChallengeMethod::S256.as_str(), "S256");
    }

    #[test]
    fn test_challenge_method_display() {
        assert_eq!(format!("{}", PkceChallengeMethod::S256), "S256");
    }

    #[test]
    fn test_challenge_method_default() {
        assert_eq!(PkceChallengeMethod::default(), PkceChallengeMethod::S256);
    }

    // -------------------------------------------------------------------------
    // RFC 7636 Test Vector
    // -------------------------------------------------------------------------

    #[test]
    fn test_rfc7636_appendix_b_test_vector() {
        // Test vector from RFC 7636 Appendix B
        // https://tools.ietf.org/html/rfc7636#appendix-B
        let verifier =
            PkceVerifier::new("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk".to_string()).unwrap();

        let challenge = PkceChallenge::from_verifier(&verifier);

        assert_eq!(
            challenge.as_str(),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM",
            "S256 challenge should match RFC 7636 Appendix B test vector"
        );

        // Also verify the reverse - verification should pass
        let stored_challenge =
            PkceChallenge::new("E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM".to_string()).unwrap();
        assert!(stored_challenge.verify(&verifier).is_ok());
    }

    // -------------------------------------------------------------------------
    // Error Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_error_predicates() {
        let verifier_len_err = PkceError::invalid_verifier_length(10);
        let verifier_char_err = PkceError::invalid_verifier_characters();
        let challenge_fmt_err = PkceError::invalid_challenge_format();
        let method_err = PkceError::unsupported_method("plain");
        let verify_err = PkceError::verification_failed();

        assert!(verifier_len_err.is_verifier_error());
        assert!(verifier_char_err.is_verifier_error());
        assert!(!verifier_len_err.is_challenge_error());

        assert!(challenge_fmt_err.is_challenge_error());
        assert!(method_err.is_challenge_error());
        assert!(!challenge_fmt_err.is_verifier_error());

        assert!(verify_err.is_verification_error());
        assert!(!verify_err.is_verifier_error());
        assert!(!verify_err.is_challenge_error());
    }

    #[test]
    fn test_error_oauth_codes() {
        assert_eq!(
            PkceError::invalid_verifier_length(10).oauth_error_code(),
            "invalid_request"
        );
        assert_eq!(
            PkceError::invalid_verifier_characters().oauth_error_code(),
            "invalid_request"
        );
        assert_eq!(
            PkceError::invalid_challenge_format().oauth_error_code(),
            "invalid_request"
        );
        assert_eq!(
            PkceError::unsupported_method("plain").oauth_error_code(),
            "invalid_request"
        );
        assert_eq!(
            PkceError::verification_failed().oauth_error_code(),
            "invalid_grant"
        );
    }
}
