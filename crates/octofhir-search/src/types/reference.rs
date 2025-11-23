//! Reference search parameter implementation.
//!
//! Reference search is used for reference fields (Reference type) and supports:
//! - Default: match by reference string (Type/id, id, or full URL)
//! - :identifier modifier: search by identifier within the reference
//! - :Type modifier: type-specific reference search (e.g., subject:Patient=123)
//! - :missing modifier: check if reference is present or absent

use crate::parameters::SearchModifier;
use crate::parser::ParsedParam;
use crate::sql_builder::{SqlBuilder, SqlBuilderError};

/// Build SQL conditions for reference search.
///
/// Reference parameters match Reference elements. The value can be:
/// - A full reference: "Patient/123"
/// - An ID only: "123" (requires single target type or type modifier)
/// - A full URL: "http://example.org/fhir/Patient/123"
pub fn build_reference_search(
    builder: &mut SqlBuilder,
    param: &ParsedParam,
    jsonb_path: &str,
    target_types: &[String],
) -> Result<(), SqlBuilderError> {
    if param.values.is_empty() {
        return Ok(());
    }

    let mut or_conditions = Vec::new();

    for value in &param.values {
        if value.raw.is_empty() {
            continue;
        }

        let condition = match &param.modifier {
            None => {
                // Default: match reference string
                let ref_value = normalize_reference(&value.raw, target_types);
                build_default_reference_condition(builder, jsonb_path, &ref_value)
            }

            Some(SearchModifier::Identifier) => {
                // Search by identifier within the reference
                build_identifier_reference_condition(builder, jsonb_path, &value.raw)
            }

            Some(SearchModifier::Type(type_name)) => {
                // Type-specific reference: subject:Patient=123
                let full_ref = format!("{type_name}/{}", value.raw);
                let p = builder.add_text_param(&full_ref);
                format!("{jsonb_path}->>'reference' = ${p}")
            }

            Some(SearchModifier::Missing) => {
                let is_missing = value.raw.eq_ignore_ascii_case("true");
                if is_missing {
                    format!(
                        "({jsonb_path} IS NULL OR {jsonb_path} = 'null' OR {jsonb_path}->>'reference' IS NULL)"
                    )
                } else {
                    format!(
                        "({jsonb_path} IS NOT NULL AND {jsonb_path} != 'null' AND {jsonb_path}->>'reference' IS NOT NULL)"
                    )
                }
            }

            Some(other) => {
                return Err(SqlBuilderError::InvalidModifier(format!("{other:?}")));
            }
        };

        or_conditions.push(condition);
    }

    if !or_conditions.is_empty() {
        builder.add_condition(SqlBuilder::build_or_clause(&or_conditions));
    }

    Ok(())
}

/// Build default reference matching condition.
fn build_default_reference_condition(
    builder: &mut SqlBuilder,
    jsonb_path: &str,
    ref_value: &str,
) -> String {
    // Reference can be stored as:
    // 1. { "reference": "Patient/123" }
    // 2. { "reference": "http://example.org/fhir/Patient/123" }
    // We need to match both relative and absolute forms

    let p = builder.add_text_param(ref_value);

    if ref_value.starts_with("http://") || ref_value.starts_with("https://") {
        // Full URL - exact match only
        format!("{jsonb_path}->>'reference' = ${p}")
    } else {
        // Relative reference - could be stored as relative or absolute
        // Also handle the case where the reference ends with the value
        format!(
            "({jsonb_path}->>'reference' = ${p} OR {jsonb_path}->>'reference' LIKE '%/' || ${p})"
        )
    }
}

/// Build identifier-based reference condition.
fn build_identifier_reference_condition(
    builder: &mut SqlBuilder,
    jsonb_path: &str,
    value: &str,
) -> String {
    // Parse system|value format
    let (system, id_value) = parse_identifier_value(value);

    match system {
        Some(sys) if !sys.is_empty() => {
            // system|value - match both
            let json = serde_json::json!({"system": sys, "value": id_value}).to_string();
            let p = builder.add_json_param(&json);
            format!("{jsonb_path}->'identifier' @> ${p}::jsonb")
        }
        Some(_) => {
            // |value - value with no system
            let p = builder.add_text_param(id_value);
            format!(
                "({jsonb_path}->'identifier'->>'system' IS NULL AND {jsonb_path}->'identifier'->>'value' = ${p})"
            )
        }
        None => {
            // value only - match any system
            let p = builder.add_text_param(id_value);
            format!("{jsonb_path}->'identifier'->>'value' = ${p}")
        }
    }
}

/// Parse identifier value into system and value parts.
fn parse_identifier_value(value: &str) -> (Option<&str>, &str) {
    if let Some(pos) = value.find('|') {
        let system = &value[..pos];
        let id_value = &value[pos + 1..];
        if system.is_empty() {
            (Some(""), id_value)
        } else {
            (Some(system), id_value)
        }
    } else {
        (None, value)
    }
}

/// Normalize a reference value based on target types.
///
/// If the value is already a full reference (Type/id or URL), return as-is.
/// If there's exactly one target type and value is just an ID, prefix with the type.
fn normalize_reference(value: &str, targets: &[String]) -> String {
    // If already a full reference (Type/id), return as-is
    if value.contains('/') {
        return value.to_string();
    }

    // If value looks like a URL, return as-is
    if value.starts_with("http://") || value.starts_with("https://") {
        return value.to_string();
    }

    // If there's exactly one target type, prefix with it
    if targets.len() == 1 {
        return format!("{}/{value}", targets[0]);
    }

    // Otherwise, return as-is and let the query handle it
    value.to_string()
}

