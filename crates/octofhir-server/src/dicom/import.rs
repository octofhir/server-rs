//! Shared metadata import for foreground calls and fenced durable workers.
//!
//! # Contents
//! Source serialization, primary-backed patient resolution and ordinary FHIR writes.
//!
//! # Invariants
//! Every instance matches one readable, existing Patient. A previously pinned
//! patient cannot change. Partial snapshots never replace a complete manifest.
//! Worker checkpoints commit in the same transaction as resource mutations.

use super::{DicomError, client::DicomWebClient, mapping};
use crate::operations::dicom::{dicom_error, entry};
use crate::{
    async_jobs::CheckpointStep,
    operations::{OperationContext, OperationError},
    server::AppState,
};
use axum::{
    Extension, Json,
    body::to_bytes,
    extract::State,
    http::{HeaderMap, HeaderValue},
    response::IntoResponse,
};
use octofhir_api::ApiError;
use octofhir_auth::smart::scopes::FhirOperation;
use octofhir_core::fhir::FhirVersion;
use octofhir_storage::{SearchParams, StoredResource};
use serde_json::{Value, json};
use sqlx_core::query_scalar::query_scalar;
use std::sync::Arc;

pub(crate) const AVAILABILITY: &str = "https://octofhir.org/StructureDefinition/dicom-availability";
pub(crate) struct Imported {
    pub patient_id: String,
    pub study_id: String,
    pub endpoint_id: String,
    pub changed: bool,
}

