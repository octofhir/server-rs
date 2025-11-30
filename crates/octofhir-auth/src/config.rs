//! Authentication and authorization configuration.
//!
//! This module provides comprehensive configuration types for the auth module,
//! including OAuth 2.0 settings, SMART on FHIR options, token signing,
//! policy engine configuration, and more.

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Root authentication and authorization configuration.
///
/// This struct contains all configuration options for the OctoFHIR auth module,
/// organized into logical subsections for different aspects of authentication
/// and authorization.
///
/// # Example (TOML)
///
/// ```toml
/// [auth]
/// enabled = true
/// issuer = "https://fhir.example.com"
///
/// [auth.oauth]
/// access_token_lifetime = "1h"
/// refresh_token_lifetime = "90d"
/// ```
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct AuthConfig {
    /// Enable/disable the auth module entirely.
    /// When disabled, all requests are unauthenticated.
    pub enabled: bool,

    /// Server issuer URL (used in token `iss` claim).
    /// This should be the public base URL of the FHIR server.
    pub issuer: String,

    /// OAuth 2.0 configuration.
    pub oauth: OAuthConfig,

    /// SMART on FHIR configuration.
    pub smart: SmartConfig,

    /// Token signing configuration.
    pub signing: SigningConfig,

    /// Policy engine configuration.
    pub policy: PolicyConfig,

    /// External IdP federation configuration.
    pub federation: FederationConfig,

    /// Rate limiting configuration.
    pub rate_limiting: RateLimitingConfig,

    /// Audit configuration.
    pub audit: AuditConfig,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            issuer: "http://localhost:8080".to_string(),
            oauth: OAuthConfig::default(),
            smart: SmartConfig::default(),
            signing: SigningConfig::default(),
            policy: PolicyConfig::default(),
            federation: FederationConfig::default(),
            rate_limiting: RateLimitingConfig::default(),
            audit: AuditConfig::default(),
        }
    }
}

/// OAuth 2.0 configuration.
///
/// Controls token lifetimes, refresh token behavior, and allowed grant types
/// for the OAuth 2.0 authorization server.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct OAuthConfig {
    /// Authorization code lifetime.
    /// Codes should be short-lived for security.
    #[serde(with = "humantime_serde")]
    pub authorization_code_lifetime: Duration,

    /// Access token lifetime.
    /// Shorter lifetimes are more secure but require more frequent refresh.
    #[serde(with = "humantime_serde")]
    pub access_token_lifetime: Duration,

    /// Refresh token lifetime.
    /// Can be longer since refresh tokens require client authentication.
    #[serde(with = "humantime_serde")]
    pub refresh_token_lifetime: Duration,

    /// Rotate refresh tokens on use.
    /// When enabled, a new refresh token is issued with each refresh.
    /// This is recommended for security (detects token theft).
    pub refresh_token_rotation: bool,

    /// Allowed OAuth 2.0 grant types.
    /// Supported: "authorization_code", "client_credentials", "refresh_token"
    pub grant_types: Vec<String>,
}

impl Default for OAuthConfig {
    fn default() -> Self {
        Self {
            authorization_code_lifetime: Duration::from_secs(600), // 10 minutes
            access_token_lifetime: Duration::from_secs(3600),      // 1 hour
            refresh_token_lifetime: Duration::from_secs(90 * 24 * 3600), // 90 days
            refresh_token_rotation: true,
            grant_types: vec![
                "authorization_code".to_string(),
                "client_credentials".to_string(),
                "refresh_token".to_string(),
            ],
        }
    }
}

/// SMART on FHIR configuration.
///
/// Controls SMART launch modes, client types, and supported capabilities
/// as defined in the SMART App Launch Implementation Guide.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct SmartConfig {
    /// Enable EHR launch mode.
    /// Apps can be launched from within an EHR context.
    pub launch_ehr_enabled: bool,

    /// Enable standalone launch mode.
    /// Apps can launch independently and request context.
    pub launch_standalone_enabled: bool,

    /// Allow public (PKCE-only) clients.
    /// These clients cannot keep a secret secure.
    pub public_clients_allowed: bool,

    /// Allow confidential clients with symmetric secrets.
    /// These clients authenticate with client_secret.
    pub confidential_symmetric_allowed: bool,

    /// Allow confidential clients with asymmetric keys.
    /// These clients authenticate with signed JWTs (private_key_jwt).
    pub confidential_asymmetric_allowed: bool,

    /// Enable refresh token support.
    /// Required for long-running applications.
    pub refresh_tokens_enabled: bool,

    /// Enable OpenID Connect.
    /// Allows apps to request identity information.
    pub openid_enabled: bool,

    /// Enable dynamic client registration (RFC 7591).
    /// Allows apps to register themselves automatically.
    pub dynamic_registration_enabled: bool,

    /// Supported scopes.
    /// These are advertised in the SMART configuration.
    pub supported_scopes: Vec<String>,
}

