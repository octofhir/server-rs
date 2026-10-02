//! $everything Operation Handler
//!
//! Implements the FHIR $everything operation for Patient, Encounter, and Group resources.
//! This operation returns all resources related to the specified resource.
//!
//! # Supported Resource Types
//! - **Patient**: Returns patient + all related resources (observations, conditions, medications, etc.)
//! - **Encounter**: Returns encounter + all resources related to that encounter
//! - **Group**: Returns resources for all members of the group (bulk export use case)
//!
//! # Query Parameters
//! - `_count`: Limit number of results (pagination)
//! - `_since`: Only resources updated since this date
//! - `_type`: Filter to specific resource types (comma-separated)
//! - `_elements`: Select specific elements to include
//! - `start` & `end`: Date range filter (Patient $everything only)

use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashSet;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::handler::{OperationError, OperationHandler};
use crate::server::AppState;
use octofhir_api::bundle_from_search;
use octofhir_storage::{SearchParams, StoredResource};

/// Handler for the $everything operation.
pub struct EverythingOperation;

impl Default for EverythingOperation {
    fn default() -> Self {
        Self::new()
    }
}

impl EverythingOperation {
    pub fn new() -> Self {
        Self
    }

    /// Patient $everything: Retrieve complete patient record
    async fn patient_everything(
        &self,
        state: &AppState,
        context: &crate::operations::OperationContext,
        patient_id: &str,
        params: &EverythingParams,
    ) -> Result<Value, OperationError> {
        // 1. Verify patient exists and fetch it
        let patient = context
            .read(state, "Patient", patient_id)
            .await?
            .ok_or_else(|| OperationError::NotFound(format!("Patient/{} not found", patient_id)))?;

        let mut resources = vec![patient.clone()];

        // 2. Query related resources using reference parameters
        // Based on FHIR Patient compartment definition
        let definition = state
            .compartment_registry
            .get("Patient")
            .map_err(|error| OperationError::Internal(error.to_string()))?;
        let compartment_searches = definition.resource_types();

        for resource_type in compartment_searches {
            // Skip if _type filter is specified and doesn't include this type
            if let Some(ref types) = params.type_filter
                && !types.contains(resource_type)
            {
                continue;
            }

            // Build search query using modern FhirStorage
            if !state.resource_type_set.load().contains(resource_type) {
                continue;
            }
            let mut search_params = SearchParams::new().with_count(1000);
            search_params.constraint =
                Some(definition.membership_constraint(patient_id, resource_type, &state.base_url));

            // Apply _since filter if specified
            if let Some(since) = params.since {
                let since_str = since
                    .format(&time::format_description::well_known::Rfc3339)
                    .unwrap_or_default();
                search_params =
                    search_params.with_param("_lastUpdated", format!("ge{}", since_str));
            }

            // Preserve FHIR date precision through native date comparisons.
            if let Some(start) = &params.start {
                search_params = search_params.with_param("date", format!("ge{start}"));
            }
            if let Some(end) = &params.end {
                search_params = search_params.with_param("date", format!("le{end}"));
            }

            match self
                .search_all(state, context, resource_type, &search_params)
                .await
            {
                Ok(result) => {
                    resources.extend(result);
                }
                Err(e) => {
                    if !e.is_forbidden() {
                        return Err(e);
                    }
                    tracing::debug!(
                        "Failed to search {} for patient {}: {}",
                        resource_type,
                        patient_id,
                        e
                    );
                }
            }
        }

        self.build_bundle(
            state,
            resources,
            params,
            &format!("Patient/{}/$everything", patient_id),
        )
    }

