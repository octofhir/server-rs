//! Search chaining implementation for FHIR search.
//!
//! This module implements forward chaining for reference parameters.
//! Chaining allows searching on properties of referenced resources.
//!
//! Examples:
//! - `Observation?patient.name=Smith` - Find observations where patient's name is Smith
//! - `Observation?subject:Patient.name=Smith` - Explicit type modifier

use crate::parameters::{SearchParameter, SearchParameterType};
use crate::registry::SearchParameterRegistry;
use crate::sql_builder::{SqlBuilder, SqlBuilderError};
use std::sync::Arc;

/// A link in a chained search parameter.
#[derive(Debug, Clone)]
pub struct ChainLink {
    /// The search parameter name (e.g., "patient", "subject")
    pub parameter: String,
    /// The target resource type (e.g., "Patient"), if specified with :Type modifier
    pub target_type: Option<String>,
    /// The expression for extracting the reference from the resource
    pub expression: String,
}

/// A fully parsed chained search parameter.
#[derive(Debug, Clone)]
pub struct ChainedParameter {
    /// The chain of reference links
    pub chain: Vec<ChainLink>,
    /// The final search parameter name
    pub final_param: String,
    /// The final search parameter type
    pub final_param_type: SearchParameterType,
    /// The final search parameter expression
    pub final_expression: String,
    /// Full search parameter definition for the final parameter
    pub final_param_def: Option<Arc<SearchParameter>>,
    /// The search value
    pub value: String,
    /// Optional modifier on the final parameter
    pub modifier: Option<String>,
}

