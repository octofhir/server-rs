//! Reverse chaining (_has) implementation for FHIR search.
//!
//! Reverse chaining allows searching for resources based on other resources
//! that reference them.
//!
//! Example: `Patient?_has:Observation:patient:code=1234`
//! Finds patients that have observations with code 1234.

use crate::parameters::SearchParameterType;
use crate::registry::SearchParameterRegistry;
use crate::sql_builder::{SqlBuilder, SqlBuilderError};

/// A parsed reverse chain (_has) parameter.
#[derive(Debug, Clone)]
pub struct ReverseChainParameter {
    /// The source resource type that references the target
    pub source_type: String,
    /// The reference parameter on the source type
    pub reference_param: String,
    /// The expression for the reference parameter
    pub reference_expression: String,
    /// The search parameter to filter on
    pub search_param: String,
    /// The search parameter type
    pub search_param_type: SearchParameterType,
    /// The expression for the search parameter
    pub search_expression: String,
    /// The search value
    pub value: String,
    /// Nested _has parameter (for chained _has)
    pub nested: Option<Box<ReverseChainParameter>>,
}

/// Error type for reverse chaining operations.
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

/// Check if a parameter name is a reverse chain (_has) parameter.
pub fn is_reverse_chain_parameter(name: &str) -> bool {
    name.starts_with("_has:")
}

/// Parse a reverse chain (_has) parameter.
///
/// Format: `_has:Type:referenceParam:searchParam=value`
///
/// # Arguments
/// * `name` - The parameter name (e.g., "_has:Observation:patient:code")
/// * `value` - The search value
/// * `registry` - The search parameter registry
/// * `base_type` - The base resource type being searched
///
/// # Returns
/// A `ReverseChainParameter` if parsing succeeds, or an error.
pub fn parse_reverse_chain(
    name: &str,
    value: &str,
    registry: &SearchParameterRegistry,
    _base_type: &str,
) -> Result<ReverseChainParameter, ReverseChainingError> {
    if !name.starts_with("_has:") {
        return Err(ReverseChainingError::InvalidHas(
            "Parameter must start with _has:".to_string(),
        ));
    }

    let parts: Vec<&str> = name[5..].split(':').collect();
    if parts.len() < 3 {
        return Err(ReverseChainingError::InvalidHas(
            "_has requires Type:referenceParam:searchParam format".to_string(),
        ));
    }

    let source_type = parts[0];
    let reference_param = parts[1];
    let search_param = parts[2];

    // Get the reference parameter definition
    let ref_param_def = registry.get(source_type, reference_param).ok_or_else(|| {
        ReverseChainingError::UnknownParameter {
            param: reference_param.to_string(),
            resource_type: source_type.to_string(),
        }
    })?;

    // Verify it's a reference type
    if ref_param_def.param_type != SearchParameterType::Reference {
        return Err(ReverseChainingError::NotReferenceType(
            reference_param.to_string(),
        ));
    }

    // Get the search parameter definition
    let search_param_def = registry.get(source_type, search_param).ok_or_else(|| {
        ReverseChainingError::UnknownParameter {
            param: search_param.to_string(),
            resource_type: source_type.to_string(),
        }
    })?;

    Ok(ReverseChainParameter {
        source_type: source_type.to_string(),
        reference_param: reference_param.to_string(),
        reference_expression: ref_param_def.expression.clone().unwrap_or_default(),
        search_param: search_param.to_string(),
        search_param_type: search_param_def.param_type,
        search_expression: search_param_def.expression.clone().unwrap_or_default(),
        value: value.to_string(),
        nested: None,
    })
}

/// Build SQL for a reverse chain (_has) parameter.
///
/// This generates an EXISTS subquery to find resources that are referenced
/// by other resources matching the search criteria.
pub fn build_reverse_chain_search(
    builder: &mut SqlBuilder,
    param: &ReverseChainParameter,
    base_type: &str,
) -> Result<(), ReverseChainingError> {
    let source_table = param.source_type.to_lowercase();
    let ref_path = extract_path(&param.reference_expression, &param.source_type);
    let search_path = extract_path(&param.search_expression, &param.source_type);

    // Build the search condition based on parameter type
    let search_condition = build_search_condition(builder, param, &search_path)?;

    // Build EXISTS subquery
    // The reference in the source resource should point to our base resource
    let condition = format!(
        "EXISTS (SELECT 1 FROM {source_table} rev \
         WHERE rev.{ref_path}->>'reference' = '{base_type}/' || {}.id::text \
         AND rev.status != 'deleted' \
         AND {search_condition})",
        builder.resource_column()
    );

    builder.add_condition(condition);

    Ok(())
}

