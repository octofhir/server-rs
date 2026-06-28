//! $run operation for executing ViewDefinitions.
//!
//! This operation executes a ViewDefinition against the FHIR database
//! and returns the results as tabular data.

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::operations::{OperationError, OperationHandler};
use crate::server::AppState;
use octofhir_sof::ViewDefinition;
use octofhir_storage::SearchParams;

/// Safety cap on how many source resources are loaded into memory for an
/// in-memory `$run`. Large datasets should use `$export` (streamed) instead.
const MAX_SOURCE_RESOURCES: usize = 10_000;
/// Page size used when streaming source resources out of storage.
const FETCH_PAGE_SIZE: u32 = 500;

/// The $run operation handler for ViewDefinition.
///
/// Executes a ViewDefinition and returns tabular results.
///
/// # Usage
///
/// ## Type-level (with inline ViewDefinition)
/// ```http
/// POST /fhir/ViewDefinition/$run
/// Content-Type: application/fhir+json
///
/// {
///   "resourceType": "Parameters",
///   "parameter": [
///     { "name": "viewDefinition", "resource": { /* ViewDefinition */ } },
///     { "name": "limit", "valueInteger": 100 }
///   ]
/// }
/// ```
///
/// ## Instance-level (execute saved ViewDefinition)
/// ```http
/// POST /fhir/ViewDefinition/my-view/$run
/// ```
pub struct ViewDefinitionRunOperation {
    enabled: bool,
}

impl ViewDefinitionRunOperation {
    /// Create a new $run operation handler.
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

    /// Extract limit parameter.
    fn extract_limit(&self, params: &Value) -> Option<usize> {
        params
            .get("parameter")
            .and_then(|p| p.as_array())
            .and_then(|parameters| {
                parameters.iter().find_map(|param| {
                    let name = param.get("name").and_then(|n| n.as_str());
                    if name == Some("limit") || name == Some("_limit") {
                        param
                            .get("valueInteger")
                            .and_then(|v| v.as_i64())
                            .map(|v| v as usize)
                    } else {
                        None
                    }
                })
            })
    }

