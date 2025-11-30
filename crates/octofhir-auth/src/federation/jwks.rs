//! Provider JWKS fetching and caching.
//!
//! This module provides caching for JSON Web Key Sets (JWKS) from external
//! identity providers, used for validating ID tokens and access tokens in
//! federated authentication flows.
//!
//! # Overview
//!
//! When validating tokens from external OIDC providers, we need their public
//! keys to verify signatures. This module provides:
//!
//! - [`ProviderJwksCache`] - Fetches and caches JWKS from provider endpoints
//! - [`ProviderJwksCacheConfig`] - Configuration for cache behavior
//! - [`JwksError`] - Error types for JWKS operations
//!
//! # Cache-Control Support
//!
//! The cache respects `Cache-Control: max-age=X` headers from providers,
//! allowing dynamic TTL based on provider recommendations. The TTL is
//! constrained by configurable minimum and maximum bounds.
//!
//! # Example
//!
//! ```ignore
//! use octofhir_auth::federation::jwks::{ProviderJwksCache, ProviderJwksCacheConfig};
//! use url::Url;
//!
//! let cache = ProviderJwksCache::new(ProviderJwksCacheConfig::default());
//! let jwks_uri = Url::parse("https://auth.example.com/.well-known/jwks.json")?;
//!
//! // Get a specific key by kid
//! let key = cache.get_key(&jwks_uri, "key-1").await?;
//!
//! // Or get all signing keys (for tokens without kid)
//! let keys = cache.find_signing_keys(&jwks_uri).await?;
//! ```
//!
//! # Security Considerations
//!
//! - Only HTTPS URIs are allowed for JWKS endpoints (configurable for testing)
//! - HTTP timeouts prevent hanging on slow endpoints
//! - Response size is limited to prevent DoS attacks
//! - TTL is bounded to prevent cache poisoning via malicious Cache-Control
//!
//! # Difference from ClientJwksCache
//!
//! - [`ProviderJwksCache`] - For validating tokens **from** external providers
//! - [`ClientJwksCache`] - For verifying client assertions in `private_key_jwt` auth

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use jsonwebtoken::jwk::{JwkSet, PublicKeyUse};
use jsonwebtoken::{Algorithm, DecodingKey};
use tokio::sync::RwLock;
use url::Url;

/// Configuration for the provider JWKS cache.
#[derive(Debug, Clone)]
pub struct ProviderJwksCacheConfig {
    /// Default TTL when Cache-Control header is absent (default: 1 hour).
    pub default_ttl: Duration,

    /// Maximum TTL regardless of Cache-Control (default: 24 hours).
    pub max_ttl: Duration,

    /// Minimum TTL regardless of Cache-Control (default: 5 minutes).
    pub min_ttl: Duration,

    /// HTTP request timeout (default: 10 seconds).
    pub request_timeout: Duration,

    /// Maximum response size in bytes (default: 1 MB).
    pub max_response_size: usize,

    /// Whether to allow HTTP (non-HTTPS) JWKS URIs.
    /// This should only be enabled for testing.
    pub allow_http: bool,
}

impl Default for ProviderJwksCacheConfig {
    fn default() -> Self {
        Self {
            default_ttl: Duration::from_secs(3600),   // 1 hour
            max_ttl: Duration::from_secs(86400),      // 24 hours
            min_ttl: Duration::from_secs(300),        // 5 minutes
            request_timeout: Duration::from_secs(10), // 10 seconds
            max_response_size: 1024 * 1024,           // 1 MB
            allow_http: false,
        }
    }
}

impl ProviderJwksCacheConfig {
    /// Creates a new configuration with default values.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the default TTL (used when Cache-Control is absent).
    #[must_use]
    pub fn with_default_ttl(mut self, ttl: Duration) -> Self {
        self.default_ttl = ttl;
        self
    }

    /// Sets the maximum TTL.
    #[must_use]
    pub fn with_max_ttl(mut self, ttl: Duration) -> Self {
        self.max_ttl = ttl;
        self
    }

    /// Sets the minimum TTL.
    #[must_use]
    pub fn with_min_ttl(mut self, ttl: Duration) -> Self {
        self.min_ttl = ttl;
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

    /// Allows HTTP (non-HTTPS) JWKS URIs.
    ///
    /// # Warning
    ///
    /// This should only be used for testing. In production, JWKS endpoints
    /// should always use HTTPS.
    #[must_use]
    pub fn with_allow_http(mut self, allow: bool) -> Self {
        self.allow_http = allow;
        self
    }
}

/// Errors that can occur during JWKS operations.
#[derive(Debug, thiserror::Error)]
pub enum JwksError {
    /// A network error occurred while fetching the JWKS.
    #[error("Network error: {0}")]
    NetworkError(String),

