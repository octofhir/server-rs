//! User provisioning service for federated authentication.
//!
//! This module provides the [`UserProvisioningService`] for handling user
//! provisioning and identity linking during federated authentication.
//!
//! # Overview
//!
//! When a user authenticates via an external identity provider, the provisioning
//! service handles:
//!
//! 1. **Find by external identity** - Check if user is already linked
//! 2. **Find by email** - Check if user exists with same email (for linking)
//! 3. **Create user** - Auto-provision new users when allowed
//! 4. **Link identity** - Add external identity to existing user
//! 5. **Create FHIR resource** - Optionally create Practitioner/Patient resource
//!
//! # Example
//!
//! ```ignore
//! use octofhir_auth::federation::provisioning::{UserProvisioningService, ProvisioningConfig};
//!
//! let config = ProvisioningConfig::default();
//! let service = UserProvisioningService::new(user_storage, Some(fhir_storage), config);
//!
//! // Process authentication result
//! let result = service.provision_user(auth_result).await?;
//! println!("User {} provisioned", result.user.id);
//! ```

use super::auth::IdpAuthResult;
use super::identity::{UserIdentity, add_identity, get_identities};
use super::provider::{FhirUserType, MappedUser};
use crate::storage::User;

/// Configuration for user provisioning.
#[derive(Debug, Clone)]
pub struct ProvisioningConfig {
    /// Whether to auto-provision new users when they don't exist.
    /// Default: `false`
    pub auto_provision: bool,

    /// Whether to link identities by matching email address.
    /// When enabled, if a user with matching verified email exists,
    /// the external identity will be linked to that user.
    /// Default: `true`
    pub link_by_email: bool,

    /// Whether to create FHIR resources (Practitioner/Patient) for new users.
    /// Default: `false`
    pub create_fhir_resources: bool,

    /// Default roles to assign to newly provisioned users.
    pub default_roles: Vec<String>,
}

impl Default for ProvisioningConfig {
    fn default() -> Self {
        Self {
            auto_provision: false,
            link_by_email: true,
            create_fhir_resources: false,
            default_roles: Vec::new(),
        }
    }
}

impl ProvisioningConfig {
    /// Creates a new provisioning configuration.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Enables auto-provisioning of new users.
    #[must_use]
    pub fn with_auto_provision(mut self, enabled: bool) -> Self {
        self.auto_provision = enabled;
        self
    }

    /// Sets whether to link identities by email matching.
    #[must_use]
    pub fn with_link_by_email(mut self, enabled: bool) -> Self {
        self.link_by_email = enabled;
        self
    }

    /// Enables FHIR resource creation for new users.
    #[must_use]
    pub fn with_create_fhir_resources(mut self, enabled: bool) -> Self {
        self.create_fhir_resources = enabled;
        self
    }

    /// Sets the default roles for newly provisioned users.
    #[must_use]
    pub fn with_default_roles(mut self, roles: Vec<String>) -> Self {
        self.default_roles = roles;
        self
    }

    /// Adds a default role for newly provisioned users.
    #[must_use]
    pub fn with_default_role(mut self, role: impl Into<String>) -> Self {
        self.default_roles.push(role.into());
        self
    }
}

/// Result of user provisioning.
#[derive(Debug)]
pub struct ProvisioningResult {
    /// The provisioned or linked user.
    pub user: User,

    /// The action that was taken.
    pub action: ProvisioningAction,

    /// FHIR resource ID if one was created.
    pub fhir_resource_id: Option<String>,
}

/// The action taken during provisioning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProvisioningAction {
    /// An existing user was found by external identity (already linked).
    ExistingIdentity,

    /// An existing user was found by email and linked.
    LinkedByEmail,

    /// A new user was created (auto-provisioned).
    Created,
}

impl ProvisioningAction {
    /// Returns `true` if a new user was created.
    #[must_use]
    pub fn is_created(&self) -> bool {
        matches!(self, Self::Created)
    }

    /// Returns `true` if an identity was newly linked (by email).
    #[must_use]
    pub fn is_linked(&self) -> bool {
        matches!(self, Self::LinkedByEmail)
    }

