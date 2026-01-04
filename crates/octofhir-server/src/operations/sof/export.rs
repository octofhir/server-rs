//! $export operation for async bulk export of ViewDefinition results.
//!
//! This operation executes a ViewDefinition against the FHIR database
//! and exports the results as NDJSON files for bulk download.
//!
//! See: <https://build.fhir.org/ig/FHIR/sql-on-fhir-v2/OperationDefinition-ViewDefinitionExport.html>

use async_trait::async_trait;
use chrono::Utc;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::async_jobs::AsyncJobRequest;
use crate::config::BulkExportConfig;
use crate::operations::bulk::NdjsonWriter;
use crate::operations::{OperationError, OperationHandler};
use crate::server::AppState;
use octofhir_sof::{ViewDefinition, ViewRunner};

/// The ViewDefinition $export operation handler.
///
/// Executes a ViewDefinition asynchronously and exports results to NDJSON files.
/// Per SQL on FHIR spec, this uses the `$export` operation on ViewDefinition resources.
///
/// # Usage
///
/// ## Type-level (with inline ViewDefinition)
/// ```http
/// POST /fhir/ViewDefinition/$export
/// Prefer: respond-async
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
/// ## Instance-level (export saved ViewDefinition)
/// ```http
/// POST /fhir/ViewDefinition/my-view/$export
/// Prefer: respond-async
/// ```
///
/// ## Response
/// Returns 202 Accepted with Content-Location header pointing to status URL.
/// When complete, status returns manifest with file download URLs.
pub struct ViewDefinitionExportOperation {
    enabled: bool,
    config: BulkExportConfig,
}

impl ViewDefinitionExportOperation {
    /// Create a new ViewDefinition $export operation handler.
    pub fn new(enabled: bool, config: BulkExportConfig) -> Self {
        Self { enabled, config }
    }

