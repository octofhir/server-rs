//! App secret generation and verification.
//!
//! This module provides cryptographically secure secret generation and
//! Argon2-based hashing for App authentication.
//!
//! # Security
//!
//! - Secrets are 256-bit random values (32 bytes) with "app_" prefix
//! - Hashing uses Argon2id (hybrid mode) with default parameters
//! - Salts are generated using OsRng (cryptographically secure RNG)
//!
//! # Example
//!
//! ```
//! use octofhir_auth::app_secret::{generate_app_secret, hash_app_secret, verify_app_secret};
//!
//! // Generate a new secret
//! let secret = generate_app_secret();
//! assert!(secret.starts_with("app_"));
//!
//! // Hash for storage
//! let hash = hash_app_secret(&secret).unwrap();
//!
//! // Verify later
//! assert!(verify_app_secret(&secret, &hash).unwrap());
//! ```

use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use rand::Rng;

/// Generate a new cryptographically secure app secret.
///
/// The secret is a 256-bit (32 bytes) random value encoded as hexadecimal
/// with an "app_" prefix for easy identification.
///
/// # Format
///
/// `app_{64 hex characters}` (68 characters total)
///
/// # Example
///
/// ```
/// use octofhir_auth::app_secret::generate_app_secret;
///
/// let secret = generate_app_secret();
/// assert_eq!(secret.len(), 68); // "app_" + 64 hex chars
/// assert!(secret.starts_with("app_"));
/// ```
pub fn generate_app_secret() -> String {
    let bytes: [u8; 32] = rand::thread_rng().r#gen();
    format!("app_{}", hex::encode(bytes))
}

/// Hash an app secret for secure storage using Argon2id.
///
/// Uses Argon2id (hybrid mode) with:
/// - Cryptographically secure random salt (OsRng)
/// - Default parameters (memory cost, time cost, parallelism)
/// - PHC string format for storage
///
/// # Arguments
///
/// * `secret` - The plaintext app secret to hash
///
/// # Returns
///
/// PHC-formatted hash string suitable for database storage.
///
/// # Errors
///
/// Returns `argon2::password_hash::Error` if hashing fails (rare).
///
/// # Example
///
/// ```
/// use octofhir_auth::app_secret::{generate_app_secret, hash_app_secret};
///
/// let secret = generate_app_secret();
/// let hash = hash_app_secret(&secret).unwrap();
/// assert!(hash.starts_with("$argon2id$"));
/// ```
pub fn hash_app_secret(secret: &str) -> Result<String, argon2::password_hash::Error> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    let hash = argon2.hash_password(secret.as_bytes(), &salt)?;
    Ok(hash.to_string())
}

/// Verify an app secret against a stored Argon2 hash.
///
/// # Arguments
///
/// * `secret` - The plaintext secret to verify
/// * `hash` - The PHC-formatted Argon2 hash from storage
///
/// # Returns
///
/// `Ok(true)` if the secret matches the hash, `Ok(false)` if it doesn't match.
/// Returns `Err` only if the hash format is invalid.
///
/// # Example
///
/// ```
/// use octofhir_auth::app_secret::{generate_app_secret, hash_app_secret, verify_app_secret};
///
/// let secret = generate_app_secret();
/// let hash = hash_app_secret(&secret).unwrap();
///
/// assert!(verify_app_secret(&secret, &hash).unwrap());
/// assert!(!verify_app_secret("wrong_secret", &hash).unwrap());
/// ```
pub fn verify_app_secret(secret: &str, hash: &str) -> Result<bool, argon2::password_hash::Error> {
    let parsed_hash = PasswordHash::new(hash)?;
    let result = Argon2::default().verify_password(secret.as_bytes(), &parsed_hash);
    Ok(result.is_ok())
}

// =============================================================================
// Generic Password Hashing
// =============================================================================