    /// Returns `true` if an existing linked identity was found.
    #[must_use]
    pub fn is_existing(&self) -> bool {
        matches!(self, Self::ExistingIdentity)
    }
}

impl std::fmt::Display for ProvisioningAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ExistingIdentity => write!(f, "existing_identity"),
            Self::LinkedByEmail => write!(f, "linked_by_email"),
            Self::Created => write!(f, "created"),
        }
    }
}

/// Errors that can occur during user provisioning.
#[derive(Debug, thiserror::Error)]
pub enum ProvisioningError {
    /// User was not found and auto-provisioning is disabled.
    #[error("User not found and auto-provisioning is disabled")]
    UserNotFound,

    /// Storage operation failed.
    #[error("Storage error: {0}")]
    StorageError(String),

    /// FHIR resource creation failed.
    #[error("FHIR resource creation failed: {0}")]
    FhirCreationFailed(String),

    /// Email is required for email-based linking but not provided or verified.
    #[error("Verified email required for account linking")]
    EmailRequired,
}

/// Creates a new user from authentication result and configuration.
///
/// This is a helper function for creating a user with the proper attributes
/// from an IdP authentication result.
pub fn create_user_from_auth_result(
    auth_result: &IdpAuthResult,
    config: &ProvisioningConfig,
) -> User {
    let mapped = &auth_result.mapped_user;

    // Generate username from email or external ID
    let username = mapped
        .preferred_username
        .clone()
        .or_else(|| {
            mapped.email.as_ref().map(|e| {
                e.split('@')
                    .next()
                    .unwrap_or(&mapped.external_id)
                    .to_string()
            })
        })
        .unwrap_or_else(|| format!("{}_{}", auth_result.provider_id, mapped.external_id));

    // Combine default roles with mapped roles
    let mut roles = config.default_roles.clone();
    for role in &mapped.roles {
        if !roles.contains(role) {
            roles.push(role.clone());
        }
    }

    let mut user = User::builder(username).active(true).roles(roles).build();

    // Set email if available
    if let Some(email) = &mapped.email {
        user.email = Some(email.clone());
    }

    // Add the external identity
    let identity = create_identity_from_auth_result(auth_result);
    add_identity(&mut user, identity);

    user
}

/// Creates a `UserIdentity` from an authentication result.
pub fn create_identity_from_auth_result(auth_result: &IdpAuthResult) -> UserIdentity {
    let mut identity = UserIdentity::new(&auth_result.provider_id, &auth_result.external_subject);

    if let Some(email) = &auth_result.mapped_user.email {
        identity = identity.with_email(email);
    }

    identity
}

/// Checks if the user already has an identity from the given provider.
#[must_use]
pub fn has_provider_identity(user: &User, provider_id: &str) -> bool {
    get_identities(user)
        .iter()
        .any(|i| i.provider_id == provider_id)
}

/// Determines the best username for a mapped user.
///
/// Priority: preferred_username > email username part > external_id
#[must_use]
pub fn determine_username(mapped: &MappedUser, provider_id: &str) -> String {
    mapped
        .preferred_username
        .clone()
        .or_else(|| {
            mapped.email.as_ref().map(|e| {
                e.split('@')
                    .next()
                    .unwrap_or(&mapped.external_id)
                    .to_string()
            })
        })
        .unwrap_or_else(|| format!("{}_{}", provider_id, mapped.external_id))
}

