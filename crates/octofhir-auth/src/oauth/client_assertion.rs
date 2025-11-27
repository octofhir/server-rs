//! JWT client assertion validation for Backend Services.
//!
//! This module implements validation for JWT client assertions per RFC 7523
//! (JSON Web Token Bearer Assertion) for use with the OAuth 2.0 client
//! credentials flow in SMART Backend Services.
//!
//! # JWT Assertion Requirements (RFC 7523)
//!
//! The client assertion JWT must contain:
//!
//! - `iss` (issuer): Must equal the client_id
//! - `sub` (subject): Must equal the client_id
//! - `aud` (audience): Must contain the token endpoint URL
//! - `exp` (expiration): Must not exceed 5 minutes from now
//! - `jti` (JWT ID): Must be unique to prevent replay attacks
//! - `iat` (issued at): Optional but recommended
//!
//! # Security Considerations
//!
//! - JTI values are tracked to prevent replay attacks
//! - Assertion lifetime is limited to 5 minutes maximum
//! - Only RS384 and ES384 algorithms are recommended for SMART
//! - All assertions must be signed with the client's private key

use std::sync::Arc;

use jsonwebtoken::{Algorithm, DecodingKey, Validation};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::AuthResult;
use crate::error::AuthError;
use crate::storage::JtiStorage;

/// JWT claims for client assertions per RFC 7523.
///
/// These claims are used when a client authenticates to the token endpoint
/// using the `private_key_jwt` method.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientAssertionClaims {
    /// Issuer - must be the client_id.
    pub iss: String,

    /// Subject - must be the client_id.
    pub sub: String,

    /// Audience - must contain the token endpoint URL.
    /// Can be a single string or an array of strings.
    pub aud: StringOrArray,

    /// Expiration time as Unix timestamp.
    /// Must not exceed 5 minutes from the current time.
    pub exp: i64,

    /// JWT ID - must be unique to prevent replay attacks.
    pub jti: String,

    /// Issued at time as Unix timestamp (optional but recommended).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iat: Option<i64>,
}

/// Audience claim can be a single string or an array of strings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum StringOrArray {
    /// Single string audience.
    String(String),
    /// Array of audience strings.
    Array(Vec<String>),
}

impl StringOrArray {
    /// Checks if the audience contains the specified value.
    #[must_use]
    pub fn contains(&self, value: &str) -> bool {
        match self {
            Self::String(s) => s == value,
            Self::Array(arr) => arr.iter().any(|s| s == value),
        }
    }

    /// Returns the first audience value.
    #[must_use]
    pub fn first(&self) -> Option<&str> {
        match self {
            Self::String(s) => Some(s.as_str()),
            Self::Array(arr) => arr.first().map(String::as_str),
        }
    }
}

/// Configuration for client assertion validation.
#[derive(Debug, Clone)]
pub struct ClientAssertionConfig {
    /// Token endpoint URL (for audience validation).
    pub token_endpoint: String,

    /// Maximum assertion lifetime in seconds (default: 300 = 5 minutes).
    pub max_lifetime_seconds: i64,
}

impl ClientAssertionConfig {
    /// Creates a new configuration with the specified token endpoint.
    #[must_use]
    pub fn new(token_endpoint: impl Into<String>) -> Self {
        Self {
            token_endpoint: token_endpoint.into(),
            max_lifetime_seconds: 300, // 5 minutes per spec
        }
    }

    /// Sets the maximum assertion lifetime.
    #[must_use]
    pub fn with_max_lifetime(mut self, seconds: i64) -> Self {
        self.max_lifetime_seconds = seconds;
        self
    }
}

/// Validates JWT client assertions per RFC 7523.
///
/// This validator checks all required claims and prevents replay attacks
/// by tracking used JTI values.
///
/// # Example
///
/// ```ignore
/// use octofhir_auth::oauth::ClientAssertionValidator;
///
/// let config = ClientAssertionConfig::new("https://auth.example.com/token");
/// let validator = ClientAssertionValidator::new(config, jti_storage);
///
/// let claims = validator.validate(
///     &assertion,
///     "client-123",
///     &decoding_key,
///     Algorithm::RS384,
/// ).await?;
/// ```
pub struct ClientAssertionValidator<S: JtiStorage> {
    /// Configuration for validation.
    config: ClientAssertionConfig,

