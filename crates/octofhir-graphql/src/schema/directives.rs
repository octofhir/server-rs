//! FHIR GraphQL directives.
//!
//! This module implements the FHIR GraphQL specification directives:
//! - `@flatten` - Field is not output; children are added directly to parent
//! - `@first` - Only take the first element from a list
//! - `@singleton` - Field collates to a single node, not a list
//! - `@slice(fhirpath: String!)` - Append FHIRPath result to field name
//!
//! These directives are handled via response transformation since async-graphql's
//! dynamic schema doesn't support custom directive definitions. The directives
//! are processed after the query execution to transform the response shape.
//!
//! See: <https://build.fhir.org/graphql.html>

use async_graphql::Value;
use serde_json::Map;
use std::collections::HashMap;

/// Directive name constants for FHIR GraphQL.
pub mod names {
    pub const FLATTEN: &str = "flatten";
    pub const FIRST: &str = "first";
    pub const SINGLETON: &str = "singleton";
    pub const SLICE: &str = "slice";
}

/// Information about directives applied to a field.
#[derive(Debug, Clone, Default)]
pub struct FieldDirectives {
    /// Whether @flatten is applied
    pub flatten: bool,
    /// Whether @first is applied
    pub first: bool,
    /// Whether @singleton is applied
    pub singleton: bool,
    /// FHIRPath expression for @slice, if present
    pub slice_fhirpath: Option<String>,
}

impl FieldDirectives {
    /// Check if any directive is applied
    pub fn has_any(&self) -> bool {
        self.flatten || self.first || self.singleton || self.slice_fhirpath.is_some()
    }
}

/// Extracts directive information from a GraphQL query field.
///
/// This parses directives from the field selection in the query.
/// In async-graphql, directives are available via the selection set.
pub fn extract_directives_from_field(
    directives: &[async_graphql_parser::types::Directive],
) -> FieldDirectives {
    let mut result = FieldDirectives::default();

    for directive in directives {
        let name = directive.name.node.as_str();
        match name {
            names::FLATTEN => result.flatten = true,
            names::FIRST => result.first = true,
            names::SINGLETON => result.singleton = true,
            names::SLICE => {
                // Extract the fhirpath argument using the helper method
                if let Some(value) = directive.get_argument("fhirpath") {
                    if let async_graphql_value::Value::String(s) = &value.node {
                        result.slice_fhirpath = Some(s.clone());
                    }
                }
            }
            _ => {} // Ignore unknown directives
        }
    }

    result
}

/// Transforms a GraphQL response value based on FHIR directives.
///
/// This is a post-processing step that applies directive transformations
/// to the response data.
pub fn transform_response_value(
    value: Value,
    directives: &FieldDirectives,
) -> Value {
    // Apply @first: take only the first element from a list
    let value = if directives.first {
        apply_first_directive(value)
    } else {
        value
    };

    // Apply @singleton: ensure value is not a list
    let value = if directives.singleton {
        apply_singleton_directive(value)
    } else {
        value
    };

    // Note: @flatten and @slice require access to the parent context
    // and are handled at a higher level in the response transformation
    value
}

/// Applies the @first directive: returns only the first element of a list.
fn apply_first_directive(value: Value) -> Value {
    match value {
        Value::List(list) => {
            list.into_iter().next().unwrap_or(Value::Null)
        }
        other => other, // Non-list values pass through unchanged
    }
}

/// Applies the @singleton directive: collapses a single-element list to its value.
fn apply_singleton_directive(value: Value) -> Value {
    match value {
        Value::List(list) if list.len() == 1 => {
            list.into_iter().next().unwrap_or(Value::Null)
        }
        Value::List(list) if list.is_empty() => Value::Null,
        other => other,
    }
}

/// Applies the @flatten directive: merges child fields into parent object.
///
/// This function takes a parent object and a field name, and if that field
/// contains an object, merges the object's fields directly into the parent.
pub fn apply_flatten_to_object(
    parent: &mut Map<String, serde_json::Value>,
    field_name: &str,
) {
    if let Some(serde_json::Value::Object(child)) = parent.remove(field_name) {
        for (key, value) in child {
            parent.insert(key, value);
        }
    }
}

