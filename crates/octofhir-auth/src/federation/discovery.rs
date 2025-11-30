//! OpenID Connect Discovery client and caching.
//!
//! This module provides functionality to fetch and cache OIDC provider metadata
//! from `.well-known/openid-configuration` endpoints.
//!
//! # Overview
//!
//! The OIDC Discovery protocol allows clients to discover OpenID Provider metadata
//! by fetching a JSON document from a well-known location. This module provides:
//!
//! - [`OidcDiscoveryClient`] - Fetches discovery documents from providers
//! - [`DiscoveryCache`] - Caches discovery documents with configurable TTL
//! - [`DiscoveryError`] - Error types for discovery operations
//!
//! # Example
//!
//! ```ignore
//! use octofhir_auth::federation::discovery::{DiscoveryCache, DiscoveryCacheConfig};
//! use url::Url;
//! use std::time::Duration;
//!
//! let cache = DiscoveryCache::new(DiscoveryCacheConfig::default());
//! let issuer = Url::parse("https://auth.example.com")?;
//!
//! let doc = cache.get(&issuer).await?;
//! println!("Token endpoint: {}", doc.token_endpoint);
//! ```
//!
//! # Security Considerations
//!
//! - Only HTTPS URIs are allowed for issuer URLs (except in tests)
//! - The issuer claim in the discovery document must match the expected issuer URL
//! - HTTP timeouts prevent hanging on slow endpoints
//! - Response size is limited to prevent DoS attacks
//!
//! # References
//!
//! - [OpenID Connect Discovery 1.0](https://openid.net/specs/openid-connect-discovery-1_0.html)
//! - [RFC 8414 - OAuth 2.0 Authorization Server Metadata](https://tools.ietf.org/html/rfc8414)

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::RwLock;
use url::Url;

use super::oidc::OidcDiscoveryDocument;

/// Configuration for the OIDC discovery cache.
#[derive(Debug, Clone)]
pub struct DiscoveryCacheConfig {
    /// Time-to-live for cached discovery documents (default: 1 hour).
    pub ttl: Duration,

    /// HTTP request timeout (default: 10 seconds).
    pub request_timeout: Duration,

    /// Maximum response size in bytes (default: 1 MB).
    pub max_response_size: usize,

    /// Whether to allow HTTP (non-HTTPS) issuer URLs.
    /// This should only be enabled for testing.
    pub allow_http: bool,
}

impl Default for DiscoveryCacheConfig {
    fn default() -> Self {
        Self {
            ttl: Duration::from_secs(3600),           // 1 hour
            request_timeout: Duration::from_secs(10), // 10 seconds
            max_response_size: 1024 * 1024,           // 1 MB
            allow_http: false,
        }
    }
}

impl DiscoveryCacheConfig {
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

    /// Allows HTTP (non-HTTPS) issuer URLs.
    ///
    /// # Warning
    ///
    /// This should only be used for testing. In production, OIDC discovery
    /// should always use HTTPS.
    #[must_use]
    pub fn with_allow_http(mut self, allow: bool) -> Self {
        self.allow_http = allow;
        self
    }
}

/// Errors that can occur during OIDC discovery.
#[derive(Debug, thiserror::Error)]
pub enum DiscoveryError {
    /// A network error occurred while fetching the discovery document.
    #[error("Network error: {0}")]
    NetworkError(String),

    /// The HTTP request returned a non-success status code.
    #[error("HTTP error: status {0}")]
    HttpError(u16),

    /// The discovery document could not be parsed as JSON.
    #[error("Failed to parse discovery document: {0}")]
    ParseError(String),

    /// The issuer URL could not be parsed or is invalid.
    #[error("Invalid issuer URL: {0}")]
    InvalidIssuer(String),

    /// The issuer in the discovery document does not match the expected issuer.
    #[error("Issuer mismatch: expected {expected}, got {actual}")]
    IssuerMismatch {
        /// The expected issuer URL.
        expected: String,
        /// The actual issuer URL from the discovery document.
        actual: String,
    },

    /// A required field is missing from the discovery document.
    #[error("Missing required field: {0}")]
    MissingField(String),

    /// The issuer URL scheme is not allowed (must be HTTPS in production).
    #[error("Invalid URL scheme: {0} (only HTTPS is allowed)")]
    InvalidScheme(String),

    /// The response exceeded the maximum allowed size.
    #[error("Response exceeds maximum size of {max_size} bytes")]
    ResponseTooLarge {
        /// The maximum allowed size.
        max_size: usize,
    },
}

