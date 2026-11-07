//! External identity provider configuration.
//!
//! This module provides types for configuring external identity providers
//! for federated authentication flows.
//!
//! # Example
//!
//! ```ignore
//! use octofhir_auth::federation::provider::{IdentityProviderConfig, UserMappingConfig, FhirUserType};
//! use url::Url;
//!
//! let config = IdentityProviderConfig::new(
//!     "google",
//!     "Google",
//!     Url::parse("https://accounts.google.com")?,
//!     "your-client-id",
//! )
//! .with_client_secret("your-client-secret")
//! .with_scopes(vec!["openid", "email", "profile"]);
//! ```

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use url::Url;

/// Configuration for an external identity provider.
///
/// This represents an OIDC-compatible identity provider that can be used
/// for federated authentication.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityProviderConfig {
    /// Unique identifier for this provider (e.g., "google", "azure-ad").
    pub id: String,

    /// Human-readable name for display (e.g., "Google", "Azure AD").
    pub name: String,

    /// The OIDC issuer URL (e.g., <https://accounts.google.com>).
    pub issuer: Url,

    /// OAuth client ID registered with the provider.
    pub client_id: String,

    /// OAuth client secret (None for public clients).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<String>,

    /// OAuth scopes to request (default: ["openid"]).
    #[serde(default = "default_scopes")]
    pub scopes: Vec<String>,

    /// Whether this provider is enabled.
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// User mapping configuration.
    #[serde(default)]
    pub user_mapping: UserMappingConfig,

    /// Optional override for the authorization endpoint.
    /// If not set, discovered from OIDC metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authorization_endpoint: Option<String>,

    /// Optional override for the token endpoint.
    /// If not set, discovered from OIDC metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_endpoint: Option<String>,

    /// Optional override for the userinfo endpoint.
    /// If not set, discovered from OIDC metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub userinfo_endpoint: Option<String>,

    /// Optional additional parameters to include in the authorization URL.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extra_auth_params: Vec<(String, String)>,

    /// A logo for its button on sign-in pages.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logo_url: Option<String>,

    /// How to talk to it.
    #[serde(default)]
    pub protocol: Protocol,

    /// For SAML: the provider's entity ID as written, and its metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub saml: Option<SamlProvider>,
}

/// What a SAML provider is known by.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SamlProvider {
    pub entity_id: String,
    pub metadata: String,
}

/// What a provider speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    /// OpenID Connect: the user comes in a signed ID token.
    #[default]
    Oidc,
    /// OAuth 2.0 alone: the user comes from a userinfo endpoint.
    Oauth2,
    /// SAML 2.0: the user comes in a signed assertion.
    Saml,
    /// UDAP Tiered OAuth: OpenID Connect with a provider this server registered with by UDAP
    /// Dynamic Client Registration; it authenticates with a signed JWT (`private_key_jwt`).
    Udap,
}

fn default_scopes() -> Vec<String> {
    vec!["openid".to_string()]
}

fn default_true() -> bool {
    true
}

impl IdentityProviderConfig {
    /// Creates a new identity provider configuration with required fields.
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        issuer: Url,
        client_id: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            issuer,
            client_id: client_id.into(),
            client_secret: None,
            scopes: default_scopes(),
            enabled: true,
            user_mapping: UserMappingConfig::default(),
            logo_url: None,
            protocol: Protocol::default(),
            saml: None,
            authorization_endpoint: None,
            token_endpoint: None,
            userinfo_endpoint: None,
            extra_auth_params: Vec::new(),
        }
    }

    /// Sets the client secret.
    #[must_use]
    pub fn with_client_secret(mut self, secret: impl Into<String>) -> Self {
        self.client_secret = Some(secret.into());
        self
    }

    /// Sets the OAuth scopes.
    #[must_use]
    pub fn with_scopes(mut self, scopes: Vec<impl Into<String>>) -> Self {
        self.scopes = scopes.into_iter().map(Into::into).collect();
        self
    }

    /// Adds a scope to the existing scopes.
    #[must_use]
    pub fn with_scope(mut self, scope: impl Into<String>) -> Self {
        self.scopes.push(scope.into());
        self
    }

    /// Sets whether the provider is enabled.
    #[must_use]
    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Sets the user mapping configuration.
    #[must_use]
    pub fn with_user_mapping(mut self, mapping: UserMappingConfig) -> Self {
        self.user_mapping = mapping;
        self
    }

    /// Sets the authorization endpoint override.
    #[must_use]
    pub fn with_authorization_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.authorization_endpoint = Some(endpoint.into());
        self
    }

    /// Sets the token endpoint override.
    #[must_use]
    pub fn with_token_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.token_endpoint = Some(endpoint.into());
        self
    }

    /// Sets the userinfo endpoint override.
    #[must_use]
    pub fn with_userinfo_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.userinfo_endpoint = Some(endpoint.into());
        self
    }

    /// Adds an extra authorization parameter.
    #[must_use]
    pub fn with_extra_auth_param(
        mut self,
        key: impl Into<String>,
        value: impl Into<String>,
    ) -> Self {
        self.extra_auth_params.push((key.into(), value.into()));
        self
    }

    /// Returns `true` if this provider uses a confidential client (has a secret).
    #[must_use]
    pub fn is_confidential(&self) -> bool {
        self.client_secret.is_some()
    }

    /// Returns `true` if this provider uses a public client (no secret).
    #[must_use]
    pub fn is_public(&self) -> bool {
        self.client_secret.is_none()
    }
}

