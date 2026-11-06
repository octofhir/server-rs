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

use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap, HashSet};

use async_trait::async_trait;
use axum::http::StatusCode;
use chrono::{DateTime, Utc};
use octofhir_api::{ApiError, OperationOutcomeIssue};
use octofhir_auth::smart::scopes::FhirOperation;
use octofhir_core::fhir_reference::parse_reference;
use octofhir_storage::{
    HistoryMethod, HistoryParams, RawSearchResult, RawStoredResource, SearchConstraint,
    SearchParams,
};
use serde_json::{Value, json};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::async_jobs::AsyncJobRequest;
use crate::config::{BulkExportConfig, BulkSpecVersion};
use crate::operations::OperationContext;
use crate::operations::handler::{OperationError, OperationHandler};
use crate::server::AppState;
use octofhir_sof::ViewDefinition;

use super::NDJSON_CONTENT_TYPE;
use super::manifest::{ManifestFile, ManifestHeader, ManifestPages};
use super::status::{AssociatedData, BulkExportLevel, BulkExportParams};
use super::writer::{ClosedFile, NdjsonWriter};

/// Parameters this server accepts on a kick-off request.
const EXPORT_PARAMETERS: [&str; 10] = [
    "_outputFormat",
    "_since",
    "_until",
    "_type",
    "_elements",
    "_typeFilter",
    "patient",
    "includeAssociatedData",
    "organizeOutputBy",
    "allowPartialManifests",
];

/// Parameters with cardinality 0..1.
const SINGLE_VALUED: [&str; 5] = [
    "_outputFormat",
    "_since",
    "_until",
    "organizeOutputBy",
    "allowPartialManifests",
];

/// Parameters STU 3 lets a client comma-delimit. A comma inside `_typeFilter` is
/// part of the search value, so it is not one of them.
const COMMA_DELIMITED: [&str; 4] = ["_type", "_elements", "patient", "includeAssociatedData"];

/// File group names beside resource types (which always start with a capital).
const OUTCOME_FILE: &str = "outcome";
const DELETED_FILE: &str = "deleted";
const PATIENT_BLOCKS_FILE: &str = "Patient-blocks";

/// Patients whose resource blocks are assembled together. One search per resource
/// type serves a whole chunk, and a chunk's resources are what the job holds in memory.
const BLOCK_PATIENTS: usize = 100;

/// Target references per Provenance search.
const PROVENANCE_TARGETS: usize = 200;

/// The value of one Parameters entry as text, checked against the type the IG gives it.
fn parameter_text(
    name: &str,
    parameter: &Value,
    spec: BulkSpecVersion,
) -> Result<String, OperationError> {
    let invalid =
        || OperationError::InvalidParameters(format!("Export {name} requires a value of its type"));
    let values: Vec<_> = parameter
        .as_object()
        .ok_or_else(|| OperationError::InvalidParameters("Invalid export parameter".into()))?
        .iter()
        .filter(|(key, _)| key.starts_with("value"))
        .collect();
    if values.len() != 1 || parameter.get("resource").is_some() || parameter.get("part").is_some() {
        return Err(OperationError::InvalidParameters(format!(
            "Export {name} requires one primitive value"
        )));
    }
    let (key, value) = values[0];
    if spec == BulkSpecVersion::Stu4 && name == "includeAssociatedData" && key != "valueCoding" {
        return Err(OperationError::InvalidParameters(
            "includeAssociatedData requires a Coding".into(),
        ));
    }
    match (name, value) {
        ("patient", Value::Object(reference)) => reference
            .get("reference")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(invalid),
        ("includeAssociatedData", Value::Object(coding)) => coding
            .get("code")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(invalid),
        ("allowPartialManifests", Value::Bool(flag)) => Ok(flag.to_string()),
        (_, Value::String(text)) => Ok(text.clone()),
        _ => Err(invalid()),
    }
}

