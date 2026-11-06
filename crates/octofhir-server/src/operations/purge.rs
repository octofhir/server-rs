//! `Patient/$purge` and `Group/$purge` (R6): removes every version of every resource in the
//! patient compartments, then the patients.
//!
//! The spec leaves the policy to the server. Here it needs an administrator, removes for
//! good, and keeps `Group` resources: a patient's membership puts a cohort in the
//! compartment, and one person's request must not take the cohort away from the others.
//! `dryRun` and a GET only count.

use std::collections::BTreeMap;

use async_trait::async_trait;
use octofhir_db_postgres::queries::crud;
use octofhir_storage::{SearchConstraint, SearchParams};
use serde_json::{Value, json};
use time::OffsetDateTime;

use super::OperationContext;
use super::expunge::{dry_run, require_administrator};
use super::handler::{OperationError, OperationHandler, OperationLevel};
use crate::server::AppState;

/// Resources removed per statement.
const BATCH: u32 = 1000;

pub struct PurgeOperation;

impl PurgeOperation {
    /// The ids of one type in these compartments, a batch at a time. Removed ids leave the
    /// search, so the next batch starts from the top again; counting walks by offset.
    async fn sweep(
        state: &AppState,
        context: &OperationContext,
        resource_type: &str,
        constraint: SearchConstraint,
        remove: bool,
    ) -> Result<u64, OperationError> {
        let mut search = SearchParams::new()
            .with_count(BATCH)
            .with_sort("_id", false);
        search.constraint = Some(constraint);
        let mut total = 0;
        loop {
            let page = context.search(state, resource_type, &search).await?;
            let ids: Vec<String> = page.entries.into_iter().map(|entry| entry.id).collect();
            total += ids.len() as u64;
            if remove && !ids.is_empty() {
                Self::expunge(state, resource_type, &ids).await?;
            } else if !remove {
                search.offset = Some(search.offset.unwrap_or(0) + ids.len() as u32);
            }
            if !page.has_more || ids.is_empty() {
                return Ok(total);
            }
        }
    }

    async fn expunge(
        state: &AppState,
        resource_type: &str,
        ids: &[String],
    ) -> Result<(), OperationError> {
        crud::expunge_many(&state.db_pool, resource_type, ids)
            .await
            .map_err(crate::handlers::map_storage_error)?;
        if let Some(cache) = &state.resource_cache {
            for id in ids {
                cache.invalidate(resource_type, id).await;
            }
        }
        Ok(())
    }

    async fn patients(
        state: &AppState,
        context: &OperationContext,
        resource_type: &str,
        id: &str,
    ) -> Result<Vec<String>, OperationError> {
        if resource_type == "Group" {
            return Ok(super::group::resolve_membership(
                state,
                context,
                id,
                OffsetDateTime::now_utc(),
            )
            .await?
            .patient_ids);
        }
        context
            .read(state, "Patient", id)
            .await?
            .ok_or_else(|| OperationError::NotFound(format!("Patient/{id} not found")))?;
        Ok(vec![id.to_owned()])
    }
}

#[async_trait]
impl OperationHandler for PurgeOperation {
    fn supports_level(&self, level: OperationLevel, resource_type: Option<&str>) -> bool {
        level == OperationLevel::Instance && matches!(resource_type, Some("Patient" | "Group"))
    }

    fn code(&self) -> &str {
        "purge"
    }

    async fn handle_instance(
        &self,
        state: &AppState,
        context: &OperationContext,
        resource_type: &str,
        id: &str,
        params: &Value,
    ) -> Result<Value, OperationError> {
        require_administrator(state, context)?;
        // A GET cannot remove anything, whatever its parameters say.
        let remove = !(dry_run(params)? || context.is_get());
        let patients = Self::patients(state, context, resource_type, id).await?;
        let definition = state
            .compartment_registry
            .get("Patient")
            .map_err(|error| OperationError::Internal(error.to_string()))?;

        let mut removed = BTreeMap::new();
        if !patients.is_empty() {
            for compartment_type in definition.resource_types() {
                if matches!(compartment_type, "Group" | "Patient")
                    || !state.resource_type_set.load().contains(compartment_type)
                {
                    continue;
                }
                let constraint = SearchConstraint::LinkedPatients {
                    patient_ids: patients.clone(),
                    fhir_base_url: state.fhir_base_url(),
                    parameters: definition
                        .get_inclusion_params(compartment_type)
                        .unwrap_or_default()
                        .to_vec(),
                };
                let count =
                    Self::sweep(state, context, compartment_type, constraint, remove).await?;
                if count > 0 {
                    removed.insert(compartment_type.to_owned(), count);
                }
            }
            // Last: the compartments are found through the patients, and a patient linked to
            // these ones is in the compartment too.
            let constraint = SearchConstraint::LinkedPatients {
                patient_ids: patients,
                fhir_base_url: state.fhir_base_url(),
                parameters: Vec::new(),
            };
            let count = Self::sweep(state, context, "Patient", constraint, remove).await?;
            removed.insert("Patient".to_owned(), count);
        }

        let summary = removed
            .iter()
            .map(|(name, count)| format!("{name}: {count}"))
            .collect::<Vec<_>>()
            .join(", ");
        if remove {
            tracing::warn!(resource_type, id, %summary, "Patient compartments purged");
        }
        let verb = if remove { "Removed" } else { "Would remove" };
        Ok(json!({"resourceType": "OperationOutcome", "issue": [{
            "severity": "information",
            "code": "success",
            "diagnostics": format!("{verb} {summary}"),
        }]}))
    }
}
