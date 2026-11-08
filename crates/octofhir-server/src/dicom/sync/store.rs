//! Durable discovery checkpoints and coalesced per-study reconciliation state.
//!
//! # Contents
//! Immutable source identity, ordered notifications, job scheduling and fenced ACKs.
//!
//! # Invariants
//! Cursor advancement and enqueue commit together. A study never changes its pinned
//! patient automatically. Only the current async execution may acknowledge work.
//! The database contains identifiers and integration state, never DICOM payloads.

use super::super::{DicomConnection, validate_uid};
use crate::{
    async_jobs::{AsyncJobRequest, current_execution_claim},
    operations::OperationContext,
    server::AppState,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx_core::{query::query, row::Row};
use sqlx_postgres::{PgConnection, PgPool};

pub(crate) const JOB: &str = "dicom-sync";
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Change {
    Changed,
    Deleted,
    Rejected,
    Restored,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Work {
    pub connection: String,
    pub study: String,
    pub revision: i64,
    pub fingerprint: String,
}

pub(crate) fn fingerprint(config: &DicomConnection) -> String {
    let sync = config.sync.as_ref().expect("sync configuration");
    hex::encode(Sha256::digest(serde_json::to_vec(&json!({
        "wado":config.wado_url,"qido":config.qido_url,"issuers":config.patient_identifier_systems,
        "default_issuer":config.default_patient_issuer,"client_id":sync.client_id,"scopes":sync.scopes,
    })).expect("serializable source configuration")))
}

pub(crate) async fn initialize(
    pool: &PgPool,
    connection: &str,
    fingerprint: &str,
) -> Result<(), String> {
    query("INSERT INTO dicom_sync_sources(connection_name,config_fingerprint) VALUES($1,$2) ON CONFLICT DO NOTHING")
        .bind(connection).bind(fingerprint).execute(pool).await.map_err(|_|"Cannot initialize DICOM source")?;
    let row = query("SELECT config_fingerprint FROM dicom_sync_sources WHERE connection_name=$1")
        .bind(connection)
        .fetch_one(pool)
        .await
        .map_err(|_| "Cannot read DICOM source")?;
    if row.get::<String, _>("config_fingerprint") != fingerprint {
        return Err("DICOM source identity differs from persisted configuration; use a distinct connection name".into());
    }
    Ok(())
}

pub(crate) async fn enqueue(
    tx: &mut PgConnection,
    connection: &str,
    study: &str,
    instances: Option<u64>,
    sequence: Option<i64>,
    change: Change,
) -> Result<bool, String> {
    validate_uid(study).map_err(|_| "Invalid notification study UID")?;
    if sequence.is_some_and(|s| s < 0) {
        return Err("Notification sequence must be nonnegative".into());
    }
    let override_state = match change {
        Change::Deleted => Some("deleted"),
        Change::Rejected => Some("rejected"),
        _ => None,
    };
    let event = match change {
        Change::Changed => "changed",
        Change::Deleted => "deleted",
        Change::Rejected => "rejected",
        Change::Restored => "restored",
    };
    let result=query("INSERT INTO dicom_sync_studies(connection_name,study_uid,expected_instances,override_state,last_sequence) \
        VALUES($1,$2,$3,$4,COALESCE($5,-1)) ON CONFLICT(connection_name,study_uid) DO UPDATE SET \
        desired_revision=CASE WHEN dicom_sync_studies.pending_job IS NULL THEN GREATEST(dicom_sync_studies.applied_revision+1,dicom_sync_studies.desired_revision) ELSE dicom_sync_studies.desired_revision+1 END, \
        expected_instances=COALESCE(EXCLUDED.expected_instances,dicom_sync_studies.expected_instances), \
        override_state=CASE WHEN $6='restored' THEN NULL WHEN $6 IN ('deleted','rejected') THEN EXCLUDED.override_state ELSE dicom_sync_studies.override_state END, \
        last_sequence=COALESCE($5,dicom_sync_studies.last_sequence), last_seen_at=clock_timestamp(), \
        next_attempt_at=clock_timestamp(), attempts=CASE WHEN $5 IS NOT NULL THEN 0 ELSE dicom_sync_studies.attempts END \
        WHERE $5 IS NULL OR $5>dicom_sync_studies.last_sequence")
        .bind(connection).bind(study).bind(instances.map(|v|v as i64)).bind(override_state).bind(sequence).bind(event)
        .execute(tx).await.map_err(|_|"Cannot enqueue DICOM study")?;
    Ok(result.rows_affected() == 1)
}

pub(crate) async fn dispatch(
    state: &AppState,
    context: &OperationContext,
    connection: &str,
    fingerprint: &str,
) -> Result<(), String> {
    let mut tx = state
        .db_pool
        .begin()
        .await
        .map_err(|_| "Cannot schedule DICOM work")?;
    let rows=query("SELECT s.study_uid,s.desired_revision FROM dicom_sync_studies s LEFT JOIN async_jobs j ON j.id=s.pending_job \
        WHERE s.connection_name=$1 AND s.desired_revision>s.applied_revision AND s.next_attempt_at<=clock_timestamp() \
        AND (s.pending_job IS NULL OR j.status NOT IN ('queued','in_progress') OR j.expires_at<=clock_timestamp()) \
        ORDER BY s.next_attempt_at,s.study_uid LIMIT 16 FOR UPDATE OF s SKIP LOCKED")
        .bind(connection).fetch_all(&mut *tx).await.map_err(|_|"Cannot select DICOM work")?;
    for row in rows {
        let work = Work {
            connection: connection.into(),
            study: row.get("study_uid"),
            revision: row.get("desired_revision"),
            fingerprint: fingerprint.into(),
        };
        let job = state
            .async_job_manager
            .submit_job_on(
                AsyncJobRequest {
                    request_type: JOB.into(),
                    method: "POST".into(),
                    url: format!("{}/$dicom-sync", state.fhir_base_url()),
                    body: Some(serde_json::to_value(&work).map_err(|_| "Invalid DICOM work")?),
                    headers: None,
                    client_id: None,
                    authorization: context
                        .job_authorization(state)
                        .map_err(|_| "Cannot delegate DICOM work")?,
                },
                &mut tx,
            )
            .await
            .map_err(|_| "Cannot persist DICOM work")?;
        query("UPDATE dicom_sync_studies SET pending_job=$3,attempts=attempts+1 WHERE connection_name=$1 AND study_uid=$2")
            .bind(connection).bind(&work.study).bind(job).execute(&mut *tx).await.map_err(|_|"Cannot attach DICOM job")?;
    }
    tx.commit().await.map_err(|_| "Cannot commit DICOM work")?;
    Ok(())
}

pub(crate) async fn acknowledge(
    state: &AppState,
    work: &Work,
    patient: Option<&str>,
) -> Result<(), String> {
    let claim = current_execution_claim().ok_or("Missing DICOM worker claim")?;
    let updated=query("UPDATE dicom_sync_studies s SET applied_revision=GREATEST(applied_revision,$3),pending_job=NULL, \
        patient_id=COALESCE(patient_id,$4),attempts=0,last_error=NULL,last_applied_at=clock_timestamp() \
        WHERE connection_name=$1 AND study_uid=$2 AND pending_job=$5 \
        AND (patient_id IS NULL OR $4 IS NULL OR patient_id=$4) \
        AND EXISTS(SELECT 1 FROM async_jobs j WHERE j.id=$5 AND j.execution_id=$6 AND j.status='in_progress' \
            AND j.lease_expires_at>clock_timestamp() AND j.expires_at>clock_timestamp())")
        .bind(&work.connection).bind(&work.study).bind(work.revision).bind(patient).bind(claim.job_id).bind(claim.execution_id)
        .execute(state.db_pool.as_ref()).await.map_err(|_|"Cannot acknowledge DICOM work")?;
    if updated.rows_affected() == 0 {
        let applied= query("SELECT applied_revision FROM dicom_sync_studies WHERE connection_name=$1 AND study_uid=$2")
            .bind(&work.connection).bind(&work.study).fetch_optional(state.db_pool.as_ref()).await.map_err(|_|"Cannot verify DICOM acknowledgement")?;
        if !applied.is_some_and(|row| row.get::<i64, _>("applied_revision") >= work.revision) {
            return Err(
                "DICOM acknowledgement lost its execution claim or patient identity".into(),
            );
        }
    }
    Ok(())
}

pub(crate) async fn failure(state: &AppState, work: &Work, retry_seconds: u64, code: &str) {
    let Some(claim) = current_execution_claim() else {
        return;
    };
    let _=query("UPDATE dicom_sync_studies s SET last_error=$3, \
        next_attempt_at=clock_timestamp()+make_interval(secs=>LEAST(3600, $4::double precision*power(2,LEAST(GREATEST(attempts-1,0),10)))) \
        WHERE connection_name=$1 AND study_uid=$2 AND pending_job=$5 \
        AND EXISTS(SELECT 1 FROM async_jobs j WHERE j.id=$5 AND j.execution_id=$6 AND j.status='in_progress' AND j.lease_expires_at>clock_timestamp())")
        .bind(&work.connection).bind(&work.study).bind(code).bind(retry_seconds as f64).bind(claim.job_id).bind(claim.execution_id)
        .execute(state.db_pool.as_ref()).await;
}
