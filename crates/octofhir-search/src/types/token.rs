//! Token search parameter implementation.
//!
//! Token search is used for coded elements (CodeableConcept, Coding, Identifier, code, etc.)
//! and supports the following modifiers:
//! - (default): match code, optionally with system
//! - :not: negation
//! - :text: search on display text
//! - :in: value set membership (requires terminology provider)
//! - :not-in: value set exclusion (requires terminology provider)
//! - :below: subsumption - descendants (requires terminology provider)
//! - :above: subsumption - ancestors (requires terminology provider)
//! - :of-type: identifier type filtering

use crate::ir::{
    render_token_identifier_clauses_as_or, render_token_path_clauses_as_or,
    render_token_scalar_code_clauses_as_or,
};
#[cfg(test)]
use crate::parameters::SearchModifier;
use crate::parser::ParsedParam;
use crate::sql_builder::{SqlBuilder, SqlBuilderError, build_jsonb_accessor};
use crate::{
    ir::TokenClause, ir::TokenIndexShape, ir::render_token_coding_array_clauses_as_or,
    ir::render_token_coding_clauses_as_or, ir::render_token_coding_subtree_clauses_as_or,
    ir::render_token_identifier_containment_clauses_as_or,
    ir::render_token_simple_code_clauses_as_or,
};

/// Parse a token value into system and code parts.
///
/// Token values can be in the following formats:
/// - `system|code` - match both system and code
/// - `|code` - match code with no system (explicit null system)
/// - `code` - match code in any system
pub fn parse_token_value(value: &str) -> (Option<&str>, &str) {
    if let Some(pos) = value.find('|') {
        let system = &value[..pos];
        let code = &value[pos + 1..];
        if system.is_empty() {
            // |code format - explicit no system
            (Some(""), code)
        } else {
            (Some(system), code)
        }
    } else {
        // code only - any system
        (None, value)
    }
}

/// Build SQL conditions for token search.
///
/// Token parameters match coded values. The format system|code is supported,
/// as well as code-only matching.
pub fn build_token_search(
    builder: &mut SqlBuilder,
    param: &ParsedParam,
    jsonb_path: &str,
) -> Result<(), SqlBuilderError> {
    let clauses = TokenClause::from_parsed_param(param, "", TokenIndexShape::Coding)?;
    if let Some(sql) = render_token_path_clauses_as_or(builder, &clauses, jsonb_path)? {
        builder.add_condition(sql);
    }
    Ok(())
}

/// Build token search for Identifier arrays.
///
/// Identifiers have system and value fields rather than coding.
pub fn build_identifier_search(
    builder: &mut SqlBuilder,
    param: &ParsedParam,
    array_path: &str,
) -> Result<(), SqlBuilderError> {
    let clauses = TokenClause::from_parsed_param(param, "", TokenIndexShape::Identifier)?;
    if let Some(sql) = render_token_identifier_clauses_as_or(builder, &clauses, array_path)? {
        builder.add_condition(sql);
    }
    Ok(())
}

/// Build GIN-optimized token search for Identifier arrays.
///
/// System/value, value-only, system-only, and :of-type forms can be expressed
/// as full-resource containment and use the generic resource GIN index. Forms
/// requiring absence checks still fall back to array traversal.
pub fn build_gin_identifier_search(
    builder: &mut SqlBuilder,
    param: &ParsedParam,
    path_segments: &[String],
) -> Result<(), SqlBuilderError> {
    let clauses = TokenClause::from_parsed_param(param, "", TokenIndexShape::Identifier)?;
    let resource_col = builder.resource_column().to_string();
    let array_path = build_jsonb_accessor(&resource_col, path_segments, false);
    if let Some(sql) = render_token_identifier_containment_clauses_as_or(
        builder,
        &clauses,
        path_segments,
        &array_path,
    )? {
        builder.add_condition(sql);
    }
    Ok(())
}

