//! External identity provider authentication service.
//!
//! This module provides the [`IdpAuthService`] for handling authentication flows
//! with external OIDC identity providers.
//!
//! # Overview
//!
//! The authentication flow consists of:
//!
//! 1. **Start Auth** - Generate authorization URL with PKCE
//! 2. **Handle Callback** - Exchange authorization code for tokens
//! 3. **Validate ID Token** - Verify JWT signature and claims
//! 4. **Map User Claims** - Extract user information from token
//!
//! # Example
//!
//! ```ignore
//! use octofhir_auth::federation::auth::{IdpAuthService, IdpAuthServiceConfig};
//! use octofhir_auth::federation::provider::IdentityProviderConfig;
//! use url::Url;
//!
//! let config = IdpAuthServiceConfig::new(
//!     Url::parse("https://my-app.com/oauth/callback")?,
//! );
//!
//! let service = IdpAuthService::new(config);
//! service.register_provider(provider_config);
//!
//! // Start authentication
//! let auth_request = service.start_auth("google", "state-123", "nonce-456").await?;
//!
//! // Redirect user to auth_request.authorization_url
//!
//! // Handle callback
//! let result = service.handle_callback(
//!     "google",
//!     "auth-code",
//!     &auth_request.pkce_verifier,
//!     "nonce-456",
//! ).await?;
//! ```

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use jsonwebtoken::{Validation, decode_header};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use url::Url;

use super::discovery::{DiscoveryCache, DiscoveryCacheConfig};
use super::error::IdpError;
use super::jwks::{ProviderJwksCache, ProviderJwksCacheConfig};
use super::provider::{IdentityProviderConfig, MappedUser, UserMappingConfig};
use crate::oauth::pkce::{PkceChallenge, PkceVerifier};

/// Configuration for the IdP authentication service.
#[derive(Debug, Clone)]
pub struct IdpAuthServiceConfig {
    /// The callback URL for OAuth redirects.
    pub callback_url: Url,

    /// HTTP request timeout (default: 30 seconds).
    pub request_timeout: Duration,

    /// Clock skew tolerance for token validation (default: 60 seconds).
    pub clock_skew_tolerance: Duration,

    /// Whether to allow HTTP endpoints (for testing only).
    pub allow_http: bool,
}

impl IdpAuthServiceConfig {
    /// Creates a new configuration with the callback URL.
    #[must_use]
    pub fn new(callback_url: Url) -> Self {
        Self {
            callback_url,
            request_timeout: Duration::from_secs(30),
            clock_skew_tolerance: Duration::from_secs(60),
            allow_http: false,
        }
    }

    /// Sets the HTTP request timeout.
    #[must_use]
    pub fn with_request_timeout(mut self, timeout: Duration) -> Self {
        self.request_timeout = timeout;
        self
    }

    /// Sets the clock skew tolerance for token validation.
    #[must_use]
    pub fn with_clock_skew_tolerance(mut self, tolerance: Duration) -> Self {
        self.clock_skew_tolerance = tolerance;
        self
    }

    /// Allows HTTP endpoints (for testing only).
    #[must_use]
    pub fn with_allow_http(mut self, allow: bool) -> Self {
        self.allow_http = allow;
        self
    }
}

/// Service for handling external IdP authentication flows.
pub struct IdpAuthService {
    /// Discovery cache for fetching OIDC metadata.
    discovery_cache: Arc<DiscoveryCache>,
    /// JWKS cache for fetching provider public keys.
    jwks_cache: Arc<ProviderJwksCache>,
    /// HTTP client for token exchange.
    http_client: reqwest::Client,
    /// Registered providers.
    providers: Arc<RwLock<HashMap<String, IdentityProviderConfig>>>,
    /// Service configuration.
    config: IdpAuthServiceConfig,
}

