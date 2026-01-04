//! Auth Operations Provider

use octofhir_core::{OperationDefinition, OperationProvider, categories, modules};

/// Provider for authentication/authorization operations
pub struct AuthOperationProvider;

impl OperationProvider for AuthOperationProvider {
    fn get_operations(&self) -> Vec<OperationDefinition> {
        vec![
            // OAuth2/OIDC public endpoints
            OperationDefinition::new(
                "auth.authorize",
                "Authorization Endpoint",
                categories::AUTH,
                vec!["GET".to_string(), "POST".to_string()],
                "/auth/authorize",
                modules::AUTH,
            )
            .with_description("OAuth 2.0 authorization endpoint")
            .with_public(true),
            OperationDefinition::new(
                "auth.token",
                "Token Endpoint",
                categories::AUTH,
                vec!["POST".to_string()],
                "/auth/token",
                modules::AUTH,
            )
            .with_description("OAuth 2.0 token endpoint")
            .with_public(true),
            OperationDefinition::new(
                "auth.jwks",
                "JWKS Endpoint",
                categories::AUTH,
                vec!["GET".to_string()],
                "/auth/jwks",
                modules::AUTH,
            )
            .with_description("JSON Web Key Set for token verification")
            .with_public(true),
            OperationDefinition::new(
                "auth.userinfo",
                "UserInfo Endpoint",
                categories::AUTH,
                vec!["GET".to_string(), "POST".to_string()],
                "/auth/userinfo",
                modules::AUTH,
            )
            .with_description("OpenID Connect userinfo endpoint"),
            OperationDefinition::new(
                "auth.revoke",
                "Token Revocation",
                categories::AUTH,
                vec!["POST".to_string()],
                "/auth/revoke",
                modules::AUTH,
            )
            .with_description("OAuth 2.0 token revocation endpoint")
            .with_public(true),
            OperationDefinition::new(
                "auth.introspect",
                "Token Introspection",
                categories::AUTH,
                vec!["POST".to_string()],
                "/auth/introspect",
                modules::AUTH,
            )
            .with_description("OAuth 2.0 token introspection endpoint"),
            // SMART launch endpoints
            OperationDefinition::new(
                "auth.launch",
                "SMART Launch",
                categories::AUTH,
                vec!["GET".to_string()],
                "/auth/launch",
                modules::AUTH,
            )
            .with_description("SMART on FHIR launch endpoint"),
            // Login UI (public)
            OperationDefinition::new(
                "auth.login",
                "Login Page",
                categories::AUTH,
                vec!["GET".to_string(), "POST".to_string()],
                "/auth/login",
                modules::AUTH,
            )
            .with_description("User login page")
            .with_public(true),
            OperationDefinition::new(
                "auth.consent",
                "Consent Page",
                categories::AUTH,
                vec!["GET".to_string(), "POST".to_string()],
                "/auth/consent",
                modules::AUTH,
            )
            .with_description("OAuth consent page"),
            // User management operations
            OperationDefinition::new(
                "user.reset-password",
                "$reset-password",
                categories::AUTH,
                vec!["POST".to_string()],
                "/User/{id}/$reset-password",
                modules::AUTH,
            )
            .with_description("Reset user password (admin operation)"),
        ]
    }

    fn module_id(&self) -> &str {
        modules::AUTH
    }
}
