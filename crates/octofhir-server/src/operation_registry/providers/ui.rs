//! UI API Operations Provider
//!
//! Only includes operations that are actually implemented in the server.

use octofhir_core::{OperationDefinition, OperationProvider, categories, modules};

/// Provider for UI-specific API operations (console, resource list, SQL, etc.)
pub struct UiOperationProvider;

impl OperationProvider for UiOperationProvider {
    fn get_operations(&self) -> Vec<OperationDefinition> {
        vec![
            // System info
            OperationDefinition::new(
                "ui.health",
                "Health Check",
                categories::UI,
                vec!["GET".to_string()],
                "/api/health",
                modules::SERVER,
            )
            .with_description("Get server health status")
            .with_public(true),
            OperationDefinition::new(
                "ui.build_info",
                "Build Info",
                categories::UI,
                vec!["GET".to_string()],
                "/api/build-info",
                modules::SERVER,
            )
            .with_description("Get server build information"),
            OperationDefinition::new(
                "ui.resource_types",
                "Resource Types",
                categories::UI,
                vec!["GET".to_string()],
                "/api/resource-types",
                modules::SERVER,
            )
            .with_description("List available FHIR resource types"),
            // SQL Console
            OperationDefinition::new(
                "ui.sql.execute",
                "Execute SQL",
                categories::UI,
                vec!["POST".to_string()],
                "/api/$sql",
                modules::SERVER,
            )
            .with_description("Execute SQL query in the DB console"),
            // PostgreSQL LSP (Language Server Protocol for SQL)
            OperationDefinition::new(
                "ui.lsp.websocket",
                "PostgreSQL LSP",
                categories::UI,
                vec!["GET".to_string()],
                "/api/pg-lsp",
                modules::SERVER,
            )
            .with_description("PostgreSQL Language Server Protocol via WebSocket"),
            // Operations registry API
            OperationDefinition::new(
                "ui.operations.list",
                "List Operations",
                categories::UI,
                vec!["GET".to_string()],
                "/api/operations",
                modules::SERVER,
            )
            .with_description("List all server operations"),
            OperationDefinition::new(
                "ui.operations.get",
                "Get Operation",
                categories::UI,
                vec!["GET".to_string()],
                "/api/operations/{id}",
                modules::SERVER,
            )
            .with_description("Get operation details"),
            OperationDefinition::new(
                "ui.operations.update",
                "Update Operation",
                categories::UI,
                vec!["PATCH".to_string()],
                "/api/operations/{id}",
                modules::SERVER,
            )
            .with_description("Update operation settings (public flag, description)"),
            OperationDefinition::new(
                "ui.rest_console.introspect",
                "REST Console Introspection",
                categories::UI,
                vec!["GET".to_string()],
                "/api/__introspect/rest-console",
                modules::SERVER,
            )
            .with_description("Retrieve metadata for the UI REST console")
            .with_public(false),
            // Admin Configuration API
            OperationDefinition::new(
                "admin.config.list",
                "List Configuration",
                categories::UI,
                vec!["GET".to_string()],
                "/admin/config",
                modules::SERVER,
            )
            .with_description("List all configuration entries"),
            OperationDefinition::new(
                "admin.config.reload",
                "Reload Configuration",
                categories::UI,
                vec!["POST".to_string()],
                "/admin/config/$reload",
                modules::SERVER,
            )
            .with_description("Reload configuration from all sources"),
            OperationDefinition::new(
                "admin.config.get_category",
                "Get Category Config",
                categories::UI,
                vec!["GET".to_string()],
                "/admin/config/{category}",
                modules::SERVER,
            )
            .with_description("Get configuration for a category"),
            OperationDefinition::new(
                "admin.config.get",
                "Get Config Value",
                categories::UI,
                vec!["GET".to_string()],
                "/admin/config/{category}/{key}",
                modules::SERVER,
            )
            .with_description("Get a specific configuration value"),
            OperationDefinition::new(
                "admin.config.set",
                "Set Config Value",
                categories::UI,
                vec!["PUT".to_string()],
                "/admin/config/{category}/{key}",
                modules::SERVER,
            )
            .with_description("Set a configuration value"),
            OperationDefinition::new(
                "admin.config.delete",
                "Delete Config Value",
                categories::UI,
                vec!["DELETE".to_string()],
                "/admin/config/{category}/{key}",
                modules::SERVER,
            )
            .with_description("Delete (reset) a configuration value"),
            // Admin Feature Flags API
            OperationDefinition::new(
                "admin.features.list",
                "List Feature Flags",
                categories::UI,
                vec!["GET".to_string()],
                "/admin/features",
                modules::SERVER,
            )
            .with_description("List all feature flags"),
            OperationDefinition::new(
                "admin.features.get",
                "Get Feature Flag",
                categories::UI,
                vec!["GET".to_string()],
                "/admin/features/{name}",
                modules::SERVER,
            )
            .with_description("Get a specific feature flag"),
            OperationDefinition::new(
                "admin.features.toggle",
                "Toggle Feature Flag",
                categories::UI,
                vec!["PUT".to_string()],
                "/admin/features/{name}",
                modules::SERVER,
            )
            .with_description("Toggle a feature flag on/off"),
            OperationDefinition::new(
                "admin.features.evaluate",
                "Evaluate Feature Flag",
                categories::UI,
                vec!["POST".to_string()],
                "/admin/features/{name}/$evaluate",
                modules::SERVER,
            )
            .with_description("Evaluate a feature flag for a context"),
        ]
    }

    fn module_id(&self) -> &str {
        modules::SERVER
    }
}
