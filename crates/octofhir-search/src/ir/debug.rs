use crate::ir::ast::{
    CompositeClause, CompositePredicate, CompositeSafety, NumberClause, NumberPredicate,
    QuantityClause, QuantityPredicate, ReferenceClause, ReferencePredicate, StringClause,
    StringPredicate, TokenClause, TokenIndexShape, TokenPredicate, TokenSetModifier,
};
use crate::ir::strategy::IndexStrategy;
use crate::parameters::{SearchParameterType, SearchPrefix};
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

/// Symbolic form of the functional date-range expression the in-place date
/// predicates (and the matching GiST functional index) are built on.
const DATE_RANGE_EXPR: &str = "tstzrange(fhir_extract_date_min(resource, paths), fhir_extract_date_max(resource, paths), '[]')";

/// Symbolic form of the normalized text-blob expression the in-place string
/// predicates (and the matching trigram GIN functional index) are built on.
const STRING_BLOB_EXPR: &str = "fhir_text_blob(fhir_extract_text(resource, paths))";

/// Symbolic form of the raw extracted text array used for `:exact`/`:missing`.
const STRING_ARRAY_EXPR: &str = "fhir_extract_text(resource, paths)";

/// Name of the per-table `GIN (resource jsonb_path_ops)` containment index.
fn gin_index_name(resource_type: &str) -> String {
    format!("idx_{}_gin", resource_type.to_lowercase())
}

/// Name of the bootstrap-created GiST functional date index for one param.
fn date_index_name(resource_type: &str, param_code: &str) -> String {
    format!("idx_{}_{param_code}_date", resource_type.to_lowercase())
}

/// Name of the bootstrap-created trigram GIN functional string index.
fn string_index_name(resource_type: &str, param_code: &str) -> String {
    format!("idx_{}_{param_code}_str", resource_type.to_lowercase())
}

/// Build safe debug output for in-place date predicates over the functional
/// date-range expression on the resource JSONB.
///
/// SQL shapes intentionally use symbolic bind names instead of actual values.
pub fn build_date_debug_plan(resource_type: &str, clauses: &[DateClause]) -> SearchDebugPlan {
    let mut plan = SearchDebugPlan::new(resource_type);
    plan.predicates = clauses.iter().map(debug_date_clause).collect();
    plan
}

/// Build safe debug output for in-place string predicates over the normalized
/// text-blob expression on the resource JSONB.
///
/// SQL shapes intentionally use symbolic bind names instead of actual values.
pub fn build_string_debug_plan(resource_type: &str, clauses: &[StringClause]) -> SearchDebugPlan {
    let mut plan = SearchDebugPlan::new(resource_type);
    plan.predicates = clauses.iter().map(debug_string_clause).collect();
    plan
}

/// Build safe debug output for number predicates.
///
/// Number search uses in-place JSONB numeric casts. Mark it explicitly as
/// non-index-backed so debug consumers do not mistake it for an optimized
/// plan.
pub fn build_number_debug_plan(resource_type: &str, clauses: &[NumberClause]) -> SearchDebugPlan {
    let mut plan = SearchDebugPlan::new(resource_type);
    plan.predicates = clauses.iter().map(debug_number_clause).collect();
    plan
}

/// Build safe debug output for quantity predicates.
///
/// Quantity search uses in-place JSONB numeric casts. When system/code
/// constraints are present, runtime also adds full-resource JSONB containment
/// so the generic resource GIN index can prefilter before numeric comparison.
pub fn build_quantity_debug_plan(
    resource_type: &str,
    clauses: &[QuantityClause],
) -> SearchDebugPlan {
    let mut plan = SearchDebugPlan::new(resource_type);
    plan.predicates = clauses
        .iter()
        .map(|clause| debug_quantity_clause(resource_type, clause))
        .collect();
    plan
}

/// Build safe debug output for composite predicates.
///
/// Composite search uses independent in-place JSONB component predicates. The
/// debug shape preserves tuple intent and marks co-occurrence risk explicitly.
pub fn build_composite_debug_plan(
    resource_type: &str,
    clauses: &[CompositeClause],
) -> SearchDebugPlan {
    let mut plan = SearchDebugPlan::new(resource_type);
    plan.predicates = clauses.iter().map(debug_composite_clause).collect();
    plan
}

/// Build safe debug output for the string `:text` fallback path.
///
/// Narrative text has no functional index, so this path is deliberately
/// marked as JSONB traversal and non-index-backed.
pub fn build_string_text_debug_predicate(param_code: &str) -> DebugPredicate {
    DebugPredicate {
        param_code: param_code.to_string(),
        search_type: SearchParameterType::String,
        strategy: IndexStrategy::JsonbTraversal,
        expected_index: None,
        index_backed: false,
        sql_shape: "to_tsvector(resource->>'text') @@ plainto_tsquery($text)".to_string(),
    }
}