impl IdpAuthService {
    /// Creates a new IdP authentication service.
    #[must_use]
    pub fn new(config: IdpAuthServiceConfig) -> Self {
        let discovery_config = DiscoveryCacheConfig::default()
            .with_request_timeout(config.request_timeout)
            .with_allow_http(config.allow_http);

        let jwks_config = ProviderJwksCacheConfig::default()
            .with_request_timeout(config.request_timeout)
            .with_allow_http(config.allow_http);

        let http_client = reqwest::Client::builder()
            .timeout(config.request_timeout)
            .build()
            .expect("Failed to create HTTP client");

        Self {
            discovery_cache: Arc::new(DiscoveryCache::new(discovery_config)),
            jwks_cache: Arc::new(ProviderJwksCache::new(jwks_config)),
            http_client,
            providers: Arc::new(RwLock::new(HashMap::new())),
            config,
        }
    }

    /// Creates a new service with custom caches (for testing or sharing caches).
    #[must_use]
    pub fn with_caches(
        config: IdpAuthServiceConfig,
        discovery_cache: Arc<DiscoveryCache>,
        jwks_cache: Arc<ProviderJwksCache>,
    ) -> Self {
        let http_client = reqwest::Client::builder()
            .timeout(config.request_timeout)
            .build()
            .expect("Failed to create HTTP client");

        Self {
            discovery_cache,
            jwks_cache,
            http_client,
            providers: Arc::new(RwLock::new(HashMap::new())),
            config,
        }
    }

    /// Registers an identity provider.
    pub async fn register_provider(&self, provider: IdentityProviderConfig) {
        let mut providers = self.providers.write().await;
        tracing::info!(
            "Registered identity provider: {} ({})",
            provider.name,
            provider.id
        );
        providers.insert(provider.id.clone(), provider);
    }

    /// Gets a provider by ID.
    pub async fn get_provider(&self, provider_id: &str) -> Option<IdentityProviderConfig> {
        let providers = self.providers.read().await;
        providers.get(provider_id).cloned()
    }

    /// Lists all registered providers.
    pub async fn list_providers(&self) -> Vec<IdentityProviderConfig> {
        let providers = self.providers.read().await;
        providers.values().cloned().collect()
    }

    /// Lists enabled providers only.
    pub async fn list_enabled_providers(&self) -> Vec<IdentityProviderConfig> {
        let providers = self.providers.read().await;
        providers.values().filter(|p| p.enabled).cloned().collect()
    }

    /// Starts an authentication flow with an external IdP.
    ///
    /// This generates an authorization URL with PKCE that the user should be
    /// redirected to.
    ///
    /// # Arguments
    ///
    /// * `provider_id` - The ID of the registered provider
    /// * `state` - The OAuth state parameter (for CSRF protection)
    /// * `nonce` - The OIDC nonce (for replay protection)
    ///
    /// # Returns
    ///
    /// Returns an [`IdpAuthRequest`] containing the authorization URL and PKCE verifier.
    pub async fn start_auth(
        &self,
        provider_id: &str,
        state: &str,
        nonce: &str,
    ) -> Result<IdpAuthRequest, IdpError> {
        let provider = self.get_enabled_provider(provider_id).await?;

        // Get discovery document
        let discovery = self.discovery_cache.get(&provider.issuer).await?;

        // Use endpoint override or discovered endpoint
        let auth_endpoint = provider
            .authorization_endpoint
            .as_ref()
            .map(|e| Url::parse(e))
            .transpose()?
            .unwrap_or_else(|| {
                Url::parse(&discovery.authorization_endpoint)
                    .expect("Discovery returned invalid authorization endpoint")
            });

        // Generate PKCE
        let pkce_verifier = PkceVerifier::generate();
        let pkce_challenge = PkceChallenge::from_verifier(&pkce_verifier);

        // Build authorization URL
        let mut url = auth_endpoint;
        {
            let mut params = url.query_pairs_mut();
            params.append_pair("response_type", "code");
            params.append_pair("client_id", &provider.client_id);
            params.append_pair("redirect_uri", self.config.callback_url.as_str());
            params.append_pair("scope", &provider.scopes.join(" "));
            params.append_pair("state", state);
            params.append_pair("nonce", nonce);
            params.append_pair("code_challenge", pkce_challenge.as_str());
            params.append_pair("code_challenge_method", "S256");

            // Add extra auth params
            for (key, value) in &provider.extra_auth_params {
                params.append_pair(key, value);
            }
        }

        tracing::debug!(
            "Generated authorization URL for provider {}: {}",
            provider_id,
            url.as_str().split('?').next().unwrap_or("")
        );

        Ok(IdpAuthRequest {
            authorization_url: url,
            state: state.to_string(),
            nonce: nonce.to_string(),
            pkce_verifier,
            provider_id: provider_id.to_string(),
        })
    }

