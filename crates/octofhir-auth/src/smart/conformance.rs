//! SMART on FHIR CapabilityStatement security extensions.
//!
//! Provides functionality to add SMART security extensions to FHIR
//! CapabilityStatement resources per the SMART App Launch specification.
//!
//! # Overview
//!
//! The SMART App Launch specification requires FHIR servers to advertise their
//! OAuth/SMART capabilities in the CapabilityStatement resource's `rest.security`
//! element. This module builds the appropriate extensions.
//!
//! # Example
//!
//! ```ignore
//! use serde_json::json;
//! use url::Url;
//! use octofhir_auth::smart::conformance::add_smart_security;
//! use octofhir_auth::config::SmartConfig;
//!
//! let mut capability_statement = json!({
//!     "resourceType": "CapabilityStatement",
//!     "rest": [{"mode": "server"}]
//! });
//!
//! let config = SmartConfig::default();
//! let base_url = Url::parse("https://fhir.example.com").unwrap();
//!
//! add_smart_security(&mut capability_statement, &config, &base_url).unwrap();
//! ```
//!
//! # References
//!
//! - [SMART App Launch: Capability Statement](https://build.fhir.org/ig/HL7/smart-app-launch/conformance.html)
//! - [FHIR CapabilityStatement](https://hl7.org/fhir/capabilitystatement.html)

use serde_json::{Value, json};
use url::Url;

use crate::config::SmartConfig;

/// Error type for conformance operations.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ConformanceError {
    /// The CapabilityStatement is missing the required 'rest' element.
    #[error("CapabilityStatement missing 'rest' element")]
    MissingRestElement,
}

/// Builder for SMART security extensions in CapabilityStatement.
///
/// Creates the `security` element for `CapabilityStatement.rest[].security`
/// including the OAuth URIs extension and SMART-on-FHIR service coding.
pub struct CapabilitySecurityBuilder {
    config: SmartConfig,
    base_url: Url,
}

impl CapabilitySecurityBuilder {
    /// Creates a new builder with the given configuration and base URL.
    pub fn new(config: SmartConfig, base_url: Url) -> Self {
        Self { config, base_url }
    }

    /// Builds the complete security element for CapabilityStatement.rest.security.
    ///
    /// Returns a JSON value containing:
    /// - `extension`: OAuth URIs extension with endpoint URLs
    /// - `service`: SMART-on-FHIR service coding
    /// - `description`: Human-readable description
    pub fn build_security(&self) -> Value {
        json!({
            "extension": [self.build_oauth_uris_extension()],
            "service": [{
                "coding": [{
                    "system": "http://terminology.hl7.org/CodeSystem/restful-security-service",
                    "code": "SMART-on-FHIR",
                    "display": "SMART-on-FHIR"
                }],
                "text": "OAuth2 using SMART-on-FHIR profile (see http://docs.smarthealthit.org)"
            }],
            "description": "This server implements SMART on FHIR authorization"
        })
    }

    /// Builds the OAuth URIs extension with all relevant endpoints.
    fn build_oauth_uris_extension(&self) -> Value {
        let base = self.base_url.as_str().trim_end_matches('/');
        let mut extensions = vec![json!({
            "url": "token",
            "valueUri": format!("{}/auth/token", base)
        })];

        // Include authorize endpoint if any launch mode is enabled
        if self.config.launch_ehr_enabled || self.config.launch_standalone_enabled {
            extensions.push(json!({
                "url": "authorize",
                "valueUri": format!("{}/auth/authorize", base)
            }));
        }

        // Always include introspect and revoke
        extensions.push(json!({
            "url": "introspect",
            "valueUri": format!("{}/auth/introspect", base)
        }));
        extensions.push(json!({
            "url": "revoke",
            "valueUri": format!("{}/auth/revoke", base)
        }));

        // Include register endpoint if dynamic registration is enabled
        if self.config.dynamic_registration_enabled {
            extensions.push(json!({
                "url": "register",
                "valueUri": format!("{}/auth/register", base)
            }));
        }

        json!({
            "url": "http://fhir-registry.smarthealthit.org/StructureDefinition/oauth-uris",
            "extension": extensions
        })
    }
}

