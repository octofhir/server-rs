//! Chained (`patient.name`) and reverse-chained (`_has:`) search as IR.
//!
//! Both forms walk reference links in place over the resource JSONB — the
//! source's reference (at the link's FHIRPath) must point at the joined target
//! row (`Type/id`). No sidecar index tables. The final / inner condition is
//! built via `dispatch_search` against the chained alias' resource JSONB, so
//! all per-type in-place predicate logic is reused.

use crate::ir::sql::{SelectStmt, SqlExpr, SqlFrom, SqlTerm};
use crate::parameters::{SearchModifier, SearchParameter, SearchParameterType};
use crate::parser::{ParsedParam, SearchParameterParser};
use crate::registry::SearchParameterRegistry;
use crate::sql_builder::{
    SqlBuilder, SqlBuilderError, SqlParam, fhirpath_to_jsonb_path,
    jsonb_reference_match_exists_expr,
};
use std::sync::Arc;

#[derive(Debug, thiserror::Error)]
pub enum ChainError {
    #[error("Invalid chained parameter: {0}")]
    InvalidChain(String),

    #[error("Invalid _has parameter: {0}")]
    InvalidHas(String),

    #[error("Unknown parameter {param} on {resource_type}")]
    UnknownParameter {
        param: String,
        resource_type: String,
    },

    #[error("Parameter {0} is not a reference type, cannot chain")]
    NotReferenceType(String),

    #[error("Ambiguous chain: parameter {0} has multiple targets, use :Type modifier")]
    AmbiguousTarget(String),

