use crate::ir::ast::{StringClause, StringPredicate};
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
}
