//! User resource type.
//!
//! This module provides the Rust representation of the User
//! FHIR resource, used for storing authentication user data
//! linked to external identity providers.
//!
//! # Example
//!
//! ```ignore
//! use octofhir_auth::federation::resources::UserResource;
//!
//! let user = UserResource::new("alice")
//!     .with_id("user-123")
//!     .with_email("alice@example.com")
//!     .with_active(true);
//!
//! user.validate()?;
//! ```

use serde::{Deserialize, Serialize};

/// User FHIR resource for authentication.
///
/// This resource stores user accounts for the authentication system.
/// Users can be linked to FHIR Practitioner, Patient, Person, or
/// RelatedPerson resources via the `fhir_user` field.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserResource {
    /// Always "User".
    pub resource_type: String,

    /// Resource ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Username for authentication.
    pub username: String,

    /// BCrypt-hashed password (None for federated/SSO users).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password_hash: Option<String>,

    /// Email address.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,

    /// Reference to the FHIR resource this user represents.
    ///
    /// Used for SMART on FHIR fhirUser claim.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fhir_user: Option<Reference>,

    /// Whether this user account is active.
    pub active: bool,

    /// Roles assigned to this user for authorization.
    #[serde(default)]
    pub roles: Vec<String>,

    /// When the user last successfully authenticated.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_login: Option<String>,

    /// Whether multi-factor authentication is enabled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mfa_enabled: Option<bool>,

    /// Encrypted TOTP secret for MFA.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mfa_secret: Option<String>,

    /// Links to external identity providers.
    #[serde(default)]
    pub identity: Vec<UserIdentityElement>,
}

impl Default for UserResource {
    fn default() -> Self {
        Self {
            resource_type: "User".to_string(),
            id: None,
            username: String::new(),
            password_hash: None,
            email: None,
            fhir_user: None,
            active: true,
            roles: Vec::new(),
            last_login: None,
            mfa_enabled: None,
            mfa_secret: None,
            identity: Vec::new(),
        }
    }
}

impl UserResource {
    /// Creates a new User with the given username.
    #[must_use]
    pub fn new(username: impl Into<String>) -> Self {
        Self {
            username: username.into(),
            ..Default::default()
        }
    }

    /// Sets the resource ID.
    #[must_use]
    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Sets the password hash.
    #[must_use]
    pub fn with_password_hash(mut self, hash: impl Into<String>) -> Self {
        self.password_hash = Some(hash.into());
        self
    }

    /// Sets the email address.
    #[must_use]
    pub fn with_email(mut self, email: impl Into<String>) -> Self {
        self.email = Some(email.into());
        self
    }

    /// Sets the FHIR user reference.
    #[must_use]
    pub fn with_fhir_user(mut self, reference: impl Into<String>) -> Self {
        self.fhir_user = Some(Reference {
            reference: Some(reference.into()),
            display: None,
        });
        self
    }

    /// Sets whether the user is active.
    #[must_use]
    pub fn with_active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }

    /// Sets the roles.
    #[must_use]
    pub fn with_roles(mut self, roles: Vec<String>) -> Self {
        self.roles = roles;
        self
    }

    /// Adds a role to the user.
    #[must_use]
    pub fn with_role(mut self, role: impl Into<String>) -> Self {
        self.roles.push(role.into());
        self
    }

    /// Sets the last login timestamp.
    #[must_use]
    pub fn with_last_login(mut self, timestamp: impl Into<String>) -> Self {
        self.last_login = Some(timestamp.into());
        self
    }

    /// Sets whether MFA is enabled.
    #[must_use]
    pub fn with_mfa_enabled(mut self, enabled: bool) -> Self {
        self.mfa_enabled = Some(enabled);
        self
    }

    /// Sets the MFA secret.
    #[must_use]
    pub fn with_mfa_secret(mut self, secret: impl Into<String>) -> Self {
        self.mfa_secret = Some(secret.into());
        self
    }

    /// Adds an identity link.
    #[must_use]
    pub fn with_identity(mut self, identity: UserIdentityElement) -> Self {
        self.identity.push(identity);
        self
    }

    /// Sets all identity links.
    #[must_use]
    pub fn with_identities(mut self, identities: Vec<UserIdentityElement>) -> Self {
        self.identity = identities;
        self
    }

    /// Validates the resource.
    ///
    /// # Errors
    ///
    /// Returns a `ValidationError` if:
    /// - Username is empty
    /// - Email format is invalid
    /// - fhirUser reference type is invalid
    /// - Identity elements are incomplete
    pub fn validate(&self) -> Result<(), ValidationError> {
        // Username is required
        if self.username.is_empty() {
            return Err(ValidationError::MissingField("username"));
        }

        // Validate email format if present
        if let Some(ref email) = self.email
            && !email.contains('@')
        {
            return Err(ValidationError::InvalidField(
                "email",
                "Invalid email format",
            ));
        }

        // Validate fhirUser reference if present
        if let Some(ref fhir_user) = self.fhir_user
            && let Some(ref reference) = fhir_user.reference
        {
            self.validate_fhir_user_reference(reference)?;
        }

        // Validate identity elements
        for (idx, identity) in self.identity.iter().enumerate() {
            if identity.external_id.is_empty() {
                return Err(ValidationError::InvalidFieldIndexed(
                    "identity",
                    idx,
                    "externalId",
                    "External ID is required",
                ));
            }
            if identity.provider.reference.is_none() {
                return Err(ValidationError::InvalidFieldIndexed(
                    "identity",
                    idx,
                    "provider",
                    "Provider reference is required",
                ));
            }
        }

        Ok(())
    }

    /// Validates fhirUser reference format and type.
    fn validate_fhir_user_reference(&self, reference: &str) -> Result<(), ValidationError> {
        // Should be ResourceType/id format
        let parts: Vec<&str> = reference.split('/').collect();
        if parts.len() < 2 {
            return Err(ValidationError::InvalidField(
                "fhirUser.reference",
                "Must be ResourceType/id format",
            ));
        }

        let resource_type = parts[parts.len() - 2];
        match resource_type {
            "Practitioner" | "Patient" | "RelatedPerson" | "Person" => Ok(()),
            _ => Err(ValidationError::InvalidField(
                "fhirUser.reference",
                "Must reference Practitioner, Patient, RelatedPerson, or Person",
            )),
        }
    }
}

