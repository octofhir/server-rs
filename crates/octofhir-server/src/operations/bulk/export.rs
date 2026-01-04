//! Bulk export operation handler ($export)
//!
//! Implements the FHIR Bulk Data Access $export operation for system,
//! patient, group, and ViewDefinition level exports.
//!
//! - System: `/$export` - Export all resources
//! - Patient: `/Patient/$export` - Export patient compartment data
//! - Group: `/Group/{id}/$export` - Export group member data
//! - ViewDefinition: `/ViewDefinition/$export` - Export ViewDefinition results (SQL on FHIR)

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::async_jobs::AsyncJobRequest;
use crate::config::BulkExportConfig;
use crate::operations::handler::{OperationError, OperationHandler};
use crate::server::AppState;
use octofhir_sof::ViewDefinition;

use super::status::{BulkExportLevel, BulkExportParams};
use super::writer::NdjsonWriter;
use super::NDJSON_CONTENT_TYPE;

/// The $export operation handler
///
/// This unified handler supports:
/// - Bulk Data Access exports (system, patient, group level)
/// - SQL on FHIR ViewDefinition exports
pub struct ExportOperation {
    /// Configuration for bulk exports
    config: BulkExportConfig,
    /// Whether SQL on FHIR is enabled
    sql_on_fhir_enabled: bool,
}

impl ExportOperation {
    /// Create a new export operation handler with the given configuration
    pub fn new(config: BulkExportConfig) -> Self {
        Self {
            config,
            sql_on_fhir_enabled: false,
        }
    }

    /// Create a new export operation handler with SQL on FHIR support
    pub fn with_sql_on_fhir(config: BulkExportConfig, sql_on_fhir_enabled: bool) -> Self {
        Self {
            config,
            sql_on_fhir_enabled,
        }
    }

