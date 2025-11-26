//! JWT token generation and validation.
//!
//! This module provides JWT (JSON Web Token) support for the OctoFHIR authentication
//! system. It supports RS256, RS384, and ES384 signing algorithms as required by
//! the SMART on FHIR specification.
//!
//! ## Supported Algorithms
//!
//! - **RS256**: RSA with SHA-256 (widely compatible)
//! - **RS384**: RSA with SHA-384 (SMART on FHIR preferred)
//! - **ES384**: ECDSA with P-384 curve (SMART on FHIR preferred, smaller keys)
//!
//! ## Example
//!
//! ```ignore
//! use octofhir_auth::token::jwt::{JwtService, SigningKeyPair, SigningAlgorithm};
//!
//! // Generate a new key pair
//! let key_pair = SigningKeyPair::generate_rsa(SigningAlgorithm::RS384)?;
//!
//! // Create JWT service
//! let jwt_service = JwtService::new(key_pair, "https://fhir.example.com".to_string());
//!
//! // Encode claims
//! let token = jwt_service.encode(&claims)?;
//!
//! // Decode and validate
//! let token_data = jwt_service.decode::<AccessTokenClaims>(&token)?;
//! ```

use std::fmt;

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use jsonwebtoken::{
    Algorithm, DecodingKey, EncodingKey, Header, TokenData, Validation, decode, encode,
};
use p384::SecretKey as EcSecretKey;
use p384::ecdsa::SigningKey as EcSigningKey;
use p384::pkcs8::EncodePrivateKey as EcEncodePrivateKey;
use rand::rngs::OsRng;
use rsa::pkcs8::{DecodePublicKey, EncodePublicKey, LineEnding};
use rsa::traits::PublicKeyParts;
use rsa::{RsaPrivateKey, RsaPublicKey};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

// ============================================================================
// Error Types
// ============================================================================

/// Errors that can occur during JWT operations.
#[derive(Debug, thiserror::Error)]
pub enum JwtError {
    /// Failed to encode a token.
    #[error("Failed to encode token: {message}")]
    EncodingError {
        /// Description of the encoding error.
        message: String,
    },

    /// Failed to decode a token.
    #[error("Failed to decode token: {message}")]
    DecodingError {
        /// Description of the decoding error.
        message: String,
    },

    /// The token has expired.
    #[error("Token expired")]
    Expired,

    /// The token signature is invalid.
    #[error("Invalid signature")]
    InvalidSignature,

    /// The token claims are invalid.
    #[error("Invalid claims: {message}")]
    InvalidClaims {
        /// Description of why claims are invalid.
        message: String,
    },

    /// A required claim is missing.
    #[error("Missing required claim: {claim}")]
    MissingClaim {
        /// Name of the missing claim.
        claim: String,
    },

    /// The specified key was not found.
    #[error("Key not found: {kid}")]
    KeyNotFound {
        /// The key ID that was not found.
        kid: String,
    },

    /// Failed to generate a cryptographic key.
    #[error("Key generation error: {message}")]
    KeyGenerationError {
        /// Description of the key generation error.
        message: String,
    },

    /// Invalid key format or data.
    #[error("Invalid key: {message}")]
    InvalidKey {
        /// Description of why the key is invalid.
        message: String,
    },
}

impl JwtError {
    /// Creates a new `EncodingError`.
    #[must_use]
    pub fn encoding_error(message: impl Into<String>) -> Self {
        Self::EncodingError {
            message: message.into(),
        }
    }

    /// Creates a new `DecodingError`.
    #[must_use]
    pub fn decoding_error(message: impl Into<String>) -> Self {
        Self::DecodingError {
            message: message.into(),
        }
    }

    /// Creates a new `InvalidClaims` error.
    #[must_use]
    pub fn invalid_claims(message: impl Into<String>) -> Self {
        Self::InvalidClaims {
            message: message.into(),
        }
    }

    /// Creates a new `MissingClaim` error.
    #[must_use]
    pub fn missing_claim(claim: impl Into<String>) -> Self {
        Self::MissingClaim {
            claim: claim.into(),
        }
    }

    /// Creates a new `KeyNotFound` error.
    #[must_use]
    pub fn key_not_found(kid: impl Into<String>) -> Self {
        Self::KeyNotFound { kid: kid.into() }
    }

