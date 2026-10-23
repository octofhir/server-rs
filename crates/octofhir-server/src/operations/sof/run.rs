//! $run operation for executing ViewDefinitions.
//!
//! This operation executes a ViewDefinition against the FHIR database
//! and returns the results as tabular data.

use crate::operations::handler::OperationLevel;

use async_trait::async_trait;
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};

use crate::operations::{OperationError, OperationHandler};
use crate::server::AppState;
use octofhir_sof::ViewDefinition;
use octofhir_storage::SearchParams;

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
    fn extract_limit(&self, params: &Value) -> Result<Option<usize>, OperationError> {
        let mut limits = params
            .get("parameter")
            .and_then(|p| p.as_array())
            .into_iter()
            .flatten()
            .filter(|param| matches!(param["name"].as_str(), Some("limit" | "_limit")));
        let Some(param) = limits.next() else {
            return Ok(None);
        };
        if limits.next().is_some() {
            return Err(OperationError::InvalidParameters(
                "Row limit must occur once".into(),
            ));
        }
        param["valueInteger"]
            .as_u64()
            .and_then(|limit| usize::try_from(limit).ok())
            .map(Some)
            .ok_or_else(|| {
                OperationError::InvalidParameters("Row limit must be a non-negative integer".into())
            })
    }

    /// Extract inline `resource` parameters (run against caller-supplied
    /// resources instead of the database — the SQL-on-FHIR spec `resource[]`).
    fn extract_inline_resources(
        &self,
        params: &Value,
    ) -> Result<Option<Vec<Value>>, OperationError> {
        let mut resources = Vec::new();
        for param in params
            .get("parameter")
            .and_then(|p| p.as_array())
            .into_iter()
            .flatten()
            .filter(|param| param["name"].as_str() == Some("resource"))
        {
            let resource = param
                .get("resource")
                .filter(|resource| resource["resourceType"].is_string())
                .ok_or_else(|| {
                    OperationError::InvalidParameters(
                        "Inline resource requires a FHIR resource".into(),
                    )
                })?;
            resources.push(resource.clone());
        }
        Ok((!resources.is_empty()).then_some(resources))
    }

    /// Resolve the ViewDefinition from `viewReference` (a saved VD id/reference),
    /// if present.
    async fn resolve_view_reference(
        &self,
        state: &AppState,
        context: &crate::operations::OperationContext,
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

        let id = reference
            .strip_prefix("ViewDefinition/")
            .unwrap_or(reference);
        octofhir_core::validate_id(id).map_err(|_| {
            OperationError::InvalidParameters(
                "viewReference requires a local ViewDefinition/id or bare id".into(),
            )
        })?;
        let stored = context
            .read(state, "ViewDefinition", id)
            .await?
            .ok_or_else(|| OperationError::NotFound(format!("ViewDefinition/{} not found", id)))?;

        let view = ViewDefinition::from_json(&stored.resource).map_err(|e| {
            OperationError::Internal(format!("Invalid stored ViewDefinition: {}", e))
        })?;
        Ok(Some(view))
    }

    /// Load authorized source pages with an explicit memory budget.
    async fn load_source_resources(
        &self,
        state: &AppState,
        context: &crate::operations::OperationContext,
        resource_type: &str,
    ) -> Result<Vec<Value>, OperationError> {
        let mut resources = Vec::new();
        let mut offset: u32 = 0;
        let mut bytes: usize = 0;
        let limits = state.config.search.include_limits;

        loop {
            let mut params = SearchParams::new().with_sort("_id", false);
            params.count = Some(FETCH_PAGE_SIZE);
            params.offset = Some(offset);

            let page = context.search(state, resource_type, &params).await?;

            let fetched = page.entries.len();
            if page.has_more && fetched == 0 {
                return Err(OperationError::Internal(
                    "View source pagination made no progress".into(),
                ));
            }
            for entry in &page.entries {
                bytes = bytes.saturating_add(
                    serde_json::to_vec(&entry.resource)
                        .map_err(|error| OperationError::Internal(error.to_string()))?
                        .len(),
                );
            }
            if bytes > limits.max_bytes
                || resources.len().saturating_add(fetched) > limits.max_resources
            {
                return Err(OperationError::InvalidParameters(
                    "View source exceeds the configured resource or byte budget".into(),
                ));
            }
            resources.extend(page.entries.into_iter().map(|entry| entry.resource));

            if !page.has_more {
                break;
            }
            offset = offset
                .checked_add(u32::try_from(fetched).map_err(|_| {
                    OperationError::InvalidParameters("View source pagination overflow".into())
                })?)
                .ok_or_else(|| {
                    OperationError::InvalidParameters("View source pagination overflow".into())
                })?;
        }

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
        context: &crate::operations::OperationContext,
        view_def: &ViewDefinition,
        inline: Option<Vec<Value>>,
        limit: Option<usize>,
    ) -> Result<Value, OperationError> {
        let resources = match inline {
            None => {
                self.load_source_resources(state, context, &view_def.resource)
                    .await?
            }
            Some(resources) => resources,
        };
        let limits = state.config.search.include_limits;
        let source_bytes = resources
            .iter()
            .try_fold(0_usize, |bytes, resource| {
                serde_json::to_vec(resource).map(|json| bytes.saturating_add(json.len()))
            })
            .map_err(|error| OperationError::Internal(error.to_string()))?;
        if resources.len() > limits.max_resources || source_bytes > limits.max_bytes {
            return Err(OperationError::InvalidParameters(
                "View source exceeds the configured resource or byte budget".into(),
            ));
        }
        let compiled = octofhir_sof::CompiledView::compile(view_def)
            .await
            .map_err(|error| {
                OperationError::InvalidParameters(format!("Invalid ViewDefinition: {error}"))
            })?;
        let mut rows = Vec::new();
        let mut output_bytes: usize = 0;
        for resource in &resources {
            if limit.is_some_and(|limit| rows.len() >= limit) {
                break;
            }
            let data = compiled.execute_resource(resource).await.map_err(|error| {
                OperationError::InvalidParameters(format!(
                    "Failed to execute ViewDefinition: {error}"
                ))
            })?;
            let result = octofhir_sof::ViewResult {
                row_count: data.len(),
                data,
                columns: compiled.columns().to_vec(),
            };
            for row in result.to_json_array() {
                if limit.is_some_and(|limit| rows.len() >= limit) {
                    break;
                }
                output_bytes = output_bytes.saturating_add(
                    serde_json::to_vec(&row)
                        .map_err(|error| OperationError::Internal(error.to_string()))?
                        .len(),
                );
                if rows.len() >= limits.max_resources || output_bytes > limits.max_bytes {
                    return Err(OperationError::InvalidParameters(
                        "View output exceeds the configured resource or byte budget".into(),
                    ));
                }
                rows.push(row);
            }
            tokio::task::yield_now().await;
        }
        let data = serde_json::to_vec(&rows)
            .map_err(|error| OperationError::Internal(error.to_string()))?;
        if rows.len() > state.config.search.include_limits.max_resources
            || data.len() > state.config.search.include_limits.max_bytes
        {
            return Err(OperationError::InvalidParameters(
                "View output exceeds the configured resource or byte budget".into(),
            ));
        }

        // Build response with metadata
        let columns: Vec<Value> = compiled
            .columns()
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
                        "data": STANDARD.encode(data)
                    }
                }
            ]
        }))
    }
}

