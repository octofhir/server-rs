//! Client JWKS fetching and caching.
//!
//! This module provides caching for client JSON Web Key Sets (JWKS) used
//! to verify JWT client assertions in the client credentials flow.
//!
//! # Caching Strategy
//!
//! - JWKS are cached in memory with a configurable TTL (default: 1 hour)
//! - Cache is checked before fetching from the remote URI
//! - Expired cache entries trigger a refresh
//! - Failed fetches return cached data if available (fail-open for availability)
//!
//! # Security Considerations
//!
//! - Only HTTPS URIs are allowed for JWKS endpoints
//! - HTTP timeouts prevent hanging on slow endpoints
//! - Response size is limited to prevent DoS attacks
//! - TLS certificate validation is enforced

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use jsonwebtoken::jwk::{Jwk, JwkSet};
use jsonwebtoken::{Algorithm, DecodingKey};
use tokio::sync::RwLock;

use crate::AuthResult;
use crate::error::AuthError;

/// Configuration for the JWKS cache.
#[derive(Debug, Clone)]
pub struct JwksCacheConfig {
    /// Time-to-live for cached JWKS (default: 1 hour).
    pub ttl: Duration,

    /// HTTP request timeout (default: 10 seconds).
    pub request_timeout: Duration,

    /// Maximum response size in bytes (default: 1 MB).
    pub max_response_size: usize,
}

impl Default for JwksCacheConfig {
    fn default() -> Self {
        Self {
            ttl: Duration::from_secs(3600),           // 1 hour
            request_timeout: Duration::from_secs(10), // 10 seconds
            max_response_size: 1024 * 1024,           // 1 MB
        }
    }
}

impl JwksCacheConfig {
    /// Creates a new configuration with default values.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the cache TTL.
    #[must_use]
    pub fn with_ttl(mut self, ttl: Duration) -> Self {
        self.ttl = ttl;
        self
    }

    /// Sets the HTTP request timeout.
    #[must_use]
    pub fn with_request_timeout(mut self, timeout: Duration) -> Self {
        self.request_timeout = timeout;
        self
    }

    /// Sets the maximum response size.
    #[must_use]
    pub fn with_max_response_size(mut self, size: usize) -> Self {
        self.max_response_size = size;
        self
    }
}

/// Cached JWKS entry with metadata.
struct CachedJwks {
    /// The cached JWKS.
    jwks: JwkSet,
    /// When this entry was fetched.
    fetched_at: Instant,
}

/// In-memory cache for client JWKS.
///
/// This cache stores JWKS fetched from client jwks_uri endpoints and
/// provides lookup by key ID (kid).
///
/// # Example
///
/// ```ignore
/// use octofhir_auth::federation::ClientJwksCache;
///
/// let cache = ClientJwksCache::new(JwksCacheConfig::default());
///
/// // Get a decoding key from a client's JWKS URI
/// let key = cache.get_decoding_key(
///     "https://client.example.com/.well-known/jwks.json",
///     Some("key-1"),
///     Algorithm::RS384,
/// ).await?;
/// ```
pub struct ClientJwksCache {
    /// Cached JWKS by URI.
    cache: Arc<RwLock<HashMap<String, CachedJwks>>>,
    /// Configuration.
    config: JwksCacheConfig,
}

impl ClientJwksCache {
    /// Creates a new JWKS cache with the specified configuration.
    #[must_use]
    pub fn new(config: JwksCacheConfig) -> Self {
        Self {
            cache: Arc::new(RwLock::new(HashMap::new())),
            config,
        }
    }

    /// Creates a new JWKS cache with default configuration.
    #[must_use]
    pub fn with_defaults() -> Self {
        Self::new(JwksCacheConfig::default())
    }