/// External identity link backbone element.
///
/// Links a user to an external identity provider account.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UserIdentityElement {
    /// Reference to the IdentityProvider resource.
    pub provider: Reference,

    /// The subject identifier from the external identity provider.
    pub external_id: String,

    /// Email address provided by the identity provider.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,

    /// When this identity was linked to the user account.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub linked_at: Option<String>,
}

impl UserIdentityElement {
    /// Creates a new identity element.
    #[must_use]
    pub fn new(provider_id: impl Into<String>, external_id: impl Into<String>) -> Self {
        let provider_id = provider_id.into();
        Self {
            provider: Reference {
                reference: Some(format!("IdentityProvider/{}", provider_id)),
                display: None,
            },
            external_id: external_id.into(),
            email: None,
            linked_at: None,
        }
    }

    /// Sets the email from the identity provider.
    #[must_use]
    pub fn with_email(mut self, email: impl Into<String>) -> Self {
        self.email = Some(email.into());
        self
    }

    /// Sets the linked timestamp.
    #[must_use]
    pub fn with_linked_at(mut self, timestamp: impl Into<String>) -> Self {
        self.linked_at = Some(timestamp.into());
        self
    }

    /// Sets the provider display name.
    #[must_use]
    pub fn with_provider_display(mut self, display: impl Into<String>) -> Self {
        self.provider.display = Some(display.into());
        self
    }
}

/// FHIR Reference datatype.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Reference {
    /// Literal reference (ResourceType/id).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,

    /// Text alternative for the resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display: Option<String>,
}

impl Reference {
    /// Creates a new reference.
    #[must_use]
    pub fn new(reference: impl Into<String>) -> Self {
        Self {
            reference: Some(reference.into()),
            display: None,
        }
    }

    /// Sets the display text.
    #[must_use]
    pub fn with_display(mut self, display: impl Into<String>) -> Self {
        self.display = Some(display.into());
        self
    }
}

