//! Composite search parameter type implementation.
//!
//! Composite search parameters combine multiple search criteria into a single parameter.
//! Format: `value1$value2` where each part matches a component of the composite.

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
    let parsed = parse_composite_value(value);

    if parsed.components.len() != components.len() {
        return Err(SqlBuilderError::InvalidSearchValue(format!(
            "Composite parameter expects {} components, got {}",
            components.len(),
            parsed.components.len()
        )));
    }

    let conditions: Vec<String> = parsed
        .components
        .iter()
        .zip(components.iter())
        .filter(|(v, _)| !v.is_empty())
        .map(|(v, def)| build_component_condition(builder, v, def))
        .collect::<Result<_, _>>()?;

    if !conditions.is_empty() {
        builder.add_condition(conditions.join(" AND "));
    }

    Ok(())
}

fn build_component_condition(
    builder: &mut SqlBuilder,
    value: &str,
    component: &CompositeComponent,
) -> Result<String, SqlBuilderError> {
    let json_path = expression_to_jsonb_path(&component.expression);

    match component.param_type.as_str() {
        "token" => build_token_component(builder, value, &json_path),
        "string" => {
            let p = builder.add_text_param(format!("{value}%"));
            Ok(format!("({json_path} ILIKE ${p})"))
        }
        "quantity" => build_quantity_component(builder, value, &json_path),
        "date" => {
            let (prefix, date_str) = extract_prefix(value);
            let p = builder.add_text_param(date_str);
            Ok(format!(
                "({json_path}::timestamp {} ${p}::timestamp)",
                prefix_to_comparator(prefix)
            ))
        }
        "reference" => {
            let base = to_object_path(&json_path);
            let p = builder.add_text_param(value);
            Ok(format!("({base}->>'reference' = ${p})"))
        }
        "number" => {
            let (prefix, num_str) = extract_prefix(value);
            let p = builder.add_text_param(num_str);
            Ok(format!(
                "({json_path}::numeric {} ${p}::numeric)",
                prefix_to_comparator(prefix)
            ))
        }
        _ => Err(SqlBuilderError::NotImplemented(format!(
            "Composite component type '{}' not supported",
            component.param_type
        ))),
    }
}

/// Convert FHIRPath expression to JSONB path.
fn expression_to_jsonb_path(expression: &str) -> String {
    let path = expression
        .find('.')
        .map_or(expression, |i| &expression[i + 1..]);
    let parts: Vec<&str> = path.split('.').filter(|p| !p.is_empty()).collect();

    if parts.is_empty() {
        return "resource".to_string();
    }

    let mut acc = "resource".to_string();
    for (i, part) in parts.iter().enumerate() {
        let op = if i == parts.len() - 1 { "->>" } else { "->" };
        acc.push_str(&format!("{op}'{part}'"));
    }
    acc
}

/// Convert text path (->>) to object path (->).
fn to_object_path(path: &str) -> String {
    if let Some(idx) = path.rfind("->>") {
        let last_part = path[idx + 3..].trim_matches('\'');
        format!("{}->'{}'", &path[..idx].trim_end_matches("->"), last_part)
    } else {
        path.to_string()
    }
}

fn build_token_component(
    builder: &mut SqlBuilder,
    value: &str,
    json_path: &str,
) -> Result<String, SqlBuilderError> {
    if let Some((system, code)) = value.split_once('|') {
        let base = to_object_path(json_path);
        match (system.is_empty(), code.is_empty()) {
            (true, false) => {
                let p = builder.add_text_param(code);
                Ok(format!("({base}->>'code' = ${p})"))
            }
            (false, true) => {
                let p = builder.add_text_param(system);
                Ok(format!("({base}->>'system' = ${p})"))
            }
            _ => {
                let p1 = builder.add_text_param(system);
                let p2 = builder.add_text_param(code);
                Ok(format!(
                    "({base}->>'system' = ${p1} AND {base}->>'code' = ${p2})"
                ))
            }
        }
    } else {
        let p = builder.add_text_param(value);
        Ok(format!("({json_path} = ${p})"))
    }
}

fn build_quantity_component(
    builder: &mut SqlBuilder,
    value: &str,
    json_path: &str,
) -> Result<String, SqlBuilderError> {
    let base = to_object_path(json_path);
    let parts: Vec<&str> = value.split('|').collect();
    let (prefix, num_str) = extract_prefix(parts[0]);

    let p = builder.add_text_param(num_str);
    let value_cond = format!(
        "({base}->>'value')::numeric {} ${p}::numeric",
        prefix_to_comparator(prefix)
    );

    if parts.len() >= 3 {
        let mut conds = vec![value_cond];
        if !parts[1].is_empty() {
            let ps = builder.add_text_param(parts[1]);
            conds.push(format!("{base}->>'system' = ${ps}"));
        }
        if !parts[2].is_empty() {
            let pc = builder.add_text_param(parts[2]);
            conds.push(format!(
                "({base}->>'code' = ${pc} OR {base}->>'unit' = ${pc})"
            ));
        }
        Ok(format!("({})", conds.join(" AND ")))
    } else {
        Ok(format!("({value_cond})"))
    }
}

fn extract_prefix(value: &str) -> (&str, &str) {
    for prefix in ["ge", "le", "gt", "lt", "ne", "sa", "eb", "ap"] {
        if let Some(rest) = value.strip_prefix(prefix) {
            return (prefix, rest);
        }
    }
    ("eq", value)
}

fn prefix_to_comparator(prefix: &str) -> &'static str {
    match prefix {
        "gt" | "sa" => ">",
        "lt" | "eb" => "<",
        "ge" => ">=",
        "le" => "<=",
        "ne" => "!=",
        _ => "=",
    }
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
    fn test_expression_to_jsonb_path() {
        assert_eq!(
            expression_to_jsonb_path("Observation.code"),
            "resource->>'code'"
        );
        assert_eq!(
            expression_to_jsonb_path("Observation.value.quantity"),
            "resource->'value'->>'quantity'"
        );
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
        assert!(clause.contains("system") && clause.contains("code") && clause.contains("value"));
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