    /// Creates a new `KeyGenerationError`.
    #[must_use]
    pub fn key_generation_error(message: impl Into<String>) -> Self {
        Self::KeyGenerationError {
            message: message.into(),
        }
    }

    /// Creates a new `InvalidKey` error.
    #[must_use]
    pub fn invalid_key(message: impl Into<String>) -> Self {
        Self::InvalidKey {
            message: message.into(),
        }
    }

    /// Returns `true` if this is a validation error (expired, invalid signature, etc.).
    #[must_use]
    pub fn is_validation_error(&self) -> bool {
        matches!(
            self,
            Self::Expired | Self::InvalidSignature | Self::InvalidClaims { .. }
        )
    }

    /// Returns `true` if this is a key-related error.
    #[must_use]
    pub fn is_key_error(&self) -> bool {
        matches!(
            self,
            Self::KeyNotFound { .. } | Self::KeyGenerationError { .. } | Self::InvalidKey { .. }
        )
    }
}

impl From<jsonwebtoken::errors::Error> for JwtError {
    fn from(err: jsonwebtoken::errors::Error) -> Self {
        use jsonwebtoken::errors::ErrorKind;

        match err.kind() {
            ErrorKind::ExpiredSignature => Self::Expired,
            ErrorKind::InvalidSignature => Self::InvalidSignature,
            ErrorKind::InvalidToken
            | ErrorKind::InvalidAlgorithm
            | ErrorKind::InvalidAlgorithmName
            | ErrorKind::MissingAlgorithm => Self::decoding_error(err.to_string()),
            ErrorKind::InvalidAudience
            | ErrorKind::InvalidIssuer
            | ErrorKind::InvalidSubject
            | ErrorKind::MissingRequiredClaim(_) => Self::invalid_claims(err.to_string()),
            ErrorKind::InvalidRsaKey(_)
            | ErrorKind::InvalidEcdsaKey
            | ErrorKind::InvalidKeyFormat => Self::invalid_key(err.to_string()),
            _ => Self::decoding_error(err.to_string()),
        }
    }
}

// ============================================================================
// Signing Algorithm
// ============================================================================

/// Supported signing algorithms for JWT tokens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SigningAlgorithm {
    /// RSA with SHA-256 (widely compatible).
    RS256,
    /// RSA with SHA-384 (SMART on FHIR preferred).
    RS384,
    /// ECDSA with P-384 curve (SMART on FHIR preferred, smaller keys).
    ES384,
}

impl SigningAlgorithm {
    /// Converts to the `jsonwebtoken` Algorithm type.
    #[must_use]
    pub fn to_jwt_algorithm(self) -> Algorithm {
        match self {
            Self::RS256 => Algorithm::RS256,
            Self::RS384 => Algorithm::RS384,
            Self::ES384 => Algorithm::ES384,
        }
    }

    /// Returns the algorithm name as used in JWK/JWT headers.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::RS256 => "RS256",
            Self::RS384 => "RS384",
            Self::ES384 => "ES384",
        }
    }

    /// Returns `true` if this is an RSA-based algorithm.
    #[must_use]
    pub fn is_rsa(&self) -> bool {
        matches!(self, Self::RS256 | Self::RS384)
    }

    /// Returns `true` if this is an EC-based algorithm.
    #[must_use]
    pub fn is_ec(&self) -> bool {
        matches!(self, Self::ES384)
    }
}

impl fmt::Display for SigningAlgorithm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

// ============================================================================
// Token Claims
// ============================================================================

/// Access token claims for SMART on FHIR.
///
/// These claims follow the SMART on FHIR specification for access tokens.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AccessTokenClaims {
    /// Issuer (OctoFHIR server URL).
    pub iss: String,

    /// Subject (user or client ID).
    pub sub: String,

    /// Audience (FHIR server URLs).
    pub aud: Vec<String>,

    /// Expiration time (Unix timestamp).
    pub exp: i64,

    /// Issued at (Unix timestamp).
    pub iat: i64,

    /// JWT ID (unique identifier for revocation).
    pub jti: String,

    /// Space-separated scopes.
    pub scope: String,

    /// OAuth client ID.
    pub client_id: String,

    /// Patient context (FHIR Patient resource ID).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub patient: Option<String>,

    /// Encounter context (FHIR Encounter resource ID).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encounter: Option<String>,

    /// User's FHIR resource reference (e.g., "Practitioner/123").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fhir_user: Option<String>,
}