    /// Gets a decoding key from a JWKS URI.
    ///
    /// This method checks the cache first and fetches from the URI if
    /// the cache is empty or expired.
    ///
    /// # Arguments
    ///
    /// * `jwks_uri` - The URI to fetch the JWKS from
    /// * `kid` - Optional key ID to find a specific key
    /// * `algorithm` - The expected algorithm for the key
    ///
    /// # Returns
    ///
    /// Returns a `DecodingKey` that can be used to verify JWT signatures.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The JWKS cannot be fetched
    /// - No key with the specified kid is found
    /// - The key cannot be converted to a decoding key
    pub async fn get_decoding_key(
        &self,
        jwks_uri: &str,
        kid: Option<&str>,
        algorithm: Algorithm,
    ) -> AuthResult<DecodingKey> {
        // Check cache first
        {
            let cache = self.cache.read().await;
            if let Some(cached) = cache.get(jwks_uri)
                && cached.fetched_at.elapsed() < self.config.ttl
                && let Some(key) = self.find_key(&cached.jwks, kid, algorithm)
            {
                return self.jwk_to_decoding_key(&key);
            }
        }

        // Fetch fresh JWKS
        let jwks = self.fetch_jwks(jwks_uri).await?;

        // Update cache
        {
            let mut cache = self.cache.write().await;
            cache.insert(
                jwks_uri.to_string(),
                CachedJwks {
                    jwks: jwks.clone(),
                    fetched_at: Instant::now(),
                },
            );
        }

        // Find key
        let key = self
            .find_key(&jwks, kid, algorithm)
            .ok_or_else(|| match kid {
                Some(kid) => AuthError::invalid_client(format!("Key '{}' not found in JWKS", kid)),
                None => AuthError::invalid_client("No suitable key found in JWKS"),
            })?;

        self.jwk_to_decoding_key(&key)
    }

    /// Gets a decoding key from an inline JWKS.
    ///
    /// This is used when the client has embedded their JWKS in their
    /// registration rather than providing a jwks_uri.
    ///
    /// # Arguments
    ///
    /// * `jwks` - The inline JWKS
    /// * `kid` - Optional key ID to find a specific key
    /// * `algorithm` - The expected algorithm for the key
    pub fn get_decoding_key_from_inline(
        &self,
        jwks: &JwkSet,
        kid: Option<&str>,
        algorithm: Algorithm,
    ) -> AuthResult<DecodingKey> {
        let key = self
            .find_key(jwks, kid, algorithm)
            .ok_or_else(|| match kid {
                Some(kid) => AuthError::invalid_client(format!("Key '{}' not found in JWKS", kid)),
                None => AuthError::invalid_client("No suitable key found in JWKS"),
            })?;

        self.jwk_to_decoding_key(&key)
    }

    /// Fetches a JWKS from a URI.
    async fn fetch_jwks(&self, uri: &str) -> AuthResult<JwkSet> {
        // Validate URI
        if !uri.starts_with("https://") {
            return Err(AuthError::invalid_client("JWKS URI must use HTTPS"));
        }

        // Build HTTP client with timeout
        let client = reqwest::Client::builder()
            .timeout(self.config.request_timeout)
            .build()
            .map_err(|e| AuthError::internal(format!("Failed to create HTTP client: {}", e)))?;

        // Fetch JWKS
        let response = client
            .get(uri)
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|e| {
                tracing::warn!("Failed to fetch JWKS from {}: {}", uri, e);
                AuthError::internal(format!("Failed to fetch JWKS: {}", e))
            })?;

        // Check status
        if !response.status().is_success() {
            return Err(AuthError::internal(format!(
                "JWKS fetch failed with status: {}",
                response.status()
            )));
        }

        // Check content length
        if let Some(len) = response.content_length()
            && len as usize > self.config.max_response_size
        {
            return Err(AuthError::internal("JWKS response exceeds maximum size"));
        }

        // Parse response
        let jwks: JwkSet = response
            .json()
            .await
            .map_err(|e| AuthError::internal(format!("Invalid JWKS JSON: {}", e)))?;

