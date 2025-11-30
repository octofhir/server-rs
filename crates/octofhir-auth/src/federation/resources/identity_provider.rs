//! IdentityProvider resource type.
//!
//! This module provides the Rust representation of the IdentityProvider
//! FHIR resource, used for storing external identity provider configurations.
//!
//! # Example
//!
//! ```ignore
//! use octofhir_auth::federation::resources::IdentityProviderResource;
//!
//! let provider = IdentityProviderResource {
//!     resource_type: "IdentityProvider".to_string(),
//!     id: Some("google".to_string()),
//!     name: "Google".to_string(),
//!     provider_type: IdentityProviderType::Oidc,
//!     issuer: "https://accounts.google.com".to_string(),
//!     client_id: "your-client-id".to_string(),
//!     active: true,
//!     ..Default::default()
//! };
//!
//! provider.validate()?;
//! ```

use serde::{Deserialize, Serialize};
use url::Url;

use crate::federation::provider::{FhirUserType, IdentityProviderConfig, UserMappingConfig};

/// IdentityProvider FHIR resource for external IdP configuration.
///
/// This resource stores the configuration for an external identity provider
/// (e.g., Google, Okta, Azure AD) used for federated authentication.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentityProviderResource {
    /// Always "IdentityProvider".
    pub resource_type: String,

    /// Resource ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Unique identifier name for this provider (e.g., "google", "okta").
    pub name: String,

    /// Type of identity provider protocol.
    #[serde(rename = "type")]
    pub provider_type: IdentityProviderType,

    /// The OIDC issuer URL.
    pub issuer: String,

    /// OAuth client_id registered with this provider.
    pub client_id: String,

    /// OAuth client_secret for confidential clients.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<String>,

    /// Whether this provider is active.
    pub active: bool,

    /// OAuth scopes to request.
    #[serde(default = "default_scopes")]
    pub scopes: Vec<String>,

    /// Override for the authorization endpoint.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authorization_endpoint: Option<String>,

    /// Override for the token endpoint.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_endpoint: Option<String>,

    /// Override for the userinfo endpoint.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub userinfo_endpoint: Option<String>,

    /// Override for the JWKS URI.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jwks_uri: Option<String>,

    /// User claim mapping configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_mapping: Option<UserMappingElement>,

    /// Display name for the login button.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,

    /// URL to provider logo.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logo_url: Option<String>,

    /// CSS color for the login button.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub button_color: Option<String>,
}

fn default_scopes() -> Vec<String> {
    vec![
        "openid".to_string(),
        "profile".to_string(),
        "email".to_string(),
    ]
}

impl Default for IdentityProviderResource {
    fn default() -> Self {
        Self {
            resource_type: "IdentityProvider".to_string(),
            id: None,
            name: String::new(),
            provider_type: IdentityProviderType::Oidc,
            issuer: String::new(),
            client_id: String::new(),
            client_secret: None,
            active: true,
            scopes: default_scopes(),
            authorization_endpoint: None,
            token_endpoint: None,
            userinfo_endpoint: None,
            jwks_uri: None,
            user_mapping: None,
            display_name: None,
            logo_url: None,
            button_color: None,
        }
    }
}