/// Build the search condition based on parameter type.
fn build_search_condition(
    builder: &mut SqlBuilder,
    param: &ReverseChainParameter,
    search_path: &str,
) -> Result<String, ReverseChainingError> {
    match param.search_param_type {
        SearchParameterType::String => {
            let p = builder.add_text_param(format!("{}%", param.value));
            Ok(format!("rev.{search_path} ILIKE ${p}"))
        }
        SearchParameterType::Token => {
            // Handle system|code format
            if param.value.contains('|') {
                let parts: Vec<&str> = param.value.splitn(2, '|').collect();
                let system = parts[0];
                let code = parts[1];

                if system.is_empty() {
                    // |code - match code only
                    let p = builder.add_text_param(code);
                    Ok(format!(
                        "(rev.{search_path}->>'code' = ${p} OR rev.{search_path} = ${p})"
                    ))
                } else {
                    // system|code - match both
                    let p1 = builder.add_text_param(system);
                    let p2 = builder.add_text_param(code);
                    Ok(format!(
                        "(rev.{search_path}->>'system' = ${p1} AND rev.{search_path}->>'code' = ${p2})"
                    ))
                }
            } else {
                // code only
                let p = builder.add_text_param(&param.value);
                Ok(format!(
                    "(rev.{search_path}->>'code' = ${p} OR rev.{search_path} = ${p})"
                ))
            }
        }
        SearchParameterType::Date => {
            let p = builder.add_text_param(&param.value);
            Ok(format!("rev.{search_path} = ${p}"))
        }
        SearchParameterType::Number | SearchParameterType::Quantity => {
            let p = builder.add_text_param(&param.value);
            Ok(format!("(rev.{search_path})::numeric = ${p}::numeric"))
        }
        SearchParameterType::Reference => {
            let p = builder.add_text_param(&param.value);
            Ok(format!("rev.{search_path}->>'reference' = ${p}"))
        }
        SearchParameterType::Uri => {
            let p = builder.add_text_param(&param.value);
            Ok(format!("rev.{search_path} = ${p}"))
        }
        _ => Err(ReverseChainingError::InvalidHas(format!(
            "Unsupported search parameter type: {:?}",
            param.search_param_type
        ))),
    }
}

/// Extract a JSONB path from a FHIRPath expression.
fn extract_path(expression: &str, resource_type: &str) -> String {
    let path = expression
        .strip_prefix(resource_type)
        .unwrap_or(expression)
        .strip_prefix('.')
        .unwrap_or(expression);

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parameters::SearchParameter;

    fn create_test_registry() -> SearchParameterRegistry {
        let mut registry = SearchParameterRegistry::new();

        // Observation.patient -> Patient
        let patient_param = SearchParameter::new(
            "patient",
            "http://hl7.org/fhir/SearchParameter/Observation-patient",
            SearchParameterType::Reference,
            vec!["Observation".to_string()],
        )
        .with_expression("Observation.subject.where(resolve() is Patient)")
        .with_targets(vec!["Patient".to_string()]);
        registry.register(patient_param);

        // Observation.code
        let code_param = SearchParameter::new(
            "code",
            "http://hl7.org/fhir/SearchParameter/Observation-code",
            SearchParameterType::Token,
            vec!["Observation".to_string()],
        )
        .with_expression("Observation.code");
        registry.register(code_param);

        // Observation.status
        let status_param = SearchParameter::new(
            "status",
            "http://hl7.org/fhir/SearchParameter/Observation-status",
            SearchParameterType::Token,
            vec!["Observation".to_string()],
        )
        .with_expression("Observation.status");
        registry.register(status_param);

        registry
    }

    #[test]
    fn test_is_reverse_chain_parameter() {
        assert!(is_reverse_chain_parameter("_has:Observation:patient:code"));
        assert!(!is_reverse_chain_parameter("patient"));
        assert!(!is_reverse_chain_parameter("_include"));
    }

    #[test]
    fn test_parse_reverse_chain() {
        let registry = create_test_registry();
        let result = parse_reverse_chain(
            "_has:Observation:patient:code",
            "1234-5",
            &registry,
            "Patient",
        );

        assert!(result.is_ok());
        let param = result.unwrap();
        assert_eq!(param.source_type, "Observation");
        assert_eq!(param.reference_param, "patient");
        assert_eq!(param.search_param, "code");
        assert_eq!(param.value, "1234-5");
    }

    #[test]
    fn test_parse_reverse_chain_invalid_format() {
        let registry = create_test_registry();
        let result = parse_reverse_chain("_has:Observation:patient", "1234", &registry, "Patient");

        assert!(result.is_err());
        assert!(matches!(result, Err(ReverseChainingError::InvalidHas(_))));
    }

    #[test]
    fn test_parse_reverse_chain_unknown_param() {
        let registry = create_test_registry();
        let result = parse_reverse_chain(
            "_has:Observation:unknown:code",
            "1234",
            &registry,
            "Patient",
        );

        assert!(result.is_err());
        assert!(matches!(
            result,
            Err(ReverseChainingError::UnknownParameter { .. })
        ));
    }

    #[test]
    fn test_build_reverse_chain_search() {
        let registry = create_test_registry();
        let param = parse_reverse_chain(
            "_has:Observation:patient:code",
            "1234-5",
            &registry,
            "Patient",
        )
        .unwrap();

        let mut builder = SqlBuilder::new();
        let result = build_reverse_chain_search(&mut builder, &param, "Patient");

        assert!(result.is_ok());
        let clause = builder.build_where_clause();
        assert!(clause.is_some());
        let clause_str = clause.unwrap();
        assert!(clause_str.contains("EXISTS"));
        assert!(clause_str.contains("observation"));
        assert!(clause_str.contains("Patient/"));
    }
}
