//! Reverse chaining (`_has`) implementation per FHIR R4 §3.1.1.5.4.
//!
//! Reverse chains let a search constrain the base resource by properties of
//! OTHER resources that reference it.
//!
//! Format:
//!     `_has:<SourceType>:<refParam>:<finalParam>[:<modifier>]=<value>`
//!
//! Example:
//!     `Patient?_has:Observation:patient:code=1234`
//!
//! Reads as: "Patients X such that there exists an Observation O whose
//! `patient` reference resolves to X and whose `code` search parameter matches
//! `1234`."
//!
//! Nested `_has` (any depth) is supported by recursive substitution:
//!     `Patient?_has:Observation:patient:_has:AuditEvent:entity:agent=Practitioner/1`
//!
//! Implementation:
//! - The link from base to source uses the `search_idx_reference` table
//!   (`target_type`/`target_id` lookup) — O(log N) index.
//! - The final-parameter condition is built via `dispatch_search` against an
//!   inner `SqlBuilder` bound to the source-table alias. This reuses all
//!   correctness behavior (modifiers, accent folding, GIN containment, etc.).
//! - Multiple `&`-occurrences of `_has:…` each emit a separate EXISTS clause;
//!   they AND naturally through `SqlBuilder::add_condition`.

use crate::parameters::{SearchModifier, SearchParameter, SearchParameterType};
use crate::parser::{ParsedParam, ParsedValue};
use crate::registry::SearchParameterRegistry;
use crate::sql_builder::{SqlBuilder, SqlBuilderError, SqlParam};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct ReverseChainParameter {
    pub source_type: String,
    pub reference_param: String,
    pub source_ref_def: Arc<SearchParameter>,
    /// Either a terminal final-parameter spec, or a nested `_has` for deeper
    /// reverse-chaining.
    pub tail: ReverseChainTail,
    pub value: String,
}

#[derive(Debug, Clone)]
pub enum ReverseChainTail {
    Final {
        param_name: String,
        modifier: Option<SearchModifier>,
        param_def: Arc<SearchParameter>,
    },
    Nested(Box<ReverseChainParameter>),
}

#[derive(Debug, thiserror::Error)]
pub enum ReverseChainingError {
    #[error("Invalid _has parameter: {0}")]
    InvalidHas(String),

    #[error("Unknown parameter {param} on {resource_type}")]
    UnknownParameter {
        param: String,
        resource_type: String,
    },

    #[error("Parameter {0} is not a reference type")]
    NotReferenceType(String),