/// Build safe debug output for token predicates.
///
/// SQL shapes intentionally preserve token form while redacting PHI-bearing
/// values and terminology URLs.
pub fn build_token_debug_plan(resource_type: &str, clauses: &[TokenClause]) -> SearchDebugPlan {
    let mut plan = SearchDebugPlan::new(resource_type);
    plan.predicates = clauses.iter().map(debug_token_clause).collect();
    plan
}

/// Build safe debug output for reference predicates.
///
/// SQL shapes intentionally distinguish local, external, identifier, and
/// missing forms while redacting all reference/identifier values.
pub fn build_reference_debug_plan(
    resource_type: &str,
    clauses: &[ReferenceClause],
) -> SearchDebugPlan {
    let mut plan = SearchDebugPlan::new(resource_type);
    plan.predicates = clauses
        .iter()
        .map(|clause| debug_reference_clause(resource_type, clause))
        .collect();
    plan
}

fn debug_date_clause(clause: &DateClause) -> DebugPredicate {
    let (strategy, expected_index, index_backed) = match &clause.predicate {
        // `:missing` checks `fhir_extract_date_min(...) IS [NOT] NULL`, which
        // the GiST range index does not serve.
        DatePredicate::Missing { .. } => (IndexStrategy::JsonbTraversal, None, false),
        _ => (
            IndexStrategy::JsonbExpressionIndex,
            Some(date_index_name(&clause.resource_type, &clause.param_code)),
            true,
        ),
    };

    DebugPredicate {
        param_code: clause.param_code.clone(),
        search_type: SearchParameterType::Date,
        strategy,
        expected_index,
        index_backed,
        sql_shape: date_sql_shape(&clause.predicate),
    }
}

fn date_sql_shape(predicate: &DatePredicate) -> String {
    match predicate {
        DatePredicate::Contains { .. } => {
            format!("{DATE_RANGE_EXPR} <@ tstzrange($lo, $hi, '[)')")
        }
        DatePredicate::NotContains { .. } => {
            format!("NOT ({DATE_RANGE_EXPR} <@ tstzrange($lo, $hi, '[)'))")
        }
        DatePredicate::Overlap { lo, hi } => {
            let lo_expr = debug_bound_expr(*lo, "$lo", "NULL");
            let hi_expr = debug_bound_expr(*hi, "$hi", "NULL");
            let bounds = debug_bounds_token(*lo, *hi);
            format!("{DATE_RANGE_EXPR} && tstzrange({lo_expr}, {hi_expr}, '{bounds}')")
        }
        DatePredicate::Ge { .. } => {
            format!(
                "{DATE_RANGE_EXPR} && tstzrange($hi, NULL, '[)') OR {DATE_RANGE_EXPR} <@ tstzrange($lo, $hi, '[)')"
            )
        }
        DatePredicate::Le { .. } => {
            format!(
                "{DATE_RANGE_EXPR} && tstzrange(NULL, $lo, '[)') OR {DATE_RANGE_EXPR} <@ tstzrange($lo, $hi, '[)')"
            )
        }
        DatePredicate::StrictlyAfter { .. } => {
            format!("lower({DATE_RANGE_EXPR}) > $hi")
        }
        DatePredicate::StrictlyBefore { .. } => {
            format!("upper({DATE_RANGE_EXPR}) < $lo")
        }
        DatePredicate::Missing { is_missing } => {
            if *is_missing {
                "fhir_extract_date_min(resource, paths) IS NULL".to_string()
            } else {
                "fhir_extract_date_min(resource, paths) IS NOT NULL".to_string()
            }
        }
    }
}

fn debug_string_clause(clause: &StringClause) -> DebugPredicate {
    let trgm_index = || {
        Some(string_index_name(
            &clause.resource_type,
            &clause.param_code,
        ))
    };
    let (strategy, expected_index, sql_shape) = match &clause.predicate {
        StringPredicate::Prefix { .. } => (
            IndexStrategy::JsonbExpressionIndex,
            trgm_index(),
            format!("{STRING_BLOB_EXPR} LIKE $prefix"),
        ),
        StringPredicate::Contains { .. } => (
            IndexStrategy::JsonbExpressionIndex,
            trgm_index(),
            format!("{STRING_BLOB_EXPR} LIKE $contains"),
        ),
        // `:text` is approximated as a substring match over the same
        // normalized text blob (and served by the same trigram index).
        StringPredicate::Text { .. } => (
            IndexStrategy::JsonbExpressionIndex,
            trgm_index(),
            format!("{STRING_BLOB_EXPR} LIKE $text"),
        ),
        // `:exact` compares raw extracted values; the trigram index on the
        // normalized blob does not serve it.
        StringPredicate::Exact { .. } => (
            IndexStrategy::JsonbTraversal,
            None,
            format!("$exact = ANY({STRING_ARRAY_EXPR})"),
        ),
        StringPredicate::Missing { is_missing } => {
            let shape = if *is_missing {
                format!("{STRING_ARRAY_EXPR} IS NULL")
            } else {
                format!("{STRING_ARRAY_EXPR} IS NOT NULL")
            };
            (IndexStrategy::JsonbTraversal, None, shape)
        }
    };

    let index_backed = expected_index.is_some();

    DebugPredicate {
        param_code: clause.param_code.clone(),
        search_type: SearchParameterType::String,
        strategy,
        expected_index,
        index_backed,
        sql_shape,
    }
}

