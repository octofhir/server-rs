//! PACS matching scoped to an already authorized Study Instance UID.
//!
//! QIDO datasets supply selection keys and origin-selected response attributes.
//! Fresh manifest metadata supplies identity, counts and gateway bulk/retrieve URLs.
use super::super::metadata;
use super::{
    attributes::{self, Level},
    broad,
};
use crate::dicom::{DicomError, client::DicomWebClient, mapping, validate_uid};
use octofhir_api::ApiError;
use serde_json::Value;
use std::collections::BTreeMap;

pub(super) type Key = (String, String, String);

pub(super) fn project(
    verified: &Value,
    origin: &Value,
    level: Level,
) -> Result<serde_json::Map<String, Value>, ApiError> {
    let invalid = || ApiError::internal("Invalid PACS QIDO attributes");
    metadata::tokens(origin)?;
    for tag in ["00100020", "00100021"] {
        if let Some(value) = mapping::scalar(origin, tag, "LO").map_err(|_| invalid())?
            && mapping::scalar(verified, tag, "LO").map_err(|_| invalid())? != Some(value)
        {
            return Err(ApiError::conflict("PACS QIDO patient identity changed"));
        }
    }
    let mut result = serde_json::Map::new();
    for (tag, attribute) in origin.as_object().ok_or_else(invalid)? {
        if attributes::attribute(tag).is_some_and(|attribute| attribute.level > level) {
            continue;
        }
        let attribute = if let Some(verified) = verified.get(tag) {
            verified
        } else {
            // Bulk links need an instance metadata binding. Never return a
            // source URL that cannot be re-derived through the gateway.
            let wrapped = serde_json::json!({(tag): attribute});
            if !metadata::tokens(&wrapped)?.is_empty() {
                continue;
            }
            attribute
        };
        let vr = attribute["vr"].as_str().ok_or_else(invalid)?;
        if vr.len() != 2 || !vr.bytes().all(|byte| byte.is_ascii_uppercase()) {
            return Err(invalid());
        }
        result.insert(tag.clone(), attribute.clone());
    }
    Ok(result)
}

pub(super) fn key(row: &Value, level: Level) -> Result<Key, ApiError> {
    let uid = |tag| {
        let value = mapping::required(row, tag, "UI")
            .map_err(|_| ApiError::internal("Invalid PACS QIDO identity"))?;
        validate_uid(value).map_err(|_| ApiError::internal("Invalid PACS QIDO identity"))?;
        Ok::<_, ApiError>(value.to_owned())
    };
    Ok((
        uid("0020000D")?,
        if level >= Level::Series {
            uid("0020000E")?
        } else {
            String::new()
        },
        if level == Level::Instance {
            uid("00080018")?
        } else {
            String::new()
        },
    ))
}

