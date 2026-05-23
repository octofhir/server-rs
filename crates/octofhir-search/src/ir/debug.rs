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

/// Build safe debug output for date sidecar predicates.
///
/// SQL shapes intentionally use symbolic bind names instead of actual values.
pub fn build_date_debug_plan(resource_type: &str, clauses: &[DateClause]) -> SearchDebugPlan {
    let mut plan = SearchDebugPlan::new(resource_type);
    plan.predicates = clauses.iter().map(debug_date_clause).collect();
    plan
}

/// Build safe debug output for string sidecar predicates.
///
/// SQL shapes intentionally use symbolic bind names instead of actual values.
pub fn build_string_debug_plan(resource_type: &str, clauses: &[StringClause]) -> SearchDebugPlan {
    let mut plan = SearchDebugPlan::new(resource_type);
    plan.predicates = clauses.iter().map(debug_string_clause).collect();
    plan
}

/// Build safe debug output for number predicates.
///
/// Current number search uses JSONB numeric casts, not a production sidecar.
/// Mark it explicitly as non-index-backed so debug consumers do not mistake it
/// for an optimized plan.
pub fn build_number_debug_plan(resource_type: &str, clauses: &[NumberClause]) -> SearchDebugPlan {
    let mut plan = SearchDebugPlan::new(resource_type);
    plan.predicates = clauses.iter().map(debug_number_clause).collect();
    plan
}

/// Build safe debug output for quantity predicates.
///
/// Current quantity search uses JSONB numeric casts plus JSONB system/code
/// comparisons, not a production sidecar.
pub fn build_quantity_debug_plan(
    resource_type: &str,
    clauses: &[QuantityClause],
) -> SearchDebugPlan {
    let mut plan = SearchDebugPlan::new(resource_type);
    plan.predicates = clauses.iter().map(debug_quantity_clause).collect();
    plan
}

/// Build safe debug output for composite predicates.
///
/// Current composite search uses independent JSONB component predicates. The
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
/// Narrative text is not stored in `search_idx_string`, so this path is
/// deliberately marked as JSONB traversal and non-index-backed.
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
    plan.predicates = clauses.iter().map(debug_reference_clause).collect();
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

fn debug_string_clause(clause: &StringClause) -> DebugPredicate {
    let (expected_index, sql_shape) = match &clause.predicate {
        StringPredicate::Prefix { .. } => (
            Some("search_idx_string_*_param_code_value_norm_trgm_idx".to_string()),
            "EXISTS search_idx_string WHERE sid.value_norm LIKE $prefix".to_string(),
        ),
        StringPredicate::Contains { .. } => (
            Some("search_idx_string_*_param_code_value_norm_trgm_idx".to_string()),
            "EXISTS search_idx_string WHERE sid.value_norm LIKE $contains".to_string(),
        ),
        StringPredicate::Exact { .. } => (
            Some("search_idx_string_*_param_code_value_exact_btree_idx".to_string()),
            "EXISTS search_idx_string WHERE sid.value_exact = $exact".to_string(),
        ),
        StringPredicate::Missing { is_missing } => {
            let shape = if *is_missing {
                "NOT EXISTS search_idx_string WHERE sid.param_code = $param_code"
            } else {
                "EXISTS search_idx_string WHERE sid.param_code = $param_code"
            };
            (
                Some("search_idx_string_*_resource_type_resource_id_idx".to_string()),
                shape.to_string(),
            )
        }
    };

    DebugPredicate {
        param_code: clause.param_code.clone(),
        search_type: SearchParameterType::String,
        strategy: IndexStrategy::SidecarString,
        expected_index,
        index_backed: true,
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
                "(jsonb_path)::numeric >= $lo AND (jsonb_path)::numeric < $hi".to_string()
            }
            SearchPrefix::Ne => {
                "(jsonb_path)::numeric < $lo OR (jsonb_path)::numeric >= $hi".to_string()
            }
            SearchPrefix::Gt | SearchPrefix::Sa => "(jsonb_path)::numeric > $value".to_string(),
            SearchPrefix::Lt | SearchPrefix::Eb => "(jsonb_path)::numeric < $value".to_string(),
            SearchPrefix::Ge => "(jsonb_path)::numeric >= $value".to_string(),
            SearchPrefix::Le => "(jsonb_path)::numeric <= $value".to_string(),
        },
        NumberPredicate::Missing { is_missing } => {
            if *is_missing {
                "jsonb_path IS NULL".to_string()
            } else {
                "jsonb_path IS NOT NULL".to_string()
            }
        }
    }
}

