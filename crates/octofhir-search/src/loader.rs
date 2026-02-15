//! SearchParameter loading from canonical manager.
//!
//! This module provides functionality to load SearchParameter resources from
//! FHIR packages via the canonical manager and populate a SearchParameterRegistry.

use async_trait::async_trait;
use serde_json::Value;

use crate::common::register_common_parameters;
use crate::parameters::{ElementTypeHint, SearchModifier, SearchParameter, SearchParameterType};
use crate::registry::SearchParameterRegistry;

/// Resolves FHIR element type information from schema at registry build time.
///
/// Implemented by the server using FhirSchema/ModelProvider to provide
/// element type info that determines which SQL builder to use for search.
#[async_trait]
pub trait ElementTypeResolver: Send + Sync {
    /// Given a resource type (e.g., "Patient") and element path (e.g., "gender"),
    /// returns the FHIR type name (e.g., "code") and whether it's an array.
    async fn resolve(&self, resource_type: &str, element_path: &str) -> Option<(String, bool)>;
}

/// Error type for search parameter loading.
#[derive(Debug, thiserror::Error)]
pub enum LoaderError {
    /// Failed to query the canonical manager
    #[error("Failed to query canonical manager: {0}")]
    QueryError(String),

    /// Invalid search parameter resource
    #[error("Invalid SearchParameter: {0}")]
    InvalidSearchParameter(String),
}

/// Load search parameters from a canonical manager.
///
/// This function:
/// 1. Creates a new registry with common parameters
/// 2. Queries the canonical manager for SearchParameter resources
/// 3. Parses and registers each valid SearchParameter
/// 4. Resolves element types from schema for correct SQL generation
/// 5. Logs warnings for invalid parameters (but continues processing)
///
/// # Arguments
///
/// * `manager` - Reference to the canonical manager
/// * `resolver` - Optional element type resolver for resolving FHIR element types from schema
///
/// # Returns
///
/// A populated `SearchParameterRegistry`, or an error if the query fails.
pub async fn load_search_parameters(
    manager: &octofhir_canonical_manager::CanonicalManager,
    resolver: Option<&dyn ElementTypeResolver>,
) -> Result<SearchParameterRegistry, LoaderError> {
    use octofhir_canonical_manager::search::SearchQuery;

    let registry = SearchParameterRegistry::new();

    // Register built-in common parameters first
    register_common_parameters(&registry);

    // Resolve element types for common parameters if resolver is available
    if let Some(resolver) = resolver {
        resolve_registry_element_types(&registry, resolver).await;
    }

    // Query for all SearchParameter resources using pagination
    // The canonical manager has a max limit of 1000 per page
    const PAGE_SIZE: usize = 1000;
    let mut offset = 0;
    let mut loaded_count = 0;
    let mut skipped_count = 0;
    let mut total_fetched = 0;

    loop {
        let query = SearchQuery {
            resource_types: vec!["SearchParameter".to_string()],
            limit: Some(PAGE_SIZE),
            offset: Some(offset),
            ..Default::default()
        };

        let results = manager
            .search_engine()
            .search(&query)
            .await
            .map_err(|e| LoaderError::QueryError(e.to_string()))?;

        let page_count = results.resources.len();
        total_fetched += page_count;

        tracing::debug!(
            page_count = page_count,
            offset = offset,
            total_count = results.total_count,
            "Fetched SearchParameter page"
        );

        for resource_match in results.resources {
            match parse_search_parameter(&resource_match.resource.content) {
                Ok(mut param) => {
                    // Resolve element type from schema
                    if let Some(resolver) = resolver {
                        param.element_type_hint =
                            resolve_element_type_for_param(&param, resolver).await;
                    }
                    tracing::debug!(
                        code = %param.code,
                        bases = ?param.base,
                        param_type = ?param.param_type,
                        element_type_hint = ?param.element_type_hint,
                        "Loaded search parameter"
                    );
                    registry.register(param);
                    loaded_count += 1;
                }
                Err(e) => {
                    let url = resource_match
                        .resource
                        .content
                        .get("url")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown");
                    tracing::warn!(
                            url = %url,
                            error = %e,
                        "Failed to parse SearchParameter, skipping"
                    );
                    skipped_count += 1;
                }
            }
        }

        // Exit loop when we've fetched all resources or got an empty page
        if page_count < PAGE_SIZE || total_fetched >= results.total_count {
            break;
        }
        offset += PAGE_SIZE;
    }

    tracing::info!(
        loaded = loaded_count,
        skipped = skipped_count,
        total = registry.len(),
        "Loaded search parameters from canonical manager"
    );

    Ok(registry)
}

