//! DICOMweb HTTP contract against a mutable independent PACS protocol peer.
//!
//! # Contents
//! QIDO matching/projections, JSON/XML negotiation, capabilities and collections.
//! # Invariants
//! Late PACS instances remain undisclosed. Collection retrieval never calls a PACS
//! aggregate export; only freshly verified instance URLs may contribute bytes.
use axum::{
    Json, Router,
    body::Body,
    extract::{Path, Query, State},
    http::{HeaderMap, header},
    response::{IntoResponse, Response},
    routing::get,
};
use base64::Engine;
use octofhir_config::ConfigurationManager;
use octofhir_server::{AppConfig, PostgresStorageConfig, build_app};
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use testcontainers::{ImageExt, runners::AsyncRunner};
use testcontainers_modules::postgres::Postgres;

#[derive(Clone, Default)]
struct Pacs {
    late: Arc<AtomicBool>,
    large: Arc<AtomicBool>,
    large_metadata: Arc<AtomicBool>,
    closed: Arc<AtomicUsize>,
    object_calls: Arc<std::sync::Mutex<Vec<String>>>,
}
async fn qido(
    Path(path): Path<String>,
    Query(query): Query<std::collections::BTreeMap<String, String>>,
) -> Response {
    if path == "studies" {
        assert_eq!(
            query.get("StudyInstanceUID").map(String::as_str),
            Some("1.2.3")
        );
    } else {
        assert!(path.starts_with("studies/1.2.3/"));
        assert!(!query.contains_key("StudyInstanceUID"));
    }
    let mut rows = if query.get("PatientID").is_some_and(|value| value == "OTHER")
        || query.get("offset").is_some_and(|value| value != "0")
    {
        vec![]
    } else {
        vec![dataset(1), dataset(2), dataset(3)]
    };
    for row in &mut rows {
        row.as_object_mut().unwrap().remove("7FE00010");
        if query.get("includefield").is_some_and(|fields| {
            fields.contains("InstitutionName")
                || fields.contains("00080080")
                || fields.contains("all")
        }) {
            row["00080080"] = json!({"vr":"LO","Value":["STALE ORIGIN VALUE"]});
        }
        if query.get("includefield").is_some_and(|fields| {
            fields.contains("PrivateOriginAttribute")
                || fields.contains("77770010")
                || fields.contains("all")
        }) {
            row["77770010"] = json!({"vr":"LO","Value":["Origin-specific attribute"]});
        }
    }
    if query.contains_key("fuzzymatching") {
        assert_eq!(query["PatientName"], "Иванов^Иван");
        assert_eq!(query["PrivateOriginFilter"], "a+b&c");
        // Simulate an origin-specific fuzzy match that an exact local filter
        // would reject. Include a late SOP and an unrelated study as well.
        rows.retain(|row| row["0020000E"]["Value"][0] != "1.2.3.2");
        if !rows.is_empty() {
            let mut foreign = dataset(1);
            foreign["0020000D"]["Value"][0] = json!("9.8.7");
            rows.push(foreign);
        }
    }
    let mut seen = std::collections::BTreeSet::new();
    rows.retain(|row| {
        let study = row["0020000D"]["Value"][0].as_str().unwrap();
        let series = if path == "studies" {
            ""
        } else {
            row["0020000E"]["Value"][0].as_str().unwrap()
        };
        let instance = if path.ends_with("instances") {
            row["00080018"]["Value"][0].as_str().unwrap()
        } else {
            ""
        };
        seen.insert((study.to_owned(), series.to_owned(), instance.to_owned()))
    });
    (
        [(header::CONTENT_TYPE, "application/dicom+json")],
        Json(rows),
    )
        .into_response()
}
struct DropBody(Arc<AtomicUsize>);
impl Drop for DropBody {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}
fn dataset(index: usize) -> Value {
    let mut value = json!({
        "00080016":{"vr":"UI","Value":["1.2.840.10008.5.1.4.1.1.7"]},"00080018":{"vr":"UI","Value":[format!("1.2.3.{index}.1")]},
        "00080020":{"vr":"DA","Value":["20260228"]},"00080030":{"vr":"TM","Value":["123059.123456"]},"00080050":{"vr":"SH","Value":["ACC-1"]},
        "00080060":{"vr":"CS","Value":[if index==1{"CT"}else{"MR"}]},"00081030":{"vr":"LO","Value":["Synthetic & private"]},
        "00100010":{"vr":"PN","Value":[{"Alphabetic":"Doe, Jane^A"}]},"00100020":{"vr":"LO","Value":["MRN-1"]},"00100021":{"vr":"LO","Value":["HOSPITAL"]},
        "00080080":{"vr":"LO","Value":["Verified institution"]},
        "0020000D":{"vr":"UI","Value":["1.2.3"]},"0020000E":{"vr":"UI","Value":[format!("1.2.3.{index}")]},"00200011":{"vr":"IS","Value":[index]},"00200013":{"vr":"IS","Value":["001"]},
        "00280010":{"vr":"US","Value":[16]},"00280011":{"vr":"US","Value":[16]},"00280100":{"vr":"US","Value":[8]},
        "00400275":{"vr":"SQ","Value":[{"00400009":{"vr":"SH","Value":["SPS-1"]},"00401001":{"vr":"SH","Value":["REQ-1"]}}]},
        "7FE00010":{"vr":"OW","BulkDataURI":format!("/wado/bulk/opaque-{index}?version=1")}
    });
    if index == 2 {
        value["7FE00010"] = json!({"vr":"OB","InlineBinary":base64::engine::general_purpose::STANDARD.encode(vec![2u8;256])});
    }
    value
}
async fn archive(
    State(pacs): State<Pacs>,
    Path(path): Path<String>,
    headers: HeaderMap,
) -> Response {
    assert!(
        !headers.contains_key(header::AUTHORIZATION),
        "FHIR bearer reached PACS"
    );
    if path.ends_with("metadata") {
        let mut rows = vec![dataset(1), dataset(2)];
        if pacs.late.load(Ordering::SeqCst) {
            rows.push(dataset(3));
        }
        if path.contains("/instances/") {
            rows.retain(|r| path.contains(r["00080018"]["Value"][0].as_str().unwrap()));
        } else if path.contains("/series/") {
            rows.retain(|r| path.contains(r["0020000E"]["Value"][0].as_str().unwrap()));
        }
        if pacs.large_metadata.load(Ordering::SeqCst) {
            rows[0]["77771001"] = json!({"vr":"UT","Value":["x".repeat(32 * 1024)]});
        }
        return (
            [(header::CONTENT_TYPE, "application/dicom+json")],
            Json(rows),
        )
            .into_response();
    }
    pacs.object_calls.lock().unwrap().push(path.clone());
    assert!(
        path.contains("/instances/") || path.starts_with("bulk/opaque-"),
        "unsafe PACS aggregate request: {path}"
    );
    if path.ends_with("rendered") || path.ends_with("thumbnail") {
        return (
            [(header::CONTENT_TYPE, "image/jpeg")],
            b"SYNTHETIC-JPEG".to_vec(),
        )
            .into_response();
    }
    let index = if path.contains("1.2.3.1.1") || path.contains("opaque-1") {
        1u8
    } else {
        2
    };
    let part_type = if path.starts_with("bulk/") {
        "application/octet-stream"
    } else {
        "application/dicom"
    };
    let prefix=format!("--pacs-boundary\r\nContent-Type: {part_type}\r\nContent-Location: https://private.example/credential-must-not-leak\r\n\r\n").into_bytes();
    let large = pacs.large.load(Ordering::SeqCst);
    let guard = DropBody(pacs.closed.clone());
    let body = async_stream::stream! {
        let _guard=guard;
        yield Ok::<_,std::io::Error>(prefix);
        if large{for _ in 0..32{tokio::time::sleep(std::time::Duration::from_millis(5)).await;yield Ok(vec![index;1024*1024]);}}
        else{yield Ok(vec![index;256]);}
        yield Ok(b"\r\n--pacs-boundary--\r\n".to_vec());
    };
    let mut response = Response::new(Body::from_stream(body));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        format!("multipart/related; type=\"{part_type}\"; boundary=pacs-boundary")
            .parse()
            .unwrap(),
    );
    response
        .headers_mut()
        .insert(header::SET_COOKIE, "private-secret=hidden".parse().unwrap());
    response
}
pub(super) fn run_scenario(remote: bool) {
    std::thread::Builder::new()
        .name("dicomweb-protocol".into())
        .stack_size(8 * 1024 * 1024)
        .spawn(move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(Box::pin(scenario(remote)))
        })
        .unwrap()
        .join()
        .unwrap();
}
async fn decoded_parts(response: reqwest::Response) -> Vec<axum::body::Bytes> {
    assert_eq!(response.status(), 200);
    let boundary = response.headers()[header::CONTENT_TYPE]
        .to_str()
        .unwrap()
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
    let mut output = vec![];
    while let Some(field) = multipart.next_field().await.unwrap() {
        output.push(field.bytes().await.unwrap());
    }
    output
}
async fn scenario(remote: bool) {
    let pg = Postgres::default()
        .with_tag("18-alpine")
        .start()
        .await
        .unwrap();
    let pacs = Pacs::default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let peer = pacs.clone();
    let archive_task = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new()
                .route("/wado/{*path}", get(archive))
                .route("/qido/{*path}", get(qido))
                .with_state(peer),
        )
        .await
        .unwrap()
    });
    let mut cfg = AppConfig::default();
    cfg.fhir.version = "R4".into();
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
        client_id: "gateway-protocol".into(),
        client_secret: Some("isolated-protocol-secret".into()),
        scopes: vec!["system/*.cruds".into()],
        name: None,
        description: None,
    });
    cfg.server.outbound_allowlist = vec!["127.0.0.1/32".into()];
    cfg.dicom=serde_json::from_value(json!({"connections":{"pacs":{"wado_url":format!("{origin}/wado"),"allow_http":true,"patient_identifier_systems":{"HOSPITAL":"urn:test:mrn"},"gateway":{"max_stream_bytes":64*1024*1024,"stream_seconds":30,"idle_seconds":5}}}})).unwrap();
    if remote {
        cfg.dicom.connections.get_mut("pacs").unwrap().qido_url = Some(format!("{origin}/qido"));
    }
    cfg.dicom
        .connections
        .get_mut("pacs")
        .unwrap()
        .max_metadata_bytes = 16 * 1024;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    cfg.server.base_url = Some(base.clone());
    let app = build_app(
        &cfg,
        Arc::new(ConfigurationManager::builder().build().await.unwrap()),
    )
    .await
    .unwrap();
    let app_task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let http = reqwest::Client::new();
    let token: Value = http
        .post(format!("{base}/auth/token"))
        .basic_auth("gateway-protocol", Some("isolated-protocol-secret"))
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
    let mut headers = HeaderMap::new();
    headers.insert(
        header::AUTHORIZATION,
        format!("Bearer {}", token["access_token"].as_str().unwrap())
            .parse()
            .unwrap(),
    );
    let client = reqwest::Client::builder()
        .default_headers(headers)
        .build()
        .unwrap();
    let patient:Value=client.post(format!("{base}/fhir/Patient")).json(&json!({"resourceType":"Patient","identifier":[{"system":"urn:test:mrn","value":"MRN-1"}]})).send().await.unwrap().json().await.unwrap();
    let imported=client.post(format!("{base}/fhir/Patient/{}/$dicom-import",patient["id"].as_str().unwrap())).json(&json!({"resourceType":"Parameters","parameter":[{"name":"connection","valueString":"pacs"},{"name":"study","valueString":"1.2.3"}]})).send().await.unwrap();
    assert_eq!(imported.status(), 200, "{}", imported.text().await.unwrap());
    pacs.late.store(true, Ordering::SeqCst);
    let gateway = format!("{base}/dicomweb/pacs");
    let study = format!("{gateway}/studies/1.2.3");
    let instance = format!("{study}/series/1.2.3.1/instances/1.2.3.1.1");
    pacs.large_metadata.store(true, Ordering::SeqCst);
    for url in [
        format!("{study}/metadata"),
        format!("{instance}/metadata"),
        instance.clone(),
    ] {
        let response = client.get(&url).send().await.unwrap();
        assert_eq!(
            response.status(),
            413,
            "{url}: {}",
            response.text().await.unwrap()
        );
        let outcome: Value = response.json().await.unwrap();
        assert_eq!(outcome["resourceType"], "OperationOutcome");
        assert_eq!(outcome["issue"][0]["code"], "too-costly");
        assert!(
            outcome["issue"][0]["diagnostics"]
                .as_str()
                .unwrap()
                .contains("max_metadata_bytes")
        );
    }
    pacs.large_metadata.store(false, Ordering::SeqCst);
    if remote {
        for fields in [
            "InstitutionName&includefield=PrivateOriginAttribute",
            "00080080,77770010",
            "all",
        ] {
            let response = client
                .get(format!(
                    "{gateway}/studies?StudyInstanceUID=1.2.3&includefield={fields}"
                ))
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), 200, "{}", response.text().await.unwrap());
            let rows: Value = response.json().await.unwrap();
            assert_eq!(rows[0]["00080080"]["Value"][0], "Verified institution");
            assert_eq!(rows[0]["77770010"]["Value"][0], "Origin-specific attribute");
        }
        for path in [
            "studies?StudyInstanceUID=1.2.3",
            "series?StudyInstanceUID=1.2.3",
            "instances?StudyInstanceUID=1.2.3",
            "studies/1.2.3/series?",
            "studies/1.2.3/instances?",
            "studies/1.2.3/series/1.2.3.1/instances?",
        ] {
            let response = client.get(format!("{gateway}/{path}&fuzzymatching=yes&PatientName=%D0%98%D0%B2%D0%B0%D0%BD%D0%BE%D0%B2%5E%D0%98%D0%B2%D0%B0%D0%BD&PrivateOriginFilter=a%2Bb%26c"))
                .send().await.unwrap();
            assert_eq!(response.status(), 200, "{}", response.text().await.unwrap());
            let rows: Value = response.json().await.unwrap();
            assert_eq!(rows.as_array().unwrap().len(), 1, "{path}: {rows}");
            assert_eq!(rows[0]["0020000D"]["Value"][0], "1.2.3");
        }
    }
    let response = client
        .get(format!(
            "{gateway}/studies?StudyInstanceUID=1.2.3&includefield=all"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200, "{}", response.text().await.unwrap());
    let rows: Value = response.json().await.unwrap();
    assert_eq!(rows[0]["00100020"]["Value"][0], "MRN-1");
    assert_eq!(rows[0]["00201206"]["Value"][0], 2);
    assert_eq!(rows[0]["00201208"]["Value"][0], 2);
    assert_eq!(rows[0]["00081030"]["Value"][0], "Synthetic & private");
    assert!(rows[0].get("00080018").is_none());
    for (path, count) in [
        ("studies", 1),
        ("series", 2),
        ("instances", 2),
        ("studies/1.2.3/series", 2),
        ("studies/1.2.3/instances", 2),
        ("studies/1.2.3/series/1.2.3.1/instances", 1),
    ] {
        let response = client
            .get(format!("{gateway}/{path}"))
            .send()
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            200,
            "{path}: {}",
            response.text().await.unwrap()
        );
        let rows: Value = response.json().await.unwrap();
        assert_eq!(rows.as_array().unwrap().len(), count, "{path}");
    }
    let rows:Value=client.get(format!("{gateway}/studies?StudyInstanceUID=1.2.3&PatientName=doe%2C%20jane*&StudyDate=20260201-20260228&StudyTime=1230&includefield=StudyDescription&includefield=IssuerOfPatientID")).send().await.unwrap().json().await.unwrap();
    assert_eq!(rows.as_array().unwrap().len(), 1);
    assert!(rows[0].get("00081030").is_some());
    assert!(rows[0].get("00100021").is_some());
    let rows:Value=client.get(format!("{study}/instances?RequestAttributesSequence.RequestedProcedureID=REQ-*&InstanceNumber=1")).send().await.unwrap().json().await.unwrap();
    assert_eq!(rows.as_array().unwrap().len(), 2);
    let empty: Value = client
        .get(format!(
            "{gateway}/studies?StudyInstanceUID=1.2.3&PatientID=OTHER"
        ))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(empty, json!([]));
    let response = client
        .get(format!(
            "{study}/metadata?accept=application%2Fdicom%2Bjson&charset=not-a-supported-charset"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let records: Value = response.json().await.unwrap();
    assert_eq!(records.as_array().unwrap().len(), 2);
    assert_eq!(
        records[0]["7FE00010"]["BulkDataURI"],
        format!(
            "{base}/dicomweb/pacs/studies/1.2.3/series/1.2.3.1/instances/1.2.3.1.1/bulkdata/7FE00010"
        )
    );
    for path in [
        format!("{study}/metadata"),
        format!("{gateway}/studies?StudyInstanceUID=1.2.3"),
    ] {
        let response = client
            .get(path)
            .header(
                header::ACCEPT,
                "multipart/related; type=\"application/dicom+xml\"",
            )
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let xml = response.text().await.unwrap();
        assert!(xml.contains("<NativeDicomModel"));
        assert!(xml.contains("<FamilyName>Doe, Jane</FamilyName>"));
        assert!(!xml.contains("opaque-3"));
    }
    assert_eq!(
        client
            .get(format!("{study}/metadata"))
            .header(header::ACCEPT, "application/not-supported")
            .send()
            .await
            .unwrap()
            .status(),
        406
    );
    assert_eq!(
        client
            .get(format!("{study}/metadata?accept=image%2Fjpeg"))
            .header(header::ACCEPT, "application/dicom+json")
            .send()
            .await
            .unwrap()
            .status(),
        400
    );
    assert_eq!(
        client
            .get(format!("{instance}/frames/2,1"))
            .send()
            .await
            .unwrap()
            .status(),
        400
    );
    assert_eq!(
        client
            .get(format!("{instance}/frames/1,1"))
            .send()
            .await
            .unwrap()
            .status(),
        400
    );
    let response = client
        .get(format!("{study}/instances?limit=1&includefield=all"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let next = response.headers()[header::LINK]
        .to_str()
        .unwrap()
        .split(['<', '>'])
        .nth(1)
        .unwrap()
        .to_owned();
    let warning = response.headers()["warning"].to_str().unwrap().to_owned();
    let first: Value = response.json().await.unwrap();
    assert_eq!(first.as_array().unwrap().len(), 1);
    let second: Value = client.get(next).send().await.unwrap().json().await.unwrap();
    assert_eq!(second.as_array().unwrap().len(), 1);
    assert_ne!(first[0]["00080018"], second[0]["00080018"]);
    assert_eq!(client.get(warning).send().await.unwrap().status(), 200);
    let response = client
        .request(reqwest::Method::OPTIONS, &gateway)
        .header(header::ACCEPT, "application/vnd.sun.wadl+xml")
        .header(header::ORIGIN, "https://viewer.example")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200, "{}", response.text().await.unwrap());
    let wadl = response.text().await.unwrap();
    assert!(wadl.contains("<application"));
    assert!(wadl.contains(&format!("base=\"{gateway}/\"")));
    assert_eq!(wadl.matches("path=\"thumbnail\"").count(), 4);
    assert!(!wadl.contains(&origin));
    let preflight = http
        .request(reqwest::Method::OPTIONS, &gateway)
        .header(header::ORIGIN, "https://viewer.example")
        .header(header::ACCESS_CONTROL_REQUEST_METHOD, "GET")
        .send()
        .await
        .unwrap();
    assert_eq!(preflight.status(), 204);
    assert_eq!(
        http.request(reqwest::Method::OPTIONS, &gateway)
            .header(header::ACCEPT, "application/vnd.sun.wadl+xml")
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    for path in [
        format!("{study}/thumbnail"),
        format!("{study}/series/1.2.3.1/thumbnail"),
        format!("{instance}/thumbnail"),
        format!("{instance}/frames/1/thumbnail"),
    ] {
        let response = client
            .get(path)
            .header(header::ACCEPT, "image/jpeg")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(response.headers()[header::CONTENT_TYPE], "image/jpeg");
    }
    pacs.object_calls.lock().unwrap().clear();
    let response = client
        .get(&study)
        .header(
            header::ACCEPT,
            "multipart/related; type=\"application/dicom\"; transfer-syntax=*",
        )
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert!(!response.headers().contains_key(header::SET_COOKIE));
    let content_type = response.headers()[header::CONTENT_TYPE]
        .to_str()
        .unwrap()
        .to_owned();
    let bytes = response.bytes().await.unwrap();
    let boundary = content_type
        .parse::<mime::Mime>()
        .unwrap()
        .get_param("boundary")
        .unwrap()
        .as_str()
        .to_owned();
    let mut multipart = multer::Multipart::new(
        futures_util::stream::once(async move { Ok::<_, std::io::Error>(bytes) }),
        boundary,
    );
    let mut count = 0;
    while let Some(field) = multipart.next_field().await.unwrap() {
        assert!(
            !field.headers()[header::CONTENT_LOCATION]
                .to_str()
                .unwrap()
                .contains("private")
        );
        let data = field.bytes().await.unwrap();
        count += 1;
        assert_eq!(data.len(), 256);
        assert!(data.iter().all(|b| *b == count));
    }
    assert_eq!(count, 2);
    let calls = pacs.object_calls.lock().unwrap().clone();
    assert_eq!(calls.len(), 2);
    assert!(!calls.iter().any(|p| p.contains("1.2.3.3")));
    let response = client
        .get(format!("{study}/series/1.2.3.1"))
        .header(
            header::ACCEPT,
            "multipart/related; type=\"application/octet-stream\"",
        )
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert!(response.bytes().await.unwrap().len() > 256);
    let response = client
        .get(format!("{study}/rendered"))
        .header(header::ACCEPT, "multipart/related; type=\"image/jpeg\"")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(
        response
            .text()
            .await
            .unwrap()
            .matches("SYNTHETIC-JPEG")
            .count(),
        2
    );
    assert_eq!(
        client
            .get(&study)
            .header(header::RANGE, "bytes=0-10")
            .send()
            .await
            .unwrap()
            .status(),
        416
    );
    let response = client
        .get(format!("{instance}/bulkdata/7FE00010"))
        .header(
            header::ACCEPT,
            "multipart/related; type=\"application/octet-stream\"",
        )
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert!(response.bytes().await.unwrap().len() > 256);
    let parts = decoded_parts(
        client
            .get(&study)
            .header(
                header::ACCEPT,
                "multipart/related; type=\"application/octet-stream\"",
            )
            .send()
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(parts.len(), 2);
    assert!(parts[0].iter().all(|v| *v == 1));
    assert!(parts[1].iter().all(|v| *v == 2));
    let parts = decoded_parts(
        client
            .get(format!("{study}/pixeldata"))
            .header(
                header::ACCEPT,
                "multipart/related; type=\"application/octet-stream\"",
            )
            .send()
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(parts.len(), 2);
    let second_instance = format!("{study}/series/1.2.3.2/instances/1.2.3.2.1");
    let parts = decoded_parts(
        client
            .get(format!("{second_instance}/pixeldata"))
            .header(
                header::ACCEPT,
                "multipart/related; type=\"application/octet-stream\"",
            )
            .send()
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(parts.len(), 1);
    assert_eq!(parts[0].len(), 256);
    let response = client
        .get(format!("{second_instance}/pixeldata"))
        .header(header::ACCEPT, "application/octet-stream")
        .header(header::RANGE, "bytes=10-19")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 206);
    assert_eq!(response.headers()[header::CONTENT_RANGE], "bytes 10-19/256");
    assert_eq!(response.bytes().await.unwrap().as_ref(), &[2u8; 10]);
    pacs.large.store(true, Ordering::SeqCst);
    pacs.object_calls.lock().unwrap().clear();
    let before = pacs.closed.load(Ordering::SeqCst);
    let mut response = client
        .get(&study)
        .header(
            header::ACCEPT,
            "multipart/related; type=\"application/dicom\"",
        )
        .send()
        .await
        .unwrap();
    assert!(response.chunk().await.unwrap().is_some());
    drop(response);
    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    assert!(pacs.closed.load(Ordering::SeqCst) > before);
    assert_eq!(
        pacs.object_calls.lock().unwrap().len(),
        1,
        "cancelled collection fetched another SOP"
    );
    app_task.abort();
    archive_task.abort();
}
