//! $sql operation for generating SQL from ViewDefinitions.
//!
//! This operation generates SQL from a ViewDefinition without executing it,
//! useful for previewing the generated query.

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::operations::{OperationError, OperationHandler};
use crate::server::AppState;
use octofhir_sof::{SqlGenerator, ViewDefinition};

/// The $sql operation handler for ViewDefinition.
///
/// Generates SQL from a ViewDefinition for preview without execution.
///
/// # Usage
///
/// ## Type-level (with inline ViewDefinition)
/// ```http
/// POST /fhir/ViewDefinition/$sql
/// Content-Type: application/fhir+json
///
/// {
///   "resourceType": "Parameters",
///   "parameter": [
///     { "name": "viewDefinition", "resource": { /* ViewDefinition */ } }
///   ]
/// }
/// ```
///
/// ## Instance-level (from saved ViewDefinition)
/// ```http
/// POST /fhir/ViewDefinition/my-view/$sql
/// ```
pub struct ViewDefinitionSqlOperation {
    enabled: bool,
}

impl ViewDefinitionSqlOperation {
    /// Create a new $sql operation handler.
    ///
    /// # Arguments
    /// * `enabled` - Whether SQL on FHIR feature is enabled
    pub fn new(enabled: bool) -> Self {
        Self { enabled }
    }

    /// Check if the feature is enabled and return an error if not.
    fn check_enabled(&self) -> Result<(), OperationError> {
        if !self.enabled {
            return Err(OperationError::NotSupported(
                "SQL on FHIR is not enabled. Please set sql_on_fhir.enabled = true in configuration.".to_string(),
            ));
        }
        Ok(())
    }

    /// Extract ViewDefinition from parameters.
    fn extract_view_definition(&self, params: &Value) -> Result<ViewDefinition, OperationError> {
        // Look for viewDefinition parameter in Parameters resource
        let parameters = params
            .get("parameter")
            .and_then(|p| p.as_array())
            .ok_or_else(|| {
                OperationError::InvalidParameters("Missing 'parameter' array".to_string())
            })?;

        for param in parameters {
            let name = param.get("name").and_then(|n| n.as_str());
            if name == Some("viewDefinition")
                && let Some(resource) = param.get("resource")
            {
                return ViewDefinition::from_json(resource).map_err(|e| {
                    OperationError::InvalidParameters(format!("Invalid ViewDefinition: {}", e))
                });
            }
        }

        Err(OperationError::InvalidParameters(
            "Missing 'viewDefinition' parameter".to_string(),
        ))
    }

    /// Generate SQL from the ViewDefinition.
    fn generate_sql(&self, view_def: &ViewDefinition) -> Result<Value, OperationError> {
        let generator = SqlGenerator::new();
        let generated = generator
            .generate(view_def)
            .map_err(|e| OperationError::Internal(format!("Failed to generate SQL: {}", e)))?;

        // Return as Parameters resource with the SQL string
        Ok(json!({
            "resourceType": "Parameters",
            "parameter": [
                {
                    "name": "sql",
                    "valueString": generated.sql
                },
                {
                    "name": "columns",
                    "part": generated.columns.iter().map(|c| {
                        json!({
                            "name": c.name.clone(),
                            "valueString": c.col_type.to_string()
                        })
                    }).collect::<Vec<_>>()
                }
            ]
        }))
    }
}

#[async_trait]
impl OperationHandler for ViewDefinitionSqlOperation {
    fn code(&self) -> &str {
        "sql"
    }

    async fn handle_type(
        &self,
        _state: &AppState,
        resource_type: &str,
        params: &Value,
    ) -> Result<Value, OperationError> {
        self.check_enabled()?;

        // This operation only works on ViewDefinition
        if resource_type != "ViewDefinition" {
            return Err(OperationError::NotSupported(format!(
                "Operation $sql is only supported on ViewDefinition, not {}",
                resource_type
            )));
        }

        // Extract and generate SQL
        let view_def = self.extract_view_definition(params)?;
        self.generate_sql(&view_def)
    }

    async fn handle_instance(
        &self,
        state: &AppState,
        resource_type: &str,
        id: &str,
        _params: &Value,
    ) -> Result<Value, OperationError> {
        self.check_enabled()?;

        // This operation only works on ViewDefinition
        if resource_type != "ViewDefinition" {
            return Err(OperationError::NotSupported(format!(
                "Operation $sql is only supported on ViewDefinition, not {}",
                resource_type
            )));
        }

        // Load the ViewDefinition from storage
        let storage = &state.storage;
        let stored = storage
            .read("ViewDefinition", id)
            .await
            .map_err(|e| OperationError::Internal(format!("Storage error: {}", e)))?
            .ok_or_else(|| OperationError::NotFound(format!("ViewDefinition/{} not found", id)))?;

        let view_def = ViewDefinition::from_json(&stored.resource).map_err(|e| {
            OperationError::Internal(format!("Invalid stored ViewDefinition: {}", e))
        })?;

        self.generate_sql(&view_def)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_extract_view_definition() {
        let op = ViewDefinitionSqlOperation::new(true);

        let params = json!({
            "resourceType": "Parameters",
            "parameter": [
                {
                    "name": "viewDefinition",
                    "resource": {
                        "resourceType": "ViewDefinition",
                        "name": "test_view",
                        "status": "active",
                        "resource": "Patient",
                        "select": [{
                            "column": [{
                                "name": "id",
                                "path": "id"
                            }]
                        }]
                    }
                }
            ]
        });

        let view_def = op.extract_view_definition(&params).unwrap();
        assert_eq!(view_def.name, "test_view");
        assert_eq!(view_def.resource, "Patient");
    }

    #[test]
    fn test_check_enabled_error() {
        let op = ViewDefinitionSqlOperation::new(false);
        let result = op.check_enabled();
        assert!(result.is_err());
        assert!(matches!(result, Err(OperationError::NotSupported(_))));
    }
}