impl Default for SmartConfig {
    fn default() -> Self {
        Self {
            launch_ehr_enabled: true,
            launch_standalone_enabled: true,
            public_clients_allowed: true,
            confidential_symmetric_allowed: true,
            confidential_asymmetric_allowed: true,
            refresh_tokens_enabled: true,
            openid_enabled: true,
            dynamic_registration_enabled: false,
            supported_scopes: vec![
                "openid".to_string(),
                "fhirUser".to_string(),
                "launch".to_string(),
                "launch/patient".to_string(),
                "launch/encounter".to_string(),
                "offline_access".to_string(),
                "online_access".to_string(),
                "patient/*.cruds".to_string(),
                "user/*.cruds".to_string(),
                "system/*.cruds".to_string(),
            ],
        }
    }
}

/// Token signing configuration.
///
/// Controls the cryptographic algorithms and key management for JWT signing.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct SigningConfig {
    /// Signing algorithm.
    /// Supported: "RS256", "RS384", "ES384"
    pub algorithm: String,

    /// Key rotation period in days.
    /// New keys are generated automatically after this period.
    pub key_rotation_days: u32,

    /// Number of old keys to keep for validation.
    /// Allows tokens signed with old keys to remain valid until expiry.
    pub keys_to_keep: u32,
}

impl Default for SigningConfig {
    fn default() -> Self {
        Self {
            algorithm: "RS384".to_string(),
            key_rotation_days: 90,
            keys_to_keep: 3,
        }
    }
}

/// Policy engine configuration.
///
/// Controls the fine-grained authorization policy evaluation system.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct PolicyConfig {
    /// Default deny mode.
    /// When enabled, access is denied unless a policy explicitly allows it.
    pub default_deny: bool,

    /// Enable Rhai scripting for policies.
    pub rhai_enabled: bool,

    /// Enable QuickJS scripting for policies.
    pub quickjs_enabled: bool,

    /// Rhai scripting configuration.
    pub rhai: RhaiConfig,

    /// QuickJS scripting configuration.
    pub quickjs: QuickJsConfig,
}

impl Default for PolicyConfig {
    fn default() -> Self {
        Self {
            default_deny: true,
            rhai_enabled: true,
            quickjs_enabled: true,
            rhai: RhaiConfig::default(),
            quickjs: QuickJsConfig::default(),
        }
    }
}

/// Rhai scripting engine configuration.
///
/// Controls resource limits for the Rhai policy scripting engine.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct RhaiConfig {
    /// Maximum operations per script execution.
    /// Prevents runaway scripts.
    pub max_operations: u64,

    /// Maximum call stack depth.
    /// Prevents stack overflow from deep recursion.
    pub max_call_levels: usize,
}

impl Default for RhaiConfig {
    fn default() -> Self {
        Self {
            max_operations: 10_000,
            max_call_levels: 32,
        }
    }
}

/// QuickJS scripting engine configuration.
///
/// Controls resource limits for the QuickJS JavaScript policy engine.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct QuickJsConfig {
    /// Memory limit in megabytes.
    pub memory_limit_mb: usize,

    /// Stack size limit in kilobytes.
    pub max_stack_size_kb: usize,

    /// Script execution timeout in milliseconds.
    pub timeout_ms: u64,

    /// Size of the QuickJS runtime pool.
    /// More runtimes allow parallel policy evaluation.
    pub pool_size: usize,
}

impl Default for QuickJsConfig {
    fn default() -> Self {
        Self {
            memory_limit_mb: 16,
            max_stack_size_kb: 256,
            timeout_ms: 100,
            pool_size: num_cpus::get().max(1),
        }
    }
}

/// External identity provider federation configuration.
///
/// Controls how the auth module interacts with external OAuth/OIDC providers.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct FederationConfig {
    /// Allow authentication via external identity providers.
    pub allow_external_idp: bool,

    /// Automatically provision users from external IdP claims.
    /// When disabled, users must be pre-registered.
    pub auto_provision_users: bool,

    /// JWKS cache time-to-live.
    /// How long to cache external IdP JWKS before refreshing.
    #[serde(with = "humantime_serde")]
    pub jwks_cache_ttl: Duration,

    /// Refresh JWKS on validation failure.
    /// When enabled, attempts to refresh JWKS if token validation fails.
    pub jwks_refresh_on_failure: bool,
}