    /// The HTTP request returned a non-success status code.
    #[error("HTTP error: status {0}")]
    HttpError(u16),

    /// The JWKS response could not be parsed as JSON.
    #[error("Failed to parse JWKS: {0}")]
    ParseError(String),

    /// The requested key was not found in the JWKS.
    #[error("Key not found: {0}")]
    KeyNotFound(String),

    /// No signing keys were found in the JWKS.
    #[error("No signing keys found in JWKS")]
    NoSigningKeys,

    /// The key could not be converted to a decoding key.
    #[error("Invalid key: {0}")]
    InvalidKey(String),

    /// The JWKS URI scheme is not allowed (must be HTTPS in production).
    #[error("Invalid URL scheme: only HTTPS is allowed")]
    InvalidScheme,

    /// The response exceeded the maximum allowed size.
    #[error("Response exceeds maximum size of {max_size} bytes")]
    ResponseTooLarge {
        /// The maximum allowed size.
        max_size: usize,
    },
}

/// Cached JWKS entry with metadata.
struct CachedJwks {
    /// The cached JWKS.
    jwks: JwkSet,
    /// When this entry expires.
    expires_at: Instant,
}

/// In-memory cache for provider JWKS.
///
/// This cache stores JWKS fetched from external identity provider endpoints
/// and provides key lookup by `kid` (key ID) for token validation.
///
/// # Features
///
/// - Automatic caching with TTL from Cache-Control headers
/// - Configurable TTL bounds (min/max)
/// - Key lookup by kid or all signing keys
/// - Automatic refresh on cache miss
/// - Manual invalidation and cleanup
pub struct ProviderJwksCache {
    /// HTTP client for fetching JWKS.
    http_client: reqwest::Client,
    /// Cached JWKS by URI.
    cache: Arc<RwLock<HashMap<String, CachedJwks>>>,
    /// Configuration.
    config: ProviderJwksCacheConfig,
}

impl ProviderJwksCache {
    /// Creates a new provider JWKS cache with the specified configuration.
    ///
    /// # Panics
    ///
    /// Panics if the HTTP client cannot be created (should not happen in practice).
    #[must_use]
    pub fn new(config: ProviderJwksCacheConfig) -> Self {
        let http_client = reqwest::Client::builder()
            .timeout(config.request_timeout)
            .build()
            .expect("Failed to create HTTP client");

        Self {
            http_client,
            cache: Arc::new(RwLock::new(HashMap::new())),
            config,
        }
    }

    /// Creates a new provider JWKS cache with default configuration.
    #[must_use]
    pub fn with_defaults() -> Self {
        Self::new(ProviderJwksCacheConfig::default())
    }

    /// Gets a decoding key by key ID from a JWKS endpoint.
    ///
    /// This method checks the cache first. If the key is not found or the
    /// cache has expired, it fetches a fresh JWKS from the endpoint.
    ///
    /// # Arguments
    ///
    /// * `jwks_uri` - The URI of the JWKS endpoint
    /// * `kid` - The key ID to look up
    ///
    /// # Returns
    ///
    /// Returns a tuple of the `DecodingKey` and optional `Algorithm` for the key.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The JWKS cannot be fetched
    /// - The key with the specified kid is not found
    /// - The key cannot be converted to a decoding key
    pub async fn get_key(
        &self,
        jwks_uri: &Url,
        kid: &str,
    ) -> Result<(DecodingKey, Option<Algorithm>), JwksError> {
        // Check cache first
        if let Some(result) = self.get_cached_key(jwks_uri, kid).await {
            tracing::trace!("Cache hit for JWKS key: {} from {}", kid, jwks_uri);
            return Ok(result);
        }

        // Fetch fresh JWKS
        tracing::debug!("Cache miss for JWKS key: {} from {}", kid, jwks_uri);
        self.refresh(jwks_uri).await?;

        // Try cache again
        self.get_cached_key(jwks_uri, kid)
            .await
            .ok_or_else(|| JwksError::KeyNotFound(kid.to_string()))
    }