impl AccessTokenClaims {
    /// Creates a new builder for access token claims.
    #[must_use]
    pub fn builder(
        issuer: impl Into<String>,
        subject: impl Into<String>,
        client_id: impl Into<String>,
    ) -> AccessTokenClaimsBuilder {
        AccessTokenClaimsBuilder::new(issuer, subject, client_id)
    }
}

/// Builder for `AccessTokenClaims`.
pub struct AccessTokenClaimsBuilder {
    iss: String,
    sub: String,
    aud: Vec<String>,
    exp: i64,
    iat: i64,
    jti: String,
    scope: String,
    client_id: String,
    patient: Option<String>,
    encounter: Option<String>,
    fhir_user: Option<String>,
}

impl AccessTokenClaimsBuilder {
    fn new(
        issuer: impl Into<String>,
        subject: impl Into<String>,
        client_id: impl Into<String>,
    ) -> Self {
        let now = OffsetDateTime::now_utc().unix_timestamp();
        Self {
            iss: issuer.into(),
            sub: subject.into(),
            aud: Vec::new(),
            exp: now + 3600, // Default 1 hour
            iat: now,
            jti: uuid::Uuid::new_v4().to_string(),
            scope: String::new(),
            client_id: client_id.into(),
            patient: None,
            encounter: None,
            fhir_user: None,
        }
    }

    /// Sets the audience.
    #[must_use]
    pub fn audience(mut self, aud: Vec<String>) -> Self {
        self.aud = aud;
        self
    }

    /// Sets the expiration time in seconds from now.
    #[must_use]
    pub fn expires_in_seconds(mut self, seconds: i64) -> Self {
        self.exp = self.iat + seconds;
        self
    }

    /// Sets the scopes.
    #[must_use]
    pub fn scope(mut self, scope: impl Into<String>) -> Self {
        self.scope = scope.into();
        self
    }

    /// Sets the patient context.
    #[must_use]
    pub fn patient(mut self, patient: impl Into<String>) -> Self {
        self.patient = Some(patient.into());
        self
    }

    /// Sets the encounter context.
    #[must_use]
    pub fn encounter(mut self, encounter: impl Into<String>) -> Self {
        self.encounter = Some(encounter.into());
        self
    }

    /// Sets the FHIR user reference.
    #[must_use]
    pub fn fhir_user(mut self, fhir_user: impl Into<String>) -> Self {
        self.fhir_user = Some(fhir_user.into());
        self
    }

    /// Builds the access token claims.
    #[must_use]
    pub fn build(self) -> AccessTokenClaims {
        AccessTokenClaims {
            iss: self.iss,
            sub: self.sub,
            aud: self.aud,
            exp: self.exp,
            iat: self.iat,
            jti: self.jti,
            scope: self.scope,
            client_id: self.client_id,
            patient: self.patient,
            encounter: self.encounter,
            fhir_user: self.fhir_user,
        }
    }
}

/// ID token claims for OpenID Connect.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IdTokenClaims {
    /// Issuer (OctoFHIR server URL).
    pub iss: String,

    /// Subject (user ID).
    pub sub: String,

    /// Audience (client ID).
    pub aud: String,

    /// Expiration time (Unix timestamp).
    pub exp: i64,

    /// Issued at (Unix timestamp).
    pub iat: i64,

    /// Nonce from authorization request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nonce: Option<String>,

    /// User's FHIR resource reference.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fhir_user: Option<String>,
}

// ============================================================================
// JWKS Types
// ============================================================================

/// JSON Web Key Set.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Jwks {
    /// The keys in this set.
    pub keys: Vec<Jwk>,
}

impl Jwks {
    /// Creates a new empty JWKS.
    #[must_use]
    pub fn new() -> Self {
        Self { keys: Vec::new() }
    }

    /// Adds a key to the set.
    pub fn add_key(&mut self, key: Jwk) {
        self.keys.push(key);
    }
}

impl Default for Jwks {
    fn default() -> Self {
        Self::new()
    }
}