fn debug_number_clause(clause: &NumberClause) -> DebugPredicate {
    DebugPredicate {
        param_code: clause.param_code.clone(),
        search_type: SearchParameterType::Number,
        strategy: IndexStrategy::JsonbTraversal,
        expected_index: None,
        index_backed: false,
        sql_shape: number_sql_shape(&clause.predicate),
    }
}

fn number_sql_shape(predicate: &NumberPredicate) -> String {
    match predicate {
        NumberPredicate::Comparison { prefix, .. } => match prefix {
            SearchPrefix::Eq | SearchPrefix::Ap => {
                "(resource->>path)::numeric >= $lo AND (resource->>path)::numeric < $hi"
                    .to_string()
            }
            SearchPrefix::Ne => {
                "NOT ((resource->>path)::numeric >= $lo AND (resource->>path)::numeric < $hi)"
                    .to_string()
            }
            SearchPrefix::Gt | SearchPrefix::Sa => {
                "(resource->>path)::numeric > $value".to_string()
            }
            SearchPrefix::Lt | SearchPrefix::Eb => {
                "(resource->>path)::numeric < $value".to_string()
            }
            SearchPrefix::Ge => "(resource->>path)::numeric >= $value".to_string(),
            SearchPrefix::Le => "(resource->>path)::numeric <= $value".to_string(),
        },
        NumberPredicate::Missing { is_missing } => {
            if *is_missing {
                "resource->path IS NULL".to_string()
            } else {
                "resource->path IS NOT NULL".to_string()
            }
        }
    }
}

fn debug_quantity_clause(resource_type: &str, clause: &QuantityClause) -> DebugPredicate {
    let (strategy, expected_index, index_backed, sql_shape) = match &clause.predicate {
        QuantityPredicate::Comparison { system, code, .. }
            if system.is_some() || code.is_some() =>
        {
            (
                IndexStrategy::JsonbContainment,
                Some(gin_index_name(resource_type)),
                true,
                quantity_sql_shape(&clause.predicate),
            )
        }
        _ => (
            IndexStrategy::JsonbTraversal,
            None,
            false,
            quantity_sql_shape(&clause.predicate),
        ),
    };

    DebugPredicate {
        param_code: clause.param_code.clone(),
        search_type: SearchParameterType::Quantity,
        strategy,
        expected_index,
        index_backed,
        sql_shape,
    }
}

fn quantity_sql_shape(predicate: &QuantityPredicate) -> String {
    match predicate {
        QuantityPredicate::Comparison {
            prefix,
            system,
            code,
            ..
        } => {
            let value_expr = "(resource->path->>'value')::numeric";
            let mut shape = match prefix {
                SearchPrefix::Eq | SearchPrefix::Ap => {
                    format!("{value_expr} >= $lo AND {value_expr} < $hi")
                }
                SearchPrefix::Ne => {
                    format!("NOT ({value_expr} >= $lo AND {value_expr} < $hi)")
                }
                SearchPrefix::Gt | SearchPrefix::Sa => format!("{value_expr} > $value"),
                SearchPrefix::Lt | SearchPrefix::Eb => format!("{value_expr} < $value"),
                SearchPrefix::Ge => format!("{value_expr} >= $value"),
                SearchPrefix::Le => format!("{value_expr} <= $value"),
            };

            match (system.is_some(), code.is_some()) {
                (false, false) => {}
                (true, false) => {
                    shape.push_str(" AND resource @> {path: {system: $system}}");
                }
                (false, true) => {
                    shape.push_str(
                        " AND (resource @> {path: {code: $code}} OR resource @> {path: {unit: $code}})",
                    );
                }
                (true, true) => {
                    shape.push_str(
                        " AND (resource @> {path: {system: $system, code: $code}} OR resource @> {path: {system: $system, unit: $code}})",
                    );
                }
            }
            shape
        }
        QuantityPredicate::Missing { is_missing } => {
            if *is_missing {
                "resource->path IS NULL".to_string()
            } else {
                "resource->path IS NOT NULL".to_string()
            }
        }
    }
}

