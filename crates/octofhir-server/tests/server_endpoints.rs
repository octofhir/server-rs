use std::sync::Arc;

use octofhir_config::ConfigurationManager;
use octofhir_server::{AppConfig, PostgresStorageConfig, build_app};
use serde_json::Value;
use testcontainers::{ContainerAsync, ImageExt, runners::AsyncRunner};
use testcontainers_modules::postgres::Postgres;
use tokio::task::JoinHandle;

async fn start_server() -> (
    String,
    tokio::sync::oneshot::Sender<()>,
    JoinHandle<()>,
    ContainerAsync<Postgres>,
) {
    let postgres = Postgres::default()
        .with_tag("17-alpine")
        .start()
        .await
        .expect("start postgres container");
    let port = postgres.get_host_port_ipv4(5432).await.expect("get port");

    let mut config = AppConfig::default();
    config.storage.postgres = Some(PostgresStorageConfig {
        url: Some(format!(
            "postgres://postgres:postgres@127.0.0.1:{port}/postgres"
        )),
        pool_size: 5,
        connect_timeout_ms: 10000,
        ..Default::default()
    });
    config.auth.policy.anonymous_access = true;

    let config_manager = Arc::new(
        ConfigurationManager::builder()
            .build()
            .await
            .expect("build config manager"),
    );
    let app = build_app(&config, config_manager).await.expect("build app");

    // Bind to an ephemeral port
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("bind");
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();

    let server = tokio::spawn(async move {
        let _ = axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = rx.await;
            })
            .await;
    });

    (format!("http://{addr}"), tx, server, postgres)
}

#[tokio::test]
async fn server_endpoints_work() {
    let (base, shutdown_tx, handle, _postgres) = start_server().await;
    let client = reqwest::Client::new();
    let fhir_base = format!("{base}/fhir");

    // GET /
    let resp = client
        .get(format!("{base}/"))
        .header("accept", "application/fhir+json")
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["service"], "OctoFHIR Server");
    assert_eq!(body["status"], "ok");

    // GET /healthz
    let resp = client
        .get(format!("{base}/healthz"))
        .header("accept", "application/fhir+json")
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "ok");

    // GET /readyz
    let resp = client
        .get(format!("{base}/readyz"))
        .header("accept", "application/fhir+json")
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "ready");

    // GET /fhir/metadata
    let resp = client
        .get(format!("{fhir_base}/metadata"))
        .header("accept", "application/fhir+json")
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["resourceType"], "CapabilityStatement");
    assert_eq!(
        body["rest"][0]["patchFormat"][0],
        "application/json-patch+json"
    );
    assert_eq!(body["rest"][0]["patchFormat"][1], "application/fhir+json");

    let patient = body["rest"][0]["resource"]
        .as_array()
        .unwrap()
        .iter()
        .find(|resource| resource["type"] == "Patient")
        .expect("Patient capability");
    let interactions = patient["interaction"].as_array().unwrap();
    assert!(
        interactions
            .iter()
            .any(|interaction| interaction["code"] == "patch")
    );
    assert_eq!(patient["versioning"], "versioned-update");
    assert_eq!(patient["readHistory"], true);
    assert_eq!(patient["updateCreate"], true);
    assert_eq!(patient["conditionalCreate"], true);
    assert_eq!(patient["conditionalRead"], "full-support");
    assert_eq!(patient["conditionalUpdate"], true);
    assert_eq!(patient["conditionalDelete"], "single");

    // GET /fhir/Patient (search placeholder)
    let resp = client
        .get(format!("{fhir_base}/Patient"))
        .header("accept", "application/fhir+json")
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());

    // shutdown
    let _ = shutdown_tx.send(());
    let _ = handle.await;
}