    /// Handles the OAuth callback from an external IdP.
    ///
    /// This exchanges the authorization code for tokens, validates the ID token,
    /// and maps the user claims.
    ///
    /// # Arguments
    ///
    /// * `provider_id` - The ID of the provider
    /// * `code` - The authorization code from the callback
    /// * `pkce_verifier` - The PKCE verifier from the original request
    /// * `expected_nonce` - The nonce from the original request
    ///
    /// # Returns
    ///
    /// Returns an [`IdpAuthResult`] containing the tokens and mapped user.
    pub async fn handle_callback(
        &self,
        provider_id: &str,
        code: &str,
        pkce_verifier: &PkceVerifier,
        expected_nonce: &str,
    ) -> Result<IdpAuthResult, IdpError> {
        let provider = self.get_enabled_provider(provider_id).await?;

        // Exchange code for tokens
        let token_response = self.exchange_code(&provider, code, pkce_verifier).await?;

        // Validate ID token
        let claims = self
            .validate_id_token(&provider, &token_response.id_token, expected_nonce)
            .await?;

        // Map user claims
        let mapped_user = self.map_user_claims(&claims, &provider.user_mapping);

        tracing::info!(
            "Successfully authenticated user {} via provider {}",
            mapped_user.external_id,
            provider_id
        );

        Ok(IdpAuthResult {
            provider_id: provider_id.to_string(),
            external_subject: claims.sub.clone(),
            mapped_user,
            id_token: token_response.id_token,
            access_token: token_response.access_token,
            refresh_token: token_response.refresh_token,
            expires_in: token_response.expires_in,
        })
    }

    /// Gets an enabled provider by ID, returning an error if not found or disabled.
    async fn get_enabled_provider(
        &self,
        provider_id: &str,
    ) -> Result<IdentityProviderConfig, IdpError> {
        let providers = self.providers.read().await;
        let provider = providers
            .get(provider_id)
            .ok_or_else(|| IdpError::ProviderNotFound(provider_id.to_string()))?;

        if !provider.enabled {
            return Err(IdpError::ProviderDisabled(provider_id.to_string()));
        }

        Ok(provider.clone())
    }

    /// Exchanges an authorization code for tokens.
    async fn exchange_code(
        &self,
        provider: &IdentityProviderConfig,
        code: &str,
        pkce_verifier: &PkceVerifier,
    ) -> Result<TokenResponse, IdpError> {
        // Get discovery document
        let discovery = self.discovery_cache.get(&provider.issuer).await?;

        // Use endpoint override or discovered endpoint
        let token_endpoint = provider
            .token_endpoint
            .as_ref()
            .map(|e| Url::parse(e))
            .transpose()?
            .unwrap_or_else(|| {
                Url::parse(&discovery.token_endpoint)
                    .expect("Discovery returned invalid token endpoint")
            });

        // Build request body
        let mut params = vec![
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", self.config.callback_url.as_str()),
            ("client_id", &provider.client_id),
            ("code_verifier", pkce_verifier.as_str()),
        ];

        // Add client secret for confidential clients
        let secret_binding;
        if let Some(secret) = &provider.client_secret {
            secret_binding = secret.clone();
            params.push(("client_secret", &secret_binding));
        }

        tracing::debug!(
            "Exchanging authorization code with token endpoint: {}",
            token_endpoint
        );

        let response = self
            .http_client
            .post(token_endpoint.as_str())
            .form(&params)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();

            // Try to parse OAuth error
            if let Ok(oauth_error) = serde_json::from_str::<OAuthErrorResponse>(&body) {
                return Err(IdpError::oauth_error(
                    oauth_error.error,
                    oauth_error.error_description.unwrap_or_default(),
                ));
            }

            return Err(IdpError::TokenExchangeFailed(format!(
                "HTTP {} - {}",
                status, body
            )));
        }

