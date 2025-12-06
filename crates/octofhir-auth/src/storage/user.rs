//! User storage trait.
//!
//! Defines the interface for user persistence operations.
//! Implementations are provided by storage backends (e.g., PostgreSQL).

use std::collections::HashMap;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::AuthResult;

// =============================================================================
// User Type
// =============================================================================

/// A user in the authentication system.
///
/// Users can authenticate and obtain access tokens to interact with
/// FHIR resources based on their assigned scopes and permissions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    /// Unique identifier for the user.
    pub id: Uuid,

    /// Username for authentication.
    pub username: String,

    /// Email address (optional, for notifications/recovery).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,

    /// BCrypt-hashed password (None for federated/SSO users).
    ///
    /// Note: This field is stored in the database for password authentication.
    /// When exposing User via API, filter this field out manually for security.
    #[serde(default)]
    pub password_hash: Option<String>,

    /// Reference to user's FHIR resource (e.g., "Practitioner/123").
    ///
    /// Used to set the `fhir_user` claim in access tokens.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fhir_user: Option<String>,

    /// User roles for authorization.
    ///
    /// Examples: "admin", "practitioner", "patient", "system"
    #[serde(default)]
    pub roles: Vec<String>,

    /// Additional user attributes for policy evaluation.
    ///
    /// Key-value pairs that can be used in access policies.
    #[serde(default)]
    pub attributes: HashMap<String, serde_json::Value>,

    /// Whether the user account is active.
    ///
    /// Inactive users cannot authenticate.
    pub active: bool,

    /// When the user was created.
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,

    /// When the user was last updated.
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

impl User {
    /// Creates a new user with the given username.
    ///
    /// The user is active by default with no password (federated/SSO).
    #[must_use]
    pub fn new(username: impl Into<String>) -> Self {
        let now = OffsetDateTime::now_utc();
        Self {
            id: Uuid::new_v4(),
            username: username.into(),
            email: None,
            password_hash: None,
            fhir_user: None,
            roles: Vec::new(),
            attributes: HashMap::new(),
            active: true,
            created_at: now,
            updated_at: now,
        }
    }

    /// Creates a new user builder.
    #[must_use]
    pub fn builder(username: impl Into<String>) -> UserBuilder {
        UserBuilder::new(username)
    }

    /// Returns `true` if the user account is active.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Returns `true` if the user has a specific role.
    #[must_use]
    pub fn has_role(&self, role: &str) -> bool {
        self.roles.iter().any(|r| r == role)
    }

    /// Returns `true` if the user has any of the specified roles.
    #[must_use]
    pub fn has_any_role(&self, roles: &[&str]) -> bool {
        roles.iter().any(|role| self.has_role(role))
    }

    /// Gets an attribute value by key.
    #[must_use]
    pub fn get_attribute(&self, key: &str) -> Option<&serde_json::Value> {
        self.attributes.get(key)
    }
}

// =============================================================================
// User Builder
// =============================================================================

/// Builder for creating `User` instances.
pub struct UserBuilder {
    user: User,
}

impl UserBuilder {
    fn new(username: impl Into<String>) -> Self {
        Self {
            user: User::new(username),
        }
    }

    /// Sets the user ID.
    #[must_use]
    pub fn id(mut self, id: Uuid) -> Self {
        self.user.id = id;
        self
    }

    /// Sets the email address.
    #[must_use]
    pub fn email(mut self, email: impl Into<String>) -> Self {
        self.user.email = Some(email.into());
        self
    }

    /// Sets the password hash.
    #[must_use]
    pub fn password_hash(mut self, hash: impl Into<String>) -> Self {
        self.user.password_hash = Some(hash.into());
        self
    }

    /// Sets the FHIR user reference.
    #[must_use]
    pub fn fhir_user(mut self, fhir_user: impl Into<String>) -> Self {
        self.user.fhir_user = Some(fhir_user.into());
        self
    }

    /// Sets the user roles.
    #[must_use]
    pub fn roles(mut self, roles: Vec<String>) -> Self {
        self.user.roles = roles;
        self
    }

    /// Adds a role to the user.
    #[must_use]
    pub fn add_role(mut self, role: impl Into<String>) -> Self {
        self.user.roles.push(role.into());
        self
    }

    /// Sets the user attributes.
    #[must_use]
    pub fn attributes(mut self, attributes: HashMap<String, serde_json::Value>) -> Self {
        self.user.attributes = attributes;
        self
    }

    /// Adds an attribute to the user.
    #[must_use]
    pub fn add_attribute(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        self.user.attributes.insert(key.into(), value);
        self
    }

    /// Sets whether the user is active.
    #[must_use]
    pub fn active(mut self, active: bool) -> Self {
        self.user.active = active;
        self
    }

    /// Builds the user.
    #[must_use]
    pub fn build(self) -> User {
        self.user
    }
}

// =============================================================================
// User Storage Trait
// =============================================================================

/// Storage operations for users.
///
/// This trait defines the interface for persisting and retrieving users.
/// Implementations handle the actual database operations.
///
/// # Example
///
/// ```ignore
/// use octofhir_auth::storage::UserStorage;
///
/// async fn example(storage: &impl UserStorage) {
///     // Find a user by ID
///     if let Some(user) = storage.find_by_id(user_id).await? {
///         println!("Found user: {}", user.username);
///     }
/// }
/// ```
#[async_trait]
pub trait UserStorage: Send + Sync {
    /// Find a user by their unique ID.
    ///
    /// Returns `None` if the user doesn't exist.
    ///
    /// # Errors
    ///
    /// Returns an error if the storage operation fails.
    async fn find_by_id(&self, user_id: Uuid) -> AuthResult<Option<User>>;

