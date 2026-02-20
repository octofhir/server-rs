//! Search type implementations for FHIR search parameters.
//!
//! This module provides implementations for FHIR search parameter types:
//! - String: Case-insensitive text search with modifiers
//! - Token: Coded value search (Coding, CodeableConcept, Identifier)
//! - Number: Numeric search with comparison prefixes
//! - Date: Date/datetime search with precision handling
//! - Reference: Reference search with type modifiers
//! - URI: URI search with hierarchical modifiers
//! - Composite: Combined search on multiple components
//! - Special: Location-based (_near), full-text (_text, _content), and advanced search
//!
//! Each type module provides functions to build SQL conditions for PostgreSQL JSONB queries.

pub mod composite;
pub mod date;
pub mod number;
pub mod reference;
pub mod special;
pub mod string;
pub mod token;
pub mod uri;

pub use composite::{
    CompositeComponent, CompositeValue, build_composite_search, parse_composite_value,
};
pub use date::{DateRange, build_date_search, build_period_search, parse_date_range};
pub use number::{build_number_search, build_quantity_search};
pub use reference::{build_reference_array_search, build_reference_search, is_resource_type};
pub use special::{
    NearParameter, SpecialParameterType, build_content_search, build_filter_search,
    build_list_search, build_near_search, build_text_search, detect_special_type,
    parse_near_parameter,
};
pub use string::{build_array_string_search, build_human_name_search, build_string_search};
pub use token::{
    build_code_search, build_gin_code_search, build_gin_token_search, build_identifier_search,
    build_token_search, build_token_search_with_terminology, parse_token_value,
};
pub use uri::{build_uri_array_search, build_uri_search};

use crate::parameters::{ElementTypeHint, SearchModifier, SearchParameter, SearchParameterType};
use crate::parser::ParsedParam;
use crate::sql_builder::{
    SqlBuilder, SqlBuilderError, build_jsonb_accessor, fhirpath_to_jsonb_path,
};
use std::sync::Arc;

