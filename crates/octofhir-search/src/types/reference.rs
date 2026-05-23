//! Reference search parameter implementation.
//!
//! Reference search uses the `search_idx_reference` denormalized index table
//! for B-tree index scans instead of runtime JSONB parsing.
//!
//! Supports:
//! - Default: match by reference (Type/id, id, or full URL) via index
//! - :identifier modifier: search by identifier via index (ref_kind=4)
//! - :Type modifier: type-specific reference search via index
//! - :missing modifier: check if reference is present or absent (uses JSONB)

use crate::parser::ParsedParam;
use crate::sql_builder::{SqlBuilder, SqlBuilderError};
use crate::{ir::ReferenceClause, ir::render_reference_clauses_as_or};

/// Build SQL conditions for reference search using the search_idx_reference table.
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
    resource_type: &str,
) -> Result<(), SqlBuilderError> {
    let clauses = ReferenceClause::from_parsed_param(param, resource_type, target_types)?;
    if let Some(sql) = render_reference_clauses_as_or(builder, &clauses, jsonb_path) {
        builder.add_condition(sql);
    }
    Ok(())
}

/// Check if a string looks like a FHIR resource type.
pub fn is_resource_type(s: &str) -> bool {
    s.chars().next().is_some_and(|c| c.is_ascii_uppercase())
        && s.chars().all(|c| c.is_ascii_alphanumeric())
}

/// Build reference search for an array of references using index table.
///
/// The index table already handles arrays — each reference in the array
/// gets its own index row, so the query is identical to single reference.
pub fn build_reference_array_search(
    builder: &mut SqlBuilder,
    param: &ParsedParam,
    array_path: &str,
    target_types: &[String],
    resource_type: &str,
) -> Result<(), SqlBuilderError> {
    // The index table already flattens arrays, so we use the same logic
    build_reference_search(builder, param, array_path, target_types, resource_type)
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
    fn test_is_resource_type() {
        assert!(is_resource_type("Patient"));
        assert!(is_resource_type("Observation"));
        assert!(!is_resource_type("patient"));
        assert!(!is_resource_type("123"));
    }

    #[test]
    fn test_reference_default_search_uses_index() {
        let mut builder = SqlBuilder::new();
        let param = make_param("subject", "Patient/123", None);

        build_reference_search(
            &mut builder,
            &param,
            "resource->'subject'",
            &["Patient".to_string()],
            "Observation",
        )
        .unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert!(clause.contains("search_idx_reference"));
        assert!(clause.contains("target_type"));
        assert!(clause.contains("target_id"));
    }

    #[test]
    fn test_reference_type_modifier_uses_index() {
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
            "Observation",
        )
        .unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert!(clause.contains("search_idx_reference"));
        assert!(clause.contains("ref_kind = 1"));
    }

    #[test]
    fn test_reference_identifier_modifier_uses_index() {
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
            "Observation",
        )
        .unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert!(clause.contains("search_idx_reference"));
        assert!(clause.contains("ref_kind = 4"));
        assert!(clause.contains("identifier_system"));
        assert!(clause.contains("identifier_value"));
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
            "Observation",
        )
        .unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert!(clause.contains("IS NULL"));
    }

    #[test]
    fn test_reference_id_only_single_target() {
        let mut builder = SqlBuilder::new();
        let param = make_param("subject", "123", None);

        build_reference_search(
            &mut builder,
            &param,
            "resource->'subject'",
            &["Patient".to_string()],
            "Observation",
        )
        .unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert!(clause.contains("search_idx_reference"));
        // Should include target_type since there's only one target
        assert!(clause.contains("target_type"));
    }

    #[test]
    fn test_reference_id_only_multiple_targets() {
        let mut builder = SqlBuilder::new();
        let param = make_param("subject", "123", None);

        build_reference_search(
            &mut builder,
            &param,
            "resource->'subject'",
            &["Patient".to_string(), "Group".to_string()],
            "Observation",
        )
        .unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert!(clause.contains("search_idx_reference"));
        // Should NOT filter by target_type when multiple targets
        assert!(!clause.contains("target_type"));
    }
}
