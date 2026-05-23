use crate::ir::ast::{
    NumberClause, NumberPredicate, QuantityClause, QuantityPredicate, ReferenceClause,
    ReferencePredicate, StringClause, StringPredicate, TokenClause, TokenPredicate, UriClause,
    UriPredicate,
};
use crate::ir::sql::{RangeOp, SelectStmt, SqlExpr, SqlOp, SqlTerm};
use crate::parameters::SearchPrefix;
use crate::sql_builder::{SqlBuilder, SqlBuilderError};
use crate::types::date_ast::DateClause;
use octofhir_core::search_index::normalize_string;

/// Render date sidecar clauses as one OR group.
pub fn render_date_clauses_as_or(
    builder: &mut SqlBuilder,
    clauses: &[DateClause],
) -> Option<String> {
    let rendered = clauses
        .iter()
        .map(|clause| clause.render(builder))
        .collect::<Vec<_>>();
    if rendered.is_empty() {
        None
    } else {
        Some(SqlBuilder::build_or_clause(&rendered))
    }
}

/// Render string sidecar clauses as one OR group.
pub fn render_string_clauses_as_or(
    builder: &mut SqlBuilder,
    clauses: &[StringClause],
) -> Option<String> {
    let rendered = clauses
        .iter()
        .map(|clause| render_string_clause(builder, clause))
        .collect::<Vec<_>>();
    if rendered.is_empty() {
        None
    } else {
        Some(SqlBuilder::build_or_clause(&rendered))
    }
}

/// Render reference clauses as one OR group.
pub fn render_reference_clauses_as_or(
    builder: &mut SqlBuilder,
    clauses: &[ReferenceClause],
    jsonb_path: &str,
) -> Option<String> {
    let rendered = clauses
        .iter()
        .map(|clause| render_reference_clause(builder, clause, jsonb_path))
        .collect::<Vec<_>>();
    if rendered.is_empty() {
        None
    } else {
        Some(SqlBuilder::build_or_clause(&rendered))
    }
}

/// Render scalar URI clauses as one OR group.
pub fn render_uri_clauses_as_or(
    builder: &mut SqlBuilder,
    clauses: &[UriClause],
    path: &str,
) -> Option<String> {
    let rendered = clauses
        .iter()
        .map(|clause| render_uri_clause(builder, clause, path))
        .collect::<Vec<_>>();
    if rendered.is_empty() {
        None
    } else {
        Some(SqlBuilder::build_or_clause(&rendered))
    }
}

/// Render URI-array clauses as one OR group.
pub fn render_uri_array_clauses_as_or(
    builder: &mut SqlBuilder,
    clauses: &[UriClause],
    array_path: &str,
) -> Option<String> {
    let rendered = clauses
        .iter()
        .map(|clause| render_uri_array_clause(builder, clause, array_path))
        .collect::<Vec<_>>();
    if rendered.is_empty() {
        None
    } else {
        Some(SqlBuilder::build_or_clause(&rendered))
    }
}

/// Render number clauses as one OR group over the current JSONB numeric-cast path.
pub fn render_number_clauses_as_or(
    builder: &mut SqlBuilder,
    clauses: &[NumberClause],
    jsonb_path: &str,
) -> Result<Option<String>, SqlBuilderError> {
    let rendered = clauses
        .iter()
        .map(|clause| render_number_clause(builder, clause, jsonb_path))
        .collect::<Result<Vec<_>, _>>()?;
    if rendered.is_empty() {
        Ok(None)
    } else {
        Ok(Some(SqlBuilder::build_or_clause(&rendered)))
    }
}

/// Render quantity clauses as one OR group over the current JSONB numeric-cast path.
pub fn render_quantity_clauses_as_or(
    builder: &mut SqlBuilder,
    clauses: &[QuantityClause],
    jsonb_path: &str,
) -> Result<Option<String>, SqlBuilderError> {
    let rendered = clauses
        .iter()
        .map(|clause| render_quantity_clause(builder, clause, jsonb_path))
        .collect::<Result<Vec<_>, _>>()?;
    if rendered.is_empty() {
        Ok(None)
    } else {
        Ok(Some(SqlBuilder::build_or_clause(&rendered)))
    }
}

/// Render simple-code token clauses as one OR group.
///
/// This covers scalar/array code SearchParameters such as `Patient.gender`.
/// CodeableConcept/Coding and Identifier token renderers remain separate slices.
pub fn render_token_simple_code_clauses_as_or(
    builder: &mut SqlBuilder,
    clauses: &[TokenClause],
    path_segments: &[String],
) -> Result<Option<String>, SqlBuilderError> {
    let rendered = clauses
        .iter()
        .map(|clause| render_token_simple_code_clause(builder, clause, path_segments))
        .collect::<Result<Vec<_>, _>>()?;
    if rendered.is_empty() {
        Ok(None)
    } else {
        Ok(Some(SqlBuilder::build_or_clause(&rendered)))
    }
}

/// Render Coding/CodeableConcept token clauses as one OR group.
pub fn render_token_coding_clauses_as_or(
    builder: &mut SqlBuilder,
    clauses: &[TokenClause],
    path_segments: &[String],
) -> Result<Option<String>, SqlBuilderError> {
    let rendered = clauses
        .iter()
        .map(|clause| render_token_coding_clause(builder, clause, path_segments))
        .collect::<Result<Vec<_>, _>>()?;
    if rendered.is_empty() {
        Ok(None)
    } else {
        Ok(Some(SqlBuilder::build_or_clause(&rendered)))
    }
}

/// Render Identifier token clauses as one OR group.
pub fn render_token_identifier_clauses_as_or(
    builder: &mut SqlBuilder,
    clauses: &[TokenClause],
    array_path: &str,
) -> Result<Option<String>, SqlBuilderError> {
    let rendered = clauses
        .iter()
        .map(|clause| render_token_identifier_clause(builder, clause, array_path))
        .collect::<Result<Vec<_>, _>>()?;
    if rendered.is_empty() {
        Ok(None)
    } else {
        Ok(Some(SqlBuilder::build_or_clause(&rendered)))
    }
}