/// Error type for chaining operations.
#[derive(Debug, thiserror::Error)]
pub enum ChainingError {
    #[error("Invalid chained parameter: {0}")]
    InvalidChain(String),

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

/// Parse a chained search parameter.
///
/// Format: `param1.param2.param3=value` or `param1:Type.param2=value`
///
/// # Arguments
/// * `name` - The parameter name (e.g., "patient.name" or "subject:Patient.name")
/// * `value` - The search value
/// * `registry` - The search parameter registry
/// * `resource_type` - The base resource type being searched
///
/// # Returns
/// A `ChainedParameter` if parsing succeeds, or an error if the chain is invalid.
pub fn parse_chained_parameter(
    name: &str,
    value: &str,
    registry: &SearchParameterRegistry,
    resource_type: &str,
) -> Result<ChainedParameter, ChainingError> {
    let parts: Vec<&str> = name.split('.').collect();

    if parts.len() < 2 {
        return Err(ChainingError::InvalidChain(
            "Chained parameter requires at least two parts".to_string(),
        ));
    }

    let mut chain = Vec::new();
    let mut current_type = resource_type.to_string();

    // Parse all but last part as chain links
    for (i, part) in parts.iter().take(parts.len() - 1).enumerate() {
        let (param_name, target_type) = parse_parameter_with_type(part);

        // Get search parameter definition
        let param_def = registry.get(&current_type, param_name).ok_or_else(|| {
            ChainingError::UnknownParameter {
                param: param_name.to_string(),
                resource_type: current_type.clone(),
            }
        })?;

        // Verify it's a reference type
        if param_def.param_type != SearchParameterType::Reference {
            return Err(ChainingError::NotReferenceType(param_name.to_string()));
        }

        // Determine target type
        let resolved_type = target_type.map(|s| s.to_string()).or_else(|| {
            if param_def.target.len() == 1 {
                Some(param_def.target[0].clone())
            } else {
                None
            }
        });

        // For intermediate links, we need a resolved type
        if resolved_type.is_none() && i < parts.len() - 2 {
            return Err(ChainingError::AmbiguousTarget(param_name.to_string()));
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

    // Parse final parameter (may have modifier)
    let final_part = parts.last().unwrap();
    let (final_param, modifier) = parse_parameter_with_modifier(final_part);

    // Get final parameter definition
    let final_param_def = registry.get(&current_type, final_param).ok_or_else(|| {
        ChainingError::UnknownParameter {
            param: final_param.to_string(),
            resource_type: current_type.clone(),
        }
    })?;

    Ok(ChainedParameter {
        chain,
        final_param: final_param.to_string(),
        final_param_type: final_param_def.param_type,
        final_expression: final_param_def.expression.clone().unwrap_or_default(),
        final_param_def: Some(final_param_def),
        value: value.to_string(),
        modifier: modifier.map(|s| s.to_string()),
    })
}

/// Parse a parameter name that may have a :Type modifier.
fn parse_parameter_with_type(part: &str) -> (&str, Option<&str>) {
    if let Some(pos) = part.find(':') {
        (&part[..pos], Some(&part[pos + 1..]))
    } else {
        (part, None)
    }
}

/// Parse a parameter name that may have a modifier.
fn parse_parameter_with_modifier(part: &str) -> (&str, Option<&str>) {
    if let Some(pos) = part.find(':') {
        (&part[..pos], Some(&part[pos + 1..]))
    } else {
        (part, None)
    }
}

/// Check if a parameter name contains a chained reference (has a dot).
pub fn is_chained_parameter(name: &str) -> bool {
    // Exclude special parameters that have dots but aren't chains
    !name.starts_with('_') && name.contains('.')
}

/// Build SQL for a chained search parameter.
///
/// This generates nested EXISTS subqueries to traverse the reference chain.
/// Supports multi-level chaining (e.g., patient.organization.name).
pub fn build_chained_search(
    builder: &mut SqlBuilder,
    chained: &ChainedParameter,
    base_type: &str,
) -> Result<(), ChainingError> {
    if chained.chain.is_empty() {
        return Err(ChainingError::InvalidChain("Empty chain".to_string()));
    }

    // Build nested EXISTS subqueries from outside in
    let condition = build_nested_chain(builder, chained, base_type, 0)?;
    builder.add_condition(condition);

    Ok(())
}

/// Recursively build nested EXISTS subqueries for multi-level chaining.
///
/// Uses the `search_idx_reference` index table for B-tree lookups instead of
/// runtime `fhir_ref_id()` extraction from JSONB.
fn build_nested_chain(
    builder: &mut SqlBuilder,
    chained: &ChainedParameter,
    current_type: &str,
    depth: usize,
) -> Result<String, ChainingError> {
    let link = &chained.chain[depth];
    let target_type = link
        .target_type
        .as_ref()
        .ok_or_else(|| ChainingError::AmbiguousTarget(link.parameter.clone()))?;

    let target_table = target_type.to_lowercase();
    let alias = format!("chain{depth}");
    let sir_alias = format!("sir{depth}");

    // Add parameters for index table lookup
    let rt_param = builder.add_text_param(current_type);
    let pc_param = builder.add_text_param(&link.parameter);
    let tt_param = builder.add_text_param(target_type);

    // Determine resource_id reference: depth 0 uses r.id, deeper levels use previous chain alias
    let resource_id_ref = if depth == 0 {
        "r.id".to_string()
    } else {
        format!("chain{}.id", depth - 1)
    };

    // Build inner condition: either next chain level or final condition
    let inner_condition = if depth + 1 < chained.chain.len() {
        // More chain links - recurse
        build_nested_chain(builder, chained, target_type, depth + 1)?
    } else {
        // Final link - build the actual search condition
        build_final_condition(builder, chained, target_type, &alias)?
    };

    Ok(format!(
        "EXISTS (SELECT 1 FROM search_idx_reference {sir_alias} \
         JOIN {target_table} {alias} ON {alias}.id = {sir_alias}.target_id \
         AND {alias}.status != 'deleted' \
         WHERE {sir_alias}.resource_type = ${rt_param} AND {sir_alias}.resource_id = {resource_id_ref} \
         AND {sir_alias}.param_code = ${pc_param} AND {sir_alias}.ref_kind = 1 \
         AND {sir_alias}.target_type = ${tt_param} \
         AND {inner_condition})"
    ))
}

/// Build the final condition for the chained search.
///
/// Uses the full `dispatch_search` pipeline when a parameter definition is
/// available, so array-aware and GIN-optimized logic is reused correctly.
/// Falls back to a simple path-based condition otherwise.
fn build_final_condition(
    builder: &mut SqlBuilder,
    chained: &ChainedParameter,
    target_type: &str,
    alias: &str,
) -> Result<String, ChainingError> {
    // If we have the full param definition, use dispatch_search for correct handling
    // of arrays, HumanName, GIN containment, etc.
    if let Some(ref param_def) = chained.final_param_def {
        let resource_col = format!("{alias}.resource");
        let mut inner_builder = SqlBuilder::with_resource_column(&resource_col)
            .with_param_offset(builder.param_count());

        // Build ParsedParam from the chained value
        let modifier = chained
            .modifier
            .as_deref()
            .and_then(crate::parameters::SearchModifier::parse);

        // Parse the value into ParsedValue(s), handling comma-separated OR values
        let values: Vec<crate::parser::ParsedValue> = chained
            .value
            .split(',')
            .filter(|v| !v.is_empty())
            .map(|v| crate::parser::ParsedValue {
                prefix: None,
                raw: v.to_string(),
            })
            .collect();

        let parsed = crate::parser::ParsedParam {
            name: chained.final_param.clone(),
            modifier,
            values,
        };

        crate::types::dispatch_search(&mut inner_builder, &parsed, param_def, target_type)?;

        // Extract conditions and params from inner builder
        let conditions = inner_builder.conditions();
        if let Some(condition) = conditions.first() {
            // Copy params from inner builder to outer builder
            for p in inner_builder.params() {
                match p {
                    crate::sql_builder::SqlParam::Text(s) => {
                        builder.add_text_param(s);
                    }
                    crate::sql_builder::SqlParam::Integer(i) => {
                        builder.add_integer_param(*i);
                    }
                    crate::sql_builder::SqlParam::Float(f) => {
                        builder.add_float_param(*f);
                    }
                    crate::sql_builder::SqlParam::Boolean(b) => {
                        builder.add_boolean_param(*b);
                    }
                    crate::sql_builder::SqlParam::Json(s) => {
                        builder.add_json_param(s);
                    }
                    crate::sql_builder::SqlParam::Timestamp(s) => {
                        builder.add_timestamp_param(s);
                    }
                }
            }
            return Ok(condition.clone());
        }
    }

    // Fallback: simple path-based condition (for when no param def is available)
    let final_path = extract_final_path(&chained.final_expression, target_type);
    match chained.final_param_type {
        SearchParameterType::String => {
            let p = builder.add_text_param(format!("{}%", chained.value));
            Ok(format!("{alias}.{final_path} ILIKE ${p}"))
        }
        SearchParameterType::Token => {
            let p = builder.add_text_param(&chained.value);
            Ok(format!("{alias}.{final_path} = ${p}"))
        }
        SearchParameterType::Reference => {
            let p = builder.add_text_param(&chained.value);
            Ok(format!("{alias}.{final_path}->>'reference' = ${p}"))
        }
        SearchParameterType::Date => {
            let p = builder.add_text_param(&chained.value);
            Ok(format!("{alias}.{final_path} = ${p}"))
        }
        SearchParameterType::Number | SearchParameterType::Quantity => {
            let p = builder.add_text_param(&chained.value);
            Ok(format!("({alias}.{final_path})::numeric = ${p}::numeric"))
        }
        SearchParameterType::Uri => {
            let p = builder.add_text_param(&chained.value);
            Ok(format!("{alias}.{final_path} = ${p}"))
        }
        _ => Err(ChainingError::InvalidChain(format!(
            "Unsupported final parameter type: {:?}",
            chained.final_param_type
        ))),
    }
}

/// Extract the final parameter path from a FHIRPath expression.
fn extract_final_path(expression: &str, resource_type: &str) -> String {
    let path = expression
        .strip_prefix(resource_type)
        .unwrap_or(expression)
        .strip_prefix('.')
        .unwrap_or(expression);

    let parts: Vec<&str> = path.split('.').filter(|p| !p.is_empty()).collect();

    if parts.is_empty() {
        return "resource".to_string();
    }

    // For simple paths, use ->> for text extraction
    if parts.len() == 1 {
        return format!("resource->>'{}'", parts[0]);
    }

    // For nested paths, navigate with -> and end with ->>
    let mut accessor = "resource".to_string();
    for (i, part) in parts.iter().enumerate() {
        if i == parts.len() - 1 {
            accessor.push_str(&format!("->>'{part}'"));
        } else {
            accessor.push_str(&format!("->'{part}'"));
        }
    }
    accessor
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parameters::SearchParameter;

    fn create_test_registry() -> SearchParameterRegistry {
        use crate::parameters::ElementTypeHint;

        let mut registry = SearchParameterRegistry::new();

        // Observation.subject -> Patient
        let subject_param = SearchParameter::new(
            "subject",
            "http://hl7.org/fhir/SearchParameter/Observation-subject",
            SearchParameterType::Reference,
            vec!["Observation".to_string()],
        )
        .with_expression("Observation.subject")
        .with_targets(vec!["Patient".to_string(), "Group".to_string()]);
        registry.register(subject_param);

        // Patient.name (HumanName)
        let name_param = SearchParameter::new(
            "name",
            "http://hl7.org/fhir/SearchParameter/Patient-name",
            SearchParameterType::String,
            vec!["Patient".to_string()],
        )
        .with_expression("Patient.name")
        .with_element_type_hint(ElementTypeHint::HumanName);
        registry.register(name_param);

        // Patient.name.family (array string field)
        let family_param = SearchParameter::new(
            "family",
            "http://hl7.org/fhir/SearchParameter/Patient-family",
            SearchParameterType::String,
            vec!["Patient".to_string()],
        )
        .with_expression("Patient.name.family")
        .with_element_type_hint(ElementTypeHint::Array("string".to_string()));
        registry.register(family_param);

        // Patient.gender
        let gender_param = SearchParameter::new(
            "gender",
            "http://hl7.org/fhir/SearchParameter/Patient-gender",
            SearchParameterType::Token,
            vec!["Patient".to_string()],
        )
        .with_expression("Patient.gender");
        registry.register(gender_param);

        // Patient.generalPractitioner -> Practitioner, Organization
        let gp_param = SearchParameter::new(
            "general-practitioner",
            "http://hl7.org/fhir/SearchParameter/Patient-general-practitioner",
            SearchParameterType::Reference,
            vec!["Patient".to_string()],
        )
        .with_expression("Patient.generalPractitioner")
        .with_targets(vec!["Practitioner".to_string(), "Organization".to_string()]);
        registry.register(gp_param);

        // Organization.name
        let org_name_param = SearchParameter::new(
            "name",
            "http://hl7.org/fhir/SearchParameter/Organization-name",
            SearchParameterType::String,
            vec!["Organization".to_string()],
        )
        .with_expression("Organization.name");
        registry.register(org_name_param);

        registry
    }

    #[test]
    fn test_is_chained_parameter() {
        assert!(is_chained_parameter("patient.name"));
        assert!(is_chained_parameter("subject:Patient.name"));
        assert!(!is_chained_parameter("name"));
        assert!(!is_chained_parameter("_id"));
        assert!(!is_chained_parameter("_lastUpdated"));
    }

    #[test]
    fn test_parse_parameter_with_type() {
        let (name, typ) = parse_parameter_with_type("subject");
        assert_eq!(name, "subject");
        assert_eq!(typ, None);

        let (name, typ) = parse_parameter_with_type("subject:Patient");
        assert_eq!(name, "subject");
        assert_eq!(typ, Some("Patient"));
    }

    #[test]
    fn test_parse_chained_parameter_simple() {
        let registry = create_test_registry();
        let result =
            parse_chained_parameter("subject:Patient.name", "Smith", &registry, "Observation");

        assert!(result.is_ok());
        let chained = result.unwrap();
        assert_eq!(chained.chain.len(), 1);
        assert_eq!(chained.chain[0].parameter, "subject");
        assert_eq!(chained.chain[0].target_type, Some("Patient".to_string()));
        assert_eq!(chained.final_param, "name");
        assert_eq!(chained.value, "Smith");
    }

    #[test]
    fn test_parse_chained_parameter_unknown_param() {
        let registry = create_test_registry();
        let result = parse_chained_parameter("unknown.name", "Smith", &registry, "Observation");

        assert!(result.is_err());
        assert!(matches!(
            result,
            Err(ChainingError::UnknownParameter { .. })
        ));
    }

    #[test]
    fn test_parse_chained_parameter_not_reference() {
        let registry = create_test_registry();
        // Try to chain through a non-reference parameter
        let result = parse_chained_parameter("gender.value", "test", &registry, "Patient");

        assert!(result.is_err());
        assert!(matches!(result, Err(ChainingError::NotReferenceType(_))));
    }

    #[test]
    fn test_build_chained_search() {
        let registry = create_test_registry();
        let chained =
            parse_chained_parameter("subject:Patient.name", "Smith", &registry, "Observation")
                .unwrap();

        let mut builder = SqlBuilder::new();
        let result = build_chained_search(&mut builder, &chained, "Observation");

        assert!(result.is_ok());
        let clause = builder.build_where_clause();
        assert!(clause.is_some());
        let clause_str = clause.unwrap();
        assert!(clause_str.contains("EXISTS"));
        assert!(clause_str.contains("search_idx_reference"));
        assert!(clause_str.contains("patient"));
        assert!(clause_str.contains("ref_kind = 1"));
    }

    #[test]
    fn test_parse_multi_level_chain() {
        let registry = create_test_registry();
        // Observation -> Patient -> Organization
        let result = parse_chained_parameter(
            "subject:Patient.general-practitioner:Organization.name",
            "Acme",
            &registry,
            "Observation",
        );

        assert!(result.is_ok());
        let chained = result.unwrap();
        assert_eq!(chained.chain.len(), 2);
        assert_eq!(chained.chain[0].parameter, "subject");
        assert_eq!(chained.chain[0].target_type, Some("Patient".to_string()));
        assert_eq!(chained.chain[1].parameter, "general-practitioner");
        assert_eq!(
            chained.chain[1].target_type,
            Some("Organization".to_string())
        );
        assert_eq!(chained.final_param, "name");
        assert_eq!(chained.value, "Acme");
    }

    #[test]
    fn test_build_multi_level_chain() {
        let registry = create_test_registry();
        let chained = parse_chained_parameter(
            "subject:Patient.general-practitioner:Organization.name",
            "Acme",
            &registry,
            "Observation",
        )
        .unwrap();

        let mut builder = SqlBuilder::new();
        let result = build_chained_search(&mut builder, &chained, "Observation");

        assert!(result.is_ok());
        let clause = builder.build_where_clause().unwrap();
        // Should have nested EXISTS with index table
        assert!(clause.contains("EXISTS"));
        assert!(clause.contains("search_idx_reference"));
        assert!(clause.contains("chain0"));
        assert!(clause.contains("chain1"));
        assert!(clause.contains("sir0"));
        assert!(clause.contains("sir1"));
        assert!(clause.contains("patient"));
        assert!(clause.contains("organization"));
    }

    #[test]
    fn test_chained_string_family_uses_array_search() {
        // Test: Observation?subject:Patient.family=DebugFamily
        // The final condition should use array-aware SQL, not naive JSONB path
        let registry = create_test_registry();
        let chained = parse_chained_parameter(
            "subject:Patient.family",
            "DebugFamily",
            &registry,
            "Observation",
        )
        .unwrap();

        assert!(
            chained.final_param_def.is_some(),
            "Should have final param def"
        );

        let mut builder = SqlBuilder::new();
        build_chained_search(&mut builder, &chained, "Observation").unwrap();

        let clause = builder.build_where_clause().unwrap();
        // Should use jsonb_array_elements for name array, NOT naive resource->'name'->>'family'
        assert!(
            clause.contains("jsonb_array_elements") || clause.contains("@>"),
            "Expected array-aware SQL (jsonb_array_elements or @>), got: {clause}"
        );
        // Should NOT use the naive ->'name'->>'family' pattern
        assert!(
            !clause.contains("resource->'name'->>'family'"),
            "Should NOT use naive JSONB path for array field, got: {clause}"
        );
    }

    #[test]
    fn test_chained_string_name_uses_human_name_search() {
        // Test: Observation?subject:Patient.name=DebugGiven
        // The final condition should use HumanName search (family, given, text)
        let registry = create_test_registry();
        let chained = parse_chained_parameter(
            "subject:Patient.name",
            "DebugGiven",
            &registry,
            "Observation",
        )
        .unwrap();

        let mut builder = SqlBuilder::new();
        build_chained_search(&mut builder, &chained, "Observation").unwrap();

        let clause = builder.build_where_clause().unwrap();
        // Should search across family, given, text via HumanName logic
        assert!(
            clause.contains("family") && clause.contains("given"),
            "Expected HumanName search with family/given fields, got: {clause}"
        );
    }
}
