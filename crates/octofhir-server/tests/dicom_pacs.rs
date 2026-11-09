//! Opt-in interoperability against an isolated external PACS with a synthetic image.
//! Set DICOM_TEST_PACS and DICOM_TEST_UPLOAD; run this test with --ignored.
//!
//! # Contents
//! Synthetic single/multiframe/JPEG retrieval, metadata, viewer and UPS checks.
//! # Invariants
//! Only explicitly selected loopback PACS receives synthetic fixture uploads.
//! Disposable FHIR databases never store the fixture's DICOM or pixel bytes.
use axum::{response::Html, routing::get};
use futures_util::{SinkExt, StreamExt};
use octofhir_config::ConfigurationManager;
use octofhir_server::{AppConfig, PostgresStorageConfig, build_app};
use serde_json::{Value, json};
use std::sync::Arc;
use testcontainers::{ImageExt, runners::AsyncRunner};
use testcontainers_modules::postgres::Postgres;
const STUDY: &str = "1.2.826.0.1.3680043.10.999.1";
const DICOM: &[u8] = include_bytes!("fixtures/dicom/synthetic-secondary-capture.dcm");
async fn parts(response: reqwest::Response) -> Vec<Vec<u8>> {
    let content_type = response.headers()["content-type"]
        .to_str()
        .unwrap()
        .to_owned();
    let boundary = content_type
        .parse::<mime::Mime>()
        .unwrap()
        .get_param("boundary")
        .unwrap()
        .as_str()
        .to_owned();
    let bytes = response.bytes().await.unwrap();
    let mut multipart = multer::Multipart::new(
        futures_util::stream::once(async move { Ok::<_, std::io::Error>(bytes) }),
        boundary,
    );
    let mut result = vec![];
    while let Some(field) = multipart.next_field().await.unwrap() {
        result.push(field.bytes().await.unwrap().to_vec());
    }
    result
}
async fn upload_fixture(http: &reqwest::Client, url: &str, data: &[u8]) {
    let response = if url.ends_with("/instances") {
        http.post(url)
            .header("content-type", "application/dicom")
            .body(data.to_vec())
            .send()
            .await
            .unwrap()
    } else {
        let mut body = b"--octofhir-fixture\r\nContent-Type: application/dicom\r\n\r\n".to_vec();
        body.extend_from_slice(data);
        body.extend_from_slice(b"\r\n--octofhir-fixture--\r\n");
        http.post(url)
            .header(
                "content-type",
                "multipart/related; type=\"application/dicom\"; boundary=octofhir-fixture",
            )
            .body(body)
            .send()
            .await
            .unwrap()
    };
    assert!(
        response.status().is_success(),
        "Synthetic upload: {} {}",
        response.status(),
        response.text().await.unwrap()
    );
}
async fn verify_variants(
    client: &reqwest::Client,
    pacs: &str,
    upload: &str,
    base: &str,
    patient: &str,
) -> Vec<Value> {
    let mut checks = vec![];
    for (data, uid, media, frames) in [
        (
            include_bytes!("fixtures/dicom/synthetic-multiframe.dcm").as_slice(),
            "1.2.826.0.1.3680043.10.999.2",
            "application/octet-stream",
            "1,2",
        ),
        (
            include_bytes!("fixtures/dicom/synthetic-jpeg-baseline.dcm").as_slice(),
            "1.2.826.0.1.3680043.10.999.3",
            "image/jpeg",
            "1",
        ),
    ] {
        upload_fixture(&reqwest::Client::new(), upload, data).await;
        let response=client.post(format!("{base}/fhir/Patient/{patient}/$dicom-import")).json(&json!({"resourceType":"Parameters","parameter":[{"name":"connection","valueString":"pacs"},{"name":"study","valueString":uid}]})).send().await.unwrap();
        assert_eq!(
            response.status(),
            200,
            "{uid}: {}",
            response.text().await.unwrap()
        );
        let frame_root = format!("studies/{uid}/series/{uid}.1/instances/{uid}.1.1/frames");
        let accept = format!("multipart/related; type=\"{media}\"");
        let mut actual = vec![];
        for frame in frames.split(',') {
            let path = format!("{frame_root}/{frame}");
            let direct = reqwest::Client::new()
                .get(format!("{}/{path}", pacs.trim_end_matches('/')))
                .header("accept", &accept)
                .send()
                .await
                .unwrap();
            assert_eq!(
                direct.status(),
                200,
                "Direct frame {uid}/{frame}: {}",
                direct.text().await.unwrap()
            );
            let expected = parts(direct).await;
            let proxied = client
                .get(format!("{base}/dicomweb/pacs/{path}"))
                .header("accept", &accept)
                .send()
                .await
                .unwrap();
            assert_eq!(
                proxied.status(),
                200,
                "Gateway frame {uid}/{frame}: {}",
                proxied.text().await.unwrap()
            );
            let decoded = parts(proxied).await;
            assert_eq!(decoded, expected);
            assert_eq!(decoded.len(), 1);
            actual.extend(decoded);
        }
        let mut list_status = 200;
        if frames == "1,2" {
            assert_eq!(
                actual,
                vec![
                    (0u8..=255).collect::<Vec<_>>(),
                    (0u8..=255).rev().collect::<Vec<_>>()
                ]
            );
            let path = format!("{frame_root}/{frames}");
            let direct = reqwest::Client::new()
                .get(format!("{}/{path}", pacs.trim_end_matches('/')))
                .header("accept", &accept)
                .send()
                .await
                .unwrap();
            list_status = direct.status().as_u16();
            let proxied = client
                .get(format!("{base}/dicomweb/pacs/{path}"))
                .header("accept", &accept)
                .send()
                .await
                .unwrap();
            assert_eq!(
                proxied.status(),
                direct.status(),
                "Frame-list support belongs to the origin"
            );
            if direct.status().is_success() {
                assert_eq!(parts(direct).await, actual);
                assert_eq!(parts(proxied).await, actual);
            } else {
                assert!(
                    matches!(list_status, 400 | 406 | 501),
                    "Unexpected frame-list failure: {list_status}"
                );
            }
        } else {
            assert_eq!(actual.len(), 1);
            assert!(actual[0].starts_with(&[0xff, 0xd8]));
        }
        let response = client
            .get(format!("{base}/dicomweb/pacs/studies/{uid}"))
            .header(
                "accept",
                "multipart/related; type=\"application/dicom\"; transfer-syntax=*",
            )
            .send()
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            200,
            "Collection {uid}: {}",
            response.text().await.unwrap()
        );
        let objects = parts(response).await;
        assert_eq!(objects.len(), 1);
        assert_eq!(&objects[0][128..132], b"DICM");
        checks.push(json!({"uid":uid,"media_type":media,"individual_frames":"passed","frame_list_status":list_status}));
    }
    checks
}