pub(super) async fn selection(
    client: &DicomWebClient,
    query: &BTreeMap<String, String>,
    study: &str,
    series: Option<&str>,
    level: Level,
    max_rows: usize,
    max_bytes: usize,
) -> Result<(BTreeMap<Key, Value>, usize), ApiError> {
    // Intersect caller UID constraints before replacing them with the admitted
    // study. Never let a service credential turn this into an archive-wide scan.
    for name in ["StudyInstanceUID", "0020000D"] {
        if let Some(value) = query.get(name).filter(|v| !v.is_empty()) {
            for value in value.split(',') {
                validate_uid(value)
                    .map_err(|_| ApiError::bad_request("Invalid Study Instance UID"))?;
            }
            if !value.split(',').any(|value| value == study) {
                return Ok((BTreeMap::new(), 0));
            }
        }
    }
    let mut query = query.clone();
    query.remove("0020000D");
    if level == Level::Study {
        query.insert("StudyInstanceUID".into(), study.into());
    } else {
        // The hierarchy already constrains the study. Strict origins reject
        // redundant StudyInstanceUID matching keys on nested search resources.
        query.remove("StudyInstanceUID");
    }
    let identities = match level {
        Level::Study => "0020000D",
        Level::Series => "0020000D,0020000E",
        Level::Instance => "0020000D,0020000E,00080018",
    };
    query
        .entry("includefield".into())
        .and_modify(|fields| {
            if !fields.split(',').any(|field| field == "all") {
                if !fields.is_empty() {
                    fields.push(',');
                }
                fields.push_str(identities);
            }
        })
        .or_insert_with(|| identities.into());
    let segments = match (level, series) {
        (Level::Study, _) => vec!["studies"],
        (Level::Series, _) => vec!["studies", study, "series"],
        (Level::Instance, Some(series)) => vec!["studies", study, "series", series, "instances"],
        (Level::Instance, None) => vec!["studies", study, "instances"],
    };
    let mut keys = BTreeMap::new();
    let mut bytes = 0usize;
    let mut offset = 0usize;
    loop {
        let limit = max_rows.saturating_sub(offset).saturating_add(1).min(1000);
        let rows = client
            .query_page(&segments, &query, offset, limit)
            .await
            .map_err(|error| match error {
                DicomError::Limit => broad(),
                DicomError::Status(400) => ApiError::bad_request("PACS rejected QIDO parameters"),
                _ => ApiError::internal("PACS QIDO search is unavailable or incomplete"),
            })?;
        if rows.is_empty() {
            break;
        }
        bytes = bytes.saturating_add(serde_json::to_vec(&rows).map_err(|_| broad())?.len());
        offset = offset.saturating_add(rows.len());
        if bytes > max_bytes || offset > max_rows {
            return Err(broad());
        }
        for mut row in rows {
            // A nested resource path is an authenticated parent identity.
            // Some origins omit these parent return keys even when requested.
            if level != Level::Study
                && mapping::scalar(&row, "0020000D", "UI")
                    .map_err(|_| ApiError::internal("Invalid PACS QIDO identity"))?
                    .is_none()
            {
                row["0020000D"] = serde_json::json!({"vr":"UI","Value":[study]});
            }
            if level == Level::Instance
                && let Some(series) = series
                && mapping::scalar(&row, "0020000E", "UI")
                    .map_err(|_| ApiError::internal("Invalid PACS QIDO identity"))?
                    .is_none()
            {
                row["0020000E"] = serde_json::json!({"vr":"UI","Value":[series]});
            }
            let key = key(&row, level)?;
            if key.0 != study || series.is_some_and(|series| key.1 != series) {
                continue;
            }
            if keys.insert(key, row).is_some() {
                return Err(ApiError::internal("PACS QIDO pagination repeated a result"));
            }
        }
        // Origins may cap page sizes below our requested limit. An empty page
        // establishes completion; a short page alone does not.
    }
    Ok((keys, bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{dicom::DicomConnection, egress::EgressPolicy};
    use serde_json::json;
    use std::sync::Arc;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path, query_param},
    };

    fn row(study: &str, series: &str, instance: &str) -> Value {
        json!({"0020000D":{"vr":"UI","Value":[study]},
            "0020000E":{"vr":"UI","Value":[series]},
            "00080018":{"vr":"UI","Value":[instance]}})
    }
    fn client(server: &MockServer) -> DicomWebClient {
        let config: DicomConnection = serde_json::from_value(json!({
            "wado_url":"https://wado.example/unused",
            "qido_url":format!("{}/qido", server.uri()), "allow_http":true,
            "patient_identifier_systems":{"HOSPITAL":"urn:test:mrn"}
        }))
        .unwrap();
        DicomWebClient::new(&config, Arc::new(EgressPolicy::permissive())).unwrap()
    }

    #[test]
    fn origin_attributes_preserve_private_fields_sequences_and_verified_bulk_links() {
        let verified = json!({"00100020":{"vr":"LO","Value":["PATIENT"]},
            "00080080":{"vr":"LO","Value":["Verified institution"]},
            "00201208":{"vr":"IS","Value":[2]},"00081190":{"vr":"UR","Value":["https://gateway/study"]},
            "00400275":{"vr":"SQ","Value":[{"00401002":{"vr":"LO","Value":["Full nested value"]}}]},
            "77771010":{"vr":"OB","BulkDataURI":"https://gateway/instance/bulkdata/77771010"}});
        let mut origin = json!({"00100020":{"vr":"LO","Value":["PATIENT"]},
            "00080080":{"vr":"LO","Value":["Stale"]},"77770010":{"vr":"LO","Value":["Private value"]},
            "00201208":{"vr":"IS","Value":[999]},"00081190":{"vr":"UR","Value":["https://private-pacs/study"]},
            "00400275":{"vr":"SQ","Value":[{"00401002":{"vr":"LO","Value":["Stale"]}}]},
            "77771010":{"vr":"OB","BulkDataURI":"https://private-pacs/bulk"},
            "77771011":{"vr":"OB","BulkDataURI":"https://private-pacs/unbound"}});
        let result = Value::Object(project(&verified, &origin, Level::Instance).unwrap());
        for tag in ["00080080", "00201208", "00081190", "00400275", "77771010"] {
            assert_eq!(result[tag], verified[tag]);
        }
        assert_eq!(result["77770010"], origin["77770010"]);
        assert!(result.get("77771011").is_none());
        origin["00100020"]["Value"][0] = json!("OTHER");
        assert!(project(&verified, &origin, Level::Study).is_err());
    }

    #[tokio::test]
    async fn forwards_unicode_fuzzy_and_private_keys_at_each_search_level() {
        for (level, series, endpoint) in [
            (Level::Study, None, "/qido/studies"),
            (Level::Series, None, "/qido/studies/1.2.3/series"),
            (Level::Instance, None, "/qido/studies/1.2.3/instances"),
            (
                Level::Instance,
                Some("1.2.3.1"),
                "/qido/studies/1.2.3/series/1.2.3.1/instances",
            ),
        ] {
            let server = MockServer::start().await;
            let fuzzy = if level == Level::Study { "true" } else { "yes" };
            let included = match level {
                Level::Study => "PatientName,0020000D",
                Level::Series => "PatientName,0020000D,0020000E",
                Level::Instance => "PatientName,0020000D,0020000E,00080018",
            };
            let query = BTreeMap::from([
                ("PatientName".into(), "Иванов^Иван".into()),
                ("fuzzymatching".into(), fuzzy.into()),
                ("PrivateOriginFilter".into(), "a+b&c".into()),
                ("includefield".into(), "PatientName".into()),
            ]);
            let mut matching_row = row("1.2.3", "1.2.3.1", "1.2.3.1.1");
            if level != Level::Study {
                matching_row.as_object_mut().unwrap().remove("0020000D");
            }
            if series.is_some() {
                matching_row.as_object_mut().unwrap().remove("0020000E");
            }
            Mock::given(method("GET"))
                .and(path(endpoint))
                .and(query_param("offset", "0"))
                .and(move |request: &wiremock::Request| {
                    let study = request
                        .url
                        .query_pairs()
                        .find(|(name, _)| name == "StudyInstanceUID")
                        .map(|(_, value)| value.into_owned());
                    if level == Level::Study {
                        study.as_deref() == Some("1.2.3")
                    } else {
                        study.is_none()
                    }
                })
                .and(query_param("PatientName", "Иванов^Иван"))
                .and(query_param("fuzzymatching", fuzzy))
                .and(query_param("PrivateOriginFilter", "a+b&c"))
                .and(query_param("includefield", included))
                .respond_with(ResponseTemplate::new(200).set_body_raw(
                    json!([matching_row, row("9.8.7", "9.8.7.1", "9.8.7.1.1")]).to_string(),
                    "application/dicom+json",
                ))
                .expect(1)
                .mount(&server)
                .await;
            Mock::given(method("GET"))
                .and(path(endpoint))
                .and(query_param("offset", "2"))
                .respond_with(ResponseTemplate::new(204))
                .expect(1)
                .mount(&server)
                .await;
            let (selected, bytes) =
                selection(&client(&server), &query, "1.2.3", series, level, 10, 65536)
                    .await
                    .unwrap();
            assert_eq!(
                selected,
                BTreeMap::from([(
                    key(&row("1.2.3", "1.2.3.1", "1.2.3.1.1"), level).unwrap(),
                    row("1.2.3", "1.2.3.1", "1.2.3.1.1")
                )])
            );
            assert!(bytes > 0);
        }
    }

    #[tokio::test]
    async fn rejects_repeated_partial_and_over_budget_origin_results() {
        for (response, rows, bytes, status) in [
            (
                ResponseTemplate::new(200).set_body_raw(
                    json!([row("1.2.3", "1.2.3.1", "1.2.3.1.1")]).to_string(),
                    "application/dicom+json",
                ),
                10,
                65536,
                500,
            ),
            (ResponseTemplate::new(206), 10, 65536, 500),
            (ResponseTemplate::new(400), 10, 65536, 400),
            (
                ResponseTemplate::new(200).set_body_raw(
                    json!([row("1.2.3", "1.2.3.1", "1.2.3.1.1")]).to_string(),
                    "application/dicom+json",
                ),
                0,
                65536,
                413,
            ),
            (
                ResponseTemplate::new(200).set_body_raw(
                    json!([row("1.2.3", "1.2.3.1", "1.2.3.1.1")]).to_string(),
                    "application/dicom+json",
                ),
                10,
                1,
                413,
            ),
        ] {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .respond_with(response)
                .mount(&server)
                .await;
            let error = selection(
                &client(&server),
                &BTreeMap::new(),
                "1.2.3",
                None,
                Level::Study,
                rows,
                bytes,
            )
            .await
            .unwrap_err();
            assert_eq!(error.status_code().as_u16(), status);
        }
    }

    #[tokio::test]
    async fn intersects_conflicting_study_constraints_without_contacting_pacs() {
        let server = MockServer::start().await;
        let query = BTreeMap::from([("0020000D".into(), "9.8.7".into())]);
        let (keys, _) = selection(
            &client(&server),
            &query,
            "1.2.3",
            None,
            Level::Study,
            10,
            65536,
        )
        .await
        .unwrap();
        assert!(keys.is_empty());
        assert!(server.received_requests().await.unwrap().is_empty());
    }
}
