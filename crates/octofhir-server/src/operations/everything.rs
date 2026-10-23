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

use crate::operations::handler::OperationLevel;

use async_trait::async_trait;
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};
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

    fn prepare_params(
        &self,
        state: &AppState,
        params: &mut EverythingParams,
    ) -> Result<(), OperationError> {
        if params.type_filter.as_ref().is_some_and(|types| {
            types
                .iter()
                .any(|resource_type| !state.resource_type_set.load().contains(resource_type))
        }) {
            return Err(OperationError::InvalidParameters(
                "_type contains an unsupported resource type".into(),
            ));
        }
        let config = state.search_config.config();
        params.count = Some(
            params
                .count
                .unwrap_or(config.default_count)
                .min(config.max_count),
        );
        Ok(())
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

        let resources = self
            .patient_resources(state, context, vec![patient], params)
            .await?;
        self.build_bundle(
            state,
            resources,
            params,
            &format!("Patient/{patient_id}/$everything"),
        )
    }

    /// A group of roots uses one compartment query per resource type, retaining
    /// the actual saved snapshots throughout collection and pagination.
    async fn patient_resources(
        &self,
        state: &AppState,
        context: &crate::operations::OperationContext,
        mut resources: Vec<StoredResource>,
        params: &EverythingParams,
    ) -> Result<Vec<StoredResource>, OperationError> {
        let patient_ids: Vec<_> = resources.iter().map(|patient| patient.id.clone()).collect();
        if patient_ids.is_empty() {
            return Ok(resources);
        }
        // 2. Query related resources using reference parameters
        // Based on FHIR Patient compartment definition
        let definition = state
            .compartment_registry
            .get("Patient")
            .map_err(|error| OperationError::Internal(error.to_string()))?;
        let compartment_searches = definition.resource_types();

        for resource_type in compartment_searches {
            // Build search query using modern FhirStorage
            if !state.resource_type_set.load().contains(resource_type) {
                continue;
            }
            let mut search_params = SearchParams::new().with_count(1000);
            search_params.constraint = Some(octofhir_storage::SearchConstraint::LinkedPatients {
                patient_ids: patient_ids.clone(),
                fhir_base_url: state.fhir_base_url(),
                parameters: definition
                    .get_inclusion_params(resource_type)
                    .unwrap_or_default()
                    .to_vec(),
            });

            match self
                .search_all(state, context, resource_type, &search_params)
                .await
            {
                Ok(result) => {
                    for resource in result {
                        if super::care_dates::matches(
                            &resource.resource,
                            params.start.as_deref(),
                            params.end.as_deref(),
                        )? {
                            resources.push(resource);
                        }
                    }
                    self.check_budget(state, &resources)?;
                }
                Err(e) => {
                    if !e.is_forbidden() {
                        return Err(e);
                    }
                    tracing::debug!(
                        "Failed to search {} for patient group: {}",
                        resource_type,
                        e
                    );
                }
            }
        }

        self.supporting_resources(state, context, resources).await
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
            if !state.resource_type_set.load().contains(resource_type) {
                continue;
            }
            let mut search_params = SearchParams::new().with_count(1000);
            search_params.constraint = Some(definition.membership_constraint(
                encounter_id,
                resource_type,
                &state.base_url,
            ));

            match self
                .search_all(state, context, resource_type, &search_params)
                .await
            {
                Ok(result) => {
                    resources.extend(result);
                    self.check_budget(state, &resources)?;
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

        let resources = self.supporting_resources(state, context, resources).await?;
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
        let membership =
            super::group::resolve_membership(state, context, group_id, OffsetDateTime::now_utc())
                .await?;
        let groups = membership.groups;
        let members = membership.patient_ids;

        if members.is_empty() {
            return self.build_bundle(
                state,
                groups,
                params,
                &format!("Group/{}/$everything", group_id),
            );
        }

        let patients = match context.read_many(state, "Patient", &members).await {
            Ok(patients) => patients,
            Err(error) if error.is_forbidden() => Vec::new(),
            Err(error) => return Err(error),
        };
        let mut all_resources = self
            .patient_resources(state, context, patients, params)
            .await?;
        all_resources.extend(groups);

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
        self.check_budget(state, &resources)?;
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
        let projections = params
            .elements
            .as_ref()
            .map(|elements| {
                std::collections::HashMap::from([("_elements".into(), elements.clone())])
            })
            .unwrap_or_default();
        crate::handlers::apply_result_params(bundle, &projections).map_err(Into::into)
    }

    async fn search_all(
        &self,
        state: &AppState,
        context: &crate::operations::OperationContext,
        resource_type: &str,
        params: &SearchParams,
    ) -> Result<Vec<StoredResource>, OperationError> {
        let mut params = params.clone();
        params = params.with_sort("_id", false);
        let mut resources = Vec::new();
        loop {
            let result = context.search(state, resource_type, &params).await?;
            let count = result.entries.len();
            resources.extend(result.entries);
            self.check_budget(state, &resources)?;
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

    fn check_budget(
        &self,
        state: &AppState,
        resources: &[StoredResource],
    ) -> Result<(), OperationError> {
        let limits = state.config.search.include_limits;
        let mut identities = HashSet::new();
        let mut bytes = 0usize;
        for resource in resources {
            if identities.insert((&resource.resource_type, &resource.id, &resource.version_id)) {
                bytes = bytes.saturating_add(
                    serde_json::to_vec(&resource.resource)
                        .map_err(|error| OperationError::Internal(error.to_string()))?
                        .len(),
                );
            }
        }
        if identities.len() > limits.max_resources || bytes > limits.max_bytes {
            return Err(OperationError::InvalidParameters(
                "$everything exceeds the configured resource or byte budget; narrow the request"
                    .into(),
            ));
        }
        Ok(())
    }

    async fn supporting_resources(
        &self,
        state: &AppState,
        context: &crate::operations::OperationContext,
        resources: Vec<StoredResource>,
    ) -> Result<Vec<StoredResource>, OperationError> {
        use octofhir_core::fhir_reference::parse_reference;
        let mut resources = self.deduplicate_resources(resources);
        let mut walker = crate::bundle_references::BundleReferenceRemapper::default();
        let mut attempted = HashSet::new();
        for resource in &resources {
            attempted.insert((resource.resource_type.clone(), resource.id.clone(), None));
            attempted.insert((
                resource.resource_type.clone(),
                resource.id.clone(),
                Some(resource.version_id.clone()),
            ));
        }
        let mut frontier = 0..resources.len();
        let mut depth = 0usize;
        loop {
            let mut current: BTreeMap<String, Vec<String>> = BTreeMap::new();
            let mut historical = Vec::new();
            for resource in &resources[frontier] {
                for reference in walker
                    .references(state.model_provider.as_ref(), &resource.resource)
                    .await?
                {
                    let Ok(reference) = parse_reference(&reference, Some(&state.fhir_base_url()))
                    else {
                        continue;
                    };
                    if !state
                        .resource_type_set
                        .load()
                        .contains(&reference.resource_type)
                        || !attempted.insert((
                            reference.resource_type.clone(),
                            reference.id.clone(),
                            reference.version.clone(),
                        ))
                    {
                        continue;
                    }
                    if reference.version.is_some() {
                        historical.push(reference);
                    } else {
                        current
                            .entry(reference.resource_type)
                            .or_default()
                            .push(reference.id);
                    }
                }
            }
            if current.is_empty() && historical.is_empty() {
                return Ok(resources);
            }
            if depth >= state.config.search.include_limits.max_depth {
                return Err(OperationError::InvalidParameters(
                    "$everything exceeds the configured reference depth budget".into(),
                ));
            }
            let first = resources.len();
            for (resource_type, ids) in current {
                for ids in ids.chunks(1000) {
                    match context.read_many(state, &resource_type, ids).await {
                        Ok(found) => resources.extend(found),
                        Err(error) if error.is_forbidden() => {}
                        Err(error) => return Err(error),
                    }
                    self.check_budget(state, &resources)?;
                }
            }
            for reference in historical {
                match context
                    .vread_reference(
                        state,
                        &reference.resource_type,
                        &reference.id,
                        reference.version.as_deref().unwrap(),
                    )
                    .await
                {
                    Ok(Some(resource)) => resources.push(resource),
                    Ok(None) => {}
                    Err(OperationError::Api(error))
                        if matches!(error.status_code().as_u16(), 403 | 404 | 410) => {}
                    Err(error) if error.is_forbidden() => {}
                    Err(error) => return Err(error),
                }
                self.check_budget(state, &resources)?;
            }
            frontier = first..resources.len();
            depth += 1;
        }
    }

    /// Deduplicate resources by type and id
    fn deduplicate_resources(&self, resources: Vec<StoredResource>) -> Vec<StoredResource> {
        let mut seen = HashSet::new();
        let mut deduplicated = Vec::new();

        for resource in resources {
            let key = (
                resource.resource_type.clone(),
                resource.id.clone(),
                resource.version_id.clone(),
            );
            if seen.insert(key) {
                deduplicated.push(resource);
            }
        }

        deduplicated.sort_by(|left, right| {
            (&left.resource_type, &left.id, &left.version_id).cmp(&(
                &right.resource_type,
                &right.id,
                &right.version_id,
            ))
        });
        deduplicated
    }
}

#[async_trait]
impl OperationHandler for EverythingOperation {
    fn supports_level(&self, level: OperationLevel, resource_type: Option<&str>) -> bool {
        (level == OperationLevel::Type && resource_type == Some("Patient"))
            || (level == OperationLevel::Instance
                && matches!(resource_type, Some("Patient" | "Encounter" | "Group")))
    }

    fn code(&self) -> &str {
        "everything"
    }

    async fn handle_type(
        &self,
        state: &AppState,
        context: &crate::operations::OperationContext,
        resource_type: &str,
        params: &Value,
    ) -> Result<Value, OperationError> {
        if resource_type != "Patient" {
            return Err(OperationError::NotSupported(
                "Type-level $everything is supported on Patient".into(),
            ));
        }
        let mut params = EverythingParams::from_value(params)?;
        self.prepare_params(state, &mut params)?;
        let patients =
            if let Some(patient_id) = context.auth().and_then(|auth| auth.patient.as_deref()) {
                vec![
                    context
                        .read(state, "Patient", patient_id)
                        .await?
                        .ok_or_else(|| OperationError::NotFound("Patient is unavailable".into()))?,
                ]
            } else {
                // Enumerating patients requires search; known launch identities can
                // be read with r independently. Neither route expands launch rights.
                self.search_all(
                    state,
                    context,
                    "Patient",
                    &SearchParams::new().with_count(1000),
                )
                .await?
            };
        let resources = self
            .patient_resources(state, context, patients, &params)
            .await?;
        self.build_bundle(state, resources, &params, "Patient/$everything")
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
        self.prepare_params(state, &mut everything_params)?;

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
    /// Output-only projection; traversal always uses the complete saved body.
    elements: Option<String>,
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
        if let Some(elements) = &self.elements {
            query.append_pair("_elements", elements);
        }
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
                    "_count"
                        | "_offset"
                        | "__offset"
                        | "_since"
                        | "_type"
                        | "_elements"
                        | "start"
                        | "end"
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
        if let Some(elements) = params.get("_elements") {
            let elements = elements.as_str().ok_or_else(|| {
                OperationError::InvalidParameters("_elements must contain element names".into())
            })?;
            if elements.split(',').any(|element| {
                element.is_empty()
                    || !element
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
            }) {
                return Err(OperationError::InvalidParameters(
                    "Invalid _elements element name".into(),
                ));
            }
            result.elements = Some(elements.to_owned());
        }

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
                if !matches!(value.len(), 4 | 7 | 10)
                    || !value
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || byte == b'-')
                {
                    return Err(OperationError::InvalidParameters(format!(
                        "{name} must be a FHIR date"
                    )));
                }
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
