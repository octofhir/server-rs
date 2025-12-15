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
        patient_id: &str,
        params: &EverythingParams,
    ) -> Result<Value, OperationError> {
        // 1. Verify patient exists and fetch it
        let patient = state
            .storage
            .read("Patient", patient_id)
            .await
            .map_err(|e| OperationError::Internal(e.to_string()))?
            .ok_or_else(|| OperationError::NotFound(format!("Patient/{} not found", patient_id)))?;

        let mut resources = vec![patient.clone()];

        // 2. Query related resources using reference parameters
        // Based on FHIR Patient compartment definition
        let compartment_searches = self.get_patient_compartment_searches(patient_id);

        for (resource_type, param_name, reference) in compartment_searches {
            // Skip if _type filter is specified and doesn't include this type
            if let Some(ref types) = params.type_filter {
                if !types.contains(&resource_type) {
                    continue;
                }
            }

            // Build search query using modern FhirStorage
            let mut search_params = SearchParams::new()
                .with_count(1000)
                .with_param(&param_name, &reference);

            // Apply _since filter if specified
            if let Some(since) = params.since {
                let since_str = since
                    .format(&time::format_description::well_known::Rfc3339)
                    .unwrap_or_default();
                search_params =
                    search_params.with_param("_lastUpdated", &format!("ge{}", since_str));
            }

            // Apply date range filters (start/end) if specified
            if let Some(start) = params.start {
                let start_str = start
                    .format(&time::format_description::well_known::Rfc3339)
                    .unwrap_or_default();
                search_params = search_params.with_param("date", &format!("ge{}", start_str));
            }
            if let Some(end) = params.end {
                let end_str = end
                    .format(&time::format_description::well_known::Rfc3339)
                    .unwrap_or_default();
                search_params = search_params.with_param("date", &format!("le{}", end_str));
            }

            match state.storage.search(&resource_type, &search_params).await {
                Ok(result) => {
                    resources.extend(result.entries);
                }
                Err(e) => {
                    tracing::warn!(
                        "Failed to search {} for patient {}: {}",
                        resource_type,
                        patient_id,
                        e
                    );
                }
            }
        }

        // 3. Deduplicate resources by id
        let deduplicated = self.deduplicate_resources(resources);

        // 4. Apply pagination if _count is specified
        let (paginated, total) = if let Some(count) = params.count {
            let offset = params.offset.unwrap_or(0);
            let total = deduplicated.len();
            let end = std::cmp::min(offset + count, total);
            (deduplicated[offset..end].to_vec(), total)
        } else {
            let total = deduplicated.len();
            (deduplicated, total)
        };

        // 5. Build searchset Bundle
        let resources_json: Vec<Value> = paginated.iter().map(|r| r.resource.clone()).collect();
        let bundle = bundle_from_search(
            total,
            resources_json,
            &state.base_url,
            &format!("Patient/{}/$everything", patient_id),
            params.offset.unwrap_or(0),
            params.count.unwrap_or(50),
            None, // No query suffix for operation
        );

        serde_json::to_value(bundle)
            .map_err(|e| OperationError::Internal(format!("Failed to serialize bundle: {}", e)))
    }

    /// Encounter $everything: Retrieve all resources related to an encounter
    async fn encounter_everything(
        &self,
        state: &AppState,
        encounter_id: &str,
        params: &EverythingParams,
    ) -> Result<Value, OperationError> {
        // 1. Verify encounter exists and fetch it
        let encounter = state
            .storage
            .read("Encounter", encounter_id)
            .await
            .map_err(|e| OperationError::Internal(e.to_string()))?
            .ok_or_else(|| {
                OperationError::NotFound(format!("Encounter/{} not found", encounter_id))
            })?;

        let mut resources = vec![encounter.clone()];

        // 2. Query related resources using encounter reference
        let compartment_searches = self.get_encounter_compartment_searches(encounter_id);

        for (resource_type, param_name, reference) in compartment_searches {
            if let Some(ref types) = params.type_filter {
                if !types.contains(&resource_type) {
                    continue;
                }
            }

            let mut search_params = SearchParams::new()
                .with_count(1000)
                .with_param(&param_name, &reference);

            if let Some(since) = params.since {
                let since_str = since
                    .format(&time::format_description::well_known::Rfc3339)
                    .unwrap_or_default();
                search_params =
                    search_params.with_param("_lastUpdated", &format!("ge{}", since_str));
            }

            match state.storage.search(&resource_type, &search_params).await {
                Ok(result) => {
                    resources.extend(result.entries);
                }
                Err(e) => {
                    tracing::warn!(
                        "Failed to search {} for encounter {}: {}",
                        resource_type,
                        encounter_id,
                        e
                    );
                }
            }
        }

        let deduplicated = self.deduplicate_resources(resources);

        let (paginated, total) = if let Some(count) = params.count {
            let offset = params.offset.unwrap_or(0);
            let total = deduplicated.len();
            let end = std::cmp::min(offset + count, total);
            (deduplicated[offset..end].to_vec(), total)
        } else {
            let total = deduplicated.len();
            (deduplicated, total)
        };

        let resources_json: Vec<Value> = paginated.iter().map(|r| r.resource.clone()).collect();
        let bundle = bundle_from_search(
            total,
            resources_json,
            &state.base_url,
            &format!("Encounter/{}/$everything", encounter_id),
            params.offset.unwrap_or(0),
            params.count.unwrap_or(50),
            None,
        );

        serde_json::to_value(bundle)
            .map_err(|e| OperationError::Internal(format!("Failed to serialize bundle: {}", e)))
    }

    /// Group $everything: Retrieve resources for all group members
    async fn group_everything(
        &self,
        state: &AppState,
        group_id: &str,
        params: &EverythingParams,
    ) -> Result<Value, OperationError> {
        // 1. Verify group exists and fetch it
        let group = state
            .storage
            .read("Group", group_id)
            .await
            .map_err(|e| OperationError::Internal(e.to_string()))?
            .ok_or_else(|| OperationError::NotFound(format!("Group/{} not found", group_id)))?;

        // 2. Extract member references from Group.member[]
        let members = self.extract_group_members(&group)?;

        if members.is_empty() {
            // Return just the group resource if no members
            let resources_json: Vec<Value> = vec![group.resource.clone()];
            let bundle = bundle_from_search(
                1,
                resources_json,
                &state.base_url,
                &format!("Group/{}/$everything", group_id),
                0,
                50,
                None,
            );

            return serde_json::to_value(bundle).map_err(|e| {
                OperationError::Internal(format!("Failed to serialize bundle: {}", e))
            });
        }

        // 3. For each member, run Patient $everything
        let mut all_resources = vec![group];

        for member_id in members {
            match self.patient_everything(state, &member_id, params).await {
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
                    tracing::warn!(
                        "Failed to fetch $everything for patient {}: {}",
                        member_id,
                        e
                    );
                }
            }
        }

        // 4. Deduplicate resources
        let deduplicated = self.deduplicate_resources(all_resources);

        // 5. Apply pagination
        let (paginated, total) = if let Some(count) = params.count {
            let offset = params.offset.unwrap_or(0);
            let total = deduplicated.len();
            let end = std::cmp::min(offset + count, total);
            (deduplicated[offset..end].to_vec(), total)
        } else {
            let total = deduplicated.len();
            (deduplicated, total)
        };

        let resources_json: Vec<Value> = paginated.iter().map(|r| r.resource.clone()).collect();
        let bundle = bundle_from_search(
            total,
            resources_json,
            &state.base_url,
            &format!("Group/{}/$everything", group_id),
            params.offset.unwrap_or(0),
            params.count.unwrap_or(50),
            None,
        );

        serde_json::to_value(bundle)
            .map_err(|e| OperationError::Internal(format!("Failed to serialize bundle: {}", e)))
    }

    /// Get Patient compartment search parameters
    /// Returns (resource_type, param_name, reference_value) tuples
    fn get_patient_compartment_searches(&self, patient_id: &str) -> Vec<(String, String, String)> {
        let reference = format!("Patient/{}", patient_id);
        vec![
            // Core clinical resources
            (
                "Observation".to_string(),
                "patient".to_string(),
                reference.clone(),
            ),
            (
                "Observation".to_string(),
                "subject".to_string(),
                reference.clone(),
            ),
            (
                "Condition".to_string(),
                "patient".to_string(),
                reference.clone(),
            ),
            (
                "Condition".to_string(),
                "subject".to_string(),
                reference.clone(),
            ),
            (
                "AllergyIntolerance".to_string(),
                "patient".to_string(),
                reference.clone(),
            ),
            (
                "MedicationRequest".to_string(),
                "patient".to_string(),
                reference.clone(),
            ),
            (
                "MedicationRequest".to_string(),
                "subject".to_string(),
                reference.clone(),
            ),
            (
                "MedicationStatement".to_string(),
                "patient".to_string(),
                reference.clone(),
            ),
            (
                "MedicationStatement".to_string(),
                "subject".to_string(),
                reference.clone(),
            ),
            (
                "MedicationAdministration".to_string(),
                "patient".to_string(),
                reference.clone(),
            ),
            (
                "MedicationAdministration".to_string(),
                "subject".to_string(),
                reference.clone(),
            ),
            (
                "Procedure".to_string(),
                "patient".to_string(),
                reference.clone(),
            ),
            (
                "Procedure".to_string(),
                "subject".to_string(),
                reference.clone(),
            ),
            (
                "Immunization".to_string(),
                "patient".to_string(),
                reference.clone(),
            ),
            (
                "DiagnosticReport".to_string(),
                "patient".to_string(),
                reference.clone(),
            ),
            (
                "DiagnosticReport".to_string(),
                "subject".to_string(),
                reference.clone(),
            ),
            (
                "Encounter".to_string(),
                "patient".to_string(),
                reference.clone(),
            ),
            (
                "Encounter".to_string(),
                "subject".to_string(),
                reference.clone(),
            ),
            (
                "CarePlan".to_string(),
                "patient".to_string(),
                reference.clone(),
            ),
            (
                "CarePlan".to_string(),
                "subject".to_string(),
                reference.clone(),
            ),
            ("Goal".to_string(), "patient".to_string(), reference.clone()),
            ("Goal".to_string(), "subject".to_string(), reference.clone()),
            (
                "Specimen".to_string(),
                "patient".to_string(),
                reference.clone(),
            ),
            (
                "Specimen".to_string(),
                "subject".to_string(),
                reference.clone(),
            ),
            // Documents and communications
            (
                "DocumentReference".to_string(),
                "patient".to_string(),
                reference.clone(),
            ),
            (
                "DocumentReference".to_string(),
                "subject".to_string(),
                reference.clone(),
            ),
            (
                "Communication".to_string(),
                "patient".to_string(),
                reference.clone(),
            ),
            (
                "Communication".to_string(),
                "subject".to_string(),
                reference.clone(),
            ),
            // Financial resources
            (
                "Claim".to_string(),
                "patient".to_string(),
                reference.clone(),
            ),
            (
                "ExplanationOfBenefit".to_string(),
                "patient".to_string(),
                reference.clone(),
            ),
            (
                "Coverage".to_string(),
                "patient".to_string(),
                reference.clone(),
            ),
            (
                "Coverage".to_string(),
                "beneficiary".to_string(),
                reference.clone(),
            ),
        ]
    }

    /// Get Encounter compartment search parameters
    fn get_encounter_compartment_searches(
        &self,
        encounter_id: &str,
    ) -> Vec<(String, String, String)> {
        let reference = format!("Encounter/{}", encounter_id);
        vec![
            (
                "Observation".to_string(),
                "encounter".to_string(),
                reference.clone(),
            ),
            (
                "Condition".to_string(),
                "encounter".to_string(),
                reference.clone(),
            ),
            (
                "Procedure".to_string(),
                "encounter".to_string(),
                reference.clone(),
            ),
            (
                "MedicationRequest".to_string(),
                "encounter".to_string(),
                reference.clone(),
            ),
            (
                "MedicationStatement".to_string(),
                "context".to_string(),
                reference.clone(),
            ),
            (
                "MedicationAdministration".to_string(),
                "encounter".to_string(),
                reference.clone(),
            ),
            (
                "DiagnosticReport".to_string(),
                "encounter".to_string(),
                reference.clone(),
            ),
            (
                "ServiceRequest".to_string(),
                "encounter".to_string(),
                reference.clone(),
            ),
            (
                "Communication".to_string(),
                "encounter".to_string(),
                reference.clone(),
            ),
            (
                "DocumentReference".to_string(),
                "encounter".to_string(),
                reference.clone(),
            ),
        ]
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
        resource_type: &str,
        id: &str,
        params: &Value,
    ) -> Result<Value, OperationError> {
        // Parse parameters
        let everything_params = EverythingParams::from_value(params)?;

        match resource_type {
            "Patient" => self.patient_everything(state, id, &everything_params).await,
            "Encounter" => {
                self.encounter_everything(state, id, &everything_params)
                    .await
            }
            "Group" => self.group_everything(state, id, &everything_params).await,
            _ => Err(OperationError::NotSupported(format!(
                "$everything operation is not supported for resource type {}",
                resource_type
            ))),
        }
    }
}

