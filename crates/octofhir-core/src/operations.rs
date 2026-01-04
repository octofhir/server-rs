//! Operation definitions and registry traits for OctoFHIR
//!
//! This module provides types and traits for defining server operations
//! that can be tracked, displayed in UI, and targeted by policies.

use serde::{Deserialize, Serialize};

/// Reference to an App that provides this operation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AppReference {
    /// App resource ID
    pub id: String,
    /// Human-readable name
    pub name: String,
}

/// Definition of a server operation
///
/// Operations represent discrete API endpoints or functionalities
/// that can be targeted by access policies and displayed in the UI.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct OperationDefinition {
    /// Unique operation ID (e.g., "fhir.read", "graphql.query", "system.metadata")
    pub id: String,

    /// Human-readable name
    pub name: String,

    /// Description of what this operation does
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Category for grouping (e.g., "fhir", "graphql", "system", "auth", "ui", "api")
    pub category: String,

    /// HTTP method(s) this operation uses (GET, POST, PUT, DELETE, PATCH)
    pub methods: Vec<String>,

    /// URL path pattern (e.g., "/{type}/{id}", "/$graphql")
    pub path_pattern: String,

    /// Whether this operation is public (no auth required)
    #[serde(default)]
    pub public: bool,

    /// Module that provides this operation (e.g., "octofhir-server", "octofhir-graphql", app ID)
    pub module: String,

    /// App that provides this operation (for gateway operations)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app: Option<AppReference>,
}

impl OperationDefinition {
    /// Create a new operation definition with required fields
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        category: impl Into<String>,
        methods: Vec<String>,
        path_pattern: impl Into<String>,
        module: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            description: None,
            category: category.into(),
            methods,
            path_pattern: path_pattern.into(),
            public: false,
            module: module.into(),
            app: None,
        }
    }

    /// Set the description
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Mark as public (no auth required)
    pub fn with_public(mut self, public: bool) -> Self {
        self.public = public;
        self
    }

    /// Set the App reference (for gateway operations)
    pub fn with_app(mut self, app: AppReference) -> Self {
        self.app = Some(app);
        self
    }
}

/// Trait for modules to register their operations
///
/// Each module (FHIR, GraphQL, Auth, UI, etc.) implements this trait
/// to expose the operations it provides. Operations are collected
/// at startup and synced to the database.
pub trait OperationProvider: Send + Sync {
    /// Get all operations provided by this module
    fn get_operations(&self) -> Vec<OperationDefinition>;

    /// Get the module identifier
    fn module_id(&self) -> &str;
}

/// Well-known operation categories
pub mod categories {
    /// FHIR REST API operations (read, create, update, delete, search, etc.)
    pub const FHIR: &str = "fhir";

    /// GraphQL API operations
    pub const GRAPHQL: &str = "graphql";

    /// System operations (metadata, health, etc.)
    pub const SYSTEM: &str = "system";

    /// Authentication and authorization operations
    pub const AUTH: &str = "auth";

    /// UI-specific API operations (console, resources list, etc.)
    pub const UI: &str = "ui";

    /// Custom API gateway operations
    pub const API: &str = "api";

    /// Notification operations
    pub const NOTIFICATIONS: &str = "notifications";
}

/// Well-known module identifiers
pub mod modules {
    /// Core FHIR server module
    pub const SERVER: &str = "octofhir-server";

    /// GraphQL module
    pub const GRAPHQL: &str = "octofhir-graphql";

    /// Auth module
    pub const AUTH: &str = "octofhir-auth";

    /// Notifications module
    pub const NOTIFICATIONS: &str = "octofhir-notifications";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_operation_definition_builder() {
        let op = OperationDefinition::new(
            "fhir.read",
            "Read Resource",
            categories::FHIR,
            vec!["GET".to_string()],
            "/{type}/{id}",
            modules::SERVER,
        )
        .with_description("Read a single FHIR resource by ID")
        .with_public(false);

        assert_eq!(op.id, "fhir.read");
        assert_eq!(op.name, "Read Resource");
        assert_eq!(op.category, "fhir");
        assert_eq!(op.methods, vec!["GET"]);
        assert_eq!(op.path_pattern, "/{type}/{id}");
        assert_eq!(op.module, "octofhir-server");
        assert_eq!(
            op.description,
            Some("Read a single FHIR resource by ID".to_string())
        );
        assert!(!op.public);
    }

    #[test]
    fn test_operation_serialization() {
        let op = OperationDefinition::new(
            "system.metadata",
            "Capability Statement",
            categories::SYSTEM,
            vec!["GET".to_string()],
            "/metadata",
            modules::SERVER,
        )
        .with_public(true);

        let json = serde_json::to_string(&op).unwrap();
        let deserialized: OperationDefinition = serde_json::from_str(&json).unwrap();
        assert_eq!(op, deserialized);
    }
}