/// `true` for `element` and `Type.element`; the IG permits root elements only.
fn valid_element(element: &str) -> bool {
    let name = match element.split_once('.') {
        Some((resource_type, name)) => {
            if !resource_type.starts_with(|c: char| c.is_ascii_uppercase()) {
                return false;
            }
            name
        }
        None => element,
    };
    !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn parse_instant(name: &str, value: &str) -> Result<DateTime<Utc>, OperationError> {
    DateTime::parse_from_rfc3339(value)
        .map(|instant| instant.with_timezone(&Utc))
        .map_err(|_| {
            OperationError::InvalidParameters(format!("Export {name} requires an instant"))
        })
}

fn bad_input(message: String) -> OperationError {
    OperationError::InvalidParameters(message)
}

fn to_offset(instant: DateTime<Utc>) -> Result<OffsetDateTime, String> {
    instant
        .timestamp_nanos_opt()
        .and_then(|nanos| OffsetDateTime::from_unix_timestamp_nanos(i128::from(nanos)).ok())
        .ok_or_else(|| "Export time is out of range".to_owned())
}

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

    /// Parse export parameters from the operation params. Under
    /// `Prefer: handling=lenient` unsupported input is dropped and remembered for
    /// the manifest; otherwise it is refused, as the IG recommends.
    fn parse_params(
        &self,
        params: &Value,
        lenient: bool,
    ) -> Result<BulkExportParams, OperationError> {
        let spec = self.config.spec_version;
        let mut ignored = Vec::new();
        let mut values: BTreeMap<String, Vec<String>> = BTreeMap::new();
        if params["resourceType"] == "Parameters" {
            for parameter in params["parameter"].as_array().ok_or_else(|| {
                OperationError::InvalidParameters(
                    "Export requires a Parameters.parameter array".into(),
                )
            })? {
                let name = parameter["name"].as_str().ok_or_else(|| {
                    OperationError::InvalidParameters("Export parameter requires a name".into())
                })?;
                if !EXPORT_PARAMETERS.contains(&name) {
                    if lenient {
                        ignored.push(format!("Ignored unsupported parameter {name}"));
                        continue;
                    }
                    return Err(bad_input(format!("Unsupported export parameter: {name}")));
                }
                let value = parameter_text(name, parameter, spec)?;
                values.entry(name.into()).or_default().push(value);
            }
        } else {
            for name in EXPORT_PARAMETERS {
                if let Some(value) = params.get(name) {
                    let text = value.as_str().ok_or_else(|| {
                        OperationError::InvalidParameters(format!(
                            "Export {name} requires a string value"
                        ))
                    })?;
                    values.insert(name.into(), vec![text.to_owned()]);
                }
            }
        }
        for name in SINGLE_VALUED {
            if values.get(name).is_some_and(|values| values.len() != 1) {
                return Err(bad_input(format!("Export {name} permits only one value")));
            }
        }
        for name in COMMA_DELIMITED {
            let Some(entries) = values.get_mut(name) else {
                continue;
            };
            if spec == BulkSpecVersion::Stu4 && entries.iter().any(|value| value.contains(',')) {
                return Err(bad_input(format!(
                    "Export {name} repeats the parameter instead of comma-delimiting its values"
                )));
            }
            *entries = entries
                .iter()
                .flat_map(|value| value.split(','))
                .map(|value| value.trim().to_owned())
                .collect();
        }
        let single = |name: &str| values.get(name).and_then(|values| values.first());

        // Validate output format if specified
        let output_format = single("_outputFormat").cloned();
        if let Some(format) = &output_format
            && !matches!(
                format.as_str(),
                NDJSON_CONTENT_TYPE | "ndjson" | "application/ndjson"
            )
        {
            return Err(bad_input(format!(
                "Unsupported _outputFormat: {format}. Only {NDJSON_CONTENT_TYPE} is supported."
            )));
        }

        let since = single("_since")
            .map(|value| parse_instant("_since", value))
            .transpose()?;
        let until = single("_until")
            .map(|value| parse_instant("_until", value))
            .transpose()?;
        if let (Some(since), Some(until)) = (since, until)
            && until <= since
        {
            return Err(bad_input("Export _until must be later than _since".into()));
        }

        let mut elements = Vec::new();
        for element in values.get("_elements").into_iter().flatten() {
            if valid_element(element) {
                elements.push(element.clone());
            } else if lenient {
                ignored.push(format!("Ignored _elements value {element}"));
            } else {
                return Err(bad_input(format!(
                    "_elements permits only root elements, not {element}"
                )));
            }
        }

        let mut associated_data = None;
        for code in values.get("includeAssociatedData").into_iter().flatten() {
            match code.as_str() {
                // The least restrictive value wins when several are given.
                "RelevantProvenanceResources" => {
                    associated_data = Some(AssociatedData::RelevantProvenanceResources);
                }
                "LatestProvenanceResources" => {
                    associated_data.get_or_insert(AssociatedData::LatestProvenanceResources);
                }
                _ if lenient => {
                    ignored.push(format!("Ignored includeAssociatedData value {code}"));
                }
                _ => {
                    return Err(bad_input(format!(
                        "Unsupported includeAssociatedData value: {code}"
                    )));
                }
            }
        }

        let organize_output_by = match single("organizeOutputBy").map(String::as_str) {
            None => None,
            Some("Patient") => Some("Patient".to_owned()),
            Some(other) if lenient => {
                ignored.push(format!("Ignored organizeOutputBy {other}"));
                None
            }
            Some(other) => {
                return Err(bad_input(format!(
                    "organizeOutputBy supports Patient only, not {other}"
                )));
            }
        };

        let allow_partial_manifests = match single("allowPartialManifests").map(String::as_str) {
            None | Some("false") => false,
            Some("true") => true,
            Some(_) => {
                return Err(bad_input("allowPartialManifests must be boolean".into()));
            }
        };

        let joined = |name: &str| values.get(name).map(|values| values.join(","));
        let params = BulkExportParams {
            output_format,
            since,
            until,
            resource_types: joined("_type"),
            type_filter: joined("_typeFilter"),
            group_id: None,
            elements,
            patients: values.get("patient").cloned().unwrap_or_default(),
            associated_data,
            organize_output_by,
            allow_partial_manifests,
            ignored,
        };
        params
            .get_type_filters()
            .map_err(OperationError::InvalidParameters)?;
        Ok(params)
    }

    /// Resolve `patient` to Patient ids this export can serve. The IG asks for an
    /// error when one is absent or, for a Group, not a member; lenient requests
    /// continue with the rest.
    async fn resolve_patients(
        &self,
        state: &AppState,
        context: &OperationContext,
        level: BulkExportLevel,
        params: &mut BulkExportParams,
        members: Option<&[String]>,
    ) -> Result<(), OperationError> {
        if params.patients.is_empty() {
            return Ok(());
        }
        if level == BulkExportLevel::System {
            if context.lenient() {
                params.patients.clear();
                params
                    .ignored
                    .push("Ignored patient: it does not apply to a system level export".into());
                return Ok(());
            }
            return Err(bad_input(
                "The patient parameter does not apply to a system level export".into(),
            ));
        }
        let mut ids = Vec::with_capacity(params.patients.len());
        for reference in &params.patients {
            let parsed = parse_reference(reference, Some(&state.fhir_base_url()))
                .ok()
                .filter(|parsed| parsed.resource_type == "Patient")
                .ok_or_else(|| {
                    bad_input(format!(
                        "patient requires a Patient reference, not {reference}"
                    ))
                })?;
            ids.push(parsed.id);
        }
        ids.sort();
        ids.dedup();
        let present: HashSet<String> = context
            .read_many(state, "Patient", &ids)
            .await?
            .into_iter()
            .map(|patient| patient.id)
            .collect();
        let mut known = Vec::with_capacity(ids.len());
        for id in ids {
            let problem = if !present.contains(&id) {
                Some("was not found")
            } else if members.is_some_and(|members| !members.contains(&id)) {
                Some("is not a member of the Group")
            } else {
                None
            };
            match problem {
                None => known.push(id),
                Some(problem) if context.lenient() => {
                    params
                        .ignored
                        .push(format!("Ignored Patient/{id}: it {problem}"));
                }
                Some(problem) => {
                    return Err(bad_input(format!("Patient/{id} {problem}")));
                }
            }
        }
        if known.is_empty() {
            return Err(bad_input("No requested patient can be exported".into()));
        }
        params.patients = known;
        Ok(())
    }

    /// Submit an export job and return the status URL
    async fn submit_export(
        &self,
        state: &AppState,
        context: &OperationContext,
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
        if self.config.spec_version == BulkSpecVersion::Stu4 && context.is_get() {
            return Err(ApiError::issues(
                StatusCode::METHOD_NOT_ALLOWED,
                vec![OperationOutcomeIssue::error(
                    "not-supported",
                    "Kick-off requires POST with a Parameters resource",
                    None,
                )],
            )
            .into());
        }

        let lenient = context.lenient();
        let organized = params.organize_output_by.is_some();
        let compartment = state
            .compartment_registry
            .get("Patient")
            .map_err(|error| OperationError::Internal(error.to_string()))?;
        let members = if level == BulkExportLevel::Group {
            let group_id = params.group_id.as_deref().ok_or_else(|| {
                OperationError::InvalidParameters("Group export requires an instance".into())
            })?;
            Some(
                crate::operations::group::resolve_membership(
                    state,
                    context,
                    group_id,
                    OffsetDateTime::now_utc(),
                )
                .await?
                .patient_ids,
            )
        } else {
            None
        };
        self.resolve_patients(state, context, level, &mut params, members.as_deref())
            .await?;
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
                if lenient {
                    params
                        .ignored
                        .push(format!("Ignored unsupported resource type {resource_type}"));
                    continue;
                }
                return Err(OperationError::InvalidParameters(format!(
                    "Unsupported export resource type: {resource_type}"
                )));
            }
            if (level != BulkExportLevel::System || organized)
                && resource_type != "Patient"
                && compartment
                    .get_inclusion_params(&resource_type)
                    .is_none_or(|params| params.is_empty())
            {
                if explicit && (lenient || organized) {
                    // Organized output may omit what cannot join a resource block.
                    params.ignored.push(format!(
                        "Omitted {resource_type}: it is outside the Patient compartment"
                    ));
                    continue;
                }
                if explicit {
                    return Err(OperationError::InvalidParameters(format!(
                        "{resource_type} is outside the supported Patient export compartment"
                    )));
                }
                continue;
            }
            match context.policy_context(
                state,
                FhirOperation::Search,
                &resource_type,
                None,
                &SearchParams::new(),
            ) {
                Ok(_) => permitted.push(resource_type),
                Err(error) if error.is_forbidden() && !explicit => {}
                Err(error) => return Err(error),
            }
        }
        if permitted.is_empty() {
            return Err(
                ApiError::forbidden("No search grant permits an export resource type").into(),
            );
        }
        // Blocks are headed by Patients and Provenance is searched separately, so
        // both need a grant of their own even when `_type` leaves them out.
        if organized {
            context.policy_context(
                state,
                FhirOperation::Search,
                "Patient",
                None,
                &SearchParams::new(),
            )?;
        }
        if params.associated_data.is_some()
            && let Err(error) = context.policy_context(
                state,
                FhirOperation::Search,
                "Provenance",
                None,
                &SearchParams::new(),
            )
        {
            if !(lenient && error.is_forbidden()) {
                return Err(error);
            }
            params.associated_data = None;
            params
                .ignored
                .push("Ignored includeAssociatedData: Provenance is not readable".into());
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
                "max_files_per_manifest": self.config.max_files_per_manifest,
                "batch_size": self.config.batch_size,
                "retention_hours": self.config.retention_hours,
            },
            "spec": self.config.spec_version,
            "separate_status": context.separate_status(),
            "request_url": request_url,
        });

        // Submit to async job manager
        let async_request = AsyncJobRequest {
            request_type: "bulk_export".to_string(),
            method: if context.is_get() { "GET" } else { "POST" }.to_string(),
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
            "separate_status": context.separate_status(),
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
        let export_params = self.parse_params(params, context.lenient())?;
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
                let export_params = self.parse_params(params, context.lenient())?;
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
                let mut export_params = self.parse_params(_params, context.lenient())?;
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

/// Where finished output files go: one manifest at the end, or pages as they close.
enum ManifestSink {
    Whole(Vec<ManifestFile>),
    Paged(ManifestPages),
}

/// One running export: what it selects, and the files written so far.
struct ExportRun<'a> {
    state: &'a AppState,
    context: &'a OperationContext,
    job_id: Uuid,
    params: &'a BulkExportParams,
    level: BulkExportLevel,
    batch_size: u32,
    started_at: DateTime<Utc>,
    writer: NdjsonWriter,
    sink: ManifestSink,
    deleted: Vec<ManifestFile>,
    outcome: Vec<ManifestFile>,
    /// Exported with the resources they describe rather than as a type of their own.
    provenance: Option<AssociatedData>,
    written_provenance: HashSet<String>,
    /// Root elements `_elements` must keep per type: the schema's mandatory ones.
    mandatory: HashMap<String, HashSet<String>>,
    /// Resources selected through a linked Patient outside their block.
    unplaced: usize,
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
    let context = OperationContext::for_job(&state, job_id)
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
    let spec: BulkSpecVersion = serde_json::from_value(params["spec"].clone()).unwrap_or_default();

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
                OffsetDateTime::now_utc(),
            )
            .await
            .map_err(|error| error.to_string())?
            .patient_ids,
        )
    } else {
        None
    };
    // `patient` narrows the selection; it was checked against the Group at kick-off.
    let selected = if export_params.patients.is_empty() {
        group_members
    } else {
        Some(export_params.patients.clone())
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
    let header = ManifestHeader {
        spec,
        transaction_time: started_at,
        request: request_url.to_owned(),
        requires_access_token: context.auth().is_some(),
        organized_by: export_params.organize_output_by.clone(),
    };
    let sink = if export_params.allow_partial_manifests {
        let per_page = config["max_files_per_manifest"].as_u64().unwrap_or(100) as usize;
        let status_url = format!("{}/_async-status/{job_id}", state.fhir_base_url());
        ManifestSink::Paged(
            ManifestPages::new(export_path, job_id, header.clone(), per_page, &status_url).await?,
        )
    } else {
        ManifestSink::Whole(Vec::new())
    };

    // Create NDJSON writer
    let writer = NdjsonWriter::new(export_path, job_id, max_resources_per_file)
        .await
        .map_err(|e| format!("Failed to create NDJSON writer: {}", e))?;

    // Determine resource types to export
    let mut resource_types = if export_params.get_resource_types().is_empty() {
        // Get all resource types from storage if not specified
        get_all_resource_types(&state).await?
    } else {
        export_params.get_resource_types()
    };

    // A Patient-level export owes the Provenance of every resource in the compartment,
    // not only the ones targeting a Patient, so it follows the exported resources.
    let provenance = export_params.associated_data.or_else(|| {
        (level != BulkExportLevel::System
            && resource_types.len() > 1
            && resource_types.iter().any(|name| name == "Provenance"))
        .then_some(AssociatedData::RelevantProvenanceResources)
    });
    if provenance.is_some() {
        resource_types.retain(|name| name != "Provenance");
    }

    tracing::info!(
        job_id = %job_id,
        resource_types = ?resource_types,
        "Exporting resource types"
    );

    let mut run = ExportRun {
        state: &state,
        context: &context,
        job_id,
        params: &export_params,
        level,
        batch_size: batch_size as u32,
        started_at,
        writer,
        sink,
        deleted: Vec::new(),
        outcome: Vec::new(),
        provenance,
        written_provenance: HashSet::new(),
        mandatory: HashMap::new(),
        unplaced: 0,
    };
    if export_params.organize_output_by.is_some() {
        run.export_blocks(&resource_types, selected).await?;
    } else {
        run.export_types(&resource_types, selected.as_deref())
            .await?;
    }
    run.write_deleted(&resource_types).await?;
    run.write_messages().await?;
    let result = run.finish(&header).await?;
    tracing::info!(job_id = %job_id, "Bulk export completed");
    Ok(result)
}

