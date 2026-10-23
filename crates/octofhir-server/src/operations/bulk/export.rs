//! Bulk export operation handler ($export)
//!
//! Implements the FHIR Bulk Data Access $export operation for system,
//! patient, group, and ViewDefinition level exports.
//!
//! - System: `/$export` - Export all resources
//! - Patient: `/Patient/$export` - Export patient compartment data
//! - Group: `/Group/{id}/$export` - Export group member data
//! - ViewDefinition: `/ViewDefinition/$export` - Export ViewDefinition results (SQL on FHIR)

use crate::operations::handler::OperationLevel;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::async_jobs::AsyncJobRequest;
use crate::config::BulkExportConfig;
use crate::operations::handler::{OperationError, OperationHandler};
use crate::server::AppState;
use octofhir_sof::ViewDefinition;

use super::NDJSON_CONTENT_TYPE;
use super::status::{BulkExportLevel, BulkExportParams};
use super::writer::NdjsonWriter;

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
        let mut values: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        if params["resourceType"] == "Parameters" {
            for parameter in params["parameter"].as_array().ok_or_else(|| {
                OperationError::InvalidParameters(
                    "Export requires a Parameters.parameter array".into(),
                )
            })? {
                let name = parameter["name"].as_str().ok_or_else(|| {
                    OperationError::InvalidParameters("Export parameter requires a name".into())
                })?;
                if !matches!(name, "_outputFormat" | "_since" | "_type" | "_typeFilter") {
                    return Err(OperationError::InvalidParameters(format!(
                        "Unsupported export parameter: {name}"
                    )));
                }
                let primitive: Vec<_> = parameter
                    .as_object()
                    .ok_or_else(|| {
                        OperationError::InvalidParameters("Invalid export parameter".into())
                    })?
                    .iter()
                    .filter(|(name, _)| name.starts_with("value"))
                    .collect();
                if primitive.len() != 1
                    || parameter.get("resource").is_some()
                    || parameter.get("part").is_some()
                {
                    return Err(OperationError::InvalidParameters(format!(
                        "Export {name} requires one primitive value"
                    )));
                }
                let value = primitive[0].1.as_str().ok_or_else(|| {
                    OperationError::InvalidParameters(format!(
                        "Export {name} requires a string value"
                    ))
                })?;
                values.entry(name.into()).or_default().push(value.into());
            }
        } else {
            for name in ["_outputFormat", "_since", "_type", "_typeFilter"] {
                if let Some(value) = params.get(name) {
                    values.insert(
                        name.into(),
                        vec![
                            value
                                .as_str()
                                .ok_or_else(|| {
                                    OperationError::InvalidParameters(format!(
                                        "Export {name} requires a string value"
                                    ))
                                })?
                                .to_owned(),
                        ],
                    );
                }
            }
        }
        for name in ["_outputFormat", "_since"] {
            if values.get(name).is_some_and(|values| values.len() != 1) {
                return Err(OperationError::InvalidParameters(format!(
                    "Export {name} permits only one value"
                )));
            }
        }
        let params = serde_json::to_value(
            values
                .into_iter()
                .map(|(name, values)| (name, values.join(",")))
                .collect::<std::collections::HashMap<_, _>>(),
        )
        .map_err(|error| OperationError::Internal(error.to_string()))?;
        let output_format = params
            .get("_outputFormat")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        // Validate output format if specified
        if let Some(ref fmt) = output_format
            && fmt != NDJSON_CONTENT_TYPE
            && fmt != "ndjson"
            && fmt != "application/ndjson"
        {
            return Err(OperationError::InvalidParameters(format!(
                "Unsupported _outputFormat: {}. Only {} is supported.",
                fmt, NDJSON_CONTENT_TYPE
            )));
        }

        let since = params
            .get("_since")
            .and_then(|v| v.as_str())
            .map(|s| {
                DateTime::parse_from_rfc3339(s)
                    .map(|dt| dt.with_timezone(&Utc))
                    .map_err(|_| {
                        OperationError::InvalidParameters(
                            "Export _since requires an instant".into(),
                        )
                    })
            })
            .transpose()?;

        let resource_types = params
            .get("_type")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let type_filter = params
            .get("_typeFilter")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let params = BulkExportParams {
            output_format,
            since,
            resource_types,
            type_filter,
            group_id: None,
        };
        params
            .get_type_filters()
            .map_err(OperationError::InvalidParameters)?;
        Ok(params)
    }

    /// Submit an export job and return the status URL
    async fn submit_export(
        &self,
        state: &AppState,
        context: &crate::operations::OperationContext,
        level: BulkExportLevel,
        mut params: BulkExportParams,
        _request_url: &str,
    ) -> Result<Value, OperationError> {
        let request_url = context.request_url();
        if !self.config.enabled {
            return Err(OperationError::NotSupported(
                "Bulk export is disabled on this server".to_string(),
            ));
        }

        let compartment = state
            .compartment_registry
            .get("Patient")
            .map_err(|error| OperationError::Internal(error.to_string()))?;
        if level == BulkExportLevel::Group {
            let group_id = params.group_id.as_deref().ok_or_else(|| {
                OperationError::InvalidParameters("Group export requires an instance".into())
            })?;
            crate::operations::group::resolve_membership(
                state,
                context,
                group_id,
                time::OffsetDateTime::now_utc(),
            )
            .await?;
        }
        let requested = params.get_resource_types();
        let explicit = !requested.is_empty();
        let candidates = if explicit {
            requested
        } else {
            get_all_resource_types(state)
                .await
                .map_err(OperationError::Internal)?
        };
        let mut permitted = Vec::new();
        for resource_type in candidates {
            if !state.resource_type_set.load().contains(&resource_type)
                || !octofhir_api::is_standard_fhir_resource(&resource_type)
            {
                return Err(OperationError::InvalidParameters(format!(
                    "Unsupported export resource type: {resource_type}"
                )));
            }
            if level != BulkExportLevel::System
                && resource_type != "Patient"
                && compartment
                    .get_inclusion_params(&resource_type)
                    .is_none_or(|params| params.is_empty())
            {
                if explicit {
                    return Err(OperationError::InvalidParameters(format!(
                        "{resource_type} is outside the supported Patient export compartment"
                    )));
                }
                continue;
            }
            match context.policy_context(
                state,
                octofhir_auth::smart::scopes::FhirOperation::Search,
                &resource_type,
                None,
                &octofhir_storage::SearchParams::new(),
            ) {
                Ok(_) => permitted.push(resource_type),
                Err(error) if error.is_forbidden() && !explicit => {}
                Err(error) => return Err(error),
            }
        }
        if permitted.is_empty() {
            return Err(octofhir_api::ApiError::forbidden(
                "No search grant permits an export resource type",
            )
            .into());
        }
        let registry = state.search_config.config().registry.snapshot();
        for (resource_type, query) in params
            .get_type_filters()
            .map_err(OperationError::InvalidParameters)?
        {
            if !permitted.contains(&resource_type) {
                return Err(OperationError::InvalidParameters(format!(
                    "_typeFilter resource {resource_type} is not selected for export"
                )));
            }
            for (name, _) in url::form_urlencoded::parse(query.as_bytes()) {
                if matches!(
                    name.as_ref(),
                    "_sort"
                        | "_count"
                        | "_offset"
                        | "_total"
                        | "_summary"
                        | "_elements"
                        | "_format"
                ) || name.starts_with("_include")
                    || name.starts_with("_revinclude")
                {
                    return Err(OperationError::InvalidParameters(
                        "_typeFilter cannot contain search response parameters".into(),
                    ));
                }
                let root = name.split([':', '.']).next().unwrap_or_default();
                if !name.starts_with("_has:") && registry.get(&resource_type, root).is_none() {
                    return Err(OperationError::InvalidParameters(format!(
                        "Unsupported _typeFilter parameter {resource_type}.{name}"
                    )));
                }
            }
        }
        permitted.sort();
        permitted.dedup();
        params.resource_types = Some(permitted.join(","));

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
            client_id: context.auth().map(|auth| auth.client_id().to_owned()),
            authorization: context.job_authorization(state)?,
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
            "status_url": format!("{}/_async-status/{}", state.fhir_base_url(), job_id),
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

    /// Submit a ViewDefinition export job
    async fn submit_viewdefinition_export(
        &self,
        state: &AppState,
        context: &crate::operations::OperationContext,
        view_def: ViewDefinition,
        _request_url: &str,
    ) -> Result<Value, OperationError> {
        super::super::sof::ViewDefinitionExportOperation::new(
            self.sql_on_fhir_enabled,
            self.config.clone(),
        )
        .submit_export(state, context, view_def, _request_url)
        .await
    }
}

