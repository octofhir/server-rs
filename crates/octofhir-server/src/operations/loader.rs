//! Operation definition loading from canonical manager.
//!
//! This module provides functionality to load FHIR OperationDefinition resources
//! from the canonical manager and populate the operation registry.

use octofhir_canonical_manager::search::SearchQuery;
use serde_json::Value;

use super::definition::{OperationDefinition, OperationKind, OperationParameter, ParameterUse};
use super::registry::OperationRegistry;

/// Error type for operation loading failures.
#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("Canonical manager not available")]
    ManagerNotAvailable,

    #[error("Search failed: {0}")]
    SearchFailed(String),

    #[error("Invalid operation definition: {0}")]
    InvalidDefinition(String),
}

/// Loads all OperationDefinition resources from the canonical manager
/// and returns a populated operation registry.
pub async fn load_operations() -> Result<OperationRegistry, LoadError> {
    let manager = crate::canonical::get_manager().ok_or(LoadError::ManagerNotAvailable)?;

    let mut registry = OperationRegistry::new();

    // Query for all OperationDefinition resources
    let query = SearchQuery {
        resource_types: vec!["OperationDefinition".to_string()],
        ..Default::default()
    };

    let results = manager
        .search_engine()
        .search(&query)
        .await
        .map_err(|e| LoadError::SearchFailed(e.to_string()))?;

    for resource_match in results.resources {
        let content = resource_match.resource.content;

        match parse_operation_definition(&content) {
            Ok(op) => {
                tracing::debug!(
                    code = %op.code,
                    url = %op.url,
                    system = op.system,
                    type_level = op.type_level,
                    instance = op.instance,
                    "Loaded operation definition"
                );
                registry.register(op);
            }
            Err(e) => {
                let url = content
                    .get("url")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                tracing::warn!(
                    url = url,
                    error = %e,
                    "Failed to parse OperationDefinition"
                );
            }
        }
    }

    tracing::info!(
        count = registry.len(),
        "Loaded operation definitions from packages"
    );

    Ok(registry)
}

/// Parses a JSON value into an OperationDefinition.
fn parse_operation_definition(value: &Value) -> Result<OperationDefinition, LoadError> {
    let code = value
        .get("code")
        .and_then(|v| v.as_str())
        .ok_or_else(|| LoadError::InvalidDefinition("Missing 'code' field".into()))?
        .to_string();

    let url = value
        .get("url")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let kind = match value.get("kind").and_then(|v| v.as_str()) {
        Some("operation") => OperationKind::Operation,
        Some("query") => OperationKind::Query,
        _ => OperationKind::Operation,
    };

    let system = value
        .get("system")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let type_level = value.get("type").and_then(|v| v.as_bool()).unwrap_or(false);

    let instance = value
        .get("instance")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let resource: Vec<String> = value
        .get("resource")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let parameters = value
        .get("parameter")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|p| parse_parameter(p).ok()).collect())
        .unwrap_or_default();

    let affects_state = value
        .get("affectsState")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    Ok(OperationDefinition {
        code,
        url,
        kind,
        system,
        type_level,
        instance,
        resource,
        parameters,
        affects_state,
    })
}

/// Parses a parameter definition from a JSON value.
fn parse_parameter(value: &Value) -> Result<OperationParameter, LoadError> {
    let name = value
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    if name.is_empty() {
        return Err(LoadError::InvalidDefinition(
            "Parameter missing 'name' field".into(),
        ));
    }

    let use_ = match value.get("use").and_then(|v| v.as_str()) {
        Some("out") => ParameterUse::Out,
        _ => ParameterUse::In,
    };

    let min = value.get("min").and_then(|v| v.as_u64()).unwrap_or(0) as u32;

    let max = value
        .get("max")
        .and_then(|v| v.as_str())
        .unwrap_or("1")
        .to_string();

    let param_type = value.get("type").and_then(|v| v.as_str()).map(String::from);

    let search_type = value
        .get("searchType")
        .and_then(|v| v.as_str())
        .map(String::from);

    let target_profile = value
        .get("targetProfile")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let parts = value
        .get("part")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|p| parse_parameter(p).ok()).collect())
        .unwrap_or_default();

    Ok(OperationParameter {
        name,
        use_,
        min,
        max,
        param_type,
        search_type,
        target_profile,
        parts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_parse_operation_definition() {
        let json = json!({
            "resourceType": "OperationDefinition",
            "url": "http://hl7.org/fhir/OperationDefinition/Resource-validate",
            "code": "validate",
            "kind": "operation",
            "system": false,
            "type": true,
            "instance": true,
            "resource": ["Resource"],
            "affectsState": false,
            "parameter": [{
                "name": "resource",
                "use": "in",
                "min": 0,
                "max": "1",
                "type": "Resource"
            }]
        });

        let op = parse_operation_definition(&json).unwrap();

        assert_eq!(op.code, "validate");
        assert_eq!(
            op.url,
            "http://hl7.org/fhir/OperationDefinition/Resource-validate"
        );
        assert_eq!(op.kind, OperationKind::Operation);
        assert!(!op.system);
        assert!(op.type_level);
        assert!(op.instance);
        assert_eq!(op.resource, vec!["Resource"]);
        assert!(!op.affects_state);
        assert_eq!(op.parameters.len(), 1);
        assert_eq!(op.parameters[0].name, "resource");
    }

    #[test]
    fn test_parse_query_operation() {
        let json = json!({
            "code": "everything",
            "kind": "query",
            "system": false,
            "type": true,
            "instance": true,
            "resource": ["Patient"]
        });

        let op = parse_operation_definition(&json).unwrap();

        assert_eq!(op.kind, OperationKind::Query);
    }

    #[test]
    fn test_parse_parameter_with_parts() {
        let json = json!({
            "name": "result",
            "use": "out",
            "min": 1,
            "max": "*",
            "part": [{
                "name": "code",
                "use": "out",
                "min": 1,
                "max": "1",
                "type": "code"
            }]
        });

        let param = parse_parameter(&json).unwrap();

        assert_eq!(param.name, "result");
        assert_eq!(param.use_, ParameterUse::Out);
        assert_eq!(param.min, 1);
        assert_eq!(param.max, "*");
        assert_eq!(param.parts.len(), 1);
        assert_eq!(param.parts[0].name, "code");
    }

    #[test]
    fn test_missing_code_fails() {
        let json = json!({
            "kind": "operation"
        });

        let result = parse_operation_definition(&json);
        assert!(result.is_err());
    }
}
