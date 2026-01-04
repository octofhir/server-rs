//! Role storage trait.
//!
//! Defines the interface for role persistence operations.
//! Implementations are provided by storage backends (e.g., PostgreSQL).

use std::collections::HashSet;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::AuthResult;

// =============================================================================
// Permission
// =============================================================================

/// A permission that can be assigned to roles.
///
/// Permissions control access to specific actions on resources.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Permission {
    /// Unique identifier for the permission.
    pub id: String,

    /// Display name for the permission.
    pub name: String,

    /// Description of what the permission allows.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Category for grouping permissions in the UI.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
}

impl Permission {
    /// Create a new permission with the given ID and name.
    #[must_use]
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            description: None,
            category: None,
        }
    }

    /// Set the description.
    #[must_use]
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Set the category.
    #[must_use]
    pub fn with_category(mut self, category: impl Into<String>) -> Self {
        self.category = Some(category.into());
        self
    }
}

// =============================================================================
// Role Type
// =============================================================================

/// A role in the authorization system.
///
/// Roles group permissions together and can be assigned to users.
/// Users inherit all permissions from their assigned roles.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Role {
    /// Unique identifier for the role.
    pub id: Uuid,

    /// Role name (e.g., "admin", "practitioner", "patient").
    pub name: String,

    /// Human-readable description of the role.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Permissions assigned to this role.
    #[serde(default)]
    pub permissions: HashSet<String>,

    /// Whether this is a system role that cannot be deleted.
    ///
    /// System roles include built-in roles like "admin" and "superuser".
    #[serde(default)]
    pub is_system: bool,

    /// Whether the role is active.
    ///
    /// Inactive roles do not grant any permissions.
    pub active: bool,

    /// When the role was created.
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,

    /// When the role was last updated.
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

impl Role {
    /// Creates a new role with the given name.
    ///
    /// The role is active by default with no permissions.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        let now = OffsetDateTime::now_utc();
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            description: None,
            permissions: HashSet::new(),
            is_system: false,
            active: true,
            created_at: now,
            updated_at: now,
        }
    }

    /// Creates a new role builder.
    #[must_use]
    pub fn builder(name: impl Into<String>) -> RoleBuilder {
        RoleBuilder::new(name)
    }

    /// Returns `true` if the role is active.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Returns `true` if the role has a specific permission.
    #[must_use]
    pub fn has_permission(&self, permission: &str) -> bool {
        self.permissions.contains(permission)
    }

    /// Returns `true` if the role has any of the specified permissions.
    #[must_use]
    pub fn has_any_permission(&self, permissions: &[&str]) -> bool {
        permissions.iter().any(|p| self.has_permission(p))
    }

    /// Returns `true` if the role has all of the specified permissions.
    #[must_use]
    pub fn has_all_permissions(&self, permissions: &[&str]) -> bool {
        permissions.iter().all(|p| self.has_permission(p))
    }
}

// =============================================================================
// Role Builder
// =============================================================================

/// Builder for creating `Role` instances.
pub struct RoleBuilder {
    role: Role,
}

impl RoleBuilder {
    fn new(name: impl Into<String>) -> Self {
        Self {
            role: Role::new(name),
        }
    }

    /// Sets the role ID.
    #[must_use]
    pub fn id(mut self, id: Uuid) -> Self {
        self.role.id = id;
        self
    }

    /// Sets the description.
    #[must_use]
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.role.description = Some(description.into());
        self
    }

    /// Sets the permissions.
    #[must_use]
    pub fn permissions(mut self, permissions: HashSet<String>) -> Self {
        self.role.permissions = permissions;
        self
    }

    /// Adds a permission to the role.
    #[must_use]
    pub fn add_permission(mut self, permission: impl Into<String>) -> Self {
        self.role.permissions.insert(permission.into());
        self
    }

    /// Sets whether this is a system role.
    #[must_use]
    pub fn system(mut self, is_system: bool) -> Self {
        self.role.is_system = is_system;
        self
    }

    /// Sets whether the role is active.
    #[must_use]
    pub fn active(mut self, active: bool) -> Self {
        self.role.active = active;
        self
    }

    /// Builds the role.
    #[must_use]
    pub fn build(self) -> Role {
        self.role
    }
}

