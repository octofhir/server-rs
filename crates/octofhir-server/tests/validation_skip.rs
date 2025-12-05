//! Integration tests for X-Skip-Validation header support.
//!
//! These tests verify that validation can be skipped when:
//! 1. The X-Skip-Validation header is present and set to "true"
//! 2. The allow_skip_validation config option is enabled
//!
//! Tests also verify that the feature is disabled by default for security.

use axum::http::StatusCode;
use octofhir_server::{AppConfig, PostgresStorageConfig, build_app};
use serde_json::json;
use std::sync::Arc;
use testcontainers::{ContainerAsync, ImageExt, runners::AsyncRunner};
use testcontainers_modules::postgres::Postgres;
use tokio::sync::{Mutex, OnceCell};
use tokio::task::JoinHandle;

/// Type alias for the shared PostgreSQL container and connection URL
type SharedPostgres = Arc<Mutex<(ContainerAsync<Postgres>, String)>>;

// Shared PostgreSQL container for all tests
// We keep the container alive by storing it in a static OnceCell
static SHARED_POSTGRES: OnceCell<SharedPostgres> = OnceCell::const_new();

/// Helper to get the shared PostgreSQL URL
/// The container is started once and reused across all tests
async fn get_postgres_url() -> String {
    let container_arc = SHARED_POSTGRES
        .get_or_init(|| async {
            let container = Postgres::default()
                .with_tag("17-alpine")
                .start()
                .await
                .expect("start postgres container");

            let host_port = container.get_host_port_ipv4(5432).await.expect("get port");
            let url = format!(
                "postgres://postgres:postgres@127.0.0.1:{}/postgres",
                host_port
            );

            Arc::new(Mutex::new((container, url)))
        })
        .await;

    let guard = container_arc.lock().await;
    guard.1.clone()
}

/// Create an AppConfig that uses the given PostgreSQL URL
fn create_config(postgres_url: &str) -> AppConfig {
    let mut config = AppConfig::default();
    config.storage.postgres = Some(PostgresStorageConfig {
        url: Some(postgres_url.to_string()),
        pool_size: 5,
        connect_timeout_ms: 10000,
        idle_timeout_ms: Some(60000),
        ..Default::default()
    });
    config
}

// Initialize canonical manager once for all tests
static CANONICAL_INIT: OnceCell<()> = OnceCell::const_new();

async fn init_canonical_once() {
    CANONICAL_INIT
        .get_or_init(|| async {
            let postgres_url = get_postgres_url().await;

            let mut config = AppConfig::default();
            config.storage.postgres = Some(PostgresStorageConfig {
                url: Some(postgres_url),
                pool_size: 5,
                connect_timeout_ms: 10000,
                idle_timeout_ms: Some(60000),
                ..Default::default()
            });

            // Set FHIR version to R4B to match the test expectations
            config.fhir.version = "R4B".to_string();

            match octofhir_server::canonical::init_from_config_async(&config).await {
                Ok(registry) => {
                    octofhir_server::canonical::set_registry(registry);
                }
                Err(e) => {
                    panic!("Canonical manager init failed: {}", e);
                }
            }
        })
        .await;
}

async fn start_server(
    config: &AppConfig,
) -> (String, tokio::sync::oneshot::Sender<()>, JoinHandle<()>) {
    // Initialize canonical manager once (shared across all tests)
    init_canonical_once().await;

    let app = build_app(config).await.expect("build app");

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

    (format!("http://{addr}"), tx, server)
}

#[tokio::test]
async fn test_skip_validation_disabled_by_default() {
    let postgres_url = get_postgres_url().await;
    let config = create_config(&postgres_url);
    let (base, shutdown_tx, handle) = start_server(&config).await;
    let client = reqwest::Client::new();

    // Create valid Patient with X-Skip-Validation header
    let patient = json!({
        "resourceType": "Patient",
        "active": true,
        "name": [{"family": "Doe", "given": ["John"]}],
    });

    // Try with X-Skip-Validation header - feature is disabled by default
    let resp = client
        .post(format!("{base}/Patient"))
        .header("accept", "application/fhir+json")
        .header("content-type", "application/fhir+json")
        .header("X-Skip-Validation", "true")
        .json(&patient)
        .send()
        .await
        .unwrap();

    // Should succeed (valid resource) but validation still runs
    assert_eq!(resp.status(), StatusCode::CREATED);

    // X-Validation-Skipped header should NOT be present
    let skipped_header = resp.headers().get("X-Validation-Skipped");
    assert!(skipped_header.is_none());

    shutdown_tx.send(()).unwrap();
    handle.await.unwrap();
}