/// Client for fetching OIDC discovery documents.
///
/// This client fetches OpenID Connect provider metadata from the
/// `.well-known/openid-configuration` endpoint and validates the response.
pub struct OidcDiscoveryClient {
    http_client: reqwest::Client,
    config: DiscoveryCacheConfig,
}

impl OidcDiscoveryClient {
    /// Creates a new discovery client with the specified configuration.
    ///
    /// # Panics
    ///
    /// Panics if the HTTP client cannot be created (should not happen in practice).
    #[must_use]
    pub fn new(config: DiscoveryCacheConfig) -> Self {
        let http_client = reqwest::Client::builder()
            .timeout(config.request_timeout)
            .build()
            .expect("Failed to create HTTP client");

        Self {
            http_client,
            config,
        }
    }

    /// Creates a new discovery client with default configuration.
    #[must_use]
    pub fn with_defaults() -> Self {
        Self::new(DiscoveryCacheConfig::default())
    }

    /// Discovers OIDC configuration from an issuer URL.
    ///
    /// This method:
    /// 1. Builds the discovery URL from the issuer
    /// 2. Fetches the discovery document
    /// 3. Validates that the issuer in the document matches
    ///
    /// # Arguments
    ///
    /// * `issuer` - The issuer URL (e.g., `https://auth.example.com`)
    ///
    /// # Returns
    ///
    /// Returns the parsed `OidcDiscoveryDocument` on success.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The issuer URL is not HTTPS (unless `allow_http` is true)
    /// - The discovery document cannot be fetched
    /// - The discovery document cannot be parsed
    /// - The issuer in the document does not match the expected issuer
    pub async fn discover(&self, issuer: &Url) -> Result<OidcDiscoveryDocument, DiscoveryError> {
        // Validate scheme
        self.validate_issuer_scheme(issuer)?;

        // Build discovery URL
        let discovery_url = self.build_discovery_url(issuer);

        // Fetch document
        let response = self
            .http_client
            .get(discovery_url.as_str())
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|e| {
                tracing::warn!("Failed to fetch OIDC discovery from {}: {}", issuer, e);
                DiscoveryError::NetworkError(e.to_string())
            })?;

        // Check status
        if !response.status().is_success() {
            return Err(DiscoveryError::HttpError(response.status().as_u16()));
        }

        // Check content length
        if let Some(len) = response.content_length()
            && len as usize > self.config.max_response_size
        {
            return Err(DiscoveryError::ResponseTooLarge {
                max_size: self.config.max_response_size,
            });
        }

        // Parse document
        let document: OidcDiscoveryDocument = response.json().await.map_err(|e| {
            tracing::warn!(
                "Failed to parse OIDC discovery document from {}: {}",
                issuer,
                e
            );
            DiscoveryError::ParseError(e.to_string())
        })?;

        // Validate issuer
        self.validate_issuer(&document, issuer)?;

        tracing::debug!(
            "Successfully discovered OIDC configuration for {}",
            document.issuer
        );

        Ok(document)
    }

    /// Validates that the issuer URL uses an allowed scheme.
    fn validate_issuer_scheme(&self, issuer: &Url) -> Result<(), DiscoveryError> {
        let scheme = issuer.scheme();

        if scheme == "https" {
            return Ok(());
        }

        if scheme == "http" && self.config.allow_http {
            return Ok(());
        }

        Err(DiscoveryError::InvalidScheme(scheme.to_string()))
    }

    /// Builds the discovery URL from an issuer URL.
    ///
    /// According to OIDC Discovery spec, the discovery document is located at:
    /// `{issuer}/.well-known/openid-configuration`
    fn build_discovery_url(&self, issuer: &Url) -> Url {
        let mut discovery_url = issuer.clone();

        // Append .well-known/openid-configuration to the path
        let path = issuer.path().trim_end_matches('/');
        discovery_url.set_path(&format!("{}/.well-known/openid-configuration", path));

        discovery_url
    }

    /// Validates that the issuer in the discovery document matches the expected issuer.
    ///
    /// According to OIDC Discovery spec (Section 4.3):
    /// "The issuer value returned MUST be identical to the Issuer URL that was
    /// directly used to retrieve the configuration information."
    fn validate_issuer(
        &self,
        document: &OidcDiscoveryDocument,
        expected: &Url,
    ) -> Result<(), DiscoveryError> {
        let document_issuer = Url::parse(&document.issuer).map_err(|e| {
            DiscoveryError::InvalidIssuer(format!(
                "Invalid issuer URL in document: {} - {}",
                document.issuer, e
            ))
        })?;

        // Compare normalized URLs (without trailing slash)
        let expected_normalized = expected.as_str().trim_end_matches('/');
        let document_normalized = document_issuer.as_str().trim_end_matches('/');

        if expected_normalized != document_normalized {
            return Err(DiscoveryError::IssuerMismatch {
                expected: expected_normalized.to_string(),
                actual: document_normalized.to_string(),
            });
        }

        Ok(())
    }
}

