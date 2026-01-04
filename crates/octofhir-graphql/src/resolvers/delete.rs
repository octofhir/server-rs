//! Delete mutation resolver.
//!
//! Handles `{Resource}Delete` mutations for deleting FHIR resources.

use async_graphql::Value;
use async_graphql::dynamic::{FieldFuture, ResolverContext};
use octofhir_auth::smart::scopes::FhirOperation;
use tracing::{debug, trace, warn};

use super::create::storage_error_to_graphql;
use super::{evaluate_access, get_graphql_context};

/// Resolver for resource delete mutations.
///
/// Handles mutations like:
/// ```graphql
/// mutation {
///   PatientDelete(id: "123") {
///     resourceType
///     issue { severity code diagnostics }
///   }
/// }
/// ```
///
/// Returns an OperationOutcome indicating success or failure.
pub struct DeleteResolver;

impl DeleteResolver {
    /// Creates a resolver for resource deletion.
    ///
    /// # Arguments
    /// * `resource_type` - The FHIR resource type (e.g., "Patient")
    pub fn resolve(
        resource_type: String,
    ) -> impl Fn(ResolverContext<'_>) -> FieldFuture<'_> + Send + Sync + Clone {
        move |ctx| {
            let resource_type = resource_type.clone();

            FieldFuture::new(async move {
                debug!(resource_type = %resource_type, "Processing delete mutation");

                // Get the GraphQL context
                let gql_ctx = get_graphql_context(&ctx)?;

                // Extract the id argument
                let id = ctx
                    .args
                    .get("id")
                    .and_then(|v| v.string().ok())
                    .ok_or_else(|| async_graphql::Error::new("Missing required argument 'id'"))?;

                // Evaluate access control
                evaluate_access(gql_ctx, FhirOperation::Delete, &resource_type, Some(id)).await?;

                trace!(
                    resource_type = %resource_type,
                    id = %id,
                    "Deleting resource via storage"
                );

                // Delete via storage
                gql_ctx
                    .storage
                    .delete(&resource_type, id)
                    .await
                    .map_err(|e| {
                        warn!(error = %e, resource_type = %resource_type, id = %id, "Delete failed");
                        storage_error_to_graphql(e)
                    })?;

                debug!(
                    resource_type = %resource_type,
                    id = %id,
                    "Resource deleted successfully"
                );

                // Return OperationOutcome indicating success
                let outcome = create_success_outcome(&resource_type, id);
                Ok(Some(outcome))
            })
        }
    }
}

/// Creates a success OperationOutcome for delete operations.
fn create_success_outcome(resource_type: &str, id: &str) -> Value {
    let mut map = async_graphql::indexmap::IndexMap::new();

    map.insert(
        async_graphql::Name::new("resourceType"),
        Value::String("OperationOutcome".to_string()),
    );

    map.insert(
        async_graphql::Name::new("issue"),
        Value::List(vec![Value::Object({
            let mut issue = async_graphql::indexmap::IndexMap::new();
            issue.insert(
                async_graphql::Name::new("severity"),
                Value::String("information".to_string()),
            );
            issue.insert(
                async_graphql::Name::new("code"),
                Value::String("informational".to_string()),
            );
            issue.insert(
                async_graphql::Name::new("diagnostics"),
                Value::String(format!(
                    "Successfully deleted {} with id {}",
                    resource_type, id
                )),
            );
            issue
        })]),
    );

    Value::Object(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_delete_resolver_created() {
        let _resolver = DeleteResolver::resolve("Patient".to_string());
    }

    #[test]
    fn test_create_success_outcome() {
        let outcome = create_success_outcome("Patient", "123");
        if let Value::Object(map) = outcome {
            assert!(map.contains_key(&async_graphql::Name::new("resourceType")));
            assert!(map.contains_key(&async_graphql::Name::new("issue")));
        } else {
            panic!("Expected Object value");
        }
    }
}