fn render_number_clause(
    builder: &mut SqlBuilder,
    clause: &NumberClause,
    jsonb_path: &str,
) -> Result<String, SqlBuilderError> {
    match &clause.predicate {
        NumberPredicate::Missing { is_missing } => {
            let condition = if *is_missing {
                format!("({jsonb_path} IS NULL OR {jsonb_path} = 'null')")
            } else {
                format!("({jsonb_path} IS NOT NULL AND {jsonb_path} != 'null')")
            };
            Ok(condition)
        }
        NumberPredicate::Comparison { prefix, value } => {
            let number = RenderDecimalParts::parse(value)?;
            Ok(render_numeric_comparison(
                builder, jsonb_path, *prefix, &number,
            ))
        }
    }
}

fn render_uri_clause(builder: &mut SqlBuilder, clause: &UriClause, path: &str) -> String {
    match &clause.predicate {
        UriPredicate::Exact { value } => {
            let p = builder.add_text_param(value);
            format!("{path} = ${p}")
        }
        UriPredicate::Below { value } => {
            let escaped = escape_like_pattern(value);
            let p = builder.add_text_param(format!("{escaped}%"));
            format!("{path} LIKE ${p}")
        }
        UriPredicate::Above { value } => {
            let p = builder.add_text_param(value);
            format!("${p} LIKE {path} || '%'")
        }
        UriPredicate::Contains { value } => {
            let escaped = escape_like_pattern(&value.to_lowercase());
            let p = builder.add_text_param(format!("%{escaped}%"));
            format!("LOWER({path}) LIKE ${p}")
        }
        UriPredicate::Missing { is_missing } => {
            if *is_missing {
                format!("({path} IS NULL OR {path} = 'null' OR {path} = '\"\"')")
            } else {
                format!("({path} IS NOT NULL AND {path} != 'null' AND {path} != '\"\"')")
            }
        }
    }
}

fn render_uri_array_clause(
    builder: &mut SqlBuilder,
    clause: &UriClause,
    array_path: &str,
) -> String {
    match &clause.predicate {
        UriPredicate::Exact { value } => {
            let p = builder.add_text_param(value);
            format!(
                "EXISTS (SELECT 1 FROM jsonb_array_elements_text({array_path}) AS uri WHERE uri = ${p})"
            )
        }
        UriPredicate::Below { value } => {
            let escaped = escape_like_pattern(value);
            let p = builder.add_text_param(format!("{escaped}%"));
            format!(
                "EXISTS (SELECT 1 FROM jsonb_array_elements_text({array_path}) AS uri WHERE uri LIKE ${p})"
            )
        }
        UriPredicate::Above { value } => {
            let p = builder.add_text_param(value);
            format!(
                "EXISTS (SELECT 1 FROM jsonb_array_elements_text({array_path}) AS uri WHERE ${p} LIKE uri || '%')"
            )
        }
        UriPredicate::Contains { value } => {
            let escaped = escape_like_pattern(&value.to_lowercase());
            let p = builder.add_text_param(format!("%{escaped}%"));
            format!(
                "EXISTS (SELECT 1 FROM jsonb_array_elements_text({array_path}) AS uri WHERE LOWER(uri) LIKE ${p})"
            )
        }
        UriPredicate::Missing { is_missing } => {
            if *is_missing {
                format!("({array_path} IS NULL OR jsonb_array_length({array_path}) = 0)")
            } else {
                format!("({array_path} IS NOT NULL AND jsonb_array_length({array_path}) > 0)")
            }
        }
    }
}

fn render_token_simple_code_clause(
    builder: &mut SqlBuilder,
    clause: &TokenClause,
    path_segments: &[String],
) -> Result<String, SqlBuilderError> {
    let resource_col = builder.resource_column().to_string();
    match &clause.predicate {
        TokenPredicate::Missing { is_missing } => {
            let text_path =
                crate::sql_builder::build_jsonb_accessor(&resource_col, path_segments, true);
            let condition = if *is_missing {
                format!("({text_path} IS NULL OR {text_path} = 'null')")
            } else {
                format!("({text_path} IS NOT NULL AND {text_path} != 'null')")
            };
            Ok(condition)
        }
        predicate => {
            let code = simple_code_token_value(predicate)?;
            let containment = build_nested_json_containment(path_segments, serde_json::json!(code));
            let json_str = containment.to_string();
            let p = builder.add_json_param(&json_str);
            let condition = format!("{resource_col} @> ${p}::jsonb");
            if clause.negated {
                Ok(format!("({condition}) = false"))
            } else {
                Ok(condition)
            }
        }
    }
}

fn render_token_identifier_clause(
    builder: &mut SqlBuilder,
    clause: &TokenClause,
    array_path: &str,
) -> Result<String, SqlBuilderError> {
    let condition = match &clause.predicate {
        TokenPredicate::AnySystemCode { code } => {
            render_identifier_value_only(builder, array_path, code)
        }
        TokenPredicate::NoSystemCode { code } => {
            render_identifier_no_system_value(builder, array_path, code)
        }
        TokenPredicate::SystemAnyCode { system } => {
            render_identifier_system_any_value(builder, array_path, system)
        }
        TokenPredicate::SystemCode { system, code } => {
            render_identifier_system_value(builder, array_path, system, code)
        }
        TokenPredicate::IdentifierOfType {
            system,
            code,
            value,
        } => render_identifier_of_type(builder, array_path, system, code, value),
        TokenPredicate::Missing { is_missing } => {
            if *is_missing {
                format!("({array_path} IS NULL OR jsonb_array_length({array_path}) = 0)")
            } else {
                format!("({array_path} IS NOT NULL AND jsonb_array_length({array_path}) > 0)")
            }
        }
        TokenPredicate::DisplayText { .. } | TokenPredicate::TerminologySet { .. } => {
            return Err(SqlBuilderError::InvalidModifier(format!(
                "{:?}",
                clause.predicate
            )));
        }
    };

    if clause.negated {
        Ok(format!("({condition}) = false"))
    } else {
        Ok(condition)
    }
}

fn render_identifier_system_value(
    builder: &mut SqlBuilder,
    array_path: &str,
    system: &str,
    value: &str,
) -> String {
    let json = serde_json::json!([{"system": system, "value": value}]).to_string();
    let p = builder.add_json_param(&json);
    format!("{array_path} @> ${p}::jsonb")
}