impl ExportRun<'_> {
    fn file_url(&self, path: &std::path::Path) -> String {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("unknown");
        format!(
            "{}/_bulk-files/{}/{name}",
            self.state.fhir_base_url(),
            self.job_id
        )
    }

    /// File finished output, publishing it right away when pages are in use.
    async fn collect_closed(&mut self) -> Result<(), String> {
        for ClosedFile {
            key,
            path,
            count,
            bytes,
        } in self.writer.take_closed()
        {
            let mut file = ManifestFile {
                resource_type: None,
                url: self.file_url(&path),
                count,
                bytes,
                severities: Vec::new(),
            };
            match key.as_str() {
                DELETED_FILE => self.deleted.push(file),
                OUTCOME_FILE => {
                    file.resource_type = Some("OperationOutcome".into());
                    file.severities = self.message_severities();
                    self.outcome.push(file);
                }
                PATIENT_BLOCKS_FILE => self.add_output(file).await?,
                _ => {
                    file.resource_type = Some(key);
                    self.add_output(file).await?;
                }
            }
        }
        Ok(())
    }

    async fn add_output(&mut self, file: ManifestFile) -> Result<(), String> {
        match &mut self.sink {
            ManifestSink::Whole(files) => {
                files.push(file);
                Ok(())
            }
            ManifestSink::Paged(pages) => pages.add(file).await,
        }
    }

    async fn export_types(
        &mut self,
        resource_types: &[String],
        selected: Option<&[String]>,
    ) -> Result<(), String> {
        for (index, resource_type) in resource_types.iter().enumerate() {
            let constraint =
                export_membership_constraint(self.state, self.level, selected, resource_type)?;
            let search = export_search_params(
                resource_type,
                self.params,
                self.batch_size,
                constraint,
                self.started_at,
            )?;
            self.export_type(resource_type, &search).await?;
            self.writer
                .close(resource_type)
                .await
                .map_err(|error| format!("Failed to write resource: {error}"))?;
            self.collect_closed().await?;

            // A failed branch fails the job; an incomplete export must never be
            // presented as a successful manifest with an empty error list.
            let progress = (index + 1) as f32 / resource_types.len() as f32;
            if let Err(e) = self
                .state
                .async_job_manager
                .update_progress(self.job_id, progress)
                .await
            {
                tracing::warn!(error = %e, "Failed to update job progress");
            }
        }
        if self.provenance.is_some() {
            self.writer
                .close("Provenance")
                .await
                .map_err(|error| format!("Failed to write resource: {error}"))?;
            self.collect_closed().await?;
        }
        Ok(())
    }

    /// One page of a search read in `_id` order, after the given id. Seeking keeps
    /// each page's cost flat and, unlike an offset, skips nothing when a resource
    /// is updated out of the result while the export runs.
    async fn page(
        &self,
        resource_type: &str,
        params: &SearchParams,
        after: Option<&str>,
    ) -> Result<RawSearchResult, String> {
        let mut search = params.clone();
        if search.sort.is_none() {
            search = search.with_sort("_id", false);
        }
        if let Some(after) = after {
            let seek = SearchConstraint::IdAfter(after.to_owned());
            search.constraint = Some(match search.constraint.take() {
                Some(existing) => SearchConstraint::And(vec![existing, seek]),
                None => seek,
            });
        }
        self.context
            .search_raw(self.state, resource_type, &search)
            .await
            .map_err(|e| format!("Search failed for {resource_type}: {e}"))
    }

    /// Export a single resource type to NDJSON files
    async fn export_type(
        &mut self,
        resource_type: &str,
        params: &SearchParams,
    ) -> Result<(), String> {
        let mut after: Option<String> = None;
        loop {
            let result = self.page(resource_type, params, after.as_deref()).await?;
            let Some(last) = result.entries.last() else {
                break;
            };
            after = Some(last.id.clone());
            let mut targets = Vec::new();
            for entry in &result.entries {
                if self.params.elements.is_empty() {
                    self.writer
                        .write_raw(resource_type, &entry.resource_json)
                        .await
                } else {
                    let resource = parse_stored(entry)?;
                    let line = self.project(resource_type, &resource).await?;
                    self.writer.write_resource(resource_type, &line).await
                }
                .map_err(|e| format!("Failed to write resource: {e}"))?;
                if self.provenance.is_some() {
                    targets.push(format!("{resource_type}/{}", entry.id));
                }
            }
            for provenance in self.provenance_of(&targets).await? {
                self.writer
                    .write_resource("Provenance", &provenance)
                    .await
                    .map_err(|e| format!("Failed to write resource: {e}"))?;
            }
            self.collect_closed().await?;
            // Check if we've retrieved all resources
            if !result.has_more {
                break;
            }
        }
        Ok(())
    }

    /// `_elements`: keep what was asked for and what the type cannot do without,
    /// and tag the rest of the resource as missing.
    async fn project<'r>(
        &mut self,
        resource_type: &str,
        resource: &'r Value,
    ) -> Result<Cow<'r, Value>, String> {
        if self.params.elements.is_empty() {
            return Ok(Cow::Borrowed(resource));
        }
        if !self.mandatory.contains_key(resource_type) {
            let required = self
                .state
                .model_provider
                .get_schema(resource_type)
                .await
                .and_then(|schema| schema.required.clone())
                .unwrap_or_default();
            self.mandatory
                .insert(resource_type.to_owned(), required.into_iter().collect());
        }
        let mandatory = &self.mandatory[resource_type];
        let requested = |name: &str| {
            self.params
                .elements
                .iter()
                .any(|element| match element.split_once('.') {
                    Some((owner, element)) => owner == resource_type && element == name,
                    None => element == name,
                })
        };
        let keep = |name: &str| {
            let base = name.strip_prefix('_').unwrap_or(name);
            matches!(base, "resourceType" | "id" | "meta")
                || mandatory.contains(base)
                || requested(base)
        };
        let Some(fields) = resource.as_object() else {
            return Ok(Cow::Borrowed(resource));
        };
        let mut projected = Value::Object(
            fields
                .iter()
                .filter(|(name, _)| keep(name))
                .map(|(name, value)| (name.clone(), value.clone()))
                .collect(),
        );
        crate::handlers::add_subsetted_tag(&mut projected);
        Ok(Cow::Owned(projected))
    }

    /// Provenance of `targets` (`Type/id`) that was not written yet.
    async fn provenance_of(&mut self, targets: &[String]) -> Result<Vec<Value>, String> {
        let Some(mode) = self.provenance else {
            return Ok(Vec::new());
        };
        let mut found: HashMap<String, Value> = HashMap::new();
        for chunk in targets.chunks(PROVENANCE_TARGETS) {
            let params = SearchParams::new()
                .with_count(self.batch_size)
                .with_param("target", chunk.join(","))
                .with_param(
                    "_lastUpdated",
                    format!("le{}", self.started_at.to_rfc3339()),
                );
            let mut after: Option<String> = None;
            loop {
                let result = self.page("Provenance", &params, after.as_deref()).await?;
                after = result.entries.last().map(|entry| entry.id.clone());
                let done = !result.has_more || result.entries.is_empty();
                for entry in &result.entries {
                    found.insert(entry.id.clone(), parse_stored(entry)?);
                }
                if done {
                    break;
                }
            }
        }
        let chosen: Vec<&String> = match mode {
            AssociatedData::RelevantProvenanceResources => found.keys().collect(),
            AssociatedData::LatestProvenanceResources => latest_provenance(&found, targets),
        };
        let chosen: Vec<String> = chosen.into_iter().cloned().collect();
        let mut lines = Vec::with_capacity(chosen.len());
        for id in chosen {
            if self.written_provenance.insert(id.clone())
                && let Some(resource) = found.remove(&id)
            {
                lines.push(self.project("Provenance", &resource).await?.into_owned());
            }
        }
        lines.sort_by(|left, right| left["id"].as_str().cmp(&right["id"].as_str()));
        Ok(lines)
    }

    /// `organizeOutputBy=Patient`: a header per Patient, then the Patient and what
    /// references it. Resources of one chunk of Patients are fetched together.
    async fn export_blocks(
        &mut self,
        resource_types: &[String],
        selected: Option<Vec<String>>,
    ) -> Result<(), String> {
        if let Some(mut ids) = selected {
            ids.sort();
            ids.dedup();
            let chunks = ids.len().div_ceil(BLOCK_PATIENTS).max(1);
            for (index, chunk) in ids.chunks(BLOCK_PATIENTS).enumerate() {
                self.export_block_chunk(chunk, resource_types).await?;
                let progress = (index + 1) as f32 / chunks as f32;
                if let Err(e) = self
                    .state
                    .async_job_manager
                    .update_progress(self.job_id, progress)
                    .await
                {
                    tracing::warn!(error = %e, "Failed to update job progress");
                }
            }
        } else {
            let search = SearchParams::new()
                .with_count(BLOCK_PATIENTS as u32)
                .with_param(
                    "_lastUpdated",
                    format!("le{}", self.started_at.to_rfc3339()),
                );
            let mut after: Option<String> = None;
            loop {
                let result = self.page("Patient", &search, after.as_deref()).await?;
                let ids: Vec<String> = result.entries.into_iter().map(|entry| entry.id).collect();
                let Some(last) = ids.last() else {
                    break;
                };
                after = Some(last.clone());
                self.export_block_chunk(&ids, resource_types).await?;
                if !result.has_more {
                    break;
                }
            }
        }
        self.writer
            .close(PATIENT_BLOCKS_FILE)
            .await
            .map_err(|error| format!("Failed to write resource: {error}"))?;
        self.collect_closed().await
    }

    async fn export_block_chunk(
        &mut self,
        patients: &[String],
        resource_types: &[String],
    ) -> Result<(), String> {
        let members: HashSet<&str> = patients.iter().map(String::as_str).collect();
        let mut blocks: HashMap<String, Vec<(String, Value)>> = HashMap::new();
        for resource_type in resource_types {
            let constraint = export_membership_constraint(
                self.state,
                BulkExportLevel::Group,
                Some(patients),
                resource_type,
            )?;
            let params = export_search_params(
                resource_type,
                self.params,
                self.batch_size,
                constraint,
                self.started_at,
            )?;
            let mut after: Option<String> = None;
            loop {
                let result = self.page(resource_type, &params, after.as_deref()).await?;
                after = result.entries.last().map(|entry| entry.id.clone());
                let done = !result.has_more || result.entries.is_empty();
                for entry in &result.entries {
                    let resource = parse_stored(entry)?;
                    let owners = block_owners(resource_type, &entry.id, &resource, &members);
                    if owners.is_empty() {
                        self.unplaced += 1;
                        continue;
                    }
                    let line = self.project(resource_type, &resource).await?.into_owned();
                    let target = format!("{resource_type}/{}", entry.id);
                    for owner in owners {
                        blocks
                            .entry(owner)
                            .or_default()
                            .push((target.clone(), line.clone()));
                    }
                }
                if done {
                    break;
                }
            }
        }
        for patient in patients {
            let Some(mut block) = blocks.remove(patient) else {
                continue;
            };
            let own = format!("Patient/{patient}");
            let targets: Vec<String> = block.iter().map(|(target, _)| target.clone()).collect();
            let provenance = self.provenance_of(&targets).await?;
            // The Patient comes right after its header.
            block.sort_by_key(|(target, _)| *target != own);
            let mut lines = Vec::with_capacity(block.len() + provenance.len() + 1);
            lines.push(json!({"resourceType": "Parameters", "parameter": [{
                "name": "header",
                "valueReference": {"reference": own},
            }]}));
            lines.extend(block.into_iter().map(|(_, line)| line));
            lines.extend(provenance);
            self.writer
                .write_block(PATIENT_BLOCKS_FILE, &lines)
                .await
                .map_err(|e| format!("Failed to write resource: {e}"))?;
            self.collect_closed().await?;
        }
        Ok(())
    }

    /// `deleted`: with `_since`, a transaction Bundle per resource deleted since then.
    /// History keeps no search values and no reliable body for a deleted resource, so
    /// only a system level export without `_typeFilter` can tell it matched the request;
    /// the IG lets a server omit the rest.
    async fn write_deleted(&mut self, resource_types: &[String]) -> Result<(), String> {
        let Some(since) = self.params.since else {
            return Ok(());
        };
        if self.level != BulkExportLevel::System || self.params.organize_output_by.is_some() {
            return Ok(());
        }
        let filtered: HashSet<String> = self
            .params
            .get_type_filters()?
            .into_iter()
            .map(|(resource_type, _)| resource_type)
            .collect();
        for resource_type in resource_types {
            if filtered.contains(resource_type) || self.has_filtered_grant(resource_type)? {
                continue;
            }
            self.write_deleted_type(resource_type, since).await?;
        }
        self.writer
            .close(DELETED_FILE)
            .await
            .map_err(|error| format!("Failed to write resource: {error}"))?;
        self.collect_closed().await
    }

    /// Whether the caller sees only part of this type: then a deletion cannot be
    /// shown, because its resource may be outside that part.
    fn has_filtered_grant(&self, resource_type: &str) -> Result<bool, String> {
        let Some(policy) = self
            .context
            .policy_context(
                self.state,
                FhirOperation::Search,
                resource_type,
                None,
                &SearchParams::new(),
            )
            .map_err(|error| error.to_string())?
        else {
            return Ok(false);
        };
        crate::authorization::query_visibility(
            &policy,
            &self.state.compartment_registry,
            &self.state.base_url,
        )
        .map(|visibility| visibility.is_some())
        .map_err(|error| error.to_string())
    }

    async fn write_deleted_type(
        &mut self,
        resource_type: &str,
        since: DateTime<Utc>,
    ) -> Result<(), String> {
        let end = to_offset(self.started_at)?;
        let until = self.params.until.map(to_offset).transpose()?;
        let mut deleted = Vec::new();
        let mut seen = HashSet::new();
        let mut offset = 0u32;
        loop {
            let mut history = HistoryParams::new().since(to_offset(since)?);
            history.count = Some(self.batch_size);
            history.offset = Some(offset);
            let entries = self
                .state
                .storage
                .history(resource_type, None, &history)
                .await
                .map_err(|error| format!("History failed for {resource_type}: {error}"))?
                .entries;
            if entries.is_empty() {
                break;
            }
            offset = offset.saturating_add(entries.len() as u32);
            for entry in entries {
                let at = entry.resource.last_updated;
                if entry.method == HistoryMethod::Delete
                    && at <= end
                    && until.is_none_or(|until| at < until)
                    && seen.insert(entry.resource.id.clone())
                {
                    deleted.push(entry.resource.id);
                }
            }
        }
        // A resource written again since then is in `output`, and must not be here too.
        let live: HashSet<String> = self
            .state
            .storage
            .read_many(resource_type, &deleted)
            .await
            .map_err(|error| format!("Read failed for {resource_type}: {error}"))?
            .into_iter()
            .map(|resource| resource.id)
            .collect();
        deleted.sort();
        for id in deleted.iter().filter(|id| !live.contains(*id)) {
            let bundle = json!({"resourceType": "Bundle", "type": "transaction", "entry": [{
                "request": {"method": "DELETE", "url": format!("{resource_type}/{id}")},
            }]});
            self.writer
                .write_resource(DELETED_FILE, &bundle)
                .await
                .map_err(|e| format!("Failed to write resource: {e}"))?;
        }
        Ok(())
    }

    /// One OperationOutcome per ignored parameter, as the IG recommends, plus a note
    /// about anything organized output had to leave out.
    async fn write_messages(&mut self) -> Result<(), String> {
        let mut messages: Vec<(&str, &str, String)> = self
            .params
            .ignored
            .iter()
            .map(|message| ("warning", "not-supported", message.clone()))
            .collect();
        if self.unplaced > 0 {
            messages.push((
                "information",
                "informational",
                format!(
                    "Omitted {} resources that reference no Patient of the export directly",
                    self.unplaced
                ),
            ));
        }
        for (severity, code, diagnostics) in messages {
            let outcome = json!({"resourceType": "OperationOutcome", "issue": [{
                "severity": severity, "code": code, "diagnostics": diagnostics,
            }]});
            self.writer
                .write_resource(OUTCOME_FILE, &outcome)
                .await
                .map_err(|e| format!("Failed to write resource: {e}"))?;
        }
        self.writer
            .close(OUTCOME_FILE)
            .await
            .map_err(|error| format!("Failed to write resource: {error}"))?;
        self.collect_closed().await
    }

    fn message_severities(&self) -> Vec<(String, usize)> {
        let mut counts = Vec::new();
        if !self.params.ignored.is_empty() {
            counts.push(("warning".to_owned(), self.params.ignored.len()));
        }
        if self.unplaced > 0 {
            counts.push(("information".to_owned(), 1));
        }
        counts
    }

    async fn finish(mut self, header: &ManifestHeader) -> Result<Value, String> {
        self.writer
            .close_all()
            .await
            .map_err(|e| format!("Failed to finish writing: {e}"))?;
        self.collect_closed().await?;
        match self.sink {
            ManifestSink::Whole(mut output) => {
                // Stable: the files of one type stay in the order they were written.
                output.sort_by(|left, right| left.resource_type.cmp(&right.resource_type));
                Ok(header.document(&output, &self.deleted, &self.outcome))
            }
            ManifestSink::Paged(pages) => pages.finish(&self.deleted, &self.outcome).await,
        }
    }
}