    /// Check if the feature is enabled and return an error if not.
    fn check_enabled(&self) -> Result<(), OperationError> {
        if !self.enabled {
            return Err(OperationError::NotSupported(
                "SQL on FHIR export is not enabled. Please set sql_on_fhir.enabled = true in configuration.".to_string(),
            ));
        }
        if !self.config.enabled {
            return Err(OperationError::NotSupported(
                "Bulk export is disabled on this server".to_string(),
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
                && let Some(resource) = param.get("resource") {
                    return ViewDefinition::from_json(resource).map_err(|e| {
                        OperationError::InvalidParameters(format!(
                            "Invalid ViewDefinition: {}",
                            e
                        ))
                    });
                }
        }

        Err(OperationError::InvalidParameters(
            "Missing 'viewDefinition' parameter".to_string(),
        ))
    }

    /// Submit an export job and return the status URL.
    async fn submit_export(
        &self,
        state: &AppState,
        view_def: ViewDefinition,
        request_url: &str,
    ) -> Result<Value, OperationError> {
        // Store job parameters as JSON for the async job
        let job_params = json!({
            "type": "viewdefinition_export",
            "view_definition": view_def,
            "config": {
                "export_path": self.config.export_path,
                "max_resources_per_file": self.config.max_resources_per_file,
                "batch_size": self.config.batch_size,
                "retention_hours": self.config.retention_hours,
            },
            "request_url": request_url,
        });

        // Submit to async job manager
        let async_request = AsyncJobRequest {
            request_type: "viewdefinition_export".to_string(),
            method: "POST".to_string(),
            url: request_url.to_string(),
            body: Some(job_params),
            headers: None,
            client_id: None,
        };

        let job_id = state
            .async_job_manager
            .submit_job(async_request)
            .await
            .map_err(|e| OperationError::Internal(format!("Failed to submit export job: {}", e)))?;

        tracing::info!(
            job_id = %job_id,
            view_name = %view_def.name,
            "ViewDefinition export job submitted"
        );

        // Return accepted response with status location
        Ok(json!({
            "status": "accepted",
            "job_id": job_id.to_string(),
            "status_url": format!("{}/_async-status/{}", state.base_url, job_id),
        }))
    }
}

#[async_trait]
impl OperationHandler for ViewDefinitionExportOperation {
    fn code(&self) -> &str {
        // Note: This handler is for ViewDefinition only, registered separately
        // from the bulk data $export. The router ensures correct dispatch.
        "viewdefinition-export"
    }

    async fn handle_type(
        &self,
        state: &AppState,
        resource_type: &str,
        params: &Value,
    ) -> Result<Value, OperationError> {
        self.check_enabled()?;

        // This handler only works on ViewDefinition
        if resource_type != "ViewDefinition" {
            return Err(OperationError::NotSupported(format!(
                "ViewDefinition $export is only supported on ViewDefinition, not {}",
                resource_type
            )));
        }

        // Extract ViewDefinition from parameters
        let view_def = self.extract_view_definition(params)?;
        let request_url = format!("{}/fhir/ViewDefinition/$export", state.base_url);

        self.submit_export(state, view_def, &request_url).await
    }

    async fn handle_instance(
        &self,
        state: &AppState,
        resource_type: &str,
        id: &str,
        _params: &Value,
    ) -> Result<Value, OperationError> {
        self.check_enabled()?;

        // This handler only works on ViewDefinition
        if resource_type != "ViewDefinition" {
            return Err(OperationError::NotSupported(format!(
                "ViewDefinition $export is only supported on ViewDefinition, not {}",
                resource_type
            )));
        }

        // Load the ViewDefinition from storage
        let storage = &state.storage;
        let stored = storage
            .read("ViewDefinition", id)
            .await
            .map_err(|e| OperationError::Internal(format!("Storage error: {}", e)))?
            .ok_or_else(|| {
                OperationError::NotFound(format!("ViewDefinition/{} not found", id))
            })?;

        let view_def = ViewDefinition::from_json(&stored.resource).map_err(|e| {
            OperationError::Internal(format!("Invalid stored ViewDefinition: {}", e))
        })?;

        let request_url = format!(
            "{}/fhir/ViewDefinition/{}/$export",
            state.base_url, id
        );

        self.submit_export(state, view_def, &request_url).await
    }
}

/// Execute a ViewDefinition export job.
///
/// This function is called by the async job executor to perform the actual export.
pub async fn execute_viewdefinition_export(
    state: AppState,
    job_id: Uuid,
    params: Value,
) -> Result<Value, String> {
    tracing::info!(job_id = %job_id, "Starting ViewDefinition export execution");

    // Parse job parameters
    let view_def: ViewDefinition = params
        .get("view_definition")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .ok_or_else(|| "Missing or invalid view_definition".to_string())?;

    let config = params.get("config").ok_or("Missing config")?;
    let export_path = config
        .get("export_path")
        .and_then(|v| v.as_str())
        .ok_or("Missing export_path")?;
    let max_resources_per_file = config
        .get("max_resources_per_file")
        .and_then(|v| v.as_u64())
        .unwrap_or(100_000) as usize;
    let batch_size = config
        .get("batch_size")
        .and_then(|v| v.as_u64())
        .unwrap_or(10_000) as usize;

    let request_url = params
        .get("request_url")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    tracing::info!(
        job_id = %job_id,
        view_name = %view_def.name,
        resource = %view_def.resource,
        "Executing ViewDefinition export"
    );

    // Update progress: starting
    if let Err(e) = state.async_job_manager.update_progress(job_id, 0.1).await {
        tracing::warn!(error = %e, "Failed to update job progress");
    }

    // Create view runner
    let pool = state.db_pool.as_ref().clone();
    let runner = ViewRunner::new(pool);

    // Execute the view
    let result = runner
        .run(&view_def)
        .await
        .map_err(|e| format!("Failed to execute ViewDefinition: {}", e))?;

    // Update progress: query complete
    if let Err(e) = state.async_job_manager.update_progress(job_id, 0.5).await {
        tracing::warn!(error = %e, "Failed to update job progress");
    }

    // Create NDJSON writer
    let mut writer = NdjsonWriter::new(export_path, job_id, max_resources_per_file)
        .await
        .map_err(|e| format!("Failed to create NDJSON writer: {}", e))?;

    let view_name = &view_def.name;
    let rows = result.to_json_array();
    let total_rows = rows.len();

    tracing::info!(
        job_id = %job_id,
        view_name = %view_name,
        total_rows = total_rows,
        "Writing ViewDefinition results to NDJSON"
    );

    // Write rows in batches
    let mut written = 0;
    for (i, row) in rows.iter().enumerate() {
        writer
            .write_resource(view_name, row)
            .await
            .map_err(|e| format!("Failed to write row: {}", e))?;

        written += 1;

        // Update progress periodically
        if i % batch_size == 0 && total_rows > 0 {
            let progress = 0.5 + (0.4 * (i as f32 / total_rows as f32));
            if let Err(e) = state.async_job_manager.update_progress(job_id, progress).await {
                tracing::warn!(error = %e, "Failed to update job progress");
            }
        }
    }

    // Finish writing and get file information
    let files = writer
        .finish()
        .await
        .map_err(|e| format!("Failed to finish writing: {}", e))?;

    // Build output manifest
    let mut output = Vec::new();
    for (resource_type, file_list) in files {
        for (path, count) in file_list {
            let filename = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown");

            output.push(json!({
                "type": resource_type,
                "url": format!("{}/fhir/_bulk-files/{}/{}", state.base_url, job_id, filename),
                "count": count,
            }));
        }
    }

    tracing::info!(
        job_id = %job_id,
        view_name = %view_name,
        total_rows = written,
        output_files = output.len(),
        "ViewDefinition export completed"
    );

    Ok(json!({
        "transactionTime": Utc::now().to_rfc3339(),
        "request": request_url,
        "requiresAccessToken": true,
        "output": output,
        "error": [],
        "extension": [{
            "url": "http://octofhir.org/export/viewDefinition",
            "valueString": view_def.name
        }, {
            "url": "http://octofhir.org/export/rowCount",
            "valueInteger": written
        }]
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_extract_view_definition() {
        let config = BulkExportConfig::default();
        let op = ViewDefinitionExportOperation::new(true, config);

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
    fn test_extract_view_definition_missing() {
        let config = BulkExportConfig::default();
        let op = ViewDefinitionExportOperation::new(true, config);

        let params = json!({
            "resourceType": "Parameters",
            "parameter": []
        });

        let result = op.extract_view_definition(&params);
        assert!(result.is_err());
    }

    #[test]
    fn test_check_enabled_disabled() {
        let config = BulkExportConfig::default();
        let op = ViewDefinitionExportOperation::new(false, config);

        let result = op.check_enabled();
        assert!(result.is_err());
        assert!(matches!(result, Err(OperationError::NotSupported(_))));
    }

    #[test]
    fn test_check_enabled_bulk_disabled() {
        let mut config = BulkExportConfig::default();
        config.enabled = false;
        let op = ViewDefinitionExportOperation::new(true, config);

        let result = op.check_enabled();
        assert!(result.is_err());
    }
}