// =============================================================================
// Role Storage Trait
// =============================================================================

/// Storage operations for roles.
///
/// This trait defines the interface for persisting and retrieving roles.
/// Implementations handle the actual database operations.
///
/// # Example
///
/// ```ignore
/// use octofhir_auth::storage::RoleStorage;
///
/// async fn example(storage: &impl RoleStorage) {
///     // Find a role by ID
///     if let Some(role) = storage.find_by_id(role_id).await? {
///         println!("Found role: {}", role.name);
///     }
/// }
/// ```
#[async_trait]
pub trait RoleStorage: Send + Sync {
    /// Find a role by its unique ID.
    ///
    /// Returns `None` if the role doesn't exist.
    ///
    /// # Errors
    ///
    /// Returns an error if the storage operation fails.
    async fn find_by_id(&self, role_id: Uuid) -> AuthResult<Option<Role>>;

    /// Find a role by its name.
    ///
    /// Returns `None` if the role doesn't exist.
    ///
    /// # Errors
    ///
    /// Returns an error if the storage operation fails.
    async fn find_by_name(&self, name: &str) -> AuthResult<Option<Role>>;

    /// Create a new role.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - A role with the same name already exists
    /// - The storage operation fails
    async fn create(&self, role: &Role) -> AuthResult<()>;

    /// Update an existing role.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The role doesn't exist
    /// - Attempting to modify a system role's name
    /// - The storage operation fails
    async fn update(&self, role: &Role) -> AuthResult<()>;

    /// Delete a role.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The role doesn't exist
    /// - The role is a system role
    /// - Users are still assigned to the role
    /// - The storage operation fails
    async fn delete(&self, role_id: Uuid) -> AuthResult<()>;

    /// List all roles with pagination.
    ///
    /// # Arguments
    ///
    /// * `limit` - Maximum number of roles to return
    /// * `offset` - Number of roles to skip for pagination
    ///
    /// # Errors
    ///
    /// Returns an error if the storage operation fails.
    async fn list(&self, limit: i64, offset: i64) -> AuthResult<Vec<Role>>;

    /// Count all roles.
    ///
    /// # Errors
    ///
    /// Returns an error if the storage operation fails.
    async fn count(&self) -> AuthResult<i64>;

    /// Find all roles that have a specific permission.
    ///
    /// # Arguments
    ///
    /// * `permission` - The permission to search for
    ///
    /// # Errors
    ///
    /// Returns an error if the storage operation fails.
    async fn find_by_permission(&self, permission: &str) -> AuthResult<Vec<Role>>;

    /// Get all available permissions.
    ///
    /// Returns a list of all defined permissions that can be assigned to roles.
    ///
    /// # Errors
    ///
    /// Returns an error if the storage operation fails.
    async fn get_available_permissions(&self) -> AuthResult<Vec<Permission>>;
}

// =============================================================================
// Default Permissions
// =============================================================================

