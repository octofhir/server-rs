//! Search chaining implementation for FHIR search.
//!
//! This module implements forward chaining for reference parameters.
//! Chaining allows searching on properties of referenced resources.
//!
//! Examples:
//! - `Observation?patient.name=Smith` - Find observations where patient's name is Smith
//! - `Observation?subject:Patient.name=Smith` - Explicit type modifier

use crate::parameters::SearchParameterType;
use crate::registry::SearchParameterRegistry;
use crate::sql_builder::{SqlBuilder, SqlBuilderError};

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

    let ref_path = extract_reference_path(&link.expression, current_type);
    let target_table = target_type.to_lowercase();
    let alias = format!("chain{depth}");

    // Determine reference source: base resource or previous chain alias
    let ref_source = if depth == 0 {
        ref_path
    } else {
        format!(
            "chain{}.{}",
            depth - 1,
            ref_path.strip_prefix("resource").unwrap_or(&ref_path)
        )
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
        "EXISTS (SELECT 1 FROM {target_table} {alias} WHERE \
         {alias}.id::text = substring({ref_source}->>'reference' from '[^/]+$') \
         AND {alias}.status != 'deleted' \
         AND {inner_condition})"
    ))
}

/// Extract the reference path from a FHIRPath expression.
fn extract_reference_path(expression: &str, resource_type: &str) -> String {
    // Convert FHIRPath like "Observation.subject" to JSONB path
    let path = expression
        .strip_prefix(resource_type)
        .unwrap_or(expression)
        .strip_prefix('.')
        .unwrap_or(expression);

    // Split by . and build JSONB accessor
    let parts: Vec<&str> = path.split('.').filter(|p| !p.is_empty()).collect();

    if parts.is_empty() {
        return "resource".to_string();
    }

    let mut accessor = "resource".to_string();
    for part in parts {
        accessor.push_str(&format!("->'{part}'"));
    }
    accessor
}

/// Build the final condition for the chained search.
fn build_final_condition(
    builder: &mut SqlBuilder,
    chained: &ChainedParameter,
    target_type: &str,
    alias: &str,
) -> Result<String, ChainingError> {
    let final_path = extract_final_path(&chained.final_expression, target_type);

    // Build condition based on parameter type
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

        // Patient.name
        let name_param = SearchParameter::new(
            "name",
            "http://hl7.org/fhir/SearchParameter/Patient-name",
            SearchParameterType::String,
            vec!["Patient".to_string()],
        )
        .with_expression("Patient.name");
        registry.register(name_param);

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
        assert!(clause_str.contains("patient"));
    }

    #[test]
    fn test_extract_reference_path() {
        let path = extract_reference_path("Observation.subject", "Observation");
        assert_eq!(path, "resource->'subject'");

        let path = extract_reference_path("Patient.generalPractitioner", "Patient");
        assert_eq!(path, "resource->'generalPractitioner'");
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
        // Should have nested EXISTS
        assert!(clause.contains("EXISTS"));
        assert!(clause.contains("chain0"));
        assert!(clause.contains("chain1"));
        assert!(clause.contains("patient"));
        assert!(clause.contains("organization"));
    }
}