fn debug_composite_clause(clause: &CompositeClause) -> DebugPredicate {
    let (index_backed, sql_shape) = match &clause.predicate {
        CompositePredicate::Tuple { safety, .. } => (
            false,
            format!(
                "composite tuple via independent JSONB component predicates ({})",
                composite_safety_name(*safety)
            ),
        ),
        CompositePredicate::Missing { is_missing } => {
            let shape = if *is_missing {
                "composite tuple path IS NULL"
            } else {
                "composite tuple path IS NOT NULL"
            };
            (false, shape.to_string())
        }
    };

    DebugPredicate {
        param_code: clause.param_code.clone(),
        search_type: SearchParameterType::Composite,
        strategy: IndexStrategy::JsonbTraversal,
        expected_index: None,
        index_backed,
        sql_shape,
    }
}

fn composite_safety_name(safety: CompositeSafety) -> &'static str {
    match safety {
        CompositeSafety::SafeIndependent => "safe-independent",
        CompositeSafety::RequiresSameElement => "requires-same-element",
        CompositeSafety::Unsupported => "unsupported",
    }
}

fn debug_token_clause(clause: &TokenClause) -> DebugPredicate {
    let strategy = token_strategy(clause);
    let expected_index = token_expected_index(clause);
    let index_backed = token_index_backed(clause);
    let mut sql_shape = token_sql_shape(clause);
    if clause.negated {
        sql_shape = format!("({sql_shape}) = false");
    }

    DebugPredicate {
        param_code: clause.param_code.clone(),
        search_type: SearchParameterType::Token,
        strategy,
        expected_index,
        index_backed,
        sql_shape,
    }
}

fn debug_reference_clause(resource_type: &str, clause: &ReferenceClause) -> DebugPredicate {
    let traversal = |shape: &str| {
        (
            IndexStrategy::JsonbTraversal,
            None,
            false,
            shape.to_string(),
        )
    };
    let (strategy, expected_index, index_backed, sql_shape) = match &clause.predicate {
        // Local/external references render as in-place JSONB array traversal
        // over the reference element; not index-backed.
        ReferencePredicate::Local { target_type, .. } => {
            if target_type.is_some() {
                traversal(
                    "EXISTS jsonb_array_elements(resource->path) AS ref WHERE ref->>'reference' = $target_type/$target_id",
                )
            } else {
                traversal(
                    "EXISTS jsonb_array_elements(resource->path) AS ref WHERE ref->>'reference' = $target_id",
                )
            }
        }
        ReferencePredicate::External { .. } => traversal(
            "EXISTS jsonb_array_elements(resource->path) AS ref WHERE ref->>'reference' = $url",
        ),
        // `:identifier` matches the embedded identifier element via full
        // resource containment, served by the generic resource GIN index.
        ReferencePredicate::Identifier {
            system,
            require_no_system,
            ..
        } => {
            if system.is_some() {
                (
                    IndexStrategy::JsonbContainment,
                    Some(gin_index_name(resource_type)),
                    true,
                    "resource @> {path: {identifier: [{system: $system, value: $value}]}}"
                        .to_string(),
                )
            } else if *require_no_system {
                traversal(
                    "EXISTS identifier WHERE ident.system IS NULL AND ident.value = $value",
                )
            } else {
                (
                    IndexStrategy::JsonbContainment,
                    Some(gin_index_name(resource_type)),
                    true,
                    "resource @> {path: {identifier: [{value: $value}]}}".to_string(),
                )
            }
        }
        ReferencePredicate::Missing { is_missing } => {
            if *is_missing {
                traversal("resource->path IS NULL OR resource->path = '[]'::jsonb")
            } else {
                traversal("resource->path IS NOT NULL AND resource->path != '[]'::jsonb")
            }
        }
    };

    DebugPredicate {
        param_code: clause.param_code.clone(),
        search_type: SearchParameterType::Reference,
        strategy,
        expected_index,
        index_backed,
        sql_shape,
    }
}

fn token_strategy(clause: &TokenClause) -> IndexStrategy {
    if token_uses_containment_shape(clause) {
        IndexStrategy::JsonbContainment
    } else {
        IndexStrategy::JsonbTraversal
    }
}

fn token_expected_index(clause: &TokenClause) -> Option<String> {
    if token_index_backed(clause) {
        Some(gin_index_name(&clause.resource_type))
    } else {
        None
    }
}

fn token_index_backed(clause: &TokenClause) -> bool {
    if clause.negated {
        return false;
    }
    token_uses_containment_shape(clause)
}

