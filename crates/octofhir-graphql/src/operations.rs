//! GraphQL Operation Provider
//!
//! Provides operation definitions for the GraphQL module.

use octofhir_core::{OperationDefinition, OperationProvider, categories, modules};

/// Provider for GraphQL operations
pub struct GraphQLOperationProvider;

impl OperationProvider for GraphQLOperationProvider {
    fn get_operations(&self) -> Vec<OperationDefinition> {
        vec![
            OperationDefinition::new(
                "graphql.query",
                "GraphQL Query",
                categories::GRAPHQL,
                vec!["GET".to_string(), "POST".to_string()],
                "/$graphql",
                modules::GRAPHQL,
            )
            .with_description("Execute GraphQL queries against FHIR resources"),
            OperationDefinition::new(
                "graphql.instance",
                "GraphQL Instance Query",
                categories::GRAPHQL,
                vec!["GET".to_string(), "POST".to_string()],
                "/fhir/{type}/{id}/$graphql",
                modules::GRAPHQL,
            )
            .with_description("Execute GraphQL queries against a specific FHIR resource instance"),
        ]
    }

    fn module_id(&self) -> &str {
        modules::GRAPHQL
    }
}
