//! Secret value encryption using AES-256-GCM
//!
//! Provides encryption at rest for sensitive configuration values
//! like API keys, passwords, and tokens.

use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit},
};
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::ConfigError;

/// Nonce size for AES-256-GCM (96 bits)
const NONCE_SIZE: usize = 12;

/// Key size for AES-256 (256 bits)
const KEY_SIZE: usize = 32;

/// An encrypted secret value
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretValue {
    /// Whether this value is encrypted
    pub encrypted: bool,
    /// Base64-encoded ciphertext
    pub ciphertext: String,
    /// Base64-encoded nonce
    pub nonce: String,
    /// Key identifier for key rotation support
    pub key_id: String,
}

impl SecretValue {
    /// Encrypt a plaintext value
    pub fn encrypt(
        plaintext: &str,
        key: &[u8; KEY_SIZE],
        key_id: &str,
    ) -> Result<Self, ConfigError> {
        let cipher = Aes256Gcm::new_from_slice(key)
            .map_err(|e| ConfigError::encryption(format!("Failed to create cipher: {e}")))?;

        // Generate random nonce
        let mut nonce_bytes = [0u8; NONCE_SIZE];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        // Encrypt
        let ciphertext = cipher
            .encrypt(nonce, plaintext.as_bytes())
            .map_err(|e| ConfigError::encryption(format!("Encryption failed: {e}")))?;

        Ok(Self {
            encrypted: true,
            ciphertext: BASE64.encode(&ciphertext),
            nonce: BASE64.encode(nonce_bytes),
            key_id: key_id.to_string(),
        })
    }

    /// Decrypt the value
    pub fn decrypt(&self, key: &[u8; KEY_SIZE]) -> Result<String, ConfigError> {
        if !self.encrypted {
            return Err(ConfigError::encryption("Value is not encrypted"));
        }

        let cipher = Aes256Gcm::new_from_slice(key)
            .map_err(|e| ConfigError::encryption(format!("Failed to create cipher: {e}")))?;

        let ciphertext = BASE64
            .decode(&self.ciphertext)
            .map_err(|e| ConfigError::encryption(format!("Invalid ciphertext base64: {e}")))?;

        let nonce_bytes = BASE64
            .decode(&self.nonce)
            .map_err(|e| ConfigError::encryption(format!("Invalid nonce base64: {e}")))?;

        if nonce_bytes.len() != NONCE_SIZE {
            return Err(ConfigError::encryption("Invalid nonce size"));
        }

        let nonce = Nonce::from_slice(&nonce_bytes);

        let plaintext = cipher
            .decrypt(nonce, ciphertext.as_ref())
            .map_err(|e| ConfigError::encryption(format!("Decryption failed: {e}")))?;

        String::from_utf8(plaintext)
            .map_err(|e| ConfigError::encryption(format!("Invalid UTF-8 in decrypted value: {e}")))
    }

    /// Create a plaintext (unencrypted) secret value
    pub fn plaintext(value: &str) -> Self {
        Self {
            encrypted: false,
            ciphertext: value.to_string(),
            nonce: String::new(),
            key_id: String::new(),
        }
    }

    /// Get the value (decrypted if needed, or plaintext)
    pub fn get(&self, key: Option<&[u8; KEY_SIZE]>) -> Result<String, ConfigError> {
        if self.encrypted {
            match key {
                Some(k) => self.decrypt(k),
                None => Err(ConfigError::encryption(
                    "Encryption key required to decrypt secret",
                )),
            }
        } else {
            Ok(self.ciphertext.clone())
        }
    }
}

/// Key entry in the keyring
#[derive(Clone)]
struct KeyEntry {
    key: [u8; KEY_SIZE],
    /// When the key was created (for rotation tracking)
    #[allow(dead_code)]
    created_at: time::OffsetDateTime,
}

/// Secrets manager with key rotation support
#[derive(Clone)]
pub struct Secrets {
    /// Current key ID
    current_key_id: String,
    /// Keyring for key rotation (key_id -> key)
    keyring: Arc<RwLock<HashMap<String, KeyEntry>>>,
}

impl Secrets {
    /// Create a new secrets manager from environment variable
    pub fn from_env() -> Result<Option<Self>, ConfigError> {
        match std::env::var("OCTOFHIR_CONFIG_KEY") {
            Ok(key_str) => {
                let key = Self::parse_key(&key_str)?;
                Ok(Some(Self::new(key, "primary")))
            }
            Err(std::env::VarError::NotPresent) => Ok(None),
            Err(e) => Err(ConfigError::encryption(format!(
                "Failed to read OCTOFHIR_CONFIG_KEY: {e}"
            ))),
        }
    }

    /// Create a new secrets manager with a key
    pub fn new(key: [u8; KEY_SIZE], key_id: &str) -> Self {
        let mut keyring = HashMap::new();
        keyring.insert(
            key_id.to_string(),
            KeyEntry {
                key,
                created_at: time::OffsetDateTime::now_utc(),
            },
        );

        Self {
            current_key_id: key_id.to_string(),
            keyring: Arc::new(RwLock::new(keyring)),
        }
    }