    /// JTI storage for replay prevention.
    jti_storage: Arc<S>,
}

impl<S: JtiStorage> ClientAssertionValidator<S> {
    /// Creates a new validator with the specified configuration.
    pub fn new(config: ClientAssertionConfig, jti_storage: Arc<S>) -> Self {
        Self {
            config,
            jti_storage,
        }
    }

    /// Validates a client assertion JWT.
    ///
    /// # Arguments
    ///
    /// * `assertion` - The JWT assertion string
    /// * `expected_client_id` - The client ID to validate against
    /// * `decoding_key` - The key to verify the signature
    /// * `algorithm` - The expected signing algorithm
    ///
    /// # Returns
    ///
    /// Returns the validated claims on success.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The JWT signature is invalid
    /// - The issuer or subject doesn't match the client ID
    /// - The audience doesn't contain the token endpoint
    /// - The assertion is expired or exceeds maximum lifetime
    /// - The JTI has already been used (replay attack)
    pub async fn validate(
        &self,
        assertion: &str,
        expected_client_id: &str,
        decoding_key: &DecodingKey,
        algorithm: Algorithm,
    ) -> AuthResult<ClientAssertionClaims> {
        // 1. Build validation with audience and issuer checks
        let mut validation = Validation::new(algorithm);
        validation.set_audience(&[&self.config.token_endpoint]);
        validation.set_issuer(&[expected_client_id]);

        // 2. Decode and verify signature
        let token_data =
            jsonwebtoken::decode::<ClientAssertionClaims>(assertion, decoding_key, &validation)
                .map_err(|e| {
                    tracing::debug!("JWT validation failed: {}", e);
                    AuthError::invalid_client(format!("Invalid client assertion: {}", e))
                })?;

        let claims = token_data.claims;

        // 3. Validate iss == sub == client_id
        if claims.iss != expected_client_id {
            return Err(AuthError::invalid_client(
                "Assertion issuer must equal client_id",
            ));
        }
        if claims.sub != expected_client_id {
            return Err(AuthError::invalid_client(
                "Assertion subject must equal client_id",
            ));
        }

        // 4. Validate audience contains token endpoint
        if !claims.aud.contains(&self.config.token_endpoint) {
            return Err(AuthError::invalid_client(
                "Assertion audience must contain token endpoint URL",
            ));
        }

        // 5. Validate expiration (already checked by jsonwebtoken)
        // but we also need to ensure it's not too far in the future
        let now = OffsetDateTime::now_utc().unix_timestamp();
        if claims.exp > now + self.config.max_lifetime_seconds {
            return Err(AuthError::invalid_client(format!(
                "Assertion exp must be within {} seconds",
                self.config.max_lifetime_seconds
            )));
        }

        // 6. Check JTI for replay prevention
        let expires_at = OffsetDateTime::from_unix_timestamp(claims.exp)
            .map_err(|_| AuthError::invalid_client("Invalid exp timestamp"))?;

        let is_new = self.jti_storage.mark_used(&claims.jti, expires_at).await?;
        if !is_new {
            return Err(AuthError::invalid_client(
                "Assertion jti already used (possible replay attack)",
            ));
        }

        Ok(claims)
    }

    /// Validates a client assertion without JTI checking.
    ///
    /// This is useful for testing or cases where JTI tracking is handled
    /// separately.
    ///
    /// # Safety
    ///
    /// This method does not check for replay attacks. Only use it when
    /// JTI checking is handled by another mechanism.
    pub fn validate_without_jti(
        &self,
        assertion: &str,
        expected_client_id: &str,
        decoding_key: &DecodingKey,
        algorithm: Algorithm,
    ) -> AuthResult<ClientAssertionClaims> {
        // Build validation
        let mut validation = Validation::new(algorithm);
        validation.set_audience(&[&self.config.token_endpoint]);
        validation.set_issuer(&[expected_client_id]);

        // Decode and verify signature
        let token_data =
            jsonwebtoken::decode::<ClientAssertionClaims>(assertion, decoding_key, &validation)
                .map_err(|e| {
                    AuthError::invalid_client(format!("Invalid client assertion: {}", e))
                })?;

        let claims = token_data.claims;

        // Validate iss == sub == client_id
        if claims.iss != expected_client_id {
            return Err(AuthError::invalid_client(
                "Assertion issuer must equal client_id",
            ));
        }
        if claims.sub != expected_client_id {
            return Err(AuthError::invalid_client(
                "Assertion subject must equal client_id",
            ));
        }

        // Validate audience
        if !claims.aud.contains(&self.config.token_endpoint) {
            return Err(AuthError::invalid_client(
                "Assertion audience must contain token endpoint URL",
            ));
        }

        // Validate expiration bounds
        let now = OffsetDateTime::now_utc().unix_timestamp();
        if claims.exp > now + self.config.max_lifetime_seconds {
            return Err(AuthError::invalid_client(format!(
                "Assertion exp must be within {} seconds",
                self.config.max_lifetime_seconds
            )));
        }

        Ok(claims)
    }
}