/// JSON Web Key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Jwk {
    /// Key type ("RSA" or "EC").
    pub kty: String,

    /// Key ID.
    pub kid: String,

    /// Key use ("sig" for signing).
    #[serde(rename = "use")]
    pub use_: String,

    /// Algorithm.
    pub alg: String,

    // RSA-specific fields
    /// RSA modulus (base64url encoded).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub n: Option<String>,

    /// RSA exponent (base64url encoded).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub e: Option<String>,

    // EC-specific fields
    /// EC curve name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crv: Option<String>,

    /// EC x coordinate (base64url encoded).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub x: Option<String>,

    /// EC y coordinate (base64url encoded).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub y: Option<String>,
}

// ============================================================================
// Signing Key Pair
// ============================================================================

/// A signing key pair for JWT operations.
pub struct SigningKeyPair {
    /// Key ID.
    pub kid: String,

    /// Signing algorithm.
    pub algorithm: SigningAlgorithm,

    /// Encoding key (private key) for signing.
    encoding_key: EncodingKey,

    /// Decoding key (public key) for verification.
    decoding_key: DecodingKey,

    /// Public key data for JWKS export.
    public_key_data: PublicKeyData,

    /// When the key was created.
    pub created_at: OffsetDateTime,
}

/// Internal representation of public key data for JWKS export.
enum PublicKeyData {
    Rsa { n: Vec<u8>, e: Vec<u8> },
    Ec { x: Vec<u8>, y: Vec<u8> },
}

impl SigningKeyPair {
    /// Generates a new RSA key pair.
    ///
    /// # Arguments
    /// * `algorithm` - The signing algorithm (must be RS256 or RS384)
    ///
    /// # Errors
    /// Returns an error if key generation fails or algorithm is not RSA-based.
    pub fn generate_rsa(algorithm: SigningAlgorithm) -> Result<Self, JwtError> {
        if !algorithm.is_rsa() {
            return Err(JwtError::invalid_key(format!(
                "Algorithm {} is not RSA-based",
                algorithm
            )));
        }

        let bits = 2048;
        let private_key = RsaPrivateKey::new(&mut OsRng, bits)
            .map_err(|e| JwtError::key_generation_error(e.to_string()))?;

        let public_key = private_key.to_public_key();
        let n = public_key.n().to_bytes_be();
        let e = public_key.e().to_bytes_be();

        let private_pem = private_key
            .to_pkcs8_pem(LineEnding::LF)
            .map_err(|e| JwtError::key_generation_error(e.to_string()))?;

        let encoding_key = EncodingKey::from_rsa_pem(private_pem.as_bytes())
            .map_err(|e| JwtError::key_generation_error(e.to_string()))?;

        let public_pem = public_key
            .to_public_key_pem(LineEnding::LF)
            .map_err(|e| JwtError::key_generation_error(e.to_string()))?;

        let decoding_key = DecodingKey::from_rsa_pem(public_pem.as_bytes())
            .map_err(|e| JwtError::key_generation_error(e.to_string()))?;

        Ok(Self {
            kid: uuid::Uuid::new_v4().to_string(),
            algorithm,
            encoding_key,
            decoding_key,
            public_key_data: PublicKeyData::Rsa { n, e },
            created_at: OffsetDateTime::now_utc(),
        })
    }

    /// Generates a new EC key pair using P-384 curve.
    ///
    /// # Errors
    /// Returns an error if key generation fails.
    pub fn generate_ec() -> Result<Self, JwtError> {
        let secret_key = EcSecretKey::random(&mut OsRng);
        let signing_key = EcSigningKey::from(&secret_key);
        let public_key = signing_key.verifying_key();

        // Get public key point
        let point = public_key.to_encoded_point(false);
        let x = point
            .x()
            .ok_or_else(|| JwtError::key_generation_error("Missing x coordinate"))?;
        let y = point
            .y()
            .ok_or_else(|| JwtError::key_generation_error("Missing y coordinate"))?;

        // Export to PKCS8 PEM (required by jsonwebtoken)
        let private_pem = secret_key
            .to_pkcs8_pem(LineEnding::LF)
            .map_err(|e| JwtError::key_generation_error(e.to_string()))?;

        let encoding_key = EncodingKey::from_ec_pem(private_pem.as_bytes())
            .map_err(|e| JwtError::key_generation_error(e.to_string()))?;

        // For EC decoding key, we need to create from components
        let x_b64 = URL_SAFE_NO_PAD.encode(x.as_slice());
        let y_b64 = URL_SAFE_NO_PAD.encode(y.as_slice());
        let decoding_key = DecodingKey::from_ec_components(&x_b64, &y_b64)
            .map_err(|e| JwtError::key_generation_error(e.to_string()))?;

        Ok(Self {
            kid: uuid::Uuid::new_v4().to_string(),
            algorithm: SigningAlgorithm::ES384,
            encoding_key,
            decoding_key,
            public_key_data: PublicKeyData::Ec {
                x: x.to_vec(),
                y: y.to_vec(),
            },
            created_at: OffsetDateTime::now_utc(),
        })
    }

