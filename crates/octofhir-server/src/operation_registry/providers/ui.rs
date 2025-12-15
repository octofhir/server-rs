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
        ]
    }

    fn module_id(&self) -> &str {
        modules::SERVER
    }
}