/// Configuration for mapping IdP claims to user attributes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserMappingConfig {
    /// The claim to use as the subject identifier (default: "sub").
    #[serde(default = "default_subject_claim")]
    pub subject_claim: String,

    /// The claim containing the user's email address.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email_claim: Option<String>,

    /// The claim containing the user's full name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name_claim: Option<String>,

    /// The claim containing the user's given name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub given_name_claim: Option<String>,

    /// The claim containing the user's family name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub family_name_claim: Option<String>,

    /// The claim containing the user's roles (expected to be an array or comma-separated).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roles_claim: Option<String>,

    /// The claim with a name for the user (default: "preferred_username").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username_claim: Option<String>,

    /// Rules that turn values of claims into roles.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub claims_to_roles: Vec<ClaimMapping>,

    /// The FHIR resource type to associate with users from this provider.
    #[serde(default)]
    pub fhir_resource_type: FhirUserType,

    /// Whether to auto-provision users on first login.
    #[serde(default = "default_true")]
    pub auto_provision: bool,
}

impl UserMappingConfig {
    /// Whether roles come from the provider rather than from this server.
    #[must_use]
    pub fn manages_roles(&self) -> bool {
        self.roles_claim.is_some() || !self.claims_to_roles.is_empty()
    }
}

/// A rule that turns a value of a claim into roles.
///
/// `value` is compared with the whole value of the claim. It is a literal, a pattern with `*`, or
/// a regular expression when it starts with `^` and ends with `$`. What `*` or a group matched
/// can be used in `roles` as `$1`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaimMapping {
    /// The claim; a dotted path reaches into objects, as in `realm_access.roles`.
    pub claim: String,
    pub value: String,
    pub roles: Vec<String>,
}

impl ClaimMapping {
    /// What `value` means as a regular expression.
    pub fn pattern(&self) -> Result<Regex, regex::Error> {
        if self.value.starts_with('^') && self.value.ends_with('$') {
            return Regex::new(&self.value);
        }
        let parts: Vec<String> = self.value.split('*').map(regex::escape).collect();
        Regex::new(&format!("^{}$", parts.join("(.*)")))
    }

    /// The roles this rule gives for `claims`.
    pub fn roles_for(&self, claims: &Value) -> Vec<String> {
        let Ok(pattern) = self.pattern() else {
            return Vec::new();
        };
        let mut found = Vec::new();
        for value in claim_values(claims, &self.claim) {
            let Some(captures) = pattern.captures(&value) else {
                continue;
            };
            for role in &self.roles {
                let mut expanded = String::new();
                captures.expand(role, &mut expanded);
                if !expanded.is_empty() {
                    found.push(expanded);
                }
            }
        }
        found
    }
}

/// The values of a claim: a list gives each of its items. A name is first looked up as it is, for
/// claims that are URLs, then as a path.
pub(crate) fn claim_values(claims: &Value, name: &str) -> Vec<String> {
    let found = claims.get(name).or_else(|| {
        name.split('.')
            .try_fold(claims, |value, part| value.get(part))
    });
    let text = |value: &Value| match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(flag) => Some(flag.to_string()),
        _ => None,
    };
    match found {
        Some(Value::Array(items)) => items.iter().filter_map(text).collect(),
        Some(value) => text(value).into_iter().collect(),
        None => Vec::new(),
    }
}

fn default_subject_claim() -> String {
    "sub".to_string()
}

impl Default for UserMappingConfig {
    fn default() -> Self {
        Self {
            subject_claim: default_subject_claim(),
            email_claim: Some("email".to_string()),
            name_claim: Some("name".to_string()),
            given_name_claim: Some("given_name".to_string()),
            family_name_claim: Some("family_name".to_string()),
            roles_claim: None,
            username_claim: None,
            claims_to_roles: Vec::new(),
            fhir_resource_type: FhirUserType::default(),
            auto_provision: true,
        }
    }
}