fn render_identifier_system_any_value(
    builder: &mut SqlBuilder,
    array_path: &str,
    system: &str,
) -> String {
    let p = builder.add_text_param(system);
    format!(
        "EXISTS (SELECT 1 FROM jsonb_array_elements({array_path}) AS ident \
         WHERE ident->>'system' = ${p})"
    )
}

fn render_identifier_no_system_value(
    builder: &mut SqlBuilder,
    array_path: &str,
    value: &str,
) -> String {
    let p = builder.add_text_param(value);
    format!(
        "EXISTS (SELECT 1 FROM jsonb_array_elements({array_path}) AS ident \
         WHERE (ident->>'system' IS NULL OR ident->>'system' = '') \
         AND ident->>'value' = ${p})"
    )
}

fn render_identifier_value_only(builder: &mut SqlBuilder, array_path: &str, value: &str) -> String {
    let p = builder.add_text_param(value);
    format!(
        "EXISTS (SELECT 1 FROM jsonb_array_elements({array_path}) AS ident \
         WHERE ident->>'value' = ${p})"
    )
}

fn render_identifier_of_type(
    builder: &mut SqlBuilder,
    array_path: &str,
    system: &str,
    code: &str,
    value: &str,
) -> String {
    let coding = serde_json::json!([{"system": system, "code": code}]).to_string();
    let p_coding = builder.add_json_param(&coding);
    let p_val = builder.add_text_param(value);
    format!(
        "EXISTS (SELECT 1 FROM jsonb_array_elements({array_path}) AS ident \
         WHERE ident->'type'->'coding' @> ${p_coding}::jsonb \
         AND ident->>'value' = ${p_val})"
    )
}

fn render_token_coding_clause(
    builder: &mut SqlBuilder,
    clause: &TokenClause,
    path_segments: &[String],
) -> Result<String, SqlBuilderError> {
    let resource_col = builder.resource_column().to_string();
    let jsonb_path = crate::sql_builder::build_jsonb_accessor(&resource_col, path_segments, false);

    let condition = match &clause.predicate {
        TokenPredicate::AnySystemCode { code } => {
            render_token_any_system_code(builder, &resource_col, path_segments, code)
        }
        TokenPredicate::NoSystemCode { code } => {
            render_token_no_system_code(builder, &jsonb_path, code)
        }
        TokenPredicate::SystemAnyCode { system } => {
            render_token_system_any_code(builder, &jsonb_path, system)
        }
        TokenPredicate::SystemCode { system, code } => {
            render_token_system_code(builder, &resource_col, path_segments, system, code)
        }
        TokenPredicate::DisplayText { text } => {
            let p = builder.add_text_param(format!("%{text}%"));
            format!(
                "EXISTS (SELECT 1 FROM jsonb_array_elements({jsonb_path}->'coding') AS c WHERE LOWER(c->>'display') LIKE LOWER(${p}))"
            )
        }
        TokenPredicate::Missing { is_missing } => {
            if *is_missing {
                format!("({jsonb_path} IS NULL OR {jsonb_path} = 'null')")
            } else {
                format!("({jsonb_path} IS NOT NULL AND {jsonb_path} != 'null')")
            }
        }
        TokenPredicate::TerminologySet { modifier, .. } => {
            return Err(SqlBuilderError::NotImplemented(format!(
                "{} modifier requires terminology provider",
                token_set_modifier_name(*modifier)
            )));
        }
        TokenPredicate::IdentifierOfType { .. } => {
            return Err(SqlBuilderError::InvalidModifier("OfType".to_string()));
        }
    };

    if clause.negated {
        Ok(format!("({condition}) = false"))
    } else {
        Ok(condition)
    }
}

fn render_token_any_system_code(
    builder: &mut SqlBuilder,
    resource_col: &str,
    path_segments: &[String],
    code: &str,
) -> String {
    let cc_clause =
        render_token_coding_containment(builder, resource_col, path_segments, None, code);

    let scalar_containment = build_nested_json_containment(path_segments, serde_json::json!(code));
    let p_scalar = builder.add_json_param(&scalar_containment.to_string());
    let scalar_clause = format!("{resource_col} @> ${p_scalar}::jsonb");

    let array_containment = build_nested_json_containment(path_segments, serde_json::json!([code]));
    let p_array = builder.add_json_param(&array_containment.to_string());
    let array_clause = format!("{resource_col} @> ${p_array}::jsonb");

    format!("({cc_clause} OR {scalar_clause} OR {array_clause})")
}

fn render_token_system_code(
    builder: &mut SqlBuilder,
    resource_col: &str,
    path_segments: &[String],
    system: &str,
    code: &str,
) -> String {
    render_token_coding_containment(builder, resource_col, path_segments, Some(system), code)
}

fn render_token_coding_containment(
    builder: &mut SqlBuilder,
    resource_col: &str,
    path_segments: &[String],
    system: Option<&str>,
    code: &str,
) -> String {
    let coding_obj = match system {
        Some(system) => serde_json::json!({"system": system, "code": code}),
        None => serde_json::json!({"code": code}),
    };
    let cc_value = serde_json::json!({"coding": [coding_obj]});
    let containment = build_nested_json_containment(path_segments, cc_value);
    let p = builder.add_json_param(&containment.to_string());
    format!("{resource_col} @> ${p}::jsonb")
}

fn render_token_no_system_code(builder: &mut SqlBuilder, jsonb_path: &str, code: &str) -> String {
    let p = builder.add_text_param(code);
    format!(
        "((({jsonb_path}->>'system' IS NULL OR {jsonb_path}->>'system' = '') \
          AND {jsonb_path}->>'code' = ${p}) OR \
         (({jsonb_path}->>'system' IS NULL OR {jsonb_path}->>'system' = '') \
          AND {jsonb_path}->>'value' = ${p}) OR \
         EXISTS (SELECT 1 FROM jsonb_array_elements({jsonb_path}->'coding') AS c \
                 WHERE (c->>'system' IS NULL OR c->>'system' = '') \
                 AND c->>'code' = ${p}))"
    )
}