    /// Gets a decoding key from cache without fetching.
    async fn get_cached_key(
        &self,
        jwks_uri: &Url,
        kid: &str,
    ) -> Option<(DecodingKey, Option<Algorithm>)> {
        let cache = self.cache.read().await;
        let key = normalize_uri(jwks_uri);

        cache.get(&key).and_then(|cached| {
            // Check if expired
            if Instant::now() >= cached.expires_at {
                return None;
            }

            // Find key by kid
            cached
                .jwks
                .keys
                .iter()
                .find(|k| k.common.key_id.as_deref() == Some(kid))
                .and_then(|jwk| {
                    DecodingKey::from_jwk(jwk)
                        .ok()
                        .map(|dk| (dk, jwk_algorithm(jwk)))
                })
        })
    }

    /// Gets all signing keys from a JWKS endpoint.
    ///
    /// This is useful when the token doesn't have a `kid` header and you need
    /// to try multiple keys to find one that validates the signature.
    ///
    /// # Arguments
    ///
    /// * `jwks_uri` - The URI of the JWKS endpoint
    ///
    /// # Returns
    ///
    /// Returns a vector of (DecodingKey, Algorithm) tuples for all signing keys.
    /// Keys with `use: "enc"` (encryption) are excluded.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The JWKS cannot be fetched
    /// - No signing keys are found
    pub async fn find_signing_keys(
        &self,
        jwks_uri: &Url,
    ) -> Result<Vec<(DecodingKey, Option<Algorithm>)>, JwksError> {
        // Ensure cache is fresh
        self.ensure_cached(jwks_uri).await?;

        let cache = self.cache.read().await;
        let key = normalize_uri(jwks_uri);

        let cached = cache
            .get(&key)
            .ok_or_else(|| JwksError::NetworkError("Cache miss after refresh".to_string()))?;

        let keys: Vec<_> = cached
            .jwks
            .keys
            .iter()
            .filter(|k| {
                // Exclude encryption keys (use != "enc")
                !matches!(&k.common.public_key_use, Some(PublicKeyUse::Encryption))
            })
            .filter_map(|jwk| {
                DecodingKey::from_jwk(jwk)
                    .ok()
                    .map(|dk| (dk, jwk_algorithm(jwk)))
            })
            .collect();

        if keys.is_empty() {
            Err(JwksError::NoSigningKeys)
        } else {
            tracing::debug!("Found {} signing keys from {}", keys.len(), jwks_uri);
            Ok(keys)
        }
    }

    /// Ensures the cache has a fresh entry for the given URI.
    async fn ensure_cached(&self, jwks_uri: &Url) -> Result<(), JwksError> {
        let key = normalize_uri(jwks_uri);

        {
            let cache = self.cache.read().await;
            if let Some(cached) = cache.get(&key)
                && Instant::now() < cached.expires_at
            {
                return Ok(());
            }
        }

        self.refresh(jwks_uri).await
    }

    /// Fetches JWKS from the endpoint and updates the cache.
    ///
    /// This method always fetches a fresh JWKS, regardless of cache state.
    ///
    /// # Arguments
    ///
    /// * `jwks_uri` - The URI of the JWKS endpoint
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The URI scheme is not HTTPS (unless `allow_http` is configured)
    /// - The HTTP request fails
    /// - The response cannot be parsed as JWKS
    pub async fn refresh(&self, jwks_uri: &Url) -> Result<(), JwksError> {
        // Validate scheme
        self.validate_scheme(jwks_uri)?;

        tracing::debug!("Fetching JWKS from {}", jwks_uri);

        let response = self
            .http_client
            .get(jwks_uri.as_str())
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|e| {
                tracing::warn!("Failed to fetch JWKS from {}: {}", jwks_uri, e);
                JwksError::NetworkError(e.to_string())
            })?;

        // Check status
        if !response.status().is_success() {
            return Err(JwksError::HttpError(response.status().as_u16()));
        }

        // Check content length
        if let Some(len) = response.content_length()
            && len as usize > self.config.max_response_size
        {
            return Err(JwksError::ResponseTooLarge {
                max_size: self.config.max_response_size,
            });
        }

        // Parse Cache-Control for TTL
        let ttl = self.parse_cache_control(response.headers());

        // Parse JWKS
        let jwks: JwkSet = response.json().await.map_err(|e| {
            tracing::warn!("Failed to parse JWKS from {}: {}", jwks_uri, e);
            JwksError::ParseError(e.to_string())
        })?;

