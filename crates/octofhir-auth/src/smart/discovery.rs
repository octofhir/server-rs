//! SMART configuration discovery endpoint.
//!
//! Implements the `/.well-known/smart-configuration` endpoint per the
//! SMART App Launch specification.
//!
//! # Overview
//!
//! The SMART configuration endpoint allows clients to discover the server's
//! OAuth 2.0 endpoints and SMART on FHIR capabilities. This is essential for
//! SMART app launch flows.
//!
//! # References
//!
//! - [SMART Configuration](https://build.fhir.org/ig/HL7/smart-app-launch/conformance.html)

use serde::Serialize;
use url::Url;

use crate::config::{AuthConfig, SmartConfig};

/// SMART configuration document.
///
/// Served at `/.well-known/smart-configuration` to advertise the server's
/// SMART on FHIR capabilities. This document allows SMART apps to discover
/// OAuth endpoints, supported grant types, and server capabilities.
///
/// # Example Response
///
/// ```json
/// {
///   "issuer": "https://fhir.example.com",
///   "authorization_endpoint": "https://fhir.example.com/auth/authorize",
///   "token_endpoint": "https://fhir.example.com/auth/token",
///   "grant_types_supported": ["authorization_code", "client_credentials", "refresh_token"],
///   "code_challenge_methods_supported": ["S256"],
///   "capabilities": ["launch-ehr", "launch-standalone", "client-public", "permission-v2"]
/// }
/// ```
#[derive(Debug, Clone, Serialize)]
pub struct SmartConfiguration {
    /// Issuer URL (when OpenID Connect is enabled).
    /// This should match the `iss` claim in ID tokens.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub issuer: Option<String>,

    /// URL to the OAuth 2.0 Authorization endpoint.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authorization_endpoint: Option<String>,

    /// URL to the OAuth 2.0 Token endpoint.
    pub token_endpoint: String,

    /// Supported grant types.
    pub grant_types_supported: Vec<String>,

    /// Supported PKCE code challenge methods.
    /// SMART requires S256 support.
    pub code_challenge_methods_supported: Vec<String>,

    /// SMART capabilities advertised by this server.
    pub capabilities: Vec<String>,

    /// URL to the server's JSON Web Key Set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jwks_uri: Option<String>,

    /// Supported OAuth scopes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scopes_supported: Option<Vec<String>>,

    /// Supported response types.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_types_supported: Option<Vec<String>>,

    /// Token introspection endpoint (RFC 7662).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub introspection_endpoint: Option<String>,

    /// Token revocation endpoint (RFC 7009).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revocation_endpoint: Option<String>,

    /// Dynamic client registration endpoint (RFC 7591).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub registration_endpoint: Option<String>,

    /// Supported token endpoint authentication methods.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_endpoint_auth_methods_supported: Option<Vec<String>>,
}

impl SmartConfiguration {
    /// Builds a SMART configuration from auth config and base URL.
    ///
    /// # Arguments
    ///
    /// * `config` - The authentication configuration
    /// * `base_url` - The base URL of the FHIR server
    ///
    /// # Example
    ///
    /// ```ignore
    /// use octofhir_auth::config::AuthConfig;
    /// use octofhir_auth::smart::SmartConfiguration;
    /// use url::Url;
    ///
    /// let config = AuthConfig::default();
    /// let base_url = Url::parse("https://fhir.example.com").unwrap();
    /// let smart_config = SmartConfiguration::build(&config, &base_url);
    /// ```
    pub fn build(config: &AuthConfig, base_url: &Url) -> Self {
        let capabilities = Self::build_capabilities(&config.smart);
        let token_auth_methods = Self::build_token_auth_methods(&config.smart);
        let base = base_url.as_str().trim_end_matches('/');

        Self {
            issuer: config.smart.openid_enabled.then(|| base.to_string()),
            authorization_endpoint: Some(format!("{}/auth/authorize", base)),
            token_endpoint: format!("{}/auth/token", base),
            grant_types_supported: config.oauth.grant_types.clone(),
            code_challenge_methods_supported: vec!["S256".to_string()],
            capabilities,
            jwks_uri: Some(format!("{}/.well-known/jwks.json", base)),
            scopes_supported: Some(config.smart.supported_scopes.clone()),
            response_types_supported: Some(vec!["code".to_string()]),
            introspection_endpoint: Some(format!("{}/auth/introspect", base)),
            revocation_endpoint: Some(format!("{}/auth/revoke", base)),
            registration_endpoint: config
                .smart
                .dynamic_registration_enabled
                .then(|| format!("{}/auth/register", base)),
            token_endpoint_auth_methods_supported: Some(token_auth_methods),
        }
    }