fn render_token_system_any_code(
    builder: &mut SqlBuilder,
    jsonb_path: &str,
    system: &str,
) -> String {
    let p = builder.add_text_param(system);
    format!(
        "({jsonb_path}->>'system' = ${p} OR \
         EXISTS (SELECT 1 FROM jsonb_array_elements({jsonb_path}->'coding') AS c \
                 WHERE c->>'system' = ${p}))"
    )
}

fn token_set_modifier_name(modifier: crate::ir::TokenSetModifier) -> &'static str {
    match modifier {
        crate::ir::TokenSetModifier::In => "in",
        crate::ir::TokenSetModifier::NotIn => "not-in",
        crate::ir::TokenSetModifier::Below => "below",
        crate::ir::TokenSetModifier::Above => "above",
    }
}

fn simple_code_token_value(predicate: &TokenPredicate) -> Result<&str, SqlBuilderError> {
    match predicate {
        TokenPredicate::AnySystemCode { code }
        | TokenPredicate::NoSystemCode { code }
        | TokenPredicate::SystemCode { code, .. } => Ok(code),
        TokenPredicate::SystemAnyCode { .. } => Ok(""),
        TokenPredicate::IdentifierOfType { .. }
        | TokenPredicate::TerminologySet { .. }
        | TokenPredicate::DisplayText { .. }
        | TokenPredicate::Missing { .. } => {
            Err(SqlBuilderError::InvalidModifier(format!("{predicate:?}")))
        }
    }
}

fn render_quantity_clause(
    builder: &mut SqlBuilder,
    clause: &QuantityClause,
    jsonb_path: &str,
) -> Result<String, SqlBuilderError> {
    match &clause.predicate {
        QuantityPredicate::Missing { is_missing } => {
            let condition = if *is_missing {
                format!("({jsonb_path} IS NULL OR {jsonb_path} = 'null')")
            } else {
                format!("({jsonb_path} IS NOT NULL AND {jsonb_path} != 'null')")
            };
            Ok(condition)
        }
        QuantityPredicate::Comparison {
            prefix,
            value,
            system,
            code,
        } => {
            let number =
                RenderDecimalParts::parse(value).map_err(|_| invalid_quantity_number(value))?;
            let num_condition = render_numeric_comparison(
                builder,
                &format!("{jsonb_path}->>'value'"),
                *prefix,
                &number,
            );

            if system.is_none() && code.is_none() {
                return Ok(num_condition);
            }

            let mut constraints = vec![num_condition];
            if let Some(system) = system {
                let p = builder.add_text_param(system);
                constraints.push(format!("{jsonb_path}->>'system' = ${p}"));
            }
            if let Some(code) = code {
                let p = builder.add_text_param(code);
                constraints.push(format!(
                    "({jsonb_path}->>'code' = ${p} OR {jsonb_path}->>'unit' = ${p})"
                ));
            }

            Ok(format!("({})", constraints.join(" AND ")))
        }
    }
}

fn render_reference_clause(
    builder: &mut SqlBuilder,
    clause: &ReferenceClause,
    jsonb_path: &str,
) -> String {
    match &clause.predicate {
        ReferencePredicate::Missing { is_missing } => {
            if *is_missing {
                format!(
                    "({jsonb_path} IS NULL OR {jsonb_path} = 'null' OR {jsonb_path}->>'reference' IS NULL)"
                )
            } else {
                format!(
                    "({jsonb_path} IS NOT NULL AND {jsonb_path} != 'null' AND {jsonb_path}->>'reference' IS NOT NULL)"
                )
            }
        }
        predicate => {
            let index_cond = render_reference_index_condition(builder, clause, predicate);
            if let Some(ref_value) = &clause.jsonb_fallback_value {
                let jsonb_cond = render_jsonb_reference_condition(
                    builder,
                    jsonb_path,
                    ref_value,
                    &clause.target_types,
                );
                format!("({index_cond} OR {jsonb_cond})")
            } else {
                index_cond
            }
        }
    }
}

fn render_reference_index_condition(
    builder: &mut SqlBuilder,
    clause: &ReferenceClause,
    predicate: &ReferencePredicate,
) -> String {
    let rt_param = builder.add_text_param(&clause.resource_type);
    let pc_param = builder.add_text_param(&clause.param_code);
    let id_col = builder.id_column();

    match predicate {
        ReferencePredicate::Local {
            target_type,
            target_id,
        } => {
            if let Some(target_type) = target_type {
                let tt_param = builder.add_text_param(target_type);
                let tid_param = builder.add_text_param(target_id);
                format!(
                    "EXISTS (SELECT 1 FROM search_idx_reference sir \
                     WHERE sir.resource_type = ${rt_param} AND sir.resource_id = {id_col} \
                     AND sir.param_code = ${pc_param} AND sir.ref_kind = 1 \
                     AND sir.target_type = ${tt_param} AND sir.target_id = ${tid_param})"
                )
            } else {
                let tid_param = builder.add_text_param(target_id);
                format!(
                    "EXISTS (SELECT 1 FROM search_idx_reference sir \
                     WHERE sir.resource_type = ${rt_param} AND sir.resource_id = {id_col} \
                     AND sir.param_code = ${pc_param} AND sir.ref_kind = 1 \
                     AND sir.target_id = ${tid_param})"
                )
            }
        }
        ReferencePredicate::External { url } => {
            let url_param = builder.add_text_param(url);
            format!(
                "EXISTS (SELECT 1 FROM search_idx_reference sir \
                 WHERE sir.resource_type = ${rt_param} AND sir.resource_id = {id_col} \
                 AND sir.param_code = ${pc_param} \
                 AND (sir.external_url = ${url_param} OR sir.raw_reference = ${url_param}))"
            )
        }
        ReferencePredicate::Identifier {
            system,
            require_no_system,
            value,
        } => {
            let val_param = builder.add_text_param(value);
            if let Some(system) = system {
                let sys_param = builder.add_text_param(system);
                format!(
                    "EXISTS (SELECT 1 FROM search_idx_reference sir \
                     WHERE sir.resource_type = ${rt_param} AND sir.resource_id = {id_col} \
                     AND sir.param_code = ${pc_param} AND sir.ref_kind = 4 \
                     AND sir.identifier_system = ${sys_param} AND sir.identifier_value = ${val_param})"
                )
            } else if *require_no_system {
                format!(
                    "EXISTS (SELECT 1 FROM search_idx_reference sir \
                     WHERE sir.resource_type = ${rt_param} AND sir.resource_id = {id_col} \
                     AND sir.param_code = ${pc_param} AND sir.ref_kind = 4 \
                     AND sir.identifier_system IS NULL AND sir.identifier_value = ${val_param})"
                )
            } else {
                format!(
                    "EXISTS (SELECT 1 FROM search_idx_reference sir \
                     WHERE sir.resource_type = ${rt_param} AND sir.resource_id = {id_col} \
                     AND sir.param_code = ${pc_param} AND sir.ref_kind = 4 \
                     AND sir.identifier_value = ${val_param})"
                )
            }
        }
        ReferencePredicate::Missing { .. } => unreachable!("handled by caller"),
    }
}