    /// Extract inline `resource` parameters (run against caller-supplied
    /// resources instead of the database — the SQL-on-FHIR spec `resource[]`).
    fn extract_inline_resources(&self, params: &Value) -> Vec<Value> {
        params
            .get("parameter")
            .and_then(|p| p.as_array())
            .map(|parameters| {
                parameters
                    .iter()
                    .filter(|param| param.get("name").and_then(|n| n.as_str()) == Some("resource"))
                    .filter_map(|param| param.get("resource").cloned())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Resolve the ViewDefinition from `viewReference` (a saved VD id/reference),
    /// if present.
    async fn resolve_view_reference(
        &self,
        state: &AppState,
        params: &Value,
    ) -> Result<Option<ViewDefinition>, OperationError> {
        let reference = params
            .get("parameter")
            .and_then(|p| p.as_array())
            .and_then(|parameters| {
                parameters.iter().find_map(|param| {
                    if param.get("name").and_then(|n| n.as_str()) == Some("viewReference") {
                        param
                            .get("valueReference")
                            .and_then(|r| r.get("reference"))
                            .or_else(|| param.get("valueString"))
                            .and_then(|v| v.as_str())
                    } else {
                        None
                    }
                })
            });

        let Some(reference) = reference else {
            return Ok(None);
        };

        // Accept either a bare id or a "ViewDefinition/{id}" reference.
        let id = reference
            .rsplit('/')
            .next()
            .unwrap_or(reference)
            .to_string();

        let stored = state
            .storage
            .read("ViewDefinition", &id)
            .await
            .map_err(|e| OperationError::Internal(format!("Storage error: {}", e)))?
            .ok_or_else(|| OperationError::NotFound(format!("ViewDefinition/{} not found", id)))?;

        let view = ViewDefinition::from_json(&stored.resource).map_err(|e| {
            OperationError::Internal(format!("Invalid stored ViewDefinition: {}", e))
        })?;
        Ok(Some(view))
    }

    /// Load source resources of the view's resource type from storage, paging
    /// through results up to [`MAX_SOURCE_RESOURCES`].
    async fn load_source_resources(
        &self,
        state: &AppState,
        resource_type: &str,
    ) -> Result<Vec<Value>, OperationError> {
        let mut resources = Vec::new();
        let mut offset: u32 = 0;

        loop {
            let mut params = SearchParams::new();
            params.count = Some(FETCH_PAGE_SIZE);
            params.offset = Some(offset);

            let page = state
                .storage
                .search(resource_type, &params)
                .await
                .map_err(|e| OperationError::Internal(format!("Storage error: {}", e)))?;

            let fetched = page.entries.len();
            resources.extend(page.entries.into_iter().map(|entry| entry.resource));

            if fetched < FETCH_PAGE_SIZE as usize
                || !page.has_more
                || resources.len() >= MAX_SOURCE_RESOURCES
            {
                break;
            }
            offset += FETCH_PAGE_SIZE;
        }

        resources.truncate(MAX_SOURCE_RESOURCES);
        Ok(resources)
    }

    /// Execute the ViewDefinition and return results.
    ///
    /// When `inline` resources are supplied they are used as the data source
    /// (database-free run); otherwise resources of the view's type are loaded
    /// from storage. Evaluation uses the conformant in-memory engine from
    /// `octofhir-sof`.
    async fn execute_view(
        &self,
        state: &AppState,
        view_def: &ViewDefinition,
        inline: Vec<Value>,
        limit: Option<usize>,
    ) -> Result<Value, OperationError> {
        let resources = if inline.is_empty() {
            self.load_source_resources(state, &view_def.resource).await?
        } else {
            inline
        };

        let result = octofhir_sof::execute(view_def, &resources)
            .await
            .map_err(|e| {
                OperationError::Internal(format!("Failed to execute ViewDefinition: {}", e))
            })?;

        // Apply row limit if specified
        let rows: Vec<Value> = if let Some(limit) = limit {
            result.to_json_array().into_iter().take(limit).collect()
        } else {
            result.to_json_array()
        };

        // Build response with metadata
        let columns: Vec<Value> = result
            .columns
            .iter()
            .map(|c| {
                json!({
                    "name": c.name,
                    "type": c.col_type.to_string()
                })
            })
            .collect();

        Ok(json!({
            "resourceType": "Parameters",
            "parameter": [
                {
                    "name": "columns",
                    "part": columns.iter().map(|c| {
                        json!({
                            "name": c["name"].as_str().unwrap_or(""),
                            "valueString": c["type"].as_str().unwrap_or("string")
                        })
                    }).collect::<Vec<_>>()
                },
                {
                    "name": "rowCount",
                    "valueInteger": rows.len()
                },
                {
                    "name": "rows",
                    "resource": {
                        "resourceType": "Binary",
                        "contentType": "application/json",
                        "data": serde_json::to_string(&rows).unwrap_or_default()
                    }
                }
            ]
        }))
    }
}

#[async_trait]
impl OperationHandler for ViewDefinitionRunOperation {
    fn code(&self) -> &str {
        "run"
    }

    async fn handle_type(
        &self,
        state: &AppState,
        resource_type: &str,
        params: &Value,
    ) -> Result<Value, OperationError> {
        self.check_enabled()?;

        // This operation only works on ViewDefinition
        if resource_type != "ViewDefinition" {
            return Err(OperationError::NotSupported(format!(
                "Operation $run is only supported on ViewDefinition, not {}",
                resource_type
            )));
        }

        // Resolve the ViewDefinition: inline `viewDefinition` or `viewReference`.
        let view_def = match self.resolve_view_reference(state, params).await? {
            Some(view) => view,
            None => self.extract_view_definition(params)?,
        };
        let inline = self.extract_inline_resources(params);
        let limit = self.extract_limit(params);

        self.execute_view(state, &view_def, inline, limit).await
    }

    async fn handle_instance(
        &self,
        state: &AppState,
        resource_type: &str,
        id: &str,
        params: &Value,
    ) -> Result<Value, OperationError> {
        self.check_enabled()?;

        // This operation only works on ViewDefinition
        if resource_type != "ViewDefinition" {
            return Err(OperationError::NotSupported(format!(
                "Operation $run is only supported on ViewDefinition, not {}",
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

        let inline = self.extract_inline_resources(params);
        let limit = self.extract_limit(params);
        self.execute_view(state, &view_def, inline, limit).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_extract_view_definition() {
        let op = ViewDefinitionRunOperation::new(true);

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
        assert_eq!(view_def.name.as_deref(), Some("test_view"));
        assert_eq!(view_def.resource, "Patient");
    }

    #[test]
    fn test_extract_limit() {
        let op = ViewDefinitionRunOperation::new(true);

        let params = json!({
            "resourceType": "Parameters",
            "parameter": [
                { "name": "limit", "valueInteger": 100 }
            ]
        });

        assert_eq!(op.extract_limit(&params), Some(100));

        let params_no_limit = json!({
            "resourceType": "Parameters",
            "parameter": []
        });

        assert_eq!(op.extract_limit(&params_no_limit), None);
    }

    #[test]
    fn test_check_enabled_error() {
        let op = ViewDefinitionRunOperation::new(false);
        let result = op.check_enabled();
        assert!(result.is_err());
        assert!(matches!(result, Err(OperationError::NotSupported(_))));
    }
}