fn parse_stored(entry: &RawStoredResource) -> Result<Value, String> {
    serde_json::from_str(&entry.resource_json)
        .map_err(|error| format!("Stored {} is not JSON: {error}", entry.id))
}

/// `Type/id` of a reference, without a base URL or version.
fn reference_target(reference: &str) -> Option<&str> {
    let unversioned = reference
        .rsplit_once("/_history/")
        .map_or(reference, |(target, _)| target);
    let mut parts = unversioned.rsplitn(3, '/');
    let id = parts.next()?;
    let resource_type = parts.next()?;
    Some(&unversioned[unversioned.len() - id.len() - resource_type.len() - 1..])
}

/// Every Patient id `value` refers to, at any depth.
fn collect_patient_references<'v>(value: &'v Value, found: &mut Vec<&'v str>) {
    match value {
        Value::Object(fields) => {
            for (name, field) in fields {
                if name == "reference"
                    && let Some(id) = field
                        .as_str()
                        .and_then(reference_target)
                        .and_then(|target| target.strip_prefix("Patient/"))
                {
                    found.push(id);
                } else {
                    collect_patient_references(field, found);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_patient_references(item, found);
            }
        }
        _ => {}
    }
}

/// The Patients of `members` whose block holds this resource.
fn block_owners(
    resource_type: &str,
    id: &str,
    resource: &Value,
    members: &HashSet<&str>,
) -> Vec<String> {
    if resource_type == "Patient" && members.contains(id) {
        return vec![id.to_owned()];
    }
    let mut referenced = Vec::new();
    collect_patient_references(resource, &mut referenced);
    referenced.sort_unstable();
    referenced.dedup();
    referenced
        .into_iter()
        .filter(|patient| members.contains(patient))
        .map(str::to_owned)
        .collect()
}

