//! Reverse reference resolver for FHIR GraphQL.
//!
//! Implements resolvers for queries that find resources referencing a specific resource.
//! This supports the FHIR GraphQL `_reference` parameter pattern.
//!
//! Example: Find all Observations that reference a specific Patient:
//! ```graphql
//! ObservationList(_reference: "patient", patient: "Patient/123") {
//!   id
//!   code
//! }
//! ```

use async_graphql::Value;
use async_graphql::dynamic::{FieldFuture, ResolverContext};
use octofhir_auth::smart::scopes::FhirOperation;
use octofhir_storage::SearchParams;
use tracing::{debug, warn};

use super::{evaluate_access, get_graphql_context, json_to_graphql_value};

/// Resolver for reverse reference queries.
///
/// This resolver handles queries that find resources referencing another resource.
/// It works by:
/// 1. Taking a `_reference` parameter that specifies which reference search param to use
/// 2. Building a search query using that reference parameter
/// 3. Returning matching resources
pub struct ReverseReferenceResolver;

impl ReverseReferenceResolver {
    /// Creates a resolver function for reverse reference queries.
    ///
    /// The `_reference` parameter is required and specifies which reference
    /// search parameter to use for the lookup. The actual reference value
    /// is provided through the corresponding search parameter.
    ///
    /// Example usage in GraphQL:
    /// ```graphql
    /// ConditionList(_reference: "patient", patient: "Patient/123")
    /// ```
    pub fn resolve(
        resource_type: String,
    ) -> impl Fn(ResolverContext<'_>) -> FieldFuture<'_> + Send + Sync + Clone {
        move |ctx| {
            let resource_type = resource_type.clone();
            FieldFuture::new(async move {
                // Get the _reference parameter (specifies which search param to use)
                let reference_param = ctx.args.get("_reference").and_then(|v| v.string().ok());

                debug!(
                    resource_type = %resource_type,
                    reference_param = ?reference_param,
                    "Resolving reverse reference query"
                );

                // Get the GraphQL context
                let gql_ctx = get_graphql_context(&ctx)?;

                // Evaluate access control
                evaluate_access(gql_ctx, FhirOperation::Search, &resource_type, None).await?;

                // Build search parameters
                let search_params =
                    build_reverse_reference_params(&ctx, reference_param);

                debug!(
                    resource_type = %resource_type,
                    params = ?search_params,
                    "Executing reverse reference search"
                );

                // Execute search
                let result = gql_ctx
                    .storage
                    .search(&resource_type, &search_params)
                    .await
                    .map_err(|e| {
                        warn!(error = %e, "Storage error during reverse reference search");
                        async_graphql::Error::new(format!("Search failed: {}", e))
                    })?;

                // Convert results to GraphQL list
                let entries: Vec<Value> = result
                    .entries
                    .into_iter()
                    .map(|stored| json_to_graphql_value(stored.resource))
                    .collect();

                debug!(
                    resource_type = %resource_type,
                    count = entries.len(),
                    "Reverse reference search completed"
                );

                Ok(Some(Value::List(entries)))
            })
        }
    }
}

/// Builds search parameters for reverse reference queries.
///
/// When `_reference` is specified, it identifies which reference parameter
/// should be used. Other parameters can be provided for additional filtering.
fn build_reverse_reference_params(
    ctx: &ResolverContext<'_>,
    reference_param: Option<&str>,
) -> SearchParams {
    let mut params = SearchParams::new();

    for (key, value) in ctx.args.iter() {
        let key_str = key.as_str();

        // Skip _reference itself - it's metadata, not a search param
        if key_str == "_reference" {
            continue;
        }

        // Handle pagination/control arguments
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
                // Convert GraphQL name to FHIR param name
                let fhir_key = if key_str.starts_with('_') {
                    key_str.to_string()
                } else {
                    key_str.replace('_', "-")
                };

                // If _reference was specified and this matches it,
                // this is the primary reference parameter
                let is_reference_param = reference_param
                    .map(|rp| {
                        let normalized_rp = rp.replace('-', "_");
                        key_str == normalized_rp || key_str == rp
                    })
                    .unwrap_or(false);

                if is_reference_param {
                    debug!(
                        param = %fhir_key,
                        "Processing primary reference parameter"
                    );
                }

                // Add the parameter
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
    fn test_reverse_reference_resolver_created() {
        let _resolver = ReverseReferenceResolver::resolve("Observation".to_string());
    }
}