fn render_jsonb_reference_condition(
    builder: &mut SqlBuilder,
    jsonb_path: &str,
    ref_value: &str,
    target_types: &[String],
) -> String {
    let mut candidates: Vec<String> = Vec::new();

    if ref_value.contains('/') {
        candidates.push(ref_value.to_string());
    } else if target_types.len() == 1 {
        candidates.push(format!("{}/{}", target_types[0], ref_value));
        candidates.push(ref_value.to_string());
    } else {
        for target_type in target_types {
            candidates.push(format!("{target_type}/{ref_value}"));
        }
        candidates.push(ref_value.to_string());
    }

    let param_nums = candidates
        .iter()
        .map(|candidate| builder.add_text_param(candidate))
        .collect::<Vec<_>>();

    let single_match = param_nums
        .iter()
        .map(|p| format!("({jsonb_path}->>'reference' = ${p})"))
        .collect::<Vec<_>>()
        .join(" OR ");
    let array_match = param_nums
        .iter()
        .map(|p| format!("e->>'reference' = ${p}"))
        .collect::<Vec<_>>()
        .join(" OR ");

    format!(
        "(({single_match}) OR (jsonb_typeof({jsonb_path}) = 'array' AND EXISTS (\
         SELECT 1 FROM jsonb_array_elements({jsonb_path}) AS e WHERE {array_match})))"
    )
}

fn build_nested_json_containment(
    path_segments: &[String],
    leaf_value: serde_json::Value,
) -> serde_json::Value {
    let mut result = leaf_value;
    for segment in path_segments.iter().rev() {
        result = serde_json::json!({ segment.as_str(): result });
    }
    result
}

fn render_string_clause(builder: &mut SqlBuilder, clause: &StringClause) -> String {
    let rt_param = builder.add_text_param(&clause.resource_type);
    let pc_param = builder.add_text_param(&clause.param_code);
    let id_col = builder.id_column();

    match &clause.predicate {
        StringPredicate::Missing { is_missing } => {
            let exists = format!(
                "EXISTS (SELECT 1 FROM search_idx_string sid \
                 WHERE sid.resource_type = ${rt_param} AND sid.resource_id = {id_col} \
                 AND sid.param_code = ${pc_param})"
            );
            if *is_missing {
                format!("NOT {exists}")
            } else {
                exists
            }
        }
        predicate => {
            let predicate_sql = match predicate {
                StringPredicate::Prefix { value } => {
                    let normalized = normalize_string(value);
                    let pattern = format!("{}%", escape_like_pattern(&normalized));
                    let p = builder.add_text_param(pattern);
                    format!("sid.value_norm LIKE ${p}")
                }
                StringPredicate::Contains { value } => {
                    let normalized = normalize_string(value);
                    let pattern = format!("%{}%", escape_like_pattern(&normalized));
                    let p = builder.add_text_param(pattern);
                    format!("sid.value_norm LIKE ${p}")
                }
                StringPredicate::Exact { value } => {
                    let p = builder.add_text_param(value);
                    format!("sid.value_exact = ${p}")
                }
                StringPredicate::Missing { .. } => unreachable!("handled above"),
            };
            format!(
                "EXISTS (SELECT 1 FROM search_idx_string sid \
                 WHERE sid.resource_type = ${rt_param} AND sid.resource_id = {id_col} \
                 AND sid.param_code = ${pc_param} \
                 AND {predicate_sql})"
            )
        }
    }
}