/// The ids of the newest Provenance of each target.
fn latest_provenance<'a>(found: &'a HashMap<String, Value>, targets: &[String]) -> Vec<&'a String> {
    let wanted: HashSet<&str> = targets.iter().map(String::as_str).collect();
    let mut newest: HashMap<&str, (DateTime<Utc>, &String)> = HashMap::new();
    for (id, provenance) in found {
        let recorded = provenance["recorded"]
            .as_str()
            .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
            .map_or(DateTime::<Utc>::MIN_UTC, |value| value.with_timezone(&Utc));
        for target in provenance["target"].as_array().into_iter().flatten() {
            let Some(target) = target["reference"].as_str().and_then(reference_target) else {
                continue;
            };
            if !wanted.contains(target) {
                continue;
            }
            let entry = newest.entry(target).or_insert((recorded, id));
            if (recorded, id) > (entry.0, entry.1) {
                *entry = (recorded, id);
            }
        }
    }
    let mut ids: Vec<&String> = newest.into_values().map(|(_, id)| id).collect();
    ids.sort();
    ids.dedup();
    ids
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
) -> Result<Option<SearchConstraint>, String> {
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
    // `patient` narrows a Patient level export the way membership narrows a Group's.
    Ok(Some(match (level, patient_ids) {
        (BulkExportLevel::Patient, None) => SearchConstraint::AllPatients {
            fhir_base_url: state.fhir_base_url(),
            parameters,
        },
        (_, Some(patient_ids)) => SearchConstraint::LinkedPatients {
            patient_ids: patient_ids.to_vec(),
            fhir_base_url: state.fhir_base_url(),
            parameters,
        },
        (_, None) => return Err("Missing resolved Group membership".into()),
    }))
}

