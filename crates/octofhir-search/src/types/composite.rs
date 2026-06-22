//! Composite search parameter type implementation.
//!
//! Composite search parameters combine multiple search criteria into a single parameter.
//! Format: `value1$value2` where each part matches a component of the composite.

use crate::ir::{CompositeClause, CompositeComponentSpec, render_composite_clauses_as_or};
use crate::parameters::ElementTypeHint;
use crate::parameters::SearchParameterType;
use crate::parser::{ParsedParam, ParsedValue};
use crate::sql_builder::{SqlBuilder, SqlBuilderError};

/// Component of a composite search parameter.
#[derive(Debug, Clone)]
pub struct CompositeComponent {
    /// The component parameter name
    pub name: String,
    /// The component parameter type (e.g., "token", "quantity")
    pub param_type: String,
    /// The FHIRPath expression for this component
    pub expression: String,
}

/// A parsed composite search value.
#[derive(Debug, Clone)]
pub struct CompositeValue {
    /// The component values, split by '$'
    pub components: Vec<String>,
}

/// Parse a composite search value (values separated by '$').
pub fn parse_composite_value(value: &str) -> CompositeValue {
    CompositeValue {
        components: value.split('$').map(String::from).collect(),
    }
}

/// Build SQL for a composite search parameter.
pub fn build_composite_search(
    builder: &mut SqlBuilder,
    value: &str,
    components: &[CompositeComponent],
) -> Result<(), SqlBuilderError> {
    let specs = components
        .iter()
        .map(|component| {
            Ok(CompositeComponentSpec {
                code: component.name.clone(),
                search_type: parse_component_type(&component.param_type)?,
                expression: component.expression.clone(),
                element_type_hint: ElementTypeHint::Unknown,
            })
        })
        .collect::<Result<Vec<_>, SqlBuilderError>>()?;
    let param = ParsedParam {
        name: "composite".to_string(),
        modifier: None,
        values: vec![ParsedValue {
            prefix: None,
            raw: value.to_string(),
        }],
    };
    build_composite_search_with_specs(builder, &param, "", &specs)
}

pub fn build_composite_search_with_specs(
    builder: &mut SqlBuilder,
    param: &ParsedParam,
    resource_type: &str,
    components: &[CompositeComponentSpec],
) -> Result<(), SqlBuilderError> {
    let clauses = CompositeClause::from_parsed_param(param, resource_type, components)?;
    if let Some(sql) = render_composite_clauses_as_or(builder, &clauses)? {
        builder.add_condition(sql);
    }

    Ok(())
}


fn parse_component_type(param_type: &str) -> Result<SearchParameterType, SqlBuilderError> {
    match param_type {
        "token" => Ok(SearchParameterType::Token),
        "string" => Ok(SearchParameterType::String),
        "quantity" => Ok(SearchParameterType::Quantity),
        "date" => Ok(SearchParameterType::Date),
        "reference" => Ok(SearchParameterType::Reference),
        "number" => Ok(SearchParameterType::Number),
        other => Err(SqlBuilderError::NotImplemented(format!(
            "Composite component type '{other}' not supported"
        ))),
    }
}

#[cfg(test)]
fn extract_prefix(value: &str) -> (&str, &str) {
    for prefix in ["ge", "le", "gt", "lt", "ne", "sa", "eb", "ap"] {
        if let Some(rest) = value.strip_prefix(prefix) {
            return (prefix, rest);
        }
    }
    ("eq", value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_composite_value() {
        let r = parse_composite_value("http://loinc.org|8480-6$gt100");
        assert_eq!(r.components, vec!["http://loinc.org|8480-6", "gt100"]);
    }

    #[test]
    fn test_extract_prefix() {
        assert_eq!(extract_prefix("100"), ("eq", "100"));
        assert_eq!(extract_prefix("gt100"), ("gt", "100"));
        assert_eq!(extract_prefix("le50.5"), ("le", "50.5"));
    }

    #[test]
    fn test_build_composite_search() {
        let mut builder = SqlBuilder::new();
        let components = vec![
            CompositeComponent {
                name: "code".to_string(),
                param_type: "token".to_string(),
                expression: "Observation.code".to_string(),
            },
            CompositeComponent {
                name: "value".to_string(),
                param_type: "quantity".to_string(),
                expression: "Observation.valueQuantity".to_string(),
            },
        ];

        let result =
            build_composite_search(&mut builder, "http://loinc.org|8480-6$gt100", &components);
        assert!(result.is_ok());
        let clause = builder.build_where_clause().unwrap();
        assert!(clause.contains("(resource->'valueQuantity'->>'value')::numeric > "));
        // Token component navigates the `code` element as a JSON object (`->`),
        // not a text accessor (`->>`), so the token renderer can reach `coding`.
        assert!(
            clause.contains("resource->'code'"),
            "CLAUSE={clause}"
        );
    }

    /// Lock the `code-value-quantity` fix with the *real* FHIR component
    /// expressions: the value component carries a polymorphic cast
    /// `value.as(Quantity)` that must lower to the stored `valueQuantity` key
    /// (not leak `as(Quantity)` as a path segment), and the token component's
    /// bare `code` must navigate as a JSON object (`->`), not text (`->>`).
    #[test]
    fn test_build_composite_search_polymorphic_cast() {
        let mut builder = SqlBuilder::new();
        let components = vec![
            CompositeComponent {
                name: "code".to_string(),
                param_type: "token".to_string(),
                expression: "code".to_string(),
            },
            CompositeComponent {
                name: "value".to_string(),
                param_type: "quantity".to_string(),
                expression: "value.as(Quantity)".to_string(),
            },
        ];

        let result = build_composite_search(&mut builder, "8302-2$159.5", &components);
        assert!(result.is_ok());
        let clause = builder.build_where_clause().unwrap();
        // value.as(Quantity) -> valueQuantity (polymorphic cast folded by the AST)
        assert!(
            clause.contains("(resource->'valueQuantity'->>'value')::numeric"),
            "CLAUSE={clause}"
        );
        // `as(Quantity)` must NOT leak into the path.
        assert!(!clause.contains("as(Quantity)"), "CLAUSE={clause}");
        // Token leaf is a JSON object accessor.
        assert!(clause.contains("resource->'code'"), "CLAUSE={clause}");
    }

    #[test]
    fn test_build_composite_search_wrong_count() {
        let mut builder = SqlBuilder::new();
        let components = vec![CompositeComponent {
            name: "code".to_string(),
            param_type: "token".to_string(),
            expression: "Observation.code".to_string(),
        }];

        let result = build_composite_search(&mut builder, "value1$value2", &components);
        assert!(matches!(
            result,
            Err(SqlBuilderError::InvalidSearchValue(_))
        ));
    }

    #[test]
    fn test_build_composite_search_empty_component() {
        let mut builder = SqlBuilder::new();
        let components = vec![
            CompositeComponent {
                name: "code".to_string(),
                param_type: "token".to_string(),
                expression: "Observation.code".to_string(),
            },
            CompositeComponent {
                name: "value".to_string(),
                param_type: "quantity".to_string(),
                expression: "Observation.valueQuantity".to_string(),
            },
        ];

        let result = build_composite_search(&mut builder, "$gt100", &components);
        assert!(result.is_ok());
        let clause = builder.build_where_clause().unwrap();
        assert!(clause.contains("value") && !clause.contains("system"));
    }
}