fn escape_like_pattern(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RenderDecimalParts {
    mantissa: i128,
    scale: u32,
}

impl RenderDecimalParts {
    fn parse(input: &str) -> Result<Self, SqlBuilderError> {
        let raw = input.trim();
        if raw.is_empty() {
            return Err(invalid_number(input));
        }

        let (negative, unsigned) = match raw.as_bytes()[0] {
            b'+' => (false, &raw[1..]),
            b'-' => (true, &raw[1..]),
            _ => (false, raw),
        };
        if unsigned.is_empty() {
            return Err(invalid_number(input));
        }

        let mut digits = String::new();
        let mut scale = 0_u32;
        let mut seen_dot = false;
        let mut seen_digit = false;

        for ch in unsigned.chars() {
            match ch {
                '0'..='9' => {
                    seen_digit = true;
                    digits.push(ch);
                    if seen_dot {
                        scale += 1;
                    }
                }
                '.' if !seen_dot => {
                    seen_dot = true;
                }
                _ => return Err(invalid_number(input)),
            }
        }

        if !seen_digit {
            return Err(invalid_number(input));
        }

        let mut mantissa = digits.parse::<i128>().map_err(|_| invalid_number(input))?;
        if negative {
            mantissa = -mantissa;
        }

        Ok(Self { mantissa, scale })
    }

    fn format(&self) -> String {
        format_decimal(self.mantissa, self.scale)
    }

    fn implicit_eq_bounds(&self) -> (String, String) {
        let scale = self.scale + 1;
        let centered = self.mantissa * 10;
        (
            format_decimal(centered - 5, scale),
            format_decimal(centered + 5, scale),
        )
    }

    fn approximate_bounds(&self) -> (String, String) {
        let scale = self.scale + 1;
        let centered = self.mantissa * 10;
        let delta = self.mantissa.abs();
        (
            format_decimal(centered - delta, scale),
            format_decimal(centered + delta, scale),
        )
    }
}

fn invalid_number(value: &str) -> SqlBuilderError {
    SqlBuilderError::InvalidSearchValue(format!("Invalid number: {value}"))
}

fn invalid_quantity_number(value: &str) -> SqlBuilderError {
    SqlBuilderError::InvalidSearchValue(format!("Invalid number in quantity: {value}"))
}

fn format_decimal(mantissa: i128, scale: u32) -> String {
    let negative = mantissa < 0;
    let digits = mantissa.abs().to_string();

    if scale == 0 {
        return if negative {
            format!("-{digits}")
        } else {
            digits
        };
    }

    let scale = scale as usize;
    let value = if digits.len() > scale {
        let split = digits.len() - scale;
        format!("{}.{}", &digits[..split], &digits[split..])
    } else {
        format!("0.{}{}", "0".repeat(scale - digits.len()), digits)
    };
    let trimmed = value.trim_end_matches('0').trim_end_matches('.');

    if negative && trimmed != "0" {
        format!("-{trimmed}")
    } else {
        trimmed.to_string()
    }
}

fn bind_numeric(builder: &mut SqlBuilder, value: impl Into<String>) -> usize {
    builder.add_text_param(value.into())
}

fn render_numeric_comparison(
    builder: &mut SqlBuilder,
    path: &str,
    prefix: SearchPrefix,
    number: &RenderDecimalParts,
) -> String {
    match prefix {
        SearchPrefix::Eq => {
            let (lower, upper) = number.implicit_eq_bounds();
            let p1 = bind_numeric(builder, lower);
            let p2 = bind_numeric(builder, upper);
            format!("(({path})::numeric >= ${p1}::numeric AND ({path})::numeric < ${p2}::numeric)")
        }
        SearchPrefix::Ne => {
            let (lower, upper) = number.implicit_eq_bounds();
            let p1 = bind_numeric(builder, lower);
            let p2 = bind_numeric(builder, upper);
            format!("(({path})::numeric < ${p1}::numeric OR ({path})::numeric >= ${p2}::numeric)")
        }
        SearchPrefix::Gt | SearchPrefix::Sa => {
            let p = bind_numeric(builder, number.format());
            format!("({path})::numeric > ${p}::numeric")
        }
        SearchPrefix::Lt | SearchPrefix::Eb => {
            let p = bind_numeric(builder, number.format());
            format!("({path})::numeric < ${p}::numeric")
        }
        SearchPrefix::Ge => {
            let p = bind_numeric(builder, number.format());
            format!("({path})::numeric >= ${p}::numeric")
        }
        SearchPrefix::Le => {
            let p = bind_numeric(builder, number.format());
            format!("({path})::numeric <= ${p}::numeric")
        }
        SearchPrefix::Ap => {
            let (lower, upper) = number.approximate_bounds();
            let p1 = bind_numeric(builder, lower);
            let p2 = bind_numeric(builder, upper);
            format!("(({path})::numeric >= ${p1}::numeric AND ({path})::numeric < ${p2}::numeric)")
        }
    }
}

/// Render the small SQL AST to parameterized SQL text.
pub fn render_sql_expr(expr: &SqlExpr) -> String {
    match expr {
        SqlExpr::And(parts) => render_joined(parts, " AND "),
        SqlExpr::Or(parts) => render_joined(parts, " OR "),
        SqlExpr::Not(inner) => format!("NOT ({})", render_sql_expr(inner)),
        SqlExpr::Exists(select) => render_select_exists(select),
        SqlExpr::Compare { lhs, op, rhs } => {
            format!(
                "{} {} {}",
                render_term(lhs),
                render_sql_op(*op),
                render_term(rhs)
            )
        }
        SqlExpr::RangeOp { lhs, op, rhs } => {
            format!(
                "{} {} {}",
                render_term(lhs),
                render_range_op(*op),
                render_term(rhs)
            )
        }
        SqlExpr::Raw(sql) => sql.clone(),
    }
}

fn render_joined(parts: &[SqlExpr], separator: &str) -> String {
    match parts {
        [] => String::new(),
        [only] => render_sql_expr(only),
        _ => format!(
            "({})",
            parts
                .iter()
                .map(render_sql_expr)
                .collect::<Vec<_>>()
                .join(separator)
        ),
    }
}

fn render_select_exists(select: &SelectStmt) -> String {
    format!("EXISTS ({})", select.sql)
}

fn render_term(term: &SqlTerm) -> String {
    match term {
        SqlTerm::Ident(name) => name.clone(),
        SqlTerm::Param(n) => format!("${n}"),
        SqlTerm::TimestampRange { lo, hi, bounds } => {
            format!(
                "tstzrange({}, {}, '{bounds}')",
                render_term(lo),
                render_term(hi)
            )
        }
        SqlTerm::Null => "NULL".to_string(),
    }
}

fn render_sql_op(op: SqlOp) -> &'static str {
    match op {
        SqlOp::Eq => "=",
        SqlOp::Ne => "!=",
    }
}