#[tokio::test]
async fn test_skip_validation_enabled_create() {
    let postgres_url = get_postgres_url().await;
    let mut config = create_config(&postgres_url);

    // Enable skip validation feature
    config.validation.allow_skip_validation = true;

    let (base, shutdown_tx, handle) = start_server(&config).await;
    let client = reqwest::Client::new();

    // Create Patient with X-Skip-Validation header
    let patient = json!({
        "resourceType": "Patient",
        "active": true,
        "name": [{"family": "Doe", "given": ["John"]}],
    });

    let resp = client
        .post(format!("{base}/Patient"))
        .header("accept", "application/fhir+json")
        .header("content-type", "application/fhir+json")
        .header("X-Skip-Validation", "true")
        .json(&patient)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::CREATED);

    // Check that X-Validation-Skipped header is present
    let skipped_header = resp.headers().get("X-Validation-Skipped");
    assert!(skipped_header.is_some());
    assert_eq!(skipped_header.unwrap(), "true");

    shutdown_tx.send(()).unwrap();
    handle.await.unwrap();
}

#[tokio::test]
async fn test_skip_validation_enabled_update() {
    let postgres_url = get_postgres_url().await;
    let mut config = create_config(&postgres_url);
    config.validation.allow_skip_validation = true;

    let (base, shutdown_tx, handle) = start_server(&config).await;
    let client = reqwest::Client::new();

    // First create a valid patient
    let patient = json!({
        "resourceType": "Patient",
        "active": true,
        "name": [{"family": "Doe", "given": ["John"]}],
    });

    let create_resp = client
        .post(format!("{base}/Patient"))
        .header("accept", "application/fhir+json")
        .header("content-type", "application/fhir+json")
        .json(&patient)
        .send()
        .await
        .unwrap();

    assert_eq!(create_resp.status(), StatusCode::CREATED);
    let created: serde_json::Value = create_resp.json().await.unwrap();
    let patient_id = created["id"].as_str().unwrap();

    // Update with X-Skip-Validation header
    let updated_patient = json!({
        "resourceType": "Patient",
        "id": patient_id,
        "active": false,
        "name": [{"family": "Doe", "given": ["Jane"]}],
    });

    let update_resp = client
        .put(format!("{base}/Patient/{patient_id}"))
        .header("accept", "application/fhir+json")
        .header("content-type", "application/fhir+json")
        .header("X-Skip-Validation", "true")
        .json(&updated_patient)
        .send()
        .await
        .unwrap();

    assert_eq!(update_resp.status(), StatusCode::OK);

    // Check that X-Validation-Skipped header is present
    let skipped_header = update_resp.headers().get("X-Validation-Skipped");
    assert!(skipped_header.is_some());
    assert_eq!(skipped_header.unwrap(), "true");

    shutdown_tx.send(()).unwrap();
    handle.await.unwrap();
}

#[tokio::test]
async fn test_validation_runs_without_header() {
    let postgres_url = get_postgres_url().await;
    let mut config = create_config(&postgres_url);
    config.validation.allow_skip_validation = true;

    let (base, shutdown_tx, handle) = start_server(&config).await;
    let client = reqwest::Client::new();

    // Create valid patient WITHOUT X-Skip-Validation header
    let patient = json!({
        "resourceType": "Patient",
        "active": true,
        "name": [{"family": "Smith", "given": ["Bob"]}],
    });

    let resp = client
        .post(format!("{base}/Patient"))
        .header("accept", "application/fhir+json")
        .header("content-type", "application/fhir+json")
        .json(&patient)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::CREATED);

    // X-Validation-Skipped header should NOT be present
    let skipped_header = resp.headers().get("X-Validation-Skipped");
    assert!(skipped_header.is_none());

    shutdown_tx.send(()).unwrap();
    handle.await.unwrap();
}