#[async_trait]
impl OperationHandler for ViewDefinitionRunOperation {
    fn supports_level(&self, level: OperationLevel, resource_type: Option<&str>) -> bool {
        self.enabled && level != OperationLevel::System && resource_type == Some("ViewDefinition")
    }

    fn code(&self) -> &str {
        "run"
    }

    async fn handle_type(
        &self,
        state: &AppState,
        context: &crate::operations::OperationContext,
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
        let view_def = match self.resolve_view_reference(state, context, params).await? {
            Some(view) => view,
            None => self.extract_view_definition(params)?,
        };
        let inline = self.extract_inline_resources(params)?;
        let limit = self.extract_limit(params)?;

        self.execute_view(state, context, &view_def, inline, limit)
            .await
    }

    async fn handle_instance(
        &self,
        state: &AppState,
        context: &crate::operations::OperationContext,
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
        let stored = context
            .read(state, "ViewDefinition", id)
            .await?
            .ok_or_else(|| OperationError::NotFound(format!("ViewDefinition/{} not found", id)))?;

        let view_def = ViewDefinition::from_json(&stored.resource).map_err(|e| {
            OperationError::Internal(format!("Invalid stored ViewDefinition: {}", e))
        })?;

        let inline = self.extract_inline_resources(params)?;
        let limit = self.extract_limit(params)?;
        self.execute_view(state, context, &view_def, inline, limit)
            .await
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

        assert_eq!(op.extract_limit(&params).unwrap(), Some(100));

        let params_no_limit = json!({
            "resourceType": "Parameters",
            "parameter": []
        });

        assert_eq!(op.extract_limit(&params_no_limit).unwrap(), None);
        assert!(
            op.extract_limit(&json!({"parameter":[{"name":"_limit","valueInteger":-1}]}))
                .is_err()
        );
        assert!(op.extract_limit(&json!({"parameter":[{"name":"limit","valueInteger":1},{"name":"_limit","valueInteger":2}]})).is_err());
    }

    #[test]
    fn test_check_enabled_error() {
        let op = ViewDefinitionRunOperation::new(false);
        let result = op.check_enabled();
        assert!(result.is_err());
        assert!(matches!(result, Err(OperationError::NotSupported(_))));
    }
}
