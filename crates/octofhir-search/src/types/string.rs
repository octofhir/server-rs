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
    ir::StringClause, ir::render_indexed_string_clauses_as_or,
    ir::render_string_array_clauses_as_or, ir::render_string_human_name_clauses_as_or,
    ir::render_string_path_clauses_as_or,
};

/// In-place string search on the resource JSONB (no sidecar): predicates run over
/// `fhir_text_blob(fhir_extract_text(col,paths))` (trigram GIN) and the raw value
/// array `fhir_extract_text(col,paths)` (`:exact` / `:missing`). `paths` expand by
/// element type (HumanName -> family/given/prefix/suffix/text; scalar -> as-is).
pub fn build_indexed_string_inplace(
    builder: &mut SqlBuilder,
    param: &ParsedParam,
    resource_type: &str,
    definition: &crate::parameters::SearchParameter,
) -> Result<(), SqlBuilderError> {
    let expression = definition.expression.as_deref().unwrap_or_default();
    let segments = crate::sql_builder::fhirpath_to_jsonb_path(expression, resource_type);
    let paths_json = crate::sql_builder::paths_to_json(&crate::sql_builder::extraction_paths(
        &segments,
        &definition.element_type_hint,
    ));
    let col = builder.resource_column();
    // Indexed params with a per-param TYPE-AWARE extraction function use it directly
    // (matches the functional GIN index); everything else uses the generic
    // lax-jsonpath extraction.
    let (arr_expr, blob_expr) = if let Some(fn_name) = definition.typed_extract_fn.as_deref() {
        let arr_expr = format!("{fn_name}({col})");
        let blob_expr = format!("fhir_text_blob({arr_expr})");
        (arr_expr, blob_expr)
    } else {
        let arr_expr = format!("fhir_extract_text({col}, '{paths_json}'::jsonb)");
        let blob_expr = format!("fhir_text_blob({arr_expr})");
        (arr_expr, blob_expr)
    };

    let clauses = StringClause::from_parsed_param(param, resource_type)?;
    if let Some(sql) = render_indexed_string_clauses_as_or(builder, &clauses, &blob_expr, &arr_expr)
    {
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

    fn make_string_definition(typed_fn: Option<&str>) -> crate::parameters::SearchParameter {
        use crate::parameters::{ElementTypeHint, SearchParameter, SearchParameterType};
        let mut def = SearchParameter::new(
            "name",
            "http://example.org/SearchParameter/Patient-name",
            SearchParameterType::String,
            vec!["Patient".to_string()],
        )
        .with_expression("Patient.name")
        .with_element_type_hint(ElementTypeHint::HumanName);
        if let Some(name) = typed_fn {
            def = def.with_typed_extract_fn(name);
        }
        def
    }

    #[test]
    fn test_indexed_string_with_typed_extract_fn_default() {
        let mut builder = SqlBuilder::new();
        let param = make_param("name", "John", None);
        let def = make_string_definition(Some("fhir_s_patient_name"));

        build_indexed_string_inplace(&mut builder, &param, "Patient", &def).unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert!(
            clause.contains("fhir_text_blob(fhir_s_patient_name("),
            "clause: {clause}"
        );
        assert!(!clause.contains("fhir_extract_text"), "clause: {clause}");
    }

    #[test]
    fn test_indexed_string_with_typed_extract_fn_exact() {
        let mut builder = SqlBuilder::new();
        let param = make_param("name", "John Doe", Some(SearchModifier::Exact));
        let def = make_string_definition(Some("fhir_s_patient_name"));

        build_indexed_string_inplace(&mut builder, &param, "Patient", &def).unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert!(
            clause.contains("= ANY(fhir_s_patient_name("),
            "clause: {clause}"
        );
        assert!(!clause.contains("fhir_extract_text"), "clause: {clause}");
    }

    #[test]
    fn test_indexed_string_without_typed_extract_fn_uses_generic() {
        let mut builder = SqlBuilder::new();
        let param = make_param("name", "John", None);
        let def = make_string_definition(None);

        build_indexed_string_inplace(&mut builder, &param, "Patient", &def).unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert!(
            clause.contains("fhir_extract_text(resource,"),
            "clause: {clause}"
        );
        assert!(!clause.contains("fhir_s_patient_name"), "clause: {clause}");
    }
}
