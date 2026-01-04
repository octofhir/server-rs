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
use async_trait::async_trait;
use octofhir_fhirpath::{Collection, EvaluationContext, FhirPathEngine, ModelProvider};
use serde_json::Map;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::warn;

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
                if let Some(value) = directive.get_argument("fhirpath")
                    && let async_graphql_value::Value::String(s) = &value.node {
                        result.slice_fhirpath = Some(s.clone());
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
pub fn transform_response_value(value: Value, directives: &FieldDirectives) -> Value {
    // Apply @first: take only the first element from a list
    let value = if directives.first {
        apply_first_directive(value)
    } else {
        value
    };

    // Apply @singleton: ensure value is not a list
    

    // Note: @flatten and @slice require access to the parent context
    // and are handled at a higher level in the response transformation
    if directives.singleton {
        apply_singleton_directive(value)
    } else {
        value
    }
}

/// Applies the @first directive: returns only the first element of a list.
fn apply_first_directive(value: Value) -> Value {
    match value {
        Value::List(list) => list.into_iter().next().unwrap_or(Value::Null),
        other => other, // Non-list values pass through unchanged
    }
}

/// Applies the @singleton directive: collapses a single-element list to its value.
fn apply_singleton_directive(value: Value) -> Value {
    match value {
        Value::List(list) if list.len() == 1 => list.into_iter().next().unwrap_or(Value::Null),
        Value::List(list) if list.is_empty() => Value::Null,
        other => other,
    }
}

/// Applies the @flatten directive: merges child fields into parent object.
///
/// This function takes a parent object and a field name, and if that field
/// contains an object, merges the object's fields directly into the parent.
pub fn apply_flatten_to_object(parent: &mut Map<String, serde_json::Value>, field_name: &str) {
    if let Some(serde_json::Value::Object(child)) = parent.remove(field_name) {
        for (key, value) in child {
            parent.insert(key, value);
        }
    }
}

/// Trait for evaluating FHIRPath expressions in slice directives.
///
/// This trait abstracts the FHIRPath evaluation to allow for different
/// implementations (e.g., with or without model provider).
#[async_trait]
pub trait SliceFhirPathEvaluator: Send + Sync {
    /// Evaluate a FHIRPath expression against a JSON value.
    ///
    /// Returns the result as a string suitable for use as a field name suffix.
    async fn evaluate_for_slice(
        &self,
        expression: &str,
        context: &serde_json::Value,
    ) -> Result<String, String>;
}

/// Default FHIRPath evaluator that uses the octofhir-fhirpath engine.
#[allow(dead_code)]
pub struct DefaultSliceEvaluator {
    engine: Arc<FhirPathEngine>,
    model_provider: Arc<dyn ModelProvider + Send + Sync>,
}

impl DefaultSliceEvaluator {
    /// Create a new evaluator with the given engine and model provider.
    #[allow(dead_code)]
    pub fn new(
        engine: Arc<FhirPathEngine>,
        model_provider: Arc<dyn ModelProvider + Send + Sync>,
    ) -> Self {
        Self {
            engine,
            model_provider,
        }
    }
}

#[async_trait]
impl SliceFhirPathEvaluator for DefaultSliceEvaluator {
    async fn evaluate_for_slice(
        &self,
        expression: &str,
        context: &serde_json::Value,
    ) -> Result<String, String> {
        // Create evaluation context from the JSON value
        let collection =
            Collection::from_json_resource(context.clone(), Some(self.model_provider.clone()))
                .await
                .map_err(|e| format!("Failed to create FHIRPath context: {}", e))?;

        let eval_context =
            EvaluationContext::new(collection, self.model_provider.clone(), None, None, None);

        // Evaluate the expression
        let result = self
            .engine
            .evaluate(expression, &eval_context)
            .await
            .map_err(|e| format!("FHIRPath evaluation failed: {}", e))?;

        // Convert result to a string for field naming
        // Take the first value if there are multiple
        if result.value.is_empty() {
            return Err("FHIRPath expression returned no value".to_string());
        }

        // Get the first value and convert to string
        let first_value = &result.value[0];
        let string_value = first_value.to_json_value();

        // Convert to a safe field name suffix
        match string_value {
            serde_json::Value::String(s) => Ok(s),
            serde_json::Value::Number(n) => Ok(n.to_string()),
            serde_json::Value::Bool(b) => Ok(b.to_string()),
            serde_json::Value::Null => Err("FHIRPath expression returned null".to_string()),
            _ => {
                // For complex values, use JSON representation
                Ok(string_value.to_string())
            }
        }
    }
}

/// Applies the @slice directive: renames list elements based on FHIRPath result.
///
/// This function takes a list value and a FHIRPath expression, evaluates the
/// expression for each element, and creates named entries in the parent object.
///
/// Each element will be named `{field_name}.{fhirpath_result}`.
///
/// # Arguments
/// * `list` - The list of JSON values to slice
/// * `fhirpath` - The FHIRPath expression to evaluate for each element
/// * `field_name` - The base field name for the sliced entries
/// * `evaluator` - The FHIRPath evaluator to use
///
/// # Example
/// Given a list of telecom entries and expression `system`, this might produce:
/// - `telecom.phone` for entries where system = "phone"
/// - `telecom.email` for entries where system = "email"
pub async fn apply_slice_to_list(
    list: Vec<serde_json::Value>,
    fhirpath: &str,
    field_name: &str,
    evaluator: Option<&dyn SliceFhirPathEvaluator>,
) -> HashMap<String, serde_json::Value> {
    let mut result = HashMap::new();

    for (index, item) in list.into_iter().enumerate() {
        let key = if let Some(eval) = evaluator {
            match eval.evaluate_for_slice(fhirpath, &item).await {
                Ok(suffix) => format!("{}.{}", field_name, suffix),
                Err(e) => {
                    warn!(
                        error = %e,
                        index,
                        expression = fhirpath,
                        "Failed to evaluate FHIRPath for @slice, using index fallback"
                    );
                    // Fall back to index-based naming on error
                    format!("{}.{}", field_name, index)
                }
            }
        } else {
            // No evaluator provided, use index-based naming
            format!("{}.{}", field_name, index)
        };

        // Handle duplicate keys by appending an index
        let final_key = if result.contains_key(&key) {
            let mut counter = 1;
            loop {
                let candidate = format!("{}_{}", key, counter);
                if !result.contains_key(&candidate) {
                    break candidate;
                }
                counter += 1;
            }
        } else {
            key
        };

        result.insert(final_key, item);
    }

    result
}

/// Synchronous fallback for apply_slice_to_list when no evaluator is available.
///
/// This uses index-based naming as the fallback mechanism.
#[allow(dead_code)]
pub fn apply_slice_to_list_sync(
    list: Vec<serde_json::Value>,
    _fhirpath: &str,
    field_name: &str,
) -> HashMap<String, serde_json::Value> {
    let mut result = HashMap::new();

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
        parent.insert(
            "child".into(),
            serde_json::json!({
                "name": "John",
                "age": 30
            }),
        );

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

    #[test]
    fn test_apply_slice_to_list_sync() {
        let list = vec![
            serde_json::json!({"system": "phone", "value": "555-1234"}),
            serde_json::json!({"system": "email", "value": "test@example.com"}),
        ];

        let result = apply_slice_to_list_sync(list, "system", "telecom");

        assert_eq!(result.len(), 2);
        assert!(result.contains_key("telecom.0"));
        assert!(result.contains_key("telecom.1"));
    }

    /// Mock evaluator for testing slice directive
    struct MockSliceEvaluator {
        /// Map of context values to results for testing
        responses: std::collections::HashMap<String, String>,
    }

    impl MockSliceEvaluator {
        fn new() -> Self {
            Self {
                responses: std::collections::HashMap::new(),
            }
        }

        fn with_response(mut self, key: &str, value: &str) -> Self {
            self.responses.insert(key.to_string(), value.to_string());
            self
        }
    }

    #[async_trait]
    impl SliceFhirPathEvaluator for MockSliceEvaluator {
        async fn evaluate_for_slice(
            &self,
            expression: &str,
            context: &serde_json::Value,
        ) -> Result<String, String> {
            // For testing, extract the field specified by the expression
            if let Some(result) = context.get(expression).and_then(|v| v.as_str()) {
                if let Some(mapped) = self.responses.get(result) {
                    return Ok(mapped.clone());
                }
                return Ok(result.to_string());
            }
            Err(format!(
                "Field '{}' not found in context",
                expression
            ))
        }
    }

    #[tokio::test]
    async fn test_apply_slice_to_list_with_evaluator() {
        let list = vec![
            serde_json::json!({"system": "phone", "value": "555-1234"}),
            serde_json::json!({"system": "email", "value": "test@example.com"}),
        ];

        let evaluator = MockSliceEvaluator::new();
        let result = apply_slice_to_list(list, "system", "telecom", Some(&evaluator)).await;

        assert_eq!(result.len(), 2);
        assert!(result.contains_key("telecom.phone"));
        assert!(result.contains_key("telecom.email"));
    }

    #[tokio::test]
    async fn test_apply_slice_to_list_without_evaluator() {
        let list = vec![
            serde_json::json!({"system": "phone"}),
            serde_json::json!({"system": "email"}),
        ];

        let result: HashMap<String, serde_json::Value> =
            apply_slice_to_list(list, "system", "telecom", None).await;

        // Without evaluator, should use index-based naming
        assert_eq!(result.len(), 2);
        assert!(result.contains_key("telecom.0"));
        assert!(result.contains_key("telecom.1"));
    }

    #[tokio::test]
    async fn test_apply_slice_to_list_with_duplicates() {
        let list = vec![
            serde_json::json!({"system": "phone", "value": "555-1234"}),
            serde_json::json!({"system": "phone", "value": "555-5678"}),
            serde_json::json!({"system": "email", "value": "test@example.com"}),
        ];

        let evaluator = MockSliceEvaluator::new();
        let result = apply_slice_to_list(list, "system", "telecom", Some(&evaluator)).await;

        assert_eq!(result.len(), 3);
        assert!(result.contains_key("telecom.phone"));
        assert!(result.contains_key("telecom.phone_1")); // Duplicate gets suffix
        assert!(result.contains_key("telecom.email"));
    }

    #[tokio::test]
    async fn test_apply_slice_to_list_fallback_on_error() {
        let list = vec![
            serde_json::json!({"other_field": "value"}), // Missing "system" field
        ];

        let evaluator = MockSliceEvaluator::new();
        let result = apply_slice_to_list(list, "system", "telecom", Some(&evaluator)).await;

        // Should fall back to index-based naming on error
        assert_eq!(result.len(), 1);
        assert!(result.contains_key("telecom.0"));
    }
}