impl IdentityProviderResource {
    /// Creates a new IdentityProvider with required fields.
    #[must_use]
    pub fn new(
        name: impl Into<String>,
        issuer: impl Into<String>,
        client_id: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            issuer: issuer.into(),
            client_id: client_id.into(),
            ..Default::default()
        }
    }

    /// Sets the resource ID.
    #[must_use]
    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Sets the client secret.
    #[must_use]
    pub fn with_client_secret(mut self, secret: impl Into<String>) -> Self {
        self.client_secret = Some(secret.into());
        self
    }

    /// Sets the provider type.
    #[must_use]
    pub fn with_type(mut self, provider_type: IdentityProviderType) -> Self {
        self.provider_type = provider_type;
        self
    }

    /// Sets whether the provider is active.
    #[must_use]
    pub fn with_active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }

    /// Sets the scopes.
    #[must_use]
    pub fn with_scopes(mut self, scopes: Vec<String>) -> Self {
        self.scopes = scopes;
        self
    }

    /// Sets the user mapping configuration.
    #[must_use]
    pub fn with_user_mapping(mut self, mapping: UserMappingElement) -> Self {
        self.user_mapping = Some(mapping);
        self
    }

    /// Sets the display name.
    #[must_use]
    pub fn with_display_name(mut self, name: impl Into<String>) -> Self {
        self.display_name = Some(name.into());
        self
    }

    /// Validates the resource.
    ///
    /// # Errors
    ///
    /// Returns a `ValidationError` if:
    /// - Required fields are missing or empty
    /// - URL fields are not valid URLs
    /// - fhirResourceType is not a valid user type
    pub fn validate(&self) -> Result<(), ValidationError> {
        // Check required fields
        if self.name.is_empty() {
            return Err(ValidationError::MissingField("name"));
        }
        if self.issuer.is_empty() {
            return Err(ValidationError::MissingField("issuer"));
        }
        if self.client_id.is_empty() {
            return Err(ValidationError::MissingField("clientId"));
        }

        // Validate issuer URL
        Url::parse(&self.issuer)
            .map_err(|_| ValidationError::InvalidField("issuer", "Must be a valid URL"))?;

        // Validate optional URL fields
        if let Some(ref url) = self.authorization_endpoint {
            Url::parse(url).map_err(|_| {
                ValidationError::InvalidField("authorizationEndpoint", "Must be a valid URL")
            })?;
        }
        if let Some(ref url) = self.token_endpoint {
            Url::parse(url).map_err(|_| {
                ValidationError::InvalidField("tokenEndpoint", "Must be a valid URL")
            })?;
        }
        if let Some(ref url) = self.userinfo_endpoint {
            Url::parse(url).map_err(|_| {
                ValidationError::InvalidField("userinfoEndpoint", "Must be a valid URL")
            })?;
        }
        if let Some(ref url) = self.jwks_uri {
            Url::parse(url)
                .map_err(|_| ValidationError::InvalidField("jwksUri", "Must be a valid URL"))?;
        }
        if let Some(ref url) = self.logo_url {
            Url::parse(url)
                .map_err(|_| ValidationError::InvalidField("logoUrl", "Must be a valid URL"))?;
        }

        // Validate fhirResourceType if present
        if let Some(ref mapping) = self.user_mapping
            && let Some(ref fhir_type) = mapping.fhir_resource_type
        {
            match fhir_type.as_str() {
                "Practitioner" | "Patient" | "RelatedPerson" | "Person" => {}
                _ => {
                    return Err(ValidationError::InvalidField(
                        "userMapping.fhirResourceType",
                        "Must be Practitioner, Patient, RelatedPerson, or Person",
                    ));
                }
            }
        }

        Ok(())
    }

    /// Converts to internal `IdentityProviderConfig` used by the auth service.
    ///
    /// # Errors
    ///
    /// Returns an error if the resource ID is missing or the issuer URL is invalid.
    pub fn to_config(&self) -> Result<IdentityProviderConfig, ConversionError> {
        let id = self.id.clone().ok_or(ConversionError::MissingId)?;
        let issuer = Url::parse(&self.issuer)?;

        let user_mapping = self
            .user_mapping
            .as_ref()
            .map(UserMappingElement::to_user_mapping_config)
            .unwrap_or_default();

        let mut config = IdentityProviderConfig::new(&id, &self.name, issuer, &self.client_id)
            .with_enabled(self.active)
            .with_scopes(self.scopes.clone())
            .with_user_mapping(user_mapping);

        // Apply optional fields
        if let Some(ref secret) = self.client_secret {
            config = config.with_client_secret(secret);
        }
        if let Some(ref endpoint) = self.authorization_endpoint {
            config = config.with_authorization_endpoint(endpoint);
        }
        if let Some(ref endpoint) = self.token_endpoint {
            config = config.with_token_endpoint(endpoint);
        }
        if let Some(ref endpoint) = self.userinfo_endpoint {
            config = config.with_userinfo_endpoint(endpoint);
        }

        Ok(config)
    }

    /// Returns the display name, falling back to the name if not set.
    #[must_use]
    pub fn display_name_or_name(&self) -> &str {
        self.display_name.as_deref().unwrap_or(&self.name)
    }
}

/// Identity provider protocol type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IdentityProviderType {
    /// OpenID Connect 1.0.
    #[default]
    Oidc,
    /// SAML 2.0 (future support).
    Saml,
}

impl IdentityProviderType {
    /// Returns the type as a string.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Oidc => "oidc",
            Self::Saml => "saml",
        }
    }
}