    /// Encounter $everything: Retrieve all resources related to an encounter
    async fn encounter_everything(
        &self,
        state: &AppState,
        context: &crate::operations::OperationContext,
        encounter_id: &str,
        params: &EverythingParams,
    ) -> Result<Value, OperationError> {
        // 1. Verify encounter exists and fetch it
        let encounter = context
            .read(state, "Encounter", encounter_id)
            .await?
            .ok_or_else(|| {
                OperationError::NotFound(format!("Encounter/{} not found", encounter_id))
            })?;

        let mut resources = vec![encounter.clone()];

        // 2. Query related resources using encounter reference
        let definition = state
            .compartment_registry
            .get("Encounter")
            .map_err(|error| OperationError::Internal(error.to_string()))?;
        let compartment_searches = definition.resource_types();

        for resource_type in compartment_searches {
            if let Some(ref types) = params.type_filter
                && !types.contains(resource_type)
            {
                continue;
            }

            if !state.resource_type_set.load().contains(resource_type) {
                continue;
            }
            let mut search_params = SearchParams::new().with_count(1000);
            search_params.constraint = Some(definition.membership_constraint(
                encounter_id,
                resource_type,
                &state.base_url,
            ));

            if let Some(since) = params.since {
                let since_str = since
                    .format(&time::format_description::well_known::Rfc3339)
                    .unwrap_or_default();
                search_params =
                    search_params.with_param("_lastUpdated", format!("ge{}", since_str));
            }

            match self
                .search_all(state, context, resource_type, &search_params)
                .await
            {
                Ok(result) => {
                    resources.extend(result);
                }
                Err(e) => {
                    if !e.is_forbidden() {
                        return Err(e);
                    }
                    tracing::debug!(
                        "Failed to search {} for encounter {}: {}",
                        resource_type,
                        encounter_id,
                        e
                    );
                }
            }
        }

        self.build_bundle(
            state,
            resources,
            params,
            &format!("Encounter/{}/$everything", encounter_id),
        )
    }

    /// Group $everything: Retrieve resources for all group members
    async fn group_everything(
        &self,
        state: &AppState,
        context: &crate::operations::OperationContext,
        group_id: &str,
        params: &EverythingParams,
    ) -> Result<Value, OperationError> {
        // 1. Verify group exists and fetch it
        let group = context
            .read(state, "Group", group_id)
            .await?
            .ok_or_else(|| OperationError::NotFound(format!("Group/{} not found", group_id)))?;

        // 2. Extract member references from Group.member[]
        let members = self.extract_group_members(&group)?;

        if members.is_empty() {
            return self.build_bundle(
                state,
                vec![group],
                params,
                &format!("Group/{}/$everything", group_id),
            );
        }

        // 3. For each member, run Patient $everything
        let mut all_resources = vec![group];
        let mut member_params = params.clone();
        member_params.count = None;
        member_params.offset = None;
        for member_id in members {
            match self
                .patient_everything(state, context, &member_id, &member_params)
                .await
            {
                Ok(bundle_value) => {
                    // Extract resources from the bundle
                    if let Some(entries) = bundle_value.get("entry").and_then(|e| e.as_array()) {
                        for entry in entries {
                            if let Some(resource) = entry.get("resource") {
                                // Create a StoredResource from the bundle entry
                                let stored = StoredResource {
                                    id: resource
                                        .get("id")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("")
                                        .to_string(),
                                    version_id: resource
                                        .get("meta")
                                        .and_then(|m| m.get("versionId"))
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("1")
                                        .to_string(),
                                    resource_type: resource
                                        .get("resourceType")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("")
                                        .to_string(),
                                    resource: resource.clone(),
                                    last_updated: time::OffsetDateTime::now_utc(),
                                    created_at: time::OffsetDateTime::now_utc(),
                                };
                                all_resources.push(stored);
                            }
                        }
                    }
                }
                Err(e) => {
                    if !e.is_forbidden() {
                        return Err(e);
                    }
                    tracing::debug!(
                        "Failed to fetch $everything for patient {}: {}",
                        member_id,
                        e
                    );
                }
            }
        }

        self.build_bundle(
            state,
            all_resources,
            params,
            &format!("Group/{}/$everything", group_id),
        )
    }

    fn build_bundle(
        &self,
        state: &AppState,
        mut resources: Vec<StoredResource>,
        params: &EverythingParams,
        path: &str,
    ) -> Result<Value, OperationError> {
        resources.retain(|resource| {
            params
                .type_filter
                .as_ref()
                .is_none_or(|types| types.contains(&resource.resource_type))
                && params
                    .since
                    .is_none_or(|since| resource.last_updated >= since)
        });
        let resources = self.deduplicate_resources(resources);
        let total = resources.len();
        let offset = params.offset.unwrap_or(0).min(total);
        let end = params
            .count
            .map_or(total, |count| offset.saturating_add(count).min(total));
        let page = &resources[offset..end];
        let fhir_base = format!("{}/fhir", state.base_url.trim_end_matches('/'));
        let mut bundle = bundle_from_search(
            total,
            page.iter()
                .map(|resource| resource.resource.clone())
                .collect(),
            &fhir_base,
            path,
            params.offset.unwrap_or(0),
            params.count.unwrap_or(50),
            params.query_suffix().as_deref(),
        );
        for (entry, resource) in bundle.entry.iter_mut().zip(page) {
            entry.full_url = Some(format!(
                "{}/{}/{}",
                fhir_base, resource.resource_type, resource.id
            ));
        }
        serde_json::to_value(bundle).map_err(|error| OperationError::Internal(error.to_string()))
    }

