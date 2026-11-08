//! Replay-safe UPS commands and clinical reconciliation on fenced async jobs.
//!
//! # Invariants
//! Persist creation intent before POST, query a stable UID after ambiguous delivery,
//! and never infer cancellation or completion from an HTTP acknowledgment. FHIR
//! writes/checkpoints share a transaction. Outputs remain in PACS and must match
//! the pinned patient and exact imported SOP references before publication.
use super::{
    model,
    protocol::Client,
    publication::{self, Progress},
    store::{self, Work},
};
use crate::{
    async_jobs::{CheckpointStep, current_execution_claim},
    operations::OperationContext,
    server::AppState,
};
use serde_json::{Value, json};
use std::sync::Arc;

pub(crate) async fn execute(state: AppState, body: Value) -> Result<Value, String> {
    let work: Work =
        serde_json::from_value(body.clone()).map_err(|_| "Invalid UPS work request")?;
    let source = state
        .config
        .dicom
        .connections
        .get(&work.connection)
        .ok_or("UPS source is unavailable")?;
    let config = source.ups.as_ref().ok_or("UPS integration is disabled")?;
    if store::fingerprint(source) != work.fingerprint {
        return Err("UPS source identity changed".into());
    }
    let retry = config.retry_seconds;
    let result = run(&state, &work, &body).await;
    if result.is_err() {
        store::failure(&state, &work, retry).await;
    }
    result
}
async fn run(state: &AppState, work: &Work, body: &Value) -> Result<Value, String> {
    let claim = current_execution_claim().ok_or("Missing UPS execution claim")?;
    let context = OperationContext::for_job(state, claim.job_id)
        .await
        .map_err(|_| "UPS delegation is unavailable")?;
    let source = &state.config.dicom.connections[&work.connection];
    let config = source.ups.as_ref().ok_or("UPS integration is disabled")?;
    let auth = context.auth().ok_or("UPS requires a backend identity")?;
    if auth.client.client_id != config.client_id
        || auth
            .scopes()
            .any(|scope| !config.scopes.iter().any(|allowed| allowed == scope))
    {
        return Err("UPS job no longer matches its configured identity".into());
    }
    let mut step = CheckpointStep::new("ups-current".into(), body)
        .map_err(|_| "Cannot initialize UPS checkpoint")?;
    if let Some(record) = state
        .async_job_manager
        .restore_checkpoint(&step)
        .await
        .map_err(|_| "Cannot recover UPS checkpoint")?
    {
        store::acknowledge(state, work, &record["metadata"], config).await?;
        return Ok(json!({"task":work.task,"recovered":true}));
    }
    let binding = store::binding(state, work).await?;
    let task = context
        .command_target(state, "Task", &work.task)
        .await
        .map_err(|_| "UPS Task is unavailable or unauthorized")?;
    if model::task_hash(&task.resource) != binding.request.task_hash {
        return Err("UPS Task inputs require manual reconciliation".into());
    }
    let patient = context
        .read_primary(state, "Patient", &binding.request.patient_id)
        .await
        .map_err(|_| "UPS Patient is unauthorized")?
        .ok_or("UPS Patient is unavailable")?;
    let input_study = context
        .read_primary(state, "ImagingStudy", &binding.request.study_id)
        .await
        .map_err(|_| "UPS input study is unauthorized")?
        .ok_or("UPS input study is unavailable")?;
    if input_study.resource["subject"]["reference"] != format!("Patient/{}", patient.id) {
        return Err("UPS input study patient changed".into());
    }
    let system = source
        .patient_identifier_systems
        .get(&binding.request.issuer)
        .ok_or("UPS assigning authority is unavailable")?;
    if !patient.resource["identifier"]
        .as_array()
        .is_some_and(|ids| {
            ids.iter().any(|id| {
                id["system"] == *system && id["value"] == binding.request.patient_identifier
            })
        })
    {
        return Err(
            "UPS assigning-authority identity changed; manual reconciliation is required".into(),
        );
    }
    let policy = Arc::new(crate::egress::EgressPolicy::from_server_config(
        &state.config.server,
    )?);
    let client = Client::new(source, policy.clone())?;
    let mut remote = client.retrieve(&binding.uid).await?;
    if remote.is_none() {
        if binding.confirmed {
            return Err(
                "Previously confirmed UPS workitem is unavailable; it must not be recreated".into(),
            );
        }
        if binding.cancel {
            if binding.creation_started {
                return Err("UPS creation outcome is uncertain; cancellation awaits a confirmed workitem, never an inferred absence".into());
            }
            step.metadata =
                json!({"state":null,"cancel_delivered":false,"cancelled_before_submission":true});
            let mut cancelled = publication::task(
                &task,
                &work.connection,
                &binding.uid,
                &binding.request,
                Progress {
                    state: None,
                    cancel_delivered: false,
                    cancelled_before_submission: true,
                    outputs: None,
                },
            )?;
            crate::dicom::security::inherit_security(
                &mut cancelled,
                &[&task.resource, &patient.resource, &input_study.resource],
            );
            crate::dicom::import::commit(
                state,
                &context,
                publication::task_entry(&task, cancelled)
                    .into_iter()
                    .collect(),
                Some(step.clone()),
            )
            .await
            .map_err(|_| "Cannot commit cancellation before UPS submission")?;
            store::acknowledge(state, work, &step.metadata, config).await?;
            return Ok(json!({"task":work.task,"cancelledBeforeSubmission":true}));
        }
        if config.notifications {
            let ready:bool=sqlx_core::query_scalar::query_scalar("SELECT COALESCE(watch_ready_until>clock_timestamp(),false) FROM dicom_ups_sources WHERE connection_name=$1").bind(&work.connection).fetch_one(state.db_pool.as_ref()).await.map_err(|_|"Cannot verify UPS deletion-lock readiness")?;
            if !ready {
                return Err(
                    "UPS notification subscription is not ready; creation remains queued".into(),
                );
            }
        }
        let (study, verified_patient) = crate::dicom::attestation::authorize(
            state,
            &context,
            &work.connection,
            &binding.request.study_uid,
        )
        .await
        .map_err(|_| "UPS input manifest is unavailable")?;
        if study.id != binding.request.study_id || verified_patient.id != binding.request.patient_id
        {
            return Err("UPS input patient/study identity changed".into());
        }
        // Revalidate live input metadata on every attempted creation, even after a
        // recovered preparation checkpoint. Checkpoints are not freshness guarantees.
        let metadata = crate::dicom::client::DicomWebClient::new(source, policy)
            .map_err(|_| "PACS is unavailable")?
            .study_metadata(&binding.request.study_uid)
            .await
            .map_err(|_| "UPS input metadata is unavailable")?;
        let release = state
            .config
            .fhir
            .release()
            .map_err(|_| "Invalid FHIR release")?;
        let fresh = crate::dicom::mapping::study(
            &work.connection,
            source,
            release,
            &binding.request.study_uid,
            &patient.id,
            &patient.resource,
            &metadata,
        )
        .map_err(|_| "UPS input patient or manifest changed")?;
        if !binding
            .request
            .instances
            .iter()
            .all(|instance| contains(&fresh, instance))
        {
            return Err(
                "UPS input instances are no longer available in their authorized source".into(),
            );
        }
        let prepared = CheckpointStep::new("ups-prepared".into(), body)
            .map_err(|_| "Cannot initialize UPS preparation")?;
        if state
            .async_job_manager
            .restore_checkpoint(&prepared)
            .await
            .map_err(|_| "Cannot recover UPS preparation")?
            .is_none()
        {
            let mut queued = publication::task(
                &task,
                &work.connection,
                &binding.uid,
                &binding.request,
                Progress {
                    state: None,
                    cancel_delivered: false,
                    cancelled_before_submission: false,
                    outputs: None,
                },
            )?;
            crate::dicom::security::inherit_security(
                &mut queued,
                &[&task.resource, &patient.resource, &input_study.resource],
            );
            crate::dicom::import::commit(
                state,
                &context,
                publication::task_entry(&task, queued).into_iter().collect(),
                Some(prepared),
            )
            .await
            .map_err(|_| "Cannot authorize and prepare UPS Task")?;
        }
        store::begin_creation(state, work).await?;
        client
            .create(&binding.uid, &binding.request, source)
            .await?;
        remote = client.retrieve(&binding.uid).await?;
    }
    let mut remote = remote.ok_or("UPS creation is not yet visible; retry its persisted UID")?;
    binding.request.verify(&remote, &binding.uid)?;
    let mut current = model::state(&remote)?.to_owned();
    transition(binding.state.as_deref(), &current)?;
    let mut cancel_delivered = binding.cancel_delivered;
    if binding.cancel && !matches!(current.as_str(), "COMPLETED" | "CANCELED") && !cancel_delivered
    {
        client.cancel(&binding.uid).await?;
        cancel_delivered = true;
        remote = client
            .retrieve(&binding.uid)
            .await?
            .ok_or("UPS workitem disappeared after cancellation request")?;
        binding.request.verify(&remote, &binding.uid)?;
        let after = model::state(&remote)?.to_owned();
        transition(Some(&current), &after)?;
        current = after;
    }
    let mut outputs = Vec::new();
    let mut entries = Vec::new();
    let mut resources = Vec::new();
    let mut output_security = Vec::new();
    if current == "COMPLETED" {
        let references = model::output_references(&remote, config.max_references)?;
        // Refuse an unmapped clinical interpretation before changing any result index.
        if references
            .iter()
            .any(|instance| !config.result_mappings.contains_key(&instance.class))
        {
            return Err("UPS output SOP Class requires an explicit result mapping".into());
        }
        for (study_uid, instances) in publication::grouped(&references) {
            let input = json!({"work":body,"study":study_uid});
            let output_step = CheckpointStep::new(format!("ups-output:{study_uid}"), &input)
                .map_err(|_| "Cannot initialize UPS output checkpoint")?;
            if state
                .async_job_manager
                .restore_checkpoint(&output_step)
                .await
                .map_err(|_| "Cannot recover UPS output checkpoint")?
                .is_none()
            {
                crate::dicom::import::import(
                    state,
                    &context,
                    &work.connection,
                    study_uid,
                    Some(&patient.id),
                    Some(instances.len() as u64),
                    Some(output_step),
                )
                .await
                .map_err(|_| "UPS output patient or metadata is unavailable")?;
            }
            let (study, output_patient) =
                crate::dicom::attestation::authorize(state, &context, &work.connection, study_uid)
                    .await
                    .map_err(|_| "UPS output manifest is unauthorized")?;
            output_security.push(json!({"meta":{"security":study.resource["meta"]["security"]}}));
            if output_patient.id != patient.id
                || !instances
                    .iter()
                    .all(|instance| contains(&study.resource, instance))
            {
                return Err(
                    "UPS output SOP references do not match the pinned patient/manifest".into(),
                );
            }
        }
        for instance in references {
            let resource = publication::result(
                state,
                &work.connection,
                &binding.request,
                &instance,
                &config.result_mappings[&instance.class],
            )?;
            let reference = format!(
                "{}/{}",
                resource["resourceType"].as_str().expect("native result"),
                resource["id"].as_str().expect("native result id")
            );
            resources.push(resource);
            outputs.push((instance, reference));
        }
    }
    // Preparation or an output import may have changed resource versions. Always
    // read the final Task snapshot again and preserve concurrent clinical additions.
    let task = context
        .command_target(state, "Task", &work.task)
        .await
        .map_err(|_| "UPS Task is no longer writable")?;
    let mut security_sources = vec![&task.resource, &patient.resource, &input_study.resource];
    security_sources.extend(output_security.iter());
    for mut resource in resources {
        crate::dicom::security::inherit_security(&mut resource, &security_sources);
        if let Some(entry) = publication::entry(state, &context, &work.connection, resource)
            .await
            .map_err(|_| "Cannot authorize UPS clinical result")?
        {
            entries.push(entry);
        }
    }
    let mut projected = publication::task(
        &task,
        &work.connection,
        &binding.uid,
        &binding.request,
        Progress {
            state: Some(&current),
            cancel_delivered,
            cancelled_before_submission: false,
            outputs: (current == "COMPLETED").then_some(outputs.as_slice()),
        },
    )?;
    crate::dicom::security::inherit_security(&mut projected, &security_sources);
    if let Some(entry) = publication::task_entry(&task, projected) {
        entries.push(entry);
    }
    if matches!(current.as_str(), "COMPLETED" | "CANCELED") {
        let mut provenance = publication::provenance(
            &work.connection,
            &binding.uid,
            &binding.request,
            &binding.requested_by,
            &current,
            &outputs,
        );
        crate::dicom::security::inherit_security(&mut provenance, &security_sources);
        if let Some(entry) = publication::entry(state, &context, &work.connection, provenance)
            .await
            .map_err(|_| "Cannot authorize UPS provenance")?
        {
            entries.push(entry);
        }
    }
    step.metadata = json!({"state":current,"cancel_delivered":cancel_delivered,"cancelled_before_submission":false});
    crate::dicom::import::commit(state, &context, entries, Some(step.clone()))
        .await
        .map_err(|_| "Cannot commit UPS Task/results/checkpoint")?;
    store::acknowledge(state, work, &step.metadata, config).await?;
    Ok(json!({"task":work.task,"state":current,"results":outputs.len()}))
}
fn contains(study: &Value, instance: &model::Instance) -> bool {
    study["series"].as_array().is_some_and(|series| {
        series.iter().any(|series| {
            series["uid"] == instance.series
                && series["instance"].as_array().is_some_and(|instances| {
                    instances.iter().any(|item| {
                        item["uid"] == instance.uid
                            && super::super::mapping::sop_class(item)
                                .is_ok_and(|class| class == instance.class)
                    })
                })
        })
    })
}
fn transition(previous: Option<&str>, current: &str) -> Result<(), String> {
    let valid = match previous {
        None | Some("SCHEDULED") => true,
        Some("IN PROGRESS") => current != "SCHEDULED",
        Some("COMPLETED" | "CANCELED") => previous == Some(current),
        _ => false,
    };
    if !valid {
        return Err("UPS state regressed; manual reconciliation is required".into());
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn terminal_states_cannot_regress_but_missed_intermediate_events_recover() {
        assert!(transition(Some("SCHEDULED"), "COMPLETED").is_ok());
        assert!(transition(Some("IN PROGRESS"), "SCHEDULED").is_err());
        assert!(transition(Some("COMPLETED"), "CANCELED").is_err());
    }
}