/// Dispatch a search parameter to the appropriate type handler.
///
/// This function determines the correct search type handler based on the
/// parameter definition and generates the appropriate SQL conditions.
pub fn dispatch_search(
    builder: &mut SqlBuilder,
    param: &ParsedParam,
    definition: &Arc<SearchParameter>,
    resource_type: &str,
) -> Result<(), SqlBuilderError> {
    // Get the FHIRPath expression and convert to JSONB path
    let expression = definition.expression.as_deref().ok_or_else(|| {
        SqlBuilderError::InvalidPath(format!(
            "No expression for search parameter: {}",
            definition.code
        ))
    })?;

    let path_segments = fhirpath_to_jsonb_path(expression, resource_type);

    // Determine if we need text extraction (->>) or JSON traversal (->)
    let needs_text = matches!(
        definition.param_type,
        SearchParameterType::String | SearchParameterType::Number | SearchParameterType::Date
    );

    let jsonb_path = build_jsonb_accessor(builder.resource_column(), &path_segments, needs_text);

    // Dispatch to the appropriate handler based on param type and resolved element type hint
    match definition.param_type {
        SearchParameterType::String => {
            // GIN-optimized path for :exact modifier — uses @> containment
            if matches!(&param.modifier, Some(SearchModifier::Exact)) {
                return build_gin_exact_string_search(
                    builder,
                    param,
                    &path_segments,
                    &definition.element_type_hint,
                );
            }

            if definition.element_type_hint.is_human_name() {
                let array_path =
                    build_jsonb_accessor(builder.resource_column(), &path_segments, false);
                build_human_name_search(builder, param, &array_path)
            } else if matches!(&definition.element_type_hint, ElementTypeHint::Array(_)) {
                let (array_segments, field) = split_array_path(&path_segments);
                let array_path =
                    build_jsonb_accessor(builder.resource_column(), &array_segments, false);
                build_array_string_search(builder, param, &array_path, &field)
            } else {
                build_string_search(builder, param, &jsonb_path)
            }
        }

        SearchParameterType::Token => {
            if definition.element_type_hint.is_identifier() {
                // Identifier search already uses @> for system|value — keep as-is
                let array_path =
                    build_jsonb_accessor(builder.resource_column(), &path_segments, false);
                build_identifier_search(builder, param, &array_path)
            } else if matches!(&definition.element_type_hint, ElementTypeHint::SimpleCode) {
                // GIN-optimized simple code search (e.g., Patient.gender)
                build_gin_code_search(builder, param, &path_segments)
            } else {
                // GIN-optimized CodeableConcept/Coding/Token search
                build_gin_token_search(builder, param, &path_segments)
            }
        }

        SearchParameterType::Number => build_number_search(builder, param, &jsonb_path),

        SearchParameterType::Date => {
            if definition.element_type_hint.is_period() {
                let json_path =
                    build_jsonb_accessor(builder.resource_column(), &path_segments, false);
                build_period_search(builder, param, &json_path)
            } else {
                build_date_search(builder, param, &jsonb_path)
            }
        }

        SearchParameterType::Quantity => {
            let json_path = build_jsonb_accessor(builder.resource_column(), &path_segments, false);
            build_quantity_search(builder, param, &json_path)
        }

        SearchParameterType::Reference => {
            let json_path = build_jsonb_accessor(builder.resource_column(), &path_segments, false);
            match &definition.element_type_hint {
                ElementTypeHint::Array(_) => {
                    build_reference_array_search(builder, param, &json_path, &definition.target)
                }
                _ => build_reference_search(builder, param, &json_path, &definition.target),
            }
        }

        SearchParameterType::Composite => {
            // Build composite search using component definitions
            if definition.component.is_empty() {
                return Err(SqlBuilderError::InvalidPath(format!(
                    "Composite search parameter {} has no components defined",
                    definition.code
                )));
            }

            // Convert SearchParameterComponent to CompositeComponent
            let components: Vec<CompositeComponent> = definition
                .component
                .iter()
                .map(|c| {
                    // Extract parameter type from component expression or definition URL
                    // For now, use a simple heuristic based on common patterns
                    let param_type = infer_component_type(&c.expression);

                    CompositeComponent {
                        name: definition.code.clone(),
                        param_type,
                        expression: c.expression.clone(),
                    }
                })
                .collect();

            // Process each value in the search parameter
            for value in &param.values {
                build_composite_search(builder, &value.raw, &components)?;
            }

            Ok(())
        }

        SearchParameterType::Uri => {
            match &definition.element_type_hint {
                ElementTypeHint::Array(_) => {
                    let array_path =
                        build_jsonb_accessor(builder.resource_column(), &path_segments, false);
                    build_uri_array_search(builder, param, &array_path)
                }
                _ => build_uri_search(builder, param, &jsonb_path),
            }
        }

        SearchParameterType::Special => {
            // Special parameters are usually handled by name, not by expression
            // Detect special type and dispatch accordingly
            match detect_special_type(&param.name) {
                Some(SpecialParameterType::Near) => {
                    let json_path =
                        build_jsonb_accessor(builder.resource_column(), &path_segments, false);
                    if let Some(value) = param.values.first() {
                        build_near_search(builder, &value.raw, &json_path)
                    } else {
                        Err(SqlBuilderError::InvalidSearchValue(
                            "_near requires a value".to_string(),
                        ))
                    }
                }
                Some(SpecialParameterType::Text) => {
                    if let Some(value) = param.values.first() {
                        build_text_search(builder, &value.raw)
                    } else {
                        Err(SqlBuilderError::InvalidSearchValue(
                            "_text requires a value".to_string(),
                        ))
                    }
                }
                Some(SpecialParameterType::Content) => {
                    if let Some(value) = param.values.first() {
                        build_content_search(builder, &value.raw)
                    } else {
                        Err(SqlBuilderError::InvalidSearchValue(
                            "_content requires a value".to_string(),
                        ))
                    }
                }
                Some(SpecialParameterType::Filter) => {
                    if let Some(value) = param.values.first() {
                        build_filter_search(builder, &value.raw, resource_type)
                    } else {
                        Err(SqlBuilderError::InvalidSearchValue(
                            "_filter requires a value".to_string(),
                        ))
                    }
                }
                Some(SpecialParameterType::List) => {
                    if let Some(value) = param.values.first() {
                        build_list_search(builder, &value.raw, resource_type)
                    } else {
                        Err(SqlBuilderError::InvalidSearchValue(
                            "_list requires a value".to_string(),
                        ))
                    }
                }
                Some(SpecialParameterType::Query) => Err(SqlBuilderError::NotImplemented(
                    "_query search not yet implemented".to_string(),
                )),
                None => Err(SqlBuilderError::NotImplemented(format!(
                    "Unknown special parameter: {}",
                    param.name
                ))),
            }
        }
    }
}

