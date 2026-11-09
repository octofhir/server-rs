//! QIDO over authorized, attested FHIR studies and bounded fresh PACS metadata.
//!
//! # Contents
//! Six standard search routes, matching, inclusion, projections and pagination.
//! # Invariants
//! Searches never enumerate PACS with its service credential. Trusted FHIR search
//! predicates select candidates; primary authorization and manifest proofs precede
//! metadata disclosure. Broad scans fail explicitly instead of silently omitting
//! matches. No DICOM datasets are stored or used as a durable cursor.
mod attributes;
mod matching;
mod remote;
use super::{
    access::{self, Manifest},
    encoding, missing,
    negotiation::Preferences,
    path::{Kind, Target},
};
use crate::{
    dicom::{client::DicomWebClient, mapping},
    operations::{OperationContext, OperationError},
    server::AppState,
};
use attributes::{ATTRIBUTES, Level};
use axum::{
    http::{HeaderValue, StatusCode, header},
    response::Response,
};
use matching::{Filter, strings};
use octofhir_api::ApiError;
use octofhir_storage::{SearchParams, SortParam};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn parse(value: Option<&str>) -> Result<BTreeMap<String, String>, ApiError> {
    let value = value.unwrap_or("");
    if value.len() > 8192 {
        return Err(bad("DICOM query is too large"));
    }
    let mut result = BTreeMap::<String, String>::new();
    for (key, value) in url::form_urlencoded::parse(value.as_bytes()) {
        if key.len() > 128 || value.len() > 4096 || result.len() > 64 {
            return Err(bad("DICOM query is too large"));
        }
        if let Some(old) = result.get_mut(key.as_ref()) {
            if key != "includefield" {
                return Err(bad("Duplicate DICOM query parameter"));
            }
            old.push(',');
            old.push_str(&value);
        } else {
            result.insert(key.into_owned(), value.into_owned());
        }
    }
    Ok(result)
}
fn bad(message: &str) -> ApiError {
    ApiError::bad_request(message)
}
fn broad() -> ApiError {
    ApiError::issues(
        StatusCode::PAYLOAD_TOO_LARGE,
        octofhir_api::OperationOutcome::single(
            "error",
            "too-costly",
            "DICOM query exceeds configured metadata budget; refine the study selection",
        )
        .issue,
    )
}
fn error(error: OperationError) -> ApiError {
    match error {
        OperationError::Api(error) => error,
        _ => ApiError::internal("DICOM search authorization failed"),
    }
}
fn number(
    query: &mut BTreeMap<String, String>,
    name: &str,
    default: usize,
) -> Result<usize, ApiError> {
    query
        .remove(name)
        .map(|v| {
            v.parse::<usize>()
                .map_err(|_| bad("Invalid QIDO pagination"))
        })
        .unwrap_or(Ok(default))
}
fn project(
    row: &Value,
    level: Level,
    parent_defaults: Level,
    include: &BTreeSet<String>,
    all: bool,
) -> Value {
    let mut result = serde_json::Map::new();
    for attr in ATTRIBUTES.iter().filter(|a| a.level <= level) {
        let shared = matches!(attr.tag, "00080005" | "00080201")
            || (attr.tag == "00080056" && level == Level::Instance);
        let default = (attr.level >= parent_defaults || shared) && attr.default > 0;
        let requested = (all && attr.level == level)
            || include.contains(attr.tag)
            || matches!(attr.tag, "0020000D" | "0020000E")
            || attr.tag == "00081190";
        if !default && !requested {
            continue;
        }
        if let Some(value) = row.get(attr.tag) {
            let mut value = value.clone();
            if attr.tag == "00400275"
                && let Some(items) = value.get_mut("Value").and_then(Value::as_array_mut)
            {
                for item in items {
                    if let Some(fields) = item.as_object_mut() {
                        fields.retain(|key, _| matches!(key.as_str(), "00400009" | "00401001"));
                        for tag in ["00400009", "00401001"] {
                            fields.entry(tag).or_insert_with(|| json!({"vr":"SH"}));
                        }
                    }
                }
            }
            result.insert(attr.tag.into(), value);
        } else if default && attr.default == 1 {
            result.insert(attr.tag.into(), json!({"vr":attr.vr}));
        }
    }
    Value::Object(result)
}
pub(super) async fn search(
    state: &AppState,
    context: &OperationContext,
    connection: &str,
    target: &Target,
    mut query: BTreeMap<String, String>,
    preferences: &Preferences,
) -> Result<Response, ApiError> {
    let config = &state.config.dicom.connections[connection];
    let gateway = config.gateway.as_ref().ok_or_else(missing)?;
    let offset = number(&mut query, "offset", 0)?;
    let limit = number(&mut query, "limit", config.max_instances)?.min(config.max_instances);
    if limit == 0 {
        return Err(bad("QIDO limit must be positive"));
    }
    let page_query = query.clone();
    let remote_query = config.qido_url.as_ref().map(|_| query.clone());
    let mut all = false;
    let mut include = BTreeSet::new();
    if let Some(fields) = query.remove("includefield") {
        for field in fields.split(',') {
            if field == "all" {
                all = true;
            } else if let Some(path) = attributes::path(field) {
                include.insert(path[0].clone());
            } else if remote_query.is_none() {
                return Err(bad("Unsupported DICOM include attribute"));
            }
        }
    }
    let level = match target.kind {
        Kind::Studies => Level::Study,
        Kind::Series => Level::Series,
        _ => Level::Instance,
    };
    if remote_query.is_none() {
        if let Some(value) = query.remove("fuzzymatching")
            && value != "false"
        {
            return Err(bad("Fuzzy person-name matching requires qido_url"));
        }
        for key in query.keys() {
            let path =
                attributes::path(key).ok_or_else(|| bad("Unsupported DICOM matching attribute"))?;
            if attributes::attribute(&path[0]).is_some_and(|a| a.level > level) {
                return Err(bad(
                    "Matching attribute belongs to a lower DICOM information level",
                ));
            }
        }
    }
    let filters = if remote_query.is_some() {
        Vec::new()
    } else {
        query
            .iter()
            .map(|(key, value)| Filter::new(key, value))
            .collect::<Result<Vec<_>, _>>()?
    };
    let mut requested = BTreeSet::new();
    if !target.study.is_empty() {
        requested.insert(target.study.clone());
    } else if let Some(value) = query
        .get("StudyInstanceUID")
        .or_else(|| query.get("0020000D"))
        .filter(|v| !v.is_empty())
    {
        for uid in value.split(',') {
            crate::dicom::validate_uid(uid).map_err(|_| bad("Invalid Study Instance UID"))?;
            requested.insert(uid.to_owned());
        }
    }
    let uids = if requested.is_empty() {
        let mut params = SearchParams {
            count: Some(gateway.max_query_studies + 1),
            sort: Some(vec![SortParam::new("_id", false)]),
            ..Default::default()
        };
        params.parameters.insert(
            "_tag".into(),
            vec![format!("{}|{}", mapping::SOURCE_TAG, connection)],
        );
        let result = context
            .search_primary(state, "ImagingStudy", &params)
            .await
            .map_err(error)?;
        if result.has_more || result.entries.len() > gateway.max_query_studies as usize {
            return Err(broad());
        }
        result
            .entries
            .iter()
            .filter_map(|r| r.resource["identifier"].as_array())
            .flatten()
            .filter(|i| i["system"] == "urn:dicom:uid")
            .filter_map(|i| {
                i["value"]
                    .as_str()?
                    .strip_prefix("urn:oid:")
                    .map(str::to_owned)
            })
            .collect::<BTreeSet<_>>()
    } else {
        requested
    };
    if uids.len() > gateway.max_query_studies as usize {
        return Err(broad());
    }
    let policy = std::sync::Arc::new(
        crate::egress::EgressPolicy::from_server_config(&state.config.server)
            .map_err(|_| ApiError::internal("Invalid DICOM egress policy"))?,
    );
    let client = DicomWebClient::new(config, policy)
        .map_err(|_| ApiError::internal("PACS is unavailable"))?;
    let parent_defaults = if target.study.is_empty() {
        Level::Study
    } else if target.series.is_none() && level == Level::Instance {
        Level::Series
    } else {
        level
    };
    let mut rows = BTreeMap::<(String, String, String), Value>::new();
    let mut bytes = 0usize;
    for uid in uids {
        let full = access::study_target(&uid);
        let manifest = match Manifest::load(state, context, connection, &full).await {
            Ok(value) => value,
            Err(error) if error.status_code() == StatusCode::NOT_FOUND => continue,
            Err(error) => return Err(error),
        };
        let selected = if let Some(query) = &remote_query {
            let (keys, used) = remote::selection(
                &client,
                query,
                &uid,
                target.series.as_deref(),
                level,
                config.max_instances,
                gateway.max_query_bytes.saturating_sub(bytes),
            )
            .await?;
            bytes = bytes.saturating_add(used);
            if keys.is_empty() {
                continue;
            }
            Some(keys)
        } else {
            None
        };
        let mut datasets = manifest
            .metadata(
                &client,
                config,
                &full,
                &format!(
                    "{}/dicomweb/{connection}",
                    state.base_url.trim_end_matches('/')
                ),
                remote_query.is_some(),
            )
            .await?;
        bytes = bytes.saturating_add(serde_json::to_vec(&datasets).map_err(|_| broad())?.len());
        if bytes > gateway.max_query_bytes {
            return Err(broad());
        }
        let modalities = datasets
            .iter()
            .flat_map(|r| strings(r, &["00080060".into()]))
            .collect::<BTreeSet<_>>();
        let mut series_counts = BTreeMap::<String, usize>::new();
        for instance in &manifest.instances {
            *series_counts.entry(instance.series.clone()).or_default() += 1;
        }
        for row in &mut datasets {
            let series = mapping::required(row, "0020000E", "UI")
                .map_err(|_| missing())?
                .to_owned();
            let instance = mapping::required(row, "00080018", "UI")
                .map_err(|_| missing())?
                .to_owned();
            if target.series.as_ref().is_some_and(|s| s != &series) {
                continue;
            }
            row["00080061"] = json!({"vr":"CS","Value":modalities});
            row["00201206"] = json!({"vr":"IS","Value":[series_counts.len()]});
            row["00201208"] = json!({"vr":"IS","Value":[manifest.instances.len()]});
            row["00201209"] = json!({"vr":"IS","Value":[series_counts[&series]]});
            if !matching::all(&filters, row) {
                continue;
            }
            if let Some(selected) = &selected
                && !selected.contains_key(&remote::key(row, level)?)
            {
                continue;
            }
            let mut retrieve = format!(
                "{}/dicomweb/{connection}/studies/{uid}",
                state.base_url.trim_end_matches('/')
            );
            if level >= Level::Series {
                retrieve.push_str(&format!("/series/{series}"));
            }
            if level == Level::Instance {
                retrieve.push_str(&format!("/instances/{instance}"));
            }
            row["00081190"] = json!({"vr":"UR","Value":[retrieve]});
            let key = (
                uid.clone(),
                if level >= Level::Series {
                    series
                } else {
                    String::new()
                },
                if level == Level::Instance {
                    instance
                } else {
                    String::new()
                },
            );
            if let std::collections::btree_map::Entry::Vacant(entry) = rows.entry(key) {
                let mut projected = project(row, level, parent_defaults, &include, all);
                if let Some(selected) = &selected
                    && let Some(origin) = selected.get(&remote::key(row, level)?)
                {
                    projected
                        .as_object_mut()
                        .expect("projected dataset")
                        .extend(remote::project(row, origin, level)?);
                }
                bytes = bytes
                    .saturating_add(serde_json::to_vec(&projected).map_err(|_| broad())?.len());
                if bytes > gateway.max_query_bytes {
                    return Err(broad());
                }
                entry.insert(projected);
            }
            if rows.len() > config.max_instances.saturating_add(offset) {
                return Err(broad());
            }
        }
    }
    let more = rows.len() > offset.saturating_add(limit);
    let values = rows
        .into_values()
        .skip(offset)
        .take(limit)
        .collect::<Vec<_>>();
    let mut response = encoding::response(&values, preferences, gateway.max_query_bytes)?;
    if more {
        let url = format!(
            "{}/dicomweb/{connection}/search-status/more-results",
            state.base_url.trim_end_matches('/')
        );
        response.headers_mut().insert(
            "warning",
            HeaderValue::from_str(&url)
                .map_err(|_| ApiError::internal("Invalid DICOM service URL"))?,
        );
        let mut next = page_query;
        next.insert("offset".into(), offset.saturating_add(limit).to_string());
        next.insert("limit".into(), limit.to_string());
        let query = url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs(next)
            .finish();
        // The response is already encoded; this link only supplies pagination.
        let path = match target.kind {
            Kind::Studies => "studies".to_owned(),
            Kind::Series if target.study.is_empty() => "series".into(),
            Kind::Series => format!("studies/{}/series", target.study),
            _ if target.study.is_empty() => "instances".into(),
            _ if target.series.is_none() => format!("studies/{}/instances", target.study),
            _ => format!(
                "studies/{}/series/{}/instances",
                target.study,
                target.series.as_deref().unwrap()
            ),
        };
        response.headers_mut().insert(
            header::LINK,
            HeaderValue::from_str(&format!(
                "<{}/dicomweb/{connection}/{path}?{query}>; rel=\"next\"",
                state.base_url.trim_end_matches('/')
            ))
            .map_err(|_| ApiError::internal("Invalid DICOM pagination URL"))?,
        );
    }
    Ok(response)
}
