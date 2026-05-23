use crate::ir::sql::{RangeOp, SelectStmt, SqlExpr, SqlOp, SqlTerm};
use crate::sql_builder::SqlBuilder;
use crate::types::date_ast::DateClause;

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
}
