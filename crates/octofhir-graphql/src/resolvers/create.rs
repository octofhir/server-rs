//! Create mutation resolver.
//!
//! Handles `{Resource}Create` mutations for creating new FHIR resources.

use async_graphql::dynamic::{FieldFuture, ResolverContext, ValueAccessor};
use async_graphql::{ErrorExtensions, Value};
use octofhir_auth::smart::scopes::FhirOperation;
use tracing::{debug, trace, warn};

use super::{evaluate_access_with_resource, get_graphql_context, json_to_graphql_value};

/// Resolver for resource creation mutations.
///
/// Handles mutations like:
/// ```graphql
/// mutation {
///   PatientCreate(res: {resource: {...}}) {
///     id
///     meta { versionId lastUpdated }
///   }
/// }
/// ```
pub struct CreateResolver;

impl CreateResolver {
    /// Creates a resolver for resource creation.
    ///
    /// # Arguments
    /// * `resource_type` - The FHIR resource type (e.g., "Patient")
    pub fn resolve(
        resource_type: String,
    ) -> impl Fn(ResolverContext<'_>) -> FieldFuture<'_> + Send + Sync + Clone {
        move |ctx| {
            let resource_type = resource_type.clone();

            FieldFuture::new(async move {
                debug!(resource_type = %resource_type, "Processing create mutation");

                // Get the GraphQL context
                let gql_ctx = get_graphql_context(&ctx)?;

                // Extract the input argument
                let input = ctx
                    .args
                    .get("res")
                    .ok_or_else(|| async_graphql::Error::new("Missing required argument 'res'"))?;

                // Extract the resource JSON from the input
                let resource_json = extract_resource_from_input(&input, &resource_type)?;

                // Evaluate access control with the resource being created
                evaluate_access_with_resource(
                    gql_ctx,
                    FhirOperation::Create,
                    &resource_type,
                    None,
                    Some(resource_json.clone()),
                )
                .await?;

                trace!(resource_type = %resource_type, "Creating resource via storage");

                // Create via storage
                let result = gql_ctx.storage.create(&resource_json).await.map_err(|e| {
                    warn!(error = %e, resource_type = %resource_type, "Create failed");
                    storage_error_to_graphql(e)
                })?;

                debug!(
                    resource_type = %resource_type,
                    id = %result.id,
                    version_id = %result.version_id,
                    "Resource created successfully"
                );

                // Convert to GraphQL value
                let graphql_value = json_to_graphql_value(result.resource);
                Ok(Some(graphql_value))
            })
        }
    }
}

/// Extracts the resource JSON from the mutation input.
///
/// The input is expected to be an InputObject with a `resource` field containing JSON.
pub(crate) fn extract_resource_from_input(
    input: &ValueAccessor<'_>,
    expected_type: &str,
) -> Result<serde_json::Value, async_graphql::Error> {
    // Get the object from input
    let obj = input
        .object()
        .map_err(|_| async_graphql::Error::new("Invalid input: expected object"))?;

    // Get the resource field
    let resource_field = obj
        .get("resource")
        .ok_or_else(|| async_graphql::Error::new("Missing required field 'resource'"))?;

    // Convert to JSON
    let resource = value_accessor_to_json(&resource_field)?;

    // Validate resource type matches
    if let Some(rt) = resource.get("resourceType").and_then(|v| v.as_str()) {
        if rt != expected_type {
            return Err(async_graphql::Error::new(format!(
                "Resource type mismatch: expected {}, got {}",
                expected_type, rt
            )));
        }
    } else {
        // Add resourceType if missing
        let mut resource = resource;
        if let serde_json::Value::Object(ref mut map) = resource {
            map.insert(
                "resourceType".to_string(),
                serde_json::Value::String(expected_type.to_string()),
            );
        }
        return Ok(resource);
    }

    Ok(resource)
}

/// Converts a ValueAccessor to serde_json::Value.
pub(crate) fn value_accessor_to_json(
    value: &ValueAccessor<'_>,
) -> Result<serde_json::Value, async_graphql::Error> {
    if value.is_null() {
        return Ok(serde_json::Value::Null);
    }

    if let Ok(b) = value.boolean() {
        return Ok(serde_json::Value::Bool(b));
    }

    if let Ok(i) = value.i64() {
        return Ok(serde_json::Value::Number(i.into()));
    }

    if let Ok(f) = value.f64() {
        return Ok(serde_json::json!(f));
    }

    if let Ok(s) = value.string() {
        return Ok(serde_json::Value::String(s.to_string()));
    }

    if let Ok(list) = value.list() {
        let items: Result<Vec<serde_json::Value>, async_graphql::Error> =
            list.iter().map(|v| value_accessor_to_json(&v)).collect();
        return Ok(serde_json::Value::Array(items?));
    }

    if let Ok(obj) = value.object() {
        let mut map = serde_json::Map::new();
        for (k, v) in obj.iter() {
            map.insert(k.to_string(), value_accessor_to_json(&v)?);
        }
        return Ok(serde_json::Value::Object(map));
    }

    // Fallback - try to deserialize
    Ok(serde_json::Value::Null)
}

/// Converts a GraphQL Value to serde_json::Value.
#[allow(dead_code)]
pub(crate) fn graphql_value_to_json(
    value: &Value,
) -> Result<serde_json::Value, async_graphql::Error> {
    match value {
        Value::Null => Ok(serde_json::Value::Null),
        Value::Boolean(b) => Ok(serde_json::Value::Bool(*b)),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(serde_json::Value::Number(i.into()))
            } else if let Some(f) = n.as_f64() {
                Ok(serde_json::json!(f))
            } else {
                Ok(serde_json::Value::Null)
            }
        }
        Value::String(s) => Ok(serde_json::Value::String(s.clone())),
        Value::List(arr) => {
            let items: Result<Vec<serde_json::Value>, async_graphql::Error> =
                arr.iter().map(graphql_value_to_json).collect();
            Ok(serde_json::Value::Array(items?))
        }
        Value::Object(obj) => {
            let mut map = serde_json::Map::new();
            for (k, v) in obj.iter() {
                map.insert(k.to_string(), graphql_value_to_json(v)?);
            }
            Ok(serde_json::Value::Object(map))
        }
        Value::Enum(e) => Ok(serde_json::Value::String(e.to_string())),
        Value::Binary(b) => {
            use base64::Engine;
            Ok(serde_json::Value::String(
                base64::engine::general_purpose::STANDARD.encode(b),
            ))
        }
    }
}