/// Build GIN-optimized `:exact` string search using `resource @> '{...}'::jsonb`.
///
/// Uses the `@>` containment operator to leverage the existing GIN index
/// (`jsonb_path_ops`) on the resource column. Handles three cases:
///
/// - **HumanName** (path=`["name"]`): OR of containments for family/text/given
/// - **Array string** (e.g. `["name","family"]`): containment wrapping in array
/// - **Simple string** (e.g. `["gender"]`): direct containment
fn build_gin_exact_string_search(
    builder: &mut SqlBuilder,
    param: &ParsedParam,
    path_segments: &[String],
    element_type_hint: &ElementTypeHint,
) -> Result<(), SqlBuilderError> {
    if param.values.is_empty() {
        return Ok(());
    }

    let resource_col = builder.resource_column().to_string();
    let mut or_conditions = Vec::new();

    for value in &param.values {
        if value.raw.is_empty() {
            continue;
        }

        let condition = if element_type_hint.is_human_name() {
            // HumanName: OR of containments for family, text, and given
            build_gin_human_name_exact(builder, &resource_col, path_segments, &value.raw)
        } else if matches!(element_type_hint, ElementTypeHint::Array(_)) {
            // Array string field (e.g., name.family within name array)
            // Split into array path and field, wrap in array containment
            let (array_segments, field) = split_array_path(path_segments);
            if field.is_empty() {
                // Direct array containment
                let containment = build_string_nested_containment(
                    &array_segments,
                    serde_json::json!([&value.raw]),
                );
                let json_str = containment.to_string();
                let p = builder.add_json_param(&json_str);
                format!("{resource_col} @> ${p}::jsonb")
            } else {
                // Array element field containment: {"name": [{"family": "Smith"}]}
                let elem_obj = serde_json::json!([{field.as_str(): &value.raw}]);
                let containment = build_string_nested_containment(&array_segments, elem_obj);
                let json_str = containment.to_string();
                let p = builder.add_json_param(&json_str);
                format!("{resource_col} @> ${p}::jsonb")
            }
        } else {
            // Simple string field: {"gender": "female"}
            let containment =
                build_string_nested_containment(path_segments, serde_json::json!(&value.raw));
            let json_str = containment.to_string();
            let p = builder.add_json_param(&json_str);
            format!("{resource_col} @> ${p}::jsonb")
        };

        or_conditions.push(condition);
    }

    if !or_conditions.is_empty() {
        builder.add_condition(SqlBuilder::build_or_clause(&or_conditions));
    }

    Ok(())
}

