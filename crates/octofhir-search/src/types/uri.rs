//! URI search parameter implementation.
//!
//! URI search is used for uri, url, and canonical fields and supports:
//! - Default: exact match
//! - :below modifier: hierarchical below (URI starts with value)
//! - :above modifier: hierarchical above (value starts with URI)
//! - :missing modifier: check if URI is present or absent

use crate::parameters::SearchModifier;
use crate::parser::ParsedParam;
use crate::sql_builder::{SqlBuilder, SqlBuilderError};

/// Build SQL conditions for URI search.
///
/// URI parameters match uri/url/canonical elements. By default, exact matching is used.
/// The :below and :above modifiers enable hierarchical matching.
pub fn build_uri_search(
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
                // Exact match
                let p = builder.add_text_param(&value.raw);
                format!("{jsonb_path} = ${p}")
            }

            Some(SearchModifier::Below) => {
                // Hierarchical below - stored URI starts with search value
                // e.g., search for "http://example.org/fhir" matches "http://example.org/fhir/Patient"
                let escaped = escape_like_pattern(&value.raw);
                let p = builder.add_text_param(format!("{escaped}%"));
                format!("{jsonb_path} LIKE ${p}")
            }

            Some(SearchModifier::Above) => {
                // Hierarchical above - search value starts with stored URI
                // e.g., search for "http://example.org/fhir/Patient/123" matches "http://example.org/fhir"
                // This requires checking if the stored URI is a prefix of the search value
                let p = builder.add_text_param(&value.raw);
                format!("${p} LIKE {jsonb_path} || '%'")
            }

            Some(SearchModifier::Missing) => {
                let is_missing = value.raw.eq_ignore_ascii_case("true");
                if is_missing {
                    format!(
                        "({jsonb_path} IS NULL OR {jsonb_path} = 'null' OR {jsonb_path} = '\"\"')"
                    )
                } else {
                    format!(
                        "({jsonb_path} IS NOT NULL AND {jsonb_path} != 'null' AND {jsonb_path} != '\"\"')"
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

/// Build URI search for an array of URIs (e.g., meta.profile).
pub fn build_uri_array_search(
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
                // Exact match in array
                let p = builder.add_text_param(&value.raw);
                format!(
                    "EXISTS (SELECT 1 FROM jsonb_array_elements_text({array_path}) AS uri WHERE uri = ${p})"
                )
            }

            Some(SearchModifier::Below) => {
                // Hierarchical below in array
                let escaped = escape_like_pattern(&value.raw);
                let p = builder.add_text_param(format!("{escaped}%"));
                format!(
                    "EXISTS (SELECT 1 FROM jsonb_array_elements_text({array_path}) AS uri WHERE uri LIKE ${p})"
                )
            }

            Some(SearchModifier::Above) => {
                // Hierarchical above in array
                let p = builder.add_text_param(&value.raw);
                format!(
                    "EXISTS (SELECT 1 FROM jsonb_array_elements_text({array_path}) AS uri WHERE ${p} LIKE uri || '%')"
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

/// Escape special characters in LIKE patterns.
fn escape_like_pattern(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
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
    fn test_uri_exact_search() {
        let mut builder = SqlBuilder::new();
        let param = make_param("url", "http://example.org/fhir/Patient", None);

        build_uri_search(&mut builder, &param, "resource->>'url'").unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert!(clause.contains("= $1"));
        assert_eq!(
            builder.params()[0].as_str(),
            "http://example.org/fhir/Patient"
        );
    }

    #[test]
    fn test_uri_below_search() {
        let mut builder = SqlBuilder::new();
        let param = make_param(
            "url",
            "http://example.org/fhir",
            Some(SearchModifier::Below),
        );

        build_uri_search(&mut builder, &param, "resource->>'url'").unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert!(clause.contains("LIKE"));
        assert!(builder.params()[0].as_str().ends_with('%'));
    }

    #[test]
    fn test_uri_above_search() {
        let mut builder = SqlBuilder::new();
        let param = make_param(
            "url",
            "http://example.org/fhir/Patient/123",
            Some(SearchModifier::Above),
        );

        build_uri_search(&mut builder, &param, "resource->>'url'").unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert!(clause.contains("LIKE"));
        assert!(clause.contains("|| '%'"));
    }

    #[test]
    fn test_uri_missing() {
        let mut builder = SqlBuilder::new();
        let param = make_param("url", "true", Some(SearchModifier::Missing));

        build_uri_search(&mut builder, &param, "resource->>'url'").unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert!(clause.contains("IS NULL"));
    }

    #[test]
    fn test_uri_array_search() {
        let mut builder = SqlBuilder::new();
        let param = make_param("_profile", "http://hl7.org/fhir/us/core/Patient", None);

        build_uri_array_search(&mut builder, &param, "resource->'meta'->'profile'").unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert!(clause.contains("jsonb_array_elements_text"));
    }

    #[test]
    fn test_uri_escapes_special_chars() {
        let mut builder = SqlBuilder::new();
        let param = make_param(
            "url",
            "http://example.org/100%",
            Some(SearchModifier::Below),
        );

        build_uri_search(&mut builder, &param, "resource->>'url'").unwrap();

        // The % in the URL should be escaped
        assert!(builder.params()[0].as_str().contains("\\%"));
    }

    #[test]
    fn test_invalid_modifier_returns_error() {
        let mut builder = SqlBuilder::new();
        let param = make_param("url", "http://example.org", Some(SearchModifier::Exact));

        let result = build_uri_search(&mut builder, &param, "resource->>'url'");
        assert!(result.is_err());
    }
}
