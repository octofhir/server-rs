//! Single resource read resolver.
//!
//! Implements resolvers for queries like `Patient(_id: "123")` that fetch
//! a single FHIR resource by its ID.

use async_graphql::dynamic::{FieldFuture, ResolverContext};
use octofhir_auth::smart::scopes::FhirOperation;
use tracing::{debug, warn};

use super::{evaluate_access, get_graphql_context, json_to_graphql_value};
use crate::error::GraphQLError;

/// Resolver for single resource read operations.
pub struct ReadResolver;

impl ReadResolver {
    /// Creates a resolver function for reading a single resource by ID.
    ///
    /// This is used to create the `Patient(_id: ID!)` style query fields.
    pub fn resolve(resource_type: String) -> impl Fn(ResolverContext<'_>) -> FieldFuture<'_> + Send + Sync + Clone {
        move |ctx| {
            let resource_type = resource_type.clone();
            FieldFuture::new(async move {
                // Extract the _id argument
                let id = ctx
                    .args
                    .get("_id")
                    .and_then(|v| v.string().ok())
                    .ok_or_else(|| {
                        async_graphql::Error::new("Missing required argument '_id'")
                    })?;

                debug!(
                    resource_type = %resource_type,
                    id = %id,
                    "Resolving single resource read"
                );

                // Get the GraphQL context with storage
                let gql_ctx = get_graphql_context(&ctx)?;

                // Evaluate access control
                evaluate_access(gql_ctx, FhirOperation::Read, &resource_type, Some(id)).await?;

                // Read from storage using FhirStorage trait
                let result = gql_ctx
                    .storage
                    .read(&resource_type, id)
                    .await
                    .map_err(|e| {
                        warn!(error = %e, "Storage error reading resource");
                        GraphQLError::from(e)
                    })?;

                match result {
                    Some(stored) => {
                        // Convert the stored resource to GraphQL value
                        let value = json_to_graphql_value(stored.resource);
                        Ok(Some(value))
                    }
                    None => {
                        // Resource not found - return null per GraphQL spec
                        // (nullable field returns null, not an error)
                        debug!(
                            resource_type = %resource_type,
                            id = %id,
                            "Resource not found"
                        );
                        Ok(None)
                    }
                }
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::json_to_graphql_value;
    use async_graphql::Value;

    #[test]
    fn test_json_to_graphql_value_primitives() {
        use serde_json::json;

        // Null
        assert!(matches!(json_to_graphql_value(json!(null)), Value::Null));

        // Boolean
        assert!(matches!(json_to_graphql_value(json!(true)), Value::Boolean(true)));

        // Number
        assert!(matches!(json_to_graphql_value(json!(42)), Value::Number(_)));

        // String
        assert!(matches!(json_to_graphql_value(json!("hello")), Value::String(s) if s == "hello"));
    }

    #[test]
    fn test_json_to_graphql_value_complex() {
        use serde_json::json;

        // Array
        let arr = json_to_graphql_value(json!([1, 2, 3]));
        assert!(matches!(arr, Value::List(_)));

        // Object
        let obj = json_to_graphql_value(json!({"name": "John"}));
        assert!(matches!(obj, Value::Object(_)));
    }
}
