//! `$viewdefinition-run` (SQL on FHIR) against the server's own data.
//!
//! Covers the operation as the IG defines it: the rows returned as the
//! requested format rather than a Parameters envelope, the data filters
//! (`patient`, `_since`), `_limit`, the 400 for a filter value that cannot be
//! resolved, and that a view over the server's data (run as SQL) returns the
//! same rows as the same view over the same resources supplied inline (run in
//! memory).
//!
//! Requirements: Docker running (testcontainers).

use std::sync::Arc;

use octofhir_config::ConfigurationManager;
use octofhir_server::{AppConfig, PostgresStorageConfig, build_app};
use reqwest::{Client, StatusCode};
use serde_json::{Value, json};
use testcontainers::{ImageExt, runners::AsyncRunner};
use testcontainers_modules::postgres::Postgres;

struct ServerHandle(tokio::task::JoinHandle<()>);

impl Drop for ServerHandle {
    fn drop(&mut self) {
        self.0.abort();
    }
}

async fn start_server(postgres_url: &str) -> (String, ServerHandle) {
    let mut config = AppConfig::default();
    config.storage.postgres = Some(PostgresStorageConfig {
        url: Some(postgres_url.to_owned()),
        pool_size: 5,
        connect_timeout_ms: 10_000,
        idle_timeout_ms: Some(60_000),
        ..Default::default()
    });
    config.auth.policy.anonymous_access = true;
    config.sql_on_fhir.enabled = true;
    config.packages.load = vec![octofhir_server::config::PackageSpec::Table {
        id: Some("hl7.fhir.r4.core".into()),
        version: Some("4.0.1".into()),
        path: None,
        url: Some("https://hl7.org/fhir/R4/hl7.fhir.r4.core.tgz".into()),
    }];
    let config_manager = Arc::new(
        ConfigurationManager::builder()
            .build()
            .await
            .expect("config manager"),
    );
    let app = build_app(&config, config_manager).await.expect("build app");
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("bind");
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (format!("http://{addr}/fhir"), ServerHandle(server))
}

async fn create(client: &Client, fhir: &str, resource: Value) -> Value {
    let resource_type = resource["resourceType"].as_str().unwrap().to_owned();
    let response = client
        .post(format!("{fhir}/{resource_type}"))
        .json(&resource)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    response.json().await.unwrap()
}

/// POST `$viewdefinition-run` with `parameters`; the status, content type and body.
async fn run(client: &Client, url: &str, parameters: Value) -> (StatusCode, String, String) {
    let response = client
        .post(url)
        .json(&json!({"resourceType": "Parameters", "parameter": parameters}))
        .send()
        .await
        .unwrap();
    let status = response.status();
    let content_type = response
        .headers()
        .get("content-type")
        .map(|value| value.to_str().unwrap().to_owned())
        .unwrap_or_default();
    (status, content_type, response.text().await.unwrap())
}

fn ndjson(body: &str) -> Vec<Value> {
    let mut rows: Vec<Value> = body
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    rows.sort_by_key(|row| row.to_string());
    rows
}