    /// Parse a key from a hex or base64 string
    fn parse_key(key_str: &str) -> Result<[u8; KEY_SIZE], ConfigError> {
        // Try hex first
        if key_str.len() == KEY_SIZE * 2 {
            let bytes = hex::decode(key_str)
                .map_err(|e| ConfigError::encryption(format!("Invalid hex key: {e}")))?;
            if bytes.len() == KEY_SIZE {
                let mut key = [0u8; KEY_SIZE];
                key.copy_from_slice(&bytes);
                return Ok(key);
            }
        }

        // Try base64
        let bytes = BASE64
            .decode(key_str.trim())
            .map_err(|e| ConfigError::encryption(format!("Invalid base64 key: {e}")))?;

        if bytes.len() != KEY_SIZE {
            return Err(ConfigError::encryption(format!(
                "Key must be {} bytes, got {}",
                KEY_SIZE,
                bytes.len()
            )));
        }

        let mut key = [0u8; KEY_SIZE];
        key.copy_from_slice(&bytes);
        Ok(key)
    }

    /// Generate a new random key
    pub fn generate_key() -> [u8; KEY_SIZE] {
        let mut key = [0u8; KEY_SIZE];
        rand::thread_rng().fill_bytes(&mut key);
        key
    }

    /// Get the current key ID
    pub fn current_key_id(&self) -> &str {
        &self.current_key_id
    }

    /// Encrypt a value with the current key
    pub async fn encrypt(&self, plaintext: &str) -> Result<SecretValue, ConfigError> {
        let keyring = self.keyring.read().await;
        let entry = keyring.get(&self.current_key_id).ok_or_else(|| {
            ConfigError::encryption(format!("Current key '{}' not found", self.current_key_id))
        })?;

        SecretValue::encrypt(plaintext, &entry.key, &self.current_key_id)
    }

    /// Decrypt a secret value
    pub async fn decrypt(&self, secret: &SecretValue) -> Result<String, ConfigError> {
        if !secret.encrypted {
            return Ok(secret.ciphertext.clone());
        }

        let keyring = self.keyring.read().await;
        let entry = keyring.get(&secret.key_id).ok_or_else(|| {
            ConfigError::encryption(format!("Key '{}' not found in keyring", secret.key_id))
        })?;

        secret.decrypt(&entry.key)
    }

    /// Add a new key to the keyring (for rotation)
    pub async fn add_key(&self, key: [u8; KEY_SIZE], key_id: &str) {
        let mut keyring = self.keyring.write().await;
        keyring.insert(
            key_id.to_string(),
            KeyEntry {
                key,
                created_at: time::OffsetDateTime::now_utc(),
            },
        );
    }

    /// Rotate to a new key
    pub async fn rotate(
        &self,
        new_key: [u8; KEY_SIZE],
        new_key_id: &str,
    ) -> Result<(), ConfigError> {
        // Add the new key
        self.add_key(new_key, new_key_id).await;

        // Note: In a real implementation, we would need to re-encrypt all secrets
        // with the new key. This is left as a database operation.

        Ok(())
    }

    /// Remove an old key from the keyring
    pub async fn remove_key(&self, key_id: &str) -> Result<(), ConfigError> {
        if key_id == self.current_key_id {
            return Err(ConfigError::encryption("Cannot remove the current key"));
        }

        let mut keyring = self.keyring.write().await;
        keyring.remove(key_id);
        Ok(())
    }
}

impl std::fmt::Debug for Secrets {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Secrets")
            .field("current_key_id", &self.current_key_id)
            .field("keyring_size", &"<redacted>")
            .finish()
    }
}

/// Helper to check if a JSON value might be a secret
pub fn is_secret_key(key: &str) -> bool {
    let lower = key.to_lowercase();
    lower.contains("password")
        || lower.contains("secret")
        || lower.contains("token")
        || lower.contains("api_key")
        || lower.contains("apikey")
        || lower.contains("private_key")
        || lower.contains("privatekey")
        || lower.contains("credential")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encrypt_decrypt() {
        let key = Secrets::generate_key();
        let plaintext = "my-secret-password";

        let secret = SecretValue::encrypt(plaintext, &key, "test-key").unwrap();
        assert!(secret.encrypted);
        assert_ne!(secret.ciphertext, plaintext);

        let decrypted = secret.decrypt(&key).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_plaintext_value() {
        let secret = SecretValue::plaintext("not-encrypted");
        assert!(!secret.encrypted);
        assert_eq!(secret.get(None).unwrap(), "not-encrypted");
    }

    #[test]
    fn test_wrong_key_fails() {
        let key1 = Secrets::generate_key();
        let key2 = Secrets::generate_key();

        let secret = SecretValue::encrypt("secret", &key1, "key1").unwrap();
        assert!(secret.decrypt(&key2).is_err());
    }

    #[test]
    fn test_is_secret_key() {
        assert!(is_secret_key("password"));
        assert!(is_secret_key("db_password"));
        assert!(is_secret_key("API_KEY"));
        assert!(is_secret_key("jwt_secret"));
        assert!(!is_secret_key("username"));
        assert!(!is_secret_key("host"));
    }

    #[tokio::test]
    async fn test_secrets_manager() {
        let key = Secrets::generate_key();
        let secrets = Secrets::new(key, "primary");

        let encrypted = secrets.encrypt("test-value").await.unwrap();
        let decrypted = secrets.decrypt(&encrypted).await.unwrap();
        assert_eq!(decrypted, "test-value");
    }
}