fn render_range_op(op: RangeOp) -> &'static str {
    match op {
        RangeOp::ContainsBy => "<@",
        RangeOp::Overlaps => "&&",
        RangeOp::StrictlyAfter => ">>",
        RangeOp::StrictlyBefore => "<<",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sql_ast_renders_range_operator_without_values() {
        let expr = SqlExpr::RangeOp {
            lhs: SqlTerm::Ident("sid.rng".to_string()),
            op: RangeOp::Overlaps,
            rhs: SqlTerm::TimestampRange {
                lo: Box::new(SqlTerm::Param(1)),
                hi: Box::new(SqlTerm::Null),
                bounds: "[)",
            },
        };

        assert_eq!(
            render_sql_expr(&expr),
            "sid.rng && tstzrange($1, NULL, '[)')"
        );
    }

    #[test]
    fn string_sidecar_render_redacts_values_into_params() {
        let mut builder = SqlBuilder::new();
        let clauses = vec![StringClause {
            resource_type: "Patient".to_string(),
            param_code: "family".to_string(),
            predicate: StringPredicate::Contains {
                value: "Sm_th%".to_string(),
            },
        }];

        let sql = render_string_clauses_as_or(&mut builder, &clauses).unwrap();

        assert!(sql.contains("search_idx_string"));
        assert!(sql.contains("sid.value_norm LIKE $3"));
        assert!(!sql.contains("Sm_th"));
        assert_eq!(builder.params()[2].as_str(), "%sm\\_th\\%%");
    }

    #[test]
    fn reference_render_preserves_default_index_plus_jsonb_fallback() {
        let mut builder = SqlBuilder::with_resource_column("r.resource");
        let clauses = ReferenceClause::from_parsed_param(
            &crate::parser::ParsedParam {
                name: "subject".to_string(),
                modifier: None,
                values: vec![crate::parser::ParsedValue {
                    prefix: None,
                    raw: "Patient/pat-123".to_string(),
                }],
            },
            "Observation",
            &["Patient".to_string(), "Group".to_string()],
        )
        .unwrap();

        let sql = render_reference_clauses_as_or(&mut builder, &clauses, "r.resource->'subject'")
            .unwrap();

        assert!(sql.contains("search_idx_reference"));
        assert!(sql.contains("sir.ref_kind = 1"));
        assert!(sql.contains("sir.target_type = $3"));
        assert!(sql.contains("sir.target_id = $4"));
        assert!(sql.contains(" OR "));
        assert!(sql.contains("r.resource->'subject'->>'reference' = $5"));
        assert!(!sql.contains("pat-123") && !sql.contains("Patient/pat-123"));
        assert_eq!(builder.params()[2].as_str(), "Patient");
        assert_eq!(builder.params()[3].as_str(), "pat-123");
        assert_eq!(builder.params()[4].as_str(), "Patient/pat-123");
    }

    #[test]
    fn reference_render_preserves_identifier_no_system_semantics() {
        let mut builder = SqlBuilder::with_resource_column("r.resource");
        let clauses = ReferenceClause::from_parsed_param(
            &crate::parser::ParsedParam {
                name: "subject".to_string(),
                modifier: Some(crate::parameters::SearchModifier::Identifier),
                values: vec![crate::parser::ParsedValue {
                    prefix: None,
                    raw: "|abc".to_string(),
                }],
            },
            "Observation",
            &["Patient".to_string()],
        )
        .unwrap();

        let sql = render_reference_clauses_as_or(&mut builder, &clauses, "r.resource->'subject'")
            .unwrap();

        assert!(sql.contains("sir.ref_kind = 4"));
        assert!(sql.contains("sir.identifier_system IS NULL"));
        assert!(sql.contains("sir.identifier_value = $3"));
        assert!(!sql.contains(" OR "));
        assert!(!sql.contains("abc"));
        assert_eq!(builder.params()[2].as_str(), "abc");
    }

    #[test]
    fn number_render_uses_half_open_decimal_bounds() {
        let mut builder = SqlBuilder::new();
        let clauses = NumberClause::from_parsed_param(
            &crate::parser::ParsedParam {
                name: "value".to_string(),
                modifier: None,
                values: vec![crate::parser::ParsedValue {
                    prefix: Some(SearchPrefix::Eq),
                    raw: "5.50".to_string(),
                }],
            },
            "Observation",
        )
        .unwrap();

        let sql = render_number_clauses_as_or(&mut builder, &clauses, "resource->>'value'")
            .unwrap()
            .unwrap();

        assert!(sql.contains(">= $1::numeric"));
        assert!(sql.contains("< $2::numeric"));
        assert!(!sql.contains("BETWEEN"));
        assert!(!sql.contains("5.50"));
        assert_eq!(builder.params()[0].as_str(), "5.495");
        assert_eq!(builder.params()[1].as_str(), "5.505");
    }

    #[test]
    fn quantity_render_uses_numeric_bounds_and_code_constraints() {
        let mut builder = SqlBuilder::new();
        let clauses = QuantityClause::from_parsed_param(
            &crate::parser::ParsedParam {
                name: "value-quantity".to_string(),
                modifier: None,
                values: vec![crate::parser::ParsedValue {
                    prefix: Some(SearchPrefix::Eq),
                    raw: "5.5|http://unitsofmeasure.org|mg".to_string(),
                }],
            },
            "Observation",
        )
        .unwrap();

        let sql =
            render_quantity_clauses_as_or(&mut builder, &clauses, "resource->'valueQuantity'")
                .unwrap()
                .unwrap();

        assert!(sql.contains("(resource->'valueQuantity'->>'value')::numeric >= $1::numeric"));
        assert!(sql.contains("(resource->'valueQuantity'->>'value')::numeric < $2::numeric"));
        assert!(sql.contains("resource->'valueQuantity'->>'system' = $3"));
        assert!(sql.contains("resource->'valueQuantity'->>'code' = $4"));
        assert!(sql.contains("resource->'valueQuantity'->>'unit' = $4"));
        assert!(!sql.contains("unitsofmeasure") && !sql.contains("mg"));
        assert_eq!(builder.params()[0].as_str(), "5.45");
        assert_eq!(builder.params()[1].as_str(), "5.55");
        assert_eq!(builder.params()[2].as_str(), "http://unitsofmeasure.org");
        assert_eq!(builder.params()[3].as_str(), "mg");
    }

    #[test]
    fn simple_code_token_render_uses_jsonb_containment() {
        let mut builder = SqlBuilder::with_resource_column("r.resource");
        let clauses = TokenClause::from_parsed_param(
            &crate::parser::ParsedParam {
                name: "gender".to_string(),
                modifier: None,
                values: vec![crate::parser::ParsedValue {
                    prefix: None,
                    raw: "female".to_string(),
                }],
            },
            "Patient",
            crate::ir::TokenIndexShape::SimpleCode,
        )
        .unwrap();

        let sql =
            render_token_simple_code_clauses_as_or(&mut builder, &clauses, &["gender".to_string()])
                .unwrap()
                .unwrap();

        assert_eq!(sql, "r.resource @> $1::jsonb");
        assert!(!sql.contains("female"));
        assert_eq!(builder.params()[0].as_str(), r#"{"gender":"female"}"#);
    }

    #[test]
    fn simple_code_token_render_not_uses_boolean_false_check() {
        let mut builder = SqlBuilder::with_resource_column("r.resource");
        let clauses = TokenClause::from_parsed_param(
            &crate::parser::ParsedParam {
                name: "gender".to_string(),
                modifier: Some(crate::parameters::SearchModifier::Not),
                values: vec![crate::parser::ParsedValue {
                    prefix: None,
                    raw: "female".to_string(),
                }],
            },
            "Patient",
            crate::ir::TokenIndexShape::SimpleCode,
        )
        .unwrap();

        let sql =
            render_token_simple_code_clauses_as_or(&mut builder, &clauses, &["gender".to_string()])
                .unwrap()
                .unwrap();

        assert_eq!(sql, "(r.resource @> $1::jsonb) = false");
        assert_eq!(builder.params()[0].as_str(), r#"{"gender":"female"}"#);
    }

    #[test]
    fn coding_token_render_preserves_system_code_as_containment() {
        let mut builder = SqlBuilder::with_resource_column("r.resource");
        let clauses = TokenClause::from_parsed_param(
            &crate::parser::ParsedParam {
                name: "code".to_string(),
                modifier: None,
                values: vec![crate::parser::ParsedValue {
                    prefix: None,
                    raw: "http://loinc.org|8480-6".to_string(),
                }],
            },
            "Observation",
            crate::ir::TokenIndexShape::Coding,
        )
        .unwrap();

        let sql = render_token_coding_clauses_as_or(&mut builder, &clauses, &["code".to_string()])
            .unwrap()
            .unwrap();

        assert_eq!(sql, "r.resource @> $1::jsonb");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&builder.params()[0].as_str()).unwrap(),
            serde_json::json!({
                "code": {
                    "coding": [{
                        "system": "http://loinc.org",
                        "code": "8480-6"
                    }]
                }
            })
        );
    }

    #[test]
    fn coding_token_render_not_uses_boolean_false_check() {
        let mut builder = SqlBuilder::with_resource_column("r.resource");
        let clauses = TokenClause::from_parsed_param(
            &crate::parser::ParsedParam {
                name: "code".to_string(),
                modifier: Some(crate::parameters::SearchModifier::Not),
                values: vec![crate::parser::ParsedValue {
                    prefix: None,
                    raw: "http://loinc.org|8480-6".to_string(),
                }],
            },
            "Observation",
            crate::ir::TokenIndexShape::Coding,
        )
        .unwrap();

        let sql = render_token_coding_clauses_as_or(&mut builder, &clauses, &["code".to_string()])
            .unwrap()
            .unwrap();

        assert_eq!(sql, "(r.resource @> $1::jsonb) = false");
        assert!(!sql.contains("NOT ("));
    }

    #[test]
    fn identifier_token_render_preserves_system_value_containment() {
        let mut builder = SqlBuilder::with_resource_column("r.resource");
        let clauses = TokenClause::from_parsed_param(
            &crate::parser::ParsedParam {
                name: "identifier".to_string(),
                modifier: None,
                values: vec![crate::parser::ParsedValue {
                    prefix: None,
                    raw: "http://test.org|debug-123".to_string(),
                }],
            },
            "Patient",
            crate::ir::TokenIndexShape::Identifier,
        )
        .unwrap();

        let sql = render_token_identifier_clauses_as_or(
            &mut builder,
            &clauses,
            "r.resource->'identifier'",
        )
        .unwrap()
        .unwrap();

        assert_eq!(sql, "r.resource->'identifier' @> $1::jsonb");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&builder.params()[0].as_str()).unwrap(),
            serde_json::json!([{
                "system": "http://test.org",
                "value": "debug-123"
            }])
        );
    }

    #[test]
    fn identifier_token_render_not_uses_boolean_false_check() {
        let mut builder = SqlBuilder::with_resource_column("r.resource");
        let clauses = TokenClause::from_parsed_param(
            &crate::parser::ParsedParam {
                name: "identifier".to_string(),
                modifier: Some(crate::parameters::SearchModifier::Not),
                values: vec![crate::parser::ParsedValue {
                    prefix: None,
                    raw: "|debug-123".to_string(),
                }],
            },
            "Patient",
            crate::ir::TokenIndexShape::Identifier,
        )
        .unwrap();

        let sql = render_token_identifier_clauses_as_or(
            &mut builder,
            &clauses,
            "r.resource->'identifier'",
        )
        .unwrap()
        .unwrap();

        assert!(sql.starts_with("(EXISTS"));
        assert!(sql.ends_with("= false"));
        assert!(!sql.contains("NOT ("));
    }

    #[test]
    fn uri_render_escapes_like_patterns() {
        let mut builder = SqlBuilder::new();
        let clauses = UriClause::from_parsed_param(
            &crate::parser::ParsedParam {
                name: "url".to_string(),
                modifier: Some(crate::parameters::SearchModifier::Below),
                values: vec![crate::parser::ParsedValue {
                    prefix: None,
                    raw: "http://example.org/100%".to_string(),
                }],
            },
            "ImplementationGuide",
        )
        .unwrap();

        let sql = render_uri_clauses_as_or(&mut builder, &clauses, "resource->>'url'").unwrap();

        assert_eq!(sql, "resource->>'url' LIKE $1");
        assert_eq!(builder.params()[0].as_str(), "http://example.org/100\\%%");
    }

    #[test]
    fn uri_array_render_uses_array_elements_text() {
        let mut builder = SqlBuilder::new();
        let clauses = UriClause::from_parsed_param(
            &crate::parser::ParsedParam {
                name: "_profile".to_string(),
                modifier: None,
                values: vec![crate::parser::ParsedValue {
                    prefix: None,
                    raw: "http://hl7.org/fhir/us/core/Patient".to_string(),
                }],
            },
            "Patient",
        )
        .unwrap();

        let sql =
            render_uri_array_clauses_as_or(&mut builder, &clauses, "resource->'meta'->'profile'")
                .unwrap();

        assert!(sql.contains("jsonb_array_elements_text(resource->'meta'->'profile')"));
        assert!(sql.contains("uri = $1"));
    }
}