/// Creates a FHIR resource JSON value for a user.
///
/// This creates a minimal Practitioner, Patient, RelatedPerson, or Person resource
/// based on the user type configured in the provider mapping.
#[must_use]
pub fn create_fhir_resource_json(
    fhir_user_type: FhirUserType,
    mapped: &MappedUser,
) -> serde_json::Value {
    let mut name = serde_json::Map::new();
    name.insert("use".to_string(), serde_json::json!("official"));

    if let Some(given) = &mapped.given_name {
        name.insert("given".to_string(), serde_json::json!([given]));
    }
    if let Some(family) = &mapped.family_name {
        name.insert("family".to_string(), serde_json::json!(family));
    }
    if let Some(full_name) = &mapped.name {
        name.insert("text".to_string(), serde_json::json!(full_name));
    }

    let mut resource = serde_json::json!({
        "resourceType": fhir_user_type.as_str(),
        "active": true
    });

    // Add name if we have any name information
    if name.len() > 1 {
        resource["name"] = serde_json::json!([name]);
    }

    // Add telecom (email) if available
    if let Some(email) = &mapped.email {
        resource["telecom"] = serde_json::json!([{
            "system": "email",
            "value": email,
            "use": "work"
        }]);
    }

    resource
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::federation::provider::FhirUserType;

    fn create_test_auth_result() -> IdpAuthResult {
        IdpAuthResult {
            provider_id: "google".to_string(),
            external_subject: "google-user-123".to_string(),
            mapped_user: MappedUser {
                external_id: "google-user-123".to_string(),
                email: Some("user@example.com".to_string()),
                email_verified: Some(true),
                name: Some("Test User".to_string()),
                given_name: Some("Test".to_string()),
                family_name: Some("User".to_string()),
                preferred_username: Some("testuser".to_string()),
                roles: vec!["user".to_string()],
                fhir_user_type: FhirUserType::Practitioner,
            },
            id_token: "mock-id-token".to_string(),
            access_token: "mock-access-token".to_string(),
            refresh_token: None,
            expires_in: Some(3600),
        }
    }

    #[test]
    fn test_provisioning_config_defaults() {
        let config = ProvisioningConfig::default();

        assert!(!config.auto_provision);
        assert!(config.link_by_email);
        assert!(!config.create_fhir_resources);
        assert!(config.default_roles.is_empty());
    }

    #[test]
    fn test_provisioning_config_builder() {
        let config = ProvisioningConfig::new()
            .with_auto_provision(true)
            .with_link_by_email(false)
            .with_create_fhir_resources(true)
            .with_default_role("user")
            .with_default_role("reader");

        assert!(config.auto_provision);
        assert!(!config.link_by_email);
        assert!(config.create_fhir_resources);
        assert_eq!(config.default_roles, vec!["user", "reader"]);
    }

    #[test]
    fn test_create_user_from_auth_result() {
        let auth_result = create_test_auth_result();
        let config = ProvisioningConfig::new().with_default_role("default-role");

        let user = create_user_from_auth_result(&auth_result, &config);

        assert_eq!(user.username, "testuser");
        assert_eq!(user.email, Some("user@example.com".to_string()));
        assert!(user.active);
        assert!(user.roles.contains(&"default-role".to_string()));
        assert!(user.roles.contains(&"user".to_string()));

        // Check identity was added
        let identities = get_identities(&user);
        assert_eq!(identities.len(), 1);
        assert_eq!(identities[0].provider_id, "google");
        assert_eq!(identities[0].external_subject, "google-user-123");
    }

    #[test]
    fn test_create_user_from_auth_result_no_preferred_username() {
        let mut auth_result = create_test_auth_result();
        auth_result.mapped_user.preferred_username = None;

        let config = ProvisioningConfig::new();
        let user = create_user_from_auth_result(&auth_result, &config);

        // Should use email username part
        assert_eq!(user.username, "user");
    }

    #[test]
    fn test_create_user_from_auth_result_no_email() {
        let mut auth_result = create_test_auth_result();
        auth_result.mapped_user.preferred_username = None;
        auth_result.mapped_user.email = None;

        let config = ProvisioningConfig::new();
        let user = create_user_from_auth_result(&auth_result, &config);

        // Should use provider_externalId format
        assert_eq!(user.username, "google_google-user-123");
    }

    #[test]
    fn test_create_identity_from_auth_result() {
        let auth_result = create_test_auth_result();
        let identity = create_identity_from_auth_result(&auth_result);

        assert_eq!(identity.provider_id, "google");
        assert_eq!(identity.external_subject, "google-user-123");
        assert_eq!(identity.email, Some("user@example.com".to_string()));
    }

    #[test]
    fn test_has_provider_identity() {
        let auth_result = create_test_auth_result();
        let config = ProvisioningConfig::new();
        let user = create_user_from_auth_result(&auth_result, &config);

        assert!(has_provider_identity(&user, "google"));
        assert!(!has_provider_identity(&user, "azure-ad"));
    }

    #[test]
    fn test_determine_username() {
        let mapped = MappedUser {
            external_id: "ext-123".to_string(),
            email: Some("user@example.com".to_string()),
            email_verified: Some(true),
            name: None,
            given_name: None,
            family_name: None,
            preferred_username: Some("preferred".to_string()),
            roles: Vec::new(),
            fhir_user_type: FhirUserType::Practitioner,
        };

        assert_eq!(determine_username(&mapped, "google"), "preferred");

        // Without preferred username, use email
        let mut mapped_no_pref = mapped.clone();
        mapped_no_pref.preferred_username = None;
        assert_eq!(determine_username(&mapped_no_pref, "google"), "user");

        // Without email, use external ID
        let mut mapped_no_email = mapped_no_pref.clone();
        mapped_no_email.email = None;
        assert_eq!(
            determine_username(&mapped_no_email, "google"),
            "google_ext-123"
        );
    }

    #[test]
    fn test_provisioning_action_display() {
        assert_eq!(
            ProvisioningAction::ExistingIdentity.to_string(),
            "existing_identity"
        );
        assert_eq!(
            ProvisioningAction::LinkedByEmail.to_string(),
            "linked_by_email"
        );
        assert_eq!(ProvisioningAction::Created.to_string(), "created");
    }

    #[test]
    fn test_provisioning_action_predicates() {
        assert!(ProvisioningAction::Created.is_created());
        assert!(!ProvisioningAction::Created.is_linked());

        assert!(ProvisioningAction::LinkedByEmail.is_linked());
        assert!(!ProvisioningAction::LinkedByEmail.is_existing());

        assert!(ProvisioningAction::ExistingIdentity.is_existing());
        assert!(!ProvisioningAction::ExistingIdentity.is_created());
    }

    #[test]
    fn test_create_fhir_resource_json_practitioner() {
        let mapped = MappedUser {
            external_id: "ext-123".to_string(),
            email: Some("doctor@hospital.com".to_string()),
            email_verified: Some(true),
            name: Some("Dr. John Smith".to_string()),
            given_name: Some("John".to_string()),
            family_name: Some("Smith".to_string()),
            preferred_username: None,
            roles: Vec::new(),
            fhir_user_type: FhirUserType::Practitioner,
        };

        let resource = create_fhir_resource_json(FhirUserType::Practitioner, &mapped);

        assert_eq!(resource["resourceType"], "Practitioner");
        assert_eq!(resource["active"], true);
        assert_eq!(resource["name"][0]["family"], "Smith");
        assert_eq!(resource["name"][0]["given"][0], "John");
        assert_eq!(resource["name"][0]["text"], "Dr. John Smith");
        assert_eq!(resource["telecom"][0]["system"], "email");
        assert_eq!(resource["telecom"][0]["value"], "doctor@hospital.com");
    }

    #[test]
    fn test_create_fhir_resource_json_patient() {
        let mapped = MappedUser {
            external_id: "ext-456".to_string(),
            email: Some("patient@email.com".to_string()),
            email_verified: Some(true),
            name: None,
            given_name: Some("Jane".to_string()),
            family_name: Some("Doe".to_string()),
            preferred_username: None,
            roles: Vec::new(),
            fhir_user_type: FhirUserType::Patient,
        };

        let resource = create_fhir_resource_json(FhirUserType::Patient, &mapped);

        assert_eq!(resource["resourceType"], "Patient");
        assert_eq!(resource["active"], true);
        assert_eq!(resource["name"][0]["family"], "Doe");
        assert_eq!(resource["name"][0]["given"][0], "Jane");
    }

    #[test]
    fn test_create_fhir_resource_json_minimal() {
        let mapped = MappedUser {
            external_id: "ext-789".to_string(),
            email: None,
            email_verified: None,
            name: None,
            given_name: None,
            family_name: None,
            preferred_username: None,
            roles: Vec::new(),
            fhir_user_type: FhirUserType::Person,
        };

        let resource = create_fhir_resource_json(FhirUserType::Person, &mapped);

        assert_eq!(resource["resourceType"], "Person");
        assert_eq!(resource["active"], true);
        // No name or telecom since no data provided
        assert!(resource.get("name").is_none());
        assert!(resource.get("telecom").is_none());
    }
}