fn token_uses_containment_shape(clause: &TokenClause) -> bool {
    match (&clause.index_shape, &clause.predicate) {
        (_, TokenPredicate::DisplayText { .. })
        | (_, TokenPredicate::Missing { .. })
        | (_, TokenPredicate::TerminologySet { .. })
        | (TokenIndexShape::Coding, TokenPredicate::NoSystemCode { .. })
        | (TokenIndexShape::Identifier, TokenPredicate::NoSystemCode { .. })
        | (TokenIndexShape::SimpleCode, TokenPredicate::SystemAnyCode { .. }) => false,
        (
            TokenIndexShape::Identifier,
            TokenPredicate::AnySystemCode { .. }
            | TokenPredicate::SystemAnyCode { .. }
            | TokenPredicate::SystemCode { .. }
            | TokenPredicate::IdentifierOfType { .. },
        ) => true,
        (
            TokenIndexShape::SimpleCode,
            TokenPredicate::AnySystemCode { .. }
            | TokenPredicate::NoSystemCode { .. }
            | TokenPredicate::SystemCode { .. },
        ) => true,
        (TokenIndexShape::SimpleCode, _) => false,
        (TokenIndexShape::Coding, TokenPredicate::AnySystemCode { .. })
        | (TokenIndexShape::Coding, TokenPredicate::SystemCode { .. }) => true,
        (TokenIndexShape::Coding, _) => false,
    }
}

fn token_sql_shape(clause: &TokenClause) -> String {
    match &clause.predicate {
        TokenPredicate::AnySystemCode { .. } => match clause.index_shape {
            TokenIndexShape::SimpleCode => "resource @> {code: $code}".to_string(),
            TokenIndexShape::Identifier => {
                "resource @> {identifier: [{value: $code}]}".to_string()
            }
            TokenIndexShape::Coding => {
                "resource @> {coding: [{code: $code}]} OR resource @> {code: $code}".to_string()
            }
        },
        TokenPredicate::NoSystemCode { .. } => match clause.index_shape {
            TokenIndexShape::Identifier => {
                "EXISTS identifier WHERE ident.system IS NULL AND ident.value = $code".to_string()
            }
            TokenIndexShape::SimpleCode => "resource @> {code: $code}".to_string(),
            TokenIndexShape::Coding => {
                "EXISTS coding WHERE coding.system IS NULL AND coding.code = $code".to_string()
            }
        },
        TokenPredicate::SystemAnyCode { .. } => match clause.index_shape {
            TokenIndexShape::Identifier => {
                "resource @> {identifier: [{system: $system}]}".to_string()
            }
            TokenIndexShape::SimpleCode => "FALSE".to_string(),
            TokenIndexShape::Coding => "EXISTS coding WHERE coding.system = $system".to_string(),
        },
        TokenPredicate::SystemCode { .. } => match clause.index_shape {
            TokenIndexShape::Identifier => "resource @> {identifier: [{system: $system, value: $code}]}".to_string(),
            TokenIndexShape::SimpleCode => "resource @> {code: $code}".to_string(),
            TokenIndexShape::Coding => {
                "resource @> {coding: [{system: $system, code: $code}]}".to_string()
            }
        },
        TokenPredicate::IdentifierOfType { .. } => {
            "resource @> {identifier: [{type: {coding: [{system: $system, code: $code}]}, value: $value}]}".to_string()
        }
        TokenPredicate::TerminologySet { modifier, .. } => {
            format!("terminology {} expansion over token code", token_set_modifier_name(*modifier))
        }
        TokenPredicate::DisplayText { .. } => {
            "EXISTS coding WHERE LOWER(coding.display) LIKE LOWER($text)".to_string()
        }
        TokenPredicate::Missing { is_missing } => {
            if *is_missing {
                "token path IS NULL".to_string()
            } else {
                "token path IS NOT NULL".to_string()
            }
        }
    }
}