        let token_response: TokenResponse = response.json().await.map_err(|e| {
            IdpError::TokenExchangeFailed(format!("Failed to parse token response: {}", e))
        })?;

        Ok(token_response)
    }

    /// Validates an ID token.
    async fn validate_id_token(
        &self,
        provider: &IdentityProviderConfig,
        id_token: &str,
        expected_nonce: &str,
    ) -> Result<IdTokenClaims, IdpError> {
        // Decode header to get key ID
        let header = decode_header(id_token)?;
        let kid = header.kid.ok_or(IdpError::MissingKeyId)?;

        // Get discovery document for JWKS URI
        let discovery = self.discovery_cache.get(&provider.issuer).await?;
        let jwks_uri = Url::parse(&discovery.jwks_uri)?;

        // Get the key
        let (decoding_key, key_alg) = self.jwks_cache.get_key(&jwks_uri, &kid).await?;

        // Determine algorithm: prefer key algorithm, fall back to header algorithm
        let alg = key_alg.unwrap_or(header.alg);

        // Build validation
        let mut validation = Validation::new(alg);
        validation.set_audience(&[&provider.client_id]);
        validation.set_issuer(&[provider.issuer.as_str().trim_end_matches('/')]);

        // Allow clock skew
        validation.leeway = self.config.clock_skew_tolerance.as_secs();

        // Decode and validate
        let token_data =
            jsonwebtoken::decode::<IdTokenClaims>(id_token, &decoding_key, &validation)?;
        let claims = token_data.claims;

        // Validate nonce
        if let Some(ref token_nonce) = claims.nonce {
            if token_nonce != expected_nonce {
                return Err(IdpError::NonceMismatch);
            }
        } else {
            // Nonce is required when we sent one in the auth request
            return Err(IdpError::NonceMismatch);
        }

        tracing::debug!(
            "Validated ID token for subject {} from issuer {}",
            claims.sub,
            claims.iss
        );

        Ok(claims)
    }

    /// Maps ID token claims to a [`MappedUser`].
    fn map_user_claims(&self, claims: &IdTokenClaims, mapping: &UserMappingConfig) -> MappedUser {
        // Get external ID from configured claim (usually "sub")
        let external_id = if mapping.subject_claim == "sub" {
            claims.sub.clone()
        } else {
            // For custom subject claims, try to get from extra claims
            claims
                .extra
                .get(&mapping.subject_claim)
                .and_then(|v| v.as_str())
                .map(String::from)
                .unwrap_or_else(|| claims.sub.clone())
        };

        // Extract roles from configured claim
        let roles = mapping.roles_claim.as_ref().map_or_else(Vec::new, |claim| {
            claims
                .extra
                .get(claim)
                .map(|v| {
                    if let Some(arr) = v.as_array() {
                        arr.iter()
                            .filter_map(|item| item.as_str())
                            .map(String::from)
                            .collect()
                    } else if let Some(s) = v.as_str() {
                        // Handle comma-separated roles
                        s.split(',').map(|r| r.trim().to_string()).collect()
                    } else {
                        Vec::new()
                    }
                })
                .unwrap_or_default()
        });

        MappedUser {
            external_id,
            email: claims.email.clone(),
            email_verified: claims.email_verified,
            name: claims.name.clone(),
            given_name: claims.given_name.clone(),
            family_name: claims.family_name.clone(),
            preferred_username: claims.preferred_username.clone(),
            roles,
            fhir_user_type: mapping.fhir_resource_type,
        }
    }

    /// Fetches user info from the IdP's userinfo endpoint.
    ///
    /// This is optional and can be used to get additional claims not included
    /// in the ID token.
    pub async fn fetch_userinfo(
        &self,
        provider_id: &str,
        access_token: &str,
    ) -> Result<serde_json::Value, IdpError> {
        let provider = self.get_enabled_provider(provider_id).await?;

        // Get discovery document
        let discovery = self.discovery_cache.get(&provider.issuer).await?;

        let userinfo_endpoint = provider
            .userinfo_endpoint
            .as_ref()
            .or(discovery.userinfo_endpoint.as_ref())
            .ok_or_else(|| IdpError::MissingField("userinfo_endpoint".to_string()))?;

        let response = self
            .http_client
            .get(userinfo_endpoint)
            .bearer_auth(access_token)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(IdpError::TokenExchangeFailed(format!(
                "Userinfo request failed: HTTP {}",
                response.status()
            )));
        }

        let userinfo: serde_json::Value = response.json().await.map_err(|e| {
            IdpError::UserMappingFailed(format!("Failed to parse userinfo response: {}", e))
        })?;

        Ok(userinfo)
    }
}

