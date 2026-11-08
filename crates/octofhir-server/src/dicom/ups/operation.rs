//! Task-scoped UPS command admission and persisted status.
//!
//! # Invariants
//! Callers select only a named source and action, never a PACS URL, bearer token,
//! workitem UID, patient or DICOM dataset. Submission binds an already attested
//! study to the readable/update-authorized Task; the source worker has its own
//! explicit backend identity and repeats authorization before external effects.
use super::{model, store};
use crate::{
    operations::{OperationContext, OperationError},
    server::AppState,
};
use octofhir_api::ApiError;
use serde_json::{Value, json};
use sqlx_core::{query::query, row::Row};
use std::collections::BTreeMap;

pub(crate) async fn operate(
    state: &AppState,
    context: &OperationContext,
    task_id: &str,
    params: &Value,
) -> Result<Value, OperationError> {
    if context.is_get() {
        return Err(OperationError::InvalidParameters(
            "Use POST for UPS commands/status".into(),
        ));
    }
    let actor = context
        .auth()
        .ok_or_else(|| {
            ApiError::forbidden("UPS operations require an authenticated FHIR identity")
        })?
        .client
        .client_id
        .clone();
    let params = parameters(params)?;
    let name = params["connection"];
    let action = params["action"];
    if !matches!(action, "submit" | "cancel" | "refresh" | "status") {
        return Err(OperationError::InvalidParameters(
            "UPS action must be submit, cancel, refresh or status".into(),
        ));
    }
    let source = state
        .config
        .dicom
        .connections
        .get(name)
        .filter(|c| c.ups.is_some())
        .ok_or_else(|| OperationError::NotFound("UPS source is not configured".into()))?;
    let task = if action == "status" {
        context
            .read_primary(state, "Task", task_id)
            .await?
            .ok_or_else(|| OperationError::NotFound("Task is unavailable".into()))?
    } else {
        context.command_target(state, "Task", task_id).await?
    };
    if action != "status" {
        let exists = query("SELECT 1 FROM dicom_ups_tasks WHERE task_id=$1")
            .bind(task_id)
            .fetch_optional(state.db_pool.as_ref())
            .await
            .map_err(|_| OperationError::Internal("Cannot read UPS binding".into()))?
            .is_some();
        let request = if action == "submit" && !exists {
            if !matches!(
                task.resource["status"].as_str(),
                Some("draft" | "requested")
            ) {
                return Err(ApiError::conflict(
                    "A new UPS workitem requires a draft/requested Task",
                )
                .into());
            }
            let focus = task.resource["focus"]["reference"]
                .as_str()
                .and_then(|r| r.strip_prefix("ImagingStudy/"))
                .ok_or_else(|| {
                    OperationError::InvalidParameters(
                        "UPS Task.focus must be a local imported ImagingStudy".into(),
                    )
                })?;
            let study = context
                .read_primary(state, "ImagingStudy", focus)
                .await?
                .ok_or_else(|| OperationError::NotFound("UPS input study is unavailable".into()))?;
            let uids: Vec<_> = study.resource["identifier"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|id| id["system"] == "urn:dicom:uid")
                .filter_map(|id| {
                    id["value"]
                        .as_str()
                        .and_then(|uid| uid.strip_prefix("urn:oid:"))
                })
                .collect();
            if uids.len() != 1 {
                return Err(OperationError::InvalidParameters(
                    "UPS input study requires one canonical DICOM UID".into(),
                ));
            }
            let (verified, patient) =
                super::super::attestation::authorize(state, context, name, uids[0]).await?;
            if verified.id != study.id {
                return Err(
                    ApiError::conflict("UPS Task refers to a different source's study").into(),
                );
            }
            Some(
                model::request(
                    &task.resource,
                    &patient.resource,
                    &verified.resource,
                    uids[0],
                    source,
                )
                .map_err(OperationError::InvalidParameters)?,
            )
        } else {
            None
        };
        store::enqueue(
            state,
            name,
            task_id,
            action,
            request,
            &actor,
            &model::task_hash(&task.resource),
        )
        .await
        .map_err(ApiError::conflict)?;
    }
    status(state, name, task_id).await
}
fn parameters(params: &Value) -> Result<BTreeMap<&str, &str>, OperationError> {
    let invalid = || {
        OperationError::InvalidParameters(
            "Expected exactly connection and action valueString Parameters".into(),
        )
    };
    if params["resourceType"] != "Parameters" {
        return Err(invalid());
    }
    let values = params["parameter"]
        .as_array()
        .filter(|v| v.len() == 2)
        .ok_or_else(invalid)?;
    let mut result = BTreeMap::new();
    for value in values {
        if value
            .as_object()
            .ok_or_else(invalid)?
            .keys()
            .any(|key| !matches!(key.as_str(), "name" | "valueString"))
        {
            return Err(invalid());
        }
        let name = value["name"]
            .as_str()
            .filter(|v| matches!(*v, "connection" | "action"))
            .ok_or_else(invalid)?;
        let value = value["valueString"]
            .as_str()
            .filter(|v| !v.is_empty() && v.len() <= 64)
            .ok_or_else(invalid)?;
        if result.insert(name, value).is_some() {
            return Err(invalid());
        }
    }
    Ok(result)
}
async fn status(state: &AppState, name: &str, task: &str) -> Result<Value, OperationError> {
    let row=query("SELECT workitem_uid,remote_state,desired_revision,applied_revision,pending_job,last_error,creation_started,cancel_requested,cancel_delivered,cancelled_before_submission FROM dicom_ups_tasks WHERE task_id=$1 AND connection_name=$2").bind(task).bind(name).fetch_optional(state.db_pool.as_ref()).await.map_err(|_|OperationError::Internal("Cannot read UPS status".into()))?.ok_or_else(||OperationError::NotFound("Task has no UPS binding on this source".into()))?;
    let source = &state.config.dicom.connections[name];
    let config = source.ups.as_ref().expect("configured UPS");
    let uid: String = row.get("workitem_uid");
    let mut url = source
        .service_url(&config.url)
        .map_err(OperationError::Internal)?;
    url.path_segments_mut()
        .map_err(|_| OperationError::Internal("Invalid UPS service root".into()))?
        .pop_if_empty()
        .extend(["workitems", &uid]);
    let mut parameters = vec![
        json!({"name":"workitem","valueUri":url.as_str()}),
        json!({"name":"queued","valueBoolean":row.get::<i64,_>("desired_revision")>row.get::<i64,_>("applied_revision")||row.get::<Option<uuid::Uuid>,_>("pending_job").is_some()}),
        json!({"name":"creationStarted","valueBoolean":row.get::<bool,_>("creation_started")}),
        json!({"name":"cancelRequested","valueBoolean":row.get::<bool,_>("cancel_requested")}),
        json!({"name":"cancelDelivered","valueBoolean":row.get::<bool,_>("cancel_delivered")}),
        json!({"name":"cancelledBeforeSubmission","valueBoolean":row.get::<bool,_>("cancelled_before_submission")}),
    ];
    if let Some(value) = row.get::<Option<String>, _>("remote_state") {
        parameters.push(json!({"name":"state","valueString":value}));
    }
    if let Some(error) = row.get::<Option<String>, _>("last_error") {
        parameters.push(json!({"name":"lastError","valueCode":error}));
    }
    Ok(json!({"resourceType":"Parameters","parameter":parameters}))
}