        tracing::debug!(
            "Cached JWKS from {} with {} keys, TTL {:?}",
            jwks_uri,
            jwks.keys.len(),
            ttl
        );

        // Update cache
        let now = Instant::now();
        let key = normalize_uri(jwks_uri);

        let mut cache = self.cache.write().await;
        cache.insert(
            key,
            CachedJwks {
                jwks,
                expires_at: now + ttl,
            },
        );

        Ok(())
    }

    /// Validates that the URI uses an allowed scheme.
    fn validate_scheme(&self, uri: &Url) -> Result<(), JwksError> {
        let scheme = uri.scheme();

        if scheme == "https" {
            return Ok(());
        }

        if scheme == "http" && self.config.allow_http {
            return Ok(());
        }

        Err(JwksError::InvalidScheme)
    }

    /// Parses Cache-Control header to determine TTL.
    ///
    /// Extracts `max-age` directive and clamps it between `min_ttl` and `max_ttl`.
    /// Returns `default_ttl` if no Cache-Control header or max-age is present.
    fn parse_cache_control(&self, headers: &reqwest::header::HeaderMap) -> Duration {
        let ttl = headers
            .get(reqwest::header::CACHE_CONTROL)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| {
                v.split(',').find_map(|directive| {
                    let directive = directive.trim();
                    if let Some(stripped) = directive.strip_prefix("max-age=") {
                        stripped.parse::<u64>().ok()
                    } else {
                        None
                    }
                })
            })
            .map(Duration::from_secs)
            .unwrap_or(self.config.default_ttl);

        // Clamp to configured bounds
        ttl.min(self.config.max_ttl).max(self.config.min_ttl)
    }

    /// Invalidates a cached JWKS entry.
    ///
    /// This forces the next `get_key` or `find_signing_keys` call to fetch
    /// a fresh JWKS from the endpoint.
    pub async fn invalidate(&self, jwks_uri: &Url) {
        let key = normalize_uri(jwks_uri);
        let mut cache = self.cache.write().await;
        cache.remove(&key);
        tracing::debug!("Invalidated JWKS cache for {}", jwks_uri);
    }

    /// Clears all expired entries from the cache.
    ///
    /// This is useful for periodic cleanup to free memory.
    pub async fn cleanup(&self) {
        let mut cache = self.cache.write().await;
        let now = Instant::now();
        let before_count = cache.len();

        cache.retain(|_, v| v.expires_at > now);

        let removed = before_count - cache.len();
        if removed > 0 {
            tracing::debug!("Cleaned up {} expired JWKS cache entries", removed);
        }
    }

    /// Clears all entries from the cache.
    pub async fn clear(&self) {
        let mut cache = self.cache.write().await;
        cache.clear();
        tracing::debug!("Cleared all JWKS cache entries");
    }

    /// Returns the number of entries in the cache.
    pub async fn len(&self) -> usize {
        self.cache.read().await.len()
    }

    /// Returns `true` if the cache is empty.
    pub async fn is_empty(&self) -> bool {
        self.cache.read().await.is_empty()
    }
}

/// Normalizes a URI for use as a cache key.
fn normalize_uri(uri: &Url) -> String {
    uri.as_str().trim_end_matches('/').to_string()
}

