//! Update mutation resolver.
//!
//! Handles `{Resource}Update` mutations for updating existing FHIR resources.

use async_graphql::dynamic::{FieldFuture, ResolverContext};
use octofhir_auth::smart::scopes::FhirOperation;
use tracing::{debug, trace, warn};

use super::create::{extract_resource_from_input, storage_error_to_graphql};
use super::{evaluate_access_with_resource, get_graphql_context, json_to_graphql_value};

/// Resolver for resource update mutations.
///
/// Handles mutations like:
/// ```graphql
/// mutation {
///   PatientUpdate(id: "123", res: {resource: {...}}, ifMatch: "W/\"1\"") {
///     id
///     meta { versionId lastUpdated }
///   }
/// }
/// ```
pub struct UpdateResolver;

impl UpdateResolver {
    /// Creates a resolver for resource updates.
    ///
    /// # Arguments
    /// * `resource_type` - The FHIR resource type (e.g., "Patient")
    pub fn resolve(
        resource_type: String,
    ) -> impl Fn(ResolverContext<'_>) -> FieldFuture<'_> + Send + Sync + Clone {
        move |ctx| {
            let resource_type = resource_type.clone();

            FieldFuture::new(async move {
                debug!(resource_type = %resource_type, "Processing update mutation");

                // Get the GraphQL context
                let gql_ctx = get_graphql_context(&ctx)?;

                // Extract the id argument
                let id = ctx
                    .args
                    .get("id")
                    .and_then(|v| v.string().ok())
                    .ok_or_else(|| async_graphql::Error::new("Missing required argument 'id'"))?;

                // Extract the input argument
                let input = ctx
                    .args
                    .get("res")
                    .ok_or_else(|| async_graphql::Error::new("Missing required argument 'res'"))?;

                // Extract optional If-Match (version) for concurrency control
                let if_match = ctx
                    .args
                    .get("ifMatch")
                    .and_then(|v| v.string().ok())
                    .map(|s| s.to_string());

                // Extract the resource JSON from the input
                let mut resource_json = extract_resource_from_input(&input, &resource_type)?;

                // Ensure id is set in the resource
                if let serde_json::Value::Object(ref mut map) = resource_json {
                    map.insert("id".to_string(), serde_json::Value::String(id.to_string()));
                    // Ensure resourceType is set
                    map.insert(
                        "resourceType".to_string(),
                        serde_json::Value::String(resource_type.clone()),
                    );
                }

                // Evaluate access control with the resource being updated
                evaluate_access_with_resource(
                    gql_ctx,
                    FhirOperation::Update,
                    &resource_type,
                    Some(id),
                    Some(resource_json.clone()),
                )
                .await?;

                trace!(
                    resource_type = %resource_type,
                    id = %id,
                    if_match = ?if_match,
                    "Updating resource via storage"
                );

                // Update via storage
                let result = gql_ctx
                    .storage
                    .update(&resource_json, if_match.as_deref())
                    .await
                    .map_err(|e| {
                        warn!(error = %e, resource_type = %resource_type, id = %id, "Update failed");
                        storage_error_to_graphql(e)
                    })?;

                debug!(
                    resource_type = %resource_type,
                    id = %result.id,
                    version_id = %result.version_id,
                    "Resource updated successfully"
                );

                // Convert to GraphQL value
                let graphql_value = json_to_graphql_value(result.resource);
                Ok(Some(graphql_value))
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_update_resolver_created() {
        let _resolver = UpdateResolver::resolve("Patient".to_string());
    }
}