impl UserMappingConfig {
    /// Creates a new user mapping configuration with default values.
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

    /// Sets the name claim.
    #[must_use]
    pub fn with_name_claim(mut self, claim: impl Into<String>) -> Self {
        self.name_claim = Some(claim.into());
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
    pub fn with_fhir_resource_type(mut self, resource_type: FhirUserType) -> Self {
        self.fhir_resource_type = resource_type;
        self
    }

    /// Sets whether to auto-provision users.
    #[must_use]
    pub fn with_auto_provision(mut self, auto: bool) -> Self {
        self.auto_provision = auto;
        self
    }
}

/// The FHIR resource type to associate with users from an identity provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum FhirUserType {
    /// A Practitioner resource (healthcare provider).
    #[default]
    Practitioner,
    /// A Patient resource.
    Patient,
    /// A RelatedPerson resource (patient's relative, guardian, etc.).
    RelatedPerson,
    /// A Person resource (generic person).
    Person,
}

impl FhirUserType {
    /// Returns the FHIR resource type as a string.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Practitioner => "Practitioner",
            Self::Patient => "Patient",
            Self::RelatedPerson => "RelatedPerson",
            Self::Person => "Person",
        }
    }

    /// Returns the FHIR user claim format (e.g., "Practitioner/123").
    #[must_use]
    pub fn format_fhir_user(&self, id: &str) -> String {
        format!("{}/{}", self.as_str(), id)
    }
}

impl std::fmt::Display for FhirUserType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Parsed user information mapped from IdP claims.
#[derive(Debug, Clone)]
pub struct MappedUser {
    /// External subject identifier from the IdP.
    pub external_id: String,

    /// User's email address.
    pub email: Option<String>,

    /// Whether the email is verified.
    pub email_verified: Option<bool>,

    /// User's full name.
    pub name: Option<String>,

    /// User's given (first) name.
    pub given_name: Option<String>,

    /// User's family (last) name.
    pub family_name: Option<String>,

    /// User's preferred username.
    pub preferred_username: Option<String>,

    /// Roles assigned to the user.
    pub roles: Vec<String>,

    /// FHIR resource type for this user.
    pub fhir_user_type: FhirUserType,
}

impl MappedUser {
    /// Creates a new mapped user with the external ID.
    #[must_use]
    pub fn new(external_id: impl Into<String>, fhir_user_type: FhirUserType) -> Self {
        Self {
            external_id: external_id.into(),
            email: None,
            email_verified: None,
            name: None,
            given_name: None,
            family_name: None,
            preferred_username: None,
            roles: Vec::new(),
            fhir_user_type,
        }
    }

    /// Returns the display name (name or email or external_id).
    #[must_use]
    pub fn display_name(&self) -> &str {
        self.name
            .as_deref()
            .or(self.email.as_deref())
            .unwrap_or(&self.external_id)
    }