/// Cached discovery document entry.
struct CachedDiscovery {
    /// The cached discovery document.
    document: OidcDiscoveryDocument,
    /// When this entry was fetched.
    fetched_at: Instant,
}

/// In-memory cache for OIDC discovery documents.
///
/// This cache stores discovery documents fetched from issuer URLs and
/// provides automatic expiration based on TTL.
///
/// # Example
///
/// ```ignore
/// use octofhir_auth::federation::discovery::{DiscoveryCache, DiscoveryCacheConfig};
/// use url::Url;
///
/// let cache = DiscoveryCache::new(DiscoveryCacheConfig::default());
/// let issuer = Url::parse("https://auth.example.com")?;
///
/// // First call fetches from the network
/// let doc1 = cache.get(&issuer).await?;
///
/// // Second call returns cached value
/// let doc2 = cache.get(&issuer).await?;
/// ```
pub struct DiscoveryCache {
    /// The underlying client for fetching documents.
    client: OidcDiscoveryClient,
    /// Cached documents by issuer URL.
    cache: Arc<RwLock<HashMap<String, CachedDiscovery>>>,
    /// Configuration.
    config: DiscoveryCacheConfig,
}

impl DiscoveryCache {
    /// Creates a new discovery cache with the specified configuration.
    #[must_use]
    pub fn new(config: DiscoveryCacheConfig) -> Self {
        let client = OidcDiscoveryClient::new(config.clone());

        Self {
            client,
            cache: Arc::new(RwLock::new(HashMap::new())),
            config,
        }
    }

    /// Creates a new discovery cache with default configuration.
    #[must_use]
    pub fn with_defaults() -> Self {
        Self::new(DiscoveryCacheConfig::default())
    }

    /// Gets a discovery document, using the cache if available.
    ///
    /// If the document is cached and not expired, returns the cached value.
    /// Otherwise, fetches a fresh document and updates the cache.
    ///
    /// # Arguments
    ///
    /// * `issuer` - The issuer URL to get the discovery document for
    ///
    /// # Returns
    ///
    /// Returns the `OidcDiscoveryDocument` for the issuer.
    ///
    /// # Errors
    ///
    /// Returns an error if the document cannot be fetched or parsed.
    pub async fn get(&self, issuer: &Url) -> Result<OidcDiscoveryDocument, DiscoveryError> {
        let key = normalize_issuer_key(issuer);

        // Check cache first
        {
            let cache = self.cache.read().await;
            if let Some(cached) = cache.get(&key) {
                if cached.fetched_at.elapsed() < self.config.ttl {
                    tracing::trace!("Cache hit for OIDC discovery: {}", issuer);
                    return Ok(cached.document.clone());
                }
                tracing::trace!("Cache expired for OIDC discovery: {}", issuer);
            }
        }

        // Fetch fresh document
        tracing::debug!("Fetching OIDC discovery document from {}", issuer);
        let document = self.client.discover(issuer).await?;

        // Update cache
        {
            let mut cache = self.cache.write().await;
            cache.insert(
                key,
                CachedDiscovery {
                    document: document.clone(),
                    fetched_at: Instant::now(),
                },
            );
        }

        Ok(document)
    }

    /// Forces a refresh of the cached discovery document.
    ///
    /// This bypasses the cache and always fetches a fresh document,
    /// then updates the cache with the new value.
    ///
    /// # Arguments
    ///
    /// * `issuer` - The issuer URL to refresh the discovery document for
    ///
    /// # Returns
    ///
    /// Returns the freshly fetched `OidcDiscoveryDocument`.
    pub async fn refresh(&self, issuer: &Url) -> Result<OidcDiscoveryDocument, DiscoveryError> {
        let key = normalize_issuer_key(issuer);

        tracing::debug!("Force refreshing OIDC discovery document from {}", issuer);
        let document = self.client.discover(issuer).await?;

        // Update cache
        {
            let mut cache = self.cache.write().await;
            cache.insert(
                key,
                CachedDiscovery {
                    document: document.clone(),
                    fetched_at: Instant::now(),
                },
            );
        }

        Ok(document)
    }

