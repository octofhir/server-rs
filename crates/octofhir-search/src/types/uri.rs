//! URI search parameter implementation.
//!
//! URI search is used for uri, url, and canonical fields and supports:
//! - Default: exact match
//! - :below modifier: hierarchical below (URI starts with value)
//! - :above modifier: hierarchical above (value starts with URI)
//! - :missing modifier: check if URI is present or absent

use crate::parser::ParsedParam;
use crate::sql_builder::{SqlBuilder, SqlBuilderError};
use crate::{ir::UriClause, ir::render_uri_array_clauses_as_or, ir::render_uri_clauses_as_or};

/// Build SQL conditions for URI search.
///
/// URI parameters match uri/url/canonical elements. By default, exact matching is used.
/// The :below and :above modifiers enable hierarchical matching.
pub fn build_uri_search(
    builder: &mut SqlBuilder,
    param: &ParsedParam,
    jsonb_path: &str,
) -> Result<(), SqlBuilderError> {
    let clauses = UriClause::from_parsed_param(param, "")?;
    if let Some(sql) = render_uri_clauses_as_or(builder, &clauses, jsonb_path) {
        builder.add_condition(sql);
    }
    Ok(())
}

/// Build URI search for an array of URIs (e.g., meta.profile).
pub fn build_uri_array_search(
    builder: &mut SqlBuilder,
    param: &ParsedParam,
    array_path: &str,
) -> Result<(), SqlBuilderError> {
    let clauses = UriClause::from_parsed_param(param, "")?;
    if let Some(sql) = render_uri_array_clauses_as_or(builder, &clauses, array_path) {
        builder.add_condition(sql);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parameters::SearchModifier;
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
