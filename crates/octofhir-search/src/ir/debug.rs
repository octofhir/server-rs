use crate::ir::strategy::IndexStrategy;
use crate::parameters::SearchParameterType;
use crate::types::date_ast::{Bound, DateClause, DatePredicate};
use serde::{Deserialize, Serialize};

/// Safe, serializable search plan debug model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchDebugPlan {
    pub resource_type: String,
    pub predicates: Vec<DebugPredicate>,
}

/// One rendered predicate annotation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DebugPredicate {
    pub param_code: String,
    pub search_type: SearchParameterType,
    pub strategy: IndexStrategy,
    pub expected_index: Option<String>,
    pub index_backed: bool,
    pub sql_shape: String,
}

impl SearchDebugPlan {
    pub fn new(resource_type: impl Into<String>) -> Self {
        Self {
            resource_type: resource_type.into(),
            predicates: Vec::new(),
        }
    }
}

/// Build safe debug output for date sidecar predicates.
///
/// SQL shapes intentionally use symbolic bind names instead of actual values.
pub fn build_date_debug_plan(resource_type: &str, clauses: &[DateClause]) -> SearchDebugPlan {
    let mut plan = SearchDebugPlan::new(resource_type);
    plan.predicates = clauses.iter().map(debug_date_clause).collect();
    plan
}

fn debug_date_clause(clause: &DateClause) -> DebugPredicate {
    DebugPredicate {
        param_code: clause.param_code.clone(),
        search_type: SearchParameterType::Date,
        strategy: IndexStrategy::SidecarDate,
        expected_index: Some("search_idx_date_*_param_code_rng_idx".to_string()),
        index_backed: true,
        sql_shape: date_sql_shape(&clause.predicate),
    }
}

fn date_sql_shape(predicate: &DatePredicate) -> String {
    match predicate {
        DatePredicate::Contains { .. } => {
            "EXISTS search_idx_date WHERE sid.rng <@ tstzrange($lo, $hi, '[)')".to_string()
        }
        DatePredicate::NotContains { .. } => {
            "NOT EXISTS search_idx_date WHERE sid.rng <@ tstzrange($lo, $hi, '[)')".to_string()
        }
        DatePredicate::Overlap { lo, hi } => {
            let lo_expr = debug_bound_expr(*lo, "$lo", "NULL");
            let hi_expr = debug_bound_expr(*hi, "$hi", "NULL");
            let bounds = debug_bounds_token(*lo, *hi);
            format!(
                "EXISTS search_idx_date WHERE sid.rng && tstzrange({lo_expr}, {hi_expr}, '{bounds}')"
            )
        }
        DatePredicate::StrictlyAfter { .. } => {
            "EXISTS search_idx_date WHERE sid.rng >> tstzrange($lo, $hi, '[)')".to_string()
        }
        DatePredicate::StrictlyBefore { .. } => {
            "EXISTS search_idx_date WHERE sid.rng << tstzrange($lo, $hi, '[)')".to_string()
        }
    }
}

fn debug_bound_expr(
    bound: Option<Bound>,
    present: &'static str,
    missing: &'static str,
) -> &'static str {
    if bound.is_some() { present } else { missing }
}

fn debug_bounds_token(lo: Option<Bound>, hi: Option<Bound>) -> &'static str {
    let lo_inc = lo.map(|b| b.inclusive).unwrap_or(true);
    let hi_inc = hi.map(|b| b.inclusive).unwrap_or(false);
    match (lo_inc, hi_inc) {
        (true, true) => "[]",
        (true, false) => "[)",
        (false, true) => "(]",
        (false, false) => "()",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parameters::SearchPrefix;
    use crate::parser::{ParsedParam, ParsedValue};

    fn parsed(prefix: SearchPrefix, raw: &str) -> ParsedParam {
        ParsedParam {
            name: "birthdate".to_string(),
            modifier: None,
            values: vec![ParsedValue {
                prefix: Some(prefix),
                raw: raw.to_string(),
            }],
        }
    }

    #[test]
    fn date_debug_plan_is_serializable_and_redacted() {
        let clauses =
            DateClause::from_parsed_param(&parsed(SearchPrefix::Eq, "2000-06-15"), "Patient")
                .unwrap();

        let plan = build_date_debug_plan("Patient", &clauses);
        let json = serde_json::to_string(&plan).unwrap();

        assert!(json.contains("Patient"));
        assert!(json.contains("birthdate"));
        assert!(json.contains("sidecar_date"));
        assert!(json.contains("search_idx_date_*_param_code_rng_idx"));
        assert!(json.contains("sid.rng <@ tstzrange($lo, $hi, '[)')"));
        assert!(
            !json.contains("2000-06-15"),
            "debug output must not include bound date values: {json}"
        );
    }

    #[test]
    fn date_debug_plan_shows_half_infinite_overlap_shape() {
        let clauses =
            DateClause::from_parsed_param(&parsed(SearchPrefix::Ge, "2000-06-15"), "Patient")
                .unwrap();

        let plan = build_date_debug_plan("Patient", &clauses);
        assert_eq!(plan.predicates.len(), 1);
        assert_eq!(
            plan.predicates[0].sql_shape,
            "EXISTS search_idx_date WHERE sid.rng && tstzrange($lo, NULL, '[)')"
        );
    }
}
