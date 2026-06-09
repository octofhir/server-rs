//! String search parameter implementation.
//!
//! Per FHIR R4 §3.1.1.5.6 (https://hl7.org/fhir/R4/search.html#string):
//! - (default): starts-with, case-insensitive AND accent-insensitive
//! - :exact: exact, case-sensitive, accent-sensitive, full-string equality
//! - :contains: substring, case-insensitive AND accent-insensitive
//! - :text: full-text search on narrative (server-defined)
//!
//! Implementation:
//! - The query value is normalized in Rust (lowercase + NFD strip combining
//!   marks) via `octofhir_core::normalize_string` before being bound.
//! - The indexed/JSONB side is normalized in SQL with `f_unaccent_lower(...)`
//!   (defined in the consolidated schema migration) so the comparison is
//!   symmetric.
//! - :exact bypasses both: raw equality.

use crate::parser::ParsedParam;
use crate::sql_builder::{SqlBuilder, SqlBuilderError};
use crate::{
    ir::StringClause, ir::render_string_array_clauses_as_or, ir::render_string_clauses_as_or,
    ir::render_string_human_name_clauses_as_or, ir::render_string_path_clauses_as_or,
};

/// Build SQL conditions for string search against the `search_idx_string`
/// sidecar. One row per extracted value (HumanName parts, Address parts,
/// repeating extension strings, …), normalised to `value_norm` for
/// case+accent-insensitive matching and `value_exact` for `:exact`.
///
/// Covered modifiers:
/// - default → `value_norm LIKE 'q%'` (starts-with) via trigram GIN
/// - `:contains` → `value_norm LIKE '%q%'` via trigram GIN
/// - `:exact` → `value_exact = q` via btree
/// - `:missing=true|false` → `NOT EXISTS` / `EXISTS` over the sidecar
///
/// `:text` falls through to the legacy JSONB path because the sidecar does
/// not store the resource narrative.
pub fn build_indexed_string_search(
    builder: &mut SqlBuilder,
    param: &ParsedParam,
    resource_type: &str,
) -> Result<(), SqlBuilderError> {
    let clauses = StringClause::from_parsed_param(param, resource_type)?;
    if let Some(sql) = render_string_clauses_as_or(builder, &clauses) {
        builder.add_condition(sql);
    }
    Ok(())
}

/// Build SQL conditions for string search.
///
/// String parameters are compared using case-insensitive matching by default.
/// The default behavior is starts-with matching.
pub fn build_string_search(
    builder: &mut SqlBuilder,
    param: &ParsedParam,
    jsonb_path: &str,
) -> Result<(), SqlBuilderError> {
    let clauses = StringClause::from_parsed_param(param, "")?;
    if let Some(sql) = render_string_path_clauses_as_or(builder, &clauses, jsonb_path) {
        builder.add_condition(sql);
    }
    Ok(())
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
    let clauses = StringClause::from_parsed_param(param, "")?;
    if let Some(sql) = render_string_array_clauses_as_or(builder, &clauses, array_path, field_name)
    {
        builder.add_condition(sql);
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
    let clauses = StringClause::from_parsed_param(param, "")?;
    if let Some(sql) = render_string_human_name_clauses_as_or(builder, &clauses, array_path) {
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
    fn test_string_default_search() {
        let mut builder = SqlBuilder::new();
        let param = make_param("name", "John", None);

        build_string_search(&mut builder, &param, "resource->>'name'").unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert!(clause.contains("f_unaccent_lower(resource->>'name') LIKE $1"));
        assert_eq!(builder.params()[0].as_str(), "john%");
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
        assert!(clause.contains("f_unaccent_lower(resource->>'name') LIKE $1"));
        assert_eq!(builder.params()[0].as_str(), "%ohn%");
    }

    #[test]
    fn test_string_escapes_special_chars() {
        let mut builder = SqlBuilder::new();
        let param = make_param("name", "100%", None);

        build_string_search(&mut builder, &param, "resource->>'name'").unwrap();

        // The % is escaped after Rust-side normalize (lowercase + accent strip).
        // "100%" normalizes to "100%" → escaped to "100\%" → suffix "%" appended.
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

    #[test]
    fn test_indexed_string_default_uses_sidecar_prefix() {
        let mut builder = SqlBuilder::new();
        let param = make_param("family", "Smíth", None);

        build_indexed_string_search(&mut builder, &param, "Patient").unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert!(clause.contains("search_idx_string"));
        assert!(clause.contains("sid.value_norm LIKE"));
        assert_eq!(builder.params()[2].as_str(), "smith%");
        assert!(!clause.contains("Smíth"));
    }

    #[test]
    fn test_indexed_string_contains_escapes_like_pattern() {
        let mut builder = SqlBuilder::new();
        let param = make_param("family", "Sm_th%", Some(SearchModifier::Contains));

        build_indexed_string_search(&mut builder, &param, "Patient").unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert!(clause.contains("sid.value_norm LIKE"));
        assert_eq!(builder.params()[2].as_str(), "%sm\\_th\\%%");
    }

    #[test]
    fn test_indexed_string_prefix_preserves_spaces() {
        let mut builder = SqlBuilder::new();
        let param = make_param("family", "Van Hel", None);

        build_indexed_string_search(&mut builder, &param, "Patient").unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert!(clause.contains("sid.value_norm LIKE"));
        assert_eq!(builder.params()[2].as_str(), "van hel%");
    }

    #[test]
    fn test_indexed_string_exact_uses_sidecar_btree_value() {
        let mut builder = SqlBuilder::new();
        let param = make_param("family", "Smíth", Some(SearchModifier::Exact));

        build_indexed_string_search(&mut builder, &param, "Patient").unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert!(clause.contains("sid.value_exact ="));
        assert_eq!(builder.params()[2].as_str(), "Smíth");
    }

    #[test]
    fn test_indexed_string_missing_uses_sidecar_exists() {
        let mut builder = SqlBuilder::new();
        let param = make_param("family", "true", Some(SearchModifier::Missing));

        build_indexed_string_search(&mut builder, &param, "Patient").unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert!(clause.contains("NOT EXISTS"));
        assert!(clause.contains("search_idx_string"));
        assert_eq!(builder.param_count(), 2);
    }
}