#[tokio::test]
async fn viewdefinition_run_over_server_data() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_test_writer()
        .try_init();
    let postgres = Postgres::default()
        .with_tag("17-alpine")
        .start()
        .await
        .expect("postgres");
    let port = postgres.get_host_port_ipv4(5432).await.unwrap();
    let (fhir, _server) = start_server(&format!(
        "postgres://postgres:postgres@127.0.0.1:{port}/postgres"
    ))
    .await;
    let client = Client::new();
    let system = format!("{fhir}/$viewdefinition-run");
    let typed = format!("{fhir}/ViewDefinition/$viewdefinition-run");

    let alice = create(
        &client,
        &fhir,
        json!({"resourceType":"Patient","active":true,
        "name":[{"family":"Alice"}]}),
    )
    .await;
    let alice_id = alice["id"].as_str().unwrap().to_owned();
    for code in ["8867-4", "8480-6"] {
        create(
            &client,
            &fhir,
            json!({"resourceType":"Observation","status":"final",
            "code":{"coding":[{"system":"http://loinc.org","code":code}]},
            "subject":{"reference":format!("Patient/{alice_id}")}}),
        )
        .await;
    }
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    let between = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, true);
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    let bob = create(
        &client,
        &fhir,
        json!({"resourceType":"Patient","active":false,
        "name":[{"family":"Bob"}]}),
    )
    .await;
    let bob_id = bob["id"].as_str().unwrap().to_owned();
    create(
        &client,
        &fhir,
        json!({"resourceType":"Observation","status":"final",
        "code":{"coding":[{"system":"http://loinc.org","code":"8867-4"}]},
        "subject":{"reference":format!("Patient/{bob_id}")}}),
    )
    .await;

    let patients = json!({"resourceType":"ViewDefinition","status":"active","resource":"Patient",
        "select":[{"column":[{"name":"id","path":"id"},{"name":"active","path":"active"}]}]});
    let observations = json!({"resourceType":"ViewDefinition","status":"active",
        "resource":"Observation","select":[{"column":[
            {"name":"subject","path":"subject.getReferenceKey(Patient)"},
            {"name":"code","path":"code.coding.code.first()"}]}]});

    // ndjson by default, raw rows, untyped primitives keep their JSON type.
    let (status, content_type, body) = run(
        &client,
        &system,
        json!([{"name":"viewResource","resource":patients}]),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(content_type, "application/x-ndjson");
    assert_eq!(
        ndjson(&body),
        ndjson(&format!(
            "{}\n{}",
            json!({"id":alice_id,"active":true}),
            json!({"id":bob_id,"active":false})
        ))
    );

    // csv with the header row; type level gives the same view.
    let (status, content_type, body) = run(
        &client,
        &typed,
        json!([
            {"name":"viewResource","resource":patients},{"name":"_format","valueCode":"csv"}
        ]),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(content_type.starts_with("text/csv"), "{content_type}");
    assert_eq!(body.lines().next(), Some("id,active"));
    assert_eq!(body.lines().count(), 3, "{body}");

    // patient: only Alice's compartment.
    let (status, _, body) = run(
        &client,
        &typed,
        json!([
            {"name":"viewResource","resource":observations},
            {"name":"patient","valueReference":{"reference":format!("Patient/{alice_id}")}}
        ]),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let rows = ndjson(&body);
    assert_eq!(rows.len(), 2, "{body}");
    assert!(rows.iter().all(|row| row["subject"] == alice_id), "{body}");

    // _since: only what changed after Alice's records.
    let (status, _, body) = run(
        &client,
        &typed,
        json!([
            {"name":"viewResource","resource":patients},
            {"name":"_since","valueInstant":between}
        ]),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(ndjson(&body), vec![json!({"id":bob_id,"active":false})]);

    // _limit caps the rows.
    let (status, _, body) = run(
        &client,
        &typed,
        json!([
            {"name":"viewResource","resource":observations},{"name":"_limit","valueInteger":1}
        ]),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body.lines().count(), 1, "{body}");

    // A patient that cannot be found is a 400, not a 404.
    let (status, _, body) = run(
        &client,
        &typed,
        json!([
            {"name":"viewResource","resource":observations},
            {"name":"patient","valueReference":{"reference":"Patient/missing"}}
        ]),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");

    // Exactly one of viewReference and viewResource.
    let (status, _, body) = run(
        &client,
        &typed,
        json!([
            {"name":"viewResource","resource":patients},
            {"name":"viewReference","valueReference":{"reference":"ViewDefinition/x"}}
        ]),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");

    // The view over the server's data (SQL) and over the same resources
    // supplied inline (in memory) return the same rows.
    let search: Value = client
        .get(format!("{fhir}/Observation?_count=100"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let inline: Vec<Value> = search["entry"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| json!({"name":"resource","resource":entry["resource"]}))
        .collect();
    let (_, _, over_server) = run(
        &client,
        &typed,
        json!([{"name":"viewResource","resource":observations}]),
    )
    .await;
    let mut parameters = vec![json!({"name":"viewResource","resource":observations})];
    parameters.extend(inline);
    let (status, _, over_inline) = run(&client, &typed, Value::Array(parameters)).await;
    assert_eq!(status, StatusCode::OK, "{over_inline}");
    assert_eq!(ndjson(&over_server), ndjson(&over_inline));
    assert_eq!(ndjson(&over_server).len(), 3);
}
