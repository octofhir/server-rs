//! FHIR Reference type for GraphQL.
//!
//! This module provides the GraphQL implementation of the FHIR Reference type
//! with lazy resource resolution via DataLoaders.

use async_graphql::dynamic::{Field, FieldFuture, FieldValue, Object, TypeRef, Union};
use async_graphql::Value;
use tracing::trace;

use crate::context::GraphQLContext;
use crate::loaders::ReferenceKey;

/// Creates the Reference GraphQL type.
///
/// The Reference type includes:
/// - `reference`: The reference string (e.g., "Patient/123")
/// - `type`: The resource type hint
/// - `display`: Human-readable display text
/// - `identifier`: Associated identifier (if any)
/// - `resource`: Lazy-loaded resolved resource
pub fn create_reference_type(resource_types: &[String]) -> Object {
    let mut reference = Object::new("Reference")
        .description("A reference from one resource to another");

    // Standard Reference fields
    reference = reference.field(
        Field::new("reference", TypeRef::named(TypeRef::STRING), |ctx| {
            FieldFuture::new(async move {
                if let Some(parent) = ctx.parent_value.as_value()
                    && let Value::Object(obj) = parent
                    && let Some(value) = obj.get(&async_graphql::Name::new("reference"))
                {
                    return Ok(Some(value.clone()));
                }
                Ok(None)
            })
        })
        .description("Literal reference, Relative, internal or absolute URL"),
    );

    reference = reference.field(
        Field::new("type", TypeRef::named(TypeRef::STRING), |ctx| {
            FieldFuture::new(async move {
                if let Some(parent) = ctx.parent_value.as_value()
                    && let Value::Object(obj) = parent
                    && let Some(value) = obj.get(&async_graphql::Name::new("type"))
                {
                    return Ok(Some(value.clone()));
                }
                Ok(None)
            })
        })
        .description("Type the reference refers to (e.g. 'Patient')"),
    );

    reference = reference.field(
        Field::new("display", TypeRef::named(TypeRef::STRING), |ctx| {
            FieldFuture::new(async move {
                if let Some(parent) = ctx.parent_value.as_value()
                    && let Value::Object(obj) = parent
                    && let Some(value) = obj.get(&async_graphql::Name::new("display"))
                {
                    return Ok(Some(value.clone()));
                }
                Ok(None)
            })
        })
        .description("Text alternative for the resource"),
    );

    // Identifier field (references Identifier type)
    reference = reference.field(
        Field::new("identifier", TypeRef::named("Identifier"), |ctx| {
            FieldFuture::new(async move {
                if let Some(parent) = ctx.parent_value.as_value()
                    && let Value::Object(obj) = parent
                    && let Some(value) = obj.get(&async_graphql::Name::new("identifier"))
                {
                    return Ok(Some(value.clone()));
                }
                Ok(None)
            })
        })
        .description("Logical reference, when literal reference is not known"),
    );

    // Resource resolution field with optional and type parameters
    // Only add if we have resource types defined
    if !resource_types.is_empty() {
        reference = reference.field(create_resource_resolution_field());
    }

    reference
}

/// Creates the `resource` field for lazy reference resolution.
fn create_resource_resolution_field() -> Field {
    Field::new("resource", TypeRef::named("AllResources"), move |ctx| {
        FieldFuture::new(async move {
            // Get arguments
            let optional = ctx
                .args
                .get("optional")
                .and_then(|v| v.boolean().ok())
                .unwrap_or(false);
            let type_filter = ctx
                .args
                .get("type")
                .and_then(|v| v.string().ok())
                .map(|s| s.to_string());

            // Get the reference string from parent
            let reference_str = if let Some(parent) = ctx.parent_value.as_value()
                && let Value::Object(obj) = parent
                && let Some(Value::String(ref_val)) = obj.get(&async_graphql::Name::new("reference"))
            {
                ref_val.clone()
            } else {
                return Ok(None);
            };

            trace!(reference = %reference_str, ?type_filter, "Resolving reference to resource");

            // Get context
            let gql_ctx = ctx.ctx.data::<GraphQLContext>().map_err(|e| {
                async_graphql::Error::new(format!("Failed to get GraphQL context: {e:?}"))
            })?;

            // Load via DataLoader
            let key = ReferenceKey::new(&reference_str);
            let result = gql_ctx
                .loaders
                .reference_loader
                .load_one(key)
                .await
                .map_err(|e| async_graphql::Error::new(format!("Reference resolution failed: {e}")))?;

            match result {
                Some(resolved) if resolved.resource.is_some() => {
                    let resource_type = &resolved.parsed.resource_type;

                    // Apply type filter if specified
                    if let Some(ref filter) = type_filter {
                        if resource_type != filter {
                            trace!(
                                expected = %filter,
                                actual = %resource_type,
                                "Reference type mismatch, returning null"
                            );
                            return Ok(None);
                        }
                    }

                    // Convert serde_json::Value to async_graphql::Value
                    let resource = resolved.resource.unwrap();
                    let graphql_value = json_to_graphql_value(&resource);
                    Ok(Some(FieldValue::value(graphql_value).with_type(resource_type.clone())))
                }
                _ => {
                    // Resource not found
                    if optional {
                        Ok(None)
                    } else {
                        // Return null for now, could return error in strict mode
                        trace!(reference = %reference_str, "Referenced resource not found");
                        Ok(None)
                    }
                }
            }
        })
    })
    .description("The actual resource, if it exists and is accessible")
    .argument(
        async_graphql::dynamic::InputValue::new("optional", TypeRef::named(TypeRef::BOOLEAN))
            .description("If true, return null instead of error for missing resources")
            .default_value(Value::Boolean(false)),
    )
    .argument(
        async_graphql::dynamic::InputValue::new("type", TypeRef::named(TypeRef::STRING))
            .description("Filter to only return resource if it matches this type"),
    )
}