/// Extracts the client ID from an unverified JWT assertion.
///
/// This is used to look up the client and retrieve their public key
/// before validating the assertion signature.
///
/// # Warning
///
/// This does NOT verify the signature. Only use this to determine
/// which key to use for validation.
pub fn extract_client_id_unverified(assertion: &str) -> AuthResult<String> {
    use base64::Engine;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;

    // Split the JWT
    let parts: Vec<&str> = assertion.split('.').collect();
    if parts.len() != 3 {
        return Err(AuthError::invalid_client("Invalid JWT format"));
    }

    // Decode the payload (middle part) without verification
    let payload_bytes = URL_SAFE_NO_PAD
        .decode(parts[1])
        .map_err(|_| AuthError::invalid_client("Invalid JWT payload encoding"))?;

    // Parse as JSON to extract claims
    #[derive(Deserialize)]
    struct MinimalClaims {
        #[serde(default)]
        iss: Option<String>,
        #[serde(default)]
        sub: Option<String>,
    }

    let claims: MinimalClaims = serde_json::from_slice(&payload_bytes)
        .map_err(|_| AuthError::invalid_client("Invalid JWT payload JSON"))?;

    // Prefer `iss` but fall back to `sub`
    claims
        .iss
        .or(claims.sub)
        .ok_or_else(|| AuthError::invalid_client("JWT missing iss and sub claims"))
}

/// Extracts the key ID (kid) from a JWT header.
///
/// This is used to find the correct public key in a JWKS.
pub fn extract_key_id(assertion: &str) -> AuthResult<Option<String>> {
    use base64::Engine;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;

    // Split the JWT
    let parts: Vec<&str> = assertion.split('.').collect();
    if parts.len() != 3 {
        return Err(AuthError::invalid_client("Invalid JWT format"));
    }

    // Decode the header (first part)
    let header_bytes = URL_SAFE_NO_PAD
        .decode(parts[0])
        .map_err(|_| AuthError::invalid_client("Invalid JWT header encoding"))?;

    // Parse as JSON to extract kid
    #[derive(Deserialize)]
    struct JwtHeader {
        #[serde(default)]
        kid: Option<String>,
    }

    let header: JwtHeader = serde_json::from_slice(&header_bytes)
        .map_err(|_| AuthError::invalid_client("Invalid JWT header JSON"))?;

    Ok(header.kid)
}