/// Build GIN-optimized token search using `resource @> '{...}'::jsonb`.
///
/// Generates containment queries that leverage the existing GIN index
/// (`jsonb_path_ops`) on the resource column. For CodeableConcept/Coding fields,
/// this produces queries like:
/// ```sql
/// resource @> '{"code": {"coding": [{"system": "http://loinc.org", "code": "8480-6"}]}}'::jsonb
/// ```
pub fn build_gin_token_search(
    builder: &mut SqlBuilder,
    param: &ParsedParam,
    path_segments: &[String],
) -> Result<(), SqlBuilderError> {
    let clauses = TokenClause::from_parsed_param(param, "", TokenIndexShape::Coding)?;
    if let Some(sql) = render_token_coding_clauses_as_or(builder, &clauses, path_segments)? {
        builder.add_condition(sql);
    }
    Ok(())
}

/// Build token search for an ARRAY-valued CodeableConcept/Coding field
/// (e.g. `Observation.category`, 0..*).
///
/// The scalar [`build_gin_token_search`] builds object-leaf containment and
/// `path->'coding'` traversal, both of which miss when `path` holds an array.
/// This routes to the array-aware render: array-wrapped `@>` containment (still
/// GIN-indexed) for system/code, outer-array iteration for `|code`/`:text`.
pub fn build_token_coding_array_search(
    builder: &mut SqlBuilder,
    param: &ParsedParam,
    array_path: &str,
) -> Result<(), SqlBuilderError> {
    let clauses = TokenClause::from_parsed_param(param, "", TokenIndexShape::Coding)?;
    if let Some(sql) = render_token_coding_array_clauses_as_or(builder, &clauses, array_path)? {
        builder.add_condition(sql);
    }
    Ok(())
}

/// Build token search for a scalar (non-array) Coding/CodeableConcept field
/// (e.g. `Encounter.class`, `Encounter.status` is SimpleCode but `class` is a Coding).
///
/// Emits subtree `@>` containment (`resource->'class' @> '{...}'`) so a dedicated
/// functional GIN on the subtree serves it, instead of the path-based scalar OR that
/// the planner can only answer with a Seq Scan.
pub fn build_token_coding_subtree_search(
    builder: &mut SqlBuilder,
    param: &ParsedParam,
    subtree_path: &str,
) -> Result<(), SqlBuilderError> {
    let clauses = TokenClause::from_parsed_param(param, "", TokenIndexShape::Coding)?;
    if let Some(sql) = render_token_coding_subtree_clauses_as_or(builder, &clauses, subtree_path)? {
        builder.add_condition(sql);
    }
    Ok(())
}

/// Build GIN-optimized search for simple code fields using `resource @> '{...}'::jsonb`.
///
/// For simple code fields like `Patient.gender`, generates:
/// ```sql
/// resource @> '{"gender": "female"}'::jsonb
/// ```
pub fn build_gin_code_search(
    builder: &mut SqlBuilder,
    param: &ParsedParam,
    path_segments: &[String],
) -> Result<(), SqlBuilderError> {
    let clauses = TokenClause::from_parsed_param(param, "", TokenIndexShape::SimpleCode)?;
    if let Some(sql) = render_token_simple_code_clauses_as_or(builder, &clauses, path_segments)? {
        builder.add_condition(sql);
    }
    Ok(())
}

/// Build a nested JSON object from path segments wrapping a leaf value.
///
/// For path `["code"]` and value `{"coding": [...]}`, produces:
/// `{"code": {"coding": [...]}}`
///
/// For path `["name", "family"]` and value `"Smith"`, produces:
/// `{"name": [{"family": "Smith"}]}`  (arrays handled by caller)
#[cfg(test)]
fn build_nested_containment(
    path_segments: &[String],
    leaf_value: serde_json::Value,
) -> serde_json::Value {
    let mut result = leaf_value;
    for segment in path_segments.iter().rev() {
        result = serde_json::json!({ segment.as_str(): result });
    }
    result
}

