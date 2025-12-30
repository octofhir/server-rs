//! System Operations Provider

use octofhir_core::{OperationDefinition, OperationProvider, categories, modules};

/// Provider for system-level operations (metadata, health, etc.)
pub struct SystemOperationProvider;

impl OperationProvider for SystemOperationProvider {
    fn get_operations(&self) -> Vec<OperationDefinition> {
        vec![
            // Root endpoint
            OperationDefinition::new(
                "system.root",
                "Root",
                categories::SYSTEM,
                vec!["GET".to_string()],
                "/",
                modules::SERVER,
            )
            .with_description("Server root endpoint")
            .with_public(true),
            // Favicon
            OperationDefinition::new(
                "system.favicon",
                "Favicon",
                categories::SYSTEM,
                vec!["GET".to_string()],
                "/favicon.ico",
                modules::SERVER,
            )
            .with_description("Browser favicon")
            .with_public(true),
            // Public operations
            OperationDefinition::new(
                "system.metadata",
                "Capability Statement",
                categories::SYSTEM,
                vec!["GET".to_string()],
                "/metadata",
                modules::SERVER,
            )
            .with_description("Get the server's capability statement")
            .with_public(true),
            OperationDefinition::new(
                "system.health",
                "Health Check",
                categories::SYSTEM,
                vec!["GET".to_string()],
                "/healthz",
                modules::SERVER,
            )
            .with_description("Check server health status")
            .with_public(true),
            OperationDefinition::new(
                "system.ready",
                "Readiness Check",
                categories::SYSTEM,
                vec!["GET".to_string()],
                "/readyz",
                modules::SERVER,
            )
            .with_description("Check server readiness")
            .with_public(true),
            OperationDefinition::new(
                "system.live",
                "Liveness Check",
                categories::SYSTEM,
                vec!["GET".to_string()],
                "/livez",
                modules::SERVER,
            )
            .with_description("Check server liveness")
            .with_public(true),
            // Well-known endpoints (public)
            OperationDefinition::new(
                "system.smart-configuration",
                "SMART Configuration",
                categories::SYSTEM,
                vec!["GET".to_string()],
                "/.well-known/smart-configuration",
                modules::SERVER,
            )
            .with_description("SMART on FHIR configuration")
            .with_public(true),
            OperationDefinition::new(
                "system.openid-configuration",
                "OpenID Configuration",
                categories::SYSTEM,
                vec!["GET".to_string()],
                "/.well-known/openid-configuration",
                modules::SERVER,
            )
            .with_description("OpenID Connect configuration")
            .with_public(true),
            // Observability endpoints
            OperationDefinition::new(
                "system.metrics",
                "Prometheus Metrics",
                categories::SYSTEM,
                vec!["GET".to_string()],
                "/metrics",
                modules::SERVER,
            )
            .with_description("Prometheus metrics for monitoring")
            .with_public(true),
        ]
    }

    fn module_id(&self) -> &str {
        modules::SERVER
    }
}
