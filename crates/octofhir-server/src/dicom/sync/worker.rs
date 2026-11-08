//! Reconcile one revision under the existing durable async-job fence.
//!
//! # Contents
//! Replay-safe FHIR writes, patient pinning, explicit availability and retry state.
//!
//! # Invariants
//! A committed checkpoint is never replayed as another write. Fresh metadata is
//! re-read for changed studies. Failures preserve previous FHIR data and stay queued.

use super::store::{self, Work};
use crate::{
    async_jobs::{CheckpointStep, current_execution_claim},
    operations::OperationContext,
    server::AppState,
};
use serde_json::{Value, json};
use sqlx_core::{query::query, row::Row};

pub(crate) async fn execute(state: AppState, body: Value) -> Result<Value, String> {
    let work: Work =
        serde_json::from_value(body.clone()).map_err(|_| "Invalid DICOM work request")?;
    let config = state
        .config
        .dicom
        .connections
        .get(&work.connection)
        .ok_or("DICOM source is no longer configured")?;
    let sync = config
        .sync
        .as_ref()
        .ok_or("DICOM synchronization is disabled")?;
    if store::fingerprint(config) != work.fingerprint {
        return Err("DICOM source configuration changed".into());
    }
    let retry = sync.retry_seconds;
    let result = run(&state, &work, &body).await;
    if result.is_err() {
        store::failure(&state, &work, retry, "reconciliation_failed").await;
    }
    result
}

async fn run(state: &AppState, work: &Work, body: &Value) -> Result<Value, String> {
    let claim = current_execution_claim().ok_or("DICOM worker has no execution claim")?;
    let context = OperationContext::for_job(state, claim.job_id)
        .await
        .map_err(|_| "DICOM service delegation is unavailable")?;
    let config = &state.config.dicom.connections[&work.connection];
    let sync = config
        .sync
        .as_ref()
        .ok_or("DICOM synchronization is disabled")?;
    let auth = context
        .auth()
        .ok_or("DICOM background work requires a backend identity")?;
    if auth.client.client_id != sync.client_id
        || auth
            .scopes()
            .any(|scope| !sync.scopes.iter().any(|allowed| allowed == scope))
    {
        return Err("DICOM worker delegation no longer matches configured service identity".into());
    }
    let step = CheckpointStep::new("dicom-study".into(), body)
        .map_err(|_| "Cannot initialize DICOM checkpoint")?;
    if let Some(record) = state
        .async_job_manager
        .restore_checkpoint(&step)
        .await
        .map_err(|_| "Cannot restore DICOM checkpoint")?
    {
        store::acknowledge(state, work, record["metadata"]["patient_id"].as_str()).await?;
        return Ok(json!({"study":work.study,"recovered":true}));
    }
    let row=query("SELECT patient_id,expected_instances,override_state FROM dicom_sync_studies WHERE connection_name=$1 AND study_uid=$2 AND pending_job=$3")
        .bind(&work.connection).bind(&work.study).bind(claim.job_id).fetch_optional(state.db_pool.as_ref()).await.map_err(|_|"Cannot read DICOM work state")?.ok_or("DICOM work was superseded")?;
    let patient: Option<String> = row.get("patient_id");
    let expected: Option<i64> = row.get("expected_instances");
    let availability: Option<String> = row.get("override_state");
    let patient = if let Some(availability) = availability {
        super::super::import::unavailable(
            state,
            &context,
            &work.connection,
            &work.study,
            &availability,
            step,
        )
        .await
        .map_err(|_| "Cannot apply explicit DICOM availability")?
    } else {
        let imported = super::super::import::import(
            state,
            &context,
            &work.connection,
            &work.study,
            patient.as_deref(),
            expected.map(|v| v as u64),
            Some(step),
        )
        .await
        .map_err(
            |_| "DICOM metadata, patient identity or FHIR authorization requires reconciliation",
        )?;
        Some(imported.patient_id)
    };
    store::acknowledge(state, work, patient.as_deref()).await?;
    Ok(json!({"study":work.study,"reconciled":true}))
}