fn debug_quantity_clause(clause: &QuantityClause) -> DebugPredicate {
    DebugPredicate {
        param_code: clause.param_code.clone(),
        search_type: SearchParameterType::Quantity,
        strategy: IndexStrategy::JsonbTraversal,
        expected_index: None,
        index_backed: false,
        sql_shape: quantity_sql_shape(&clause.predicate),
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
            let mut shape = match prefix {
                SearchPrefix::Eq | SearchPrefix::Ap => {
                    "(jsonb_path->>'value')::numeric >= $lo AND (jsonb_path->>'value')::numeric < $hi"
                        .to_string()
                }
                SearchPrefix::Ne => {
                    "(jsonb_path->>'value')::numeric < $lo OR (jsonb_path->>'value')::numeric >= $hi"
                        .to_string()
                }
                SearchPrefix::Gt | SearchPrefix::Sa => {
                    "(jsonb_path->>'value')::numeric > $value".to_string()
                }
                SearchPrefix::Lt | SearchPrefix::Eb => {
                    "(jsonb_path->>'value')::numeric < $value".to_string()
                }
                SearchPrefix::Ge => "(jsonb_path->>'value')::numeric >= $value".to_string(),
                SearchPrefix::Le => "(jsonb_path->>'value')::numeric <= $value".to_string(),
            };

            if system.is_some() {
                shape.push_str(" AND jsonb_path->>'system' = $system");
            }
            if code.is_some() {
                shape.push_str(" AND (jsonb_path->>'code' = $code OR jsonb_path->>'unit' = $code)");
            }
            shape
        }
        QuantityPredicate::Missing { is_missing } => {
            if *is_missing {
                "jsonb_path IS NULL".to_string()
            } else {
                "jsonb_path IS NOT NULL".to_string()
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
        sql_shape = format!("NOT ({sql_shape})");
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

fn debug_reference_clause(clause: &ReferenceClause) -> DebugPredicate {
    let (strategy, expected_index, index_backed, sql_shape) = match &clause.predicate {
        ReferencePredicate::Local { target_type, .. } => {
            let (index, shape) = if target_type.is_some() {
                (
                    "idx_ref_local",
                    "EXISTS search_idx_reference WHERE sir.ref_kind = 1 AND sir.target_type = $target_type AND sir.target_id = $target_id",
                )
            } else {
                (
                    "idx_ref_local_untyped",
                    "EXISTS search_idx_reference WHERE sir.ref_kind = 1 AND sir.target_id = $target_id",
                )
            };
            (
                IndexStrategy::SidecarReference,
                Some(index.to_string()),
                true,
                shape.to_string(),
            )
        }
        ReferencePredicate::External { .. } => (
            IndexStrategy::SidecarReference,
            Some("idx_ref_external".to_string()),
            true,
            "EXISTS search_idx_reference WHERE sir.external_url = $url OR sir.raw_reference = $url"
                .to_string(),
        ),
        ReferencePredicate::Identifier {
            system,
            require_no_system,
            ..
        } => {
            let shape = if system.is_some() {
                "EXISTS search_idx_reference WHERE sir.ref_kind = 4 AND sir.identifier_system = $system AND sir.identifier_value = $value"
            } else if *require_no_system {
                "EXISTS search_idx_reference WHERE sir.ref_kind = 4 AND sir.identifier_system IS NULL AND sir.identifier_value = $value"
            } else {
                "EXISTS search_idx_reference WHERE sir.ref_kind = 4 AND sir.identifier_value = $value"
            };
            (
                IndexStrategy::SidecarReference,
                Some("idx_ref_identifier".to_string()),
                true,
                shape.to_string(),
            )
        }
        ReferencePredicate::Missing { is_missing } => {
            let shape = if *is_missing {
                "reference jsonb path IS NULL"
            } else {
                "reference jsonb path IS NOT NULL"
            };
            (
                IndexStrategy::JsonbTraversal,
                None,
                false,
                shape.to_string(),
            )
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
    match clause.predicate {
        TokenPredicate::DisplayText { .. }
        | TokenPredicate::Missing { .. }
        | TokenPredicate::TerminologySet { .. } => IndexStrategy::JsonbTraversal,
        _ => IndexStrategy::JsonbContainment,
    }
}

fn token_expected_index(clause: &TokenClause) -> Option<String> {
    if token_index_backed(clause) {
        Some(format!("idx_{}_gin", clause.resource_type.to_lowercase()))
    } else {
        None
    }
}

fn token_index_backed(clause: &TokenClause) -> bool {
    if clause.negated {
        return false;
    }
    match (&clause.index_shape, &clause.predicate) {
        (_, TokenPredicate::DisplayText { .. })
        | (_, TokenPredicate::Missing { .. })
        | (_, TokenPredicate::TerminologySet { .. }) => false,
        (TokenIndexShape::Identifier, _) => false,
        (TokenIndexShape::SimpleCode, TokenPredicate::AnySystemCode { .. }) => true,
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
                "EXISTS identifier WHERE ident.value = $code".to_string()
            }
            TokenIndexShape::Coding => {
                "resource @> {coding: [{code: $code}]} OR resource @> {code: $code}".to_string()
            }
        },
        TokenPredicate::NoSystemCode { .. } => match clause.index_shape {
            TokenIndexShape::Identifier => {
                "EXISTS identifier WHERE ident.system IS NULL AND ident.value = $code".to_string()
            }
            _ => "EXISTS coding WHERE coding.system IS NULL AND coding.code = $code".to_string(),
        },
        TokenPredicate::SystemAnyCode { .. } => match clause.index_shape {
            TokenIndexShape::Identifier => {
                "EXISTS identifier WHERE ident.system = $system".to_string()
            }
            _ => "EXISTS coding WHERE coding.system = $system".to_string(),
        },
        TokenPredicate::SystemCode { .. } => match clause.index_shape {
            TokenIndexShape::Identifier => "resource @> {identifier: [{system: $system, value: $code}]}".to_string(),
            TokenIndexShape::SimpleCode => "resource @> {code: $code}".to_string(),
            TokenIndexShape::Coding => {
                "resource @> {coding: [{system: $system, code: $code}]}".to_string()
            }
        },
        TokenPredicate::IdentifierOfType { .. } => {
            "EXISTS identifier WHERE ident.type.coding @> [{system: $system, code: $code}] AND ident.value = $value".to_string()
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
        assert!(json.contains("sidecar_string"));
        assert!(json.contains("sid.value_norm LIKE $contains"));
        assert!(json.contains("search_idx_string_*_param_code_value_norm_trgm_idx"));
        assert!(
            !json.contains("Smíth") && !json.contains("smith"),
            "debug output must not include bound string values: {json}"
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
    fn number_debug_plan_marks_jsonb_cast_as_non_index_backed_and_redacted() {
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
        assert!(plan.predicates[0].sql_shape.contains("::numeric >= $value"));
        assert!(
            !json.contains("123.45"),
            "debug output must not include bound number values: {json}"
        );
    }

    #[test]
    fn quantity_debug_plan_marks_jsonb_cast_as_non_index_backed_and_redacted() {
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
        assert_eq!(plan.predicates[0].strategy, IndexStrategy::JsonbTraversal);
        assert!(!plan.predicates[0].index_backed);
        assert_eq!(plan.predicates[0].expected_index, None);
        assert!(plan.predicates[0].sql_shape.contains("::numeric >= $lo"));
        assert!(plan.predicates[0].sql_shape.contains("system' = $system"));
        assert!(plan.predicates[0].sql_shape.contains("unit' = $code"));
        assert!(
            !json.contains("5.5") && !json.contains("unitsofmeasure") && !json.contains("mg"),
            "debug output must not include bound quantity values: {json}"
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
    fn reference_debug_plan_marks_sidecar_and_redacts_values() {
        let clauses = vec![ReferenceClause {
            resource_type: "Observation".to_string(),
            param_code: "subject".to_string(),
            predicate: ReferencePredicate::Local {
                target_type: Some("Patient".to_string()),
                target_id: "pat-123".to_string(),
            },
            target_types: vec!["Patient".to_string()],
            jsonb_fallback_value: Some("Patient/pat-123".to_string()),
        }];

        let plan = build_reference_debug_plan("Observation", &clauses);
        let json = serde_json::to_string(&plan).unwrap();

        assert_eq!(
            plan.predicates[0].search_type,
            SearchParameterType::Reference
        );
        assert_eq!(plan.predicates[0].strategy, IndexStrategy::SidecarReference);
        assert!(plan.predicates[0].index_backed);
        assert_eq!(
            plan.predicates[0].expected_index,
            Some("idx_ref_local".to_string())
        );
        assert!(
            plan.predicates[0]
                .sql_shape
                .contains("search_idx_reference")
        );
        assert!(
            !json.contains("Patient") && !json.contains("pat-123"),
            "debug output must not include reference values: {json}"
        );
    }

    #[test]
    fn reference_missing_debug_plan_marks_jsonb_fallback() {
        let clauses = vec![ReferenceClause {
            resource_type: "Observation".to_string(),
            param_code: "subject".to_string(),
            predicate: ReferencePredicate::Missing { is_missing: true },
            target_types: vec!["Patient".to_string()],
            jsonb_fallback_value: None,
        }];

        let plan = build_reference_debug_plan("Observation", &clauses);

        assert_eq!(plan.predicates[0].strategy, IndexStrategy::JsonbTraversal);
        assert!(!plan.predicates[0].index_backed);
        assert_eq!(plan.predicates[0].expected_index, None);
        assert!(plan.predicates[0].sql_shape.contains("IS NULL"));
    }
}