/// Native disjunctions preserve repeated type filters while grants and
/// compartment membership remain independent conjunctions.
fn export_search_params(
    resource_type: &str,
    params: &BulkExportParams,
    count: u32,
    membership: Option<SearchConstraint>,
    started_at: DateTime<Utc>,
) -> Result<SearchParams, String> {
    let mut search = SearchParams::new()
        .with_count(count)
        .with_sort("_id", false);
    let mut constraints = membership.into_iter().collect::<Vec<_>>();
    let mut filters = Vec::new();
    for (filter_type, query) in params.get_type_filters()? {
        if filter_type == resource_type {
            let mut parameters: HashMap<String, Vec<String>> = HashMap::new();
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
    if let Some(until) = &params.until {
        search = search.with_param("_lastUpdated", format!("lt{}", until.to_rfc3339()));
    }
    Ok(search.with_param("_lastUpdated", format!("le{}", started_at.to_rfc3339())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_params_empty() {
        let config = BulkExportConfig::default();
        let op = ExportOperation::new(config);
        let params = json!({});
        let result = op.parse_params(&params, false).unwrap();
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
        let result = op.parse_params(&params, false).unwrap();
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
        let result = op.parse_params(&params, false);
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
        let parsed = op.parse_params(&params, false).unwrap();
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
                op.parse_params(
                    &json!({"resourceType":"Parameters","parameter":[parameter]}),
                    false
                )
                .is_err()
            );
        }
    }

    fn parse_stu4(parameters: Value) -> Result<BulkExportParams, OperationError> {
        let config = BulkExportConfig {
            spec_version: BulkSpecVersion::Stu4,
            ..Default::default()
        };
        ExportOperation::new(config).parse_params(
            &json!({"resourceType":"Parameters","parameter":parameters}),
            false,
        )
    }

    #[test]
    fn export_parameters_cover_the_ig_list() {
        let op = ExportOperation::new(BulkExportConfig::default());
        let parsed = op
            .parse_params(
                &json!({"resourceType":"Parameters","parameter":[
                    {"name":"_since","valueInstant":"2025-01-01T00:00:00Z"},
                    {"name":"_until","valueInstant":"2025-02-01T00:00:00Z"},
                    {"name":"_elements","valueString":"id,Patient.name"},
                    {"name":"patient","valueReference":{"reference":"Patient/1"}},
                    {"name":"patient","valueReference":{"reference":"Patient/2"}},
                    {"name":"includeAssociatedData","valueCode":"LatestProvenanceResources"},
                    {"name":"includeAssociatedData","valueCode":"RelevantProvenanceResources"},
                    {"name":"organizeOutputBy","valueString":"Patient"},
                    {"name":"allowPartialManifests","valueBoolean":true}
                ]}),
                false,
            )
            .unwrap();
        assert!(parsed.until.is_some());
        assert_eq!(parsed.elements, vec!["id", "Patient.name"]);
        assert_eq!(parsed.patients, vec!["Patient/1", "Patient/2"]);
        assert_eq!(
            parsed.associated_data,
            Some(AssociatedData::RelevantProvenanceResources),
            "the least restrictive value wins"
        );
        assert_eq!(parsed.organize_output_by.as_deref(), Some("Patient"));
        assert!(parsed.allow_partial_manifests);
    }

    #[test]
    fn unsupported_input_is_refused_unless_the_request_is_lenient() {
        let op = ExportOperation::new(BulkExportConfig::default());
        for parameter in [
            json!({"name":"organizeOutputBy","valueString":"Encounter"}),
            json!({"name":"includeAssociatedData","valueCode":"_custom"}),
            json!({"name":"_elements","valueString":"Patient.name.family"}),
            json!({"name":"unknown","valueString":"x"}),
        ] {
            let request = json!({"resourceType":"Parameters","parameter":[parameter]});
            assert!(op.parse_params(&request, false).is_err(), "{request}");
            let lenient = op.parse_params(&request, true).unwrap();
            assert_eq!(lenient.ignored.len(), 1, "{request}");
        }
        assert!(
            op.parse_params(
                &json!({"resourceType":"Parameters","parameter":[
                    {"name":"_since","valueInstant":"2025-02-01T00:00:00Z"},
                    {"name":"_until","valueInstant":"2025-01-01T00:00:00Z"}
                ]}),
                true
            )
            .is_err(),
            "an empty window is a client error even when lenient"
        );
    }

    #[test]
    fn stu4_refuses_what_it_removed() {
        assert!(parse_stu4(json!([{"name":"_type","valueString":"Patient,Observation"}])).is_err());
        assert!(
            parse_stu4(
                json!([{"name":"includeAssociatedData","valueCode":"LatestProvenanceResources"}])
            )
            .is_err()
        );
        let parsed = parse_stu4(json!([
            {"name":"_type","valueString":"Patient"},
            {"name":"_type","valueString":"Observation"},
            {"name":"includeAssociatedData","valueCoding":{"code":"LatestProvenanceResources"}}
        ]))
        .unwrap();
        assert_eq!(parsed.get_resource_types(), vec!["Patient", "Observation"]);
        assert_eq!(
            parsed.associated_data,
            Some(AssociatedData::LatestProvenanceResources)
        );
    }

    #[test]
    fn references_reduce_to_their_target() {
        for (reference, target) in [
            ("Patient/1", Some("Patient/1")),
            ("https://example.org/fhir/Patient/1", Some("Patient/1")),
            ("Patient/1/_history/2", Some("Patient/1")),
            ("urn:uuid:1", None),
            ("#contained", None),
        ] {
            assert_eq!(reference_target(reference), target, "{reference}");
        }
    }

    #[test]
    fn blocks_hold_what_references_their_patient() {
        let members = HashSet::from(["a", "b"]);
        let observation = json!({"subject":{"reference":"Patient/a"},
            "performer":[{"reference":"https://example.org/fhir/Patient/b"}],
            "hasMember":[{"reference":"Observation/1"}]});
        assert_eq!(
            block_owners("Observation", "1", &observation, &members),
            vec!["a", "b"]
        );
        assert!(
            block_owners(
                "Observation",
                "1",
                &json!({"subject":{"reference":"Patient/z"}}),
                &members
            )
            .is_empty()
        );
        assert_eq!(
            block_owners("Patient", "a", &json!({}), &members),
            vec!["a"]
        );
    }

    #[test]
    fn latest_provenance_is_chosen_per_target() {
        let found = HashMap::from([
            (
                "old".to_owned(),
                json!({"recorded":"2024-01-01T00:00:00Z","target":[{"reference":"Patient/1"}]}),
            ),
            (
                "new".to_owned(),
                json!({"recorded":"2025-01-01T00:00:00+00:00","target":[{"reference":"Patient/1"}]}),
            ),
            (
                "other".to_owned(),
                json!({"recorded":"2020-01-01T00:00:00Z","target":[{"reference":"Patient/2"}]}),
            ),
        ]);
        let targets = ["Patient/1".to_owned(), "Patient/2".to_owned()];
        let mut chosen: Vec<_> = latest_provenance(&found, &targets)
            .into_iter()
            .map(String::as_str)
            .collect();
        chosen.sort_unstable();
        assert_eq!(chosen, vec!["new", "other"]);
    }
}
