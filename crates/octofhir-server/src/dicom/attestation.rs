//! Atomic proof that an authorized manifest originated from the connector.
//!
//! # Contents
//! Source identity, projection hashing and a private task-local storage capability.
//!
//! # Invariants
//! FHIR tags alone are forgeable. Gateway admission checks this independent record.
//! Proofs commit with FHIR writes/checkpoints, including unchanged imports. Patient
//! and source bindings survive deletion of a FHIR representation.

use super::DicomConnection;
use octofhir_api::ApiError;
use octofhir_storage::{DicomManifest, Transaction};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

tokio::task_local! { static MANIFEST: DicomManifest; }

pub(crate) fn source_hash(config: &DicomConnection) -> String {
    digest(
        &json!({"wado":config.wado_url,"issuers":config.patient_identifier_systems,"default_issuer":config.default_patient_issuer}),
    )
}
fn digest(value: &Value) -> String {
    let mut canonical = value.clone();
    canonical.sort_all_objects();
    let value = &canonical;
    hex::encode(Sha256::digest(
        serde_json::to_vec(value).expect("JSON projection"),
    ))
}
pub(crate) fn manifest_hash(resource: &Value) -> String {
    // Exclude clinical additions/security labels; their current read policy still applies.
    let identifiers: Vec<_> = resource["identifier"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|id| id["system"] == "urn:dicom:uid")
        .collect();
    let availability: Vec<_> = resource["extension"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|e| e["url"] == super::import::AVAILABILITY)
        .collect();
    digest(
        &json!({"id":resource["id"],"identifier":identifiers,"subject":resource["subject"],"series":resource["series"],"status":resource["status"],"availability":availability,"numberOfInstances":resource["numberOfInstances"],"numberOfSeries":resource["numberOfSeries"]}),
    )
}
pub(crate) async fn scope<F: std::future::Future>(
    connection: &str,
    uid: &str,
    config: &DicomConnection,
    patient: &str,
    resource: &Value,
    future: F,
) -> F::Output {
    MANIFEST
        .scope(
            DicomManifest {
                connection: connection.into(),
                study_uid: uid.into(),
                resource_id: resource["id"].as_str().expect("mapped id").into(),
                patient_id: patient.into(),
                source_hash: source_hash(config),
                manifest_hash: manifest_hash(resource),
            },
            future,
        )
        .await
}
pub(crate) fn active() -> bool {
    MANIFEST.try_with(|_| ()).is_ok()
}
pub(crate) async fn save(tx: &mut dyn Transaction) -> Result<(), ApiError> {
    let manifest = MANIFEST
        .try_with(|m| DicomManifest {
            connection: m.connection.clone(),
            study_uid: m.study_uid.clone(),
            resource_id: m.resource_id.clone(),
            patient_id: m.patient_id.clone(),
            source_hash: m.source_hash.clone(),
            manifest_hash: m.manifest_hash.clone(),
        })
        .ok();
    if let Some(manifest) = manifest {
        tx.save_dicom_manifest(&manifest)
            .await
            .map_err(|_| ApiError::conflict("DICOM source or patient binding cannot change"))?;
    }
    Ok(())
}

/// Shared admission for gateway retrieval and UPS input selection.
pub(crate) async fn authorize(
    state: &crate::server::AppState,
    context: &crate::operations::OperationContext,
    connection: &str,
    uid: &str,
) -> Result<
    (
        octofhir_storage::StoredResource,
        octofhir_storage::StoredResource,
    ),
    ApiError,
> {
    use sqlx_core::{query::query, row::Row};
    let unavailable = || ApiError::not_found("DICOM study is unavailable");
    let error = |error: crate::operations::OperationError| match error {
        crate::operations::OperationError::Api(error) => error,
        crate::operations::OperationError::NotFound(_) => unavailable(),
        _ => ApiError::internal("DICOM authorization failed"),
    };
    if context.auth().is_none() {
        return Err(ApiError::forbidden(
            "DICOM access requires an authenticated FHIR identity",
        ));
    }
    let config = state
        .config
        .dicom
        .connections
        .get(connection)
        .ok_or_else(unavailable)?;
    let id = super::mapping::resource_id(connection, "study", uid);
    let study = context
        .read_primary(state, "ImagingStudy", &id)
        .await
        .map_err(error)?
        .ok_or_else(unavailable)?;
    let patient_id = study.resource["subject"]["reference"]
        .as_str()
        .and_then(|r| r.strip_prefix("Patient/"))
        .ok_or_else(unavailable)?;
    let patient = context
        .read_primary(state, "Patient", patient_id)
        .await
        .map_err(error)?
        .ok_or_else(unavailable)?;
    let proof=query("SELECT resource_id,patient_id,source_hash,manifest_hash FROM dicom_manifests WHERE connection_name=$1 AND study_uid=$2").bind(connection).bind(uid).fetch_optional(state.db_pool.as_ref()).await.map_err(|_|ApiError::internal("Cannot verify DICOM manifest"))?.ok_or_else(unavailable)?;
    if proof.get::<String, _>("resource_id") != id
        || proof.get::<String, _>("patient_id") != patient_id
        || proof.get::<String, _>("source_hash") != source_hash(config)
        || proof.get::<String, _>("manifest_hash") != manifest_hash(&study.resource)
        || study.resource["status"] != "available"
    {
        return Err(unavailable());
    }
    Ok((study, patient))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn projection_digest_ignores_object_key_order_and_clinical_notes() {
        let first:Value=serde_json::from_str(r#"{"id":"a","series":[{"uid":"1","instance":[{"uid":"2","sopClass":{"code":"3","system":"x"}}]}]}"#).unwrap();
        let mut second:Value=serde_json::from_str(r#"{"series":[{"instance":[{"sopClass":{"system":"x","code":"3"},"uid":"2"}],"uid":"1"}],"id":"a"}"#).unwrap();
        second["note"] = json!([{"text":"annotation"}]);
        assert_eq!(manifest_hash(&first), manifest_hash(&second));
        second["series"][0]["instance"][0]["uid"] = json!("9");
        assert_ne!(manifest_hash(&first), manifest_hash(&second));
    }
}