/// Validation errors for User resources.
#[derive(Debug, thiserror::Error)]
pub enum ValidationError {
    /// A required field is missing or empty.
    #[error("Missing required field: {0}")]
    MissingField(&'static str),

    /// A field has an invalid value.
    #[error("Invalid field {0}: {1}")]
    InvalidField(&'static str, &'static str),

    /// A field at a specific index has an invalid value.
    #[error("Invalid field {0}[{1}].{2}: {3}")]
    InvalidFieldIndexed(&'static str, usize, &'static str, &'static str),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_user_resource() {
        let user = UserResource::new("alice")
            .with_id("user-123")
            .with_email("alice@example.com")
            .with_active(true);

        assert!(user.validate().is_ok());
    }

    #[test]
    fn test_missing_username() {
        let user = UserResource::default();

        let result = user.validate();
        assert!(matches!(
            result,
            Err(ValidationError::MissingField("username"))
        ));
    }

    #[test]
    fn test_invalid_email() {
        let user = UserResource::new("alice").with_email("not-an-email");

        let result = user.validate();
        assert!(matches!(
            result,
            Err(ValidationError::InvalidField("email", _))
        ));
    }

    #[test]
    fn test_invalid_fhir_user_reference_format() {
        let user = UserResource::new("alice").with_fhir_user("invalid");

        let result = user.validate();
        assert!(matches!(
            result,
            Err(ValidationError::InvalidField("fhirUser.reference", _))
        ));
    }

    #[test]
    fn test_invalid_fhir_user_reference_type() {
        let user = UserResource::new("alice").with_fhir_user("Observation/123");

        let result = user.validate();
        assert!(matches!(
            result,
            Err(ValidationError::InvalidField("fhirUser.reference", _))
        ));
    }

    #[test]
    fn test_valid_fhir_user_references() {
        for resource_type in ["Practitioner", "Patient", "RelatedPerson", "Person"] {
            let user = UserResource::new("alice").with_fhir_user(format!("{}/123", resource_type));
            assert!(user.validate().is_ok(), "Should accept {}", resource_type);
        }
    }

    #[test]
    fn test_identity_missing_external_id() {
        let identity = UserIdentityElement {
            provider: Reference::new("IdentityProvider/google"),
            external_id: String::new(),
            email: None,
            linked_at: None,
        };

        let user = UserResource::new("alice").with_identity(identity);

        let result = user.validate();
        assert!(matches!(
            result,
            Err(ValidationError::InvalidFieldIndexed(
                "identity",
                0,
                "externalId",
                _
            ))
        ));
    }

    #[test]
    fn test_identity_missing_provider() {
        let identity = UserIdentityElement {
            provider: Reference::default(),
            external_id: "ext-123".to_string(),
            email: None,
            linked_at: None,
        };

        let user = UserResource::new("alice").with_identity(identity);

        let result = user.validate();
        assert!(matches!(
            result,
            Err(ValidationError::InvalidFieldIndexed(
                "identity",
                0,
                "provider",
                _
            ))
        ));
    }

    #[test]
    fn test_valid_user_with_identity() {
        let identity = UserIdentityElement::new("google", "google-abc123")
            .with_email("alice@gmail.com")
            .with_provider_display("Google");

        let user = UserResource::new("alice")
            .with_email("alice@example.com")
            .with_identity(identity);

        assert!(user.validate().is_ok());
    }

    #[test]
    fn test_user_with_roles() {
        let user = UserResource::new("admin")
            .with_role("admin")
            .with_role("practitioner");

        assert_eq!(user.roles, vec!["admin", "practitioner"]);
    }

    #[test]
    fn test_serialization() {
        let user = UserResource::new("alice")
            .with_id("user-123")
            .with_email("alice@example.com")
            .with_active(true);

        let json = serde_json::to_string(&user).unwrap();
        assert!(json.contains(r#""resourceType":"User""#));
        assert!(json.contains(r#""username":"alice""#));
        assert!(json.contains(r#""email":"alice@example.com""#));

        let deserialized: UserResource = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.username, "alice");
        assert_eq!(deserialized.id, Some("user-123".to_string()));
    }

    #[test]
    fn test_serialization_with_identity() {
        let identity =
            UserIdentityElement::new("google", "google-abc123").with_email("alice@gmail.com");

        let user = UserResource::new("alice").with_identity(identity);

        let json = serde_json::to_string(&user).unwrap();
        assert!(json.contains(r#""identity""#));
        assert!(json.contains(r#""externalId":"google-abc123""#));
        assert!(json.contains(r#""provider""#));
        assert!(json.contains(r#""IdentityProvider/google""#));

        let deserialized: UserResource = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.identity.len(), 1);
        assert_eq!(deserialized.identity[0].external_id, "google-abc123");
    }

    #[test]
    fn test_fhir_user_serialization() {
        let user = UserResource::new("alice").with_fhir_user("Practitioner/123");

        let json = serde_json::to_string(&user).unwrap();
        assert!(json.contains(r#""fhirUser""#));
        assert!(json.contains(r#""reference":"Practitioner/123""#));
    }

    #[test]
    fn test_identity_element_builder() {
        let identity = UserIdentityElement::new("okta", "okta-xyz789")
            .with_email("alice@company.com")
            .with_linked_at("2024-01-15T10:30:00Z")
            .with_provider_display("Okta SSO");

        assert_eq!(
            identity.provider.reference,
            Some("IdentityProvider/okta".to_string())
        );
        assert_eq!(identity.provider.display, Some("Okta SSO".to_string()));
        assert_eq!(identity.external_id, "okta-xyz789");
        assert_eq!(identity.email, Some("alice@company.com".to_string()));
        assert_eq!(identity.linked_at, Some("2024-01-15T10:30:00Z".to_string()));
    }

    #[test]
    fn test_reference_builder() {
        let reference = Reference::new("Patient/456").with_display("John Doe");

        assert_eq!(reference.reference, Some("Patient/456".to_string()));
        assert_eq!(reference.display, Some("John Doe".to_string()));
    }

    #[test]
    fn test_default_values() {
        let user = UserResource::default();

        assert_eq!(user.resource_type, "User");
        assert!(user.id.is_none());
        assert!(user.username.is_empty());
        assert!(user.active);
        assert!(user.roles.is_empty());
        assert!(user.identity.is_empty());
    }
}