/// Check if a string looks like a FHIR resource type.
pub fn is_resource_type(s: &str) -> bool {
    // Resource types start with uppercase letter
    s.chars().next().is_some_and(|c| c.is_ascii_uppercase())
        && s.chars().all(|c| c.is_ascii_alphanumeric())
}

/// Build reference search for an array of references.
pub fn build_reference_array_search(
    builder: &mut SqlBuilder,
    param: &ParsedParam,
    array_path: &str,
    target_types: &[String],
) -> Result<(), SqlBuilderError> {
    if param.values.is_empty() {
        return Ok(());
    }

    let mut or_conditions = Vec::new();

    for value in &param.values {
        if value.raw.is_empty() {
            continue;
        }

        let condition = match &param.modifier {
            None => {
                let ref_value = normalize_reference(&value.raw, target_types);
                let p = builder.add_text_param(&ref_value);
                format!(
                    "EXISTS (SELECT 1 FROM jsonb_array_elements({array_path}) AS ref \
                     WHERE ref->>'reference' = ${p} OR ref->>'reference' LIKE '%/' || ${p})"
                )
            }

            Some(SearchModifier::Type(type_name)) => {
                let full_ref = format!("{type_name}/{}", value.raw);
                let p = builder.add_text_param(&full_ref);
                format!(
                    "EXISTS (SELECT 1 FROM jsonb_array_elements({array_path}) AS ref \
                     WHERE ref->>'reference' = ${p})"
                )
            }

            Some(SearchModifier::Missing) => {
                let is_missing = value.raw.eq_ignore_ascii_case("true");
                if is_missing {
                    format!("({array_path} IS NULL OR jsonb_array_length({array_path}) = 0)")
                } else {
                    format!("({array_path} IS NOT NULL AND jsonb_array_length({array_path}) > 0)")
                }
            }

            Some(other) => {
                return Err(SqlBuilderError::InvalidModifier(format!("{other:?}")));
            }
        };

        or_conditions.push(condition);
    }

    if !or_conditions.is_empty() {
        builder.add_condition(SqlBuilder::build_or_clause(&or_conditions));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::ParsedValue;

    fn make_param(name: &str, value: &str, modifier: Option<SearchModifier>) -> ParsedParam {
        ParsedParam {
            name: name.to_string(),
            modifier,
            values: vec![ParsedValue {
                prefix: None,
                raw: value.to_string(),
            }],
        }
    }

    #[test]
    fn test_normalize_reference_with_type() {
        let result = normalize_reference("123", &["Patient".to_string()]);
        assert_eq!(result, "Patient/123");
    }

    #[test]
    fn test_normalize_reference_already_full() {
        let result = normalize_reference("Patient/123", &["Patient".to_string()]);
        assert_eq!(result, "Patient/123");
    }

    #[test]
    fn test_normalize_reference_multiple_targets() {
        let result =
            normalize_reference("123", &["Patient".to_string(), "Practitioner".to_string()]);
        assert_eq!(result, "123");
    }

    #[test]
    fn test_normalize_reference_url() {
        let result = normalize_reference(
            "http://example.org/fhir/Patient/123",
            &["Patient".to_string()],
        );
        assert_eq!(result, "http://example.org/fhir/Patient/123");
    }

    #[test]
    fn test_is_resource_type() {
        assert!(is_resource_type("Patient"));
        assert!(is_resource_type("Observation"));
        assert!(!is_resource_type("patient"));
        assert!(!is_resource_type("123"));
    }

    #[test]
    fn test_reference_default_search() {
        let mut builder = SqlBuilder::new();
        let param = make_param("subject", "Patient/123", None);

        build_reference_search(
            &mut builder,
            &param,
            "resource->'subject'",
            &["Patient".to_string()],
        )
        .unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert!(clause.contains("reference"));
    }

    #[test]
    fn test_reference_type_modifier() {
        let mut builder = SqlBuilder::new();
        let param = make_param(
            "subject",
            "123",
            Some(SearchModifier::Type("Patient".to_string())),
        );

        build_reference_search(
            &mut builder,
            &param,
            "resource->'subject'",
            &["Patient".to_string(), "Group".to_string()],
        )
        .unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert!(clause.contains("$1"));
        // The parameterized value should contain "Patient/123"
        assert_eq!(builder.params()[0].as_str(), "Patient/123");
    }

    #[test]
    fn test_reference_identifier_modifier() {
        let mut builder = SqlBuilder::new();
        let param = make_param(
            "subject",
            "http://hospital.org|MRN123",
            Some(SearchModifier::Identifier),
        );

        build_reference_search(
            &mut builder,
            &param,
            "resource->'subject'",
            &["Patient".to_string()],
        )
        .unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert!(clause.contains("identifier"));
    }

    #[test]
    fn test_reference_missing() {
        let mut builder = SqlBuilder::new();
        let param = make_param("subject", "true", Some(SearchModifier::Missing));

        build_reference_search(
            &mut builder,
            &param,
            "resource->'subject'",
            &["Patient".to_string()],
        )
        .unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert!(clause.contains("IS NULL"));
    }

    #[test]
    fn test_parse_identifier_value() {
        let (sys, val) = parse_identifier_value("http://sys|123");
        assert_eq!(sys, Some("http://sys"));
        assert_eq!(val, "123");

        let (sys, val) = parse_identifier_value("|123");
        assert_eq!(sys, Some(""));
        assert_eq!(val, "123");

        let (sys, val) = parse_identifier_value("123");
        assert_eq!(sys, None);
        assert_eq!(val, "123");
    }
}