/// Hash a password for secure storage using Argon2id.
///
/// This is a generic password hashing function suitable for:
/// - User passwords
/// - Client secrets
/// - Any other credentials requiring secure storage
///
/// Uses Argon2id (hybrid mode) with:
/// - Cryptographically secure random salt (OsRng)
/// - Default parameters (memory cost, time cost, parallelism)
/// - PHC string format for storage
///
/// # Arguments
///
/// * `password` - The plaintext password to hash
///
/// # Returns
///
/// PHC-formatted hash string suitable for database storage.
///
/// # Errors
///
/// Returns `argon2::password_hash::Error` if hashing fails (rare).
///
/// # Example
///
/// ```
/// use octofhir_auth::app_secret::{hash_password, verify_password};
///
/// let password = "my_secure_password";
/// let hash = hash_password(password).unwrap();
/// assert!(hash.starts_with("$argon2id$"));
/// assert!(verify_password(password, &hash).unwrap());
/// ```
pub fn hash_password(password: &str) -> Result<String, argon2::password_hash::Error> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    let hash = argon2.hash_password(password.as_bytes(), &salt)?;
    Ok(hash.to_string())
}

/// Verify a password against a stored Argon2 hash.
///
/// # Arguments
///
/// * `password` - The plaintext password to verify
/// * `hash` - The PHC-formatted Argon2 hash from storage
///
/// # Returns
///
/// `Ok(true)` if the password matches the hash, `Ok(false)` if it doesn't match.
/// Returns `Err` only if the hash format is invalid.
///
/// # Example
///
/// ```
/// use octofhir_auth::app_secret::{hash_password, verify_password};
///
/// let password = "my_secure_password";
/// let hash = hash_password(password).unwrap();
///
/// assert!(verify_password(password, &hash).unwrap());
/// assert!(!verify_password("wrong_password", &hash).unwrap());
/// ```
pub fn verify_password(password: &str, hash: &str) -> Result<bool, argon2::password_hash::Error> {
    let parsed_hash = PasswordHash::new(hash)?;
    let result = Argon2::default().verify_password(password.as_bytes(), &parsed_hash);
    Ok(result.is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_secret_format() {
        let secret = generate_app_secret();
        assert!(secret.starts_with("app_"), "Secret should start with 'app_'");
        assert_eq!(secret.len(), 68, "Secret should be 68 chars (app_ + 64 hex)");

        // Verify it's valid hex after the prefix
        let hex_part = &secret[4..];
        assert!(hex::decode(hex_part).is_ok(), "Secret should be valid hex after prefix");
    }

    #[test]
    fn test_generate_secret_uniqueness() {
        let secret1 = generate_app_secret();
        let secret2 = generate_app_secret();
        assert_ne!(secret1, secret2, "Secrets should be unique");
    }

    #[test]
    fn test_hash_app_secret() {
        let secret = generate_app_secret();
        let hash = hash_app_secret(&secret).unwrap();

        // Verify PHC format
        assert!(hash.starts_with("$argon2id$"), "Hash should use Argon2id");
        assert!(hash.contains('$'), "Hash should be in PHC format");
    }

    #[test]
    fn test_verify_correct_secret() {
        let secret = generate_app_secret();
        let hash = hash_app_secret(&secret).unwrap();

        assert!(verify_app_secret(&secret, &hash).unwrap(),
            "Correct secret should verify successfully");
    }

    #[test]
    fn test_verify_wrong_secret() {
        let secret = generate_app_secret();
        let hash = hash_app_secret(&secret).unwrap();

        assert!(!verify_app_secret("app_wrong_secret_123456789012345678901234567890123456789012345678", &hash).unwrap(),
            "Wrong secret should not verify");
    }

    #[test]
    fn test_verify_different_secrets() {
        let secret1 = generate_app_secret();
        let secret2 = generate_app_secret();
        let hash1 = hash_app_secret(&secret1).unwrap();

        assert!(!verify_app_secret(&secret2, &hash1).unwrap(),
            "Different secret should not verify against another's hash");
    }

    #[test]
    fn test_hash_produces_different_hashes() {
        let secret = generate_app_secret();
        let hash1 = hash_app_secret(&secret).unwrap();
        let hash2 = hash_app_secret(&secret).unwrap();

        // Same secret should produce different hashes due to random salt
        assert_ne!(hash1, hash2, "Same secret should produce different hashes (different salts)");

        // But both should verify correctly
        assert!(verify_app_secret(&secret, &hash1).unwrap());
        assert!(verify_app_secret(&secret, &hash2).unwrap());
    }

    #[test]
    fn test_verify_invalid_hash_format() {
        let secret = generate_app_secret();
        let result = verify_app_secret(&secret, "invalid_hash_format");

        assert!(result.is_err(), "Invalid hash format should return an error");
    }
}