/// Returns the default set of permissions available in the system.
///
/// These permissions are used to control access to various features.
#[must_use]
pub fn default_permissions() -> Vec<Permission> {
    vec![
        // Resource permissions
        Permission::new("resource:read", "Read Resources")
            .with_description("View FHIR resources")
            .with_category("Resources"),
        Permission::new("resource:create", "Create Resources")
            .with_description("Create new FHIR resources")
            .with_category("Resources"),
        Permission::new("resource:update", "Update Resources")
            .with_description("Modify existing FHIR resources")
            .with_category("Resources"),
        Permission::new("resource:delete", "Delete Resources")
            .with_description("Delete FHIR resources")
            .with_category("Resources"),
        // User management permissions
        Permission::new("user:read", "Read Users")
            .with_description("View user accounts")
            .with_category("User Management"),
        Permission::new("user:create", "Create Users")
            .with_description("Create new user accounts")
            .with_category("User Management"),
        Permission::new("user:update", "Update Users")
            .with_description("Modify user accounts")
            .with_category("User Management"),
        Permission::new("user:delete", "Delete Users")
            .with_description("Delete user accounts")
            .with_category("User Management"),
        Permission::new("user:reset-password", "Reset Passwords")
            .with_description("Reset user passwords")
            .with_category("User Management"),
        // Role management permissions
        Permission::new("role:read", "Read Roles")
            .with_description("View roles and permissions")
            .with_category("Role Management"),
        Permission::new("role:create", "Create Roles")
            .with_description("Create new roles")
            .with_category("Role Management"),
        Permission::new("role:update", "Update Roles")
            .with_description("Modify roles and permissions")
            .with_category("Role Management"),
        Permission::new("role:delete", "Delete Roles")
            .with_description("Delete roles")
            .with_category("Role Management"),
        // Client management permissions
        Permission::new("client:read", "Read Clients")
            .with_description("View OAuth clients")
            .with_category("Client Management"),
        Permission::new("client:create", "Create Clients")
            .with_description("Create OAuth clients")
            .with_category("Client Management"),
        Permission::new("client:update", "Update Clients")
            .with_description("Modify OAuth clients")
            .with_category("Client Management"),
        Permission::new("client:delete", "Delete Clients")
            .with_description("Delete OAuth clients")
            .with_category("Client Management"),
        // System permissions
        Permission::new("system:admin", "System Administration")
            .with_description("Full administrative access")
            .with_category("System"),
        Permission::new("system:config", "Configuration Management")
            .with_description("Manage server configuration")
            .with_category("System"),
        Permission::new("system:audit", "Audit Log Access")
            .with_description("View audit logs")
            .with_category("System"),
        Permission::new("system:packages", "Package Management")
            .with_description("Manage FHIR packages")
            .with_category("System"),
    ]
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_role_new() {
        let role = Role::new("admin");
        assert_eq!(role.name, "admin");
        assert!(role.active);
        assert!(role.permissions.is_empty());
        assert!(!role.is_system);
        assert!(role.description.is_none());
    }

    #[test]
    fn test_role_builder() {
        let role = Role::builder("practitioner")
            .description("Healthcare practitioner role")
            .add_permission("resource:read")
            .add_permission("resource:create")
            .system(false)
            .active(true)
            .build();

        assert_eq!(role.name, "practitioner");
        assert_eq!(
            role.description,
            Some("Healthcare practitioner role".to_string())
        );
        assert!(role.has_permission("resource:read"));
        assert!(role.has_permission("resource:create"));
        assert!(!role.has_permission("resource:delete"));
        assert!(role.is_active());
        assert!(!role.is_system);
    }

    #[test]
    fn test_role_has_any_permission() {
        let role = Role::builder("reader")
            .add_permission("resource:read")
            .build();

        assert!(role.has_any_permission(&["resource:read", "resource:create"]));
        assert!(!role.has_any_permission(&["resource:create", "resource:delete"]));
    }

    #[test]
    fn test_role_has_all_permissions() {
        let role = Role::builder("editor")
            .add_permission("resource:read")
            .add_permission("resource:create")
            .add_permission("resource:update")
            .build();

        assert!(role.has_all_permissions(&["resource:read", "resource:create"]));
        assert!(!role.has_all_permissions(&["resource:read", "resource:delete"]));
    }

    #[test]
    fn test_permission_new() {
        let perm = Permission::new("resource:read", "Read Resources")
            .with_description("View FHIR resources")
            .with_category("Resources");

        assert_eq!(perm.id, "resource:read");
        assert_eq!(perm.name, "Read Resources");
        assert_eq!(perm.description, Some("View FHIR resources".to_string()));
        assert_eq!(perm.category, Some("Resources".to_string()));
    }

    #[test]
    fn test_default_permissions() {
        let permissions = default_permissions();
        assert!(!permissions.is_empty());

        // Check some expected permissions exist
        let perm_ids: Vec<_> = permissions.iter().map(|p| p.id.as_str()).collect();
        assert!(perm_ids.contains(&"resource:read"));
        assert!(perm_ids.contains(&"user:create"));
        assert!(perm_ids.contains(&"system:admin"));
    }
}
