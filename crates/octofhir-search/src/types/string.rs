//! String search parameter implementation.
//!
//! String search supports the following modifiers:
//! - (default): starts-with, case-insensitive
//! - :exact: exact match, case-sensitive
//! - :contains: contains, case-insensitive
//! - :text: full-text search on narrative

use crate::parameters::SearchModifier;
use crate::parser::ParsedParam;
use crate::sql_builder::{SqlBuilder, SqlBuilderError};

/// Build SQL conditions for string search.
///
/// String parameters are compared using case-insensitive matching by default.
/// The default behavior is starts-with matching.
pub fn build_string_search(
    builder: &mut SqlBuilder,
    param: &ParsedParam,
    jsonb_path: &str,
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
                // Default: starts-with, case-insensitive
                let escaped = escape_like_pattern(&value.raw);
                let p = builder.add_text_param(format!("{escaped}%"));
                format!("LOWER({jsonb_path}) LIKE LOWER(${p})")
            }

            Some(SearchModifier::Exact) => {
                // Exact match, case-sensitive
                let p = builder.add_text_param(&value.raw);
                format!("{jsonb_path} = ${p}")
            }

            Some(SearchModifier::Contains) => {
                // Contains, case-insensitive
                let escaped = escape_like_pattern(&value.raw);
                let p = builder.add_text_param(format!("%{escaped}%"));
                format!("LOWER({jsonb_path}) LIKE LOWER(${p})")
            }

            Some(SearchModifier::Text) => {
                // Full-text search - search in text field
                // This searches the narrative text of the resource
                let resource_col = builder.resource_column().to_string();
                let p = builder.add_text_param(&value.raw);
                format!(
                    "to_tsvector('english', {resource_col}->>'text') @@ plainto_tsquery('english', ${p})"
                )
            }

            Some(SearchModifier::Missing) => {
                // Missing modifier: check if field is null or absent
                let is_missing = value.raw.eq_ignore_ascii_case("true");
                if is_missing {
                    format!("({jsonb_path} IS NULL OR {jsonb_path} = 'null')")
                } else {
                    format!("({jsonb_path} IS NOT NULL AND {jsonb_path} != 'null')")
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

/// Escape special characters in LIKE patterns.
fn escape_like_pattern(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

/// Build a JSONB path for string search that handles arrays.
///
/// For HumanName fields, we need to search across array elements.
/// This function generates appropriate SQL for array-based string fields.
pub fn build_array_string_search(
    builder: &mut SqlBuilder,
    param: &ParsedParam,
    array_path: &str,
    field_name: &str,
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
                // Default: starts-with, case-insensitive
                // Use EXISTS to search across array elements.
                // Also handle nested string arrays (e.g., name[].given[] where given is an array of strings)
                // by checking both scalar and array extraction. Guard with jsonb_typeof to avoid
                // "cannot extract elements from a scalar" errors.
                let escaped = escape_like_pattern(&value.raw);
                let p = builder.add_text_param(format!("{escaped}%"));
                format!(
                    "EXISTS (SELECT 1 FROM jsonb_array_elements({array_path}) AS elem WHERE \
                     LOWER(elem->>'{field_name}') LIKE LOWER(${p}) OR \
                     (jsonb_typeof(elem->'{field_name}') = 'array' AND \
                      EXISTS (SELECT 1 FROM jsonb_array_elements_text(elem->'{field_name}') AS sub \
                      WHERE LOWER(sub) LIKE LOWER(${p}))))"
                )
            }

            Some(SearchModifier::Exact) => {
                // Exact match across array elements (scalar or nested array)
                let p = builder.add_text_param(&value.raw);
                format!(
                    "EXISTS (SELECT 1 FROM jsonb_array_elements({array_path}) AS elem WHERE \
                     elem->>'{field_name}' = ${p} OR \
                     (jsonb_typeof(elem->'{field_name}') = 'array' AND \
                      EXISTS (SELECT 1 FROM jsonb_array_elements_text(elem->'{field_name}') AS sub \
                      WHERE sub = ${p})))"
                )
            }

            Some(SearchModifier::Contains) => {
                // Contains, case-insensitive across array elements (scalar or nested array)
                let escaped = escape_like_pattern(&value.raw);
                let p = builder.add_text_param(format!("%{escaped}%"));
                format!(
                    "EXISTS (SELECT 1 FROM jsonb_array_elements({array_path}) AS elem WHERE \
                     LOWER(elem->>'{field_name}') LIKE LOWER(${p}) OR \
                     (jsonb_typeof(elem->'{field_name}') = 'array' AND \
                      EXISTS (SELECT 1 FROM jsonb_array_elements_text(elem->'{field_name}') AS sub \
                      WHERE LOWER(sub) LIKE LOWER(${p}))))"
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

/// Build search for HumanName type which has multiple sub-fields.
///
/// Searches across family, given (array), and text fields.
pub fn build_human_name_search(
    builder: &mut SqlBuilder,
    param: &ParsedParam,
    array_path: &str,
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
                // Default: starts-with, case-insensitive
                // Search in family, given[], and text
                let escaped = escape_like_pattern(&value.raw);
                let p = builder.add_text_param(format!("{escaped}%"));
                format!(
                    "EXISTS (SELECT 1 FROM jsonb_array_elements({array_path}) AS name WHERE \
                     LOWER(name->>'family') LIKE LOWER(${p}) OR \
                     LOWER(name->>'text') LIKE LOWER(${p}) OR \
                     EXISTS (SELECT 1 FROM jsonb_array_elements_text(COALESCE(name->'given', '[]'::jsonb)) AS g WHERE LOWER(g) LIKE LOWER(${p})))"
                )
            }

            Some(SearchModifier::Exact) => {
                let p = builder.add_text_param(&value.raw);
                format!(
                    "EXISTS (SELECT 1 FROM jsonb_array_elements({array_path}) AS name WHERE \
                     name->>'family' = ${p} OR \
                     name->>'text' = ${p} OR \
                     EXISTS (SELECT 1 FROM jsonb_array_elements_text(COALESCE(name->'given', '[]'::jsonb)) AS g WHERE g = ${p}))"
                )
            }

            Some(SearchModifier::Contains) => {
                let escaped = escape_like_pattern(&value.raw);
                let p = builder.add_text_param(format!("%{escaped}%"));
                format!(
                    "EXISTS (SELECT 1 FROM jsonb_array_elements({array_path}) AS name WHERE \
                     LOWER(name->>'family') LIKE LOWER(${p}) OR \
                     LOWER(name->>'text') LIKE LOWER(${p}) OR \
                     EXISTS (SELECT 1 FROM jsonb_array_elements_text(COALESCE(name->'given', '[]'::jsonb)) AS g WHERE LOWER(g) LIKE LOWER(${p})))"
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
    fn test_string_default_search() {
        let mut builder = SqlBuilder::new();
        let param = make_param("name", "John", None);

        build_string_search(&mut builder, &param, "resource->>'name'").unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert!(clause.contains("LOWER(resource->>'name') LIKE LOWER($1)"));
        assert_eq!(builder.params()[0].as_str(), "John%");
    }

    #[test]
    fn test_string_exact_search() {
        let mut builder = SqlBuilder::new();
        let param = make_param("name", "John Doe", Some(SearchModifier::Exact));

        build_string_search(&mut builder, &param, "resource->>'name'").unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert!(clause.contains("resource->>'name' = $1"));
        assert_eq!(builder.params()[0].as_str(), "John Doe");
    }

    #[test]
    fn test_string_contains_search() {
        let mut builder = SqlBuilder::new();
        let param = make_param("name", "ohn", Some(SearchModifier::Contains));

        build_string_search(&mut builder, &param, "resource->>'name'").unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert!(clause.contains("LOWER(resource->>'name') LIKE LOWER($1)"));
        assert_eq!(builder.params()[0].as_str(), "%ohn%");
    }

    #[test]
    fn test_string_escapes_special_chars() {
        let mut builder = SqlBuilder::new();
        let param = make_param("name", "100%", None);

        build_string_search(&mut builder, &param, "resource->>'name'").unwrap();

        // The % should be escaped
        assert_eq!(builder.params()[0].as_str(), "100\\%%");
    }

    #[test]
    fn test_string_missing_true() {
        let mut builder = SqlBuilder::new();
        let param = make_param("name", "true", Some(SearchModifier::Missing));

        build_string_search(&mut builder, &param, "resource->>'name'").unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert!(clause.contains("IS NULL"));
    }

    #[test]
    fn test_string_missing_false() {
        let mut builder = SqlBuilder::new();
        let param = make_param("name", "false", Some(SearchModifier::Missing));

        build_string_search(&mut builder, &param, "resource->>'name'").unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert!(clause.contains("IS NOT NULL"));
    }

    #[test]
    fn test_multiple_values_creates_or() {
        let mut builder = SqlBuilder::new();
        let param = ParsedParam {
            name: "name".to_string(),
            modifier: None,
            values: vec![
                ParsedValue {
                    prefix: None,
                    raw: "John".to_string(),
                },
                ParsedValue {
                    prefix: None,
                    raw: "Jane".to_string(),
                },
            ],
        };

        build_string_search(&mut builder, &param, "resource->>'name'").unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert!(clause.contains(" OR "));
        assert_eq!(builder.param_count(), 2);
    }

    #[test]
    fn test_invalid_modifier_returns_error() {
        let mut builder = SqlBuilder::new();
        let param = make_param("name", "test", Some(SearchModifier::Below));

        let result = build_string_search(&mut builder, &param, "resource->>'name'");
        assert!(result.is_err());
    }
}