/// Creates the AllResources union type for polymorphic reference resolution.
///
/// This union includes all FHIR resource types, allowing references to resolve
/// to any resource type with proper type discrimination.
pub fn create_all_resources_union(resource_types: &[String]) -> Union {
    let mut union = Union::new("AllResources")
        .description("Union of all FHIR resource types for polymorphic reference resolution");

    for resource_type in resource_types {
        union = union.possible_type(resource_type);
    }

    union
}

/// Converts a serde_json::Value to async_graphql::Value.
fn json_to_graphql_value(json: &serde_json::Value) -> Value {
    match json {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(b) => Value::Boolean(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::Number(i.into())
            } else if let Some(f) = n.as_f64() {
                Value::Number(
                    async_graphql::Number::from_f64(f).unwrap_or(async_graphql::Number::from(0)),
                )
            } else {
                Value::Null
            }
        }
        serde_json::Value::String(s) => Value::String(s.clone()),
        serde_json::Value::Array(arr) => {
            Value::List(arr.iter().map(json_to_graphql_value).collect())
        }
        serde_json::Value::Object(obj) => {
            let map: async_graphql::indexmap::IndexMap<async_graphql::Name, Value> = obj
                .iter()
                .map(|(k, v)| (async_graphql::Name::new(k), json_to_graphql_value(v)))
                .collect();
            Value::Object(map)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_json_to_graphql_null() {
        let json = serde_json::Value::Null;
        let result = json_to_graphql_value(&json);
        assert!(matches!(result, Value::Null));
    }

    #[test]
    fn test_json_to_graphql_bool() {
        let json = serde_json::json!(true);
        let result = json_to_graphql_value(&json);
        assert!(matches!(result, Value::Boolean(true)));
    }

    #[test]
    fn test_json_to_graphql_string() {
        let json = serde_json::json!("test");
        let result = json_to_graphql_value(&json);
        assert!(matches!(result, Value::String(s) if s == "test"));
    }

    #[test]
    fn test_json_to_graphql_number() {
        let json = serde_json::json!(42);
        let result = json_to_graphql_value(&json);
        if let Value::Number(n) = result {
            assert_eq!(n.as_i64(), Some(42));
        } else {
            panic!("Expected number");
        }
    }

    #[test]
    fn test_json_to_graphql_array() {
        let json = serde_json::json!([1, 2, 3]);
        let result = json_to_graphql_value(&json);
        if let Value::List(arr) = result {
            assert_eq!(arr.len(), 3);
        } else {
            panic!("Expected array");
        }
    }

    #[test]
    fn test_json_to_graphql_object() {
        let json = serde_json::json!({"key": "value"});
        let result = json_to_graphql_value(&json);
        if let Value::Object(obj) = result {
            assert!(obj.contains_key("key"));
        } else {
            panic!("Expected object");
        }
    }

    #[test]
    fn test_create_all_resources_union() {
        let types = vec!["Patient".to_string(), "Observation".to_string()];
        let union = create_all_resources_union(&types);
        assert_eq!(union.type_name(), "AllResources");
    }

    #[test]
    fn test_create_reference_type() {
        let types = vec!["Patient".to_string()];
        let reference = create_reference_type(&types);
        assert_eq!(reference.type_name(), "Reference");
    }
}