impl std::fmt::Display for IdentityProviderType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// User claim mapping configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserMappingElement {
    /// Claim to use as subject identifier (default: "sub").
    #[serde(default = "default_subject_claim")]
    pub subject_claim: String,

    /// Claim containing the user's email address.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email_claim: Option<String>,

    /// Claim containing user roles.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub roles_claim: Option<String>,

    /// FHIR resource type to create for users.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fhir_resource_type: Option<String>,

    /// Whether to auto-provision users on first login.
    #[serde(default)]
    pub auto_provision: bool,
}

fn default_subject_claim() -> String {
    "sub".to_string()
}

impl UserMappingElement {
    /// Creates a new user mapping with default values.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the subject claim.
    #[must_use]
    pub fn with_subject_claim(mut self, claim: impl Into<String>) -> Self {
        self.subject_claim = claim.into();
        self
    }

    /// Sets the email claim.
    #[must_use]
    pub fn with_email_claim(mut self, claim: impl Into<String>) -> Self {
        self.email_claim = Some(claim.into());
        self
    }

    /// Sets the roles claim.
    #[must_use]
    pub fn with_roles_claim(mut self, claim: impl Into<String>) -> Self {
        self.roles_claim = Some(claim.into());
        self
    }

    /// Sets the FHIR resource type.
    #[must_use]
    pub fn with_fhir_resource_type(mut self, resource_type: impl Into<String>) -> Self {
        self.fhir_resource_type = Some(resource_type.into());
        self
    }

    /// Sets auto-provisioning.
    #[must_use]
    pub fn with_auto_provision(mut self, auto: bool) -> Self {
        self.auto_provision = auto;
        self
    }

    /// Converts to internal `UserMappingConfig`.
    #[must_use]
    pub fn to_user_mapping_config(&self) -> UserMappingConfig {
        let fhir_resource_type = self
            .fhir_resource_type
            .as_ref()
            .map(|t| match t.as_str() {
                "Practitioner" => FhirUserType::Practitioner,
                "Patient" => FhirUserType::Patient,
                "RelatedPerson" => FhirUserType::RelatedPerson,
                _ => FhirUserType::Person,
            })
            .unwrap_or(FhirUserType::Practitioner);

        UserMappingConfig::new()
            .with_subject_claim(&self.subject_claim)
            .with_fhir_resource_type(fhir_resource_type)
            .with_auto_provision(self.auto_provision)
    }
}

/// Validation errors for IdentityProvider resources.
#[derive(Debug, thiserror::Error)]
pub enum ValidationError {
    /// A required field is missing or empty.
    #[error("Missing required field: {0}")]
    MissingField(&'static str),

    /// A field has an invalid value.
    #[error("Invalid field {0}: {1}")]
    InvalidField(&'static str, &'static str),
}

/// Errors that can occur during conversion to internal config.
#[derive(Debug, thiserror::Error)]
pub enum ConversionError {
    /// The resource ID is missing.
    #[error("Resource ID is required for conversion")]
    MissingId,

    /// URL parsing failed.
    #[error("Invalid URL: {0}")]
    InvalidUrl(#[from] url::ParseError),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_identity_provider() {
        let provider =
            IdentityProviderResource::new("google", "https://accounts.google.com", "client-123")
                .with_id("google")
                .with_active(true);

        assert!(provider.validate().is_ok());
    }

    #[test]
    fn test_missing_name() {
        let provider = IdentityProviderResource {
            name: String::new(),
            issuer: "https://example.com".to_string(),
            client_id: "client-123".to_string(),
            ..Default::default()
        };

        let result = provider.validate();
        assert!(matches!(result, Err(ValidationError::MissingField("name"))));
    }

    #[test]
    fn test_missing_issuer() {
        let provider = IdentityProviderResource {
            name: "test".to_string(),
            issuer: String::new(),
            client_id: "client-123".to_string(),
            ..Default::default()
        };

        let result = provider.validate();
        assert!(matches!(
            result,
            Err(ValidationError::MissingField("issuer"))
        ));
    }

    #[test]
    fn test_missing_client_id() {
        let provider = IdentityProviderResource {
            name: "test".to_string(),
            issuer: "https://example.com".to_string(),
            client_id: String::new(),
            ..Default::default()
        };

        let result = provider.validate();
        assert!(matches!(
            result,
            Err(ValidationError::MissingField("clientId"))
        ));
    }