#[async_trait]
impl OperationHandler for ExportOperation {
    fn supports_level(&self, level: OperationLevel, resource_type: Option<&str>) -> bool {
        match (level, resource_type) {
            (OperationLevel::System, _)
            | (OperationLevel::Type, Some("Patient"))
            | (OperationLevel::Instance, Some("Group")) => self.config.enabled,
            (OperationLevel::Type | OperationLevel::Instance, Some("ViewDefinition")) => {
                self.config.enabled && self.sql_on_fhir_enabled
            }
            _ => false,
        }
    }

    fn code(&self) -> &str {
        "export"
    }

    /// System-level export: GET /$export
    async fn handle_system(
        &self,
        state: &AppState,
        context: &crate::operations::OperationContext,
        params: &Value,
    ) -> Result<Value, OperationError> {
        let export_params = self.parse_params(params)?;
        let request_url = format!("{}/fhir/$export", state.base_url);

        self.submit_export(
            state,
            context,
            BulkExportLevel::System,
            export_params,
            &request_url,
        )
        .await
    }

    /// Type-level export: GET /Patient/$export or POST /ViewDefinition/$export
    async fn handle_type(
        &self,
        state: &AppState,
        context: &crate::operations::OperationContext,
        resource_type: &str,
        params: &Value,
    ) -> Result<Value, OperationError> {
        match resource_type {
            "Patient" => {
                let export_params = self.parse_params(params)?;
                let request_url = format!("{}/fhir/Patient/$export", state.base_url);

                self.submit_export(
                    state,
                    context,
                    BulkExportLevel::Patient,
                    export_params,
                    &request_url,
                )
                .await
            }
            "ViewDefinition" => {
                self.check_sql_on_fhir_enabled()?;
                let view_def = self.extract_view_definition(params)?;
                let request_url = format!("{}/fhir/ViewDefinition/$export", state.base_url);
                self.submit_viewdefinition_export(state, context, view_def, &request_url)
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
        context: &crate::operations::OperationContext,
        resource_type: &str,
        id: &str,
        _params: &Value,
    ) -> Result<Value, OperationError> {
        match resource_type {
            "Group" => {
                let mut export_params = self.parse_params(_params)?;
                export_params.group_id = Some(id.to_string());

                let request_url = format!("{}/fhir/Group/{}/$export", state.base_url, id);

                self.submit_export(
                    state,
                    context,
                    BulkExportLevel::Group,
                    export_params,
                    &request_url,
                )
                .await
            }
            "ViewDefinition" => {
                self.check_sql_on_fhir_enabled()?;

                // Load the ViewDefinition from storage
                let stored = context
                    .read(state, "ViewDefinition", id)
                    .await?
                    .ok_or_else(|| {
                        OperationError::NotFound(format!("ViewDefinition/{} not found", id))
                    })?;

                let view_def = ViewDefinition::from_json(&stored.resource).map_err(|e| {
                    OperationError::Internal(format!("Invalid stored ViewDefinition: {}", e))
                })?;

                let request_url = format!("{}/fhir/ViewDefinition/{}/$export", state.base_url, id);

                self.submit_viewdefinition_export(state, context, view_def, &request_url)
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
    let context = crate::operations::OperationContext::for_job(&state, job_id)
        .await
        .map_err(|error| error.to_string())?;

    // Parse job parameters
    let level: BulkExportLevel = params
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

    let started_at = Utc::now();
    let group_members = if level == BulkExportLevel::Group {
        let group_id = export_params
            .group_id
            .as_deref()
            .ok_or("Missing Group instance")?;
        Some(
            crate::operations::group::resolve_membership(
                &state,
                &context,
                group_id,
                time::OffsetDateTime::now_utc(),
            )
            .await
            .map_err(|error| error.to_string())?
            .patient_ids,
        )
    } else {
        None
    };

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

    if batch_size == 0 || batch_size > u32::MAX as usize {
        return Err("Export batch size must fit a positive search page".into());
    }

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
    for (index, resource_type) in resource_types.iter().enumerate() {
        let constraint =
            export_membership_constraint(&state, level, group_members.as_deref(), resource_type)?;
        let search_params = export_search_params(
            resource_type,
            &export_params,
            batch_size as u32,
            constraint,
            started_at,
        )?;
        let count =
            export_resource_type(&state, &context, &mut writer, resource_type, &search_params)
                .await?;
        total_exported += count;

        // A failed branch fails the job; an incomplete export must never be
        // presented as a successful manifest with an empty error list.
        let progress = (index + 1) as f32 / resource_types.len() as f32;

        if let Err(e) = state
            .async_job_manager
            .update_progress(job_id, progress)
            .await
        {
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

    output.sort_by(|left, right| {
        (left["type"].as_str(), left["url"].as_str())
            .cmp(&(right["type"].as_str(), right["url"].as_str()))
    });
    tracing::info!(
        job_id = %job_id,
        total_exported = total_exported,
        output_files = output.len(),
        "Bulk export completed"
    );

    Ok(json!({
        "transactionTime": started_at.to_rfc3339(),
        "request": request_url,
        "requiresAccessToken": context.auth().is_some(),
        "output": output,
        "error": [],
    }))
}

/// Get all available resource types from storage
async fn get_all_resource_types(state: &AppState) -> Result<Vec<String>, String> {
    let mut types: Vec<_> = state
        .resource_type_set
        .load()
        .iter()
        .filter(|resource_type| octofhir_api::is_standard_fhir_resource(resource_type))
        .cloned()
        .collect();
    types.sort();
    Ok(types)
}

fn export_membership_constraint(
    state: &AppState,
    level: BulkExportLevel,
    patient_ids: Option<&[String]>,
    resource_type: &str,
) -> Result<Option<octofhir_storage::SearchConstraint>, String> {
    use octofhir_storage::SearchConstraint;
    if level == BulkExportLevel::System {
        return Ok(None);
    }
    let definition = state
        .compartment_registry
        .get("Patient")
        .map_err(|error| error.to_string())?;
    let parameters = definition
        .get_inclusion_params(resource_type)
        .unwrap_or_default()
        .to_vec();
    if resource_type != "Patient" && parameters.is_empty() {
        return Err(format!(
            "{resource_type} is outside the supported Patient export compartment"
        ));
    }
    Ok(Some(match level {
        BulkExportLevel::Patient => SearchConstraint::AllPatients {
            fhir_base_url: state.fhir_base_url(),
            parameters,
        },
        BulkExportLevel::Group => SearchConstraint::LinkedPatients {
            patient_ids: patient_ids
                .ok_or("Missing resolved Group membership")?
                .to_vec(),
            fhir_base_url: state.fhir_base_url(),
            parameters,
        },
        BulkExportLevel::System => unreachable!(),
    }))
}

/// Native disjunctions preserve repeated type filters while grants and
/// compartment membership remain independent conjunctions.
fn export_search_params(
    resource_type: &str,
    params: &BulkExportParams,
    count: u32,
    membership: Option<octofhir_storage::SearchConstraint>,
    started_at: DateTime<Utc>,
) -> Result<octofhir_storage::SearchParams, String> {
    use octofhir_storage::{SearchConstraint, SearchParams};
    let mut search = SearchParams::new()
        .with_count(count)
        .with_sort("_id", false);
    let mut constraints = membership.into_iter().collect::<Vec<_>>();
    let mut filters = Vec::new();
    for (filter_type, query) in params.get_type_filters()? {
        if filter_type == resource_type {
            let mut parameters: std::collections::HashMap<String, Vec<String>> =
                std::collections::HashMap::new();
            for (key, value) in url::form_urlencoded::parse(query.as_bytes()) {
                parameters
                    .entry(key.into_owned())
                    .or_default()
                    .push(value.into_owned());
            }
            filters.push(SearchConstraint::Query(parameters));
        }
    }
    if !filters.is_empty() {
        constraints.push(SearchConstraint::Or(filters));
    }
    search.constraint = Some(SearchConstraint::And(constraints));
    if let Some(since) = &params.since {
        search = search.with_param("_lastUpdated", format!("gt{}", since.to_rfc3339()));
    }
    Ok(search.with_param("_lastUpdated", format!("le{}", started_at.to_rfc3339())))
}

/// Export a single resource type to NDJSON files
async fn export_resource_type(
    state: &AppState,
    context: &crate::operations::OperationContext,
    writer: &mut NdjsonWriter,
    resource_type: &str,
    params: &octofhir_storage::SearchParams,
) -> Result<usize, String> {
    let mut total = 0;
    let mut offset = 0;
    loop {
        let mut search_params = params.clone();
        search_params.offset =
            Some(u32::try_from(offset).map_err(|_| "Export search offset overflow")?);
        // Execute search
        search_params = search_params.with_sort("_id", false);
        let result = context
            .search(state, resource_type, &search_params)
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
        if !result.has_more {
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
        assert_eq!(
            result.resource_types,
            Some("Patient,Observation".to_string())
        );
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

    #[test]
    fn export_parameters_preserve_repeats_and_reject_invalid_since() {
        let op = ExportOperation::new(BulkExportConfig::default());
        let params = json!({"resourceType":"Parameters","parameter":[
            {"name":"_type","valueString":"Patient"},
            {"name":"_type","valueString":"Observation"},
            {"name":"_outputFormat","valueCode":"application/ndjson"},
            {"name":"_since","valueInstant":"2025-02-28T12:00:00Z"}
        ]});
        let parsed = op.parse_params(&params).unwrap();
        assert_eq!(parsed.get_resource_types(), vec!["Patient", "Observation"]);
        assert!(parsed.since.is_some());
        for parameter in [
            json!({"name":"_since","valueString":"invalid"}),
            json!({"name":"_since","valueString":"2025-02-28"}),
            json!({"name":"_type","valueBoolean":true}),
            json!({"name":"_type","valueString":"Patient","valueCode":"Observation"}),
            json!({"name":"unknown","valueString":"Patient"}),
        ] {
            assert!(
                op.parse_params(&json!({"resourceType":"Parameters","parameter":[parameter]}))
                    .is_err()
            );
        }
    }
}