    /// Invalidates a cached entry.
    ///
    /// This removes the cached document for the specified issuer, forcing
    /// the next `get` call to fetch a fresh document.
    pub async fn invalidate(&self, issuer: &Url) {
        let key = normalize_issuer_key(issuer);
        let mut cache = self.cache.write().await;
        cache.remove(&key);
        tracing::debug!("Invalidated cache for OIDC discovery: {}", issuer);
    }

    /// Clears all expired entries from the cache.
    ///
    /// This is useful for periodic cleanup to free memory from expired entries.
    pub async fn cleanup(&self) {
        let mut cache = self.cache.write().await;
        let ttl = self.config.ttl;
        let before_count = cache.len();

        cache.retain(|_, v| v.fetched_at.elapsed() < ttl);

        let removed = before_count - cache.len();
        if removed > 0 {
            tracing::debug!(
                "Cleaned up {} expired OIDC discovery cache entries",
                removed
            );
        }
    }

    /// Clears all entries from the cache.
    pub async fn clear(&self) {
        let mut cache = self.cache.write().await;
        cache.clear();
        tracing::debug!("Cleared all OIDC discovery cache entries");
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

/// Normalizes an issuer URL for use as a cache key.
///
/// This removes trailing slashes to ensure consistent lookups.
fn normalize_issuer_key(issuer: &Url) -> String {
    issuer.as_str().trim_end_matches('/').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_defaults() {
        let config = DiscoveryCacheConfig::default();
        assert_eq!(config.ttl, Duration::from_secs(3600));
        assert_eq!(config.request_timeout, Duration::from_secs(10));
        assert_eq!(config.max_response_size, 1024 * 1024);
        assert!(!config.allow_http);
    }

    #[test]
    fn test_config_builder() {
        let config = DiscoveryCacheConfig::new()
            .with_ttl(Duration::from_secs(1800))
            .with_request_timeout(Duration::from_secs(5))
            .with_max_response_size(512 * 1024)
            .with_allow_http(true);

        assert_eq!(config.ttl, Duration::from_secs(1800));
        assert_eq!(config.request_timeout, Duration::from_secs(5));
        assert_eq!(config.max_response_size, 512 * 1024);
        assert!(config.allow_http);
    }

    #[test]
    fn test_build_discovery_url() {
        let config = DiscoveryCacheConfig::default().with_allow_http(true);
        let client = OidcDiscoveryClient::new(config);

        // Simple case
        let issuer = Url::parse("https://auth.example.com").unwrap();
        let url = client.build_discovery_url(&issuer);
        assert_eq!(
            url.as_str(),
            "https://auth.example.com/.well-known/openid-configuration"
        );

        // With trailing slash
        let issuer = Url::parse("https://auth.example.com/").unwrap();
        let url = client.build_discovery_url(&issuer);
        assert_eq!(
            url.as_str(),
            "https://auth.example.com/.well-known/openid-configuration"
        );

        // With path
        let issuer = Url::parse("https://auth.example.com/tenant/abc").unwrap();
        let url = client.build_discovery_url(&issuer);
        assert_eq!(
            url.as_str(),
            "https://auth.example.com/tenant/abc/.well-known/openid-configuration"
        );
    }

    #[test]
    fn test_normalize_issuer_key() {
        let url1 = Url::parse("https://auth.example.com").unwrap();
        let url2 = Url::parse("https://auth.example.com/").unwrap();

        assert_eq!(normalize_issuer_key(&url1), normalize_issuer_key(&url2));
        assert_eq!(normalize_issuer_key(&url1), "https://auth.example.com");
    }

    #[test]
    fn test_validate_issuer_scheme() {
        // HTTPS is always allowed
        let config = DiscoveryCacheConfig::default();
        let client = OidcDiscoveryClient::new(config);

        let https_issuer = Url::parse("https://auth.example.com").unwrap();
        assert!(client.validate_issuer_scheme(&https_issuer).is_ok());

        let http_issuer = Url::parse("http://auth.example.com").unwrap();
        assert!(client.validate_issuer_scheme(&http_issuer).is_err());

        // HTTP allowed when configured
        let config = DiscoveryCacheConfig::default().with_allow_http(true);
        let client = OidcDiscoveryClient::new(config);

        assert!(client.validate_issuer_scheme(&http_issuer).is_ok());
    }

    #[tokio::test]
    async fn test_cache_invalidate() {
        let config = DiscoveryCacheConfig::default().with_allow_http(true);
        let cache = DiscoveryCache::new(config);

        // Manually add entry to cache
        {
            let mut c = cache.cache.write().await;
            c.insert(
                "https://example.com".to_string(),
                CachedDiscovery {
                    document: create_test_document("https://example.com"),
                    fetched_at: Instant::now(),
                },
            );
        }

        // Verify it's there
        assert_eq!(cache.len().await, 1);

        // Invalidate
        let issuer = Url::parse("https://example.com").unwrap();
        cache.invalidate(&issuer).await;

        // Verify it's gone
        assert!(cache.is_empty().await);
    }

    #[tokio::test]
    async fn test_cache_clear() {
        let config = DiscoveryCacheConfig::default().with_allow_http(true);
        let cache = DiscoveryCache::new(config);

        // Add entries
        {
            let mut c = cache.cache.write().await;
            c.insert(
                "https://a.example.com".to_string(),
                CachedDiscovery {
                    document: create_test_document("https://a.example.com"),
                    fetched_at: Instant::now(),
                },
            );
            c.insert(
                "https://b.example.com".to_string(),
                CachedDiscovery {
                    document: create_test_document("https://b.example.com"),
                    fetched_at: Instant::now(),
                },
            );
        }

        assert_eq!(cache.len().await, 2);

        // Clear
        cache.clear().await;

        assert!(cache.is_empty().await);
    }

    #[tokio::test]
    async fn test_cache_cleanup() {
        let config = DiscoveryCacheConfig::default()
            .with_allow_http(true)
            .with_ttl(Duration::from_millis(1)); // Very short TTL for testing
        let cache = DiscoveryCache::new(config);

        // Add entry
        {
            let mut c = cache.cache.write().await;
            c.insert(
                "https://example.com".to_string(),
                CachedDiscovery {
                    document: create_test_document("https://example.com"),
                    fetched_at: Instant::now() - Duration::from_secs(1), // Already expired
                },
            );
        }

        assert_eq!(cache.len().await, 1);

        // Cleanup
        cache.cleanup().await;

        // Should be removed due to expiration
        assert!(cache.is_empty().await);
    }

    #[test]
    fn test_discovery_error_display() {
        let err = DiscoveryError::NetworkError("connection refused".to_string());
        assert_eq!(err.to_string(), "Network error: connection refused");

        let err = DiscoveryError::HttpError(404);
        assert_eq!(err.to_string(), "HTTP error: status 404");

        let err = DiscoveryError::IssuerMismatch {
            expected: "https://a.com".to_string(),
            actual: "https://b.com".to_string(),
        };
        assert_eq!(
            err.to_string(),
            "Issuer mismatch: expected https://a.com, got https://b.com"
        );

        let err = DiscoveryError::InvalidScheme("http".to_string());
        assert_eq!(
            err.to_string(),
            "Invalid URL scheme: http (only HTTPS is allowed)"
        );

        let err = DiscoveryError::ResponseTooLarge { max_size: 1024 };
        assert_eq!(
            err.to_string(),
            "Response exceeds maximum size of 1024 bytes"
        );
    }

    /// Helper to create a test discovery document.
    fn create_test_document(issuer: &str) -> OidcDiscoveryDocument {
        OidcDiscoveryDocument {
            issuer: issuer.to_string(),
            authorization_endpoint: format!("{}/authorize", issuer),
            token_endpoint: format!("{}/token", issuer),
            jwks_uri: format!("{}/.well-known/jwks.json", issuer),
            response_types_supported: vec!["code".to_string()],
            subject_types_supported: vec!["public".to_string()],
            id_token_signing_alg_values_supported: vec!["RS256".to_string()],
            userinfo_endpoint: None,
            scopes_supported: None,
            claims_supported: None,
            registration_endpoint: None,
            token_endpoint_auth_methods_supported: None,
            token_endpoint_auth_signing_alg_values_supported: None,
            grant_types_supported: None,
            acr_values_supported: None,
            response_modes_supported: None,
            code_challenge_methods_supported: None,
            revocation_endpoint: None,
            introspection_endpoint: None,
            end_session_endpoint: None,
            request_object_signing_alg_values_supported: None,
            request_parameter_supported: None,
            request_uri_parameter_supported: None,
            require_request_uri_registration: None,
            claims_parameter_supported: None,
            service_documentation: None,
            ui_locales_supported: None,
            op_policy_uri: None,
            op_tos_uri: None,
            pushed_authorization_request_endpoint: None,
            require_pushed_authorization_requests: None,
        }
    }
}

// Integration tests that require wiremock.
// These are commented out as wiremock is not included in dev-dependencies.
// To enable, add `wiremock = "0.5"` to dev-dependencies and uncomment.
//
// #[cfg(test)]
// mod integration_tests {
//     use super::*;
//     use wiremock::matchers::{method, path};
//     use wiremock::{Mock, MockServer, ResponseTemplate};
//
//     #[tokio::test]
//     async fn test_discover_oidc_config() {
//         let mock_server = MockServer::start().await;
//
//         let discovery_doc = serde_json::json!({
//             "issuer": mock_server.uri(),
//             "authorization_endpoint": format!("{}/authorize", mock_server.uri()),
//             "token_endpoint": format!("{}/token", mock_server.uri()),
//             "jwks_uri": format!("{}/.well-known/jwks.json", mock_server.uri()),
//             "response_types_supported": ["code"],
//             "subject_types_supported": ["public"],
//             "id_token_signing_alg_values_supported": ["RS256"]
//         });
//
//         Mock::given(method("GET"))
//             .and(path("/.well-known/openid-configuration"))
//             .respond_with(ResponseTemplate::new(200).set_body_json(&discovery_doc))
//             .mount(&mock_server)
//             .await;
//
//         let config = DiscoveryCacheConfig::default().with_allow_http(true);
//         let client = OidcDiscoveryClient::new(config);
//         let issuer = Url::parse(&mock_server.uri()).unwrap();
//
//         let result = client.discover(&issuer).await.unwrap();
//
//         assert_eq!(result.issuer, mock_server.uri());
//         assert!(result.response_types_supported.contains(&"code".to_string()));
//     }
//
//     #[tokio::test]
//     async fn test_issuer_mismatch() {
//         let mock_server = MockServer::start().await;
//
//         let discovery_doc = serde_json::json!({
//             "issuer": "https://different-issuer.com",
//             "authorization_endpoint": "https://different-issuer.com/authorize",
//             "token_endpoint": "https://different-issuer.com/token",
//             "jwks_uri": "https://different-issuer.com/.well-known/jwks.json",
//             "response_types_supported": ["code"],
//             "subject_types_supported": ["public"],
//             "id_token_signing_alg_values_supported": ["RS256"]
//         });
//
//         Mock::given(method("GET"))
//             .and(path("/.well-known/openid-configuration"))
//             .respond_with(ResponseTemplate::new(200).set_body_json(&discovery_doc))
//             .mount(&mock_server)
//             .await;
//
//         let config = DiscoveryCacheConfig::default().with_allow_http(true);
//         let client = OidcDiscoveryClient::new(config);
//         let issuer = Url::parse(&mock_server.uri()).unwrap();
//
//         let result = client.discover(&issuer).await;
//
//         assert!(matches!(result, Err(DiscoveryError::IssuerMismatch { .. })));
//     }
//
//     #[tokio::test]
//     async fn test_discovery_cache() {
//         let mock_server = MockServer::start().await;
//
//         let discovery_doc = serde_json::json!({
//             "issuer": mock_server.uri(),
//             "authorization_endpoint": format!("{}/authorize", mock_server.uri()),
//             "token_endpoint": format!("{}/token", mock_server.uri()),
//             "jwks_uri": format!("{}/.well-known/jwks.json", mock_server.uri()),
//             "response_types_supported": ["code"],
//             "subject_types_supported": ["public"],
//             "id_token_signing_alg_values_supported": ["RS256"]
//         });
//
//         Mock::given(method("GET"))
//             .and(path("/.well-known/openid-configuration"))
//             .respond_with(ResponseTemplate::new(200).set_body_json(&discovery_doc))
//             .expect(1) // Should only be called once
//             .mount(&mock_server)
//             .await;
//
//         let config = DiscoveryCacheConfig::default()
//             .with_allow_http(true)
//             .with_ttl(Duration::from_secs(3600));
//         let cache = DiscoveryCache::new(config);
//         let issuer = Url::parse(&mock_server.uri()).unwrap();
//
//         // First call fetches
//         let _ = cache.get(&issuer).await.unwrap();
//         // Second call uses cache
//         let _ = cache.get(&issuer).await.unwrap();
//
//         // Mock expectation verifies only one request was made
//     }
// }