impl Default for FederationConfig {
    fn default() -> Self {
        Self {
            allow_external_idp: true,
            auto_provision_users: false,
            jwks_cache_ttl: Duration::from_secs(3600), // 1 hour
            jwks_refresh_on_failure: true,
        }
    }
}

/// Rate limiting configuration.
///
/// Controls rate limiting for authentication endpoints to prevent abuse.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct RateLimitingConfig {
    /// Token requests per minute per client.
    pub token_requests_per_minute: u32,

    /// Token requests per hour per client.
    pub token_requests_per_hour: u32,

    /// Authorization requests per minute per IP.
    pub auth_requests_per_minute: u32,

    /// Maximum failed authentication attempts before lockout.
    pub max_failed_attempts: u32,

    /// Account lockout duration after max failed attempts.
    #[serde(with = "humantime_serde")]
    pub lockout_duration: Duration,
}

impl Default for RateLimitingConfig {
    fn default() -> Self {
        Self {
            token_requests_per_minute: 60,
            token_requests_per_hour: 1000,
            auth_requests_per_minute: 30,
            max_failed_attempts: 5,
            lockout_duration: Duration::from_secs(300), // 5 minutes
        }
    }
}

/// Audit logging configuration.
///
/// Controls what authentication/authorization events are logged.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct AuditConfig {
    /// Log successful authentication events.
    pub log_successful_auth: bool,

    /// Log failed authentication events.
    pub log_failed_auth: bool,

    /// Log access control decisions.
    pub log_access_decisions: bool,

    /// Log token operations (issue, refresh, revoke).
    pub log_token_operations: bool,
}

impl Default for AuditConfig {
    fn default() -> Self {
        Self {
            log_successful_auth: true,
            log_failed_auth: true,
            log_access_decisions: true,
            log_token_operations: true,
        }
    }
}

/// Configuration validation errors.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ConfigError {
    /// An invalid configuration value was provided.
    #[error("Invalid configuration value: {0}")]
    InvalidValue(String),

    /// A required configuration value is missing.
    #[error("Missing required configuration: {0}")]
    Missing(String),
}