    #[error("SQL builder error: {0}")]
    SqlBuilder(#[from] SqlBuilderError),
}

/// True for parameter names beginning with `_has:` (FHIR R4 §3.1.1.5.4).
pub fn is_reverse_chain_parameter(name: &str) -> bool {
    name.starts_with("_has:")
}

/// Parse a `_has:` parameter, validating the source-resource reference link
/// and final-parameter SP against the registry. Supports nested `_has`.
pub fn parse_reverse_chain(
    name: &str,
    value: &str,
    registry: &SearchParameterRegistry,
    _base_type: &str,
) -> Result<ReverseChainParameter, ReverseChainingError> {
    parse_recursive(name, value, registry)
}

fn parse_recursive(
    name: &str,
    value: &str,
    registry: &SearchParameterRegistry,
) -> Result<ReverseChainParameter, ReverseChainingError> {
    let rest = name
        .strip_prefix("_has:")
        .ok_or_else(|| ReverseChainingError::InvalidHas("expected `_has:` prefix".to_string()))?;

    // Split into at most 3 parts: SourceType, refParam, tail.
    // The tail may itself begin with `_has:` for nested chains, or be the
    // final-parameter spec (`code` or `code:modifier`).
    let (source_type, after_type) = rest.split_once(':').ok_or_else(|| {
        ReverseChainingError::InvalidHas("_has requires <SourceType>:<refParam>:…".to_string())
    })?;
    let (ref_param, tail_str) = after_type.split_once(':').ok_or_else(|| {
        ReverseChainingError::InvalidHas("_has requires <SourceType>:<refParam>:…".to_string())
    })?;

    let source_ref_def = registry.get(source_type, ref_param).ok_or_else(|| {
        ReverseChainingError::UnknownParameter {
            param: ref_param.to_string(),
            resource_type: source_type.to_string(),
        }
    })?;
    if source_ref_def.param_type != SearchParameterType::Reference {
        return Err(ReverseChainingError::NotReferenceType(
            ref_param.to_string(),
        ));
    }

    let tail = if tail_str.starts_with("_has:") {
        // Nested reverse chain — parse with source_type as the implicit base
        // for its own validation step.
        let nested = parse_recursive(tail_str, value, registry)?;
        ReverseChainTail::Nested(Box::new(nested))
    } else {
        // Terminal final-parameter spec — may carry one modifier.
        let (final_name, modifier_str) = match tail_str.split_once(':') {
            Some((n, m)) => (n, Some(m)),
            None => (tail_str, None),
        };
        let param_def = registry.get(source_type, final_name).ok_or_else(|| {
            ReverseChainingError::UnknownParameter {
                param: final_name.to_string(),
                resource_type: source_type.to_string(),
            }
        })?;
        let modifier = modifier_str.and_then(parse_modifier);
        ReverseChainTail::Final {
            param_name: final_name.to_string(),
            modifier,
            param_def,
        }
    };

    Ok(ReverseChainParameter {
        source_type: source_type.to_string(),
        reference_param: ref_param.to_string(),
        source_ref_def,
        tail,
        value: value.to_string(),
    })
}

fn parse_modifier(s: &str) -> Option<SearchModifier> {
    match s {
        "exact" => Some(SearchModifier::Exact),
        "contains" => Some(SearchModifier::Contains),
        "text" => Some(SearchModifier::Text),
        "in" => Some(SearchModifier::In),
        "not-in" => Some(SearchModifier::NotIn),
        "below" => Some(SearchModifier::Below),
        "above" => Some(SearchModifier::Above),
        "not" => Some(SearchModifier::Not),
        "identifier" => Some(SearchModifier::Identifier),
        "missing" => Some(SearchModifier::Missing),
        "of-type" | "ofType" => Some(SearchModifier::OfType),
        other if !other.is_empty() => Some(SearchModifier::Type(other.to_string())),
        _ => None,
    }
}

/// Generate the EXISTS clause for a parsed `_has` parameter and attach it to
/// `builder`.
///
/// `base_type` is the outer search's resource type (the one whose `id`
/// matches the inner `target_id`).
pub fn build_reverse_chain_search(
    builder: &mut SqlBuilder,
    param: &ReverseChainParameter,
    base_type: &str,
    registry: &SearchParameterRegistry,
) -> Result<(), ReverseChainingError> {
    // Build the EXISTS clause; each invocation uses a unique alias suffix so
    // multiple `_has` occurrences (or nested ones) don't collide in SQL.
    let depth = 0;
    let outer_id_ref = format!("{}->>'id'", builder.resource_column());
    let outer_id_ref = format!("({outer_id_ref})");
    // For the top-level outer table the alias is `r`, so r.id is cleaner.
    let outer_id_expr = if builder.resource_column() == "r.resource" {
        "r.id".to_string()
    } else {
        outer_id_ref
    };

    let clause = build_has_level(builder, param, base_type, &outer_id_expr, registry, depth)?;
    builder.add_condition(clause);
    Ok(())
}

/// Recursively build one EXISTS level. The outer-table id reference comes from
/// `outer_id_expr` (e.g. `r.id` at the top, `has<n>.id` for nested levels).
fn build_has_level(
    builder: &mut SqlBuilder,
    param: &ReverseChainParameter,
    base_type: &str,
    outer_id_expr: &str,
    registry: &SearchParameterRegistry,
    depth: usize,
) -> Result<String, ReverseChainingError> {
    let source_table = param.source_type.to_lowercase();
    let src_alias = format!("has{depth}");
    let sir_alias = format!("hasir{depth}");

    let rt_param = builder.add_text_param(&param.source_type);
    let pc_param = builder.add_text_param(&param.reference_param);
    let tt_param = builder.add_text_param(base_type);

    // Inner condition: either dispatch_search on terminal final-param, or a
    // recursive EXISTS for the nested _has level.
    let inner = match &param.tail {
        ReverseChainTail::Final {
            param_name,
            modifier,
            param_def,
        } => build_final_condition(
            builder,
            param_name,
            modifier.clone(),
            param_def,
            &param.value,
            &param.source_type,
            &src_alias,
            registry,
        )?,
        ReverseChainTail::Nested(inner_param) => {
            let inner_outer_id = format!("{src_alias}.id");
            build_has_level(
                builder,
                inner_param,
                &param.source_type,
                &inner_outer_id,
                registry,
                depth + 1,
            )?
        }
    };

    Ok(format!(
        "EXISTS (SELECT 1 FROM search_idx_reference {sir_alias} \
         JOIN \"{source_table}\" {src_alias} \
           ON {src_alias}.id = {sir_alias}.resource_id \
          AND {src_alias}.status != 'deleted' \
         WHERE {sir_alias}.resource_type = ${rt_param} \
           AND {sir_alias}.param_code = ${pc_param} \
           AND {sir_alias}.ref_kind = 1 \
           AND {sir_alias}.target_type = ${tt_param} \
           AND {sir_alias}.target_id = {outer_id_expr} \
           AND {inner})"
    ))
}

/// Build the inner search condition using `dispatch_search` on a builder
/// scoped to the source-table alias. Copies the produced SQL params back into
/// the outer builder.
fn build_final_condition(
    builder: &mut SqlBuilder,
    final_param: &str,
    modifier: Option<SearchModifier>,
    param_def: &Arc<SearchParameter>,
    raw_value: &str,
    source_type: &str,
    src_alias: &str,
    registry: &SearchParameterRegistry,
) -> Result<String, ReverseChainingError> {
    let resource_col = format!("{src_alias}.resource");
    let mut inner =
        SqlBuilder::with_resource_column(&resource_col).with_param_offset(builder.param_count());

    // Comma-OR within the single value entry (per FHIR §3.1.1.5).
    let values: Vec<ParsedValue> = raw_value
        .split(',')
        .filter(|v| !v.is_empty())
        .map(|v| ParsedValue {
            prefix: None,
            raw: v.to_string(),
        })
        .collect();

    let parsed = ParsedParam {
        name: final_param.to_string(),
        modifier,
        values,
    };

    crate::types::dispatch_search_with_registry(
        &mut inner,
        &parsed,
        param_def,
        source_type,
        registry,
    )?;

    // Promote inner params into the outer builder so bind indices stay
    // consistent with the produced SQL fragment.
    for p in inner.params() {
        match p {
            SqlParam::Text(s) => {
                builder.add_text_param(s);
            }
            SqlParam::Integer(i) => {
                builder.add_integer_param(*i);
            }
            SqlParam::Float(f) => {
                builder.add_float_param(*f);
            }
            SqlParam::Boolean(b) => {
                builder.add_boolean_param(*b);
            }
            SqlParam::Json(s) => {
                builder.add_json_param(s);
            }
            SqlParam::Timestamp(s) => {
                builder.add_timestamp_param(s);
            }
        }
    }

    let conditions = inner.conditions();
    match conditions.first() {
        Some(cond) => Ok(cond.clone()),
        None => Err(ReverseChainingError::InvalidHas(format!(
            "final parameter `{final_param}` produced no SQL condition"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parameters::SearchParameter;

    fn create_test_registry() -> SearchParameterRegistry {
        let registry = SearchParameterRegistry::new();

        let patient_param = SearchParameter::new(
            "patient",
            "http://hl7.org/fhir/SearchParameter/Observation-patient",
            SearchParameterType::Reference,
            vec!["Observation".to_string()],
        )
        .with_expression("Observation.subject.where(resolve() is Patient)")
        .with_targets(vec!["Patient".to_string()]);
        registry.register(patient_param);

        let code_param = SearchParameter::new(
            "code",
            "http://hl7.org/fhir/SearchParameter/Observation-code",
            SearchParameterType::Token,
            vec!["Observation".to_string()],
        )
        .with_expression("Observation.code");
        registry.register(code_param);

        registry
    }

    #[test]
    fn test_is_reverse_chain_parameter() {
        assert!(is_reverse_chain_parameter("_has:Observation:patient:code"));
        assert!(!is_reverse_chain_parameter("patient"));
        assert!(!is_reverse_chain_parameter("_include"));
    }

    #[test]
    fn test_parse_terminal_has() {
        let registry = create_test_registry();
        let parsed = parse_reverse_chain(
            "_has:Observation:patient:code",
            "1234",
            &registry,
            "Patient",
        )
        .expect("parse should succeed");
        assert_eq!(parsed.source_type, "Observation");
        assert_eq!(parsed.reference_param, "patient");
        match parsed.tail {
            ReverseChainTail::Final {
                param_name,
                modifier,
                ..
            } => {
                assert_eq!(param_name, "code");
                assert!(modifier.is_none());
            }
            _ => panic!("expected Final tail"),
        }
        assert_eq!(parsed.value, "1234");
    }

    #[test]
    fn test_parse_terminal_with_modifier() {
        let registry = create_test_registry();
        let parsed = parse_reverse_chain(
            "_has:Observation:patient:code:not",
            "1234",
            &registry,
            "Patient",
        )
        .expect("parse should succeed");
        match parsed.tail {
            ReverseChainTail::Final { modifier, .. } => {
                assert!(matches!(modifier, Some(SearchModifier::Not)));
            }
            _ => panic!("expected Final tail"),
        }
    }

    #[test]
    fn test_unknown_reference_param_fails() {
        let registry = create_test_registry();
        let err = parse_reverse_chain(
            "_has:Observation:unknown:code",
            "1234",
            &registry,
            "Patient",
        )
        .unwrap_err();
        assert!(matches!(err, ReverseChainingError::UnknownParameter { .. }));
    }
}