/// Parse a FHIR SearchParameter resource into our internal representation.
///
/// # Arguments
///
/// * `value` - The JSON value of the SearchParameter resource
///
/// # Returns
///
/// A `SearchParameter` struct, or an error if required fields are missing.
pub fn parse_search_parameter(value: &Value) -> Result<SearchParameter, LoaderError> {
    let code = value
        .get("code")
        .and_then(|v| v.as_str())
        .ok_or_else(|| LoaderError::InvalidSearchParameter("Missing 'code' field".into()))?
        .to_string();

    let url = value
        .get("url")
        .and_then(|v| v.as_str())
        .ok_or_else(|| LoaderError::InvalidSearchParameter("Missing 'url' field".into()))?
        .to_string();

    let param_type = value
        .get("type")
        .and_then(|v| v.as_str())
        .and_then(SearchParameterType::parse)
        .ok_or_else(|| {
            LoaderError::InvalidSearchParameter("Invalid or missing 'type' field".into())
        })?;

    let expression = value
        .get("expression")
        .and_then(|v| v.as_str())
        .map(String::from);

    let _xpath = value
        .get("xpath")
        .and_then(|v| v.as_str())
        .map(String::from);

    let base: Vec<String> = value
        .get("base")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    // Base is required - at least one resource type
    if base.is_empty() {
        return Err(LoaderError::InvalidSearchParameter(
            "Missing or empty 'base' field".into(),
        ));
    }

    let target: Vec<String> = value
        .get("target")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let modifier: Vec<SearchModifier> = value
        .get("modifier")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().and_then(SearchModifier::parse))
                .collect()
        })
        .unwrap_or_default();

    let comparators: Vec<String> = value
        .get("comparator")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    let description = value
        .get("description")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    // Build using builder pattern to ensure cached_jsonb_path is computed
    let mut param = SearchParameter::new(code, url, param_type, base);

    if let Some(expr) = expression {
        param = param.with_expression(expr);
    }

    if !target.is_empty() {
        param = param.with_targets(target);
    }

    if !modifier.is_empty() {
        param = param.with_modifiers(modifier);
    }

    if !comparators.is_empty() {
        param = param.with_comparators(comparators);
    }

    if !description.is_empty() {
        param = param.with_description(description);
    }

    // Note: xpath is rarely used and we skip it since there's no builder method
    // If needed, it can be added later via a with_xpath() method

    Ok(param)
}

/// Resolve element type hint for a parsed search parameter using the schema resolver.
///
/// Extracts resource type and element path from the FHIRPath expression,
/// then queries the resolver for the FHIR element type. For multi-resource
/// expressions, tries each expression to find a matching base type.
async fn resolve_element_type_for_param(
    param: &SearchParameter,
    resolver: &dyn ElementTypeResolver,
) -> ElementTypeHint {
    let Some(expression) = &param.expression else {
        return ElementTypeHint::Unknown;
    };

    // Parse expression to get resource type and element path.
    // Expressions can be multi-resource like "Patient.name | Practitioner.name"
    // or contain FHIRPath functions like "Encounter.subject.where(resolve() is Patient)".
    //
    // We iterate over each `|`-separated expression to find one that resolves,
    // and strip FHIRPath functions before querying the resolver.
    for expr_part in expression.split('|') {
        let expr_part = expr_part.trim();

        let Some((resource_type, element_path)) = expr_part.split_once('.') else {
            continue;
        };

        let element_path = element_path.trim();
        if element_path.is_empty() {
            continue;
        }

        // Strip FHIRPath functions (e.g., ".where(resolve() is Patient)") from
        // the element path — the resolver only understands schema element names.
        let clean_path = crate::sql_builder::strip_fhirpath_functions(element_path);
        if clean_path.is_empty() {
            continue;
        }

        // For common params with base "Resource", resolve against a concrete type
        let resolve_type = if resource_type == "Resource" || resource_type == "DomainResource" {
            "Patient"
        } else {
            resource_type
        };

        if let Some((type_name, is_array)) = resolver.resolve(resolve_type, &clean_path).await {
            let hint = ElementTypeHint::from_fhir_type(&type_name, is_array);
            tracing::trace!(
                code = %param.code,
                expression = %expression,
                resolved_expr = %expr_part,
                clean_path = %clean_path,
                fhir_type = %type_name,
                is_array = is_array,
                hint = ?hint,
                "Resolved element type for search parameter"
            );
            return hint;
        }
    }

    tracing::debug!(
        code = %param.code,
        expression = %expression,
        "Could not resolve element type from schema"
    );
    ElementTypeHint::Unknown
}