    /// Returns `true` if the user has a verified email.
    #[must_use]
    pub fn has_verified_email(&self) -> bool {
        self.email.is_some() && self.email_verified == Some(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provider_config_builder() {
        let issuer = Url::parse("https://accounts.google.com").unwrap();
        let config = IdentityProviderConfig::new("google", "Google", issuer.clone(), "client-123")
            .with_client_secret("secret-456")
            .with_scopes(vec!["openid", "email", "profile"])
            .with_enabled(true);

        assert_eq!(config.id, "google");
        assert_eq!(config.name, "Google");
        assert_eq!(config.issuer, issuer);
        assert_eq!(config.client_id, "client-123");
        assert_eq!(config.client_secret, Some("secret-456".to_string()));
        assert_eq!(config.scopes.len(), 3);
        assert!(config.enabled);
        assert!(config.is_confidential());
        assert!(!config.is_public());
    }

    #[test]
    fn test_provider_config_public_client() {
        let issuer = Url::parse("https://auth.example.com").unwrap();
        let config = IdentityProviderConfig::new("spa", "SPA Client", issuer, "public-client");

        assert!(config.is_public());
        assert!(!config.is_confidential());
    }

    #[test]
    fn test_user_mapping_defaults() {
        let mapping = UserMappingConfig::default();

        assert_eq!(mapping.subject_claim, "sub");
        assert_eq!(mapping.email_claim, Some("email".to_string()));
        assert_eq!(mapping.name_claim, Some("name".to_string()));
        assert!(mapping.auto_provision);
        assert_eq!(mapping.fhir_resource_type, FhirUserType::Practitioner);
    }

    #[test]
    fn test_user_mapping_builder() {
        let mapping = UserMappingConfig::new()
            .with_subject_claim("user_id")
            .with_email_claim("mail")
            .with_roles_claim("groups")
            .with_fhir_resource_type(FhirUserType::Patient)
            .with_auto_provision(false);

        assert_eq!(mapping.subject_claim, "user_id");
        assert_eq!(mapping.email_claim, Some("mail".to_string()));
        assert_eq!(mapping.roles_claim, Some("groups".to_string()));
        assert_eq!(mapping.fhir_resource_type, FhirUserType::Patient);
        assert!(!mapping.auto_provision);
    }

    #[test]
    fn test_fhir_user_type() {
        assert_eq!(FhirUserType::Practitioner.as_str(), "Practitioner");
        assert_eq!(FhirUserType::Patient.as_str(), "Patient");
        assert_eq!(FhirUserType::RelatedPerson.as_str(), "RelatedPerson");
        assert_eq!(FhirUserType::Person.as_str(), "Person");

        assert_eq!(
            FhirUserType::Practitioner.format_fhir_user("123"),
            "Practitioner/123"
        );
        assert_eq!(FhirUserType::Patient.format_fhir_user("abc"), "Patient/abc");
    }

    #[test]
    fn test_mapped_user() {
        let mut user = MappedUser::new("ext-123", FhirUserType::Patient);
        assert_eq!(user.display_name(), "ext-123");

        user.email = Some("user@example.com".to_string());
        assert_eq!(user.display_name(), "user@example.com");

        user.name = Some("John Doe".to_string());
        assert_eq!(user.display_name(), "John Doe");

        assert!(!user.has_verified_email());
        user.email_verified = Some(true);
        assert!(user.has_verified_email());
    }

    #[test]
    fn test_provider_config_serialization() {
        let issuer = Url::parse("https://auth.example.com").unwrap();
        let config = IdentityProviderConfig::new("test", "Test Provider", issuer, "client-id");

        let json = serde_json::to_string(&config).unwrap();
        let deserialized: IdentityProviderConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.id, config.id);
        assert_eq!(deserialized.client_id, config.client_id);
    }

    #[test]
    fn test_fhir_user_type_serialization() {
        let practitioner = FhirUserType::Practitioner;
        let json = serde_json::to_string(&practitioner).unwrap();
        assert_eq!(json, "\"Practitioner\"");

        let patient = FhirUserType::Patient;
        let json = serde_json::to_string(&patient).unwrap();
        assert_eq!(json, "\"Patient\"");
    }

    fn rule(claim: &str, value: &str, roles: &[&str]) -> ClaimMapping {
        ClaimMapping {
            claim: claim.to_owned(),
            value: value.to_owned(),
            roles: roles.iter().map(|role| (*role).to_owned()).collect(),
        }
    }

    #[test]
    fn a_rule_matches_the_whole_value() {
        let claims = serde_json::json!({"groups": ["fhir-admins", "not-fhir-admins"]});
        assert_eq!(
            rule("groups", "fhir-admins", &["admin"]).roles_for(&claims),
            ["admin"]
        );
        assert!(
            rule("groups", "fhir", &["admin"])
                .roles_for(&claims)
                .is_empty()
        );
        // A dot in a literal is a dot.
        let claims = serde_json::json!({"groups": ["aXb"]});
        assert!(
            rule("groups", "a.b", &["admin"])
                .roles_for(&claims)
                .is_empty()
        );
    }

    #[test]
    fn what_a_pattern_caught_goes_into_the_role() {
        let claims = serde_json::json!({"groups": ["fhir-viewer", "fhir-editor", "other"]});
        assert_eq!(
            rule("groups", "fhir-*", &["$1"]).roles_for(&claims),
            ["viewer", "editor"]
        );
        assert_eq!(
            rule("groups", "^fhir-(view|edit)[eo]r$", &["can-${1}"]).roles_for(&claims),
            ["can-view", "can-edit"]
        );
    }

    #[test]
    fn claims_can_be_nested_or_named_like_urls() {
        let claims = serde_json::json!({
            "realm_access": {"roles": ["clinician"]},
            "https://example.org/claims/groups": ["team"],
            "department": "cardiology",
            "level": 3,
        });
        let roles = |name, value| rule(name, value, &["x"]).roles_for(&claims).len();
        assert_eq!(roles("realm_access.roles", "clinician"), 1);
        assert_eq!(roles("https://example.org/claims/groups", "team"), 1);
        assert_eq!(roles("department", "cardiology"), 1);
        assert_eq!(roles("level", "3"), 1);
        assert_eq!(roles("missing", "*"), 0);
    }

    #[test]
    fn a_rule_that_is_no_regular_expression_gives_nothing() {
        let broken = rule("groups", "^(unclosed$", &["admin"]);
        assert!(broken.pattern().is_err());
        assert!(
            broken
                .roles_for(&serde_json::json!({"groups": ["a"]}))
                .is_empty()
        );
    }
}