impl AuthConfig {
    /// Validates the configuration.
    ///
    /// Returns an error if any configuration values are invalid or inconsistent.
    ///
    /// # Errors
    ///
    /// Returns `ConfigError::InvalidValue` if:
    /// - The issuer URL is empty
    /// - The signing algorithm is not supported
    /// - An invalid grant type is specified
    /// - QuickJS memory or timeout limits are zero
    pub fn validate(&self) -> Result<(), ConfigError> {
        // Validate issuer URL
        if self.issuer.is_empty() {
            return Err(ConfigError::InvalidValue(
                "issuer cannot be empty".to_string(),
            ));
        }

        // Validate signing algorithm
        match self.signing.algorithm.as_str() {
            "RS256" | "RS384" | "ES384" => {}
            other => {
                return Err(ConfigError::InvalidValue(format!(
                    "Invalid signing algorithm: '{}'. Must be RS256, RS384, or ES384",
                    other
                )));
            }
        }

        // Validate grant types
        for grant in &self.oauth.grant_types {
            match grant.as_str() {
                "authorization_code" | "client_credentials" | "refresh_token" => {}
                other => {
                    return Err(ConfigError::InvalidValue(format!(
                        "Invalid grant type: '{}'. Must be authorization_code, client_credentials, or refresh_token",
                        other
                    )));
                }
            }
        }

        // Validate QuickJS limits
        if self.policy.quickjs_enabled {
            if self.policy.quickjs.memory_limit_mb == 0 {
                return Err(ConfigError::InvalidValue(
                    "QuickJS memory_limit_mb must be > 0".to_string(),
                ));
            }

            if self.policy.quickjs.timeout_ms == 0 {
                return Err(ConfigError::InvalidValue(
                    "QuickJS timeout_ms must be > 0".to_string(),
                ));
            }

            if self.policy.quickjs.pool_size == 0 {
                return Err(ConfigError::InvalidValue(
                    "QuickJS pool_size must be > 0".to_string(),
                ));
            }
        }

        // Validate Rhai limits
        if self.policy.rhai_enabled {
            if self.policy.rhai.max_operations == 0 {
                return Err(ConfigError::InvalidValue(
                    "Rhai max_operations must be > 0".to_string(),
                ));
            }

            if self.policy.rhai.max_call_levels == 0 {
                return Err(ConfigError::InvalidValue(
                    "Rhai max_call_levels must be > 0".to_string(),
                ));
            }
        }

        // Validate rate limiting
        if self.rate_limiting.max_failed_attempts == 0 {
            return Err(ConfigError::InvalidValue(
                "max_failed_attempts must be > 0".to_string(),
            ));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = AuthConfig::default();
        assert!(config.enabled);
        assert_eq!(config.issuer, "http://localhost:8080");
        assert!(config.oauth.refresh_token_rotation);
        assert_eq!(config.signing.algorithm, "RS384");
    }

    #[test]
    fn test_default_config_validates() {
        let config = AuthConfig::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_empty_issuer_fails_validation() {
        let mut config = AuthConfig::default();
        config.issuer = String::new();
        let err = config.validate().unwrap_err();
        assert!(matches!(err, ConfigError::InvalidValue(_)));
        assert!(err.to_string().contains("issuer"));
    }

    #[test]
    fn test_invalid_algorithm_fails_validation() {
        let mut config = AuthConfig::default();
        config.signing.algorithm = "HS256".to_string();
        let err = config.validate().unwrap_err();
        assert!(matches!(err, ConfigError::InvalidValue(_)));
        assert!(err.to_string().contains("signing algorithm"));
    }

    #[test]
    fn test_valid_algorithms() {
        for alg in ["RS256", "RS384", "ES384"] {
            let mut config = AuthConfig::default();
            config.signing.algorithm = alg.to_string();
            assert!(
                config.validate().is_ok(),
                "Algorithm {} should be valid",
                alg
            );
        }
    }

    #[test]
    fn test_invalid_grant_type_fails_validation() {
        let mut config = AuthConfig::default();
        config.oauth.grant_types = vec!["password".to_string()];
        let err = config.validate().unwrap_err();
        assert!(matches!(err, ConfigError::InvalidValue(_)));
        assert!(err.to_string().contains("grant type"));
    }

    #[test]
    fn test_zero_quickjs_memory_fails_validation() {
        let mut config = AuthConfig::default();
        config.policy.quickjs.memory_limit_mb = 0;
        let err = config.validate().unwrap_err();
        assert!(err.to_string().contains("memory_limit_mb"));
    }

    #[test]
    fn test_zero_quickjs_timeout_fails_validation() {
        let mut config = AuthConfig::default();
        config.policy.quickjs.timeout_ms = 0;
        let err = config.validate().unwrap_err();
        assert!(err.to_string().contains("timeout_ms"));
    }

    #[test]
    fn test_oauth_default_lifetimes() {
        let oauth = OAuthConfig::default();
        assert_eq!(oauth.authorization_code_lifetime, Duration::from_secs(600));
        assert_eq!(oauth.access_token_lifetime, Duration::from_secs(3600));
        assert_eq!(
            oauth.refresh_token_lifetime,
            Duration::from_secs(90 * 24 * 3600)
        );
    }

    #[test]
    fn test_smart_default_scopes() {
        let smart = SmartConfig::default();
        assert!(smart.supported_scopes.contains(&"openid".to_string()));
        assert!(smart.supported_scopes.contains(&"fhirUser".to_string()));
        assert!(smart.supported_scopes.contains(&"launch".to_string()));
        assert!(
            smart
                .supported_scopes
                .contains(&"patient/*.cruds".to_string())
        );
    }

    #[test]
    fn test_quickjs_default_pool_size() {
        let quickjs = QuickJsConfig::default();
        assert!(quickjs.pool_size >= 1);
    }

    #[test]
    fn test_rate_limiting_defaults() {
        let rate = RateLimitingConfig::default();
        assert_eq!(rate.token_requests_per_minute, 60);
        assert_eq!(rate.max_failed_attempts, 5);
        assert_eq!(rate.lockout_duration, Duration::from_secs(300));
    }

    #[test]
    fn test_disabled_quickjs_skips_validation() {
        let mut config = AuthConfig::default();
        config.policy.quickjs_enabled = false;
        config.policy.quickjs.memory_limit_mb = 0;
        // Should pass validation since QuickJS is disabled
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_disabled_rhai_skips_validation() {
        let mut config = AuthConfig::default();
        config.policy.rhai_enabled = false;
        config.policy.rhai.max_operations = 0;
        // Should pass validation since Rhai is disabled
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_config_error_display() {
        let err = ConfigError::InvalidValue("test error".to_string());
        assert_eq!(err.to_string(), "Invalid configuration value: test error");

        let err = ConfigError::Missing("required_field".to_string());
        assert_eq!(
            err.to_string(),
            "Missing required configuration: required_field"
        );
    }

    #[test]
    fn test_serde_roundtrip() {
        let config = AuthConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        let parsed: AuthConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(config.enabled, parsed.enabled);
        assert_eq!(config.issuer, parsed.issuer);
        assert_eq!(config.signing.algorithm, parsed.signing.algorithm);
    }
}
