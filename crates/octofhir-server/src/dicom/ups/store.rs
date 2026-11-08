//! Durable UPS identities and coalesced commands on the existing async executor.
//!
//! # Invariants
//! A Task has one immutable source/workitem/request binding. Job creation and queue
//! attachment share a transaction. Only the live execution claim acknowledges a
//! revision; typed request metadata never includes DICOM datasets or credentials.
use super::{config::DicomUpsConfig, model::Request};
use crate::{
    async_jobs::{AsyncJobRequest, current_execution_claim},
    operations::OperationContext,
    server::AppState,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx_core::{query::query, row::Row};
use sqlx_postgres::PgPool;
pub(crate) const JOB: &str = "dicom-ups";
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Work {
    pub connection: String,
    pub task: String,
    pub revision: i64,
    pub fingerprint: String,
}
pub(super) struct Binding {
    pub uid: String,
    pub request: Request,
    pub requested_by: String,
    pub state: Option<String>,
    pub confirmed: bool,
    pub creation_started: bool,
    pub cancel: bool,
    pub cancel_delivered: bool,
}
pub(super) fn fingerprint(connection: &super::super::DicomConnection) -> String {
    let config = connection.ups.as_ref().expect("UPS source");
    let mut value = json!({"url":config.url,"wado":connection.wado_url,"issuers":connection.patient_identifier_systems,"default_issuer":connection.default_patient_issuer,"client_id":config.client_id,"scopes":config.scopes,"ae_title":config.ae_title,"notifications":config.notifications});
    value.sort_all_objects();
    hex::encode(Sha256::digest(
        serde_json::to_vec(&value).expect("UPS source identity"),
    ))
}
pub(super) async fn initialize(
    pool: &PgPool,
    connection: &str,
    fingerprint: &str,
) -> Result<(), String> {
    query("INSERT INTO dicom_ups_sources(connection_name,config_fingerprint) VALUES($1,$2) ON CONFLICT DO NOTHING").bind(connection).bind(fingerprint).execute(pool).await.map_err(|_|"Cannot initialize UPS source")?;
    let row = query("SELECT config_fingerprint FROM dicom_ups_sources WHERE connection_name=$1")
        .bind(connection)
        .fetch_one(pool)
        .await
        .map_err(|_| "Cannot read UPS source identity")?;
    if row.get::<String, _>("config_fingerprint") != fingerprint {
        return Err("UPS source identity changed; use a distinct DICOM connection name".into());
    }
    Ok(())
}
/// Duplicate submissions preserve the first schedule time and workitem UID.
pub(super) async fn enqueue(
    state: &AppState,
    connection: &str,
    task: &str,
    action: &str,
    request: Option<Request>,
    actor: &str,
    expected_hash: &str,
) -> Result<(String, bool), String> {
    let mut tx = state
        .db_pool
        .begin()
        .await
        .map_err(|_| "Cannot begin UPS command")?;
    // Serialize binding creation with ordinary FHIR writes; the row trigger
    // then enforces the immutable input projection for every later write path.
    let current: Option<Value> = sqlx_core::query_scalar::query_scalar(
        "SELECT octofhir_dicom_task_projection(resource) FROM task WHERE id=$1 FOR UPDATE",
    )
    .bind(task)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| "Cannot lock UPS Task inputs")?;
    let current = current.ok_or("UPS Task is unavailable")?;
    if super::model::task_hash(&current) != expected_hash {
        return Err("Task inputs changed; retry UPS submission".into());
    }
    let mut fresh = false;
    if let Some(mut request) = request {
        let prefix: String = sqlx_core::query_scalar::query_scalar(
            "SELECT subscription_prefix FROM dicom_ups_sources WHERE connection_name=$1",
        )
        .bind(connection)
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| "Cannot read UPS subscription namespace")?;
        request.label = format!("{prefix}{}", &request.task_hash[..24]);
        let uid = format!("2.25.{}", uuid::Uuid::new_v4().as_u128());
        let metadata =
            serde_json::to_value(request).map_err(|_| "Cannot encode UPS request metadata")?;
        fresh=query("INSERT INTO dicom_ups_tasks(task_id,connection_name,workitem_uid,request_metadata,requested_by) VALUES($1,$2,$3,$4,$5) ON CONFLICT(task_id) DO NOTHING").bind(task).bind(connection).bind(uid).bind(metadata).bind(actor).execute(&mut *tx).await.map_err(|_|"Cannot persist UPS request")?.rows_affected()==1;
    }
    let row=query("SELECT connection_name,workitem_uid,cancel_requested,request_metadata,desired_revision,applied_revision,pending_job,cancelled_before_submission FROM dicom_ups_tasks WHERE task_id=$1 FOR UPDATE").bind(task).fetch_optional(&mut *tx).await.map_err(|_|"Cannot lock UPS command")?.ok_or("Task has no UPS workitem")?;
    if row.get::<String, _>("connection_name") != connection {
        return Err("Task is already bound to a different UPS connection".into());
    }
    let existing: Request = serde_json::from_value(row.get("request_metadata"))
        .map_err(|_| "Invalid persisted UPS request")?;
    if existing.task_hash != expected_hash {
        return Err("Task inputs differ from its immutable UPS binding; use a new Task".into());
    }
    let already_queued = row.get::<i64, _>("desired_revision")
        > row.get::<i64, _>("applied_revision")
        || row.get::<Option<uuid::Uuid>, _>("pending_job").is_some();
    if row.get::<bool, _>("cancelled_before_submission") {
        if action == "refresh" {
            return Err("Task was cancelled before submission; use a new Task".into());
        }
        let uid = row.get("workitem_uid");
        tx.commit()
            .await
            .map_err(|_| "Cannot acknowledge prior UPS cancellation")?;
        return Ok((uid, false));
    }
    let changed =
        action == "refresh" || (action == "cancel" && !row.get::<bool, _>("cancel_requested"));
    if changed {
        query("UPDATE dicom_ups_tasks SET cancel_requested=cancel_requested OR $2,desired_revision=CASE WHEN pending_job IS NULL THEN GREATEST(applied_revision+1,desired_revision) ELSE desired_revision+1 END,next_attempt_at=clock_timestamp(),attempts=0 WHERE task_id=$1").bind(task).bind(action=="cancel").execute(&mut *tx).await.map_err(|_|"Cannot enqueue UPS command")?;
    }
    let uid = row.get("workitem_uid");
    tx.commit().await.map_err(|_| "Cannot commit UPS command")?;
    Ok((uid, fresh || changed || already_queued))
}
pub(super) async fn dispatch(
    state: &AppState,
    context: &OperationContext,
    connection: &str,
    fingerprint: &str,
) -> Result<(), String> {
    let mut tx = state
        .db_pool
        .begin()
        .await
        .map_err(|_| "Cannot schedule UPS reconciliation")?;
    let rows=query("SELECT t.task_id,GREATEST(t.desired_revision,t.applied_revision+1) AS revision FROM dicom_ups_tasks t LEFT JOIN async_jobs j ON j.id=t.pending_job WHERE t.connection_name=$1 AND NOT t.cancelled_before_submission AND t.next_attempt_at<=clock_timestamp() AND (t.desired_revision>t.applied_revision OR t.remote_state IS NULL OR t.remote_state NOT IN ('COMPLETED','CANCELED')) AND (t.pending_job IS NULL OR j.status NOT IN ('queued','in_progress') OR j.expires_at<=clock_timestamp()) ORDER BY t.next_attempt_at,t.task_id LIMIT 16 FOR UPDATE OF t SKIP LOCKED").bind(connection).fetch_all(&mut *tx).await.map_err(|_|"Cannot select UPS reconciliation")?;
    for row in rows {
        let work = Work {
            connection: connection.into(),
            task: row.get("task_id"),
            revision: row.get("revision"),
            fingerprint: fingerprint.into(),
        };
        let job = state
            .async_job_manager
            .submit_job_on(
                AsyncJobRequest {
                    request_type: JOB.into(),
                    method: "POST".into(),
                    url: format!("{}/Task/{}/$dicom-ups", state.fhir_base_url(), work.task),
                    body: Some(serde_json::to_value(&work).map_err(|_| "Invalid UPS work")?),
                    headers: None,
                    client_id: None,
                    authorization: context
                        .job_authorization(state)
                        .map_err(|_| "Cannot delegate UPS work")?,
                },
                &mut tx,
            )
            .await
            .map_err(|_| "Cannot persist UPS work")?;
        query("UPDATE dicom_ups_tasks SET pending_job=$2,desired_revision=$3,attempts=attempts+1 WHERE task_id=$1").bind(&work.task).bind(job).bind(work.revision).execute(&mut *tx).await.map_err(|_|"Cannot attach UPS work")?;
    }
    tx.commit()
        .await
        .map_err(|_| "Cannot commit UPS scheduling")?;
    Ok(())
}
pub(super) async fn binding(state: &AppState, work: &Work) -> Result<Binding, String> {
    let claim = current_execution_claim().ok_or("Missing UPS execution claim")?;
    let row=query("SELECT workitem_uid,request_metadata,requested_by,remote_state,created_confirmed,creation_started,cancel_requested,cancel_delivered FROM dicom_ups_tasks WHERE task_id=$1 AND connection_name=$2 AND pending_job=$3").bind(&work.task).bind(&work.connection).bind(claim.job_id).fetch_optional(state.db_pool.as_ref()).await.map_err(|_|"Cannot read UPS binding")?.ok_or("UPS work was superseded")?;
    Ok(Binding {
        uid: row.get("workitem_uid"),
        request: serde_json::from_value(row.get("request_metadata"))
            .map_err(|_| "Invalid UPS request metadata")?,
        requested_by: row.get("requested_by"),
        state: row.get("remote_state"),
        confirmed: row.get("created_confirmed"),
        creation_started: row.get("creation_started"),
        cancel: row.get("cancel_requested"),
        cancel_delivered: row.get("cancel_delivered"),
    })
}
pub(super) async fn acknowledge(
    state: &AppState,
    work: &Work,
    metadata: &Value,
    config: &DicomUpsConfig,
) -> Result<(), String> {
    let claim = current_execution_claim().ok_or("Missing UPS execution claim")?;
    let remote_state = metadata["state"].as_str();
    let suppressed = metadata["cancelled_before_submission"]
        .as_bool()
        .unwrap_or(false);
    if remote_state.is_none() && !suppressed {
        return Err("UPS checkpoint has no source state".into());
    }
    let delivered = metadata["cancel_delivered"].as_bool().unwrap_or(false);
    let result=query("UPDATE dicom_ups_tasks t SET applied_revision=GREATEST(applied_revision,$3),pending_job=NULL,remote_state=$4,created_confirmed=created_confirmed OR $4::text IS NOT NULL,cancelled_before_submission=$9,cancel_delivered=cancel_delivered OR $5,attempts=0,last_error=NULL,last_applied_at=clock_timestamp(),next_attempt_at=CASE WHEN desired_revision>$3 THEN clock_timestamp() ELSE clock_timestamp()+make_interval(secs=>$6::double precision) END WHERE task_id=$1 AND connection_name=$2 AND pending_job=$7 AND (NOT $9 OR NOT creation_started) AND EXISTS(SELECT 1 FROM async_jobs j WHERE j.id=$7 AND j.execution_id=$8 AND j.status='in_progress' AND j.lease_expires_at>clock_timestamp() AND j.expires_at>clock_timestamp())").bind(&work.task).bind(&work.connection).bind(work.revision).bind(remote_state).bind(delivered).bind(config.poll_seconds as f64).bind(claim.job_id).bind(claim.execution_id).bind(suppressed).execute(state.db_pool.as_ref()).await.map_err(|_|"Cannot acknowledge UPS reconciliation")?;
    if result.rows_affected() == 0 {
        let applied: Option<i64> = sqlx_core::query_scalar::query_scalar(
            "SELECT applied_revision FROM dicom_ups_tasks WHERE task_id=$1 AND connection_name=$2",
        )
        .bind(&work.task)
        .bind(&work.connection)
        .fetch_optional(state.db_pool.as_ref())
        .await
        .map_err(|_| "Cannot verify UPS acknowledgement")?;
        if !applied.is_some_and(|v| v >= work.revision) {
            return Err("UPS reconciliation lost its execution claim".into());
        }
    }
    Ok(())
}
pub(super) async fn failure(state: &AppState, work: &Work, retry: u64) {
    let Some(claim) = current_execution_claim() else {
        return;
    };
    let _=query("UPDATE dicom_ups_tasks t SET last_error='reconciliation_failed',next_attempt_at=clock_timestamp()+make_interval(secs=>LEAST(3600,$4::double precision*power(2,LEAST(GREATEST(attempts-1,0),10)))) WHERE task_id=$1 AND connection_name=$2 AND pending_job=$3 AND EXISTS(SELECT 1 FROM async_jobs j WHERE j.id=$3 AND j.execution_id=$5 AND j.status='in_progress' AND j.lease_expires_at>clock_timestamp())").bind(&work.task).bind(&work.connection).bind(claim.job_id).bind(retry as f64).bind(claim.execution_id).execute(state.db_pool.as_ref()).await;
}

/// Publish intent before the remote POST. A cancellation wins this compare-and-set
/// if no creation has started; an ambiguous POST can never become a false "absent".
pub(super) async fn begin_creation(state: &AppState, work: &Work) -> Result<(), String> {
    let claim = current_execution_claim().ok_or("Missing UPS execution claim")?;
    let result=query("UPDATE dicom_ups_tasks t SET creation_started=true WHERE task_id=$1 AND connection_name=$2 AND pending_job=$3 AND NOT cancel_requested AND NOT cancelled_before_submission AND EXISTS(SELECT 1 FROM async_jobs j WHERE j.id=$3 AND j.execution_id=$4 AND j.status='in_progress' AND j.lease_expires_at>clock_timestamp() AND j.expires_at>clock_timestamp())").bind(&work.task).bind(&work.connection).bind(claim.job_id).bind(claim.execution_id).execute(state.db_pool.as_ref()).await.map_err(|_|"Cannot persist UPS creation intent")?;
    if result.rows_affected() != 1 {
        return Err("UPS creation was cancelled or lost its execution claim".into());
    }
    Ok(())
}