    /// Parse export parameters from the operation params
    fn parse_params(&self, params: &Value) -> Result<BulkExportParams, OperationError> {
        let output_format = params
            .get("_outputFormat")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        // Validate output format if specified
        if let Some(ref fmt) = output_format
            && fmt != NDJSON_CONTENT_TYPE && fmt != "ndjson" {
                return Err(OperationError::InvalidParameters(format!(
                    "Unsupported _outputFormat: {}. Only {} is supported.",
                    fmt, NDJSON_CONTENT_TYPE
                )));
            }

        let since = params
            .get("_since")
            .and_then(|v| v.as_str())
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&Utc));

        let resource_types = params
            .get("_type")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let type_filter = params
            .get("_typeFilter")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let group_id = params
            .get("groupId")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        Ok(BulkExportParams {
            output_format,
            since,
            resource_types,
            type_filter,
            group_id,
        })
    }

    /// Submit an export job and return the status URL
    async fn submit_export(
        &self,
        state: &AppState,
        level: BulkExportLevel,
        params: BulkExportParams,
        request_url: &str,
    ) -> Result<Value, OperationError> {
        if !self.config.enabled {
            return Err(OperationError::NotSupported(
                "Bulk export is disabled on this server".to_string(),
            ));
        }

        // Store job parameters as JSON for the async job
        // The async job manager will assign its own job ID
        let job_params = json!({
            "level": level.to_string(),
            "params": params,
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
            request_type: "bulk_export".to_string(),
            method: "GET".to_string(),
            url: request_url.to_string(),
            body: Some(job_params),
            headers: None,
            client_id: None, // TODO: Extract from auth context
        };

        let job_id = state
            .async_job_manager
            .submit_job(async_request)
            .await
            .map_err(|e| OperationError::Internal(format!("Failed to submit export job: {}", e)))?;

        tracing::info!(
            job_id = %job_id,
            level = %level,
            "Bulk export job submitted"
        );

        // Return accepted response with status location
        // The actual response headers are set by the router
        Ok(json!({
            "status": "accepted",
            "job_id": job_id.to_string(),
            "status_url": format!("{}/_async-status/{}", state.base_url, job_id),
        }))
    }

    /// Check if SQL on FHIR is enabled
    fn check_sql_on_fhir_enabled(&self) -> Result<(), OperationError> {
        if !self.sql_on_fhir_enabled {
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

    /// Extract ViewDefinition from Parameters resource
    fn extract_view_definition(&self, params: &Value) -> Result<ViewDefinition, OperationError> {
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

    /// Submit a ViewDefinition export job
    async fn submit_viewdefinition_export(
        &self,
        state: &AppState,
        view_def: ViewDefinition,
        request_url: &str,
    ) -> Result<Value, OperationError> {
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

        Ok(json!({
            "status": "accepted",
            "job_id": job_id.to_string(),
            "status_url": format!("{}/_async-status/{}", state.base_url, job_id),
        }))
    }
}

#[async_trait]
impl OperationHandler for ExportOperation {
    fn code(&self) -> &str {
        "export"
    }

    /// System-level export: GET /$export
    async fn handle_system(
        &self,
        state: &AppState,
        params: &Value,
    ) -> Result<Value, OperationError> {
        let export_params = self.parse_params(params)?;
        let request_url = format!("{}/fhir/$export", state.base_url);

        self.submit_export(state, BulkExportLevel::System, export_params, &request_url)
            .await
    }

    /// Type-level export: GET /Patient/$export or POST /ViewDefinition/$export
    async fn handle_type(
        &self,
        state: &AppState,
        resource_type: &str,
        params: &Value,
    ) -> Result<Value, OperationError> {
        match resource_type {
            "Patient" => {
                let export_params = self.parse_params(params)?;
                let request_url = format!("{}/fhir/Patient/$export", state.base_url);

                self.submit_export(state, BulkExportLevel::Patient, export_params, &request_url)
                    .await
            }
            "ViewDefinition" => {
                self.check_sql_on_fhir_enabled()?;
                let view_def = self.extract_view_definition(params)?;
                let request_url = format!("{}/fhir/ViewDefinition/$export", state.base_url);
                self.submit_viewdefinition_export(state, view_def, &request_url)
                    .await
            }
            _ => Err(OperationError::NotSupported(format!(
                "$export is only supported on Patient or ViewDefinition, not {}",
                resource_type
            ))),
        }
    }

    /// Instance-level export: GET /Group/{id}/$export or POST /ViewDefinition/{id}/$export
    async fn handle_instance(
        &self,
        state: &AppState,
        resource_type: &str,
        id: &str,
        _params: &Value,
    ) -> Result<Value, OperationError> {
        match resource_type {
            "Group" => {
                let mut export_params = self.parse_params(_params)?;
                export_params.group_id = Some(id.to_string());

                let request_url = format!("{}/fhir/Group/{}/$export", state.base_url, id);

                self.submit_export(state, BulkExportLevel::Group, export_params, &request_url)
                    .await
            }
            "ViewDefinition" => {
                self.check_sql_on_fhir_enabled()?;

                // Load the ViewDefinition from storage
                let stored = state
                    .storage
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

                self.submit_viewdefinition_export(state, view_def, &request_url)
                    .await
            }
            _ => Err(OperationError::NotSupported(format!(
                "$export is only supported on Group or ViewDefinition instances, not {}",
                resource_type
            ))),
        }
    }
}

/// Execute a bulk export job
///
/// This function is called by the async job executor to perform the actual export.
pub async fn execute_bulk_export(
    state: AppState,
    job_id: Uuid,
    params: Value,
) -> Result<Value, String> {
    tracing::info!(job_id = %job_id, "Starting bulk export execution");

    // Parse job parameters
    let _level: BulkExportLevel = params
        .get("level")
        .and_then(|v| v.as_str())
        .and_then(|s| match s {
            "system" => Some(BulkExportLevel::System),
            "patient" => Some(BulkExportLevel::Patient),
            "group" => Some(BulkExportLevel::Group),
            _ => None,
        })
        .ok_or_else(|| "Missing or invalid export level".to_string())?;

    let export_params: BulkExportParams = params
        .get("params")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .ok_or_else(|| "Missing or invalid export params".to_string())?;

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
        .unwrap_or(1000) as usize;

    let request_url = params
        .get("request_url")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    // Create NDJSON writer
    let mut writer = NdjsonWriter::new(export_path, job_id, max_resources_per_file)
        .await
        .map_err(|e| format!("Failed to create NDJSON writer: {}", e))?;

    // Determine resource types to export
    let resource_types = if export_params.get_resource_types().is_empty() {
        // Get all resource types from storage if not specified
        get_all_resource_types(&state).await?
    } else {
        export_params.get_resource_types()
    };

    tracing::info!(
        job_id = %job_id,
        resource_types = ?resource_types,
        "Exporting resource types"
    );

    let mut total_exported = 0;

    // Export each resource type
    for resource_type in &resource_types {
        match export_resource_type(&state, &mut writer, resource_type, &export_params, batch_size)
            .await
        {
            Ok(count) => {
                total_exported += count;
                tracing::debug!(
                    job_id = %job_id,
                    resource_type = %resource_type,
                    count = count,
                    "Exported resource type"
                );
            }
            Err(e) => {
                tracing::warn!(
                    job_id = %job_id,
                    resource_type = %resource_type,
                    error = %e,
                    "Failed to export resource type"
                );
                // Continue with other types even if one fails
            }
        }

        // Update progress
        let progress =
            (resource_types.iter().position(|t| t == resource_type).unwrap_or(0) + 1) as f32
                / resource_types.len() as f32;

        if let Err(e) = state.async_job_manager.update_progress(job_id, progress).await {
            tracing::warn!(error = %e, "Failed to update job progress");
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
        total_exported = total_exported,
        output_files = output.len(),
        "Bulk export completed"
    );

    Ok(json!({
        "transactionTime": Utc::now().to_rfc3339(),
        "request": request_url,
        "requiresAccessToken": true,
        "output": output,
        "error": [],
    }))
}

/// Get all available resource types from storage
async fn get_all_resource_types(_state: &AppState) -> Result<Vec<String>, String> {
    // Common FHIR resource types for bulk export
    // In production, this would query the StructureDefinitions or capability statement
    Ok(vec![
        "Patient".to_string(),
        "Observation".to_string(),
        "Condition".to_string(),
        "Procedure".to_string(),
        "MedicationRequest".to_string(),
        "DiagnosticReport".to_string(),
        "Encounter".to_string(),
        "AllergyIntolerance".to_string(),
        "Immunization".to_string(),
        "CarePlan".to_string(),
        "CareTeam".to_string(),
        "Device".to_string(),
        "DocumentReference".to_string(),
        "Goal".to_string(),
        "Location".to_string(),
        "Medication".to_string(),
        "Organization".to_string(),
        "Practitioner".to_string(),
        "PractitionerRole".to_string(),
        "Provenance".to_string(),
    ])
}

/// Export a single resource type to NDJSON files
async fn export_resource_type(
    state: &AppState,
    writer: &mut NdjsonWriter,
    resource_type: &str,
    params: &BulkExportParams,
    batch_size: usize,
) -> Result<usize, String> {
    use octofhir_storage::SearchParams;

    let mut total = 0;
    let mut offset = 0;

    loop {
        // Build search parameters
        let mut search_params = SearchParams::default();
        search_params.count = Some(batch_size as u32);
        search_params.offset = Some(offset as u32);

        // Add _since filter if specified
        if let Some(since) = &params.since {
            search_params
                .parameters
                .entry("_lastUpdated".to_string())
                .or_default()
                .push(format!("ge{}", since.to_rfc3339()));
        }

        // Add type-specific filters from _typeFilter
        for (filter_type, query) in params.get_type_filters() {
            if filter_type == resource_type {
                // Parse query string and add to params
                for pair in query.split('&') {
                    if let Some((key, value)) = pair.split_once('=') {
                        search_params
                            .parameters
                            .entry(key.to_string())
                            .or_default()
                            .push(value.to_string());
                    }
                }
            }
        }

        // Execute search
        let result = state
            .storage
            .search(resource_type, &search_params)
            .await
            .map_err(|e| format!("Search failed for {}: {}", resource_type, e))?;

        let entries = result.entries;
        let count = entries.len();

        if count == 0 {
            break;
        }

        // Write resources to NDJSON
        for entry in &entries {
            writer
                .write_resource(resource_type, &entry.resource)
                .await
                .map_err(|e| format!("Failed to write resource: {}", e))?;
        }

        total += count;
        offset += count;

        // Check if we've retrieved all resources
        if count < batch_size {
            break;
        }
    }

    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_params_empty() {
        let config = BulkExportConfig::default();
        let op = ExportOperation::new(config);
        let params = json!({});
        let result = op.parse_params(&params).unwrap();
        assert!(result.output_format.is_none());
        assert!(result.since.is_none());
    }

    #[test]
    fn test_parse_params_with_values() {
        let config = BulkExportConfig::default();
        let op = ExportOperation::new(config);
        let params = json!({
            "_outputFormat": "application/fhir+ndjson",
            "_type": "Patient,Observation",
            "_since": "2024-01-01T00:00:00Z"
        });
        let result = op.parse_params(&params).unwrap();
        assert_eq!(
            result.output_format,
            Some("application/fhir+ndjson".to_string())
        );
        assert_eq!(result.resource_types, Some("Patient,Observation".to_string()));
        assert!(result.since.is_some());
    }

    #[test]
    fn test_parse_params_invalid_format() {
        let config = BulkExportConfig::default();
        let op = ExportOperation::new(config);
        let params = json!({
            "_outputFormat": "application/json"
        });
        let result = op.parse_params(&params);
        assert!(result.is_err());
    }
}