/// Parameters for the $everything operation
#[derive(Debug, Default)]
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
    start: Option<OffsetDateTime>,
    /// Date range end (Patient $everything only)
    end: Option<OffsetDateTime>,
}

impl EverythingParams {
    fn from_value(params: &Value) -> Result<Self, OperationError> {
        let mut result = EverythingParams::default();

        // Parse _count parameter
        if let Some(count) = params.get("_count") {
            result.count = count
                .as_u64()
                .map(|n| n as usize)
                .or_else(|| count.as_str().and_then(|s| s.parse().ok()));
        }

        // Parse __offset parameter (internal pagination)
        if let Some(offset) = params.get("__offset") {
            result.offset = offset
                .as_u64()
                .map(|n| n as usize)
                .or_else(|| offset.as_str().and_then(|s| s.parse().ok()));
        }

        // Parse _since parameter
        if let Some(since) = params.get("_since").and_then(|v| v.as_str()) {
            result.since = OffsetDateTime::parse(since, &Rfc3339).ok().or_else(|| {
                // Try parsing as date only (without time)
                time::Date::parse(
                    since,
                    &time::format_description::parse("[year]-[month]-[day]").unwrap(),
                )
                .ok()
                .map(|d| d.midnight().assume_utc())
            });
        }

        // Parse _type parameter (comma-separated list)
        if let Some(type_param) = params.get("_type").and_then(|v| v.as_str()) {
            let types: HashSet<String> = type_param
                .split(',')
                .map(|s| s.trim().to_string())
                .collect();
            result.type_filter = Some(types);
        }

        // Parse start parameter
        if let Some(start) = params.get("start").and_then(|v| v.as_str()) {
            result.start = OffsetDateTime::parse(start, &Rfc3339).ok().or_else(|| {
                time::Date::parse(
                    start,
                    &time::format_description::parse("[year]-[month]-[day]").unwrap(),
                )
                .ok()
                .map(|d| d.midnight().assume_utc())
            });
        }

        // Parse end parameter
        if let Some(end) = params.get("end").and_then(|v| v.as_str()) {
            result.end = OffsetDateTime::parse(end, &Rfc3339).ok().or_else(|| {
                time::Date::parse(
                    end,
                    &time::format_description::parse("[year]-[month]-[day]").unwrap(),
                )
                .ok()
                .map(|d| d.midnight().assume_utc())
            });
        }

        Ok(result)
    }
}
