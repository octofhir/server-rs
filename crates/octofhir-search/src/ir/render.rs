use crate::ir::ast::{ReferenceClause, ReferencePredicate, StringClause, StringPredicate};
use crate::ir::sql::{RangeOp, SelectStmt, SqlExpr, SqlOp, SqlTerm};
use crate::sql_builder::SqlBuilder;
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
}