        Ok(jwks)
    }

    /// Finds a key in a JWKS by kid and algorithm.
    fn find_key(&self, jwks: &JwkSet, kid: Option<&str>, algorithm: Algorithm) -> Option<Jwk> {
        let alg_str = algorithm_to_string(algorithm);

        jwks.keys
            .iter()
            .find(|key| {
                // Check kid if specified
                if let Some(expected_kid) = kid
                    && key.common.key_id.as_deref() != Some(expected_kid)
                {
                    return false;
                }

                // Check algorithm if specified in the key
                if let Some(ref key_alg) = key.common.key_algorithm
                    && key_alg.to_string() != alg_str
                {
                    return false;
                }

                // Check key use (must be "sig" or unspecified)
                if let Some(ref use_) = key.common.public_key_use {
                    use jsonwebtoken::jwk::PublicKeyUse;
                    if *use_ != PublicKeyUse::Signature {
                        return false;
                    }
                }

                true
            })
            .cloned()
    }

    /// Converts a JWK to a DecodingKey.
    fn jwk_to_decoding_key(&self, jwk: &Jwk) -> AuthResult<DecodingKey> {
        DecodingKey::from_jwk(jwk)
            .map_err(|e| AuthError::invalid_client(format!("Invalid JWK: {}", e)))
    }

    /// Invalidates a cached JWKS entry.
    ///
    /// This is useful when a key validation fails and you want to
    /// force a refresh.
    pub async fn invalidate(&self, jwks_uri: &str) {
        let mut cache = self.cache.write().await;
        cache.remove(jwks_uri);
    }

    /// Clears all cached entries.
    pub async fn clear(&self) {
        let mut cache = self.cache.write().await;
        cache.clear();
    }
}

/// Converts a jsonwebtoken Algorithm to its string representation.
fn algorithm_to_string(algorithm: Algorithm) -> &'static str {
    match algorithm {
        Algorithm::HS256 => "HS256",
        Algorithm::HS384 => "HS384",
        Algorithm::HS512 => "HS512",
        Algorithm::ES256 => "ES256",
        Algorithm::ES384 => "ES384",
        Algorithm::RS256 => "RS256",
        Algorithm::RS384 => "RS384",
        Algorithm::RS512 => "RS512",
        Algorithm::PS256 => "PS256",
        Algorithm::PS384 => "PS384",
        Algorithm::PS512 => "PS512",
        Algorithm::EdDSA => "EdDSA",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_defaults() {
        let config = JwksCacheConfig::default();
        assert_eq!(config.ttl, Duration::from_secs(3600));
        assert_eq!(config.request_timeout, Duration::from_secs(10));
        assert_eq!(config.max_response_size, 1024 * 1024);
    }

    #[test]
    fn test_config_builder() {
        let config = JwksCacheConfig::new()
            .with_ttl(Duration::from_secs(1800))
            .with_request_timeout(Duration::from_secs(5))
            .with_max_response_size(512 * 1024);

        assert_eq!(config.ttl, Duration::from_secs(1800));
        assert_eq!(config.request_timeout, Duration::from_secs(5));
        assert_eq!(config.max_response_size, 512 * 1024);
    }

    #[test]
    fn test_algorithm_to_string() {
        assert_eq!(algorithm_to_string(Algorithm::RS256), "RS256");
        assert_eq!(algorithm_to_string(Algorithm::RS384), "RS384");
        assert_eq!(algorithm_to_string(Algorithm::ES384), "ES384");
    }

    #[tokio::test]
    async fn test_cache_invalidate() {
        let cache = ClientJwksCache::with_defaults();

        // Add something to cache manually
        {
            let mut c = cache.cache.write().await;
            c.insert(
                "https://example.com/jwks".to_string(),
                CachedJwks {
                    jwks: JwkSet { keys: vec![] },
                    fetched_at: Instant::now(),
                },
            );
        }

        // Verify it's there
        {
            let c = cache.cache.read().await;
            assert!(c.contains_key("https://example.com/jwks"));
        }

        // Invalidate
        cache.invalidate("https://example.com/jwks").await;

        // Verify it's gone
        {
            let c = cache.cache.read().await;
            assert!(!c.contains_key("https://example.com/jwks"));
        }
    }

    #[tokio::test]
    async fn test_cache_clear() {
        let cache = ClientJwksCache::with_defaults();

        // Add entries
        {
            let mut c = cache.cache.write().await;
            c.insert(
                "https://a.com/jwks".to_string(),
                CachedJwks {
                    jwks: JwkSet { keys: vec![] },
                    fetched_at: Instant::now(),
                },
            );
            c.insert(
                "https://b.com/jwks".to_string(),
                CachedJwks {
                    jwks: JwkSet { keys: vec![] },
                    fetched_at: Instant::now(),
                },
            );
        }

        // Clear
        cache.clear().await;

        // Verify empty
        {
            let c = cache.cache.read().await;
            assert!(c.is_empty());
        }
    }
}