/// Converts a storage error to a GraphQL error with OperationOutcome in extensions.
pub(crate) fn storage_error_to_graphql(
    error: octofhir_storage::StorageError,
) -> async_graphql::Error {
    use octofhir_storage::ErrorCategory;

    let message = error.to_string();

    // Map error category to strings
    let (category, severity, code) = match error.category() {
        ErrorCategory::NotFound => ("not_found", "error", "not-found"),
        ErrorCategory::Conflict => ("conflict", "error", "conflict"),
        ErrorCategory::Validation => ("validation", "error", "invalid"),
        ErrorCategory::Transaction => ("transaction", "fatal", "exception"),
        ErrorCategory::Infrastructure => ("infrastructure", "fatal", "transient"),
        ErrorCategory::Internal => ("internal", "fatal", "exception"),
        ErrorCategory::Deleted => ("deleted", "error", "deleted"),
    };

    // Create error with extensions
    async_graphql::Error::new(&message).extend_with(|_, e| {
        e.set("category", category);

        // Create an OperationOutcome-like structure
        e.set(
            "operationOutcome",
            async_graphql::Value::Object({
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
                            Value::String(severity.to_string()),
                        );
                        issue.insert(
                            async_graphql::Name::new("code"),
                            Value::String(code.to_string()),
                        );
                        issue.insert(
                            async_graphql::Name::new("diagnostics"),
                            Value::String(message.clone()),
                        );
                        issue
                    })]),
                );
                map
            }),
        );
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_graphql_value_to_json_primitives() {
        assert_eq!(
            graphql_value_to_json(&Value::Null).unwrap(),
            serde_json::Value::Null
        );
        assert_eq!(
            graphql_value_to_json(&Value::Boolean(true)).unwrap(),
            serde_json::json!(true)
        );
        assert_eq!(
            graphql_value_to_json(&Value::String("test".to_string())).unwrap(),
            serde_json::json!("test")
        );
    }

    #[test]
    fn test_graphql_value_to_json_object() {
        let mut obj = async_graphql::indexmap::IndexMap::new();
        obj.insert(
            async_graphql::Name::new("key"),
            Value::String("value".to_string()),
        );
        let graphql_obj = Value::Object(obj);

        let result = graphql_value_to_json(&graphql_obj).unwrap();
        assert_eq!(result, serde_json::json!({"key": "value"}));
    }

    #[test]
    fn test_create_resolver_created() {
        let _resolver = CreateResolver::resolve("Patient".to_string());
    }
}