/// Adds SMART security extensions to a CapabilityStatement.
///
/// Modifies the provided CapabilityStatement JSON in place, adding the
/// `security` element to the first `rest` entry.
///
/// # Arguments
///
/// * `capability_statement` - Mutable reference to the CapabilityStatement JSON
/// * `config` - SMART configuration with feature flags
/// * `base_url` - Base URL of the FHIR server
///
/// # Errors
///
/// Returns `ConformanceError::MissingRestElement` if the CapabilityStatement
/// does not contain a valid `rest` array.
///
/// # Example
///
/// ```ignore
/// let mut cs = json!({"resourceType": "CapabilityStatement", "rest": [{"mode": "server"}]});
/// add_smart_security(&mut cs, &config, &base_url)?;
/// ```
pub fn add_smart_security(
    capability_statement: &mut Value,
    config: &SmartConfig,
    base_url: &Url,
) -> Result<(), ConformanceError> {
    let builder = CapabilitySecurityBuilder::new(config.clone(), base_url.clone());
    let security = builder.build_security();

    let rest = capability_statement
        .get_mut("rest")
        .and_then(|r| r.as_array_mut())
        .ok_or(ConformanceError::MissingRestElement)?;

    if let Some(first_rest) = rest.get_mut(0) {
        first_rest["security"] = security;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_config() -> SmartConfig {
        SmartConfig::default()
    }

    fn test_base_url() -> Url {
        Url::parse("https://fhir.example.com").unwrap()
    }

    #[test]
    fn test_build_security_structure() {
        let builder = CapabilitySecurityBuilder::new(default_config(), test_base_url());
        let security = builder.build_security();

        // Should have extension, service, and description
        assert!(security.get("extension").is_some());
        assert!(security.get("service").is_some());
        assert!(security.get("description").is_some());
    }

    #[test]
    fn test_service_coding() {
        let builder = CapabilitySecurityBuilder::new(default_config(), test_base_url());
        let security = builder.build_security();

        let service = &security["service"][0];
        let coding = &service["coding"][0];

        assert_eq!(
            coding["system"],
            "http://terminology.hl7.org/CodeSystem/restful-security-service"
        );
        assert_eq!(coding["code"], "SMART-on-FHIR");
        assert_eq!(coding["display"], "SMART-on-FHIR");
    }

    #[test]
    fn test_oauth_uris_extension_url() {
        let builder = CapabilitySecurityBuilder::new(default_config(), test_base_url());
        let security = builder.build_security();

        let extension = &security["extension"][0];
        assert_eq!(
            extension["url"],
            "http://fhir-registry.smarthealthit.org/StructureDefinition/oauth-uris"
        );
    }

    #[test]
    fn test_token_endpoint_always_present() {
        let builder = CapabilitySecurityBuilder::new(default_config(), test_base_url());
        let security = builder.build_security();

        let extensions = security["extension"][0]["extension"].as_array().unwrap();
        let token_ext = extensions
            .iter()
            .find(|e| e["url"] == "token")
            .expect("token endpoint should be present");

        assert_eq!(token_ext["valueUri"], "https://fhir.example.com/auth/token");
    }

    #[test]
    fn test_authorize_endpoint_when_launch_enabled() {
        let config = SmartConfig {
            launch_ehr_enabled: true,
            launch_standalone_enabled: false,
            ..default_config()
        };
        let builder = CapabilitySecurityBuilder::new(config, test_base_url());
        let security = builder.build_security();

        let extensions = security["extension"][0]["extension"].as_array().unwrap();
        let auth_ext = extensions.iter().find(|e| e["url"] == "authorize");

        assert!(auth_ext.is_some());
        assert_eq!(
            auth_ext.unwrap()["valueUri"],
            "https://fhir.example.com/auth/authorize"
        );
    }

    #[test]
    fn test_authorize_endpoint_absent_when_no_launch() {
        let config = SmartConfig {
            launch_ehr_enabled: false,
            launch_standalone_enabled: false,
            ..default_config()
        };
        let builder = CapabilitySecurityBuilder::new(config, test_base_url());
        let security = builder.build_security();

        let extensions = security["extension"][0]["extension"].as_array().unwrap();
        let auth_ext = extensions.iter().find(|e| e["url"] == "authorize");

        assert!(auth_ext.is_none());
    }

    #[test]
    fn test_introspect_and_revoke_always_present() {
        let builder = CapabilitySecurityBuilder::new(default_config(), test_base_url());
        let security = builder.build_security();

        let extensions = security["extension"][0]["extension"].as_array().unwrap();

        let introspect_ext = extensions.iter().find(|e| e["url"] == "introspect");
        assert!(introspect_ext.is_some());
        assert_eq!(
            introspect_ext.unwrap()["valueUri"],
            "https://fhir.example.com/auth/introspect"
        );

        let revoke_ext = extensions.iter().find(|e| e["url"] == "revoke");
        assert!(revoke_ext.is_some());
        assert_eq!(
            revoke_ext.unwrap()["valueUri"],
            "https://fhir.example.com/auth/revoke"
        );
    }

    #[test]
    fn test_register_endpoint_when_dynamic_registration_enabled() {
        let config = SmartConfig {
            dynamic_registration_enabled: true,
            ..default_config()
        };
        let builder = CapabilitySecurityBuilder::new(config, test_base_url());
        let security = builder.build_security();

        let extensions = security["extension"][0]["extension"].as_array().unwrap();
        let register_ext = extensions.iter().find(|e| e["url"] == "register");

        assert!(register_ext.is_some());
        assert_eq!(
            register_ext.unwrap()["valueUri"],
            "https://fhir.example.com/auth/register"
        );
    }

    #[test]
    fn test_register_endpoint_absent_when_disabled() {
        let config = SmartConfig {
            dynamic_registration_enabled: false,
            ..default_config()
        };
        let builder = CapabilitySecurityBuilder::new(config, test_base_url());
        let security = builder.build_security();

        let extensions = security["extension"][0]["extension"].as_array().unwrap();
        let register_ext = extensions.iter().find(|e| e["url"] == "register");

        assert!(register_ext.is_none());
    }

    #[test]
    fn test_trailing_slash_handling() {
        let base_url = Url::parse("https://fhir.example.com/").unwrap();
        let builder = CapabilitySecurityBuilder::new(default_config(), base_url);
        let security = builder.build_security();

        let extensions = security["extension"][0]["extension"].as_array().unwrap();
        let token_ext = extensions.iter().find(|e| e["url"] == "token").unwrap();

        // Should not have double slash
        assert_eq!(token_ext["valueUri"], "https://fhir.example.com/auth/token");
    }

    #[test]
    fn test_add_smart_security_success() {
        let mut cs = json!({
            "resourceType": "CapabilityStatement",
            "rest": [{"mode": "server"}]
        });

        let result = add_smart_security(&mut cs, &default_config(), &test_base_url());
        assert!(result.is_ok());

        // Security should be added
        assert!(cs["rest"][0]["security"].is_object());
        assert!(cs["rest"][0]["security"]["extension"].is_array());
    }

    #[test]
    fn test_add_smart_security_missing_rest() {
        let mut cs = json!({
            "resourceType": "CapabilityStatement"
        });

        let result = add_smart_security(&mut cs, &default_config(), &test_base_url());
        assert!(matches!(result, Err(ConformanceError::MissingRestElement)));
    }

    #[test]
    fn test_add_smart_security_empty_rest() {
        let mut cs = json!({
            "resourceType": "CapabilityStatement",
            "rest": []
        });

        // Empty rest array is valid, just no modification happens
        let result = add_smart_security(&mut cs, &default_config(), &test_base_url());
        assert!(result.is_ok());
    }

    #[test]
    fn test_add_smart_security_rest_not_array() {
        let mut cs = json!({
            "resourceType": "CapabilityStatement",
            "rest": "invalid"
        });

        let result = add_smart_security(&mut cs, &default_config(), &test_base_url());
        assert!(matches!(result, Err(ConformanceError::MissingRestElement)));
    }
}