    /// Loads a key pair from PEM strings.
    ///
    /// # Arguments
    /// * `kid` - Key ID
    /// * `algorithm` - Signing algorithm
    /// * `private_pem` - PEM-encoded private key
    /// * `public_pem` - PEM-encoded public key
    ///
    /// # Errors
    /// Returns an error if the PEM data is invalid.
    pub fn from_pem(
        kid: impl Into<String>,
        algorithm: SigningAlgorithm,
        private_pem: &str,
        public_pem: &str,
    ) -> Result<Self, JwtError> {
        let (encoding_key, decoding_key, public_key_data) = if algorithm.is_rsa() {
            let encoding_key = EncodingKey::from_rsa_pem(private_pem.as_bytes())
                .map_err(|e| JwtError::invalid_key(e.to_string()))?;
            let decoding_key = DecodingKey::from_rsa_pem(public_pem.as_bytes())
                .map_err(|e| JwtError::invalid_key(e.to_string()))?;

            // Parse public key to extract n and e
            let public_key = RsaPublicKey::from_public_key_pem(public_pem)
                .map_err(|e| JwtError::invalid_key(e.to_string()))?;
            let n = public_key.n().to_bytes_be();
            let e = public_key.e().to_bytes_be();

            (encoding_key, decoding_key, PublicKeyData::Rsa { n, e })
        } else {
            let encoding_key = EncodingKey::from_ec_pem(private_pem.as_bytes())
                .map_err(|e| JwtError::invalid_key(e.to_string()))?;

            // Parse EC private key to get public key
            let secret_key = EcSecretKey::from_sec1_pem(private_pem)
                .map_err(|e| JwtError::invalid_key(e.to_string()))?;
            let signing_key = EcSigningKey::from(&secret_key);
            let point = signing_key.verifying_key().to_encoded_point(false);
            let x = point
                .x()
                .ok_or_else(|| JwtError::invalid_key("Missing x coordinate"))?;
            let y = point
                .y()
                .ok_or_else(|| JwtError::invalid_key("Missing y coordinate"))?;

            let x_b64 = URL_SAFE_NO_PAD.encode(x.as_slice());
            let y_b64 = URL_SAFE_NO_PAD.encode(y.as_slice());
            let decoding_key = DecodingKey::from_ec_components(&x_b64, &y_b64)
                .map_err(|e| JwtError::invalid_key(e.to_string()))?;

            (
                encoding_key,
                decoding_key,
                PublicKeyData::Ec {
                    x: x.to_vec(),
                    y: y.to_vec(),
                },
            )
        };

        Ok(Self {
            kid: kid.into(),
            algorithm,
            encoding_key,
            decoding_key,
            public_key_data,
            created_at: OffsetDateTime::now_utc(),
        })
    }

    /// Exports the public key as a JWK.
    #[must_use]
    pub fn to_jwk(&self) -> Jwk {
        match &self.public_key_data {
            PublicKeyData::Rsa { n, e } => Jwk {
                kty: "RSA".to_string(),
                kid: self.kid.clone(),
                use_: "sig".to_string(),
                alg: self.algorithm.as_str().to_string(),
                n: Some(URL_SAFE_NO_PAD.encode(n)),
                e: Some(URL_SAFE_NO_PAD.encode(e)),
                crv: None,
                x: None,
                y: None,
            },
            PublicKeyData::Ec { x, y } => Jwk {
                kty: "EC".to_string(),
                kid: self.kid.clone(),
                use_: "sig".to_string(),
                alg: self.algorithm.as_str().to_string(),
                n: None,
                e: None,
                crv: Some("P-384".to_string()),
                x: Some(URL_SAFE_NO_PAD.encode(x)),
                y: Some(URL_SAFE_NO_PAD.encode(y)),
            },
        }
    }
}