/// Extracts the algorithm from a JWK.
fn jwk_algorithm(jwk: &jsonwebtoken::jwk::Jwk) -> Option<Algorithm> {
    jwk.common.key_algorithm.as_ref().and_then(|alg| match alg {
        jsonwebtoken::jwk::KeyAlgorithm::RS256 => Some(Algorithm::RS256),
        jsonwebtoken::jwk::KeyAlgorithm::RS384 => Some(Algorithm::RS384),
        jsonwebtoken::jwk::KeyAlgorithm::RS512 => Some(Algorithm::RS512),
        jsonwebtoken::jwk::KeyAlgorithm::ES256 => Some(Algorithm::ES256),
        jsonwebtoken::jwk::KeyAlgorithm::ES384 => Some(Algorithm::ES384),
        jsonwebtoken::jwk::KeyAlgorithm::PS256 => Some(Algorithm::PS256),
        jsonwebtoken::jwk::KeyAlgorithm::PS384 => Some(Algorithm::PS384),
        jsonwebtoken::jwk::KeyAlgorithm::PS512 => Some(Algorithm::PS512),
        jsonwebtoken::jwk::KeyAlgorithm::EdDSA => Some(Algorithm::EdDSA),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_defaults() {
        let config = ProviderJwksCacheConfig::default();
        assert_eq!(config.default_ttl, Duration::from_secs(3600));
        assert_eq!(config.max_ttl, Duration::from_secs(86400));
        assert_eq!(config.min_ttl, Duration::from_secs(300));
        assert_eq!(config.request_timeout, Duration::from_secs(10));
        assert_eq!(config.max_response_size, 1024 * 1024);
        assert!(!config.allow_http);
    }

    #[test]
    fn test_config_builder() {
        let config = ProviderJwksCacheConfig::new()
            .with_default_ttl(Duration::from_secs(1800))
            .with_max_ttl(Duration::from_secs(7200))
            .with_min_ttl(Duration::from_secs(60))
            .with_request_timeout(Duration::from_secs(5))
            .with_max_response_size(512 * 1024)
            .with_allow_http(true);

        assert_eq!(config.default_ttl, Duration::from_secs(1800));
        assert_eq!(config.max_ttl, Duration::from_secs(7200));
        assert_eq!(config.min_ttl, Duration::from_secs(60));
        assert_eq!(config.request_timeout, Duration::from_secs(5));
        assert_eq!(config.max_response_size, 512 * 1024);
        assert!(config.allow_http);
    }

    #[test]
    fn test_validate_scheme() {
        let config = ProviderJwksCacheConfig::default();
        let cache = ProviderJwksCache::new(config);

        let https = Url::parse("https://example.com/jwks").unwrap();
        assert!(cache.validate_scheme(&https).is_ok());

        let http = Url::parse("http://example.com/jwks").unwrap();
        assert!(cache.validate_scheme(&http).is_err());

        // With allow_http
        let config = ProviderJwksCacheConfig::default().with_allow_http(true);
        let cache = ProviderJwksCache::new(config);
        assert!(cache.validate_scheme(&http).is_ok());
    }

    #[test]
    fn test_parse_cache_control() {
        let config = ProviderJwksCacheConfig::default()
            .with_default_ttl(Duration::from_secs(3600))
            .with_min_ttl(Duration::from_secs(60))
            .with_max_ttl(Duration::from_secs(7200));
        let cache = ProviderJwksCache::new(config);

        // No header - use default
        let headers = reqwest::header::HeaderMap::new();
        assert_eq!(
            cache.parse_cache_control(&headers),
            Duration::from_secs(3600)
        );

        // max-age present
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::CACHE_CONTROL,
            "public, max-age=1800".parse().unwrap(),
        );
        assert_eq!(
            cache.parse_cache_control(&headers),
            Duration::from_secs(1800)
        );

        // max-age below min - clamped to min
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::CACHE_CONTROL,
            "max-age=30".parse().unwrap(),
        );
        assert_eq!(cache.parse_cache_control(&headers), Duration::from_secs(60));

        // max-age above max - clamped to max
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::CACHE_CONTROL,
            "max-age=100000".parse().unwrap(),
        );
        assert_eq!(
            cache.parse_cache_control(&headers),
            Duration::from_secs(7200)
        );

        // Invalid max-age - use default
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::CACHE_CONTROL,
            "max-age=invalid".parse().unwrap(),
        );
        assert_eq!(
            cache.parse_cache_control(&headers),
            Duration::from_secs(3600)
        );
    }

    #[test]
    fn test_normalize_uri() {
        let uri1 = Url::parse("https://example.com/jwks").unwrap();
        let uri2 = Url::parse("https://example.com/jwks/").unwrap();
        assert_eq!(normalize_uri(&uri1), normalize_uri(&uri2));
        assert_eq!(normalize_uri(&uri1), "https://example.com/jwks");
    }

    #[tokio::test]
    async fn test_cache_invalidate() {
        let config = ProviderJwksCacheConfig::default().with_allow_http(true);
        let cache = ProviderJwksCache::new(config);

        // Manually add entry
        {
            let mut c = cache.cache.write().await;
            c.insert(
                "https://example.com/jwks".to_string(),
                CachedJwks {
                    jwks: JwkSet { keys: vec![] },
                    expires_at: Instant::now() + Duration::from_secs(3600),
                },
            );
        }

        assert_eq!(cache.len().await, 1);

        let uri = Url::parse("https://example.com/jwks").unwrap();
        cache.invalidate(&uri).await;

        assert!(cache.is_empty().await);
    }

    #[tokio::test]
    async fn test_cache_clear() {
        let config = ProviderJwksCacheConfig::default().with_allow_http(true);
        let cache = ProviderJwksCache::new(config);

        // Add entries
        {
            let mut c = cache.cache.write().await;
            c.insert(
                "https://a.example.com/jwks".to_string(),
                CachedJwks {
                    jwks: JwkSet { keys: vec![] },
                    expires_at: Instant::now() + Duration::from_secs(3600),
                },
            );
            c.insert(
                "https://b.example.com/jwks".to_string(),
                CachedJwks {
                    jwks: JwkSet { keys: vec![] },
                    expires_at: Instant::now() + Duration::from_secs(3600),
                },
            );
        }

        assert_eq!(cache.len().await, 2);

        cache.clear().await;

        assert!(cache.is_empty().await);
    }

    #[tokio::test]
    async fn test_cache_cleanup() {
        let config = ProviderJwksCacheConfig::default().with_allow_http(true);
        let cache = ProviderJwksCache::new(config);

        // Add expired entry
        {
            let mut c = cache.cache.write().await;
            c.insert(
                "https://expired.example.com/jwks".to_string(),
                CachedJwks {
                    jwks: JwkSet { keys: vec![] },
                    expires_at: Instant::now() - Duration::from_secs(3600), // Already expired
                },
            );
            c.insert(
                "https://valid.example.com/jwks".to_string(),
                CachedJwks {
                    jwks: JwkSet { keys: vec![] },
                    expires_at: Instant::now() + Duration::from_secs(3600),
                },
            );
        }

        assert_eq!(cache.len().await, 2);

        cache.cleanup().await;

        assert_eq!(cache.len().await, 1);
    }

    #[test]
    fn test_jwks_error_display() {
        let err = JwksError::NetworkError("connection refused".to_string());
        assert_eq!(err.to_string(), "Network error: connection refused");

        let err = JwksError::HttpError(404);
        assert_eq!(err.to_string(), "HTTP error: status 404");

        let err = JwksError::KeyNotFound("key-1".to_string());
        assert_eq!(err.to_string(), "Key not found: key-1");

        let err = JwksError::NoSigningKeys;
        assert_eq!(err.to_string(), "No signing keys found in JWKS");

        let err = JwksError::InvalidScheme;
        assert_eq!(err.to_string(), "Invalid URL scheme: only HTTPS is allowed");

        let err = JwksError::ResponseTooLarge { max_size: 1024 };
        assert_eq!(
            err.to_string(),
            "Response exceeds maximum size of 1024 bytes"
        );
    }
}