/// Applies the @slice directive: renames list elements based on FHIRPath result.
///
/// This function takes a list value and a FHIRPath expression, evaluates the
/// expression for each element, and creates named entries in the parent object.
///
/// Note: FHIRPath evaluation requires integration with octofhir-fhirpath.
/// This is a placeholder that will need to be connected to the FHIRPath engine.
pub fn apply_slice_to_list(
    list: Vec<serde_json::Value>,
    _fhirpath: &str,
    field_name: &str,
) -> HashMap<String, serde_json::Value> {
    let mut result = HashMap::new();

    // TODO: Integrate with octofhir-fhirpath to evaluate the expression
    // For now, use index-based naming as a fallback
    for (index, item) in list.into_iter().enumerate() {
        let key = format!("{}.{}", field_name, index);
        result.insert(key, item);
    }

    result
}

/// Schema directive SDL definitions for documentation purposes.
///
/// These SDL definitions describe the FHIR GraphQL directives that the server supports.
/// While async-graphql's dynamic schema doesn't directly support custom directive
/// registration, these definitions document the expected behavior.
pub const FHIR_DIRECTIVES_SDL: &str = r#"
"""
Flatten this field's children into the parent object.
The field itself is not output; its children are added directly to the parent.
"""
directive @flatten on FIELD

"""
Take only the first element from a list.
If the field is an array, only the first element will be returned.
"""
directive @first on FIELD

"""
Indicate that this field should be treated as a single node, not a list.
Overrides default flattening behavior.
"""
directive @singleton on FIELD

"""
Slice list elements by appending FHIRPath expression result to field name.
Each element will be named 'fieldName.{fhirpath_result}'.
"""
directive @slice(fhirpath: String!) on FIELD
"#;

/// Log that FHIR directives are available (called during schema build).
///
/// Since async-graphql dynamic schema doesn't support custom directive registration,
/// we log that the directives are handled via response transformation.
pub fn log_fhir_directives_support() {
    tracing::info!(
        directives = ?[names::FLATTEN, names::FIRST, names::SINGLETON, names::SLICE],
        "FHIR GraphQL directives supported via response transformation"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_apply_first_directive_on_list() {
        let list = Value::List(vec![
            Value::String("first".into()),
            Value::String("second".into()),
        ]);
        let result = apply_first_directive(list);
        assert_eq!(result, Value::String("first".into()));
    }

    #[test]
    fn test_apply_first_directive_on_empty_list() {
        let list = Value::List(vec![]);
        let result = apply_first_directive(list);
        assert_eq!(result, Value::Null);
    }

    #[test]
    fn test_apply_first_directive_on_non_list() {
        let value = Value::String("single".into());
        let result = apply_first_directive(value.clone());
        assert_eq!(result, value);
    }

    #[test]
    fn test_apply_singleton_directive_single_element() {
        let list = Value::List(vec![Value::String("only".into())]);
        let result = apply_singleton_directive(list);
        assert_eq!(result, Value::String("only".into()));
    }

    #[test]
    fn test_apply_singleton_directive_multiple_elements() {
        let list = Value::List(vec![
            Value::String("first".into()),
            Value::String("second".into()),
        ]);
        let result = apply_singleton_directive(list.clone());
        // Multiple elements remain as list
        assert_eq!(result, list);
    }

    #[test]
    fn test_apply_flatten_to_object() {
        let mut parent = Map::new();
        parent.insert("other".into(), serde_json::json!("value"));
        parent.insert("child".into(), serde_json::json!({
            "name": "John",
            "age": 30
        }));

        apply_flatten_to_object(&mut parent, "child");

        assert!(!parent.contains_key("child"));
        assert_eq!(parent.get("name"), Some(&serde_json::json!("John")));
        assert_eq!(parent.get("age"), Some(&serde_json::json!(30)));
        assert_eq!(parent.get("other"), Some(&serde_json::json!("value")));
    }

    #[test]
    fn test_field_directives_has_any() {
        let empty = FieldDirectives::default();
        assert!(!empty.has_any());

        let with_first = FieldDirectives {
            first: true,
            ..Default::default()
        };
        assert!(with_first.has_any());
    }
}