/// Build search for simple code fields (not CodeableConcept).
///
/// Used for fields like Patient.gender which are simple code values.
pub fn build_code_search(
    builder: &mut SqlBuilder,
    param: &ParsedParam,
    jsonb_path: &str,
) -> Result<(), SqlBuilderError> {
    let clauses = TokenClause::from_parsed_param(param, "", TokenIndexShape::SimpleCode)?;
    if let Some(sql) = render_token_scalar_code_clauses_as_or(builder, &clauses, jsonb_path)? {
        builder.add_condition(sql);
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
    fn test_parse_token_value() {
        let (sys, code) = parse_token_value("http://loinc.org|1234-5");
        assert_eq!(sys, Some("http://loinc.org"));
        assert_eq!(code, "1234-5");

        let (sys, code) = parse_token_value("|1234-5");
        assert_eq!(sys, Some(""));
        assert_eq!(code, "1234-5");

        let (sys, code) = parse_token_value("1234-5");
        assert_eq!(sys, None);
        assert_eq!(code, "1234-5");
    }

    #[test]
    fn test_token_system_and_code() {
        let mut builder = SqlBuilder::new();
        let param = make_param("code", "http://loinc.org|1234-5", None);

        build_token_search(&mut builder, &param, "resource->'code'").unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert!(clause.contains("@>"));
        assert!(!clause.contains("http://loinc.org"));
        assert_eq!(builder.params().len(), 3);
    }

    #[test]
    fn test_token_coding_array_uses_subtree_containment() {
        // Array-valued CodeableConcept (e.g. Observation.category 0..*): the predicate
        // must be `<array_path> @> '[<cc>]'` (subtree containment) so a dedicated GIN on
        // the subtree serves it — and the leaf is array-wrapped because the subtree IS an
        // array (object-leaf `{"coding":...}` never matches an array element set).
        let mut builder = SqlBuilder::new();
        let param = make_param("category", "vital-signs", None);

        build_token_coding_array_search(&mut builder, &param, "resource->'category'").unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert!(
            clause.contains("resource->'category' @>"),
            "expected subtree containment, got: {clause}"
        );
        // Array-wrapped leaf: a JSON array of CodeableConcept.
        let json: Vec<String> = builder.params().iter().map(|p| p.as_str()).collect();
        assert!(
            json.iter()
                .any(|p| p.contains("[{\"coding\":[{\"code\":\"vital-signs\"}]}]")),
            "no array-wrapped coding containment in params: {json:?}"
        );
    }

    #[test]
    fn test_token_coding_subtree_scalar_uses_subtree_containment() {
        // Scalar Coding (e.g. Encounter.class): predicate must be `<subtree> @> '{...}'`
        // (NOT array-wrapped — the subtree is an object), so a dedicated GIN on the
        // subtree serves it instead of a Seq Scan.
        let mut builder = SqlBuilder::new();
        let param = make_param("class", "AMB", None);

        build_token_coding_subtree_search(&mut builder, &param, "resource->'class'").unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert!(
            clause.contains("resource->'class' @>"),
            "expected subtree containment, got: {clause}"
        );
        let json: Vec<String> = builder.params().iter().map(|p| p.as_str()).collect();
        // Bare-Coding object leaf (not array-wrapped) plus the CodeableConcept fallback.
        assert!(
            json.iter().any(|p| p.contains("{\"code\":\"AMB\"}")),
            "no bare-Coding object containment in params: {json:?}"
        );
    }

    #[test]
    fn test_token_coding_subtree_system_code() {
        let mut builder = SqlBuilder::new();
        let param = make_param(
            "class",
            "http://terminology.hl7.org/CodeSystem/v3-ActCode|AMB",
            None,
        );

        build_token_coding_subtree_search(&mut builder, &param, "resource->'class'").unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert!(clause.contains("resource->'class' @>"), "got: {clause}");
        let json: Vec<String> = builder.params().iter().map(|p| p.as_str()).collect();
        assert!(
            json.iter().any(|p| p.contains("\"system\":\"http://terminology.hl7.org/CodeSystem/v3-ActCode\"")
                && p.contains("\"code\":\"AMB\"")),
            "no system+code containment in params: {json:?}"
        );
    }

    #[test]
    fn test_token_coding_array_no_system_iterates_outer_array() {
        // `|code` (system absent) can't use `@>` containment — must iterate the
        // outer array with jsonb_array_elements.
        let mut builder = SqlBuilder::new();
        let param = make_param("category", "|vital-signs", None);

        build_token_coding_array_search(&mut builder, &param, "resource->'category'").unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert!(
            clause.contains("jsonb_array_elements(resource->'category')"),
            "clause: {clause}"
        );
    }

    #[test]
    fn test_token_code_only() {
        let mut builder = SqlBuilder::new();
        let param = make_param("code", "1234-5", None);

        build_token_search(&mut builder, &param, "resource->'code'").unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert!(clause.contains("$1"));
    }

    #[test]
    fn test_token_not_modifier_uses_boolean_false_check() {
        let mut builder = SqlBuilder::new();
        let param = make_param("status", "active", Some(SearchModifier::Not));

        build_token_search(&mut builder, &param, "resource->'status'").unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert!(clause.starts_with("("));
        assert!(clause.ends_with("= false"));
        assert!(!clause.contains("NOT ("));
    }

    #[test]
    fn test_token_text_modifier() {
        let mut builder = SqlBuilder::new();
        let param = make_param("code", "blood pressure", Some(SearchModifier::Text));

        build_token_search(&mut builder, &param, "resource->'code'").unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert!(clause.contains("display"));
        assert!(clause.contains("LIKE"));
    }

    #[test]
    fn test_identifier_search() {
        let mut builder = SqlBuilder::new();
        let param = make_param("identifier", "http://hospital.org|12345", None);

        build_identifier_search(&mut builder, &param, "resource->'identifier'").unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert!(clause.contains("@>"));
    }

    #[test]
    fn test_code_search() {
        let mut builder = SqlBuilder::new();
        let param = make_param("gender", "female", None);

        build_code_search(&mut builder, &param, "resource->>'gender'").unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert!(clause.contains("resource->>'gender' = $1"));
    }

    #[test]
    fn test_code_search_not_modifier_uses_boolean_false_check() {
        let mut builder = SqlBuilder::new();
        let param = make_param("gender", "female", Some(SearchModifier::Not));

        build_code_search(&mut builder, &param, "resource->>'gender'").unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert_eq!(clause, "(resource->>'gender' = $1) = false");
        assert!(!clause.contains("NOT ("));
        assert_eq!(builder.params()[0].as_str(), "female");
    }

    #[test]
    fn test_token_in_modifier_not_implemented() {
        let mut builder = SqlBuilder::new();
        let param = make_param("code", "http://vs.org", Some(SearchModifier::In));

        let result = build_token_search(&mut builder, &param, "resource->'code'");
        assert!(matches!(result, Err(SqlBuilderError::NotImplemented(_))));
    }

    // Note: Tests for :in, :not-in, :below, :above modifiers with terminology provider
    // require a real PostgreSQL connection pool and are moved to integration tests.
    // These tests verified that without a terminology provider, errors are returned.
    // The sync version (build_token_search) already tests this behavior.

    #[tokio::test]
    async fn test_token_below_requires_system() {
        let mut builder = SqlBuilder::new();
        // Code without system - should fail for below/above
        let param = make_param("code", "73211009", Some(SearchModifier::Below));

        // Even without a real terminology provider, we should get an error about missing system
        // We need to provide a mock or skip the terminology check
        // For this test, we verify the sync version still fails
        let result = build_token_search(&mut builder, &param, "resource->'code'");
        assert!(matches!(result, Err(SqlBuilderError::NotImplemented(_))));
    }

    // Note: Async tests for non-terminology modifiers (default, :not) are redundant
    // with the sync version tests above. The async version is primarily for
    // terminology-requiring modifiers (:in, :not-in, :below, :above) which
    // require integration tests with a real PostgreSQL pool.

    // ========================================================================
    // GIN-optimized token search tests
    // ========================================================================

    #[test]
    fn test_gin_code_search_simple() {
        let mut builder = SqlBuilder::new();
        let param = make_param("gender", "female", None);

        build_gin_code_search(&mut builder, &param, &["gender".to_string()]).unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert!(
            clause.contains("@>"),
            "Expected @> containment, got: {clause}"
        );
        assert!(clause.contains("::jsonb"));
    }

    #[test]
    fn test_gin_code_search_not_modifier_uses_boolean_false_check() {
        let mut builder = SqlBuilder::new();
        let param = make_param("gender", "female", Some(SearchModifier::Not));

        build_gin_code_search(&mut builder, &param, &["gender".to_string()]).unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert_eq!(clause, "(resource @> $1::jsonb) = false");
        assert!(clause.contains("@>"));
        assert!(!clause.contains("NOT ("));
    }

    #[test]
    fn test_gin_token_search_system_and_code() {
        let mut builder = SqlBuilder::new();
        let param = make_param("code", "http://loinc.org|8480-6", None);

        build_gin_token_search(&mut builder, &param, &["code".to_string()]).unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert!(
            clause.contains("@>"),
            "Expected @> containment, got: {clause}"
        );
        assert!(clause.contains("::jsonb"));
    }

    #[test]
    fn test_gin_token_search_code_only() {
        let mut builder = SqlBuilder::new();
        let param = make_param("code", "8480-6", None);

        build_gin_token_search(&mut builder, &param, &["code".to_string()]).unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert!(
            clause.contains("@>"),
            "Expected @> containment, got: {clause}"
        );
    }

    #[test]
    fn test_gin_token_search_no_system_code_uses_no_system_semantics() {
        let mut builder = SqlBuilder::new();
        let param = make_param("code", "|8480-6", None);

        build_gin_token_search(&mut builder, &param, &["code".to_string()]).unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert!(
            clause.contains("system' IS NULL") && clause.contains("c->>'system' IS NULL"),
            "|code must require absent system, got: {clause}"
        );
        assert!(
            !clause.contains("@>"),
            "|code cannot be represented by broad code-only containment: {clause}"
        );
        assert_eq!(builder.params()[0].as_str(), "8480-6");
    }

    #[test]
    fn test_gin_token_search_system_any_code_uses_system_only_semantics() {
        let mut builder = SqlBuilder::new();
        let param = make_param("code", "http://loinc.org|", None);

        build_gin_token_search(&mut builder, &param, &["code".to_string()]).unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert!(
            clause.contains("c->>'system' = $1") && !clause.contains("c->>'code'"),
            "system| must match any code in that system, got: {clause}"
        );
        assert!(
            !clause.contains("@>"),
            "system| cannot be represented as containment with empty code: {clause}"
        );
        assert_eq!(builder.params()[0].as_str(), "http://loinc.org");
    }

    #[test]
    fn test_gin_token_search_not_modifier_uses_boolean_false_check() {
        let mut builder = SqlBuilder::new();
        let param = make_param("code", "http://loinc.org|8480-6", Some(SearchModifier::Not));

        build_gin_token_search(&mut builder, &param, &["code".to_string()]).unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert_eq!(clause, "(resource @> $1::jsonb) = false");
        assert!(clause.contains("@>"));
        assert!(!clause.contains("NOT ("));
    }

    #[test]
    fn test_gin_nested_containment() {
        // Verify the nested containment builder produces correct JSON
        let result = build_nested_containment(
            &["code".to_string()],
            serde_json::json!({"coding": [{"system": "http://loinc.org", "code": "8480-6"}]}),
        );
        let expected = serde_json::json!({
            "code": {"coding": [{"system": "http://loinc.org", "code": "8480-6"}]}
        });
        assert_eq!(result, expected);

        // Simple code
        let result = build_nested_containment(&["gender".to_string()], serde_json::json!("female"));
        let expected = serde_json::json!({"gender": "female"});
        assert_eq!(result, expected);
    }

    #[test]
    fn test_identifier_system_value_search() {
        // Exact test case from integration: Patient?identifier=http://test.org|debug-123
        let mut builder = SqlBuilder::new();
        let param = make_param("identifier", "http://test.org|debug-123", None);

        build_identifier_search(&mut builder, &param, "r.resource->'identifier'").unwrap();

        let clause = builder.build_where_clause().unwrap();
        // Should use @> containment for system|value
        assert!(
            clause.contains("@>"),
            "Expected @> containment, got: {clause}"
        );
        // Check the JSON param contains correct system and value
        let params = builder.params();
        assert_eq!(params.len(), 1, "Expected 1 param, got {}", params.len());
        let json_str = params[0].as_str();
        assert!(
            json_str.contains("http://test.org") && json_str.contains("debug-123"),
            "Expected JSON with system and value, got: {json_str}"
        );
    }

    #[test]
    fn test_identifier_system_any_value_search() {
        let mut builder = SqlBuilder::new();
        let param = make_param("identifier", "http://test.org|", None);

        build_identifier_search(&mut builder, &param, "r.resource->'identifier'").unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert!(
            clause.contains("ident->>'system' = $1") && !clause.contains("ident->>'value'"),
            "system| should match any identifier value in system, got: {clause}"
        );
        assert_eq!(builder.params()[0].as_str(), "http://test.org");
    }

    #[test]
    fn test_identifier_no_system_value_search() {
        let mut builder = SqlBuilder::new();
        let param = make_param("identifier", "|debug-123", None);

        build_identifier_search(&mut builder, &param, "r.resource->'identifier'").unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert!(
            clause.contains("ident->>'system' IS NULL") && clause.contains("ident->>'value' = $1"),
            "|value should require absent identifier system, got: {clause}"
        );
        assert_eq!(builder.params()[0].as_str(), "debug-123");
    }

    #[test]
    fn test_identifier_not_modifier_uses_boolean_false_check() {
        let mut builder = SqlBuilder::new();
        let param = make_param("identifier", "|debug-123", Some(SearchModifier::Not));

        build_identifier_search(&mut builder, &param, "r.resource->'identifier'").unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert!(
            clause.starts_with("(EXISTS")
                && clause.contains("ident->>'system' IS NULL")
                && clause.contains("ident->>'value' = $1")
                && clause.ends_with("= false"),
            ":not must negate the |value form, got: {clause}"
        );
        assert!(!clause.contains("NOT ("));
        assert_eq!(builder.params()[0].as_str(), "debug-123");
    }

    #[test]
    fn test_identifier_value_only_search() {
        // Test: Patient?identifier=debug-123
        let mut builder = SqlBuilder::new();
        let param = make_param("identifier", "debug-123", None);

        build_identifier_search(&mut builder, &param, "r.resource->'identifier'").unwrap();

        let clause = builder.build_where_clause().unwrap();
        // Should use EXISTS with value check
        assert!(
            clause.contains("ident->>'value' = $1"),
            "Expected value check, got: {clause}"
        );
    }

    #[test]
    fn test_gin_identifier_system_value_search() {
        let mut builder = SqlBuilder::new();
        let param = make_param("identifier", "http://test.org|debug-123", None);

        build_gin_identifier_search(&mut builder, &param, &["identifier".to_string()]).unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert_eq!(clause, "resource @> $1::jsonb");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&builder.params()[0].as_str()).unwrap(),
            serde_json::json!({
                "identifier": [{
                    "system": "http://test.org",
                    "value": "debug-123"
                }]
            })
        );
    }

    #[test]
    fn test_gin_identifier_no_system_value_keeps_absence_check() {
        let mut builder = SqlBuilder::new();
        let param = make_param("identifier", "|debug-123", None);

        build_gin_identifier_search(&mut builder, &param, &["identifier".to_string()]).unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert!(
            clause.contains("jsonb_array_elements(resource->'identifier')")
                && clause.contains("ident->>'system' IS NULL")
                && clause.contains("ident->>'value' = $1"),
            "|value should keep explicit no-system semantics, got: {clause}"
        );
        assert_eq!(builder.params()[0].as_str(), "debug-123");
    }

    #[test]
    fn test_identifier_dispatch_with_element_type_hint() {
        // Verify that identifier token search is dispatched correctly through dispatch_search
        use crate::parameters::{ElementTypeHint, SearchParameter, SearchParameterType};

        let mut builder = SqlBuilder::new();
        let param = crate::parser::ParsedParam {
            name: "identifier".to_string(),
            modifier: None,
            values: vec![crate::parser::ParsedValue {
                prefix: None,
                raw: "http://test.org|debug-123".to_string(),
            }],
        };
        let def = std::sync::Arc::new(
            SearchParameter::new(
                "identifier",
                "http://hl7.org/fhir/SearchParameter/Patient-identifier",
                SearchParameterType::Token,
                vec!["Patient".to_string()],
            )
            .with_expression("Patient.identifier")
            .with_element_type_hint(ElementTypeHint::Identifier),
        );

        crate::types::dispatch_search(&mut builder, &param, &def, "Patient").unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert_eq!(clause, "resource @> $1::jsonb");
        let params = builder.params();
        let json_str = params[0].as_str();
        assert!(
            json_str.contains("\"identifier\"")
                && json_str.contains("\"value\"")
                && json_str.contains("\"system\""),
            "Expected identifier JSON with system/value, got: {json_str}"
        );
        assert!(
            !json_str.contains("coding"),
            "Should NOT contain 'coding' for identifier search, got: {json_str}"
        );
    }
}