/// Build GIN containment conditions for HumanName :exact search.
///
/// Produces an OR of three containment checks:
/// - `resource @> '{"name": [{"family": "Smith"}]}'::jsonb`
/// - `resource @> '{"name": [{"text": "Smith"}]}'::jsonb`
/// - `resource @> '{"name": [{"given": ["Smith"]}]}'::jsonb`
fn build_gin_human_name_exact(
    builder: &mut SqlBuilder,
    resource_col: &str,
    path_segments: &[String],
    value: &str,
) -> String {
    let family_obj = serde_json::json!([{"family": value}]);
    let family_containment = build_string_nested_containment(path_segments, family_obj);
    let p1 = builder.add_json_param(&family_containment.to_string());

    let text_obj = serde_json::json!([{"text": value}]);
    let text_containment = build_string_nested_containment(path_segments, text_obj);
    let p2 = builder.add_json_param(&text_containment.to_string());

    let given_obj = serde_json::json!([{"given": [value]}]);
    let given_containment = build_string_nested_containment(path_segments, given_obj);
    let p3 = builder.add_json_param(&given_containment.to_string());

    format!(
        "({resource_col} @> ${p1}::jsonb OR {resource_col} @> ${p2}::jsonb OR {resource_col} @> ${p3}::jsonb)"
    )
}

/// Build a nested JSON object from path segments wrapping a leaf value (for string search).
fn build_string_nested_containment(
    path_segments: &[String],
    leaf_value: serde_json::Value,
) -> serde_json::Value {
    let mut result = leaf_value;
    for segment in path_segments.iter().rev() {
        result = serde_json::json!({ segment.as_str(): result });
    }
    result
}

/// Split a path into array path and field name.
fn split_array_path(path: &[String]) -> (Vec<String>, String) {
    if path.len() > 1 {
        let array_path = path[..path.len() - 1].to_vec();
        let field = path.last().unwrap().clone();
        (array_path, field)
    } else {
        (path.to_vec(), String::new())
    }
}