pub(crate) fn availability(resource: &mut Value, state: &str) {
    let mut extensions = resource["extension"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    extensions.retain(|extension| extension["url"] != AVAILABILITY);
    extensions.push(json!({"url":AVAILABILITY,"valueCode":state}));
    resource["extension"] = json!(extensions);
}

pub(crate) async fn commit(
    state: &AppState,
    context: &OperationContext,
    entries: Vec<Value>,
    step: Option<CheckpointStep>,
) -> Result<bool, OperationError> {
    let changed = !entries.is_empty();
    if !changed && super::attestation::active() {
        let future = async {
            let mut tx = state
                .storage
                .begin_transaction()
                .await
                .map_err(crate::handlers::map_storage_error)?;
            super::attestation::save(&mut *tx).await?;
            crate::async_jobs::save_transaction_result(
                &mut *tx,
                &json!({}),
                state.config.search.include_limits.max_bytes,
            )
            .await?;
            tx.commit()
                .await
                .map_err(crate::handlers::map_storage_error)
        };
        if let Some(step) = step {
            step.scope(future).await?;
        } else {
            future.await?;
        }
        return Ok(false);
    }
    if let Some(step) = step {
        if changed {
            crate::handlers::execute_worker_write_chunk(state, entries, false, step).await?;
        } else {
            state
                .async_job_manager
                .save_read_checkpoint(
                    &step,
                    &json!({}),
                    state.config.search.include_limits.max_bytes,
                )
                .await?;
        }
    } else if changed {
        let auth = context
            .auth()
            .cloned()
            .map(Arc::new)
            .unwrap_or_else(|| state.anonymous_auth_context.clone());
        let mut headers = HeaderMap::new();
        headers.insert("prefer", HeaderValue::from_static("return=minimal"));
        let response = Box::pin(crate::handlers::transaction_handler(
            State(state.clone()),
            Extension(auth),
            headers,
            Json(json!({"resourceType":"Bundle","type":"transaction","entry":entries})),
        ))
        .await?
        .into_response();
        if !response.status().is_success() {
            return Err(OperationError::Internal(
                "DICOM FHIR transaction failed".into(),
            ));
        }
        to_bytes(response.into_body(), 64 * 1024)
            .await
            .map_err(|_| OperationError::Internal("Invalid DICOM transaction response".into()))?;
    }
    Ok(changed)
}

pub(crate) async fn study_lock(
    state: &AppState,
    connection: &str,
    uid: &str,
) -> Result<sqlx_core::pool::PoolConnection<sqlx_postgres::Postgres>, OperationError> {
    let lock_id = mapping::resource_id(connection, "study", uid);
    let mut lock = state
        .dicom_lock_pool
        .acquire()
        .await
        .map_err(|_| ApiError::conflict("DICOM import capacity is busy; retry later"))?;
    lock.close_on_drop();
    let acquired: bool = query_scalar("SELECT pg_try_advisory_lock(hashtextextended($1,0))")
        .bind(format!("dicom-import:{lock_id}"))
        .fetch_one(&mut *lock)
        .await
        .map_err(|_| OperationError::Internal("Cannot acquire import lock".into()))?;
    if !acquired {
        return Err(ApiError::conflict("This study is already being imported; retry later").into());
    }
    Ok(lock)
}

fn escape_token(value: &str) -> String {
    let mut result = String::new();
    for ch in value.chars() {
        if matches!(ch, '\\' | '|' | ',' | '$') {
            result.push('\\');
        }
        result.push(ch);
    }
    result
}

async fn resolve_patient(
    state: &AppState,
    context: &OperationContext,
    config: &super::DicomConnection,
    datasets: &[Value],
    patient: Option<&str>,
) -> Result<StoredResource, OperationError> {
    if let Some(id) = patient {
        return context
            .read_primary(state, "Patient", id)
            .await?
            .ok_or_else(|| OperationError::NotFound("Patient is unavailable".into()));
    }
    let first = datasets
        .first()
        .ok_or_else(|| dicom_error(DicomError::Partial))?;
    let (system, id) = mapping::patient_identifier(first, config).map_err(dicom_error)?;
    let params = SearchParams::new().with_count(2).with_param(
        "identifier",
        format!("{}|{}", escape_token(&system), escape_token(&id)),
    );
    let result = context.search_primary(state, "Patient", &params).await?;
    if result.entries.len() != 1 || result.has_more {
        return Err(ApiError::unprocessable_entity("DICOM identity requires exactly one readable Patient; manual reconciliation is required",None).into());
    }
    Ok(result.entries.into_iter().next().unwrap())
}

pub(crate) async fn import(
    state: &AppState,
    context: &OperationContext,
    connection: &str,
    uid: &str,
    patient_id: Option<&str>,
    expected_instances: Option<u64>,
    mut step: Option<CheckpointStep>,
) -> Result<Imported, OperationError> {
    super::validate_uid(uid).map_err(dicom_error)?;
    let config = state
        .config
        .dicom
        .connections
        .get(connection)
        .ok_or_else(|| OperationError::NotFound("DICOM connection is not configured".into()))?;
    let study_id = mapping::resource_id(connection, "study", uid);
    let endpoint_id = mapping::resource_id(connection, "endpoint", "");
    for kind in ["ImagingStudy", "Endpoint"] {
        context.policy_context(state, FhirOperation::Read, kind, None, &SearchParams::new())?;
        context.policy_context(
            state,
            FhirOperation::Update,
            kind,
            None,
            &SearchParams::new(),
        )?;
    }
    if patient_id.is_none() {
        context.policy_context(
            state,
            FhirOperation::Search,
            "Patient",
            None,
            &SearchParams::new(),
        )?;
    }
    if config.sync.is_some() {
        let blocked: bool = query_scalar("SELECT EXISTS(SELECT 1 FROM dicom_sync_studies WHERE connection_name=$1 AND study_uid=$2 AND override_state IS NOT NULL)")
            .bind(connection).bind(uid).fetch_one(state.db_pool.as_ref()).await.map_err(|_|OperationError::Internal("Cannot read DICOM availability".into()))?;
        if blocked {
            return Err(ApiError::conflict(
                "DICOM source requires an explicit restoration notification before re-import",
            )
            .into());
        }
    }
    let _lock = study_lock(state, connection, uid).await?;
    let policy = Arc::new(
        crate::egress::EgressPolicy::from_server_config(&state.config.server)
            .map_err(OperationError::Internal)?,
    );
    let client = DicomWebClient::new(config, policy).map_err(dicom_error)?;
    // Resolve explicit patients before network access; background discovery must
    // first read source metadata, then resolves identity on the primary database.
    let explicit = if let Some(id) = patient_id {
        Some(resolve_patient(state, context, config, &[], Some(id)).await?)
    } else {
        None
    };
    if let Some(patient) = &explicit {
        let mapped = patient.resource["identifier"]
            .as_array()
            .is_some_and(|ids| {
                ids.iter().any(|id| {
                    id["value"].as_str().is_some_and(|v| !v.trim().is_empty())
                        && config
                            .patient_identifier_systems
                            .values()
                            .any(|system| id["system"] == *system)
                })
            });
        if !mapped {
            return Err(ApiError::unprocessable_entity(
                "Patient has no identifier for a configured DICOM assigning authority",
                None,
            )
            .into());
        }
    }
    let datasets = client.study_metadata(uid).await.map_err(dicom_error)?;
    if expected_instances.is_some_and(|expected| (datasets.len() as u64) < expected) {
        return Err(dicom_error(DicomError::Partial));
    }
    let patient = match explicit {
        Some(patient) => patient,
        None => resolve_patient(state, context, config, &datasets, None).await?,
    };
    let release: FhirVersion = state
        .config
        .fhir
        .version
        .parse()
        .map_err(|_| OperationError::Internal("Invalid FHIR release".into()))?;
    let mut study = mapping::study(
        connection,
        config,
        release,
        uid,
        &patient.id,
        &patient.resource,
        &datasets,
    )
    .map_err(dicom_error)?;
    availability(&mut study, "available");
    super::security::inherit_security(&mut study, &[&patient.resource]);
    let mut endpoint = mapping::endpoint(connection, config, release);
    if config.gateway.is_some() {
        endpoint["address"] = json!(format!(
            "{}/dicomweb/{connection}",
            state.base_url.trim_end_matches('/')
        ));
    }
    let endpoint_id = endpoint["id"].as_str().unwrap_or(&endpoint_id).to_owned();
    let mut entries = Vec::new();
    let manifest = study.clone();
    for resource in [endpoint, study] {
        if let Some(entry) = entry(state, context, connection, resource).await? {
            entries.push(entry);
        }
    }
    if let Some(step) = &mut step {
        step.metadata = json!({"patient_id":patient.id});
    }
    let changed = super::attestation::scope(
        connection,
        uid,
        config,
        &patient.id,
        &manifest,
        commit(state, context, entries, step),
    )
    .await?;
    Ok(Imported {
        patient_id: patient.id,
        study_id,
        endpoint_id,
        changed,
    })
}

pub(crate) async fn unavailable(
    state: &AppState,
    context: &OperationContext,
    connection: &str,
    uid: &str,
    status: &str,
    mut step: CheckpointStep,
) -> Result<Option<String>, OperationError> {
    let _lock = study_lock(state, connection, uid).await?;
    let id = mapping::resource_id(connection, "study", uid);
    let Some(previous) = context.read_primary(state, "ImagingStudy", &id).await? else {
        commit(state, context, Vec::new(), Some(step)).await?;
        return Ok(None);
    };
    mapping::merge(&previous.resource, &previous.resource, connection).map_err(dicom_error)?;
    let patient = previous.resource["subject"]["reference"]
        .as_str()
        .and_then(|r| r.strip_prefix("Patient/"))
        .ok_or_else(|| ApiError::conflict("Imported study has no local patient"))?
        .to_owned();
    let source_patient = context
        .read_primary(state, "Patient", &patient)
        .await?
        .ok_or_else(|| OperationError::NotFound("Patient is unavailable".into()))?;
    let mut resource = previous.resource.clone();
    resource["status"] = json!("unknown");
    availability(&mut resource, status);
    super::security::inherit_security(&mut resource, &[&source_patient.resource]);
    step.metadata = json!({"patient_id":patient});
    let manifest = resource.clone();
    let entries = if resource == previous.resource {
        Vec::new()
    } else {
        vec![
            json!({"resource":resource,"request":{"method":"PUT","url":format!("ImagingStudy/{id}"),"ifMatch":format!("W/\"{}\"",previous.version_id)}}),
        ]
    };
    super::attestation::scope(
        connection,
        uid,
        &state.config.dicom.connections[connection],
        &patient,
        &manifest,
        commit(state, context, entries, Some(step)),
    )
    .await?;
    Ok(Some(patient))
}