fn token_set_modifier_name(modifier: TokenSetModifier) -> &'static str {
    match modifier {
        TokenSetModifier::In => ":in",
        TokenSetModifier::NotIn => ":not-in",
        TokenSetModifier::Below => ":below",
        TokenSetModifier::Above => ":above",
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
        assert!(json.contains("jsonb_expression_index"));
        assert!(json.contains("idx_patient_birthdate_date"));
        assert!(json.contains("tstzrange(fhir_extract_date_min(resource, paths)"));
        assert!(json.contains("<@ tstzrange($lo, $hi, '[)')"));
        assert!(plan.predicates[0].index_backed);
        assert!(
            !json.contains("2000-06-15"),
            "debug output must not include bound date values: {json}"
        );
    }

    #[test]
    fn date_debug_plan_shows_half_infinite_overlap_shape() {
        let clauses =
            DateClause::from_parsed_param(&parsed(SearchPrefix::Gt, "2000-06-15"), "Patient")
                .unwrap();

        let plan = build_date_debug_plan("Patient", &clauses);
        assert_eq!(plan.predicates.len(), 1);
        assert_eq!(
            plan.predicates[0].sql_shape,
            format!("{DATE_RANGE_EXPR} && tstzrange($lo, NULL, '[)')")
        );
        assert_eq!(
            plan.predicates[0].strategy,
            IndexStrategy::JsonbExpressionIndex
        );
    }

    #[test]
    fn string_debug_plan_is_serializable_and_redacted() {
        let clauses = vec![StringClause {
            resource_type: "Patient".to_string(),
            param_code: "family".to_string(),
            predicate: StringPredicate::Contains {
                value: "Smíth".to_string(),
            },
        }];

        let plan = build_string_debug_plan("Patient", &clauses);
        let json = serde_json::to_string(&plan).unwrap();

        assert!(json.contains("Patient"));
        assert!(json.contains("family"));
        assert!(json.contains("jsonb_expression_index"));
        assert!(json.contains("fhir_text_blob(fhir_extract_text(resource, paths)) LIKE $contains"));
        assert!(json.contains("idx_patient_family_str"));
        assert!(plan.predicates[0].index_backed);
        assert!(
            !json.contains("Smíth") && !json.contains("smith"),
            "debug output must not include bound string values: {json}"
        );
    }

    #[test]
    fn string_exact_debug_plan_is_traversal_over_raw_values() {
        let clauses = vec![StringClause {
            resource_type: "Patient".to_string(),
            param_code: "family".to_string(),
            predicate: StringPredicate::Exact {
                value: "Smith".to_string(),
            },
        }];

        let plan = build_string_debug_plan("Patient", &clauses);

        assert_eq!(plan.predicates[0].strategy, IndexStrategy::JsonbTraversal);
        assert!(!plan.predicates[0].index_backed);
        assert_eq!(plan.predicates[0].expected_index, None);
        assert_eq!(
            plan.predicates[0].sql_shape,
            "$exact = ANY(fhir_extract_text(resource, paths))"
        );
    }

    #[test]
    fn string_text_debug_predicate_is_non_index_backed_and_redacted() {
        let predicate = build_string_text_debug_predicate("name");
        let json = serde_json::to_string(&predicate).unwrap();

        assert_eq!(predicate.search_type, SearchParameterType::String);
        assert_eq!(predicate.strategy, IndexStrategy::JsonbTraversal);
        assert!(!predicate.index_backed);
        assert_eq!(predicate.expected_index, None);
        assert!(predicate.sql_shape.contains("to_tsvector"));
        assert!(
            !json.contains("blood pressure"),
            "debug output must not include bound string text: {json}"
        );
    }

    #[test]
    fn number_debug_plan_marks_jsonb_traversal_and_redacts_values() {
        let clauses = vec![NumberClause {
            resource_type: "Observation".to_string(),
            param_code: "value".to_string(),
            predicate: NumberPredicate::Comparison {
                prefix: SearchPrefix::Ge,
                value: "123.45".to_string(),
            },
        }];

        let plan = build_number_debug_plan("Observation", &clauses);
        let json = serde_json::to_string(&plan).unwrap();

        assert_eq!(plan.predicates[0].search_type, SearchParameterType::Number);
        assert_eq!(plan.predicates[0].strategy, IndexStrategy::JsonbTraversal);
        assert!(!plan.predicates[0].index_backed);
        assert_eq!(plan.predicates[0].expected_index, None);
        assert_eq!(
            plan.predicates[0].sql_shape,
            "(resource->>path)::numeric >= $value"
        );
        assert!(
            !json.contains("123.45"),
            "debug output must not include bound number values: {json}"
        );
    }

    #[test]
    fn quantity_debug_plan_marks_gin_containment_and_redacts_values() {
        let clauses = vec![QuantityClause {
            resource_type: "Observation".to_string(),
            param_code: "value-quantity".to_string(),
            predicate: QuantityPredicate::Comparison {
                prefix: SearchPrefix::Eq,
                value: "5.5".to_string(),
                system: Some("http://unitsofmeasure.org".to_string()),
                code: Some("mg".to_string()),
            },
        }];

        let plan = build_quantity_debug_plan("Observation", &clauses);
        let json = serde_json::to_string(&plan).unwrap();

        assert_eq!(
            plan.predicates[0].search_type,
            SearchParameterType::Quantity
        );
        assert_eq!(plan.predicates[0].strategy, IndexStrategy::JsonbContainment);
        assert!(plan.predicates[0].index_backed);
        assert_eq!(
            plan.predicates[0].expected_index,
            Some("idx_observation_gin".to_string())
        );
        assert!(
            plan.predicates[0]
                .sql_shape
                .contains("(resource->path->>'value')::numeric >= $lo")
        );
        assert!(
            plan.predicates[0]
                .sql_shape
                .contains("resource @> {path: {system: $system, code: $code}}")
        );
        assert!(
            plan.predicates[0]
                .sql_shape
                .contains("resource @> {path: {system: $system, unit: $code}}")
        );
        assert!(
            !json.contains("5.5") && !json.contains("unitsofmeasure") && !json.contains("mg"),
            "debug output must not include bound quantity values: {json}"
        );
    }

    #[test]
    fn quantity_debug_plan_without_units_is_pure_traversal() {
        let clauses = vec![QuantityClause {
            resource_type: "Observation".to_string(),
            param_code: "value-quantity".to_string(),
            predicate: QuantityPredicate::Comparison {
                prefix: SearchPrefix::Gt,
                value: "5.5".to_string(),
                system: None,
                code: None,
            },
        }];

        let plan = build_quantity_debug_plan("Observation", &clauses);

        assert_eq!(plan.predicates[0].strategy, IndexStrategy::JsonbTraversal);
        assert!(!plan.predicates[0].index_backed);
        assert_eq!(plan.predicates[0].expected_index, None);
        assert_eq!(
            plan.predicates[0].sql_shape,
            "(resource->path->>'value')::numeric > $value"
        );
    }

    #[test]
    fn composite_debug_plan_marks_tuple_risk_and_redacts_values() {
        let clauses = vec![CompositeClause {
            resource_type: "Observation".to_string(),
            param_code: "code-value-quantity".to_string(),
            predicate: CompositePredicate::Tuple {
                safety: CompositeSafety::RequiresSameElement,
                components: Vec::new(),
            },
        }];

        let plan = build_composite_debug_plan("Observation", &clauses);
        let json = serde_json::to_string(&plan).unwrap();

        assert_eq!(
            plan.predicates[0].search_type,
            SearchParameterType::Composite
        );
        assert_eq!(plan.predicates[0].strategy, IndexStrategy::JsonbTraversal);
        assert!(!plan.predicates[0].index_backed);
        assert_eq!(plan.predicates[0].expected_index, None);
        assert!(
            plan.predicates[0]
                .sql_shape
                .contains("requires-same-element")
        );
        assert!(
            !json.contains("8480-6") && !json.contains("mg"),
            "debug output must not include composite component values: {json}"
        );
    }

    #[test]
    fn token_debug_plan_is_serializable_and_redacted() {
        let clauses = vec![TokenClause {
            resource_type: "Observation".to_string(),
            param_code: "code".to_string(),
            predicate: TokenPredicate::SystemCode {
                system: "http://loinc.org".to_string(),
                code: "8480-6".to_string(),
            },
            negated: false,
            index_shape: TokenIndexShape::Coding,
        }];

        let plan = build_token_debug_plan("Observation", &clauses);
        let json = serde_json::to_string(&plan).unwrap();

        assert!(json.contains("Observation"));
        assert!(json.contains("code"));
        assert!(json.contains("jsonb_containment"));
        assert!(json.contains("idx_observation_gin"));
        assert!(json.contains("system: $system"));
        assert!(
            !json.contains("loinc") && !json.contains("8480-6"),
            "debug output must not include token values: {json}"
        );
    }

    #[test]
    fn identifier_token_debug_plan_marks_gin_containment_forms() {
        let clauses = vec![TokenClause {
            resource_type: "Patient".to_string(),
            param_code: "identifier".to_string(),
            predicate: TokenPredicate::SystemCode {
                system: "http://hospital.example/mrn".to_string(),
                code: "12345".to_string(),
            },
            negated: false,
            index_shape: TokenIndexShape::Identifier,
        }];

        let plan = build_token_debug_plan("Patient", &clauses);
        let json = serde_json::to_string(&plan).unwrap();

        assert_eq!(plan.predicates[0].strategy, IndexStrategy::JsonbContainment);
        assert!(plan.predicates[0].index_backed);
        assert_eq!(
            plan.predicates[0].expected_index,
            Some("idx_patient_gin".to_string())
        );
        assert!(
            plan.predicates[0]
                .sql_shape
                .contains("resource @> {identifier")
        );
        assert!(plan.predicates[0].sql_shape.contains("value: $code"));
        assert!(
            !json.contains("hospital.example") && !json.contains("12345"),
            "debug output must not include identifier token values: {json}"
        );
    }

    #[test]
    fn identifier_no_system_token_debug_plan_stays_traversal() {
        let clauses = vec![TokenClause {
            resource_type: "Patient".to_string(),
            param_code: "identifier".to_string(),
            predicate: TokenPredicate::NoSystemCode {
                code: "12345".to_string(),
            },
            negated: false,
            index_shape: TokenIndexShape::Identifier,
        }];

        let plan = build_token_debug_plan("Patient", &clauses);

        assert_eq!(plan.predicates[0].strategy, IndexStrategy::JsonbTraversal);
        assert!(!plan.predicates[0].index_backed);
        assert_eq!(plan.predicates[0].expected_index, None);
        assert!(plan.predicates[0].sql_shape.contains("system IS NULL"));
    }

    #[test]
    fn simple_code_no_system_token_debug_plan_uses_containment() {
        let clauses = vec![TokenClause {
            resource_type: "Patient".to_string(),
            param_code: "gender".to_string(),
            predicate: TokenPredicate::NoSystemCode {
                code: "female".to_string(),
            },
            negated: false,
            index_shape: TokenIndexShape::SimpleCode,
        }];

        let plan = build_token_debug_plan("Patient", &clauses);

        assert_eq!(plan.predicates[0].strategy, IndexStrategy::JsonbContainment);
        assert!(plan.predicates[0].index_backed);
        assert_eq!(
            plan.predicates[0].expected_index,
            Some("idx_patient_gin".to_string())
        );
        assert_eq!(plan.predicates[0].sql_shape, "resource @> {code: $code}");
    }

    #[test]
    fn simple_code_system_any_token_debug_plan_matches_nothing() {
        let clauses = vec![TokenClause {
            resource_type: "Patient".to_string(),
            param_code: "gender".to_string(),
            predicate: TokenPredicate::SystemAnyCode {
                system: "http://example.org".to_string(),
            },
            negated: false,
            index_shape: TokenIndexShape::SimpleCode,
        }];

        let plan = build_token_debug_plan("Patient", &clauses);

        assert_eq!(plan.predicates[0].strategy, IndexStrategy::JsonbTraversal);
        assert!(!plan.predicates[0].index_backed);
        assert_eq!(plan.predicates[0].expected_index, None);
        assert_eq!(plan.predicates[0].sql_shape, "FALSE");
    }

    #[test]
    fn token_negation_debug_shape_matches_boolean_false_runtime_style() {
        let clauses = vec![TokenClause {
            resource_type: "Patient".to_string(),
            param_code: "gender".to_string(),
            predicate: TokenPredicate::AnySystemCode {
                code: "female".to_string(),
            },
            negated: true,
            index_shape: TokenIndexShape::SimpleCode,
        }];

        let plan = build_token_debug_plan("Patient", &clauses);

        assert!(plan.predicates[0].sql_shape.ends_with("= false"));
        assert!(!plan.predicates[0].sql_shape.contains("NOT ("));
    }

    #[test]
    fn reference_debug_plan_marks_inplace_traversal_and_redacts_values() {
        let clauses = vec![ReferenceClause {
            resource_type: "Observation".to_string(),
            param_code: "subject".to_string(),
            predicate: ReferencePredicate::Local {
                target_type: Some("Patient".to_string()),
                target_id: "pat-123".to_string(),
            },
            target_types: vec!["Patient".to_string()],
        }];

        let plan = build_reference_debug_plan("Observation", &clauses);
        let json = serde_json::to_string(&plan).unwrap();

        assert_eq!(
            plan.predicates[0].search_type,
            SearchParameterType::Reference
        );
        assert_eq!(plan.predicates[0].strategy, IndexStrategy::JsonbTraversal);
        assert!(!plan.predicates[0].index_backed);
        assert_eq!(plan.predicates[0].expected_index, None);
        assert_eq!(
            plan.predicates[0].sql_shape,
            "EXISTS jsonb_array_elements(resource->path) AS ref WHERE ref->>'reference' = $target_type/$target_id"
        );
        assert!(
            !json.contains("\"Patient\"") && !json.contains("pat-123"),
            "debug output must not include reference values: {json}"
        );
    }

    #[test]
    fn reference_identifier_debug_plan_marks_gin_containment() {
        let clauses = vec![ReferenceClause {
            resource_type: "Observation".to_string(),
            param_code: "subject".to_string(),
            predicate: ReferencePredicate::Identifier {
                system: Some("http://hospital.example/mrn".to_string()),
                require_no_system: false,
                value: "12345".to_string(),
            },
            target_types: vec!["Patient".to_string()],
        }];

        let plan = build_reference_debug_plan("Observation", &clauses);
        let json = serde_json::to_string(&plan).unwrap();

        assert_eq!(plan.predicates[0].strategy, IndexStrategy::JsonbContainment);
        assert!(plan.predicates[0].index_backed);
        assert_eq!(
            plan.predicates[0].expected_index,
            Some("idx_observation_gin".to_string())
        );
        assert_eq!(
            plan.predicates[0].sql_shape,
            "resource @> {path: {identifier: [{system: $system, value: $value}]}}"
        );
        assert!(
            !json.contains("hospital.example") && !json.contains("12345"),
            "debug output must not include identifier values: {json}"
        );
    }

    #[test]
    fn reference_missing_debug_plan_marks_inplace_presence_check() {
        let clauses = vec![ReferenceClause {
            resource_type: "Observation".to_string(),
            param_code: "subject".to_string(),
            predicate: ReferencePredicate::Missing { is_missing: true },
            target_types: vec!["Patient".to_string()],
        }];

        let plan = build_reference_debug_plan("Observation", &clauses);

        assert_eq!(plan.predicates[0].strategy, IndexStrategy::JsonbTraversal);
        assert!(!plan.predicates[0].index_backed);
        assert_eq!(plan.predicates[0].expected_index, None);
        assert_eq!(
            plan.predicates[0].sql_shape,
            "resource->path IS NULL OR resource->path = '[]'::jsonb"
        );
    }
}