    /// Find a user by their username.
    ///
    /// Returns `None` if the user doesn't exist.
    ///
    /// # Errors
    ///
    /// Returns an error if the storage operation fails.
    async fn find_by_username(&self, username: &str) -> AuthResult<Option<User>>;

    /// Find a user by their email address.
    ///
    /// Returns `None` if the user doesn't exist or has no email.
    ///
    /// # Errors
    ///
    /// Returns an error if the storage operation fails.
    async fn find_by_email(&self, email: &str) -> AuthResult<Option<User>>;

    /// Find a user by external identity provider link.
    ///
    /// Searches for a user that has a linked identity from the specified
    /// provider with the given external subject identifier.
    ///
    /// Returns `None` if no user is found with the matching external identity.
    ///
    /// # Arguments
    ///
    /// * `provider_id` - The identity provider ID (e.g., "google", "azure-ad")
    /// * `external_subject` - The subject identifier from the IdP
    ///
    /// # Errors
    ///
    /// Returns an error if the storage operation fails.
    async fn find_by_external_identity(
        &self,
        provider_id: &str,
        external_subject: &str,
    ) -> AuthResult<Option<User>>;

    /// Create a new user.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - A user with the same username already exists
    /// - A user with the same email already exists (if email is set)
    /// - The storage operation fails
    async fn create(&self, user: &User) -> AuthResult<()>;

    /// Update an existing user.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The user doesn't exist
    /// - The storage operation fails
    async fn update(&self, user: &User) -> AuthResult<()>;

    /// Delete a user.
    ///
    /// Implementations should perform a soft delete to preserve audit trail.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The user doesn't exist
    /// - The storage operation fails
    async fn delete(&self, user_id: Uuid) -> AuthResult<()>;

    /// Verify a user's password.
    ///
    /// Compares the provided password against the stored BCrypt hash.
    ///
    /// # Arguments
    ///
    /// * `user_id` - The user ID
    /// * `password` - The plaintext password to verify
    ///
    /// # Returns
    ///
    /// - `Ok(true)` if the password matches
    /// - `Ok(false)` if the password doesn't match or user has no password
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The user doesn't exist
    /// - The storage operation fails
    async fn verify_password(&self, user_id: Uuid, password: &str) -> AuthResult<bool>;

    /// List all users with pagination.
    ///
    /// # Arguments
    ///
    /// * `limit` - Maximum number of users to return
    /// * `offset` - Number of users to skip for pagination
    ///
    /// # Errors
    ///
    /// Returns an error if the storage operation fails.
    async fn list(&self, limit: i64, offset: i64) -> AuthResult<Vec<User>>;
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_user_new() {
        let user = User::new("testuser");
        assert_eq!(user.username, "testuser");
        assert!(user.active);
        assert!(user.roles.is_empty());
        assert!(user.email.is_none());
        assert!(user.password_hash.is_none());
        assert!(user.fhir_user.is_none());
    }

    #[test]
    fn test_user_builder() {
        let user = User::builder("testuser")
            .email("test@example.com")
            .fhir_user("Practitioner/123")
            .add_role("admin")
            .add_role("practitioner")
            .add_attribute("department", serde_json::json!("cardiology"))
            .active(true)
            .build();

        assert_eq!(user.username, "testuser");
        assert_eq!(user.email, Some("test@example.com".to_string()));
        assert_eq!(user.fhir_user, Some("Practitioner/123".to_string()));
        assert_eq!(user.roles, vec!["admin", "practitioner"]);
        assert!(user.is_active());
        assert!(user.has_role("admin"));
        assert!(user.has_role("practitioner"));
        assert!(!user.has_role("patient"));
    }

    #[test]
    fn test_user_has_any_role() {
        let user = User::builder("testuser").add_role("practitioner").build();

        assert!(user.has_any_role(&["admin", "practitioner"]));
        assert!(!user.has_any_role(&["admin", "patient"]));
    }

    #[test]
    fn test_user_get_attribute() {
        let user = User::builder("testuser")
            .add_attribute("department", serde_json::json!("cardiology"))
            .add_attribute("level", serde_json::json!(5))
            .build();

        assert_eq!(
            user.get_attribute("department"),
            Some(&serde_json::json!("cardiology"))
        );
        assert_eq!(user.get_attribute("level"), Some(&serde_json::json!(5)));
        assert_eq!(user.get_attribute("nonexistent"), None);
    }

    #[test]
    fn test_user_serialization() {
        let user = User::builder("testuser")
            .email("test@example.com")
            .fhir_user("Practitioner/123")
            .password_hash("$2b$12$...")
            .build();

        let json = serde_json::to_string(&user).unwrap();
        assert!(json.contains("testuser"));
        assert!(json.contains("test@example.com"));
        assert!(json.contains("Practitioner/123"));
        // password_hash is serialized for storage (filter it out when exposing via API)
        assert!(json.contains("password_hash"));
    }
}