/// Request data for starting an IdP authentication flow.
#[derive(Debug)]
pub struct IdpAuthRequest {
    /// The authorization URL to redirect the user to.
    pub authorization_url: Url,

    /// The state parameter for CSRF protection.
    pub state: String,

    /// The nonce for replay protection.
    pub nonce: String,

    /// The PKCE verifier (must be stored and used in callback).
    pub pkce_verifier: PkceVerifier,

    /// The provider ID.
    pub provider_id: String,
}

/// Result of a successful IdP authentication.
#[derive(Debug)]
pub struct IdpAuthResult {
    /// The provider ID.
    pub provider_id: String,

    /// The external subject identifier from the IdP.
    pub external_subject: String,

    /// The mapped user information.
    pub mapped_user: MappedUser,

    /// The raw ID token (JWT).
    pub id_token: String,

    /// The access token for API calls.
    pub access_token: String,

    /// Optional refresh token.
    pub refresh_token: Option<String>,

    /// Token expiration time in seconds.
    pub expires_in: Option<u64>,
}

/// OAuth token response from the IdP.
#[derive(Debug, Deserialize)]
pub struct TokenResponse {
    /// The access token.
    pub access_token: String,

    /// The token type (usually "Bearer").
    pub token_type: String,

    /// Token expiration in seconds.
    pub expires_in: Option<u64>,

    /// Optional refresh token.
    pub refresh_token: Option<String>,

    /// The ID token (JWT).
    pub id_token: String,

    /// Granted scopes.
    pub scope: Option<String>,
}

/// OAuth error response from the IdP.
#[derive(Debug, Deserialize)]
struct OAuthErrorResponse {
    error: String,
    error_description: Option<String>,
}

/// Standard OIDC ID token claims.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdTokenClaims {
    /// Issuer identifier.
    pub iss: String,

    /// Subject identifier.
    pub sub: String,

    /// Audience (can be string or array, handled by serde).
    #[serde(deserialize_with = "deserialize_audience")]
    pub aud: Vec<String>,

    /// Expiration time (Unix timestamp).
    pub exp: i64,

    /// Issued at time (Unix timestamp).
    pub iat: i64,

    /// Nonce value.
    pub nonce: Option<String>,

    /// Time of authentication.
    pub auth_time: Option<i64>,

    /// Access token hash.
    pub at_hash: Option<String>,

    /// Code hash.
    pub c_hash: Option<String>,

    /// Authentication context class reference.
    pub acr: Option<String>,

    /// Authentication methods references.
    pub amr: Option<Vec<String>>,

    /// Authorized party.
    pub azp: Option<String>,

    // Standard OIDC profile claims
    /// User's email address.
    pub email: Option<String>,

    /// Whether email is verified.
    pub email_verified: Option<bool>,

    /// User's full name.
    pub name: Option<String>,

    /// User's given name.
    pub given_name: Option<String>,

    /// User's family name.
    pub family_name: Option<String>,

    /// User's preferred username.
    pub preferred_username: Option<String>,

    /// URL of user's profile picture.
    pub picture: Option<String>,

    /// User's locale.
    pub locale: Option<String>,

    /// User's timezone.
    pub zoneinfo: Option<String>,

    /// Time user's info was last updated.
    pub updated_at: Option<i64>,

    /// Extra claims not defined in the struct.
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