// ============================================================================
// JWT Service
// ============================================================================

/// Service for encoding and decoding JWT tokens.
///
/// This service is thread-safe (`Send + Sync`) and can be shared across
/// async tasks.
pub struct JwtService {
    signing_key: SigningKeyPair,
    issuer: String,
}

impl JwtService {
    /// Creates a new JWT service.
    ///
    /// # Arguments
    /// * `signing_key` - The key pair to use for signing/verification
    /// * `issuer` - The issuer claim value (typically the server URL)
    #[must_use]
    pub fn new(signing_key: SigningKeyPair, issuer: impl Into<String>) -> Self {
        Self {
            signing_key,
            issuer: issuer.into(),
        }
    }

    /// Encodes claims into a JWT string.
    ///
    /// # Errors
    /// Returns an error if encoding fails.
    pub fn encode<T: Serialize>(&self, claims: &T) -> Result<String, JwtError> {
        let mut header = Header::new(self.signing_key.algorithm.to_jwt_algorithm());
        header.kid = Some(self.signing_key.kid.clone());

        encode(&header, claims, &self.signing_key.encoding_key)
            .map_err(|e| JwtError::encoding_error(e.to_string()))
    }

    /// Decodes and validates a JWT string.
    ///
    /// # Errors
    /// Returns an error if decoding or validation fails.
    pub fn decode<T: DeserializeOwned>(&self, token: &str) -> Result<TokenData<T>, JwtError> {
        let mut validation = Validation::new(self.signing_key.algorithm.to_jwt_algorithm());
        validation.set_issuer(&[&self.issuer]);
        validation.validate_exp = true;
        validation.validate_aud = false; // Audience validated at application layer

        decode(token, &self.signing_key.decoding_key, &validation).map_err(JwtError::from)
    }

    /// Decodes a JWT without validating expiration (useful for introspection).
    ///
    /// # Errors
    /// Returns an error if decoding fails (signature is still validated).
    pub fn decode_allow_expired<T: DeserializeOwned>(
        &self,
        token: &str,
    ) -> Result<TokenData<T>, JwtError> {
        let mut validation = Validation::new(self.signing_key.algorithm.to_jwt_algorithm());
        validation.set_issuer(&[&self.issuer]);
        validation.validate_exp = false;
        validation.validate_aud = false;

        decode(token, &self.signing_key.decoding_key, &validation).map_err(JwtError::from)
    }

    /// Returns the current signing key ID.
    #[must_use]
    pub fn current_kid(&self) -> &str {
        &self.signing_key.kid
    }

    /// Returns the issuer URL.
    #[must_use]
    pub fn issuer(&self) -> &str {
        &self.issuer
    }