/// Extracts the algorithm from a JWT header.
pub fn extract_algorithm(assertion: &str) -> AuthResult<Algorithm> {
    use base64::Engine;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;

    // Split the JWT
    let parts: Vec<&str> = assertion.split('.').collect();
    if parts.len() != 3 {
        return Err(AuthError::invalid_client("Invalid JWT format"));
    }

    // Decode the header
    let header_bytes = URL_SAFE_NO_PAD
        .decode(parts[0])
        .map_err(|_| AuthError::invalid_client("Invalid JWT header encoding"))?;

    // Parse as JSON to extract alg
    #[derive(Deserialize)]
    struct JwtHeader {
        alg: String,
    }

    let header: JwtHeader = serde_json::from_slice(&header_bytes)
        .map_err(|_| AuthError::invalid_client("Invalid JWT header JSON"))?;

    match header.alg.as_str() {
        "RS256" => Ok(Algorithm::RS256),
        "RS384" => Ok(Algorithm::RS384),
        "RS512" => Ok(Algorithm::RS512),
        "ES256" => Ok(Algorithm::ES256),
        "ES384" => Ok(Algorithm::ES384),
        "PS256" => Ok(Algorithm::PS256),
        "PS384" => Ok(Algorithm::PS384),
        "PS512" => Ok(Algorithm::PS512),
        _ => Err(AuthError::invalid_client(format!(
            "Unsupported JWT algorithm: {}",
            header.alg
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;

    #[test]
    fn test_string_or_array_contains() {
        // Single string
        let aud = StringOrArray::String("https://example.com/token".to_string());
        assert!(aud.contains("https://example.com/token"));
        assert!(!aud.contains("https://other.com/token"));

        // Array
        let aud = StringOrArray::Array(vec![
            "https://example.com/token".to_string(),
            "https://example.com/fhir".to_string(),
        ]);
        assert!(aud.contains("https://example.com/token"));
        assert!(aud.contains("https://example.com/fhir"));
        assert!(!aud.contains("https://other.com/token"));
    }

    #[test]
    fn test_string_or_array_first() {
        let aud = StringOrArray::String("https://example.com".to_string());
        assert_eq!(aud.first(), Some("https://example.com"));

        let aud = StringOrArray::Array(vec!["first".to_string(), "second".to_string()]);
        assert_eq!(aud.first(), Some("first"));

        let aud = StringOrArray::Array(vec![]);
        assert_eq!(aud.first(), None);
    }

    #[test]
    fn test_extract_client_id_unverified() {
        // Create a minimal valid JWT (not cryptographically valid, just structurally)
        // Header: {"alg":"RS384","typ":"JWT"}
        // Payload: {"iss":"client-123","sub":"client-123","aud":"https://token","exp":9999999999,"jti":"abc"}
        let header = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(r#"{"alg":"RS384","typ":"JWT"}"#);
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(
            r#"{"iss":"client-123","sub":"client-123","aud":"https://token","exp":9999999999,"jti":"abc"}"#,
        );
        let signature = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode("fake-sig");

        let jwt = format!("{}.{}.{}", header, payload, signature);
        let result = extract_client_id_unverified(&jwt).unwrap();
        assert_eq!(result, "client-123");
    }

    #[test]
    fn test_extract_key_id() {
        // Header with kid
        let header = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(r#"{"alg":"RS384","typ":"JWT","kid":"key-1"}"#);
        let payload =
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(r#"{"iss":"client"}"#);
        let signature = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode("sig");

        let jwt = format!("{}.{}.{}", header, payload, signature);
        let kid = extract_key_id(&jwt).unwrap();
        assert_eq!(kid, Some("key-1".to_string()));

        // Header without kid
        let header = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(r#"{"alg":"RS384","typ":"JWT"}"#);
        let jwt = format!("{}.{}.{}", header, payload, signature);
        let kid = extract_key_id(&jwt).unwrap();
        assert_eq!(kid, None);
    }

    #[test]
    fn test_extract_algorithm() {
        let test_cases = vec![
            ("RS256", Algorithm::RS256),
            ("RS384", Algorithm::RS384),
            ("ES384", Algorithm::ES384),
        ];

        for (alg_str, expected) in test_cases {
            let header = base64::engine::general_purpose::URL_SAFE_NO_PAD
                .encode(format!(r#"{{"alg":"{}","typ":"JWT"}}"#, alg_str));
            let payload =
                base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(r#"{"iss":"client"}"#);
            let signature = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode("sig");

            let jwt = format!("{}.{}.{}", header, payload, signature);
            let alg = extract_algorithm(&jwt).unwrap();
            assert_eq!(alg, expected);
        }
    }

    #[test]
    fn test_config_builder() {
        let config = ClientAssertionConfig::new("https://example.com/token").with_max_lifetime(600);

        assert_eq!(config.token_endpoint, "https://example.com/token");
        assert_eq!(config.max_lifetime_seconds, 600);
    }

    #[test]
    fn test_config_defaults() {
        let config = ClientAssertionConfig::new("https://example.com/token");
        assert_eq!(config.max_lifetime_seconds, 300); // 5 minutes default
    }
}
