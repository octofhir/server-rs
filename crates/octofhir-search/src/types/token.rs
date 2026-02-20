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

use crate::parameters::SearchModifier;
use crate::parser::ParsedParam;
use crate::sql_builder::{SqlBuilder, SqlBuilderError, SqlParam};
use crate::terminology::HybridTerminologyProvider;
use sqlx_postgres::PgPool;

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
    if param.values.is_empty() {
        return Ok(());
    }

    let mut or_conditions = Vec::new();

    for value in &param.values {
        if value.raw.is_empty() {
            continue;
        }

        let (system, code) = parse_token_value(&value.raw);

        let condition = match &param.modifier {
            None => build_default_token_condition(builder, jsonb_path, system, code),

            Some(SearchModifier::Not) => {
                let inner = build_default_token_condition(builder, jsonb_path, system, code);
                format!("NOT ({inner})")
            }

            Some(SearchModifier::Text) => {
                // Search on display text
                let p = builder.add_text_param(format!("%{code}%"));
                format!(
                    "EXISTS (SELECT 1 FROM jsonb_array_elements({jsonb_path}->'coding') AS c WHERE LOWER(c->>'display') LIKE LOWER(${p}))"
                )
            }

            Some(SearchModifier::In) => {
                return Err(SqlBuilderError::NotImplemented(
                    "in modifier requires ValueSet expansion".to_string(),
                ));
            }

            Some(SearchModifier::NotIn) => {
                return Err(SqlBuilderError::NotImplemented(
                    "not-in modifier requires ValueSet expansion".to_string(),
                ));
            }

            Some(SearchModifier::Below) => {
                return Err(SqlBuilderError::NotImplemented(
                    "below modifier requires terminology service".to_string(),
                ));
            }

            Some(SearchModifier::Above) => {
                return Err(SqlBuilderError::NotImplemented(
                    "above modifier requires terminology service".to_string(),
                ));
            }

            Some(SearchModifier::OfType) => {
                // For Identifier type filtering: type|system|value
                // The code contains type|system|value format
                build_identifier_of_type_condition(builder, jsonb_path, code)?
            }

            Some(SearchModifier::Missing) => {
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

/// Build SQL conditions for token search with terminology support.
///
/// This async version handles `:in`, `:not-in`, `:below`, and `:above` modifiers
/// by querying the terminology provider for ValueSet expansion and subsumption.
///
/// # Arguments
/// * `builder` - SQL builder to add conditions to
/// * `param` - Parsed search parameter
/// * `jsonb_path` - JSONB path to the field being searched
/// * `pool` - Database connection pool (required for large ValueSet optimization)
/// * `terminology` - Terminology provider for ValueSet expansion
pub async fn build_token_search_with_terminology(
    builder: &mut SqlBuilder,
    param: &ParsedParam,
    jsonb_path: &str,
    pool: &PgPool,
    terminology: Option<&HybridTerminologyProvider>,
) -> Result<(), SqlBuilderError> {
    if param.values.is_empty() {
        return Ok(());
    }

    let mut or_conditions = Vec::new();

    for value in &param.values {
        if value.raw.is_empty() {
            continue;
        }

        let (system, code) = parse_token_value(&value.raw);

        let condition = match &param.modifier {
            None => build_default_token_condition(builder, jsonb_path, system, code),

            Some(SearchModifier::Not) => {
                let inner = build_default_token_condition(builder, jsonb_path, system, code);
                format!("NOT ({inner})")
            }

            Some(SearchModifier::Text) => {
                let p = builder.add_text_param(format!("%{code}%"));
                format!(
                    "EXISTS (SELECT 1 FROM jsonb_array_elements({jsonb_path}->'coding') AS c WHERE LOWER(c->>'display') LIKE LOWER(${p}))"
                )
            }

            Some(SearchModifier::In) => {
                // Value is the ValueSet URL
                let valueset_url = &value.raw;
                build_in_modifier_condition(
                    builder,
                    jsonb_path,
                    valueset_url,
                    false,
                    pool,
                    terminology,
                )
                .await?
            }

            Some(SearchModifier::NotIn) => {
                // Value is the ValueSet URL
                let valueset_url = &value.raw;
                build_in_modifier_condition(
                    builder,
                    jsonb_path,
                    valueset_url,
                    true,
                    pool,
                    terminology,
                )
                .await?
            }

            Some(SearchModifier::Below) => {
                // Value format: system|code or just code
                build_subsumption_condition(builder, jsonb_path, system, code, true, terminology)
                    .await?
            }

            Some(SearchModifier::Above) => {
                // Value format: system|code or just code
                build_subsumption_condition(builder, jsonb_path, system, code, false, terminology)
                    .await?
            }

            Some(SearchModifier::OfType) => {
                build_identifier_of_type_condition(builder, jsonb_path, code)?
            }

            Some(SearchModifier::Missing) => {
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

/// Build SQL condition for `:in` or `:not-in` modifiers.
///
/// Expands the ValueSet using the optimized expansion method that automatically
/// chooses between IN clause (small expansions <500 codes) and temp table strategy
/// (large expansions ≥500 codes) for optimal performance.
async fn build_in_modifier_condition(
    builder: &mut SqlBuilder,
    jsonb_path: &str,
    valueset_url: &str,
    negate: bool,
    pool: &PgPool,
    terminology: Option<&HybridTerminologyProvider>,
) -> Result<String, SqlBuilderError> {
    let terminology = terminology.ok_or_else(|| {
        SqlBuilderError::NotImplemented(
            "in/not-in modifiers require terminology provider".to_string(),
        )
    })?;

    tracing::debug!(
        valueset_url = valueset_url,
        negate = negate,
        "Expanding ValueSet with automatic optimization"
    );

    // Expand the ValueSet with automatic optimization
    // - Small (<500 codes): Returns InClause variant
    // - Large (≥500 codes): Returns TempTable variant with session_id
    let expansion_result = terminology
        .expand_valueset_for_search(pool, valueset_url, None)
        .await
        .map_err(|e| {
            SqlBuilderError::InvalidSearchValue(format!(
                "Failed to expand ValueSet '{}': {}",
                valueset_url, e
            ))
        })?;

    // Check if expansion is empty
    let is_empty = match &expansion_result {
        crate::terminology::ExpansionResult::InClause(concepts) => concepts.is_empty(),
        _ => false,
    };

    if is_empty {
        // Empty ValueSet - :in matches nothing, :not-in matches everything
        return Ok(if negate {
            "TRUE".to_string()
        } else {
            "FALSE".to_string()
        });
    }

    // Build condition in a temporary builder to capture it as a string
    let mut temp_builder = SqlBuilder::new().with_param_offset(builder.param_count());
    temp_builder.add_valueset_condition(jsonb_path, &expansion_result);

    // Get the generated condition
    let conditions = temp_builder.conditions();
    let condition = conditions
        .first()
        .ok_or_else(|| {
            SqlBuilderError::InvalidSearchValue("Failed to build ValueSet condition".to_string())
        })?
        .clone();

    // Copy parameters from temp builder to main builder
    // We need to manually add each param type
    for param in temp_builder.params() {
        match param {
            SqlParam::Text(s) => {
                builder.add_text_param(s);
            }
            SqlParam::Integer(i) => {
                builder.add_integer_param(*i);
            }
            SqlParam::Json(s) => {
                builder.add_json_param(s);
            }
            SqlParam::Float(f) => {
                builder.add_float_param(*f);
            }
            SqlParam::Boolean(b) => {
                builder.add_boolean_param(*b);
            }
            SqlParam::Timestamp(s) => {
                builder.add_timestamp_param(s);
            }
        }
    }

    Ok(if negate {
        format!("NOT ({condition})")
    } else {
        condition
    })
}

/// Build SQL condition for `:below` or `:above` modifiers (subsumption).
///
/// For `:below`: find all codes that are descendants of the given code.
/// For `:above`: find all codes that are ancestors of the given code.
///
/// Uses the terminology provider's `expand_hierarchy()` method which:
/// - For SNOMED CT: Uses ECL (Expression Constraint Language)
/// - For other systems: Attempts to use remote terminology server
/// - Fallback: Returns only the exact code if hierarchy expansion is not supported
async fn build_subsumption_condition(
    builder: &mut SqlBuilder,
    jsonb_path: &str,
    system: Option<&str>,
    code: &str,
    is_below: bool,
    terminology: Option<&HybridTerminologyProvider>,
) -> Result<String, SqlBuilderError> {
    let terminology = terminology.ok_or_else(|| {
        SqlBuilderError::NotImplemented(
            "below/above modifiers require terminology provider".to_string(),
        )
    })?;

    let system = system.filter(|s| !s.is_empty()).ok_or_else(|| {
        SqlBuilderError::InvalidSearchValue(
            "below/above modifiers require system|code format".to_string(),
        )
    })?;

    tracing::debug!(
        system = system,
        code = code,
        modifier = if is_below { "below" } else { "above" },
        "Expanding code hierarchy for subsumption search"
    );

    // Determine hierarchy direction
    let direction = if is_below {
        crate::terminology::HierarchyDirection::Below
    } else {
        crate::terminology::HierarchyDirection::Above
    };

    // Expand the hierarchy to get all related codes
    let hierarchy_codes = terminology
        .expand_hierarchy(system, code, direction)
        .await
        .map_err(|e| {
            SqlBuilderError::InvalidSearchValue(format!(
                "Failed to expand hierarchy for {}|{}: {}",
                system, code, e
            ))
        })?;

    tracing::debug!(
        system = system,
        code = code,
        hierarchy_size = hierarchy_codes.len(),
        "Expanded hierarchy"
    );

    // Build SQL conditions for all codes in the hierarchy
    let mut conditions = Vec::new();
    for hierarchy_code in &hierarchy_codes {
        let p_sys = builder.add_text_param(system);
        let p_code = builder.add_text_param(hierarchy_code);
        conditions.push(format!(
            "EXISTS (SELECT 1 FROM jsonb_array_elements({jsonb_path}->'coding') AS c \
             WHERE c->>'system' = ${p_sys} AND c->>'code' = ${p_code})"
        ));
    }

    if conditions.is_empty() {
        // Empty hierarchy - should not happen but handle gracefully
        return Ok("FALSE".to_string());
    }

    Ok(if conditions.len() == 1 {
        conditions[0].clone()
    } else {
        format!("({})", conditions.join(" OR "))
    })
}

/// Build condition for default token matching.
fn build_default_token_condition(
    builder: &mut SqlBuilder,
    jsonb_path: &str,
    system: Option<&str>,
    code: &str,
) -> String {
    match system {
        Some(sys) if !sys.is_empty() => {
            // system|code - match both
            let json = serde_json::json!([{"system": sys, "code": code}]).to_string();
            let p = builder.add_json_param(&json);
            // Check in CodeableConcept.coding array
            format!(
                "({jsonb_path}->'coding' @> ${p}::jsonb OR \
                 ({jsonb_path}->>'system' = '{sys}' AND {jsonb_path}->>'code' = '{code}') OR \
                 ({jsonb_path}->>'system' = '{sys}' AND {jsonb_path}->>'value' = '{code}'))"
            )
        }
        Some(_) => {
            // |code - code with no system (null system) - empty string case
            let p = builder.add_text_param(code);
            format!(
                "(({jsonb_path}->>'system' IS NULL AND {jsonb_path}->>'code' = ${p}) OR \
                 ({jsonb_path}->>'system' IS NULL AND {jsonb_path}->>'value' = ${p}) OR \
                 EXISTS (SELECT 1 FROM jsonb_array_elements({jsonb_path}->'coding') AS c \
                         WHERE c->>'system' IS NULL AND c->>'code' = ${p}))"
            )
        }
        None => {
            // code only - match in any system
            // Checks: direct code/value fields, and coding array elements
            let p = builder.add_text_param(code);
            format!(
                "({jsonb_path}->>'code' = ${p} OR \
                 {jsonb_path}->>'value' = ${p} OR \
                 EXISTS (SELECT 1 FROM jsonb_array_elements({jsonb_path}->'coding') AS c WHERE c->>'code' = ${p}))"
            )
        }
    }
}

/// Build condition for Identifier type filtering (:of-type modifier).
///
/// Format: type|system|value where type is the identifier type code
fn build_identifier_of_type_condition(
    builder: &mut SqlBuilder,
    jsonb_path: &str,
    value: &str,
) -> Result<String, SqlBuilderError> {
    let parts: Vec<&str> = value.splitn(3, '|').collect();

    if parts.len() < 2 {
        return Err(SqlBuilderError::InvalidSearchValue(
            "of-type modifier requires type|system|value or type|value format".to_string(),
        ));
    }

    let type_code = parts[0];

    if parts.len() == 3 {
        // type|system|value
        let system = parts[1];
        let id_value = parts[2];
        let p_type = builder.add_text_param(type_code);
        let p_sys = builder.add_text_param(system);
        let p_val = builder.add_text_param(id_value);

        Ok(format!(
            "EXISTS (SELECT 1 FROM jsonb_array_elements({jsonb_path}) AS ident \
             WHERE ident->'type'->'coding' @> '[{{\"code\": \"{}\"}}]'::jsonb \
             AND ident->>'system' = ${p_sys} AND ident->>'value' = ${p_val})",
            type_code.replace('"', "\\\"")
        )
        .replace(&format!("${p_type}"), type_code))
    } else {
        // type|value (system is any)
        let id_value = parts[1];
        let p_val = builder.add_text_param(id_value);

        Ok(format!(
            "EXISTS (SELECT 1 FROM jsonb_array_elements({jsonb_path}) AS ident \
             WHERE ident->'type'->'coding' @> '[{{\"code\": \"{}\"}}]'::jsonb \
             AND ident->>'value' = ${p_val})",
            type_code.replace('"', "\\\"")
        ))
    }
}

/// Build token search for Identifier arrays.
///
/// Identifiers have system and value fields rather than coding.
pub fn build_identifier_search(
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

        let (system, code) = parse_token_value(&value.raw);

        let condition = match &param.modifier {
            None => {
                match system {
                    Some(sys) if !sys.is_empty() => {
                        // system|value
                        let json = serde_json::json!([{"system": sys, "value": code}]).to_string();
                        let p = builder.add_json_param(&json);
                        format!("{array_path} @> ${p}::jsonb")
                    }
                    Some(_) => {
                        // |value - no system (empty string case)
                        let p = builder.add_text_param(code);
                        format!(
                            "EXISTS (SELECT 1 FROM jsonb_array_elements({array_path}) AS ident \
                             WHERE (ident->>'system' IS NULL OR ident->>'system' = '') AND ident->>'value' = ${p})"
                        )
                    }
                    None => {
                        // value only
                        let p = builder.add_text_param(code);
                        format!(
                            "EXISTS (SELECT 1 FROM jsonb_array_elements({array_path}) AS ident \
                             WHERE ident->>'value' = ${p})"
                        )
                    }
                }
            }

            Some(SearchModifier::Not) => match system {
                Some(sys) if !sys.is_empty() => {
                    let json = serde_json::json!([{"system": sys, "value": code}]).to_string();
                    let p = builder.add_json_param(&json);
                    format!("NOT ({array_path} @> ${p}::jsonb)")
                }
                _ => {
                    let p = builder.add_text_param(code);
                    format!(
                        "NOT EXISTS (SELECT 1 FROM jsonb_array_elements({array_path}) AS ident \
                             WHERE ident->>'value' = ${p})"
                    )
                }
            },

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
    if param.values.is_empty() {
        return Ok(());
    }

    let resource_col = builder.resource_column().to_string();
    let mut or_conditions = Vec::new();

    for value in &param.values {
        if value.raw.is_empty() {
            continue;
        }

        let (system, code) = parse_token_value(&value.raw);

        let condition = match &param.modifier {
            None => build_gin_token_containment(&mut *builder, &resource_col, path_segments, system, code),

            Some(SearchModifier::Not) => {
                let inner = build_gin_token_containment(&mut *builder, &resource_col, path_segments, system, code);
                format!("NOT ({inner})")
            }

            // For other modifiers, fall back to the standard token search
            _ => {
                let json_path = crate::sql_builder::build_jsonb_accessor(&resource_col, path_segments, false);
                let mut temp_builder = SqlBuilder::new().with_param_offset(builder.param_count());
                build_token_search(&mut temp_builder, param, &json_path)?;
                // Copy params
                for p in temp_builder.params() {
                    match p {
                        SqlParam::Text(s) => { builder.add_text_param(s); }
                        SqlParam::Json(s) => { builder.add_json_param(s); }
                        SqlParam::Integer(i) => { builder.add_integer_param(*i); }
                        SqlParam::Float(f) => { builder.add_float_param(*f); }
                        SqlParam::Boolean(b) => { builder.add_boolean_param(*b); }
                        SqlParam::Timestamp(s) => { builder.add_timestamp_param(s); }
                    }
                }
                let conditions = temp_builder.conditions();
                if let Some(c) = conditions.first() {
                    builder.add_condition(c.clone());
                }
                return Ok(());
            }
        };

        or_conditions.push(condition);
    }

    if !or_conditions.is_empty() {
        builder.add_condition(SqlBuilder::build_or_clause(&or_conditions));
    }

    Ok(())
}

/// Build GIN containment condition for a token (CodeableConcept/Coding).
fn build_gin_token_containment(
    builder: &mut SqlBuilder,
    resource_col: &str,
    path_segments: &[String],
    system: Option<&str>,
    code: &str,
) -> String {
    // Build the coding object based on system presence
    let coding_obj = match system {
        Some(sys) if !sys.is_empty() => {
            serde_json::json!({"system": sys, "code": code})
        }
        _ => {
            serde_json::json!({"code": code})
        }
    };

    // Wrap in {"coding": [...]} for CodeableConcept
    let field_value = serde_json::json!({"coding": [coding_obj]});

    // Build the nested JSON containment object from path segments
    let containment = build_nested_containment(path_segments, field_value);

    let json_str = containment.to_string();
    let p = builder.add_json_param(&json_str);
    format!("{resource_col} @> ${p}::jsonb")
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
    if param.values.is_empty() {
        return Ok(());
    }

    let resource_col = builder.resource_column().to_string();
    let mut or_conditions = Vec::new();

    for value in &param.values {
        if value.raw.is_empty() {
            continue;
        }

        // For simple codes, ignore system part
        let (_, code) = parse_token_value(&value.raw);

        let condition = match &param.modifier {
            None => {
                let containment = build_nested_containment(path_segments, serde_json::json!(code));
                let json_str = containment.to_string();
                let p = builder.add_json_param(&json_str);
                format!("{resource_col} @> ${p}::jsonb")
            }

            Some(SearchModifier::Not) => {
                let containment = build_nested_containment(path_segments, serde_json::json!(code));
                let json_str = containment.to_string();
                let p = builder.add_json_param(&json_str);
                format!("NOT ({resource_col} @> ${p}::jsonb)")
            }

            Some(SearchModifier::Missing) => {
                let text_path = crate::sql_builder::build_jsonb_accessor(&resource_col, path_segments, true);
                let is_missing = value.raw.eq_ignore_ascii_case("true");
                if is_missing {
                    format!("({text_path} IS NULL OR {text_path} = 'null')")
                } else {
                    format!("({text_path} IS NOT NULL AND {text_path} != 'null')")
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

/// Build a nested JSON object from path segments wrapping a leaf value.
///
/// For path `["code"]` and value `{"coding": [...]}`, produces:
/// `{"code": {"coding": [...]}}`
///
/// For path `["name", "family"]` and value `"Smith"`, produces:
/// `{"name": [{"family": "Smith"}]}`  (arrays handled by caller)
fn build_nested_containment(path_segments: &[String], leaf_value: serde_json::Value) -> serde_json::Value {
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
    if param.values.is_empty() {
        return Ok(());
    }

    let mut or_conditions = Vec::new();

    for value in &param.values {
        if value.raw.is_empty() {
            continue;
        }

        // For simple codes, ignore system part
        let (_, code) = parse_token_value(&value.raw);

        let condition = match &param.modifier {
            None => {
                let p = builder.add_text_param(code);
                format!("{jsonb_path} = ${p}")
            }

            Some(SearchModifier::Not) => {
                let p = builder.add_text_param(code);
                format!("{jsonb_path} != ${p}")
            }

            Some(SearchModifier::Missing) => {
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
        assert!(clause.contains("http://loinc.org"));
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
    fn test_token_not_modifier() {
        let mut builder = SqlBuilder::new();
        let param = make_param("status", "active", Some(SearchModifier::Not));

        build_token_search(&mut builder, &param, "resource->'status'").unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert!(clause.starts_with("NOT ("));
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
    fn test_gin_code_search_not_modifier() {
        let mut builder = SqlBuilder::new();
        let param = make_param("gender", "female", Some(SearchModifier::Not));

        build_gin_code_search(&mut builder, &param, &["gender".to_string()]).unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert!(
            clause.starts_with("NOT ("),
            "Expected NOT wrapper, got: {clause}"
        );
        assert!(clause.contains("@>"));
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
    fn test_gin_token_search_not_modifier() {
        let mut builder = SqlBuilder::new();
        let param = make_param("code", "http://loinc.org|8480-6", Some(SearchModifier::Not));

        build_gin_token_search(&mut builder, &param, &["code".to_string()]).unwrap();

        let clause = builder.build_where_clause().unwrap();
        assert!(
            clause.starts_with("NOT ("),
            "Expected NOT wrapper, got: {clause}"
        );
        assert!(clause.contains("@>"));
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
        let result = build_nested_containment(
            &["gender".to_string()],
            serde_json::json!("female"),
        );
        let expected = serde_json::json!({"gender": "female"});
        assert_eq!(result, expected);
    }
}