    /// Returns the JWKS containing the public key(s).
    #[must_use]
    pub fn jwks(&self) -> Jwks {
        let mut jwks = Jwks::new();
        jwks.add_key(self.signing_key.to_jwk());
        jwks
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_rsa_rs256_key_pair() {
        let key_pair = SigningKeyPair::generate_rsa(SigningAlgorithm::RS256).unwrap();
        assert_eq!(key_pair.algorithm, SigningAlgorithm::RS256);
        assert!(!key_pair.kid.is_empty());
    }

    #[test]
    fn test_generate_rsa_rs384_key_pair() {
        let key_pair = SigningKeyPair::generate_rsa(SigningAlgorithm::RS384).unwrap();
        assert_eq!(key_pair.algorithm, SigningAlgorithm::RS384);
        assert!(!key_pair.kid.is_empty());
    }

    #[test]
    fn test_generate_ec_key_pair() {
        let key_pair = SigningKeyPair::generate_ec().unwrap();
        assert_eq!(key_pair.algorithm, SigningAlgorithm::ES384);
        assert!(!key_pair.kid.is_empty());
    }

    #[test]
    fn test_rs256_encode_decode() {
        let key_pair = SigningKeyPair::generate_rsa(SigningAlgorithm::RS256).unwrap();
        let service = JwtService::new(key_pair, "https://fhir.example.com");

        let claims = AccessTokenClaims::builder("https://fhir.example.com", "user123", "client456")
            .audience(vec!["https://fhir.example.com".to_string()])
            .scope("patient/*.read")
            .expires_in_seconds(3600)
            .build();

        let token = service.encode(&claims).unwrap();
        assert!(!token.is_empty());

        let decoded = service.decode::<AccessTokenClaims>(&token).unwrap();
        assert_eq!(decoded.claims.sub, "user123");
        assert_eq!(decoded.claims.client_id, "client456");
        assert_eq!(decoded.claims.scope, "patient/*.read");
    }

    #[test]
    fn test_rs384_encode_decode() {
        let key_pair = SigningKeyPair::generate_rsa(SigningAlgorithm::RS384).unwrap();
        let service = JwtService::new(key_pair, "https://fhir.example.com");

        let claims = AccessTokenClaims::builder("https://fhir.example.com", "user123", "client456")
            .audience(vec!["https://fhir.example.com".to_string()])
            .scope("patient/*.read")
            .build();

        let token = service.encode(&claims).unwrap();
        let decoded = service.decode::<AccessTokenClaims>(&token).unwrap();
        assert_eq!(decoded.claims.sub, "user123");
    }

    #[test]
    fn test_es384_encode_decode() {
        let key_pair = SigningKeyPair::generate_ec().unwrap();
        let service = JwtService::new(key_pair, "https://fhir.example.com");

        let claims = AccessTokenClaims::builder("https://fhir.example.com", "user123", "client456")
            .audience(vec!["https://fhir.example.com".to_string()])
            .scope("patient/*.read")
            .build();

        let token = service.encode(&claims).unwrap();
        let decoded = service.decode::<AccessTokenClaims>(&token).unwrap();
        assert_eq!(decoded.claims.sub, "user123");
    }

    #[test]
    fn test_access_token_claims_serialization() {
        let claims = AccessTokenClaims::builder("https://issuer.com", "sub123", "client123")
            .audience(vec!["aud1".to_string()])
            .scope("openid profile")
            .patient("Patient/123")
            .build();

        let json = serde_json::to_string(&claims).unwrap();
        assert!(json.contains("\"iss\":\"https://issuer.com\""));
        assert!(json.contains("\"sub\":\"sub123\""));
        assert!(json.contains("\"patient\":\"Patient/123\""));

        // Optional fields that are None should not be serialized
        let claims_no_patient =
            AccessTokenClaims::builder("https://issuer.com", "sub123", "client123").build();
        let json_no_patient = serde_json::to_string(&claims_no_patient).unwrap();
        assert!(!json_no_patient.contains("patient"));
    }

    #[test]
    fn test_id_token_claims_serialization() {
        let claims = IdTokenClaims {
            iss: "https://issuer.com".to_string(),
            sub: "user123".to_string(),
            aud: "client123".to_string(),
            exp: 1700000000,
            iat: 1699996400,
            nonce: Some("abc123".to_string()),
            fhir_user: Some("Practitioner/456".to_string()),
        };

        let json = serde_json::to_string(&claims).unwrap();
        assert!(json.contains("\"nonce\":\"abc123\""));
        assert!(json.contains("\"fhir_user\":\"Practitioner/456\""));
    }

    #[test]
    fn test_expired_token_rejected() {
        let key_pair = SigningKeyPair::generate_rsa(SigningAlgorithm::RS256).unwrap();
        let service = JwtService::new(key_pair, "https://fhir.example.com");

        // Create a token that's already expired
        let claims = AccessTokenClaims::builder("https://fhir.example.com", "user123", "client456")
            .expires_in_seconds(-3600) // Expired 1 hour ago
            .build();

        let token = service.encode(&claims).unwrap();
        let result = service.decode::<AccessTokenClaims>(&token);

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), JwtError::Expired));
    }

    #[test]
    fn test_invalid_signature_rejected() {
        // Create two different key pairs
        let key_pair1 = SigningKeyPair::generate_rsa(SigningAlgorithm::RS256).unwrap();
        let key_pair2 = SigningKeyPair::generate_rsa(SigningAlgorithm::RS256).unwrap();

        let service1 = JwtService::new(key_pair1, "https://fhir.example.com");
        let service2 = JwtService::new(key_pair2, "https://fhir.example.com");

        let claims =
            AccessTokenClaims::builder("https://fhir.example.com", "user123", "client456").build();

        // Sign with key1, try to verify with key2
        let token = service1.encode(&claims).unwrap();
        let result = service2.decode::<AccessTokenClaims>(&token);

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), JwtError::InvalidSignature));
    }

    #[test]
    fn test_jwks_generation_rsa() {
        let key_pair = SigningKeyPair::generate_rsa(SigningAlgorithm::RS384).unwrap();
        let jwk = key_pair.to_jwk();

        assert_eq!(jwk.kty, "RSA");
        assert_eq!(jwk.use_, "sig");
        assert_eq!(jwk.alg, "RS384");
        assert!(jwk.n.is_some());
        assert!(jwk.e.is_some());
        assert!(jwk.crv.is_none());
        assert!(jwk.x.is_none());
        assert!(jwk.y.is_none());

        // Verify it serializes to valid JSON
        let json = serde_json::to_string(&jwk).unwrap();
        assert!(json.contains("\"kty\":\"RSA\""));
    }

    #[test]
    fn test_jwks_generation_ec() {
        let key_pair = SigningKeyPair::generate_ec().unwrap();
        let jwk = key_pair.to_jwk();

        assert_eq!(jwk.kty, "EC");
        assert_eq!(jwk.use_, "sig");
        assert_eq!(jwk.alg, "ES384");
        assert_eq!(jwk.crv, Some("P-384".to_string()));
        assert!(jwk.x.is_some());
        assert!(jwk.y.is_some());
        assert!(jwk.n.is_none());
        assert!(jwk.e.is_none());

        // Verify it serializes to valid JSON
        let json = serde_json::to_string(&jwk).unwrap();
        assert!(json.contains("\"kty\":\"EC\""));
        assert!(json.contains("\"crv\":\"P-384\""));
    }

    #[test]
    fn test_jwks_set() {
        let key_pair = SigningKeyPair::generate_rsa(SigningAlgorithm::RS256).unwrap();
        let service = JwtService::new(key_pair, "https://fhir.example.com");

        let jwks = service.jwks();
        assert_eq!(jwks.keys.len(), 1);
        assert_eq!(jwks.keys[0].kty, "RSA");

        // Verify JWKS serializes correctly
        let json = serde_json::to_string(&jwks).unwrap();
        assert!(json.contains("\"keys\":["));
    }

    #[test]
    fn test_signing_algorithm_properties() {
        assert!(SigningAlgorithm::RS256.is_rsa());
        assert!(SigningAlgorithm::RS384.is_rsa());
        assert!(!SigningAlgorithm::ES384.is_rsa());

        assert!(!SigningAlgorithm::RS256.is_ec());
        assert!(!SigningAlgorithm::RS384.is_ec());
        assert!(SigningAlgorithm::ES384.is_ec());

        assert_eq!(SigningAlgorithm::RS256.as_str(), "RS256");
        assert_eq!(SigningAlgorithm::RS384.as_str(), "RS384");
        assert_eq!(SigningAlgorithm::ES384.as_str(), "ES384");
    }

    #[test]
    fn test_jwt_error_predicates() {
        assert!(JwtError::Expired.is_validation_error());
        assert!(JwtError::InvalidSignature.is_validation_error());
        assert!(JwtError::invalid_claims("test").is_validation_error());

        assert!(!JwtError::Expired.is_key_error());
        assert!(JwtError::key_not_found("kid").is_key_error());
        assert!(JwtError::key_generation_error("err").is_key_error());
        assert!(JwtError::invalid_key("err").is_key_error());
    }

    #[test]
    fn test_decode_allow_expired() {
        let key_pair = SigningKeyPair::generate_rsa(SigningAlgorithm::RS256).unwrap();
        let service = JwtService::new(key_pair, "https://fhir.example.com");

        // Create an expired token
        let claims = AccessTokenClaims::builder("https://fhir.example.com", "user123", "client456")
            .expires_in_seconds(-3600)
            .build();

        let token = service.encode(&claims).unwrap();

        // Regular decode should fail
        assert!(service.decode::<AccessTokenClaims>(&token).is_err());

        // decode_allow_expired should succeed
        let decoded = service
            .decode_allow_expired::<AccessTokenClaims>(&token)
            .unwrap();
        assert_eq!(decoded.claims.sub, "user123");
    }
}