    async fn search_all(
        &self,
        state: &AppState,
        context: &crate::operations::OperationContext,
        resource_type: &str,
        params: &SearchParams,
    ) -> Result<Vec<StoredResource>, OperationError> {
        let mut params = params.clone();
        let mut resources = Vec::new();
        loop {
            let result = context.search(state, resource_type, &params).await?;
            let count = result.entries.len();
            resources.extend(result.entries);
            if !result.has_more {
                return Ok(resources);
            }
            if count == 0 {
                return Err(OperationError::Internal(
                    "Search made no progress during $everything".into(),
                ));
            }
            params.offset = Some(
                params
                    .offset
                    .unwrap_or(0)
                    .checked_add(count as u32)
                    .ok_or_else(|| {
                        OperationError::Internal("$everything search offset overflow".into())
                    })?,
            );
        }
    }

    /// Extract patient member IDs from a Group resource
    fn extract_group_members(&self, group: &StoredResource) -> Result<Vec<String>, OperationError> {
        let mut member_ids = Vec::new();

        if let Some(members) = group.resource.get("member").and_then(|m| m.as_array()) {
            for member in members {
                if let Some(entity) = member.get("entity").and_then(|e| e.get("reference"))
                    && let Some(reference) = entity.as_str()
                {
                    // Extract ID from reference like "Patient/123"
                    if let Some(id) = reference.strip_prefix("Patient/") {
                        member_ids.push(id.to_string());
                    }
                }
            }
        }

        Ok(member_ids)
    }

    /// Deduplicate resources by type and id
    fn deduplicate_resources(&self, resources: Vec<StoredResource>) -> Vec<StoredResource> {
        let mut seen = HashSet::new();
        let mut deduplicated = Vec::new();

        for resource in resources {
            let key = (resource.resource_type.clone(), resource.id.clone());
            if seen.insert(key) {
                deduplicated.push(resource);
            }
        }

        deduplicated.sort_by(|left, right| {
            (&left.resource_type, &left.id).cmp(&(&right.resource_type, &right.id))
        });
        deduplicated
    }
}

#[async_trait]
impl OperationHandler for EverythingOperation {
    fn code(&self) -> &str {
        "everything"
    }

    async fn handle_instance(
        &self,
        state: &AppState,
        context: &crate::operations::OperationContext,
        resource_type: &str,
        id: &str,
        params: &Value,
    ) -> Result<Value, OperationError> {
        // Parse parameters
        let mut everything_params = EverythingParams::from_value(params)?;
        if let Some(types) = &everything_params.type_filter
            && types
                .iter()
                .any(|resource_type| !state.resource_type_set.load().contains(resource_type))
        {
            return Err(OperationError::InvalidParameters(
                "_type contains an unsupported resource type".into(),
            ));
        }
        everything_params
            .count
            .get_or_insert(state.search_config.config().default_count);

        match resource_type {
            "Patient" => {
                self.patient_everything(state, context, id, &everything_params)
                    .await
            }
            "Encounter" => {
                self.encounter_everything(state, context, id, &everything_params)
                    .await
            }
            "Group" => {
                self.group_everything(state, context, id, &everything_params)
                    .await
            }
            _ => Err(OperationError::NotSupported(format!(
                "$everything operation is not supported for resource type {}",
                resource_type
            ))),
        }
    }
}

/// Parameters for the $everything operation
#[derive(Debug, Default, Clone)]
struct EverythingParams {
    /// Limit number of results
    count: Option<usize>,
    /// Offset for pagination
    offset: Option<usize>,
    /// Only resources updated since this date
    since: Option<OffsetDateTime>,
    /// Filter to specific resource types
    type_filter: Option<HashSet<String>>,
    /// Date range start (Patient $everything only)
    start: Option<String>,
    /// Date range end (Patient $everything only)
    end: Option<String>,
}

