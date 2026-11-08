//! HTTP gateway authorization, manifest provenance and streaming cancellation.
//! PostgreSQL is disposable; PACS emits incremental frames without a buffered body.
use axum::{
    Json, Router,
    body::Body,
    extract::{Path, State},
    http::{HeaderMap, header},
    response::{IntoResponse, Response},
    routing::get,
};
use octofhir_config::ConfigurationManager;
use octofhir_server::{AppConfig, PostgresStorageConfig, build_app};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use testcontainers::{ImageExt, runners::AsyncRunner};
use testcontainers_modules::postgres::Postgres;

#[derive(Clone, Default)]
struct Pacs {
    requests: Arc<AtomicUsize>,
    closed: Arc<AtomicUsize>,
    sent: Arc<AtomicUsize>,
}
struct DropStream(Arc<AtomicUsize>);
impl Drop for DropStream {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}
fn dataset() -> Value {
    json!({
     "0020000D":{"vr":"UI","Value":["1.2.3"]},"0020000E":{"vr":"UI","Value":["1.2.3.1"]},"00080018":{"vr":"UI","Value":["1.2.3.1.1"]},"00080016":{"vr":"UI","Value":["1.2.840.10008.5.1.4.1.1.2"]},"00080060":{"vr":"CS","Value":["CT"]},"00100020":{"vr":"LO","Value":["MRN-1"]},"00100021":{"vr":"LO","Value":["HOSPITAL"]},
     "7FE00010":{"vr":"OW","BulkDataURI":"/wado/studies/1.2.3/series/1.2.3.1/instances/1.2.3.1.1/pixeldata"}
    })
}
async fn archive(
    State(state): State<Pacs>,
    Path(path): Path<String>,
    headers: HeaderMap,
) -> Response {
    state.requests.fetch_add(1, Ordering::SeqCst);
    assert!(
        !headers.contains_key(header::AUTHORIZATION),
        "FHIR bearer token reached PACS"
    );
    if path.ends_with("metadata") {
        return (
            [(header::CONTENT_TYPE, "application/dicom+json")],
            Json(json!([dataset()])),
        )
            .into_response();
    }
    if headers
        .get(header::RANGE)
        .is_some_and(|v| v == "bytes=999999999-")
    {
        return (
            axum::http::StatusCode::RANGE_NOT_SATISFIABLE,
            [
                (header::CONTENT_RANGE, "bytes */33554432"),
                (header::SET_COOKIE, "secret=hidden"),
            ],
            "private PACS failure",
        )
            .into_response();
    }
    if headers
        .get(header::ACCEPT)
        .is_some_and(|v| v == "application/test-unsupported")
    {
        return (
            axum::http::StatusCode::NOT_ACCEPTABLE,
            "private PACS failure",
        )
            .into_response();
    }
    assert!(path.starts_with("studies/1.2.3/series/1.2.3.1/instances/1.2.3.1.1/"));
    assert!(
        headers
            .get(header::ACCEPT)
            .unwrap()
            .to_str()
            .unwrap()
            .contains("transfer-syntax=*")
    );
    let count = if path.ends_with("frames/2") { 100 } else { 32 };
    let guard = DropStream(state.closed.clone());
    let chunks = futures_util::stream::unfold(
        (0, guard, state.clone()),
        move |(index, guard, state)| async move {
            if index == count {
                return None;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
            state.sent.fetch_add(1, Ordering::SeqCst);
            Some((
                Ok::<_, std::io::Error>(vec![index as u8; 1024 * 1024]),
                (index + 1, guard, state),
            ))
        },
    );
    let mut response = Response::new(Body::from_stream(chunks));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        "multipart/related; type=\"application/octet-stream\"; boundary=test"
            .parse()
            .unwrap(),
    );
    response.headers_mut().insert(
        header::SET_COOKIE,
        "PACS-secret=must-not-escape".parse().unwrap(),
    );
    response
}
#[test]
fn authorizes_attested_membership_and_streams_without_persistence() {
    std::thread::Builder::new()
        .name("dicom-gateway".into())
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
    let pg = Postgres::default()
        .with_tag("18-alpine")
        .start()
        .await
        .unwrap();
    let database = format!(
        "postgres://postgres:postgres@127.0.0.1:{}/postgres",
        pg.get_host_port_ipv4(5432).await.unwrap()
    );
    let pacs = Pacs::default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let archive_url = format!("http://{}", listener.local_addr().unwrap());
    let archive_state = pacs.clone();
    let archive_task = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new()
                .route("/wado/{*path}", get(archive))
                .with_state(archive_state),
        )
        .await
        .unwrap()
    });
    let mut config = AppConfig::default();
    config.fhir.version = "R4".into();
    config.storage.postgres = Some(PostgresStorageConfig {
        url: Some(database.clone()),
        pool_size: 8,
        ..Default::default()
    });
    config.packages.load = vec![octofhir_server::config::PackageSpec::Table {
        id: Some("hl7.fhir.r4.core".into()),
        version: Some("4.0.1".into()),
        path: None,
        url: Some("https://hl7.org/fhir/R4/hl7.fhir.r4.core.tgz".into()),
    }];
    config.auth.policy.anonymous_access = true;
    config.bootstrap.backend_client = Some(octofhir_server::config::BackendClientConfig {
        client_id: "gateway-test".into(),
        client_secret: Some("isolated-gateway-secret".into()),
        scopes: vec!["system/*.cruds".into()],
        name: None,
        description: None,
    });
    config.server.outbound_allowlist = vec!["127.0.0.1/32".into()];
    config.dicom=serde_json::from_value(json!({"connections":{"pacs":{"wado_url":format!("{archive_url}/wado"),"allow_http":true,"patient_identifier_systems":{"HOSPITAL":"urn:test:mrn"},"gateway":{"max_stream_bytes":64*1024*1024,"stream_seconds":30,"idle_seconds":5}}}})).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    config.server.base_url = Some(base.clone());
    let app = build_app(
        &config,
        Arc::new(ConfigurationManager::builder().build().await.unwrap()),
    )
    .await
    .unwrap();
    let app_task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let anonymous = reqwest::Client::new();
    let patient:Value=anonymous.post(format!("{base}/fhir/Patient")).json(&json!({"resourceType":"Patient","identifier":[{"system":"urn:test:mrn","value":"MRN-1"}]})).send().await.unwrap().json().await.unwrap();
    let operation = format!(
        "{base}/fhir/Patient/{}/$dicom-import",
        patient["id"].as_str().unwrap()
    );
    let params = json!({"resourceType":"Parameters","parameter":[{"name":"connection","valueString":"pacs"},{"name":"study","valueString":"1.2.3"}]});
    let imported: Value = anonymous
        .post(&operation)
        .json(&params)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let study_ref = imported["parameter"][0]["valueReference"]["reference"]
        .as_str()
        .expect("import response");
    let study_url = format!("{base}/fhir/{study_ref}");
    let token = |scope: &str| {
        anonymous
            .post(format!("{base}/auth/token"))
            .basic_auth("gateway-test", Some("isolated-gateway-secret"))
            .form(&[("grant_type", "client_credentials"), ("scope", scope)])
    };
    let full: Value = token("system/*.cruds")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let narrow: Value = token("system/Patient.r")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let mut auth = HeaderMap::new();
    auth.insert(
        header::AUTHORIZATION,
        format!("Bearer {}", full["access_token"].as_str().unwrap())
            .parse()
            .unwrap(),
    );
    let client = reqwest::Client::builder()
        .default_headers(auth)
        .build()
        .unwrap();
    let root = format!("{base}/dicomweb/pacs/studies/1.2.3");
    let instance = format!("{root}/series/1.2.3.1/instances/1.2.3.1.1");
    let before = pacs.requests.load(Ordering::SeqCst);
    assert_eq!(
        anonymous
            .get(format!("{root}/metadata"))
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    assert_eq!(
        anonymous
            .get(format!("{root}/metadata"))
            .bearer_auth(narrow["access_token"].as_str().unwrap())
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    assert_eq!(
        client
            .get(format!("{root}/series/1.2.3.1/instances/9.9/frames/1"))
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
    assert_eq!(pacs.requests.load(Ordering::SeqCst), before);
    let response = client.get(format!("{root}/metadata")).send().await.unwrap();
    assert_eq!(response.status(), 200, "{}", response.text().await.unwrap());
    let result: Value = client
        .get(format!("{root}/metadata"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let uri = result[0]["7FE00010"]["BulkDataURI"].as_str().unwrap();
    assert_eq!(
        uri,
        format!(
            "{base}/dicomweb/pacs/studies/1.2.3/series/1.2.3.1/instances/1.2.3.1.1/bulkdata/7FE00010"
        )
    );
    // The trusted record is independent of ordinary FHIR updates, even with a source tag.
    let original: Value = client
        .get(&study_url)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let mut forged = original.clone();
    forged["series"][0]["instance"][0]["uid"] = json!("9.9");
    let updated = client.put(&study_url).json(&forged).send().await.unwrap();
    assert!(
        updated.status().is_success(),
        "{}",
        updated.text().await.unwrap()
    );
    let before = pacs.requests.load(Ordering::SeqCst);
    assert_eq!(
        client
            .get(format!("{root}/metadata"))
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
    assert_eq!(pacs.requests.load(Ordering::SeqCst), before);
    let repaired = client.post(&operation).json(&params).send().await.unwrap();
    assert_eq!(repaired.status(), 200, "{}", repaired.text().await.unwrap());
    // Clinical annotations remain readable without re-importing the projection.
    let mut annotated: Value = client
        .get(&study_url)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    annotated["note"] = json!([{"text":"Clinical note"}]);
    assert!(
        client
            .put(&study_url)
            .json(&annotated)
            .send()
            .await
            .unwrap()
            .status()
            .is_success()
    );
    let accept = "multipart/related; type=\"application/octet-stream\"; transfer-syntax=*";
    let rejected = client
        .get(format!("{instance}/frames/1"))
        .header(header::ACCEPT, "application/test-unsupported")
        .send()
        .await
        .unwrap();
    assert_eq!(rejected.status(), 406);
    assert!(!rejected.text().await.unwrap().contains("private PACS"));
    let rejected = client
        .get(format!("{instance}/frames/1"))
        .header(header::RANGE, "bytes=999999999-")
        .send()
        .await
        .unwrap();
    assert_eq!(rejected.status(), 416);
    assert_eq!(
        rejected.headers()[header::CONTENT_RANGE],
        "bytes */33554432"
    );
    assert!(!rejected.headers().contains_key(header::SET_COOKIE));
    assert!(!rejected.text().await.unwrap().contains("private PACS"));
    let response = client
        .get(format!("{instance}/frames/1"))
        .header(header::ACCEPT, accept)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert!(!response.headers().contains_key(header::SET_COOKIE));
    let data = response.bytes().await.unwrap();
    assert_eq!(data.len(), 32 * 1024 * 1024);
    for (index, chunk) in data.as_chunks::<{ 1024 * 1024 }>().0.iter().enumerate() {
        assert!(chunk.iter().all(|byte| *byte == index as u8));
    }
    let mut response = client
        .get(format!("{instance}/frames/1"))
        .header(header::ACCEPT, accept)
        .send()
        .await
        .unwrap();
    assert!(response.chunk().await.unwrap().is_some());
    let closed = pacs.closed.load(Ordering::SeqCst);
    drop(response);
    for _ in 0..80 {
        if pacs.closed.load(Ordering::SeqCst) > closed {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert!(
        pacs.closed.load(Ordering::SeqCst) > closed,
        "PACS body survived downstream cancellation"
    );
    let response = client
        .get(format!("{instance}/frames/2"))
        .header(header::ACCEPT, accept)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert!(
        response.bytes().await.is_err(),
        "Chunked size limit was not enforced"
    );
    let pool = sqlx_postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(&database)
        .await
        .unwrap();
    let proof_size: i64 = sqlx_core::query_scalar::query_scalar(
        "SELECT max(pg_column_size(m))::bigint FROM dicom_manifests m",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(proof_size < 2048);
    let stored_size: i64 = sqlx_core::query_scalar::query_scalar(
        "SELECT max(octet_length(resource::text))::bigint FROM imagingstudy",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(stored_size < 32768);
    app_task.abort();
    archive_task.abort();
}
