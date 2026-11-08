//! Authorized, opt-in DICOMweb retrieval over trusted FHIR study manifests.
//!
//! # Contents
//! Closed route grammar, primary-backed authorization, constrained QIDO projection,
//! metadata rewriting and backpressured instance/frame/bulk retrieval.
//!
//! # Invariants
//! Every request reads the current Patient and attested ImagingStudy under its own
//! identity. Shared PACS credentials never authorize a study or expand membership.
//! Whole-study/series binary retrieval is intentionally absent: it could include
//! newly arrived, unindexed instances. Pixel bytes stream only to the caller.

mod config;
mod metadata;
mod path;
mod stream;
use super::{attestation, client::DicomWebClient, mapping};
use crate::{operations::OperationContext, server::AppState};
use axum::{
    Json, Router,
    extract::{OriginalUri, Path, State},
    http::{HeaderMap, HeaderValue, header},
    response::{IntoResponse, Response},
    routing::get,
};
pub use config::DicomGatewayConfig;
use octofhir_api::ApiError;
use path::{Kind, Target};
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc};

pub(crate) fn routes() -> Router<AppState> {
    Router::new().route("/dicomweb/{connection}/{*path}", get(handle))
}
fn missing() -> ApiError {
    ApiError::not_found("DICOM study is unavailable")
}
fn json_response(value: Value) -> Response {
    let mut response = Json(value).into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/dicom+json"),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        header::VARY,
        HeaderValue::from_static("Authorization, Accept"),
    );
    response
}
async fn handle(
    State(state): State<AppState>,
    context: OperationContext,
    Path((connection, path)): Path<(String, String)>,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    if context.auth().is_none() {
        return Err(ApiError::forbidden(
            "DICOMweb requires an authenticated FHIR identity",
        ));
    }
    let config = state
        .config
        .dicom
        .connections
        .get(&connection)
        .ok_or_else(missing)?;
    let gateway = config.gateway.as_ref().ok_or_else(missing)?;
    let permit = state
        .dicom_streams
        .clone()
        .try_acquire_owned()
        .map_err(|_| ApiError::conflict("DICOM gateway capacity is busy; retry later"))?;
    let mut query = BTreeMap::new();
    for (k, v) in url::form_urlencoded::parse(uri.query().unwrap_or("").as_bytes()) {
        if query.insert(k.into_owned(), v.into_owned()).is_some() {
            return Err(ApiError::bad_request("Duplicate DICOMweb query parameter"));
        }
    }
    let target = Target::parse(
        &path,
        query
            .get("StudyInstanceUID")
            .or_else(|| query.get("0020000D"))
            .map(String::as_str),
    )?;
    let (study, patient) =
        attestation::authorize(&state, &context, &connection, &target.study).await?;
    let members = members(&study.resource)?;
    let selected: Vec<_> = members
        .iter()
        .filter(|(series, instance, _)| {
            target.series.as_ref().is_none_or(|s| s == series)
                && target.instance.as_ref().is_none_or(|i| i == instance)
        })
        .collect();
    if selected.is_empty() {
        return Err(missing());
    }
    if matches!(target.kind, Kind::Studies | Kind::Series | Kind::Instances) {
        for key in query.keys() {
            if !matches!(
                key.as_str(),
                "StudyInstanceUID" | "0020000D" | "includefield" | "offset" | "limit"
            ) {
                return Err(ApiError::bad_request(
                    "Only exact authorized-study queries are supported",
                ));
            }
        }
        let offset = query
            .get("offset")
            .map(|v| v.parse::<usize>())
            .transpose()
            .map_err(|_| ApiError::bad_request("Invalid QIDO offset"))?
            .unwrap_or(0);
        let limit = query
            .get("limit")
            .map(|v| v.parse::<usize>())
            .transpose()
            .map_err(|_| ApiError::bad_request("Invalid QIDO limit"))?
            .unwrap_or(config.max_instances)
            .min(config.max_instances);
        let rows = projection(&target, &study.resource, &selected)?;
        return Ok(json_response(json!(
            rows.into_iter()
                .skip(offset)
                .take(limit)
                .collect::<Vec<_>>()
        )));
    }
    let policy = Arc::new(
        crate::egress::EgressPolicy::from_server_config(&state.config.server)
            .map_err(|_| ApiError::internal("Invalid DICOM egress policy"))?,
    );
    let client = DicomWebClient::new(config, policy)
        .map_err(|_| ApiError::internal("PACS is unavailable"))?;
    if target.kind == Kind::Metadata {
        if !query.is_empty() {
            return Err(ApiError::bad_request(
                "Metadata query parameters are unsupported",
            ));
        }
        // Fetch instance-specific metadata when possible. Filter broader snapshots to
        // attested membership so late arrivals never become an authorization expansion.
        let mut segments = vec!["studies", target.study.as_str()];
        if let Some(series) = &target.series {
            segments.extend(["series", series]);
        }
        if let Some(instance) = &target.instance {
            segments.extend(["instances", instance]);
        }
        segments.push("metadata");
        let datasets = client
            .metadata(client.resource_url(&segments).map_err(|_| missing())?)
            .await
            .map_err(|_| ApiError::internal("PACS metadata is unavailable"))?;
        let mut result = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        for mut dataset in datasets {
            let series = mapping::required(&dataset, "0020000E", "UI")
                .map_err(|_| missing())?
                .to_owned();
            let instance = mapping::required(&dataset, "00080018", "UI")
                .map_err(|_| missing())?
                .to_owned();
            if !selected.iter().any(|m| m.0 == series && m.1 == instance) {
                continue;
            }
            verify(
                &dataset,
                &target.study,
                &series,
                &instance,
                &patient.resource,
                config,
            )?;
            if !seen.insert((series.clone(), instance.clone())) {
                return Err(ApiError::internal("Duplicate PACS instance metadata"));
            }
            let base = client
                .resource_url(&[
                    "studies",
                    &target.study,
                    "series",
                    &series,
                    "instances",
                    &instance,
                    "metadata",
                ])
                .map_err(|_| missing())?;
            let root = format!(
                "/dicomweb/{connection}/studies/{}/series/{series}/instances/{instance}",
                target.study
            );
            // Resolve relative URIs against the request that actually returned them.
            let requested = client.resource_url(&segments).map_err(|_| missing())?;
            normalize_bulk(&mut dataset, &requested)?;
            metadata::rewrite(&mut dataset, &base, &root)?;
            result.push(dataset);
        }
        if result.len() != selected.len() {
            return Err(ApiError::conflict(
                "PACS manifest is incomplete; reconcile before retrieval",
            ));
        }
        return Ok(json_response(json!(result)));
    }
    // Before any pixel request, verify fresh source identity and the exact SOP tuple.
    let mut segments = target.instance_path();
    segments.push("metadata");
    let metadata_url = client.resource_url(&segments).map_err(|_| missing())?;
    let datasets = client
        .metadata(metadata_url.clone())
        .await
        .map_err(|_| ApiError::internal("PACS instance metadata is unavailable"))?;
    if datasets.len() != 1 {
        return Err(ApiError::conflict("PACS instance metadata is incomplete"));
    }
    verify(
        &datasets[0],
        &target.study,
        target.series.as_deref().unwrap(),
        target.instance.as_deref().unwrap(),
        &patient.resource,
        config,
    )?;
    let mut url = if let Kind::Bulk(token) = &target.kind {
        if !query.is_empty() {
            return Err(ApiError::bad_request(
                "Bulk data query parameters are unsupported",
            ));
        }
        metadata::lookup(&datasets[0], token, &metadata_url)?
    } else {
        let mut segments = target.instance_path();
        segments.extend(target.suffix.iter().map(String::as_str));
        client.resource_url(&segments).map_err(|_| missing())?
    };
    if !query.is_empty() {
        if !target
            .suffix
            .iter()
            .any(|s| s == "rendered" || s == "thumbnail")
        {
            return Err(ApiError::bad_request(
                "Query parameters require a rendered resource",
            ));
        }
        for (k, v) in &query {
            if !matches!(
                k.as_str(),
                "viewport" | "window" | "quality" | "iccprofile" | "annotation"
            ) || v.len() > 1024
            {
                return Err(ApiError::bad_request("Unsupported rendering parameter"));
            }
            url.query_pairs_mut().append_pair(k, v);
        }
    }
    stream::retrieve(&client, url, &headers, gateway, permit).await
}
fn members(study: &Value) -> Result<Vec<(String, String, Value)>, ApiError> {
    let mut result = Vec::new();
    for series in study["series"].as_array().ok_or_else(missing)? {
        for instance in series["instance"].as_array().ok_or_else(missing)? {
            result.push((
                series["uid"].as_str().ok_or_else(missing)?.into(),
                instance["uid"].as_str().ok_or_else(missing)?.into(),
                instance.clone(),
            ));
        }
    }
    Ok(result)
}
fn verify(
    dataset: &Value,
    study: &str,
    series: &str,
    instance: &str,
    patient: &Value,
    config: &super::DicomConnection,
) -> Result<(), ApiError> {
    for (tag, uid) in [
        ("0020000D", study),
        ("0020000E", series),
        ("00080018", instance),
    ] {
        if mapping::required(dataset, tag, "UI").map_err(|_| missing())? != uid {
            return Err(missing());
        }
    }
    mapping::patient_matches(dataset, patient, config).map_err(|_| {
        ApiError::conflict("PACS patient identity changed; reconcile before retrieval")
    })
}
fn normalize_bulk(dataset: &mut Value, base: &url::Url) -> Result<(), ApiError> {
    // serde_json already bounds nesting; only DICOM SQ children are traversed.
    fn walk(value: &mut Value, base: &url::Url, depth: usize) -> Result<(), ApiError> {
        if depth > 32 {
            return Err(ApiError::bad_request(
                "DICOM metadata nesting exceeds limit",
            ));
        }
        for attribute in value.as_object_mut().ok_or_else(missing)?.values_mut() {
            if let Some(uri) = attribute.get_mut("BulkDataURI") {
                let target = base
                    .join(uri.as_str().ok_or_else(missing)?)
                    .map_err(|_| missing())?;
                *uri = json!(target.as_str());
            }
            if attribute["vr"] == "SQ"
                && let Some(items) = attribute.get_mut("Value").and_then(Value::as_array_mut)
            {
                for item in items {
                    walk(item, base, depth + 1)?;
                }
            }
        }
        Ok(())
    }
    walk(dataset, base, 0)
}
fn projection(
    target: &Target,
    study: &Value,
    selected: &[&(String, String, Value)],
) -> Result<Vec<Value>, ApiError> {
    let uid = |v: &str| json!({"vr":"UI","Value":[v]});
    Ok(match target.kind {
        Kind::Studies => vec![json!({"0020000D":uid(&target.study),"00201208":{"vr":"IS","Value":[selected.len()]}})],
        Kind::Series => {
            let mut result = BTreeMap::new();
            for (series, _, _) in selected {
                let source = study["series"].as_array().unwrap().iter().find(|s| s["uid"] == *series).unwrap();
                let modality = source["modality"]["code"].as_str().or_else(|| source["modality"]["coding"][0]["code"].as_str()).unwrap_or("OT");
                result.insert(series.clone(), json!({"0020000D":uid(&target.study),"0020000E":uid(series),"00080060":{"vr":"CS","Value":[modality]},"00201209":{"vr":"IS","Value":[source["numberOfInstances"]]}}));
            }
            result.into_values().collect()
        },
        _ => selected.iter().map(|(series, instance, source)| {
            let class = super::mapping::sop_class(source).map_err(|_| missing())?;
            Ok(json!({"0020000D":uid(&target.study),"0020000E":uid(series),"00080018":uid(instance),"00080016":uid(class)}))
        }).collect::<Result<Vec<_>, ApiError>>()?,
    })
}
