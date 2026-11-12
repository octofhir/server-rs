//! Guarded DICOMweb metadata and streaming retrieval using the egress policy.
//!
//! # Invariants
//! Only the configured origin is contacted; redirects and environment HTTP proxies
//! are disabled. Response bodies and request URLs never appear in errors. Pixel
//! data is requested only by the authorized gateway. Metadata limits apply to
//! chunked responses as well as Content-Length; the gateway bounds streamed bytes.

use std::{sync::Arc, time::Duration};

use serde_json::Value;

use super::{DicomConnection, DicomError, validate_uid};
use crate::egress::EgressPolicy;

pub(crate) struct DicomWebClient {
    client: reqwest::Client,
    config: DicomConnection,
    policy: Arc<EgressPolicy>,
}

pub(crate) struct QueryStudy {
    pub uid: String,
    pub instances: Option<u64>,
    pub dataset: Value,
}

impl DicomWebClient {
    pub(crate) fn new(
        config: &DicomConnection,
        policy: Arc<EgressPolicy>,
    ) -> Result<Self, DicomError> {
        let url = config.base_url().map_err(DicomError::Configuration)?;
        policy.check_url(&url).map_err(DicomError::Configuration)?;
        let mut headers = reqwest::header::HeaderMap::new();
        if let Some(variable) = &config.bearer_token_env {
            let token = std::env::var(variable).map_err(|_| {
                DicomError::Configuration(
                    "PACS credential environment variable is unavailable".into(),
                )
            })?;
            if token.trim().is_empty() {
                return Err(DicomError::Configuration(
                    "PACS bearer credential is empty".into(),
                ));
            }
            let mut value = reqwest::header::HeaderValue::from_str(&format!("Bearer {token}"))
                .map_err(|_| {
                    DicomError::Configuration("PACS bearer credential is invalid".into())
                })?;
            value.set_sensitive(true);
            headers.insert(reqwest::header::AUTHORIZATION, value);
        }
        let client = policy
            .client_builder()
            .no_gzip()
            .no_brotli()
            .no_deflate()
            .no_zstd()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(config.timeout_seconds))
            .default_headers(headers)
            .build()
            .map_err(|_| DicomError::Transport)?;
        Ok(Self {
            client,
            config: config.clone(),
            policy,
        })
    }

    pub(crate) fn resource_url(&self, segments: &[&str]) -> Result<url::Url, DicomError> {
        let mut url = self.config.base_url().map_err(DicomError::Configuration)?;
        url.path_segments_mut()
            .map_err(|_| DicomError::Transport)?
            .pop_if_empty()
            .extend(segments.iter().copied());
        Ok(url)
    }
    pub(crate) async fn metadata(&self, url: url::Url) -> Result<Vec<Value>, DicomError> {
        self.datasets(url, false, self.config.max_instances).await
    }
    pub(crate) async fn single_metadata(&self, url: url::Url) -> Result<Vec<Value>, DicomError> {
        self.datasets_kind(url, false, 1, true).await
    }
    pub(crate) async fn query_page(
        &self,
        segments: &[&str],
        query: &std::collections::BTreeMap<String, String>,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<Value>, DicomError> {
        let base = self
            .config
            .qido_url
            .as_deref()
            .ok_or_else(|| DicomError::Configuration("QIDO-RS is not configured".into()))?;
        let mut url = self
            .config
            .service_url(base)
            .map_err(DicomError::Configuration)?;
        url.path_segments_mut()
            .map_err(|_| DicomError::Transport)?
            .pop_if_empty()
            .extend(segments.iter().copied());
        url.query_pairs_mut()
            .extend_pairs(query)
            .append_pair("offset", &offset.to_string())
            .append_pair("limit", &limit.to_string());
        self.datasets(url, true, limit).await
    }
    pub(crate) async fn stream(
        &self,
        url: url::Url,
        headers: reqwest::header::HeaderMap,
        seconds: u64,
    ) -> Result<reqwest::Response, DicomError> {
        self.policy
            .check_url(&url)
            .map_err(DicomError::Configuration)?;
        self.client
            .get(url)
            .headers(headers)
            .timeout(Duration::from_secs(seconds))
            .send()
            .await
            .map_err(|_| DicomError::Transport)
    }

    /// Metadata-only UPS control request; never follows redirects or reads an
    /// unbounded/remote error body. Dropping the response cancels any such body.
    pub(crate) async fn command(
        &self,
        method: reqwest::Method,
        url: url::Url,
        payload: Option<&Value>,
    ) -> Result<(u16, Option<String>), DicomError> {
        self.policy
            .check_url(&url)
            .map_err(DicomError::Configuration)?;
        let mut request = self
            .client
            .request(method, url)
            .header(reqwest::header::ACCEPT, "application/dicom+json")
            .header(reqwest::header::CONTENT_TYPE, "application/dicom+json");
        if let Some(payload) = payload {
            let bytes = serde_json::to_vec(payload)
                .map_err(|_| DicomError::Metadata("Invalid control payload".into()))?;
            if bytes.len() > self.config.max_metadata_bytes {
                return Err(DicomError::Limit);
            }
            request = request.body(bytes);
        }
        let response = request.send().await.map_err(|_| DicomError::Transport)?;
        let location = response
            .headers()
            .get(reqwest::header::LOCATION)
            .map(|value| {
                value
                    .to_str()
                    .map(String::from)
                    .map_err(|_| DicomError::Transport)
            })
            .transpose()?;
        Ok((response.status().as_u16(), location))
    }

    pub(crate) async fn study_metadata(&self, uid: &str) -> Result<Vec<Value>, DicomError> {
        validate_uid(uid)?;
        let mut url = self.config.base_url().map_err(DicomError::Configuration)?;
        url.path_segments_mut()
            .map_err(|_| DicomError::Configuration("invalid service root".into()))?
            .pop_if_empty()
            .extend(["studies", uid, "metadata"]);
        self.datasets(url, false, self.config.max_instances).await
    }

    pub(crate) async fn query_studies(
        &self,
        offset: u64,
        limit: u32,
        patient: Option<(&str, &str)>,
    ) -> Result<Vec<QueryStudy>, DicomError> {
        let base = self
            .config
            .qido_url
            .as_deref()
            .ok_or_else(|| DicomError::Configuration("QIDO-RS is not configured".into()))?;
        let mut url = self
            .config
            .service_url(base)
            .map_err(DicomError::Configuration)?;
        url.path_segments_mut()
            .map_err(|_| DicomError::Configuration("Invalid QIDO root".into()))?
            .pop_if_empty()
            .push("studies");
        {
            let mut query = url.query_pairs_mut();
            query
                .append_pair("offset", &offset.to_string())
                .append_pair("limit", &limit.to_string())
                .append_pair("includefield", "0020000D,00201208,00100020,00100021");
            if let Some((id, issuer)) = patient {
                if id.contains(['*', '?', '\\']) || issuer.contains(['*', '?', '\\']) {
                    return Err(DicomError::Metadata("Patient identifiers containing DICOM wildcards require explicit UID import".into()));
                }
                query
                    .append_pair("PatientID", id)
                    .append_pair("IssuerOfPatientID", issuer);
            }
        }
        let datasets = self.datasets(url, true, limit as usize).await?;
        let mut seen = std::collections::BTreeSet::new();
        let mut studies = datasets
            .into_iter()
            .map(|dataset| {
                let uid = super::mapping::required(&dataset, "0020000D", "UI")?.to_owned();
                validate_uid(&uid)?;
                if !seen.insert(uid.clone()) {
                    return Err(DicomError::Metadata(
                        "Duplicate study in QIDO response".into(),
                    ));
                }
                let instances = match dataset.get("00201208") {
                    None => None,
                    Some(attribute) if attribute["vr"] == "IS" => {
                        let values = attribute["Value"]
                            .as_array()
                            .filter(|v| v.len() == 1)
                            .ok_or_else(|| {
                                DicomError::Metadata("Invalid QIDO instance count".into())
                            })?;
                        Some(
                            values[0]
                                .as_u64()
                                .or_else(|| values[0].as_str().and_then(|v| v.parse().ok()))
                                .filter(|n| *n <= 1_000_000_000)
                                .ok_or_else(|| {
                                    DicomError::Metadata("Invalid QIDO instance count".into())
                                })?,
                        )
                    }
                    Some(_) => {
                        return Err(DicomError::Metadata("Invalid QIDO instance count".into()));
                    }
                };
                Ok(QueryStudy {
                    uid,
                    instances,
                    dataset,
                })
            })
            .collect::<Result<Vec<_>, DicomError>>()?;
        for study in &mut studies {
            let missing_id = super::mapping::scalar(&study.dataset, "00100020", "LO")?.is_none();
            let missing_issuer = super::mapping::scalar(&study.dataset, "00100021", "LO")?
                .is_none()
                && self.config.default_patient_issuer.is_none();
            if !missing_id && !missing_issuer {
                continue;
            }
            // Some origins omit requested identity fields from QIDO. Recover
            // them only from a complete, internally consistent WADO snapshot.
            let metadata = self.study_metadata(&study.uid).await?;
            let first = &metadata[0];
            let identity = super::mapping::patient_identifier(first, &self.config)?;
            for instance in &metadata {
                if super::mapping::required(instance, "0020000D", "UI")? != study.uid
                    || super::mapping::patient_identifier(instance, &self.config)? != identity
                {
                    return Err(DicomError::Metadata(
                        "Inconsistent study patient identity".into(),
                    ));
                }
            }
            for tag in ["00100020", "00100021"] {
                let current = super::mapping::scalar(&study.dataset, tag, "LO")?;
                let verified = super::mapping::scalar(first, tag, "LO")?.or(if tag == "00100021" {
                    self.config.default_patient_issuer.as_deref()
                } else {
                    None
                });
                if current.is_some() && current != verified {
                    return Err(DicomError::Metadata(
                        "QIDO patient identity differs from metadata".into(),
                    ));
                }
                if current.is_none()
                    && let Some(attribute) = first.get(tag)
                {
                    study.dataset[tag] = attribute.clone();
                }
            }
        }
        Ok(studies)
    }

    async fn datasets(
        &self,
        url: url::Url,
        allow_empty: bool,
        max_datasets: usize,
    ) -> Result<Vec<Value>, DicomError> {
        self.datasets_kind(url, allow_empty, max_datasets, false)
            .await
    }

    async fn datasets_kind(
        &self,
        url: url::Url,
        allow_empty: bool,
        max_datasets: usize,
        single: bool,
    ) -> Result<Vec<Value>, DicomError> {
        self.policy
            .check_url(&url)
            .map_err(DicomError::Configuration)?;
        let mut response = self
            .client
            .get(url)
            .header(reqwest::header::ACCEPT, "application/dicom+json")
            .header(reqwest::header::CACHE_CONTROL, "no-cache")
            .send()
            .await
            .map_err(|_| DicomError::Transport)?;
        let warning = response.headers().contains_key(reqwest::header::WARNING);
        if response.status() == reqwest::StatusCode::NO_CONTENT && allow_empty {
            if warning {
                return Err(DicomError::Partial);
            }
            return Ok(Vec::new());
        }
        if response.status() == reqwest::StatusCode::PARTIAL_CONTENT {
            return Err(DicomError::Partial);
        }
        if response.status() != reqwest::StatusCode::OK {
            return Err(DicomError::Status(response.status().as_u16()));
        }
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .split(';')
            .next()
            .unwrap_or("")
            .trim();
        if !content_type.eq_ignore_ascii_case("application/dicom+json") {
            return Err(DicomError::Metadata(
                "expected application/dicom+json".into(),
            ));
        }
        if response
            .content_length()
            .is_some_and(|length| length > self.config.max_metadata_bytes as u64)
        {
            return Err(DicomError::Limit);
        }
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| DicomError::Transport)? {
            if chunk.len() > self.config.max_metadata_bytes.saturating_sub(body.len()) {
                return Err(DicomError::Limit);
            }
            body.extend_from_slice(&chunk);
        }
        let decoded: Value = serde_json::from_slice(&body)
            .map_err(|_| DicomError::Metadata("invalid DICOM JSON".into()))?;
        let datasets = match decoded {
            Value::Array(datasets) => datasets,
            Value::Object(dataset) if single => vec![Value::Object(dataset)],
            _ => return Err(DicomError::Metadata("expected DICOM JSON array".into())),
        };
        if datasets.len() > max_datasets {
            return Err(DicomError::Limit);
        }
        if datasets.is_empty() && warning {
            return Err(DicomError::Partial);
        }
        if datasets.is_empty() && !allow_empty {
            return Err(DicomError::Metadata("study metadata is empty".into()));
        }
        Ok(datasets)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{header, method, path},
    };

    async fn setup(response: ResponseTemplate, bytes: usize) -> (MockServer, DicomWebClient) {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/dicom/studies/1.2.3/metadata"))
            .and(header("accept", "application/dicom+json"))
            .respond_with(response)
            .mount(&server)
            .await;
        let c: DicomConnection = serde_json::from_value(serde_json::json!({
            "wado_url":format!("{}/dicom/", server.uri()), "allow_http":true,
            "patient_identifier_systems":{"HOSPITAL":"urn:oid:1.2.3"}, "max_metadata_bytes":bytes
        }))
        .unwrap();
        let client = DicomWebClient::new(&c, Arc::new(EgressPolicy::permissive())).unwrap();
        (server, client)
    }

    #[tokio::test]
    async fn recovers_missing_qido_issuer_only_from_consistent_study_metadata() {
        use serde_json::json;
        for (second_study, second_patient, accepted) in [
            ("1.2.3", "MRN-1", true),
            ("1.2.3", "OTHER", false),
            ("9.8.7", "MRN-1", false),
        ] {
            let server = MockServer::start().await;
            let row = |study, patient| {
                json!({"0020000D":{"vr":"UI","Value":[study]},
                "00100020":{"vr":"LO","Value":[patient]},"00100021":{"vr":"LO","Value":["HOSPITAL"]}})
            };
            Mock::given(method("GET")).and(path("/dicom/studies"))
                .respond_with(ResponseTemplate::new(200).set_body_raw(
                    json!([{"0020000D":{"vr":"UI","Value":["1.2.3"]},"00100020":{"vr":"LO","Value":["MRN-1"]}}]).to_string(),
                    "application/dicom+json")).mount(&server).await;
            Mock::given(method("GET"))
                .and(path("/dicom/studies/1.2.3/metadata"))
                .respond_with(ResponseTemplate::new(200).set_body_raw(
                    json!([row("1.2.3", "MRN-1"), row(second_study, second_patient)]).to_string(),
                    "application/dicom+json",
                ))
                .mount(&server)
                .await;
            let config: DicomConnection = serde_json::from_value(json!({
                "wado_url":format!("{}/dicom", server.uri()), "qido_url":format!("{}/dicom", server.uri()),
                "allow_http":true,"patient_identifier_systems":{"HOSPITAL":"urn:test:mrn"}
            })).unwrap();
            let client =
                DicomWebClient::new(&config, Arc::new(EgressPolicy::permissive())).unwrap();
            let result = client
                .query_studies(0, 10, Some(("MRN-1", "HOSPITAL")))
                .await;
            if accepted {
                assert_eq!(
                    result.unwrap()[0].dataset["00100021"]["Value"][0],
                    "HOSPITAL"
                );
            } else {
                assert!(result.is_err());
            }
        }
    }

    #[tokio::test]
    async fn requests_metadata_without_pixels_and_bounds_responses() {
        let (_server, client) = setup(
            ResponseTemplate::new(200).set_body_raw("[{}]", "application/dicom+json"),
            100,
        )
        .await;
        assert_eq!(client.study_metadata("1.2.3").await.unwrap().len(), 1);
        let (_server, client) = setup(
            ResponseTemplate::new(200).set_body_raw("[{}]", "application/dicom+json"),
            2,
        )
        .await;
        assert!(matches!(
            client.study_metadata("1.2.3").await,
            Err(DicomError::Limit)
        ));
    }

    #[tokio::test]
    async fn large_study_metadata_uses_configurable_limits_with_or_without_content_length() {
        use axum::{
            Router, body::Body, http::header::CONTENT_TYPE, response::Response, routing::get,
        };

        let payload = serde_json::to_vec(&serde_json::json!([{
            "00081030": {"vr": "LO", "Value": ["x".repeat(11 * 1024 * 1024)]}
        }]))
        .unwrap();
        for chunked in [false, true] {
            let payload = payload.clone();
            let app = Router::new().route(
                "/studies/1.2.3/metadata",
                get(move || {
                    let payload = payload.clone();
                    async move {
                        let body = if chunked {
                            let chunks: Vec<_> = payload
                                .chunks(64 * 1024)
                                .map(|chunk| Ok::<_, std::io::Error>(chunk.to_vec()))
                                .collect();
                            Body::from_stream(futures_util::stream::iter(chunks))
                        } else {
                            Body::from(payload)
                        };
                        Response::builder()
                            .header(CONTENT_TYPE, "application/dicom+json")
                            .body(body)
                            .unwrap()
                    }
                }),
            );
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
            for limit in [None, Some(8 * 1024 * 1024)] {
                let mut value = serde_json::json!({
                    "wado_url": format!("http://{address}"), "allow_http": true,
                    "patient_identifier_systems": {"HOSPITAL": "urn:test:mrn"}
                });
                if let Some(limit) = limit {
                    value["max_metadata_bytes"] = limit.into();
                }
                let config = serde_json::from_value(value).unwrap();
                let client =
                    DicomWebClient::new(&config, Arc::new(EgressPolicy::permissive())).unwrap();
                let result = client.study_metadata("1.2.3").await;
                if limit.is_none() {
                    assert_eq!(result.unwrap().len(), 1);
                } else {
                    assert!(matches!(result, Err(DicomError::Limit)));
                }
            }
            server.abort();
        }
    }

    #[tokio::test]
    async fn accepts_bare_dicom_json_only_for_single_resource_requests() {
        let dataset = serde_json::json!({"0020000D":{"vr":"UI","Value":["1.2.3"]}});
        let (server, client) = setup(
            ResponseTemplate::new(200).set_body_raw(dataset.to_string(), "application/dicom+json"),
            1000,
        )
        .await;
        let url =
            url::Url::parse(&format!("{}/dicom/studies/1.2.3/metadata", server.uri())).unwrap();
        assert_eq!(client.single_metadata(url).await.unwrap(), vec![dataset]);
        assert!(client.study_metadata("1.2.3").await.is_err());
        let (server, client) = setup(
            ResponseTemplate::new(200).set_body_raw("[{},{}]", "application/dicom+json"),
            1000,
        )
        .await;
        let url =
            url::Url::parse(&format!("{}/dicom/studies/1.2.3/metadata", server.uri())).unwrap();
        assert!(matches!(
            client.single_metadata(url).await,
            Err(DicomError::Limit)
        ));
    }

    #[tokio::test]
    async fn refuses_partial_snapshots_and_incomplete_empty_query_pages() {
        for response in [
            ResponseTemplate::new(206).set_body_raw("[{}]", "application/dicom+json"),
            ResponseTemplate::new(200)
                .insert_header("warning", "299 pacs incomplete")
                .set_body_raw("[]", "application/dicom+json"),
            ResponseTemplate::new(204).insert_header("warning", "299 pacs incomplete"),
        ] {
            let (server, client) = setup(response, 1000).await;
            let url =
                url::Url::parse(&format!("{}/dicom/studies/1.2.3/metadata", server.uri())).unwrap();
            assert!(matches!(
                client.datasets(url, true, 10).await,
                Err(DicomError::Partial)
            ));
        }
    }

    #[tokio::test]
    async fn rejects_redirects_wrong_media_types_and_non_arrays() {
        for response in [
            ResponseTemplate::new(302).insert_header("location", "http://127.0.0.1/private"),
            ResponseTemplate::new(200).set_body_raw("[{}]", "text/html"),
            ResponseTemplate::new(200).set_body_raw("{}", "application/dicom+json"),
        ] {
            let (_server, client) = setup(response, 1000).await;
            assert!(client.study_metadata("1.2.3").await.is_err());
        }
    }
}