async fn verify_unicode_query(client: &reqwest::Client, pacs: &str, upload: &str, base: &str) {
    let http = reqwest::Client::new();
    for (suffix, label, data) in [
        (
            4,
            "A",
            include_bytes!("fixtures/dicom/synthetic-cyrillic-a.dcm").as_slice(),
        ),
        (
            5,
            "B",
            include_bytes!("fixtures/dicom/synthetic-cyrillic-b.dcm").as_slice(),
        ),
    ] {
        upload_fixture(&http, upload, data).await;
        let patient: Value = client.post(format!("{base}/fhir/Patient"))
            .json(&json!({"resourceType":"Patient","identifier":[{"system":"urn:test:mrn","value":format!("OCTOFHIR-DICOM-CYRILLIC-{label}")}]}))
            .send().await.unwrap().json().await.unwrap();
        let response = client.post(format!("{base}/fhir/Patient/{}/$dicom-import", patient["id"].as_str().unwrap()))
            .json(&json!({"resourceType":"Parameters","parameter":[{"name":"connection","valueString":"pacs"},{"name":"study","valueString":format!("1.2.826.0.1.3680043.10.999.{suffix}")}]}))
            .send().await.unwrap();
        assert_eq!(response.status(), 200, "{}", response.text().await.unwrap());
    }
    for fuzzy in ["false", "true"] {
        let mut expected = std::collections::BTreeSet::new();
        for suffix in [4, 5] {
            let uid = format!("1.2.826.0.1.3680043.10.999.{suffix}");
            let response = http
                .get(format!("{pacs}/studies"))
                .query(&[
                    ("StudyInstanceUID", uid.as_str()),
                    ("PatientName", "Иванов^Иван"),
                    ("fuzzymatching", fuzzy),
                    ("includefield", "InstitutionName"),
                ])
                .header("accept", "application/dicom+json")
                .send()
                .await
                .unwrap();
            assert!(
                response.status().is_success(),
                "Direct Unicode QIDO: {}",
                response.status()
            );
            if response.status() != 204 {
                let rows: Vec<Value> = response.json().await.unwrap();
                expected.extend(
                    rows.into_iter()
                        .map(|row| row["0020000D"]["Value"][0].as_str().unwrap().to_owned()),
                );
            }
        }
        let response = client
            .get(format!("{base}/dicomweb/pacs/studies"))
            .query(&[
                (
                    "StudyInstanceUID",
                    "1.2.826.0.1.3680043.10.999.4,1.2.826.0.1.3680043.10.999.5",
                ),
                ("PatientName", "Иванов^Иван"),
                ("fuzzymatching", fuzzy),
                ("includefield", "InstitutionName"),
            ])
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200, "{}", response.text().await.unwrap());
        let rows: Vec<Value> = response.json().await.unwrap();
        let actual = rows
            .iter()
            .map(|row| {
                assert_eq!(
                    row["00080080"]["Value"][0],
                    "OctoFHIR synthetic institution"
                );
                row["0020000D"]["Value"][0].as_str().unwrap().to_owned()
            })
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            actual, expected,
            "Origin Unicode matching must be preserved (fuzzy={fuzzy})"
        );
        assert!(actual.contains("1.2.826.0.1.3680043.10.999.4"));
    }
}
#[test]
#[ignore = "requires isolated DICOM_TEST_PACS and DICOM_TEST_UPLOAD endpoints"]
fn real_pacs_metadata_frame_and_viewer_retrieval() {
    std::thread::Builder::new()
        .name("dicom-real-pacs".into())
        .stack_size(8 * 1024 * 1024)
        .spawn(|| {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(Box::pin(scenario()))
        })
        .unwrap()
        .join()
        .unwrap();
}
async fn scenario() {
    let pacs = std::env::var("DICOM_TEST_PACS").expect("DICOM_TEST_PACS");
    let upload = std::env::var("DICOM_TEST_UPLOAD").expect("DICOM_TEST_UPLOAD");
    // This test must never upload a synthetic fixture to a clinical/public archive.
    for address in [&pacs, &upload] {
        let url = url::Url::parse(address).unwrap();
        assert!(
            matches!(url.host_str(), Some("127.0.0.1" | "localhost")),
            "Use an isolated loopback PACS"
        );
    }
    let http = reqwest::Client::new();
    let response = if upload.ends_with("/instances") {
        http.post(&upload)
            .header("content-type", "application/dicom")
            .body(DICOM)
            .send()
            .await
            .unwrap()
    } else {
        let mut body = b"--octofhir-fixture\r\nContent-Type: application/dicom\r\n\r\n".to_vec();
        body.extend_from_slice(DICOM);
        body.extend_from_slice(b"\r\n--octofhir-fixture--\r\n");
        http.post(&upload)
            .header(
                "content-type",
                "multipart/related; type=\"application/dicom\"; boundary=octofhir-fixture",
            )
            .body(body)
            .send()
            .await
            .unwrap()
    };
    assert!(
        response.status().is_success(),
        "Fixture upload: {} {}",
        response.status(),
        response.text().await.unwrap()
    );
    let pg = Postgres::default()
        .with_tag("18-alpine")
        .start()
        .await
        .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let base = format!("http://127.0.0.1:{port}");
    let mut cfg = AppConfig::default();
    cfg.fhir.version = "R4".into();
    cfg.server.host = "127.0.0.1".into();
    cfg.server.port = port;
    cfg.storage.postgres = Some(PostgresStorageConfig {
        url: Some(format!(
            "postgres://postgres:postgres@127.0.0.1:{}/postgres",
            pg.get_host_port_ipv4(5432).await.unwrap()
        )),
        pool_size: 8,
        ..Default::default()
    });
    cfg.packages.load = vec![octofhir_server::config::PackageSpec::Table {
        id: Some("hl7.fhir.r4.core".into()),
        version: Some("4.0.1".into()),
        path: None,
        url: Some("https://hl7.org/fhir/R4/hl7.fhir.r4.core.tgz".into()),
    }];
    cfg.auth.policy.anonymous_access = true;
    cfg.bootstrap.backend_client = Some(octofhir_server::config::BackendClientConfig {
        client_id: "pacs-test".into(),
        client_secret: Some("isolated-pacs-secret".into()),
        scopes: vec!["system/*.cruds".into()],
        name: None,
        description: None,
    });
    cfg.server.outbound_allowlist = vec!["127.0.0.1/32", "localhost"]
        .into_iter()
        .map(Into::into)
        .collect();
    cfg.dicom=serde_json::from_value(json!({"connections":{"pacs":{"wado_url":pacs,"qido_url":pacs,"allow_http":true,"patient_identifier_systems":{"HOSPITAL":"urn:test:mrn"},"gateway":{},"sync":{"client_id":"pacs-test","scopes":["system/*.cruds"],"discovery":"notifications"}}}})).unwrap();
    let ups = std::env::var("DICOM_TEST_UPS").ok();
    if let Some(url) = &ups {
        assert_eq!(url::Url::parse(url).unwrap().host_str(), Some("127.0.0.1"));
        let notification = std::env::var("DICOM_TEST_UPS_WS").expect("DICOM_TEST_UPS_WS");
        let connection = cfg.dicom.connections.get_mut("pacs").unwrap();
        connection.ups = Some(serde_json::from_value(json!({
            "url":url,"notification_url":notification,"client_id":"pacs-test","scopes":["system/*.cruds"],
            "ae_title":"OCTOFHIR","poll_seconds":1,"retry_seconds":1,
            "coding_schemes":{"urn:test:procedure":"99OCTOFHIR","http://unitsofmeasure.org":"UCUM"},
            "result_mappings":{"1.2.840.10008.5.1.4.1.1.7":{"kind":"document_reference","system":"urn:test:result","code":"synthetic-output"}}
        })).unwrap());
    }
    let viewer_token = Arc::new(tokio::sync::RwLock::new(String::new()));
    let viewer_identity = viewer_token.clone();
    let app = build_app(
        &cfg,
        Arc::new(ConfigurationManager::builder().build().await.unwrap()),
    )
    .await
    .unwrap()
    .route(
        "/__dicom_viewer",
        get(move || {
            let identity = viewer_identity.clone();
            async move {
                let html = include_str!("fixtures/dicom/viewer.html")
                    .replace("__TOKEN__", &identity.read().await)
                    .replace("__STUDY__", STUDY);
                Html(html)
            }
        }),
    );
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let token: Value = http
        .post(format!("{base}/auth/token"))
        .basic_auth("pacs-test", Some("isolated-pacs-secret"))
        .form(&[
            ("grant_type", "client_credentials"),
            ("scope", "system/*.cruds"),
        ])
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let token = token["access_token"].as_str().expect("backend token");
    *viewer_token.write().await = token.into();
    let mut auth = reqwest::header::HeaderMap::new();
    auth.insert("authorization", format!("Bearer {token}").parse().unwrap());
    let client = reqwest::Client::builder()
        .default_headers(auth)
        .build()
        .unwrap();
    let patient:Value=client.post(format!("{base}/fhir/Patient")).json(&json!({"resourceType":"Patient","identifier":[{"system":"urn:test:mrn","value":"OCTOFHIR-DICOM-TEST"}]})).send().await.unwrap().json().await.unwrap();
    let patient_id = patient["id"].as_str().unwrap();
    let response=client.post(format!("{base}/fhir/Patient/{patient_id}/$dicom-discover")).json(&json!({"resourceType":"Parameters","parameter":[{"name":"connection","valueString":"pacs"}]})).send().await.unwrap();
    let status = response.status();
    let result: Value = response.json().await.unwrap();
    assert_eq!(status, 200, "{result}");
    assert!(result.to_string().contains(STUDY));
    let response=client.post(format!("{base}/fhir/Patient/{patient_id}/$dicom-import")).json(&json!({"resourceType":"Parameters","parameter":[{"name":"connection","valueString":"pacs"},{"name":"study","valueString":STUDY}]})).send().await.unwrap();
    let status = response.status();
    let result: Value = response.json().await.unwrap();
    assert_eq!(status, 200, "{result}");
    let study_ref = result["parameter"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|p| {
            p["valueReference"]["reference"]
                .as_str()
                .filter(|r| r.starts_with("ImagingStudy/"))
        })
        .unwrap()
        .to_owned();
    let endpoint_ref = result["parameter"][1]["valueReference"]["reference"]
        .as_str()
        .unwrap();
    let endpoint: Value = client
        .get(format!("{base}/fhir/{endpoint_ref}"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(endpoint["address"], format!("{base}/dicomweb/pacs"));
    let root = format!("{base}/dicomweb/pacs/studies/{STUDY}");
    let capabilities = client
        .request(reqwest::Method::OPTIONS, format!("{base}/dicomweb/pacs"))
        .header("accept", "application/vnd.sun.wadl+xml")
        .send()
        .await
        .unwrap();
    assert_eq!(capabilities.status(), 200);
    assert!(capabilities.text().await.unwrap().contains("<application"));
    let studies: Value = client
        .get(format!(
            "{base}/dicomweb/pacs/studies?StudyInstanceUID={STUDY}&includefield=all"
        ))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(studies[0]["00100020"]["Value"][0], "OCTOFHIR-DICOM-TEST");
    assert_eq!(studies[0]["00201208"]["Value"][0], 1);
    let xml = client
        .get(format!("{root}/metadata"))
        .header(
            "accept",
            "multipart/related; type=\"application/dicom+xml\"",
        )
        .send()
        .await
        .unwrap();
    assert_eq!(xml.status(), 200);
    assert!(xml.text().await.unwrap().contains("<NativeDicomModel"));
    for target in [&root, &format!("{root}/series/{STUDY}.1")] {
        let response = client
            .get(target)
            .header(
                "accept",
                "multipart/related; type=\"application/dicom\"; transfer-syntax=*",
            )
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200, "{}", response.text().await.unwrap());
        let parts = parts(response).await;
        assert_eq!(parts.len(), 1);
        assert_eq!(&parts[0][128..132], b"DICM");
        assert!(
            parts[0]
                .windows(256)
                .any(|part| part == (0u8..=255).collect::<Vec<_>>())
        );
    }
    let qido = client
        .get(format!("{root}/series/{STUDY}.1/instances"))
        .send()
        .await
        .unwrap();
    assert_eq!(qido.status(), 200, "{}", qido.text().await.unwrap());
    let qido: Value = qido.json().await.unwrap();
    assert_eq!(qido[0]["00080016"]["Value"][0], "1.2.840.10008.5.1.4.1.1.7");
    let response = client.get(format!("{root}/metadata")).send().await.unwrap();
    let status = response.status();
    let metadata: Value = response.json().await.unwrap();
    assert_eq!(status, 200, "{metadata}");
    assert_eq!(metadata.as_array().unwrap().len(), 1);
    for suffix in [
        "thumbnail",
        &format!("series/{STUDY}.1/thumbnail"),
        &format!("series/{STUDY}.1/instances/{STUDY}.1.1/thumbnail"),
        &format!("series/{STUDY}.1/instances/{STUDY}.1.1/frames/1/thumbnail"),
    ] {
        let response = client
            .get(format!("{root}/{suffix}"))
            .header("accept", "image/jpeg")
            .send()
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            200,
            "Thumbnail: {}",
            response.text().await.unwrap()
        );
        assert!(response.bytes().await.unwrap().starts_with(&[0xff, 0xd8]));
    }
    let rendered = client
        .get(format!("{root}/rendered"))
        .header("accept", "multipart/related; type=\"image/jpeg\"")
        .send()
        .await
        .unwrap();
    assert_eq!(
        rendered.status(),
        200,
        "Rendered study: {}",
        rendered.text().await.unwrap()
    );
    let renderings = parts(rendered).await;
    assert_eq!(renderings.len(), 1);
    assert!(renderings[0].starts_with(&[0xff, 0xd8]));
    let transport = verify_variants(&client, &pacs, &upload, &base, patient_id).await;
    verify_unicode_query(&client, &pacs, &upload, &base).await;
    let instance = format!("{root}/series/{STUDY}.1/instances/{STUDY}.1.1");
    let response = client
        .get(format!("{instance}/frames/1"))
        .header(
            "accept",
            "multipart/related; type=\"application/octet-stream\"; transfer-syntax=*",
        )
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let content_type = response.headers()["content-type"]
        .to_str()
        .unwrap()
        .to_owned();
    let body = response.bytes().await.unwrap();
    let boundary = content_type
        .split(';')
        .find_map(|v| v.trim().strip_prefix("boundary="))
        .unwrap()
        .trim_matches('"');
    let start = body.windows(4).position(|b| b == b"\r\n\r\n").unwrap() + 4;
    let end_marker = format!("\r\n--{boundary}");
    let end = body
        .windows(end_marker.len())
        .rposition(|b| b == end_marker.as_bytes())
        .unwrap();
    assert_eq!(&body[start..end], &(0u8..=255).collect::<Vec<_>>());
    // Both pass-through DICOM objects and rewritten pixel references are real PACS requests.
    if let Some(uri) = metadata[0]["7FE00010"]["BulkDataURI"].as_str() {
        let response = client
            .get(url::Url::parse(&base).unwrap().join(uri).unwrap())
            .header(
                "accept",
                "multipart/related; type=\"application/octet-stream\"",
            )
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        assert!(!response.bytes().await.unwrap().is_empty());
    }
    let response = client
        .get(&instance)
        .header(
            "accept",
            "multipart/related; type=\"application/dicom\"; transfer-syntax=*",
        )
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert!(
        response
            .bytes()
            .await
            .unwrap()
            .windows(4)
            .any(|s| s == b"DICM")
    );
    let ups_checks = if let Some(ups) = ups {
        Some(real_ups(&client, &http, &base, &ups, patient_id, &study_ref).await)
    } else {
        None
    };
    if let Ok(file) = std::env::var("DICOM_VIEWER_SESSION_FILE") {
        let session_id = uuid::Uuid::new_v4().to_string();
        let session = json!({"session_id":session_id,"base_url":base,"gateway_url":format!("{base}/dicomweb/pacs"),"transport":transport,"ups":ups_checks,
            "access_token":viewer_token.read().await.as_str(),
            "studies":[{"uid":STUDY,"kind":"native"},
                {"uid":"1.2.826.0.1.3680043.10.999.2","kind":"multiframe"},
                {"uid":"1.2.826.0.1.3680043.10.999.3","kind":"jpeg"}]});
        std::fs::write(&file, serde_json::to_vec(&session).unwrap()).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(300);
        let done = format!("{file}.done");
        let mut completed = false;
        while tokio::time::Instant::now() < deadline {
            if let Ok(bytes) = std::fs::read(&done)
                && let Ok(result) = serde_json::from_slice::<Value>(&bytes)
                && result["session_id"] == session_id
                && result["status"] == "passed"
            {
                completed = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
        assert!(completed, "OHIF verification did not complete");
    }
    if let Ok(file) = std::env::var("DICOM_VIEWER_READY_FILE") {
        std::fs::write(file, format!("{base}/__dicom_viewer")).unwrap();
        let seconds = std::env::var("DICOM_VIEWER_HOLD_SECONDS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(120)
            .min(300);
        tokio::time::sleep(std::time::Duration::from_secs(seconds)).await;
    }
    server.abort();
}

async fn ups_command(client: &reqwest::Client, base: &str, task: &str, action: &str) -> Value {
    let response = client.post(format!("{base}/fhir/Task/{task}/$dicom-ups"))
        .json(&json!({"resourceType":"Parameters","parameter":[{"name":"connection","valueString":"pacs"},{"name":"action","valueString":action}]}))
        .send().await.unwrap();
    let status = response.status();
    let body: Value = response.json().await.unwrap();
    assert_eq!(status, 200, "UPS {action}: {body}");
    body
}
async fn wait_real_task(client: &reqwest::Client, base: &str, task: &str, wanted: &str) -> Value {
    let mut last = json!(null);
    for _ in 0..200 {
        last = client
            .get(format!("{base}/fhir/Task/{task}"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        if last["status"] == wanted {
            return last;
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    let diagnostic = ups_command(client, base, task, "status").await;
    panic!("Task did not reach {wanted}: {last}; {diagnostic}");
}
async fn real_ups(
    client: &reqwest::Client,
    performer: &reqwest::Client,
    base: &str,
    ups: &str,
    patient: &str,
    study: &str,
) -> Value {
    let definition = json!({
        "resourceType":"Task","status":"draft","intent":"order",
        "for":{"reference":format!("Patient/{patient}")},"focus":{"reference":study},
        "code":{"coding":[{"system":"urn:test:procedure","code":"TEST","display":"Synthetic interoperability procedure"}]},
        "input":[{"type":{"coding":[{"system":"urn:test:procedure","code":"PARAM","display":"Synthetic parameter"}]},"valueQuantity":{"value":0.7,"system":"http://unitsofmeasure.org","code":"1","unit":"ratio"}}]
    });
    let task: Value = client
        .post(format!("{base}/fhir/Task"))
        .json(&definition)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let id = task["id"].as_str().unwrap();
    let submitted = ups_command(client, base, id, "submit").await;
    let workitem = submitted["parameter"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "workitem")
        .unwrap()["valueUri"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(workitem.starts_with(ups));
    for _ in 0..200 {
        let response = performer
            .get(&workitem)
            .header("accept", "application/dicom+json")
            .send()
            .await
            .unwrap();
        if response.status().is_success() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    let ws = std::env::var("DICOM_TEST_UPS_WS").unwrap();
    let (mut notifications, _) = tokio_tungstenite::connect_async(
        ws.strip_suffix("OCTOFHIR")
            .expect("Test WS URL must end with OCTOFHIR")
            .to_owned()
            + "OCTOPERFORMER",
    )
    .await
    .unwrap();
    let notification_task = tokio::spawn(async move {
        while let Some(Ok(message)) = notifications.next().await {
            if let tokio_tungstenite::tungstenite::Message::Ping(bytes) = message
                && notifications
                    .send(tokio_tungstenite::tungstenite::Message::Pong(bytes))
                    .await
                    .is_err()
            {
                break;
            }
        }
    });
    let subscription = performer
        .post(format!(
            "{workitem}/subscribers/OCTOPERFORMER?deletionlock=false"
        ))
        .send()
        .await
        .unwrap();
    assert!(subscription.status().is_success());
    let transaction = format!("2.25.{}", uuid::Uuid::new_v4().as_u128());
    let attr = |vr: &str, value: &str| json!({"vr":vr,"Value":[value]});
    // This test acts as the external performer. Production code never claims this transaction UID.
    let response = performer
        .put(format!("{workitem}/state?requester=OCTOPERFORMER"))
        .header("content-type", "application/dicom+json")
        .json(&json!([{"00081195":attr("UI",&transaction),"00741000":attr("CS","IN PROGRESS")}]))
        .send()
        .await
        .unwrap();
    assert!(
        response.status().is_success(),
        "Performer claim: {} {}",
        response.status(),
        response.text().await.unwrap()
    );
    wait_real_task(client, base, id, "in-progress").await;
    let direct_cancel = performer
        .post(format!("{workitem}/cancelrequest?requester=OCTOFHIR"))
        .header("content-type", "application/dicom+json")
        .json(&json!([{}]))
        .send()
        .await
        .unwrap();
    let cancel_status = direct_cancel.status().as_u16();
    assert!(matches!(cancel_status, 200 | 202 | 204 | 409));
    ups_command(client, base, id, "cancel").await;
    let mut delivered = false;
    for _ in 0..if cancel_status == 409 { 1 } else { 100 } {
        let status = ups_command(client, base, id, "status").await;
        if status["parameter"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["name"] == "cancelDelivered" && p["valueBoolean"] == true)
        {
            delivered = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert_eq!(
        delivered,
        cancel_status != 409,
        "Cancel delivery must reflect origin capability"
    );
    let response = performer
        .get(&workitem)
        .header("accept", "application/dicom+json")
        .send()
        .await
        .unwrap();
    let remote: Value = response.json().await.unwrap();
    let remote = if remote.is_array() {
        &remote[0]
    } else {
        &remote
    };
    assert_eq!(
        remote["00741000"]["Value"][0], "IN PROGRESS",
        "Cancel is only a request"
    );
    let now = chrono::Utc::now().format("%Y%m%d%H%M%S+0000").to_string();
    let response=performer.post(format!("{workitem}?Transaction-uid={transaction}&requester=OCTOPERFORMER")).header("content-type","application/dicom+json")
        .json(&json!([{"00081195":attr("UI",&transaction),"00741216":{"vr":"SQ","Value":[{
            "00404050":attr("DT",&now),"00404051":attr("DT",&now),
            "00404028":{"vr":"SQ","Value":[{"00080100":attr("SH","TEST-STATION"),"00080102":attr("SH","99OCTOFHIR"),"00080104":attr("LO","Isolated synthetic performer")}]},"00404019":{"vr":"SQ","Value":[{"00080100":attr("SH","TEST"),"00080102":attr("SH","99OCTOFHIR"),"00080104":attr("LO","Synthetic interoperability procedure")}]},"00404033":{"vr":"SQ","Value":[{"0040E020":attr("CS","DICOM"),"0020000D":attr("UI",STUDY),"0020000E":attr("UI",&format!("{STUDY}.1")),"00081199":{"vr":"SQ","Value":[{"00081150":attr("UI","1.2.840.10008.5.1.4.1.1.7"),"00081155":attr("UI",&format!("{STUDY}.1.1"))}]}}]}
        }]}}])).send().await.unwrap();
    assert!(
        response.status().is_success(),
        "Performer update: {} {}",
        response.status(),
        response.text().await.unwrap()
    );
    let response = performer
        .put(format!("{workitem}/state?requester=OCTOPERFORMER"))
        .header("content-type", "application/dicom+json")
        .json(&json!([{"00081195":attr("UI",&transaction),"00741000":attr("CS","COMPLETED")}]))
        .send()
        .await
        .unwrap();
    assert!(
        response.status().is_success(),
        "Performer completion: {} {}",
        response.status(),
        response.text().await.unwrap()
    );
    let completed = wait_real_task(client, base, id, "completed").await;
    assert_eq!(completed["output"].as_array().unwrap().len(), 1);
    let reference = completed["output"][0]["valueReference"]["reference"]
        .as_str()
        .unwrap();
    let result: Value = client
        .get(format!("{base}/fhir/{reference}"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(result["resourceType"], "DocumentReference");
    let object = result["content"][0]["attachment"]["url"].as_str().unwrap();
    let response = client
        .get(object)
        .header(
            "accept",
            "multipart/related; type=\"application/dicom\"; transfer-syntax=*",
        )
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert!(
        response
            .bytes()
            .await
            .unwrap()
            .windows(4)
            .any(|bytes| bytes == b"DICM")
    );
    notification_task.abort();
    // Cancellation of an unclaimed workitem is supported independently of
    // whether the origin can notify an already-running performer.
    let queued: Value = client
        .post(format!("{base}/fhir/Task"))
        .json(&definition)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let queued_id = queued["id"].as_str().unwrap();
    let submitted = ups_command(client, base, queued_id, "submit").await;
    let queued_uri = submitted["parameter"]
        .as_array()
        .unwrap()
        .iter()
        .find(|parameter| parameter["name"] == "workitem")
        .unwrap()["valueUri"]
        .as_str()
        .unwrap();
    let mut created = false;
    for _ in 0..200 {
        if performer
            .get(queued_uri)
            .header("accept", "application/dicom+json")
            .send()
            .await
            .unwrap()
            .status()
            .is_success()
        {
            created = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert!(
        created,
        "Queued cancellation requires an actual remote workitem"
    );
    ups_command(client, base, queued_id, "cancel").await;
    wait_real_task(client, base, queued_id, "cancelled").await;
    json!({"completion":"passed","output_publication":"passed","queued_cancel":"passed",
        "cancel_in_progress_status":cancel_status,"cancel_in_progress_delivered":delivered})
}