/// Custom deserializer for audience which can be a string or array.
fn deserialize_audience<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OneOrMany {
        One(String),
        Many(Vec<String>),
    }

    match OneOrMany::deserialize(deserializer)? {
        OneOrMany::One(s) => Ok(vec![s]),
        OneOrMany::Many(v) => Ok(v),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_service_config_builder() {
        let callback = Url::parse("https://app.example.com/callback").unwrap();
        let config = IdpAuthServiceConfig::new(callback.clone())
            .with_request_timeout(Duration::from_secs(60))
            .with_clock_skew_tolerance(Duration::from_secs(120))
            .with_allow_http(true);

        assert_eq!(config.callback_url, callback);
        assert_eq!(config.request_timeout, Duration::from_secs(60));
        assert_eq!(config.clock_skew_tolerance, Duration::from_secs(120));
        assert!(config.allow_http);
    }

    #[tokio::test]
    async fn test_register_and_get_provider() {
        let callback = Url::parse("https://app.example.com/callback").unwrap();
        let config = IdpAuthServiceConfig::new(callback).with_allow_http(true);
        let service = IdpAuthService::new(config);

        let issuer = Url::parse("https://auth.example.com").unwrap();
        let provider = IdentityProviderConfig::new("test", "Test Provider", issuer, "client-id");

        service.register_provider(provider.clone()).await;

        let retrieved = service.get_provider("test").await;
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().id, "test");

        let not_found = service.get_provider("nonexistent").await;
        assert!(not_found.is_none());
    }

    #[tokio::test]
    async fn test_list_providers() {
        let callback = Url::parse("https://app.example.com/callback").unwrap();
        let config = IdpAuthServiceConfig::new(callback).with_allow_http(true);
        let service = IdpAuthService::new(config);

        let issuer1 = Url::parse("https://auth1.example.com").unwrap();
        let provider1 = IdentityProviderConfig::new("p1", "Provider 1", issuer1, "client-1");

        let issuer2 = Url::parse("https://auth2.example.com").unwrap();
        let provider2 = IdentityProviderConfig::new("p2", "Provider 2", issuer2, "client-2")
            .with_enabled(false);

        service.register_provider(provider1).await;
        service.register_provider(provider2).await;

        let all = service.list_providers().await;
        assert_eq!(all.len(), 2);

        let enabled = service.list_enabled_providers().await;
        assert_eq!(enabled.len(), 1);
        assert_eq!(enabled[0].id, "p1");
    }

    #[tokio::test]
    async fn test_get_enabled_provider_errors() {
        let callback = Url::parse("https://app.example.com/callback").unwrap();
        let config = IdpAuthServiceConfig::new(callback).with_allow_http(true);
        let service = IdpAuthService::new(config);

        // Provider not found
        let result = service.get_enabled_provider("nonexistent").await;
        assert!(matches!(result, Err(IdpError::ProviderNotFound(_))));

        // Provider disabled
        let issuer = Url::parse("https://auth.example.com").unwrap();
        let provider = IdentityProviderConfig::new("disabled", "Disabled", issuer, "client")
            .with_enabled(false);
        service.register_provider(provider).await;

        let result = service.get_enabled_provider("disabled").await;
        assert!(matches!(result, Err(IdpError::ProviderDisabled(_))));
    }

    #[test]
    fn test_token_claims_deserialize() {
        let json = r#"{
            "iss": "https://auth.example.com",
            "sub": "user-123",
            "aud": "client-id",
            "exp": 1700000000,
            "iat": 1699999000,
            "nonce": "test-nonce",
            "email": "user@example.com",
            "email_verified": true,
            "name": "Test User",
            "custom_claim": "custom_value"
        }"#;

        let claims: IdTokenClaims = serde_json::from_str(json).unwrap();
        assert_eq!(claims.iss, "https://auth.example.com");
        assert_eq!(claims.sub, "user-123");
        assert_eq!(claims.aud, vec!["client-id"]);
        assert_eq!(claims.email, Some("user@example.com".to_string()));
        assert!(claims.extra.contains_key("custom_claim"));
    }

    #[test]
    fn test_token_claims_array_audience() {
        let json = r#"{
            "iss": "https://auth.example.com",
            "sub": "user-123",
            "aud": ["client-1", "client-2"],
            "exp": 1700000000,
            "iat": 1699999000
        }"#;

        let claims: IdTokenClaims = serde_json::from_str(json).unwrap();
        assert_eq!(claims.aud, vec!["client-1", "client-2"]);
    }

    #[test]
    fn test_map_user_claims() {
        let claims = IdTokenClaims {
            iss: "https://auth.example.com".to_string(),
            sub: "user-123".to_string(),
            aud: vec!["client-id".to_string()],
            exp: 1700000000,
            iat: 1699999000,
            nonce: Some("nonce".to_string()),
            auth_time: None,
            at_hash: None,
            c_hash: None,
            acr: None,
            amr: None,
            azp: None,
            email: Some("user@example.com".to_string()),
            email_verified: Some(true),
            name: Some("Test User".to_string()),
            given_name: Some("Test".to_string()),
            family_name: Some("User".to_string()),
            preferred_username: Some("testuser".to_string()),
            picture: None,
            locale: None,
            zoneinfo: None,
            updated_at: None,
            extra: {
                let mut m = HashMap::new();
                m.insert("roles".to_string(), serde_json::json!(["admin", "user"]));
                m
            },
        };

        let mapping = UserMappingConfig::default().with_roles_claim("roles");

        let callback = Url::parse("https://app.example.com/callback").unwrap();
        let config = IdpAuthServiceConfig::new(callback);
        let service = IdpAuthService::new(config);

        let mapped = service.map_user_claims(&claims, &mapping);

        assert_eq!(mapped.external_id, "user-123");
        assert_eq!(mapped.email, Some("user@example.com".to_string()));
        assert_eq!(mapped.name, Some("Test User".to_string()));
        assert_eq!(mapped.roles, vec!["admin", "user"]);
        assert!(mapped.has_verified_email());
    }

    #[test]
    fn test_map_user_claims_comma_separated_roles() {
        let claims = IdTokenClaims {
            iss: "https://auth.example.com".to_string(),
            sub: "user-456".to_string(),
            aud: vec!["client-id".to_string()],
            exp: 1700000000,
            iat: 1699999000,
            nonce: None,
            auth_time: None,
            at_hash: None,
            c_hash: None,
            acr: None,
            amr: None,
            azp: None,
            email: None,
            email_verified: None,
            name: None,
            given_name: None,
            family_name: None,
            preferred_username: None,
            picture: None,
            locale: None,
            zoneinfo: None,
            updated_at: None,
            extra: {
                let mut m = HashMap::new();
                m.insert(
                    "groups".to_string(),
                    serde_json::json!("admin, editor, viewer"),
                );
                m
            },
        };

        let mapping = UserMappingConfig::default().with_roles_claim("groups");

        let callback = Url::parse("https://app.example.com/callback").unwrap();
        let config = IdpAuthServiceConfig::new(callback);
        let service = IdpAuthService::new(config);

        let mapped = service.map_user_claims(&claims, &mapping);

        assert_eq!(mapped.roles, vec!["admin", "editor", "viewer"]);
    }
}
