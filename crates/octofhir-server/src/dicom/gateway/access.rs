//! Authorized manifest selection and fresh PACS identity checks.
//!
//! # Contents
//! Primary-backed study admission, immutable instance tuples and metadata snapshots.
//! # Invariants
//! Every disclosed dataset belongs to the attested manifest and pinned Patient.
//! Late PACS arrivals never enlarge membership, including collection retrieval.
use super::{
    metadata, missing,
    path::{Kind, Target},
};
use crate::{
    dicom::{DicomConnection, DicomError, attestation, client::DicomWebClient, mapping},
    operations::OperationContext,
    server::AppState,
};
use octofhir_api::ApiError;
use octofhir_storage::StoredResource;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone)]
pub(super) struct Instance {
    pub study: String,
    pub series: String,
    pub uid: String,
    pub class: String,
}
impl Instance {
    pub fn path(&self) -> Vec<&str> {
        vec![
            "studies",
            &self.study,
            "series",
            &self.series,
            "instances",
            &self.uid,
        ]
    }
    pub fn gateway(&self, service: &str) -> String {
        format!(
            "{service}/studies/{}/series/{}/instances/{}",
            self.study, self.series, self.uid
        )
    }
}
pub(super) struct Manifest {
    pub patient: StoredResource,
    pub instances: Vec<Instance>,
}
impl Manifest {
    pub async fn load(
        state: &AppState,
        context: &OperationContext,
        connection: &str,
        target: &Target,
    ) -> Result<Self, ApiError> {
        let (study, patient) =
            attestation::authorize(state, context, connection, &target.study).await?;
        let mut instances = vec![];
        let mut seen = BTreeSet::new();
        for series in study.resource["series"].as_array().ok_or_else(missing)? {
            let series_uid = series["uid"].as_str().ok_or_else(missing)?;
            for instance in series["instance"].as_array().ok_or_else(missing)? {
                let uid = instance["uid"].as_str().ok_or_else(missing)?;
                if !seen.insert((series_uid.to_owned(), uid.to_owned())) {
                    return Err(missing());
                }
                if target.series.as_ref().is_none_or(|s| s == series_uid)
                    && target.instance.as_ref().is_none_or(|i| i == uid)
                {
                    instances.push(Instance {
                        study: target.study.clone(),
                        series: series_uid.into(),
                        uid: uid.into(),
                        class: mapping::sop_class(instance).map_err(|_| missing())?.into(),
                    });
                }
            }
        }
        instances.sort_by(|a, b| (&a.series, &a.uid).cmp(&(&b.series, &b.uid)));
        if instances.is_empty() {
            return Err(missing());
        }
        Ok(Self { patient, instances })
    }
    pub async fn metadata(
        &self,
        client: &DicomWebClient,
        config: &DicomConnection,
        target: &Target,
        gateway_base: &str,
        rewrite: bool,
    ) -> Result<Vec<Value>, ApiError> {
        let mut segments = vec!["studies", target.study.as_str()];
        if let Some(series) = &target.series {
            segments.extend(["series", series]);
        }
        if let Some(instance) = &target.instance {
            segments.extend(["instances", instance]);
        }
        segments.push("metadata");
        let requested = client.resource_url(&segments).map_err(|_| missing())?;
        let datasets = if target.instance.is_some() {
            client.single_metadata(requested.clone()).await
        } else {
            client.metadata(requested.clone()).await
        }
        .map_err(|error| metadata_error(error, "PACS metadata is unavailable"))?;
        let selected: BTreeMap<_, _> = self
            .instances
            .iter()
            .map(|i| ((i.series.as_str(), i.uid.as_str()), i))
            .collect();
        let mut result = BTreeMap::new();
        for mut dataset in datasets {
            let series = mapping::required(&dataset, "0020000E", "UI")
                .map_err(|_| missing())?
                .to_owned();
            let uid = mapping::required(&dataset, "00080018", "UI")
                .map_err(|_| missing())?
                .to_owned();
            let Some(instance) = selected.get(&(series.as_str(), uid.as_str())) else {
                continue;
            };
            verify(&dataset, instance, &self.patient.resource, config)?;
            if rewrite {
                normalize_bulk(&mut dataset, &requested, 0)?;
                let mut path = instance.path();
                path.push("metadata");
                let base = client.resource_url(&path).map_err(|_| missing())?;
                metadata::rewrite(
                    &mut dataset,
                    &base,
                    &client.resource_url(&[]).map_err(|_| missing())?,
                    &instance.gateway(gateway_base),
                )?;
            }
            if result.insert((series, uid), dataset).is_some() {
                return Err(ApiError::internal("Duplicate PACS instance metadata"));
            }
        }
        if result.len() != selected.len() {
            return Err(ApiError::conflict(
                "PACS manifest is incomplete; reconcile before retrieval",
            ));
        }
        Ok(result.into_values().collect())
    }
}
pub(super) async fn fresh(
    client: &DicomWebClient,
    config: &DicomConnection,
    instance: &Instance,
    patient: &Value,
) -> Result<(Value, url::Url), ApiError> {
    let mut path = instance.path();
    path.push("metadata");
    let url = client.resource_url(&path).map_err(|_| missing())?;
    let mut datasets = client
        .single_metadata(url.clone())
        .await
        .map_err(|error| metadata_error(error, "PACS instance metadata is unavailable"))?;
    if datasets.len() != 1 {
        return Err(ApiError::conflict("PACS instance metadata is incomplete"));
    }
    let dataset = datasets.pop().expect("one dataset");
    verify(&dataset, instance, patient, config)?;
    Ok((dataset, url))
}
fn metadata_error(error: DicomError, unavailable: &str) -> ApiError {
    match error {
        DicomError::Limit => super::metadata_limit(
            "PACS metadata exceeds max_metadata_bytes or max_instances for this DICOM connection",
        ),
        _ => ApiError::internal(unavailable),
    }
}
fn verify(
    dataset: &Value,
    instance: &Instance,
    patient: &Value,
    config: &DicomConnection,
) -> Result<(), ApiError> {
    for (tag, uid) in [
        ("0020000D", &instance.study),
        ("0020000E", &instance.series),
        ("00080018", &instance.uid),
        ("00080016", &instance.class),
    ] {
        if mapping::required(dataset, tag, "UI").map_err(|_| missing())? != uid {
            return Err(missing());
        }
    }
    mapping::patient_matches(dataset, patient, config).map_err(|_| {
        ApiError::conflict("PACS patient identity changed; reconcile before retrieval")
    })
}
fn normalize_bulk(value: &mut Value, base: &url::Url, depth: usize) -> Result<(), ApiError> {
    if depth > 32 {
        return Err(ApiError::bad_request(
            "DICOM metadata nesting exceeds limit",
        ));
    }
    for attribute in value.as_object_mut().ok_or_else(missing)?.values_mut() {
        if let Some(uri) = attribute.get_mut("BulkDataURI") {
            *uri = json!(
                base.join(uri.as_str().ok_or_else(missing)?)
                    .map_err(|_| missing())?
                    .as_str()
            );
        }
        if attribute["vr"] == "SQ"
            && let Some(items) = attribute.get_mut("Value").and_then(Value::as_array_mut)
        {
            for item in items {
                normalize_bulk(item, base, depth + 1)?;
            }
        }
    }
    Ok(())
}
pub(super) fn study_target(uid: &str) -> Target {
    Target {
        study: uid.into(),
        series: None,
        instance: None,
        suffix: vec![],
        kind: Kind::Metadata,
    }
}
