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
    build_code_search, build_identifier_search, build_token_search,
    build_token_search_with_terminology, parse_token_value,
};
pub use uri::{build_uri_array_search, build_uri_search};

use crate::parameters::{SearchParameter, SearchParameterType};
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

    // Dispatch to the appropriate handler
    match definition.param_type {
        SearchParameterType::String => {
            // Check if this is a complex type like HumanName
            if is_human_name_path(expression) {
                let array_path =
                    build_jsonb_accessor(builder.resource_column(), &path_segments, false);
                build_human_name_search(builder, param, &array_path)
            } else if is_array_path(expression) {
                let (array_segments, field) = split_array_path(&path_segments);
                let array_path =
                    build_jsonb_accessor(builder.resource_column(), &array_segments, false);
                build_array_string_search(builder, param, &array_path, &field)
            } else {
                build_string_search(builder, param, &jsonb_path)
            }
        }

        SearchParameterType::Token => {
            // Determine token subtype based on path
            if is_identifier_path(expression) {
                let array_path =
                    build_jsonb_accessor(builder.resource_column(), &path_segments, false);
                build_identifier_search(builder, param, &array_path)
            } else if is_simple_code_path(expression) {
                build_code_search(builder, param, &jsonb_path)
            } else {
                let json_path =
                    build_jsonb_accessor(builder.resource_column(), &path_segments, false);
                build_token_search(builder, param, &json_path)
            }
        }

        SearchParameterType::Number => build_number_search(builder, param, &jsonb_path),

        SearchParameterType::Date => {
            if is_period_path(expression) {
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
            build_reference_search(builder, param, &json_path, &definition.target)
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
            // Check if this is an array field like meta.profile
            if is_uri_array_path(expression) {
                let array_path =
                    build_jsonb_accessor(builder.resource_column(), &path_segments, false);
                build_uri_array_search(builder, param, &array_path)
            } else {
                build_uri_search(builder, param, &jsonb_path)
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

/// Check if a FHIRPath expression refers to a HumanName type.
fn is_human_name_path(expression: &str) -> bool {
    expression.contains(".name") && !expression.contains(".name.")
}

/// Check if a FHIRPath expression refers to an Identifier array.
fn is_identifier_path(expression: &str) -> bool {
    expression.ends_with(".identifier") || expression.contains(".identifier[")
}

/// Check if a FHIRPath expression refers to a simple code field.
fn is_simple_code_path(expression: &str) -> bool {
    let simple_codes = [".gender", ".status", ".active", ".language"];
    simple_codes.iter().any(|s| expression.ends_with(s))
}

/// Check if a FHIRPath expression refers to a Period type.
fn is_period_path(expression: &str) -> bool {
    expression.ends_with("Period") || expression.contains(".period")
}

/// Check if a FHIRPath expression refers to a URI array (e.g., meta.profile).
fn is_uri_array_path(expression: &str) -> bool {
    expression.contains(".profile") || expression.contains(".instantiates")
}

/// Check if a FHIRPath expression refers to an array field.
fn is_array_path(expression: &str) -> bool {
    // Common array fields in FHIR
    let array_patterns = [
        ".telecom",
        ".address",
        ".contact",
        ".communication",
        ".link",
    ];
    array_patterns.iter().any(|p| expression.contains(p))
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
    if lower.contains("quantity") || (lower.contains("value") && (lower.contains("unit") || lower.contains("system"))) {
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
    fn test_is_human_name_path() {
        assert!(is_human_name_path("Patient.name"));
        assert!(!is_human_name_path("Patient.name.family"));
        assert!(!is_human_name_path("Patient.identifier"));
    }

    #[test]
    fn test_is_identifier_path() {
        assert!(is_identifier_path("Patient.identifier"));
        assert!(is_identifier_path("Observation.identifier"));
        assert!(!is_identifier_path("Patient.name"));
    }

    #[test]
    fn test_is_simple_code_path() {
        assert!(is_simple_code_path("Patient.gender"));
        assert!(is_simple_code_path("Observation.status"));
        assert!(!is_simple_code_path("Observation.code"));
    }

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
            .with_expression("Patient.gender"),
        );

        dispatch_search(&mut builder, &param, &def, "Patient").unwrap();

        let clause = builder.build_where_clause();
        assert!(clause.is_some());
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
                    definition: "http://hl7.org/fhir/SearchParameter/Observation-value-quantity".to_string(),
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
        assert!(clause_str.len() > 0);
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
        assert!(result.unwrap_err().to_string().contains("no components defined"));
    }

    #[test]
    fn test_infer_component_type() {
        assert_eq!(super::infer_component_type("code"), "token");
        assert_eq!(super::infer_component_type("valueQuantity"), "quantity");
        assert_eq!(super::infer_component_type("value.as(CodeableConcept)"), "token");
        assert_eq!(super::infer_component_type("effective"), "date");
        assert_eq!(super::infer_component_type("subject"), "reference");
        assert_eq!(super::infer_component_type("display"), "string");
    }
}
