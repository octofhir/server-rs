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
use crate::mapping::json_from_envelope;
use crate::server::AppState;
use octofhir_api::bundle_from_search;
use octofhir_core::{FhirDateTime, ResourceEnvelope, ResourceType};
use octofhir_storage::legacy::query::{QueryFilter, SearchQuery};

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
            .get(&ResourceType::Patient, patient_id)
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
                let type_str = resource_type.to_string();
                if !types.contains(&type_str) {
                    continue;
                }
            }

            // Build search query
            // Using Contains filter for reference parameters since Token expects system|code format
            let mut filters = vec![QueryFilter::Contains {
                field: param_name.to_string(),
                value: reference.clone(),
            }];

            // Apply _since filter if specified
            if let Some(since) = params.since {
                filters.push(QueryFilter::DateRange {
                    field: "_lastUpdated".to_string(),
                    start: Some(FhirDateTime::new(since)),
                    end: None,
                });
            }

            // Apply date range filters (start/end) if specified
            if let Some(start) = params.start {
                filters.push(QueryFilter::DateRange {
                    field: "date".to_string(),
                    start: Some(FhirDateTime::new(start)),
                    end: params.end.map(FhirDateTime::new),
                });
            }

            let query = SearchQuery {
                resource_type: resource_type.clone(),
                filters,
                offset: 0,
                count: 1000, // Large limit to fetch all related resources
                sort_field: None,
                sort_ascending: true,
            };

            match state.storage.search(&query).await {
                Ok(result) => {
                    resources.extend(result.resources);
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
        let resources_json: Vec<Value> = paginated.iter().map(json_from_envelope).collect();
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
            .get(&ResourceType::Encounter, encounter_id)
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
                let type_str = resource_type.to_string();
                if !types.contains(&type_str) {
                    continue;
                }
            }

            let mut filters = vec![QueryFilter::Contains {
                field: param_name.to_string(),
                value: reference.clone(),
            }];

            if let Some(since) = params.since {
                filters.push(QueryFilter::DateRange {
                    field: "_lastUpdated".to_string(),
                    start: Some(FhirDateTime::new(since)),
                    end: None,
                });
            }

            let query = SearchQuery {
                resource_type: resource_type.clone(),
                filters,
                offset: 0,
                count: 1000,
                sort_field: None,
                sort_ascending: true,
            };

            match state.storage.search(&query).await {
                Ok(result) => {
                    resources.extend(result.resources);
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

        let resources_json: Vec<Value> = paginated.iter().map(json_from_envelope).collect();
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
            .get(&ResourceType::Custom("Group".to_string()), group_id)
            .await
            .map_err(|e| OperationError::Internal(e.to_string()))?
            .ok_or_else(|| OperationError::NotFound(format!("Group/{} not found", group_id)))?;

        // 2. Extract member references from Group.member[]
        let members = self.extract_group_members(&group)?;

        if members.is_empty() {
            // Return just the group resource if no members
            let resources_json: Vec<Value> = vec![json_from_envelope(&group)];
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
                                // Try to convert back to ResourceEnvelope
                                match serde_json::from_value::<ResourceEnvelope>(resource.clone()) {
                                    Ok(envelope) => all_resources.push(envelope),
                                    Err(e) => {
                                        tracing::warn!("Failed to parse resource envelope: {}", e)
                                    }
                                }
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

        let resources_json: Vec<Value> = paginated.iter().map(json_from_envelope).collect();
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
    fn get_patient_compartment_searches(
        &self,
        patient_id: &str,
    ) -> Vec<(ResourceType, String, String)> {
        let reference = format!("Patient/{}", patient_id);
        vec![
            // Core clinical resources
            (
                ResourceType::Observation,
                "patient".to_string(),
                reference.clone(),
            ),
            (
                ResourceType::Observation,
                "subject".to_string(),
                reference.clone(),
            ),
            (
                ResourceType::Condition,
                "patient".to_string(),
                reference.clone(),
            ),
            (
                ResourceType::Condition,
                "subject".to_string(),
                reference.clone(),
            ),
            (
                ResourceType::Custom("AllergyIntolerance".to_string()),
                "patient".to_string(),
                reference.clone(),
            ),
            (
                ResourceType::MedicationRequest,
                "patient".to_string(),
                reference.clone(),
            ),
            (
                ResourceType::MedicationRequest,
                "subject".to_string(),
                reference.clone(),
            ),
            (
                ResourceType::Custom("MedicationStatement".to_string()),
                "patient".to_string(),
                reference.clone(),
            ),
            (
                ResourceType::Custom("MedicationStatement".to_string()),
                "subject".to_string(),
                reference.clone(),
            ),
            (
                ResourceType::Custom("MedicationAdministration".to_string()),
                "patient".to_string(),
                reference.clone(),
            ),
            (
                ResourceType::Custom("MedicationAdministration".to_string()),
                "subject".to_string(),
                reference.clone(),
            ),
            (
                ResourceType::Procedure,
                "patient".to_string(),
                reference.clone(),
            ),
            (
                ResourceType::Procedure,
                "subject".to_string(),
                reference.clone(),
            ),
            (
                ResourceType::Custom("Immunization".to_string()),
                "patient".to_string(),
                reference.clone(),
            ),
            (
                ResourceType::DiagnosticReport,
                "patient".to_string(),
                reference.clone(),
            ),
            (
                ResourceType::DiagnosticReport,
                "subject".to_string(),
                reference.clone(),
            ),
            (
                ResourceType::Encounter,
                "patient".to_string(),
                reference.clone(),
            ),
            (
                ResourceType::Encounter,
                "subject".to_string(),
                reference.clone(),
            ),
            (
                ResourceType::Custom("CarePlan".to_string()),
                "patient".to_string(),
                reference.clone(),
            ),
            (
                ResourceType::Custom("CarePlan".to_string()),
                "subject".to_string(),
                reference.clone(),
            ),
            (
                ResourceType::Custom("Goal".to_string()),
                "patient".to_string(),
                reference.clone(),
            ),
            (
                ResourceType::Custom("Goal".to_string()),
                "subject".to_string(),
                reference.clone(),
            ),
            (
                ResourceType::Specimen,
                "patient".to_string(),
                reference.clone(),
            ),
            (
                ResourceType::Specimen,
                "subject".to_string(),
                reference.clone(),
            ),
            // Documents and communications
            (
                ResourceType::DocumentReference,
                "patient".to_string(),
                reference.clone(),
            ),
            (
                ResourceType::DocumentReference,
                "subject".to_string(),
                reference.clone(),
            ),
            (
                ResourceType::Custom("Communication".to_string()),
                "patient".to_string(),
                reference.clone(),
            ),
            (
                ResourceType::Custom("Communication".to_string()),
                "subject".to_string(),
                reference.clone(),
            ),
            // Financial resources
            (
                ResourceType::Custom("Claim".to_string()),
                "patient".to_string(),
                reference.clone(),
            ),
            (
                ResourceType::Custom("ExplanationOfBenefit".to_string()),
                "patient".to_string(),
                reference.clone(),
            ),
            (
                ResourceType::Custom("Coverage".to_string()),
                "patient".to_string(),
                reference.clone(),
            ),
            (
                ResourceType::Custom("Coverage".to_string()),
                "beneficiary".to_string(),
                reference.clone(),
            ),
        ]
    }

    /// Get Encounter compartment search parameters
    fn get_encounter_compartment_searches(
        &self,
        encounter_id: &str,
    ) -> Vec<(ResourceType, String, String)> {
        let reference = format!("Encounter/{}", encounter_id);
        vec![
            (
                ResourceType::Observation,
                "encounter".to_string(),
                reference.clone(),
            ),
            (
                ResourceType::Condition,
                "encounter".to_string(),
                reference.clone(),
            ),
            (
                ResourceType::Procedure,
                "encounter".to_string(),
                reference.clone(),
            ),
            (
                ResourceType::MedicationRequest,
                "encounter".to_string(),
                reference.clone(),
            ),
            (
                ResourceType::Custom("MedicationStatement".to_string()),
                "context".to_string(),
                reference.clone(),
            ),
            (
                ResourceType::Custom("MedicationAdministration".to_string()),
                "encounter".to_string(),
                reference.clone(),
            ),
            (
                ResourceType::DiagnosticReport,
                "encounter".to_string(),
                reference.clone(),
            ),
            (
                ResourceType::Custom("ServiceRequest".to_string()),
                "encounter".to_string(),
                reference.clone(),
            ),
            (
                ResourceType::Custom("Communication".to_string()),
                "encounter".to_string(),
                reference.clone(),
            ),
            (
                ResourceType::DocumentReference,
                "encounter".to_string(),
                reference.clone(),
            ),
        ]
    }

    /// Extract patient member IDs from a Group resource
    fn extract_group_members(
        &self,
        group: &ResourceEnvelope,
    ) -> Result<Vec<String>, OperationError> {
        let mut member_ids = Vec::new();

        if let Some(members) = group.data.get("member").and_then(|m| m.as_array()) {
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
    fn deduplicate_resources(&self, resources: Vec<ResourceEnvelope>) -> Vec<ResourceEnvelope> {
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
