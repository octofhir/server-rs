//! List/search resolver for FHIR resources.
//!
//! Implements resolvers for queries like `PatientList(name: "John", _count: 10)`
//! that search for FHIR resources using search parameters.

use async_graphql::dynamic::{FieldFuture, ResolverContext};
use async_graphql::Value;
use octofhir_auth::smart::scopes::FhirOperation;
use octofhir_storage::{SearchParams, TotalMode};
use tracing::{debug, warn};

use super::{evaluate_access, get_graphql_context, json_to_graphql_value};

/// Resolver for list/search operations.
pub struct SearchResolver;

impl SearchResolver {
    /// Creates a resolver function for searching resources.
    ///
    /// This is used to create the `PatientList(...)` style query fields.
    pub fn resolve(resource_type: String) -> impl Fn(ResolverContext<'_>) -> FieldFuture<'_> + Send + Sync + Clone {
        move |ctx| {
            let resource_type = resource_type.clone();
            FieldFuture::new(async move {
                debug!(
                    resource_type = %resource_type,
                    "Resolving list/search query"
                );

                // Get the GraphQL context
                let gql_ctx = get_graphql_context(&ctx)?;

                // Evaluate access control
                evaluate_access(gql_ctx, FhirOperation::Search, &resource_type, None).await?;

                // Build search parameters from GraphQL arguments
                let search_params = build_search_params(&ctx);

                debug!(
                    resource_type = %resource_type,
                    params = ?search_params,
                    "Executing search"
                );

                // Execute search using the FhirStorage trait
                let result = gql_ctx
                    .storage
                    .search(&resource_type, &search_params)
                    .await
                    .map_err(|e| {
                        warn!(error = %e, "Storage error during search");
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
                    "Search completed"
                );

                Ok(Some(Value::List(entries)))
            })
        }
    }
}

/// Builds search parameters from GraphQL arguments.
fn build_search_params(ctx: &ResolverContext<'_>) -> SearchParams {
    let mut params = SearchParams::new();

    for (key, value) in ctx.args.iter() {
        // Handle pagination/control arguments separately
        match key.as_str() {
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
                    // Parse sort string (e.g., "-date,name" -> sort by date desc, name asc)
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
            "_total" => {
                if let Ok(s) = value.string() {
                    match s {
                        "accurate" => params = params.with_total(TotalMode::Accurate),
                        "estimate" => params = params.with_total(TotalMode::Estimate),
                        "none" => params = params.with_total(TotalMode::None),
                        _ => {}
                    }
                }
            }
            // Skip _reference parameter - it's metadata indicating which reference
            // search parameter to use, not a search parameter itself
            "_reference" => continue,
            _ => {
                // Convert underscore back to hyphen for FHIR param names
                // GraphQL doesn't allow hyphens in field names
                let fhir_key = if key.starts_with('_') {
                    // Keep as-is for FHIR special params
                    key.to_string()
                } else {
                    // Convert middle underscores to hyphens (e.g., birth_date -> birth-date)
                    key.replace('_', "-")
                };

                if let Ok(s) = value.string() {
                    params = params.with_param(&fhir_key, s);
                } else if let Ok(n) = value.i64() {
                    params = params.with_param(&fhir_key, n.to_string());
                } else if let Ok(b) = value.boolean() {
                    params = params.with_param(&fhir_key, b.to_string());
                } else if let Ok(arr) = value.list() {
                    // Multiple values for same parameter = OR logic
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
    #[test]
    fn test_search_resolver_created() {
        // This test verifies the resolver can be created
        use super::SearchResolver;
        let _resolver = SearchResolver::resolve("Patient".to_string());
    }
}
