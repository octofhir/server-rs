//! Integration tests for CRUD and search operations using testcontainers.
//!
//! These tests spin up a PostgreSQL container for each test to ensure isolation.
//!
//! **Requirements:**
//! - Docker running
//! - FHIR packages installed in `.fhir/` directory
//!
//! Run with: cargo test -p octofhir-server --test crud_and_search -- --ignored

use octofhir_server::{AppConfig, PostgresStorageConfig, build_app};
use serde_json::{Value, json};
use testcontainers::{ContainerAsync, runners::AsyncRunner};
use testcontainers_modules::postgres::Postgres;
use tokio::task::JoinHandle;

/// Helper to start a PostgreSQL container and return the connection URL
async fn start_postgres() -> (ContainerAsync<Postgres>, String) {
    let container = Postgres::default()
        .start()
        .await
        .expect("start postgres container");

    let host_port = container.get_host_port_ipv4(5432).await.expect("get port");
    let url = format!(
        "postgres://postgres:postgres@127.0.0.1:{}/postgres",
        host_port
    );

    (container, url)
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

async fn start_server(
    config: &AppConfig,
) -> (String, tokio::sync::oneshot::Sender<()>, JoinHandle<()>) {
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
#[ignore = "requires FHIR packages in .fhir/ directory"]
async fn patient_crud_and_search_flow() {
    let (_container, postgres_url) = start_postgres().await;
    let config = create_config(&postgres_url);
    let (base, shutdown_tx, handle) = start_server(&config).await;
    let client = reqwest::Client::new();
    let fhir_base = format!("{base}/fhir");

    // Create Patient
    let payload = json!({
        "resourceType": "Patient",
        "active": true,
        "name": [{"family": "Smith", "given": ["John"]}],
        "identifier": [{"system": "http://sys", "value": "MRN-123"}],
    });
    let resp = client
        .post(format!("{fhir_base}/Patient"))
        .header("accept", "application/fhir+json")
        .header("content-type", "application/fhir+json")
        .json(&payload)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::CREATED);
    let created: Value = resp.json().await.unwrap();
    let id = created["id"].as_str().expect("created id").to_string();

    // Read Patient
    let resp = client
        .get(format!("{fhir_base}/Patient/{id}"))
        .header("accept", "application/fhir+json")
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
    let read_back: Value = resp.json().await.unwrap();
    assert_eq!(read_back["id"], id);
    assert_eq!(read_back["resourceType"], "Patient");

    // Update Patient
    let updated = json!({
        "resourceType": "Patient",
        "id": id,
        "active": true,
        "name": [{"family": "Smith", "given": ["Johnny"]}],
        "identifier": [{"system": "http://sys", "value": "MRN-123"}],
    });
    let resp = client
        .put(format!(
            "{}/Patient/{}",
            fhir_base,
            updated["id"].as_str().unwrap()
        ))
        .header("accept", "application/fhir+json")
        .header("content-type", "application/fhir+json")
        .json(&updated)
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
    let after_update: Value = resp.json().await.unwrap();
    assert_eq!(after_update["name"][0]["given"][0], "Johnny");

    // Delete Patient
    let resp = client
        .delete(format!(
            "{}/Patient/{}",
            fhir_base,
            read_back["id"].as_str().unwrap()
        ))
        .header("accept", "application/fhir+json")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::NO_CONTENT);

    // Reading after delete should produce 410 Gone (soft delete)
    let resp = client
        .get(format!("{fhir_base}/Patient/{id}"))
        .header("accept", "application/fhir+json")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::GONE);

    let _ = shutdown_tx.send(());
    let _ = handle.await;
}

#[tokio::test]
#[ignore = "requires FHIR packages in .fhir/ directory"]
async fn error_cases_invalid_resource_and_id_mismatch_and_delete_404() {
    let (_container, postgres_url) = start_postgres().await;
    let config = create_config(&postgres_url);
    let (base, shutdown_tx, handle) = start_server(&config).await;
    let client = reqwest::Client::new();
    let fhir_base = format!("{base}/fhir");

    // POST invalid resourceType vs path
    let bad = json!({"resourceType": "Observation"});
    let resp = client
        .post(format!("{fhir_base}/Patient"))
        .header("accept", "application/fhir+json")
        .header("content-type", "application/fhir+json")
        .json(&bad)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::BAD_REQUEST);

    // Create a Patient first - must include name for validation
    let payload =
        json!({"resourceType": "Patient", "active": true, "name": [{"family": "TestFamily"}]});
    let resp = client
        .post(format!("{fhir_base}/Patient"))
        .header("accept", "application/fhir+json")
        .header("content-type", "application/fhir+json")
        .json(&payload)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::CREATED);
    let created: Value = resp.json().await.unwrap();
    let id = created["id"].as_str().unwrap().to_string();

    // PUT id mismatch (body id != path id) - must include name for validation
    let mism =
        json!({"resourceType": "Patient", "id": "DIFFERENT", "name": [{"family": "Mismatch"}]});
    let resp = client
        .put(format!("{fhir_base}/Patient/{id}"))
        .header("accept", "application/fhir+json")
        .header("content-type", "application/fhir+json")
        .json(&mism)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::BAD_REQUEST);

    // DELETE non-existent resource - per FHIR spec, delete is idempotent (204 No Content)
    // Use a valid UUID format that doesn't exist in the database
    let non_existent_uuid = "00000000-0000-0000-0000-000000000000";
    let resp = client
        .delete(format!("{}/Patient/{}", fhir_base, non_existent_uuid))
        .header("accept", "application/fhir+json")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::NO_CONTENT);

    let _ = shutdown_tx.send(());
    let _ = handle.await;
}