impl EverythingParams {
    fn query_suffix(&self) -> Option<String> {
        let mut query = url::form_urlencoded::Serializer::new(String::new());
        if let Some(types) = &self.type_filter {
            let mut types: Vec<_> = types.iter().map(String::as_str).collect();
            types.sort_unstable();
            query.append_pair("_type", &types.join(","));
        }
        if let Some(since) = self.since {
            query.append_pair("_since", &since.format(&Rfc3339).unwrap_or_default());
        }
        for (name, value) in [("start", &self.start), ("end", &self.end)] {
            if let Some(value) = value {
                query.append_pair(name, value);
            }
        }
        let query = query.finish();
        (!query.is_empty()).then_some(query)
    }

    fn from_value(params: &Value) -> Result<Self, OperationError> {
        let mut flat = params.as_object().cloned().unwrap_or_default();
        if let Some(parameters) = params.get("parameter").and_then(Value::as_array) {
            for parameter in parameters {
                let Some(name) = parameter["name"].as_str() else {
                    continue;
                };
                if !matches!(
                    name,
                    "_count" | "_offset" | "__offset" | "_since" | "_type" | "start" | "end"
                ) {
                    continue;
                }
                let value = parameter
                    .as_object()
                    .and_then(|parameter| {
                        parameter
                            .iter()
                            .find(|(key, _)| key.starts_with("value"))
                            .map(|(_, value)| value.clone())
                    })
                    .ok_or_else(|| {
                        OperationError::InvalidParameters(format!("Missing value for {name}"))
                    })?;
                if name == "_type"
                    && let Some(existing) = flat.get(name).and_then(Value::as_str)
                {
                    let value = value.as_str().ok_or_else(|| {
                        OperationError::InvalidParameters(
                            "_type must contain resource type codes".into(),
                        )
                    })?;
                    flat.insert(name.into(), Value::String(format!("{existing},{value}")));
                } else if flat.insert(name.into(), value).is_some() {
                    return Err(OperationError::InvalidParameters(format!(
                        "Repeated {name} parameter"
                    )));
                }
            }
        }
        let params = Value::Object(flat);
        let mut result = EverythingParams::default();

        let unsigned = |name: &str, value: &Value| -> Result<usize, OperationError> {
            value
                .as_u64()
                .and_then(|value| usize::try_from(value).ok())
                .or_else(|| value.as_str().and_then(|value| value.parse().ok()))
                .ok_or_else(|| {
                    OperationError::InvalidParameters(format!(
                        "{name} must be a nonnegative integer"
                    ))
                })
        };
        if let Some(count) = params.get("_count") {
            result.count = Some(unsigned("_count", count)?);
        }
        if let Some(offset) = params.get("_offset").or_else(|| params.get("__offset")) {
            result.offset = Some(unsigned("_offset", offset)?);
        }
        if let Some(since) = params.get("_since") {
            result.since = Some(
                OffsetDateTime::parse(since.as_str().unwrap_or_default(), &Rfc3339).map_err(
                    |error| {
                        OperationError::InvalidParameters(format!(
                            "Invalid _since instant: {error}"
                        ))
                    },
                )?,
            );
        }
        if let Some(types) = params.get("_type") {
            let types = types.as_str().ok_or_else(|| {
                OperationError::InvalidParameters("_type must contain resource type codes".into())
            })?;
            let types: HashSet<String> = types
                .split(',')
                .map(|value| value.trim().to_owned())
                .collect();
            if types.iter().any(|value| {
                value.is_empty() || value.parse::<octofhir_core::ResourceType>().is_err()
            }) {
                return Err(OperationError::InvalidParameters(
                    "Invalid _type resource code".into(),
                ));
            }
            result.type_filter = Some(types);
        }
        for (name, destination) in [("start", &mut result.start), ("end", &mut result.end)] {
            if let Some(value) = params.get(name) {
                let value = value.as_str().ok_or_else(|| {
                    OperationError::InvalidParameters(format!("{name} must be a FHIR date"))
                })?;
                octofhir_search::types::date::parse_date_range(value).map_err(|error| {
                    OperationError::InvalidParameters(format!("Invalid {name}: {error}"))
                })?;
                *destination = Some(value.into());
            }
        }
        if let (Some(start), Some(end)) = (&result.start, &result.end) {
            let start = octofhir_search::types::date::parse_date_range(start).unwrap();
            let end = octofhir_search::types::date::parse_date_range(end).unwrap();
            if start.start >= end.end {
                return Err(OperationError::InvalidParameters(
                    "start is after end".into(),
                ));
            }
        }

        Ok(result)
    }
}
