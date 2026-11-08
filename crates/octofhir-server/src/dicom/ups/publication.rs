//! Explicit UPS state and result-reference publication through ordinary FHIR writes.
//!
//! # Contents
//! Task identity/state projection, configured DocumentReference/DiagnosticReport
//! results, clinical-field preservation and stable provenance.
//!
//! # Invariants
//! No measurements or clinical conclusions are invented from UPS completion.
//! Result objects are verified in attested PACS manifests before publication.
//! Repeated projections preserve clinical annotations and do not add versions.
use super::{
    config::{ResultKind, ResultMapping},
    model::{Instance, Request},
};
use crate::{
    dicom::mapping,
    operations::{OperationContext, OperationError},
    server::AppState,
};
use octofhir_api::ApiError;
use octofhir_storage::StoredResource;
use serde_json::{Value, json};
use std::collections::BTreeMap;
pub(super) const IDENTIFIER: &str = "https://octofhir.org/identifier/dicom-ups";
const OUTPUT: &str = "https://octofhir.org/CodeSystem/dicom-ups-output";
const CANCEL: &str = "https://octofhir.org/StructureDefinition/dicom-ups-cancel-requested";
fn tagged(resource: &Value, connection: &str) -> bool {
    resource["meta"]["tag"].as_array().is_some_and(|tags| {
        tags.iter()
            .any(|tag| tag["system"] == mapping::SOURCE_TAG && tag["code"] == connection)
    })
}
fn tag(resource: &mut Value, connection: &str) {
    let mut tags = resource["meta"]["tag"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    if !tags
        .iter()
        .any(|tag| tag["system"] == mapping::SOURCE_TAG && tag["code"] == connection)
    {
        tags.push(json!({"system":mapping::SOURCE_TAG,"code":connection}));
    }
    resource["meta"]["tag"] = json!(tags);
}
use crate::dicom::security::inherit_security;

pub(super) struct Progress<'a> {
    pub state: Option<&'a str>,
    pub cancel_delivered: bool,
    pub cancelled_before_submission: bool,
    pub outputs: Option<&'a [(Instance, String)]>,
}
pub(super) fn task(
    previous: &StoredResource,
    connection: &str,
    uid: &str,
    request: &Request,
    progress: Progress<'_>,
) -> Result<Value, String> {
    let Progress {
        state,
        cancel_delivered,
        cancelled_before_submission,
        outputs,
    } = progress;
    let mut task = previous.resource.clone();
    if super::model::task_hash(&task) != request.task_hash {
        return Err("Managed UPS Task inputs changed".into());
    }
    let mut identifiers = task["identifier"].as_array().cloned().unwrap_or_default();
    if identifiers
        .iter()
        .any(|id| id["system"] == IDENTIFIER && id["value"] != format!("urn:oid:{uid}"))
    {
        return Err("Task carries a different UPS identity".into());
    }
    identifiers.retain(|id| id["system"] != IDENTIFIER);
    identifiers.push(json!({"system":IDENTIFIER,"value":format!("urn:oid:{uid}")}));
    task["identifier"] = json!(identifiers);
    let current = task["status"].as_str().unwrap_or("");
    let target = if cancelled_before_submission {
        "cancelled"
    } else {
        match state {
            None | Some("SCHEDULED") => "requested",
            Some("IN PROGRESS") => "in-progress",
            Some("COMPLETED") => "completed",
            Some("CANCELED") => "cancelled",
            _ => return Err("Unknown UPS state".into()),
        }
    };
    // A manual terminal/error state is not an instruction to change a remote UPS.
    if matches!(
        current,
        "completed" | "cancelled" | "failed" | "entered-in-error" | "rejected"
    ) && current != target
    {
        return Err("Task's manual terminal state conflicts with its UPS workitem".into());
    }
    if !matches!(
        current,
        "draft"
            | "requested"
            | "received"
            | "accepted"
            | "ready"
            | "in-progress"
            | "completed"
            | "cancelled"
    ) {
        return Err("Task status requires manual reconciliation".into());
    }
    task["status"] = json!(target);
    task["businessStatus"] = json!({"coding":[{"system":"https://octofhir.org/CodeSystem/dicom-ups-state","code":if cancelled_before_submission {"cancelled-before-submission"}else{state.unwrap_or("QUEUED")}} ]});
    tag(&mut task, connection);
    let mut extensions = task["extension"].as_array().cloned().unwrap_or_default();
    extensions.retain(|e| e["url"] != CANCEL);
    if cancel_delivered && !matches!(state, Some("COMPLETED" | "CANCELED")) {
        extensions.push(json!({"url":CANCEL,"valueBoolean":true}));
    }
    if extensions.is_empty() {
        task.as_object_mut().unwrap().remove("extension");
    } else {
        task["extension"] = json!(extensions);
    }
    if let Some(outputs) = outputs {
        let previous_outputs = task["output"].as_array().cloned().unwrap_or_default();
        let owned = |output: &Value| {
            output["type"]["coding"]
                .as_array()
                .is_some_and(|codes| codes.iter().any(|code| code["system"] == OUTPUT))
        };
        let old: std::collections::BTreeSet<_> = previous_outputs
            .iter()
            .filter(|o| owned(o))
            .filter_map(|o| o["valueReference"]["reference"].as_str())
            .collect();
        let new: std::collections::BTreeSet<_> = outputs
            .iter()
            .map(|(_, reference)| reference.as_str())
            .collect();
        if !old.is_subset(&new) {
            return Err("UPS output list lost previously published results; manual reconciliation is required".into());
        }
        let mut merged: Vec<_> = previous_outputs.into_iter().filter(|o| !owned(o)).collect();
        merged.extend(outputs.iter().map(|(instance,reference)|json!({"type":{"coding":[{"system":OUTPUT,"code":"dicom-result"},{"system":"urn:dicom:sop-class","code":instance.class}]},"valueReference":{"reference":reference}})));
        if merged.is_empty() {
            task.as_object_mut().unwrap().remove("output");
        } else {
            task["output"] = json!(merged);
        }
    }
    Ok(task)
}
pub(super) fn result(
    state: &AppState,
    connection: &str,
    request: &Request,
    instance: &Instance,
    mapping: &ResultMapping,
) -> Result<Value, String> {
    let config = &state.config.dicom.connections[connection];
    let mut url = if config.gateway.is_some() {
        url::Url::parse(&format!(
            "{}/dicomweb/{connection}/",
            state.base_url.trim_end_matches('/')
        ))
        .map_err(|_| "Invalid gateway URL")?
    } else {
        config.base_url()?
    };
    url.path_segments_mut()
        .map_err(|_| "Invalid result retrieval root")?
        .pop_if_empty()
        .extend([
            "studies",
            &instance.study,
            "series",
            &instance.series,
            "instances",
            &instance.uid,
        ]);
    let kind = match mapping.kind {
        ResultKind::DocumentReference => "DocumentReference",
        ResultKind::DiagnosticReport => "DiagnosticReport",
    };
    let id = mapping::resource_id(connection, &format!("ups-result-{kind}"), &instance.uid);
    let mut coding = json!({"system":mapping.system,"code":mapping.code});
    if let Some(display) = &mapping.display {
        coding["display"] = json!(display);
    }
    let code = json!({"coding":[coding]});
    let attachment = json!({"contentType":"application/dicom","url":url.as_str()});
    let mut resource = json!({"resourceType":kind,"id":id,"identifier":[{"system":"urn:dicom:uid","value":format!("urn:oid:{}",instance.uid)}],"subject":{"reference":format!("Patient/{}",request.patient_id)}});
    match mapping.kind {
        ResultKind::DocumentReference => {
            resource["status"] = json!("current");
            resource["type"] = code;
            resource["content"] = json!([{"attachment":attachment}]);
        }
        ResultKind::DiagnosticReport => {
            resource["status"] = json!(
                mapping
                    .report_status
                    .as_ref()
                    .ok_or("An explicit clinical report status is required")?
            );
            resource["code"] = code;
            resource["presentedForm"] = json!([attachment]);
            let study = mapping::resource_id(connection, "study", &instance.study);
            let field = if state.fhir_version == "R5" {
                "study"
            } else {
                "imagingStudy"
            };
            resource[field] = json!([{"reference":format!("ImagingStudy/{study}")}]);
        }
    }
    tag(&mut resource, connection);
    Ok(resource)
}
pub(super) fn provenance(
    connection: &str,
    uid: &str,
    request: &Request,
    actor: &str,
    remote_state: &str,
    outputs: &[(Instance, String)],
) -> Value {
    let id = mapping::resource_id(
        connection,
        "ups-provenance",
        &format!("{uid}:{remote_state}"),
    );
    let mut targets = vec![json!({"reference":format!("Task/{}",request.task_id)})];
    targets.extend(
        outputs
            .iter()
            .map(|(_, reference)| json!({"reference":reference})),
    );
    let mut resource = json!({"resourceType":"Provenance","id":id,"target":targets,"recorded":chrono::Utc::now().to_rfc3339(),"activity":{"text":"UPS state and configured DICOM result reference publication"},"agent":[{"type":{"text":"Task Manager"},"who":{"identifier":{"system":"https://octofhir.org/identifier/dicom-connection","value":connection}}},{"type":{"text":"Requesting client"},"who":{"identifier":{"system":"https://octofhir.org/identifier/client-id","value":actor}}}],"entity":[{"role":"source","what":{"identifier":{"system":"urn:dicom:uid","value":format!("urn:oid:{uid}")}}}]});
    tag(&mut resource, connection);
    resource
}
/// Each existing result is read under current policy and merged only at the
/// connector's declared projection fields. Immutable patient/UID checks precede it.
pub(super) async fn entry(
    state: &AppState,
    context: &OperationContext,
    connection: &str,
    incoming: Value,
) -> Result<Option<Value>, OperationError> {
    let kind = incoming["resourceType"]
        .as_str()
        .expect("native result type");
    let id = incoming["id"].as_str().expect("native result id");
    let previous = context.read_primary(state, kind, id).await?;
    let (resource, version) = if let Some(previous) = previous {
        if !tagged(&previous.resource, connection) {
            return Err(ApiError::conflict("UPS target is not owned by this DICOM source").into());
        }
        if kind == "Provenance" {
            if previous.resource["target"] != incoming["target"]
                || previous.resource["entity"] != incoming["entity"]
            {
                return Err(ApiError::conflict("UPS provenance target identity changed").into());
            }
        } else if previous.resource["subject"] != incoming["subject"]
            || previous.resource["identifier"] != incoming["identifier"]
        {
            return Err(
                ApiError::conflict("UPS result patient or DICOM identity cannot change").into(),
            );
        }
        let fields: &[&str] = match kind {
            "Provenance" => &[],
            "DocumentReference" => &["status", "type", "content"],
            "DiagnosticReport" => &["status", "code", "presentedForm", "imagingStudy", "study"],
            _ => return Err(ApiError::internal("Unknown UPS result projection").into()),
        };
        let mut merged = previous.resource.clone();
        for field in fields {
            if let Some(value) = incoming.get(*field) {
                merged[*field] = value.clone();
            } else {
                merged.as_object_mut().unwrap().remove(*field);
            }
        }
        inherit_security(&mut merged, &[&incoming]);
        if merged == previous.resource {
            return Ok(None);
        }
        (merged, Some(previous.version_id))
    } else {
        (incoming.clone(), None)
    };
    let mut request = json!({"method":"PUT","url":format!("{kind}/{id}")});
    if let Some(version) = version {
        request["ifMatch"] = json!(format!("W/\"{version}\""));
    }
    Ok(Some(json!({"request":request,"resource":resource})))
}
pub(super) fn task_entry(previous: &StoredResource, incoming: Value) -> Option<Value> {
    if previous.resource == incoming {
        return None;
    }
    Some(
        json!({"resource":incoming,"request":{"method":"PUT","url":format!("Task/{}",previous.id),"ifMatch":format!("W/\"{}\"",previous.version_id)}}),
    )
}
pub(super) fn grouped(instances: &[Instance]) -> BTreeMap<&str, Vec<&Instance>> {
    let mut groups = BTreeMap::new();
    for instance in instances {
        groups
            .entry(instance.study.as_str())
            .or_insert_with(Vec::new)
            .push(instance);
    }
    groups
}
