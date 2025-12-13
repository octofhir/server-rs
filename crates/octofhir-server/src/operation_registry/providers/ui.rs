//! UI API Operations Provider

use octofhir_core::{OperationDefinition, OperationProvider, categories, modules};

/// Provider for UI-specific API operations (console, resource list, SQL, etc.)
pub struct UiOperationProvider;

impl OperationProvider for UiOperationProvider {
    fn get_operations(&self) -> Vec<OperationDefinition> {
        vec![
            // Resource management
            OperationDefinition::new(
                "ui.resources.list",
                "List Resources",
                categories::UI,
                vec!["GET".to_string()],
                "/api/resources",
                modules::SERVER,
            )
            .with_description("List resources for the UI console"),
            OperationDefinition::new(
                "ui.resources.get",
                "Get Resource",
                categories::UI,
                vec!["GET".to_string()],
                "/api/resources/{type}/{id}",
                modules::SERVER,
            )
            .with_description("Get a resource for the UI console"),
            // SQL Console
            OperationDefinition::new(
                "ui.sql.execute",
                "Execute SQL",
                categories::UI,
                vec!["POST".to_string()],
                "/api/sql/execute",
                modules::SERVER,
            )
            .with_description("Execute SQL query in the DB console"),
            OperationDefinition::new(
                "ui.sql.explain",
                "Explain SQL",
                categories::UI,
                vec!["POST".to_string()],
                "/api/sql/explain",
                modules::SERVER,
            )
            .with_description("Explain SQL query execution plan"),
            // LSP (Language Server Protocol for SQL)
            OperationDefinition::new(
                "ui.lsp.completion",
                "SQL Completion",
                categories::UI,
                vec!["POST".to_string()],
                "/api/lsp/completion",
                modules::SERVER,
            )
            .with_description("SQL autocomplete suggestions"),
            OperationDefinition::new(
                "ui.lsp.hover",
                "SQL Hover",
                categories::UI,
                vec!["POST".to_string()],
                "/api/lsp/hover",
                modules::SERVER,
            )
            .with_description("SQL hover information"),
            // Admin: Users
            OperationDefinition::new(
                "ui.admin.users.list",
                "List Users",
                categories::UI,
                vec!["GET".to_string()],
                "/api/admin/users",
                modules::SERVER,
            )
            .with_description("List all users"),
            OperationDefinition::new(
                "ui.admin.users.create",
                "Create User",
                categories::UI,
                vec!["POST".to_string()],
                "/api/admin/users",
                modules::SERVER,
            )
            .with_description("Create a new user"),
            OperationDefinition::new(
                "ui.admin.users.get",
                "Get User",
                categories::UI,
                vec!["GET".to_string()],
                "/api/admin/users/{id}",
                modules::SERVER,
            )
            .with_description("Get user details"),
            OperationDefinition::new(
                "ui.admin.users.update",
                "Update User",
                categories::UI,
                vec!["PUT".to_string(), "PATCH".to_string()],
                "/api/admin/users/{id}",
                modules::SERVER,
            )
            .with_description("Update user details"),
            OperationDefinition::new(
                "ui.admin.users.delete",
                "Delete User",
                categories::UI,
                vec!["DELETE".to_string()],
                "/api/admin/users/{id}",
                modules::SERVER,
            )
            .with_description("Delete a user"),
            // Admin: Clients
            OperationDefinition::new(
                "ui.admin.clients.list",
                "List Clients",
                categories::UI,
                vec!["GET".to_string()],
                "/api/admin/clients",
                modules::SERVER,
            )
            .with_description("List OAuth clients"),
            OperationDefinition::new(
                "ui.admin.clients.create",
                "Create Client",
                categories::UI,
                vec!["POST".to_string()],
                "/api/admin/clients",
                modules::SERVER,
            )
            .with_description("Create a new OAuth client"),
            OperationDefinition::new(
                "ui.admin.clients.get",
                "Get Client",
                categories::UI,
                vec!["GET".to_string()],
                "/api/admin/clients/{id}",
                modules::SERVER,
            )
            .with_description("Get OAuth client details"),
            OperationDefinition::new(
                "ui.admin.clients.update",
                "Update Client",
                categories::UI,
                vec!["PUT".to_string(), "PATCH".to_string()],
                "/api/admin/clients/{id}",
                modules::SERVER,
            )
            .with_description("Update OAuth client"),
            OperationDefinition::new(
                "ui.admin.clients.delete",
                "Delete Client",
                categories::UI,
                vec!["DELETE".to_string()],
                "/api/admin/clients/{id}",
                modules::SERVER,
            )
            .with_description("Delete OAuth client"),
            // Admin: Policies
            OperationDefinition::new(
                "ui.admin.policies.list",
                "List Policies",
                categories::UI,
                vec!["GET".to_string()],
                "/api/admin/policies",
                modules::SERVER,
            )
            .with_description("List access policies"),
            OperationDefinition::new(
                "ui.admin.policies.create",
                "Create Policy",
                categories::UI,
                vec!["POST".to_string()],
                "/api/admin/policies",
                modules::SERVER,
            )
            .with_description("Create a new access policy"),
            OperationDefinition::new(
                "ui.admin.policies.get",
                "Get Policy",
                categories::UI,
                vec!["GET".to_string()],
                "/api/admin/policies/{id}",
                modules::SERVER,
            )
            .with_description("Get access policy details"),
            OperationDefinition::new(
                "ui.admin.policies.update",
                "Update Policy",
                categories::UI,
                vec!["PUT".to_string(), "PATCH".to_string()],
                "/api/admin/policies/{id}",
                modules::SERVER,
            )
            .with_description("Update access policy"),
            OperationDefinition::new(
                "ui.admin.policies.delete",
                "Delete Policy",
                categories::UI,
                vec!["DELETE".to_string()],
                "/api/admin/policies/{id}",
                modules::SERVER,
            )
            .with_description("Delete access policy"),
            // Admin: Identity Providers
            OperationDefinition::new(
                "ui.admin.idp.list",
                "List Identity Providers",
                categories::UI,
                vec!["GET".to_string()],
                "/api/admin/identity-providers",
                modules::SERVER,
            )
            .with_description("List external identity providers"),
            OperationDefinition::new(
                "ui.admin.idp.create",
                "Create Identity Provider",
                categories::UI,
                vec!["POST".to_string()],
                "/api/admin/identity-providers",
                modules::SERVER,
            )
            .with_description("Create a new identity provider"),
            OperationDefinition::new(
                "ui.admin.idp.get",
                "Get Identity Provider",
                categories::UI,
                vec!["GET".to_string()],
                "/api/admin/identity-providers/{id}",
                modules::SERVER,
            )
            .with_description("Get identity provider details"),
            OperationDefinition::new(
                "ui.admin.idp.update",
                "Update Identity Provider",
                categories::UI,
                vec!["PUT".to_string(), "PATCH".to_string()],
                "/api/admin/identity-providers/{id}",
                modules::SERVER,
            )
            .with_description("Update identity provider"),
            OperationDefinition::new(
                "ui.admin.idp.delete",
                "Delete Identity Provider",
                categories::UI,
                vec!["DELETE".to_string()],
                "/api/admin/identity-providers/{id}",
                modules::SERVER,
            )
            .with_description("Delete identity provider"),
            // Configuration
            OperationDefinition::new(
                "ui.config.get",
                "Get Configuration",
                categories::UI,
                vec!["GET".to_string()],
                "/api/config",
                modules::SERVER,
            )
            .with_description("Get server configuration"),
            OperationDefinition::new(
                "ui.config.update",
                "Update Configuration",
                categories::UI,
                vec!["PUT".to_string(), "PATCH".to_string()],
                "/api/config",
                modules::SERVER,
            )
            .with_description("Update server configuration"),
            // Operations list (for this very feature!)
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
            // API Gateway
            OperationDefinition::new(
                "ui.gateway.apps.list",
                "List Gateway Apps",
                categories::UI,
                vec!["GET".to_string()],
                "/api/gateway/apps",
                modules::SERVER,
            )
            .with_description("List API gateway applications"),
            OperationDefinition::new(
                "ui.gateway.apps.create",
                "Create Gateway App",
                categories::UI,
                vec!["POST".to_string()],
                "/api/gateway/apps",
                modules::SERVER,
            )
            .with_description("Create a new API gateway application"),
            OperationDefinition::new(
                "ui.gateway.apps.get",
                "Get Gateway App",
                categories::UI,
                vec!["GET".to_string()],
                "/api/gateway/apps/{id}",
                modules::SERVER,
            )
            .with_description("Get API gateway application details"),
            OperationDefinition::new(
                "ui.gateway.apps.update",
                "Update Gateway App",
                categories::UI,
                vec!["PUT".to_string(), "PATCH".to_string()],
                "/api/gateway/apps/{id}",
                modules::SERVER,
            )
            .with_description("Update API gateway application"),
            OperationDefinition::new(
                "ui.gateway.apps.delete",
                "Delete Gateway App",
                categories::UI,
                vec!["DELETE".to_string()],
                "/api/gateway/apps/{id}",
                modules::SERVER,
            )
            .with_description("Delete API gateway application"),
        ]
    }

    fn module_id(&self) -> &str {
        modules::SERVER
    }
}