// Integration tests with wiremock are commented out since wiremock is not in dev-dependencies.
// To enable, add `wiremock = "0.5"` to dev-dependencies and uncomment.
//
// #[cfg(test)]
// mod integration_tests {
//     use super::*;
//     use wiremock::matchers::{method, path};
//     use wiremock::{Mock, MockServer, ResponseTemplate};
//
//     fn create_test_jwks() -> serde_json::Value {
//         serde_json::json!({
//             "keys": [
//                 {
//                     "kty": "RSA",
//                     "kid": "key-1",
//                     "use": "sig",
//                     "alg": "RS256",
//                     "n": "0vx7agoebGcQSuuPiLJXZptN9nndrQmbXEps2aiAFbWhM78LhWx4cbbfAAtVT86zwu1RK7aPFFxuhDR1L6tSoc_BJECPebWKRXjBZCiFV4n3oknjhMstn64tZ_2W-5JsGY4Hc5n9yBXArwl93lqt7_RN5w6Cf0h4QyQ5v-65YGjQR0_FDW2QvzqY368QQMicAtaSqzs8KJZgnYb9c7d0zgdAZHzu6qMQvRL5hajrn1n91CbOpbISD08qNLyrdkt-bFTWhAI4vMQFh6WeZu0fM4lFd2NcRwr3XPksINHaQ-G_xBniIqbw0Ls1jF44-csFCur-kEgU8awapJzKnqDKgw",
//                     "e": "AQAB"
//                 },
//                 {
//                     "kty": "RSA",
//                     "kid": "key-2",
//                     "use": "sig",
//                     "alg": "RS384",
//                     "n": "0vx7agoebGcQSuuPiLJXZptN9nndrQmbXEps2aiAFbWhM78LhWx4cbbfAAtVT86zwu1RK7aPFFxuhDR1L6tSoc_BJECPebWKRXjBZCiFV4n3oknjhMstn64tZ_2W-5JsGY4Hc5n9yBXArwl93lqt7_RN5w6Cf0h4QyQ5v-65YGjQR0_FDW2QvzqY368QQMicAtaSqzs8KJZgnYb9c7d0zgdAZHzu6qMQvRL5hajrn1n91CbOpbISD08qNLyrdkt-bFTWhAI4vMQFh6WeZu0fM4lFd2NcRwr3XPksINHaQ-G_xBniIqbw0Ls1jF44-csFCur-kEgU8awapJzKnqDKgw",
//                     "e": "AQAB"
//                 },
//                 {
//                     "kty": "RSA",
//                     "kid": "enc-key",
//                     "use": "enc",
//                     "alg": "RSA-OAEP",
//                     "n": "0vx7agoebGcQSuuPiLJXZptN9nndrQmbXEps2aiAFbWhM78LhWx4cbbfAAtVT86zwu1RK7aPFFxuhDR1L6tSoc_BJECPebWKRXjBZCiFV4n3oknjhMstn64tZ_2W-5JsGY4Hc5n9yBXArwl93lqt7_RN5w6Cf0h4QyQ5v-65YGjQR0_FDW2QvzqY368QQMicAtaSqzs8KJZgnYb9c7d0zgdAZHzu6qMQvRL5hajrn1n91CbOpbISD08qNLyrdkt-bFTWhAI4vMQFh6WeZu0fM4lFd2NcRwr3XPksINHaQ-G_xBniIqbw0Ls1jF44-csFCur-kEgU8awapJzKnqDKgw",
//                     "e": "AQAB"
//                 }
//             ]
//         })
//     }
//
//     #[tokio::test]
//     async fn test_get_key_by_kid() {
//         let mock_server = MockServer::start().await;
//
//         Mock::given(method("GET"))
//             .and(path("/.well-known/jwks.json"))
//             .respond_with(
//                 ResponseTemplate::new(200)
//                     .set_body_json(create_test_jwks())
//                     .insert_header("Cache-Control", "max-age=3600")
//             )
//             .mount(&mock_server)
//             .await;
//
//         let config = ProviderJwksCacheConfig::default().with_allow_http(true);
//         let cache = ProviderJwksCache::new(config);
//         let jwks_uri = Url::parse(&format!("{}/.well-known/jwks.json", mock_server.uri())).unwrap();
//
//         let (_, alg) = cache.get_key(&jwks_uri, "key-1").await.unwrap();
//         assert_eq!(alg, Some(Algorithm::RS256));
//     }
//
//     #[tokio::test]
//     async fn test_find_signing_keys() {
//         let mock_server = MockServer::start().await;
//
//         Mock::given(method("GET"))
//             .and(path("/.well-known/jwks.json"))
//             .respond_with(
//                 ResponseTemplate::new(200)
//                     .set_body_json(create_test_jwks())
//             )
//             .mount(&mock_server)
//             .await;
//
//         let config = ProviderJwksCacheConfig::default().with_allow_http(true);
//         let cache = ProviderJwksCache::new(config);
//         let jwks_uri = Url::parse(&format!("{}/.well-known/jwks.json", mock_server.uri())).unwrap();
//
//         let keys = cache.find_signing_keys(&jwks_uri).await.unwrap();
//         // Should have 2 signing keys, not the encryption key
//         assert_eq!(keys.len(), 2);
//     }
//
//     #[tokio::test]
//     async fn test_cache_respects_cache_control() {
//         let mock_server = MockServer::start().await;
//
//         Mock::given(method("GET"))
//             .and(path("/.well-known/jwks.json"))
//             .respond_with(
//                 ResponseTemplate::new(200)
//                     .set_body_json(create_test_jwks())
//                     .insert_header("Cache-Control", "max-age=3600")
//             )
//             .expect(1) // Only one request
//             .mount(&mock_server)
//             .await;
//
//         let config = ProviderJwksCacheConfig::default().with_allow_http(true);
//         let cache = ProviderJwksCache::new(config);
//         let jwks_uri = Url::parse(&format!("{}/.well-known/jwks.json", mock_server.uri())).unwrap();
//
//         // First call fetches
//         let _ = cache.get_key(&jwks_uri, "key-1").await.unwrap();
//         // Second call uses cache
//         let _ = cache.get_key(&jwks_uri, "key-2").await.unwrap();
//     }
// }
