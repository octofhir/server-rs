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

use crate::parameters::SearchModifier;
use crate::parser::ParsedParam;
use crate::sql_builder::{SqlBuilder, SqlBuilderError};

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
                // Default: match reference via index table OR direct JSONB match.
                // The direct JSONB fallback covers cases where the
                // search_idx_reference index has not been populated for this
                // resource (e.g., index extractor not run after insert).
                let index_cond = build_index_reference_condition(
                    builder,
                    resource_type,
                    &param.name,
                    &value.raw,
                    target_types,
                );
                let jsonb_cond =
                    build_jsonb_reference_condition(builder, jsonb_path, &value.raw, target_types);
                format!("({index_cond} OR {jsonb_cond})")
            }

            Some(SearchModifier::Identifier) => {
                // Search by identifier via index table (ref_kind=4)
                build_index_identifier_condition(builder, resource_type, &param.name, &value.raw)
            }

            Some(SearchModifier::Type(type_name)) => {
                // Type-specific reference: subject:Patient=123
                let rt_param = builder.add_text_param(resource_type);
                let pc_param = builder.add_text_param(&param.name);
                let tt_param = builder.add_text_param(type_name);
                let tid_param = builder.add_text_param(&value.raw);
                format!(
                    "EXISTS (SELECT 1 FROM search_idx_reference sir \
                     WHERE sir.resource_type = ${rt_param} AND sir.resource_id = r.id \
                     AND sir.param_code = ${pc_param} AND sir.ref_kind = 1 \
                     AND sir.target_type = ${tt_param} AND sir.target_id = ${tid_param})"
                )
            }

            Some(SearchModifier::Missing) => {
                // :missing still uses JSONB path (checks field presence)
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

/// Build a direct JSONB reference match condition.
///
/// This produces a condition that compares the `reference` string field of
/// a Reference JSONB element directly. It supports both single Reference
/// objects (jsonb_path->>'reference') and arrays of References
/// (any element's 'reference' equals the search value). It accepts both
/// `Type/id` form and bare `id` form (with single target type).
fn build_jsonb_reference_condition(
    builder: &mut SqlBuilder,
    jsonb_path: &str,
    ref_value: &str,
    target_types: &[String],
) -> String {
    // Compose all the candidate reference string values to compare against.
    let mut candidates: Vec<String> = Vec::new();

    if ref_value.contains('/') {
        // Already Type/id — match as-is.
        candidates.push(ref_value.to_string());
    } else if target_types.len() == 1 {
        // Bare id with a single target type — accept both forms.
        candidates.push(format!("{}/{}", target_types[0], ref_value));
        candidates.push(ref_value.to_string());
    } else {
        // Bare id with multiple/no targets — try each known target type plus
        // a bare-id form as a last resort.
        for t in target_types {
            candidates.push(format!("{}/{}", t, ref_value));
        }
        candidates.push(ref_value.to_string());
    }

    // Bind each candidate as a text parameter.
    let mut param_nums: Vec<usize> = Vec::with_capacity(candidates.len());
    for c in &candidates {
        param_nums.push(builder.add_text_param(c));
    }

    // Build single-value and array-value match clauses. We can't know at SQL
    // build time whether jsonb_path is an object or an array, so we OR a
    // direct ->>'reference' compare with an EXISTS over jsonb_array_elements.
    let single_match: String = param_nums
        .iter()
        .map(|p| format!("({jsonb_path}->>'reference' = ${p})"))
        .collect::<Vec<_>>()
        .join(" OR ");

    let array_match: String = param_nums
        .iter()
        .map(|p| format!("e->>'reference' = ${p}"))
        .collect::<Vec<_>>()
        .join(" OR ");

    format!(
        "(({single_match}) OR (jsonb_typeof({jsonb_path}) = 'array' AND EXISTS (\
         SELECT 1 FROM jsonb_array_elements({jsonb_path}) AS e WHERE {array_match})))"
    )
}

/// Build default reference matching condition using index table.
fn build_index_reference_condition(
    builder: &mut SqlBuilder,
    resource_type: &str,
    param_code: &str,
    ref_value: &str,
    target_types: &[String],
) -> String {
    let rt_param = builder.add_text_param(resource_type);
    let pc_param = builder.add_text_param(param_code);

    if ref_value.starts_with("http://") || ref_value.starts_with("https://") {
        // URL — match external_url or raw_reference
        let url_param = builder.add_text_param(ref_value);
        format!(
            "EXISTS (SELECT 1 FROM search_idx_reference sir \
             WHERE sir.resource_type = ${rt_param} AND sir.resource_id = r.id \
             AND sir.param_code = ${pc_param} \
             AND (sir.external_url = ${url_param} OR sir.raw_reference = ${url_param}))"
        )
    } else if ref_value.contains('/') {
        // Type/id format — exact match on target_type + target_id
        let parts: Vec<&str> = ref_value.splitn(2, '/').collect();
        let tt_param = builder.add_text_param(parts[0]);
        let tid_param = builder.add_text_param(parts[1]);
        format!(
            "EXISTS (SELECT 1 FROM search_idx_reference sir \
             WHERE sir.resource_type = ${rt_param} AND sir.resource_id = r.id \
             AND sir.param_code = ${pc_param} AND sir.ref_kind = 1 \
             AND sir.target_type = ${tt_param} AND sir.target_id = ${tid_param})"
        )
    } else if target_types.len() == 1 {
        // ID only with single target type
        let tt_param = builder.add_text_param(&target_types[0]);
        let tid_param = builder.add_text_param(ref_value);
        format!(
            "EXISTS (SELECT 1 FROM search_idx_reference sir \
             WHERE sir.resource_type = ${rt_param} AND sir.resource_id = r.id \
             AND sir.param_code = ${pc_param} AND sir.ref_kind = 1 \
             AND sir.target_type = ${tt_param} AND sir.target_id = ${tid_param})"
        )
    } else {
        // ID only with multiple target types — match any target_type
        let tid_param = builder.add_text_param(ref_value);
        format!(
            "EXISTS (SELECT 1 FROM search_idx_reference sir \
             WHERE sir.resource_type = ${rt_param} AND sir.resource_id = r.id \
             AND sir.param_code = ${pc_param} AND sir.ref_kind = 1 \
             AND sir.target_id = ${tid_param})"
        )
    }
}

/// Build identifier-based reference condition using index table (ref_kind=4).
fn build_index_identifier_condition(
    builder: &mut SqlBuilder,
    resource_type: &str,
    param_code: &str,
    value: &str,
) -> String {
    let rt_param = builder.add_text_param(resource_type);
    let pc_param = builder.add_text_param(param_code);

    let (system, id_value) = parse_identifier_value(value);

    match system {
        Some(sys) if !sys.is_empty() => {
            // system|value — match both
            let sys_param = builder.add_text_param(sys);
            let val_param = builder.add_text_param(id_value);
            format!(
                "EXISTS (SELECT 1 FROM search_idx_reference sir \
                 WHERE sir.resource_type = ${rt_param} AND sir.resource_id = r.id \
                 AND sir.param_code = ${pc_param} AND sir.ref_kind = 4 \
                 AND sir.identifier_system = ${sys_param} AND sir.identifier_value = ${val_param})"
            )
        }
        Some(_) => {
            // |value — no system
            let val_param = builder.add_text_param(id_value);
            format!(
                "EXISTS (SELECT 1 FROM search_idx_reference sir \
                 WHERE sir.resource_type = ${rt_param} AND sir.resource_id = r.id \
                 AND sir.param_code = ${pc_param} AND sir.ref_kind = 4 \
                 AND sir.identifier_system IS NULL AND sir.identifier_value = ${val_param})"
            )
        }
        None => {
            // value only — any system
            let val_param = builder.add_text_param(id_value);
            format!(
                "EXISTS (SELECT 1 FROM search_idx_reference sir \
                 WHERE sir.resource_type = ${rt_param} AND sir.resource_id = r.id \
                 AND sir.param_code = ${pc_param} AND sir.ref_kind = 4 \
                 AND sir.identifier_value = ${val_param})"
            )
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
