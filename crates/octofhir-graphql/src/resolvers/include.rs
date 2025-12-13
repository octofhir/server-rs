//! Include resolvers for nested resource queries.
//!
//! Implements resolvers for forward and reverse include functionality in GraphQL.
//! These resolvers enable efficient querying of related resources within nested contexts.

use async_graphql::dynamic::{FieldFuture, ResolverContext};
use async_graphql::Value;
use octofhir_auth::smart::scopes::FhirOperation;
use octofhir_storage::SearchParams;
use tracing::{debug, trace, warn};

use super::{evaluate_access, get_graphql_context, json_to_graphql_value};

/// Resolver for reverse reference queries in nested context.
///
/// This resolver finds resources that reference the parent resource.
/// It's used when querying related resources from within a resource type.
///
/// Example query:
/// ```graphql
/// Patient(_id: "123") {
///   id
///   ObservationList_subject {
///     id
///     code { text }
///   }
/// }
/// ```
///
/// The field `ObservationList_subject` will return all Observations
/// where `subject` references the parent Patient.
pub struct NestedReverseReferenceResolver;

impl NestedReverseReferenceResolver {
    /// Creates a resolver for finding resources that reference the parent resource.
    ///
    /// # Arguments
    /// * `source_type` - The type of resources to search for (e.g., "Observation")
    /// * `reference_param` - The search parameter name for the reference (e.g., "subject")
    /// * `target_type` - The type of the parent resource (e.g., "Patient")
    pub fn resolve(
        source_type: String,
        reference_param: String,
        target_type: String,
    ) -> impl Fn(ResolverContext<'_>) -> FieldFuture<'_> + Send + Sync + Clone {
        move |ctx| {
            let source_type = source_type.clone();
            let reference_param = reference_param.clone();
            let target_type = target_type.clone();

            FieldFuture::new(async move {
                // Get the parent resource's ID from context
                let parent_id = extract_parent_id(&ctx)?;

                debug!(
                    source_type = %source_type,
                    reference_param = %reference_param,
                    target_type = %target_type,
                    parent_id = %parent_id,
                    "Resolving nested reverse reference query"
                );

                let gql_ctx = get_graphql_context(&ctx)?;

                // Evaluate access control for searching the source type
                evaluate_access(gql_ctx, FhirOperation::Search, &source_type, None).await?;

                // Build search params with reference to parent
                let reference_value = format!("{}/{}", target_type, parent_id);
                let mut search_params = SearchParams::new().with_param(&reference_param, &reference_value);

                // Apply additional search parameters from arguments
                search_params = apply_search_arguments(&ctx, search_params);

                trace!(
                    source_type = %source_type,
                    params = ?search_params,
                    "Executing nested reverse reference search"
                );

                // Execute search
                let result = gql_ctx
                    .storage
                    .search(&source_type, &search_params)
                    .await
                    .map_err(|e| {
                        warn!(error = %e, "Storage error during nested reverse reference search");
                        async_graphql::Error::new(format!("Search failed: {}", e))
                    })?;

                // Convert results to GraphQL list
                let entries: Vec<Value> = result
                    .entries
                    .into_iter()
                    .map(|stored| json_to_graphql_value(stored.resource))
                    .collect();

                debug!(
                    source_type = %source_type,
                    count = entries.len(),
                    "Nested reverse reference search completed"
                );

                Ok(Some(Value::List(entries)))
            })
        }
    }
}

/// Extracts the parent resource's ID from the resolver context.
fn extract_parent_id(ctx: &ResolverContext<'_>) -> Result<String, async_graphql::Error> {
    if let Some(parent) = ctx.parent_value.as_value()
        && let Value::Object(obj) = parent
        && let Some(Value::String(id)) = obj.get(&async_graphql::Name::new("id"))
    {
        return Ok(id.clone());
    }

    Err(async_graphql::Error::new(
        "Cannot resolve reverse reference: parent resource has no id",
    ))
}

/// Applies additional search arguments from the GraphQL context.
fn apply_search_arguments(ctx: &ResolverContext<'_>, mut params: SearchParams) -> SearchParams {
    for (key, value) in ctx.args.iter() {
        let key_str = key.as_str();

        match key_str {
            "_count" => {
                if let Ok(n) = value.i64() {
                    params = params.with_count(n as u32);
                }
            }
            "_offset" => {
                if let Ok(n) = value.i64() {
                    params = params.with_offset(n as u32);
                }
            }
            "_sort" => {
                if let Ok(s) = value.string() {
                    for sort_param in s.split(',') {
                        let sort_param = sort_param.trim();
                        if let Some(field) = sort_param.strip_prefix('-') {
                            params = params.with_sort(field, true);
                        } else {
                            params = params.with_sort(sort_param, false);
                        }
                    }
                }
            }
            _ => {
                // Convert underscore back to hyphen for FHIR param names
                let fhir_key = if key_str.starts_with('_') {
                    key_str.to_string()
                } else {
                    key_str.replace('_', "-")
                };

                if let Ok(s) = value.string() {
                    params = params.with_param(&fhir_key, s);
                } else if let Ok(n) = value.i64() {
                    params = params.with_param(&fhir_key, n.to_string());
                } else if let Ok(b) = value.boolean() {
                    params = params.with_param(&fhir_key, b.to_string());
                } else if let Ok(arr) = value.list() {
                    for v in arr.iter() {
                        if let Ok(s) = v.string() {
                            params = params.with_param(&fhir_key, s);
                        }
                    }
                }
            }
        }
    }

    params
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nested_reverse_reference_resolver_created() {
        let _resolver = NestedReverseReferenceResolver::resolve(
            "Observation".to_string(),
            "subject".to_string(),
            "Patient".to_string(),
        );
    }
}