    #[error("SQL builder error: {0}")]
    SqlBuilder(#[from] SqlBuilderError),
}

// ============================================================================
// Forward chaining: `subject:Patient.name=Smith`
// ============================================================================

/// A single reference link in a chained search parameter.
#[derive(Debug, Clone)]
pub struct ChainLink {
    /// The search parameter code (e.g., "patient", "subject").
    pub parameter: String,
    /// The resolved target resource type (from `:Type` modifier or a single target).
    pub target_type: Option<String>,
    /// The FHIRPath expression locating the reference on the source resource.
    pub expression: String,
}

/// A fully parsed chained search parameter.
#[derive(Debug, Clone)]
pub struct ChainClause {
    pub chain: Vec<ChainLink>,
    pub final_param: String,
    pub final_param_def: Arc<SearchParameter>,
    pub value: String,
    pub modifier: Option<String>,
}

/// True for parameter names containing a chained reference (a dot, not a `_`
/// control parameter).
pub fn is_chained_parameter(name: &str) -> bool {
    !name.starts_with('_') && name.contains('.')
}

fn parse_parameter_with_type(part: &str) -> (&str, Option<&str>) {
    match part.find(':') {
        Some(pos) => (&part[..pos], Some(&part[pos + 1..])),
        None => (part, None),
    }
}

impl ChainClause {
    /// Parse `param1.param2…=value` (each leading link a reference parameter).
    pub fn parse(
        name: &str,
        value: &str,
        registry: &SearchParameterRegistry,
        resource_type: &str,
    ) -> Result<Self, ChainError> {
        let parts: Vec<&str> = name.split('.').collect();
        if parts.len() < 2 {
            return Err(ChainError::InvalidChain(
                "Chained parameter requires at least two parts".to_string(),
            ));
        }

        let mut chain = Vec::new();
        let mut current_type = resource_type.to_string();

        for (i, part) in parts.iter().take(parts.len() - 1).enumerate() {
            let (param_name, target_type) = parse_parameter_with_type(part);

            let param_def = registry.get(&current_type, param_name).ok_or_else(|| {
                ChainError::UnknownParameter {
                    param: param_name.to_string(),
                    resource_type: current_type.clone(),
                }
            })?;

            if param_def.param_type != SearchParameterType::Reference {
                return Err(ChainError::NotReferenceType(param_name.to_string()));
            }

            let resolved_type = target_type.map(|s| s.to_string()).or_else(|| {
                if param_def.target.len() == 1 {
                    Some(param_def.target[0].clone())
                } else {
                    None
                }
            });

            if resolved_type.is_none() && i < parts.len() - 2 {
                return Err(ChainError::AmbiguousTarget(param_name.to_string()));
            }

            let expression = param_def.expression.clone().unwrap_or_default();
            chain.push(ChainLink {
                parameter: param_name.to_string(),
                target_type: resolved_type.clone(),
                expression,
            });

            if let Some(t) = resolved_type {
                current_type = t;
            }
        }

        let final_part = parts.last().unwrap();
        let (final_param, modifier) = parse_parameter_with_type(final_part);

        let final_param_def = registry.get(&current_type, final_param).ok_or_else(|| {
            ChainError::UnknownParameter {
                param: final_param.to_string(),
                resource_type: current_type.clone(),
            }
        })?;

        Ok(ChainClause {
            chain,
            final_param: final_param.to_string(),
            final_param_def,
            value: value.to_string(),
            modifier: modifier.map(|s| s.to_string()),
        })
    }
}

/// Render a chained search clause as one in-place EXISTS condition and attach
/// it to `builder`.
pub fn render_chain_clause(
    builder: &mut SqlBuilder,
    clause: &ChainClause,
    base_type: &str,
    registry: &SearchParameterRegistry,
) -> Result<(), ChainError> {
    if clause.chain.is_empty() {
        return Err(ChainError::InvalidChain("Empty chain".to_string()));
    }
    let condition = render_nested_chain(builder, clause, base_type, registry, 0)?;
    builder.add_condition(condition);
    Ok(())
}

fn render_nested_chain(
    builder: &mut SqlBuilder,
    clause: &ChainClause,
    current_type: &str,
    registry: &SearchParameterRegistry,
    depth: usize,
) -> Result<SqlExpr, ChainError> {
    let link = &clause.chain[depth];
    let target_type = link
        .target_type
        .as_ref()
        .ok_or_else(|| ChainError::AmbiguousTarget(link.parameter.clone()))?;

    let target_table = target_type.to_lowercase();
    let alias = format!("chain{depth}");

    // Outer resource holding the reference: base table at depth 0, the previous
    // chain alias deeper.
    let outer_resource_col = if depth == 0 {
        builder.resource_column().to_string()
    } else {
        format!("chain{}.resource", depth - 1)
    };

    let segments = fhirpath_to_jsonb_path(&link.expression, current_type);
    let ref_predicate = format!(
        "(ref->>'reference' = '{target_type}/' || {alias}.id \
          OR ref->>'reference' LIKE '%/{target_type}/' || {alias}.id)"
    );
    let ref_exists =
        jsonb_reference_match_exists_expr(&outer_resource_col, &segments, &ref_predicate);

    let inner_condition = if depth + 1 < clause.chain.len() {
        render_nested_chain(builder, clause, target_type, registry, depth + 1)?
    } else {
        render_final_condition(
            builder,
            &clause.final_param,
            clause.modifier.as_deref().and_then(SearchModifier::parse),
            &clause.final_param_def,
            &clause.value,
            target_type,
            &alias,
            registry,
        )?
    };

    Ok(SqlExpr::Exists(Box::new(SelectStmt {
        projection: vec![SqlTerm::Integer(1)],
        from: SqlFrom {
            table: format!("\"{target_table}\""),
            alias: Some(alias.clone()),
        },
        where_clause: Some(SqlExpr::And(vec![
            SqlExpr::Raw(format!("{alias}.status != 'deleted'")),
            ref_exists,
            inner_condition,
        ])),
    })))
}

// ============================================================================
// Reverse chaining: `_has:Observation:patient:code=1234`
// ============================================================================

#[derive(Debug, Clone)]
pub struct HasClause {
    pub source_type: String,
    pub reference_param: String,
    pub source_ref_def: Arc<SearchParameter>,
    pub tail: HasTail,
    pub value: String,
}

#[derive(Debug, Clone)]
pub enum HasTail {
    Final {
        param_name: String,
        modifier: Option<SearchModifier>,
        param_def: Arc<SearchParameter>,
    },
    Nested(Box<HasClause>),
}

/// True for parameter names beginning with `_has:` (FHIR R4 §3.1.1.5.4).
pub fn is_reverse_chain_parameter(name: &str) -> bool {
    name.starts_with("_has:")
}

fn parse_has_modifier(s: &str) -> Option<SearchModifier> {
    SearchModifier::parse(s)
        .or_else(|| (s == "of-type").then_some(SearchModifier::OfType))
        .or_else(|| (!s.is_empty()).then(|| SearchModifier::Type(s.to_string())))
}

impl HasClause {
    /// Parse `_has:<SourceType>:<refParam>:<finalParam>[:modifier]=value`,
    /// supporting nested `_has` to any depth.
    pub fn parse(
        name: &str,
        value: &str,
        registry: &SearchParameterRegistry,
    ) -> Result<Self, ChainError> {
        let rest = name
            .strip_prefix("_has:")
            .ok_or_else(|| ChainError::InvalidHas("expected `_has:` prefix".to_string()))?;

        let (source_type, after_type) = rest.split_once(':').ok_or_else(|| {
            ChainError::InvalidHas("_has requires <SourceType>:<refParam>:…".to_string())
        })?;
        let (ref_param, tail_str) = after_type.split_once(':').ok_or_else(|| {
            ChainError::InvalidHas("_has requires <SourceType>:<refParam>:…".to_string())
        })?;

        let source_ref_def =
            registry
                .get(source_type, ref_param)
                .ok_or_else(|| ChainError::UnknownParameter {
                    param: ref_param.to_string(),
                    resource_type: source_type.to_string(),
                })?;
        if source_ref_def.param_type != SearchParameterType::Reference {
            return Err(ChainError::NotReferenceType(ref_param.to_string()));
        }

        let tail = if tail_str.starts_with("_has:") {
            HasTail::Nested(Box::new(HasClause::parse(tail_str, value, registry)?))
        } else {
            let (final_name, modifier_str) = match tail_str.split_once(':') {
                Some((n, m)) => (n, Some(m)),
                None => (tail_str, None),
            };
            let param_def = registry.get(source_type, final_name).ok_or_else(|| {
                ChainError::UnknownParameter {
                    param: final_name.to_string(),
                    resource_type: source_type.to_string(),
                }
            })?;
            let modifier = modifier_str.and_then(parse_has_modifier);
            HasTail::Final {
                param_name: final_name.to_string(),
                modifier,
                param_def,
            }
        };

        Ok(HasClause {
            source_type: source_type.to_string(),
            reference_param: ref_param.to_string(),
            source_ref_def,
            tail,
            value: value.to_string(),
        })
    }
}

/// Render a `_has` clause as one in-place EXISTS condition and attach it to
/// `builder`. `base_type` is the outer search's resource type.
pub fn render_has_clause(
    builder: &mut SqlBuilder,
    clause: &HasClause,
    base_type: &str,
    registry: &SearchParameterRegistry,
) -> Result<(), ChainError> {
    let outer_id_expr = if builder.resource_column() == "r.resource" {
        "r.id".to_string()
    } else {
        format!("({}->>'id')", builder.resource_column())
    };
    let cond = render_has_level(builder, clause, base_type, &outer_id_expr, registry, 0)?;
    builder.add_condition(cond);
    Ok(())
}

fn render_has_level(
    builder: &mut SqlBuilder,
    clause: &HasClause,
    base_type: &str,
    outer_id_expr: &str,
    registry: &SearchParameterRegistry,
    depth: usize,
) -> Result<SqlExpr, ChainError> {
    let source_table = clause.source_type.to_lowercase();
    let src_alias = format!("has{depth}");

    let segments = fhirpath_to_jsonb_path(
        clause
            .source_ref_def
            .expression
            .as_deref()
            .unwrap_or_default(),
        &clause.source_type,
    );
    let ref_predicate = format!(
        "(ref->>'reference' = '{base_type}/' || {outer_id_expr} \
          OR ref->>'reference' LIKE '%/{base_type}/' || {outer_id_expr})"
    );
    let ref_exists = jsonb_reference_match_exists_expr(
        &format!("{src_alias}.resource"),
        &segments,
        &ref_predicate,
    );

    let inner = match &clause.tail {
        HasTail::Final {
            param_name,
            modifier,
            param_def,
        } => render_final_condition(
            builder,
            param_name,
            modifier.clone(),
            param_def,
            &clause.value,
            &clause.source_type,
            &src_alias,
            registry,
        )?,
        HasTail::Nested(inner_clause) => {
            let inner_outer_id = format!("{src_alias}.id");
            render_has_level(
                builder,
                inner_clause,
                &clause.source_type,
                &inner_outer_id,
                registry,
                depth + 1,
            )?
        }
    };

    Ok(SqlExpr::Exists(Box::new(SelectStmt {
        projection: vec![SqlTerm::Integer(1)],
        from: SqlFrom {
            table: format!("\"{source_table}\""),
            alias: Some(src_alias.clone()),
        },
        where_clause: Some(SqlExpr::And(vec![
            SqlExpr::Raw(format!("{src_alias}.status != 'deleted'")),
            ref_exists,
            inner,
        ])),
    })))
}

// ============================================================================
// Shared final/inner condition
// ============================================================================

/// Build the terminal search condition via `dispatch_search` against the
/// chained alias' resource JSONB, promoting its bind params into `builder`.
#[allow(clippy::too_many_arguments)]
fn render_final_condition(
    builder: &mut SqlBuilder,
    final_param: &str,
    modifier: Option<SearchModifier>,
    param_def: &Arc<SearchParameter>,
    raw_value: &str,
    target_type: &str,
    alias: &str,
    registry: &SearchParameterRegistry,
) -> Result<SqlExpr, ChainError> {
    let resource_col = format!("{alias}.resource");
    let mut inner =
        SqlBuilder::with_resource_column(&resource_col).with_param_offset(builder.param_count());

    let values = SearchParameterParser::parse_values_for_type(raw_value, &param_def.param_type);
    let parsed = ParsedParam {
        name: final_param.to_string(),
        modifier,
        values,
    };

    crate::types::dispatch_search_with_registry(
        &mut inner,
        &parsed,
        param_def,
        target_type,
        registry,
    )?;

    for p in inner.params() {
        match p {
            SqlParam::Text(s) => builder.add_text_param(s),
            SqlParam::Integer(i) => builder.add_integer_param(*i),
            SqlParam::Float(f) => builder.add_float_param(*f),
            SqlParam::Boolean(b) => builder.add_boolean_param(*b),
            SqlParam::Json(s) => builder.add_json_param(s),
            SqlParam::Timestamp(s) => builder.add_timestamp_param(s),
        };
    }

    match inner.conditions().first() {
        Some(cond) => Ok(cond.clone()),
        None => Err(ChainError::InvalidChain(format!(
            "final parameter `{final_param}` produced no SQL condition"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parameters::{ElementTypeHint, SearchParameter};

    fn registry() -> SearchParameterRegistry {
        let registry = SearchParameterRegistry::new();

        let subject = SearchParameter::new(
            "subject",
            "http://hl7.org/fhir/SearchParameter/Observation-subject",
            SearchParameterType::Reference,
            vec!["Observation".to_string()],
        )
        .with_expression("Observation.subject")
        .with_targets(vec!["Patient".to_string(), "Group".to_string()]);
        registry.register(subject);

        let patient = SearchParameter::new(
            "patient",
            "http://hl7.org/fhir/SearchParameter/Observation-patient",
            SearchParameterType::Reference,
            vec!["Observation".to_string()],
        )
        .with_expression("Observation.subject.where(resolve() is Patient)")
        .with_targets(vec!["Patient".to_string()]);
        registry.register(patient);

        let name = SearchParameter::new(
            "name",
            "http://hl7.org/fhir/SearchParameter/Patient-name",
            SearchParameterType::String,
            vec!["Patient".to_string()],
        )
        .with_expression("Patient.name")
        .with_element_type_hint(ElementTypeHint::HumanName);
        registry.register(name);

        let family = SearchParameter::new(
            "family",
            "http://hl7.org/fhir/SearchParameter/Patient-family",
            SearchParameterType::String,
            vec!["Patient".to_string()],
        )
        .with_expression("Patient.name.family")
        .with_element_type_hint(ElementTypeHint::Array("string".to_string()));
        registry.register(family);

        let birthdate = SearchParameter::new(
            "birthdate",
            "http://hl7.org/fhir/SearchParameter/Patient-birthdate",
            SearchParameterType::Date,
            vec!["Patient".to_string()],
        )
        .with_expression("Patient.birthDate");
        registry.register(birthdate);

        let gp = SearchParameter::new(
            "general-practitioner",
            "http://hl7.org/fhir/SearchParameter/Patient-general-practitioner",
            SearchParameterType::Reference,
            vec!["Patient".to_string()],
        )
        .with_expression("Patient.generalPractitioner")
        .with_targets(vec!["Practitioner".to_string(), "Organization".to_string()]);
        registry.register(gp);

        let org_name = SearchParameter::new(
            "name",
            "http://hl7.org/fhir/SearchParameter/Organization-name",
            SearchParameterType::String,
            vec!["Organization".to_string()],
        )
        .with_expression("Organization.name");
        registry.register(org_name);

        let code = SearchParameter::new(
            "code",
            "http://hl7.org/fhir/SearchParameter/Observation-code",
            SearchParameterType::Token,
            vec!["Observation".to_string()],
        )
        .with_expression("Observation.code");
        registry.register(code);

        let date = SearchParameter::new(
            "date",
            "http://hl7.org/fhir/SearchParameter/Observation-date",
            SearchParameterType::Date,
            vec!["Observation".to_string()],
        )
        .with_expression("Observation.effective");
        registry.register(date);

        registry
    }

    #[test]
    fn chain_is_detected() {
        assert!(is_chained_parameter("patient.name"));
        assert!(is_chained_parameter("subject:Patient.name"));
        assert!(!is_chained_parameter("name"));
        assert!(!is_chained_parameter("_id"));
    }

    #[test]
    fn chain_renders_inplace_no_sidecar() {
        let reg = registry();
        let clause =
            ChainClause::parse("subject:Patient.name", "Smith", &reg, "Observation").unwrap();
        let mut builder = SqlBuilder::new();
        render_chain_clause(&mut builder, &clause, "Observation", &reg).unwrap();
        let sql = builder.build_where_clause().unwrap();
        assert!(sql.contains("EXISTS"));
        assert!(!sql.contains("search_idx_reference"));
        assert!(!sql.contains("search_idx_string"));
        assert!(sql.contains("\"patient\""));
        assert!(sql.contains("jsonb_array_elements"));
        assert!(sql.contains("chain0.resource"));
    }

    #[test]
    fn multi_level_chain_renders_inplace() {
        let reg = registry();
        let clause = ChainClause::parse(
            "subject:Patient.general-practitioner:Organization.name",
            "Acme",
            &reg,
            "Observation",
        )
        .unwrap();
        assert_eq!(clause.chain.len(), 2);
        let mut builder = SqlBuilder::new();
        render_chain_clause(&mut builder, &clause, "Observation", &reg).unwrap();
        let sql = builder.build_where_clause().unwrap();
        assert!(!sql.contains("search_idx_reference"));
        assert!(sql.contains("chain0"));
        assert!(sql.contains("chain1"));
        assert!(sql.contains("\"patient\""));
        assert!(sql.contains("\"organization\""));
    }

    #[test]
    fn chain_through_non_reference_fails() {
        let reg = registry();
        let err = ChainClause::parse("birthdate.value", "x", &reg, "Patient").unwrap_err();
        assert!(matches!(err, ChainError::NotReferenceType(_)));
    }

    #[test]
    fn has_is_detected() {
        assert!(is_reverse_chain_parameter("_has:Observation:patient:code"));
        assert!(!is_reverse_chain_parameter("patient"));
    }

    #[test]
    fn has_renders_inplace_no_sidecar() {
        let reg = registry();
        let clause = HasClause::parse("_has:Observation:patient:code", "1234", &reg).unwrap();
        let mut builder = SqlBuilder::with_resource_column("r.resource");
        render_has_clause(&mut builder, &clause, "Patient", &reg).unwrap();
        let sql = builder.build_where_clause().unwrap();
        assert!(sql.contains("EXISTS"));
        assert!(!sql.contains("search_idx_reference"));
        assert!(sql.contains("\"observation\""));
        assert!(sql.contains("has0.resource"));
    }

    #[test]
    fn has_date_preserves_prefix() {
        let reg = registry();
        let clause =
            HasClause::parse("_has:Observation:patient:date", "ge2020-01-01", &reg).unwrap();
        let mut builder = SqlBuilder::with_resource_column("r.resource");
        render_has_clause(&mut builder, &clause, "Patient", &reg).unwrap();
        // No legacy sidecar table may appear in the rendered SQL.
        let sql = builder.build_where_clause().unwrap();
        assert!(!sql.contains("search_idx_date"));
        assert!(
            builder
                .params()
                .iter()
                .any(|p| matches!(p, SqlParam::Timestamp(s) if s.starts_with("2020-01-01")))
        );
    }

    #[test]
    fn has_unknown_ref_param_fails() {
        let reg = registry();
        let err = HasClause::parse("_has:Observation:unknown:code", "1", &reg).unwrap_err();
        assert!(matches!(err, ChainError::UnknownParameter { .. }));
    }
}