/// Resolve element types for all already-registered parameters in the registry.
///
/// Used after `register_common_parameters` to retroactively resolve types
/// for the built-in common parameters.
async fn resolve_registry_element_types(
    registry: &SearchParameterRegistry,
    resolver: &dyn ElementTypeResolver,
) {
    // Get all common parameters and resolve their types
    let common_params = registry.get_common_parameters();
    for param in &common_params {
        let hint = resolve_element_type_for_param(param, resolver).await;
        if hint != ElementTypeHint::Unknown {
            // Re-register with resolved hint
            let mut updated = (**param).clone();
            updated.element_type_hint = hint;
            registry.register(updated);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_parse_search_parameter_valid() {
        let value = json!({
            "resourceType": "SearchParameter",
            "url": "http://hl7.org/fhir/SearchParameter/Patient-name",
            "code": "name",
            "type": "string",
            "base": ["Patient"],
            "expression": "Patient.name",
            "description": "A patient's name"
        });

        let result = parse_search_parameter(&value);
        assert!(result.is_ok());

        let param = result.unwrap();
        assert_eq!(param.code, "name");
        assert_eq!(
            param.url,
            "http://hl7.org/fhir/SearchParameter/Patient-name"
        );
        assert_eq!(param.param_type, SearchParameterType::String);
        assert_eq!(param.base, vec!["Patient"]);
        assert_eq!(param.expression.as_deref(), Some("Patient.name"));
    }

    #[test]
    fn test_parse_search_parameter_missing_code() {
        let value = json!({
            "resourceType": "SearchParameter",
            "url": "http://example.org/sp",
            "type": "string",
            "base": ["Patient"]
        });

        let result = parse_search_parameter(&value);
        assert!(result.is_err());
        assert!(matches!(
            result,
            Err(LoaderError::InvalidSearchParameter(_))
        ));
    }

    #[test]
    fn test_parse_search_parameter_missing_base() {
        let value = json!({
            "resourceType": "SearchParameter",
            "url": "http://example.org/sp",
            "code": "test",
            "type": "string"
        });

        let result = parse_search_parameter(&value);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_search_parameter_invalid_type() {
        let value = json!({
            "resourceType": "SearchParameter",
            "url": "http://example.org/sp",
            "code": "test",
            "type": "invalid_type",
            "base": ["Patient"]
        });

        let result = parse_search_parameter(&value);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_search_parameter_with_targets() {
        let value = json!({
            "resourceType": "SearchParameter",
            "url": "http://hl7.org/fhir/SearchParameter/Patient-organization",
            "code": "organization",
            "type": "reference",
            "base": ["Patient"],
            "target": ["Organization"],
            "expression": "Patient.managingOrganization"
        });

        let result = parse_search_parameter(&value);
        assert!(result.is_ok());

        let param = result.unwrap();
        assert_eq!(param.param_type, SearchParameterType::Reference);
        assert_eq!(param.target, vec!["Organization"]);
    }

    #[test]
    fn test_parse_search_parameter_with_modifiers() {
        let value = json!({
            "resourceType": "SearchParameter",
            "url": "http://hl7.org/fhir/SearchParameter/Patient-name",
            "code": "name",
            "type": "string",
            "base": ["Patient"],
            "modifier": ["exact", "contains", "missing"]
        });

        let result = parse_search_parameter(&value);
        assert!(result.is_ok());

        let param = result.unwrap();
        assert_eq!(param.modifier.len(), 3);
        assert!(param.modifier.contains(&SearchModifier::Exact));
        assert!(param.modifier.contains(&SearchModifier::Contains));
        assert!(param.modifier.contains(&SearchModifier::Missing));
    }

    #[test]
    fn test_parse_search_parameter_with_comparators() {
        let value = json!({
            "resourceType": "SearchParameter",
            "url": "http://hl7.org/fhir/SearchParameter/Observation-value-quantity",
            "code": "value-quantity",
            "type": "quantity",
            "base": ["Observation"],
            "comparator": ["eq", "ne", "gt"]
        });

        let result = parse_search_parameter(&value);
        assert!(result.is_ok());

        let param = result.unwrap();
        assert_eq!(param.comparator.len(), 3);
        assert!(param.comparator.contains(&"eq".to_string()));
        assert!(param.comparator.contains(&"ne".to_string()));
        assert!(param.comparator.contains(&"gt".to_string()));
    }

    #[test]
    fn test_parse_search_parameter_multiple_bases() {
        let value = json!({
            "resourceType": "SearchParameter",
            "url": "http://hl7.org/fhir/SearchParameter/clinical-patient",
            "code": "patient",
            "type": "reference",
            "base": ["Observation", "Condition", "Procedure"],
            "target": ["Patient"]
        });

        let result = parse_search_parameter(&value);
        assert!(result.is_ok());

        let param = result.unwrap();
        assert_eq!(param.base.len(), 3);
        assert!(param.base.contains(&"Observation".to_string()));
        assert!(param.base.contains(&"Condition".to_string()));
        assert!(param.base.contains(&"Procedure".to_string()));
    }
}