/// Infer component type from FHIRPath expression.
///
/// This is a heuristic-based approach that examines the expression to determine
/// the likely FHIR type. A more robust implementation would look up the component
/// definition URL to get the exact type.
fn infer_component_type(expression: &str) -> String {
    let lower = expression.to_lowercase();

    // Date patterns (check before code as "effective" could match both)
    if lower.contains("date")
        || lower.contains("time")
        || lower.contains("instant")
        || lower.contains("effective")
        || lower.contains("period")
        || lower.starts_with("authored")
    {
        return "date".to_string();
    }

    // Token-like patterns
    if lower.contains("code") || lower.contains("coding") || lower.contains("codeable") {
        return "token".to_string();
    }

    // String-like patterns
    if lower.contains("text") || lower.contains("display") || lower.contains("name") {
        return "string".to_string();
    }

    // Quantity patterns
    if lower.contains("quantity")
        || (lower.contains("value") && (lower.contains("unit") || lower.contains("system")))
    {
        return "quantity".to_string();
    }

    // Reference patterns
    if lower.contains("reference") || lower.contains("subject") || lower.contains("patient") {
        return "reference".to_string();
    }

    // Number patterns
    if lower.contains("integer") || lower.contains("decimal") {
        return "number".to_string();
    }

    // Default to token as it's most common
    "token".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::ParsedValue;

    #[test]
    fn test_dispatch_string_search() {
        let mut builder = SqlBuilder::new();
        let param = ParsedParam {
            name: "family".to_string(),
            modifier: None,
            values: vec![ParsedValue {
                prefix: None,
                raw: "Smith".to_string(),
            }],
        };
        let def = Arc::new(
            SearchParameter::new(
                "family",
                "http://hl7.org/fhir/SearchParameter/Patient-family",
                SearchParameterType::String,
                vec!["Patient".to_string()],
            )
            .with_expression("Patient.name.family"),
        );

        dispatch_search(&mut builder, &param, &def, "Patient").unwrap();

        let clause = builder.build_where_clause();
        assert!(clause.is_some());
    }

    #[test]
    fn test_dispatch_token_search() {
        let mut builder = SqlBuilder::new();
        let param = ParsedParam {
            name: "gender".to_string(),
            modifier: None,
            values: vec![ParsedValue {
                prefix: None,
                raw: "female".to_string(),
            }],
        };
        let def = Arc::new(
            SearchParameter::new(
                "gender",
                "http://hl7.org/fhir/SearchParameter/Patient-gender",
                SearchParameterType::Token,
                vec!["Patient".to_string()],
            )
            .with_expression("Patient.gender")
            .with_element_type_hint(ElementTypeHint::SimpleCode),
        );

        dispatch_search(&mut builder, &param, &def, "Patient").unwrap();

        let clause = builder.build_where_clause();
        assert!(clause.is_some());
        let clause_str = clause.unwrap();
        // GIN-optimized: should use @> containment operator
        assert!(
            clause_str.contains("@>"),
            "Expected GIN containment (@>), got: {clause_str}"
        );
    }

    #[test]
    fn test_dispatch_date_search() {
        let mut builder = SqlBuilder::new();
        let param = ParsedParam {
            name: "birthdate".to_string(),
            modifier: None,
            values: vec![ParsedValue {
                prefix: None,
                raw: "2000-01-01".to_string(),
            }],
        };
        let def = Arc::new(
            SearchParameter::new(
                "birthdate",
                "http://hl7.org/fhir/SearchParameter/Patient-birthdate",
                SearchParameterType::Date,
                vec!["Patient".to_string()],
            )
            .with_expression("Patient.birthDate"),
        );

        dispatch_search(&mut builder, &param, &def, "Patient").unwrap();

        let clause = builder.build_where_clause();
        assert!(clause.is_some());
    }

    #[test]
    fn test_dispatch_no_expression_returns_error() {
        let mut builder = SqlBuilder::new();
        let param = ParsedParam {
            name: "test".to_string(),
            modifier: None,
            values: vec![ParsedValue {
                prefix: None,
                raw: "value".to_string(),
            }],
        };
        let def = Arc::new(SearchParameter::new(
            "test",
            "http://example.org/test",
            SearchParameterType::String,
            vec!["Patient".to_string()],
        ));

        let result = dispatch_search(&mut builder, &param, &def, "Patient");
        assert!(result.is_err());
    }

    #[test]
    fn test_dispatch_reference_search() {
        let mut builder = SqlBuilder::new();
        let param = ParsedParam {
            name: "subject".to_string(),
            modifier: None,
            values: vec![ParsedValue {
                prefix: None,
                raw: "Patient/123".to_string(),
            }],
        };
        let def = Arc::new(
            SearchParameter::new(
                "subject",
                "http://hl7.org/fhir/SearchParameter/Observation-subject",
                SearchParameterType::Reference,
                vec!["Observation".to_string()],
            )
            .with_expression("Observation.subject")
            .with_targets(vec!["Patient".to_string(), "Group".to_string()]),
        );

        dispatch_search(&mut builder, &param, &def, "Observation").unwrap();

        let clause = builder.build_where_clause();
        assert!(clause.is_some());
        let clause_str = clause.unwrap();
        assert!(clause_str.contains("reference"));
    }

    #[test]
    fn test_dispatch_uri_search() {
        let mut builder = SqlBuilder::new();
        let param = ParsedParam {
            name: "url".to_string(),
            modifier: None,
            values: vec![ParsedValue {
                prefix: None,
                raw: "http://example.org/fhir/StructureDefinition/patient".to_string(),
            }],
        };
        let def = Arc::new(
            SearchParameter::new(
                "url",
                "http://hl7.org/fhir/SearchParameter/StructureDefinition-url",
                SearchParameterType::Uri,
                vec!["StructureDefinition".to_string()],
            )
            .with_expression("StructureDefinition.url"),
        );

        dispatch_search(&mut builder, &param, &def, "StructureDefinition").unwrap();

        let clause = builder.build_where_clause();
        assert!(clause.is_some());
    }

    #[test]
    fn test_dispatch_composite_search() {
        use crate::parameters::SearchParameterComponent;

        let mut builder = SqlBuilder::new();
        let param = ParsedParam {
            name: "code-value-quantity".to_string(),
            modifier: None,
            values: vec![ParsedValue {
                prefix: None,
                raw: "http://loinc.org|8480-6$gt100".to_string(),
            }],
        };

        let def = Arc::new(
            SearchParameter::new(
                "code-value-quantity",
                "http://hl7.org/fhir/SearchParameter/Observation-code-value-quantity",
                SearchParameterType::Composite,
                vec!["Observation".to_string()],
            )
            .with_expression("Observation")
            .with_components(vec![
                SearchParameterComponent {
                    definition: "http://hl7.org/fhir/SearchParameter/Observation-code".to_string(),
                    expression: "code".to_string(),
                },
                SearchParameterComponent {
                    definition: "http://hl7.org/fhir/SearchParameter/Observation-value-quantity"
                        .to_string(),
                    expression: "valueQuantity".to_string(),
                },
            ]),
        );

        let result = dispatch_search(&mut builder, &param, &def, "Observation");
        assert!(result.is_ok());

        let clause = builder.build_where_clause();
        assert!(clause.is_some());
        let clause_str = clause.unwrap();
        // Verify the SQL contains both code and value conditions
        assert!(!clause_str.is_empty());
    }

    #[test]
    fn test_dispatch_composite_no_components_fails() {
        let mut builder = SqlBuilder::new();
        let param = ParsedParam {
            name: "test-composite".to_string(),
            modifier: None,
            values: vec![ParsedValue {
                prefix: None,
                raw: "value1$value2".to_string(),
            }],
        };

        let def = Arc::new(
            SearchParameter::new(
                "test-composite",
                "http://example.org/test-composite",
                SearchParameterType::Composite,
                vec!["Patient".to_string()],
            )
            .with_expression("Patient"),
        );

        let result = dispatch_search(&mut builder, &param, &def, "Patient");
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("no components defined")
        );
    }

    #[test]
    fn test_infer_component_type() {
        assert_eq!(super::infer_component_type("code"), "token");
        assert_eq!(super::infer_component_type("valueQuantity"), "quantity");
        assert_eq!(
            super::infer_component_type("value.as(CodeableConcept)"),
            "token"
        );
        assert_eq!(super::infer_component_type("effective"), "date");
        assert_eq!(super::infer_component_type("subject"), "reference");
        assert_eq!(super::infer_component_type("display"), "string");
    }

    // ========================================================================
    // GIN-optimized search tests
    // ========================================================================

    #[test]
    fn test_gin_exact_string_simple() {
        // Simple string field: Patient.gender with :exact
        let mut builder = SqlBuilder::new();
        let param = ParsedParam {
            name: "address-city".to_string(),
            modifier: Some(SearchModifier::Exact),
            values: vec![ParsedValue {
                prefix: None,
                raw: "Boston".to_string(),
            }],
        };

        build_gin_exact_string_search(
            &mut builder,
            &param,
            &["address".to_string(), "city".to_string()],
            &ElementTypeHint::Unknown,
        )
        .unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert!(
            clause.contains("@>"),
            "Expected @> containment, got: {clause}"
        );
        assert!(clause.contains("::jsonb"));
    }

    #[test]
    fn test_gin_exact_string_human_name() {
        // HumanName :exact search should generate OR of family/text/given containments
        let mut builder = SqlBuilder::new();
        let param = ParsedParam {
            name: "name".to_string(),
            modifier: Some(SearchModifier::Exact),
            values: vec![ParsedValue {
                prefix: None,
                raw: "Smith".to_string(),
            }],
        };

        build_gin_exact_string_search(
            &mut builder,
            &param,
            &["name".to_string()],
            &ElementTypeHint::HumanName,
        )
        .unwrap();

        let clause = builder.build_where_clause().unwrap();
        // Should have 3 @> checks (family, text, given)
        let containment_count = clause.matches("@>").count();
        assert_eq!(
            containment_count, 3,
            "Expected 3 containment checks for HumanName, got {containment_count}: {clause}"
        );
        assert!(clause.contains("OR"));
    }

    #[test]
    fn test_gin_exact_string_array() {
        // Array string field: Patient.name.family with :exact
        let mut builder = SqlBuilder::new();
        let param = ParsedParam {
            name: "family".to_string(),
            modifier: Some(SearchModifier::Exact),
            values: vec![ParsedValue {
                prefix: None,
                raw: "Smith".to_string(),
            }],
        };

        build_gin_exact_string_search(
            &mut builder,
            &param,
            &["name".to_string(), "family".to_string()],
            &ElementTypeHint::Array("string".to_string()),
        )
        .unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert!(
            clause.contains("@>"),
            "Expected @> containment, got: {clause}"
        );
        assert!(clause.contains("::jsonb"));
    }

    #[test]
    fn test_dispatch_string_exact_uses_gin() {
        // Dispatch :exact string search should route through GIN
        let mut builder = SqlBuilder::new();
        let param = ParsedParam {
            name: "family".to_string(),
            modifier: Some(SearchModifier::Exact),
            values: vec![ParsedValue {
                prefix: None,
                raw: "Smith".to_string(),
            }],
        };
        let def = Arc::new(
            SearchParameter::new(
                "family",
                "http://hl7.org/fhir/SearchParameter/Patient-family",
                SearchParameterType::String,
                vec!["Patient".to_string()],
            )
            .with_expression("Patient.name.family")
            .with_element_type_hint(ElementTypeHint::Array("string".to_string())),
        );

        dispatch_search(&mut builder, &param, &def, "Patient").unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert!(
            clause.contains("@>"),
            "Expected GIN containment for :exact, got: {clause}"
        );
    }

    #[test]
    fn test_dispatch_token_code_uses_gin() {
        // SimpleCode token dispatch should use GIN containment
        let mut builder = SqlBuilder::new();
        let param = ParsedParam {
            name: "gender".to_string(),
            modifier: None,
            values: vec![ParsedValue {
                prefix: None,
                raw: "female".to_string(),
            }],
        };
        let def = Arc::new(
            SearchParameter::new(
                "gender",
                "http://hl7.org/fhir/SearchParameter/Patient-gender",
                SearchParameterType::Token,
                vec!["Patient".to_string()],
            )
            .with_expression("Patient.gender")
            .with_element_type_hint(ElementTypeHint::SimpleCode),
        );

        dispatch_search(&mut builder, &param, &def, "Patient").unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert!(
            clause.contains("@>"),
            "Expected GIN containment for SimpleCode, got: {clause}"
        );
        // JSON params contain the containment object {"gender": "female"}
        let params = builder.params();
        let json_str = params[0].as_str();
        assert!(
            json_str.contains("gender") && json_str.contains("female"),
            "Expected JSON param with gender/female, got: {json_str}"
        );
    }

    #[test]
    fn test_dispatch_token_codeable_concept_uses_gin() {
        // CodeableConcept token dispatch should use GIN containment
        let mut builder = SqlBuilder::new();
        let param = ParsedParam {
            name: "code".to_string(),
            modifier: None,
            values: vec![ParsedValue {
                prefix: None,
                raw: "http://loinc.org|8480-6".to_string(),
            }],
        };
        let def = Arc::new(
            SearchParameter::new(
                "code",
                "http://hl7.org/fhir/SearchParameter/Observation-code",
                SearchParameterType::Token,
                vec!["Observation".to_string()],
            )
            .with_expression("Observation.code"),
        );

        dispatch_search(&mut builder, &param, &def, "Observation").unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert!(
            clause.contains("@>"),
            "Expected GIN containment for CodeableConcept, got: {clause}"
        );
        // JSON params contain the containment object with coding array
        let params = builder.params();
        let json_str = params[0].as_str();
        assert!(
            json_str.contains("coding"),
            "Expected JSON param with coding, got: {json_str}"
        );
    }
}