    #[test]
    fn test_invalid_issuer_url() {
        let provider = IdentityProviderResource {
            name: "test".to_string(),
            issuer: "not-a-url".to_string(),
            client_id: "client-123".to_string(),
            ..Default::default()
        };

        let result = provider.validate();
        assert!(matches!(
            result,
            Err(ValidationError::InvalidField("issuer", _))
        ));
    }

    #[test]
    fn test_invalid_fhir_resource_type() {
        let provider = IdentityProviderResource {
            name: "test".to_string(),
            issuer: "https://example.com".to_string(),
            client_id: "client-123".to_string(),
            user_mapping: Some(UserMappingElement {
                fhir_resource_type: Some("Observation".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };

        let result = provider.validate();
        assert!(matches!(
            result,
            Err(ValidationError::InvalidField(
                "userMapping.fhirResourceType",
                _
            ))
        ));
    }

    #[test]
    fn test_valid_fhir_resource_types() {
        for fhir_type in ["Practitioner", "Patient", "RelatedPerson", "Person"] {
            let provider = IdentityProviderResource {
                name: "test".to_string(),
                issuer: "https://example.com".to_string(),
                client_id: "client-123".to_string(),
                user_mapping: Some(UserMappingElement {
                    fhir_resource_type: Some(fhir_type.to_string()),
                    ..Default::default()
                }),
                ..Default::default()
            };

            assert!(provider.validate().is_ok(), "Should accept {}", fhir_type);
        }
    }

    #[test]
    fn test_to_config() {
        let provider =
            IdentityProviderResource::new("okta", "https://dev-123.okta.com", "client-abc")
                .with_id("okta")
                .with_client_secret("secret")
                .with_active(true)
                .with_scopes(vec!["openid".to_string(), "profile".to_string()])
                .with_user_mapping(
                    UserMappingElement::new()
                        .with_auto_provision(true)
                        .with_fhir_resource_type("Practitioner"),
                );

        let config = provider.to_config().unwrap();

        assert_eq!(config.id, "okta");
        assert_eq!(config.client_id, "client-abc");
        assert!(config.enabled);
        assert!(config.user_mapping.auto_provision);
    }

    #[test]
    fn test_to_config_missing_id() {
        let provider = IdentityProviderResource::new("test", "https://example.com", "client-123");

        let result = provider.to_config();
        assert!(matches!(result, Err(ConversionError::MissingId)));
    }

    #[test]
    fn test_serialization() {
        let provider = IdentityProviderResource::new("test", "https://example.com", "client-123")
            .with_id("test-id");

        let json = serde_json::to_string(&provider).unwrap();
        assert!(json.contains(r#""resourceType":"IdentityProvider""#));
        assert!(json.contains(r#""name":"test""#));
        assert!(json.contains(r#""issuer":"https://example.com""#));

        let deserialized: IdentityProviderResource = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.name, "test");
        assert_eq!(deserialized.id, Some("test-id".to_string()));
    }

    #[test]
    fn test_identity_provider_type() {
        assert_eq!(IdentityProviderType::Oidc.as_str(), "oidc");
        assert_eq!(IdentityProviderType::Saml.as_str(), "saml");
        assert_eq!(IdentityProviderType::Oidc.to_string(), "oidc");
    }

    #[test]
    fn test_default_scopes() {
        let provider = IdentityProviderResource::default();
        assert!(provider.scopes.contains(&"openid".to_string()));
        assert!(provider.scopes.contains(&"profile".to_string()));
        assert!(provider.scopes.contains(&"email".to_string()));
    }

    #[test]
    fn test_display_name_fallback() {
        let provider = IdentityProviderResource::new("google", "https://example.com", "client");
        assert_eq!(provider.display_name_or_name(), "google");

        let provider_with_display = provider.with_display_name("Google Sign-In");
        assert_eq!(
            provider_with_display.display_name_or_name(),
            "Google Sign-In"
        );
    }

    #[test]
    fn test_user_mapping_to_config() {
        let mapping = UserMappingElement::new()
            .with_subject_claim("user_id")
            .with_email_claim("mail")
            .with_roles_claim("groups")
            .with_fhir_resource_type("Patient")
            .with_auto_provision(true);

        let config = mapping.to_user_mapping_config();

        assert_eq!(config.subject_claim, "user_id");
        assert_eq!(config.fhir_resource_type, FhirUserType::Patient);
        assert!(config.auto_provision);
    }
}