    /// Builds the capabilities list based on SMART configuration.
    fn build_capabilities(smart: &SmartConfig) -> Vec<String> {
        let mut caps = vec!["permission-v2".to_string()];

        if smart.launch_ehr_enabled {
            caps.push("launch-ehr".to_string());
            caps.push("context-ehr-patient".to_string());
            caps.push("context-ehr-encounter".to_string());
        }

        if smart.launch_standalone_enabled {
            caps.push("launch-standalone".to_string());
            caps.push("context-standalone-patient".to_string());
        }

        if smart.public_clients_allowed {
            caps.push("client-public".to_string());
        }

        if smart.confidential_symmetric_allowed {
            caps.push("client-confidential-symmetric".to_string());
        }

        if smart.confidential_asymmetric_allowed {
            caps.push("client-confidential-asymmetric".to_string());
        }

        if smart.refresh_tokens_enabled {
            caps.push("permission-offline".to_string());
            caps.push("permission-online".to_string());
        }

        if smart.openid_enabled {
            caps.push("sso-openid-connect".to_string());
        }

        // Always advertise patient and user permission scopes
        caps.push("permission-patient".to_string());
        caps.push("permission-user".to_string());

        caps
    }

    /// Builds the token endpoint authentication methods based on configuration.
    fn build_token_auth_methods(smart: &SmartConfig) -> Vec<String> {
        let mut methods = Vec::new();

        if smart.confidential_symmetric_allowed {
            methods.push("client_secret_basic".to_string());
            methods.push("client_secret_post".to_string());
        }

        if smart.confidential_asymmetric_allowed {
            methods.push("private_key_jwt".to_string());
        }

        methods
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_smart_configuration_required_fields() {
        let config = AuthConfig::default();
        let base_url = Url::parse("https://fhir.example.com").unwrap();

        let smart_config = SmartConfiguration::build(&config, &base_url);

        assert_eq!(
            smart_config.token_endpoint,
            "https://fhir.example.com/auth/token"
        );
        assert!(
            smart_config
                .code_challenge_methods_supported
                .contains(&"S256".to_string())
        );
        assert!(!smart_config.capabilities.is_empty());
    }

    #[test]
    fn test_smart_configuration_capabilities() {
        let mut config = AuthConfig::default();
        config.smart.launch_ehr_enabled = true;
        config.smart.public_clients_allowed = true;

        let base_url = Url::parse("https://fhir.example.com").unwrap();
        let smart_config = SmartConfiguration::build(&config, &base_url);

        assert!(
            smart_config
                .capabilities
                .contains(&"launch-ehr".to_string())
        );
        assert!(
            smart_config
                .capabilities
                .contains(&"client-public".to_string())
        );
        assert!(
            smart_config
                .capabilities
                .contains(&"permission-v2".to_string())
        );
    }

    #[test]
    fn test_openid_issuer() {
        let mut config = AuthConfig::default();
        config.smart.openid_enabled = true;

        let base_url = Url::parse("https://fhir.example.com").unwrap();
        let smart_config = SmartConfiguration::build(&config, &base_url);

        assert_eq!(
            smart_config.issuer,
            Some("https://fhir.example.com".to_string())
        );
    }

    #[test]
    fn test_no_issuer_without_openid() {
        let mut config = AuthConfig::default();
        config.smart.openid_enabled = false;

        let base_url = Url::parse("https://fhir.example.com").unwrap();
        let smart_config = SmartConfiguration::build(&config, &base_url);

        assert!(smart_config.issuer.is_none());
    }

    #[test]
    fn test_dynamic_registration_endpoint() {
        let mut config = AuthConfig::default();
        config.smart.dynamic_registration_enabled = true;

        let base_url = Url::parse("https://fhir.example.com").unwrap();
        let smart_config = SmartConfiguration::build(&config, &base_url);

        assert_eq!(
            smart_config.registration_endpoint,
            Some("https://fhir.example.com/auth/register".to_string())
        );
    }

    #[test]
    fn test_no_registration_endpoint_when_disabled() {
        let mut config = AuthConfig::default();
        config.smart.dynamic_registration_enabled = false;

        let base_url = Url::parse("https://fhir.example.com").unwrap();
        let smart_config = SmartConfiguration::build(&config, &base_url);

        assert!(smart_config.registration_endpoint.is_none());
    }

    #[test]
    fn test_token_auth_methods() {
        let mut config = AuthConfig::default();
        config.smart.confidential_symmetric_allowed = true;
        config.smart.confidential_asymmetric_allowed = true;

        let base_url = Url::parse("https://fhir.example.com").unwrap();
        let smart_config = SmartConfiguration::build(&config, &base_url);

        let methods = smart_config.token_endpoint_auth_methods_supported.unwrap();
        assert!(methods.contains(&"client_secret_basic".to_string()));
        assert!(methods.contains(&"private_key_jwt".to_string()));
    }

    #[test]
    fn test_trailing_slash_handling() {
        let config = AuthConfig::default();
        let base_url = Url::parse("https://fhir.example.com/").unwrap();

        let smart_config = SmartConfiguration::build(&config, &base_url);

        // Should not have double slashes
        assert_eq!(
            smart_config.token_endpoint,
            "https://fhir.example.com/auth/token"
        );
        assert_eq!(
            smart_config.authorization_endpoint,
            Some("https://fhir.example.com/auth/authorize".to_string())
        );
    }

    #[test]
    fn test_all_capabilities_enabled() {
        let mut config = AuthConfig::default();
        config.smart.launch_ehr_enabled = true;
        config.smart.launch_standalone_enabled = true;
        config.smart.public_clients_allowed = true;
        config.smart.confidential_symmetric_allowed = true;
        config.smart.confidential_asymmetric_allowed = true;
        config.smart.refresh_tokens_enabled = true;
        config.smart.openid_enabled = true;

        let base_url = Url::parse("https://fhir.example.com").unwrap();
        let smart_config = SmartConfiguration::build(&config, &base_url);

        // Check all capabilities are present
        let caps = &smart_config.capabilities;
        assert!(caps.contains(&"permission-v2".to_string()));
        assert!(caps.contains(&"launch-ehr".to_string()));
        assert!(caps.contains(&"context-ehr-patient".to_string()));
        assert!(caps.contains(&"context-ehr-encounter".to_string()));
        assert!(caps.contains(&"launch-standalone".to_string()));
        assert!(caps.contains(&"context-standalone-patient".to_string()));
        assert!(caps.contains(&"client-public".to_string()));
        assert!(caps.contains(&"client-confidential-symmetric".to_string()));
        assert!(caps.contains(&"client-confidential-asymmetric".to_string()));
        assert!(caps.contains(&"permission-offline".to_string()));
        assert!(caps.contains(&"permission-online".to_string()));
        assert!(caps.contains(&"sso-openid-connect".to_string()));
        assert!(caps.contains(&"permission-patient".to_string()));
        assert!(caps.contains(&"permission-user".to_string()));
    }

    #[test]
    fn test_serialization() {
        let config = AuthConfig::default();
        let base_url = Url::parse("https://fhir.example.com").unwrap();

        let smart_config = SmartConfiguration::build(&config, &base_url);
        let json = serde_json::to_string(&smart_config).unwrap();

        // Required fields must be present
        assert!(json.contains("\"token_endpoint\""));
        assert!(json.contains("\"grant_types_supported\""));
        assert!(json.contains("\"code_challenge_methods_supported\""));
        assert!(json.contains("\"capabilities\""));

        // Optional None fields should not be present if OpenID is disabled
        let config_no_openid = {
            let mut c = AuthConfig::default();
            c.smart.openid_enabled = false;
            c
        };
        let smart_config_no_openid = SmartConfiguration::build(&config_no_openid, &base_url);
        let json_no_openid = serde_json::to_string(&smart_config_no_openid).unwrap();
        assert!(!json_no_openid.contains("\"issuer\""));
    }
}
