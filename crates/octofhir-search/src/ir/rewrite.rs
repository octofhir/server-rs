use crate::ir::ast::SearchExpr;
use crate::types::date_ast::{DateClause, merge_overlap_windows};

/// Rewrite date predicates into planner-friendly canonical form.
pub fn rewrite_date_clauses(clauses: Vec<DateClause>) -> Vec<DateClause> {
    merge_overlap_windows(clauses)
}

/// Placeholder for whole-tree rewrite passes.
///
/// Kept explicit so future phases can add token/composite rewrites without
/// coupling them to SQL rendering.
pub fn rewrite_search_expr(expr: SearchExpr) -> SearchExpr {
    match expr {
        SearchExpr::And(children) => {
            SearchExpr::And(children.into_iter().map(rewrite_search_expr).collect())
        }
        SearchExpr::Or(children) => {
            SearchExpr::Or(children.into_iter().map(rewrite_search_expr).collect())
        }
        SearchExpr::Not(inner) => SearchExpr::Not(Box::new(rewrite_search_expr(*inner))),
        SearchExpr::Param(param) => SearchExpr::Param(param),
    }
}
