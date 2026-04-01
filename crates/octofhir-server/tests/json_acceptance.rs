use std::sync::Arc;

use octofhir_config::ConfigurationManager;
use octofhir_server::{AppConfig, build_app};
use serde_json::Value;
use tokio::task::JoinHandle;

async fn start_server() -> (String, tokio::sync::oneshot::Sender<()>, JoinHandle<()>) {
    let config_manager = Arc::new(
        ConfigurationManager::builder()
            .build()
            .await
            .expect("build config manager"),
    );
    let app = build_app(&AppConfig::default(), config_manager)
        .await
        .expect("build app");

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
async fn accepts_application_json_in_accept_header() {
    let (base, shutdown_tx, handle) = start_server().await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{base}/healthz"))
        .header("accept", "application/json")
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "ok");

    let _ = shutdown_tx.send(());
    let _ = handle.await;
}

#[tokio::test]
async fn accepts_application_json_content_type_on_post() {
    let (base, shutdown_tx, handle) = start_server().await;
    let client = reqwest::Client::new();
    let fhir_base = format!("{base}/fhir");

    let payload = serde_json::json!({"resourceType":"Patient", "name": [{"family": "Test"}]});

    let resp = client
        .post(format!("{fhir_base}/Patient"))
        .header("accept", "application/json")
        .header("content-type", "application/json")
        .json(&payload)
        .send()
        .await
        .unwrap();

    // Handler should create the resource now that POST is implemented
    assert_eq!(resp.status(), reqwest::StatusCode::CREATED);
    let location = resp
        .headers()
        .get("location")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    let created: Value = resp.json().await.unwrap();
    let id = created["id"].as_str().unwrap();
    assert!(
        location.starts_with(&format!("{fhir_base}/Patient/{id}/_history/")),
        "unexpected location header: {location}"
    );

    let _ = shutdown_tx.send(());
    let _ = handle.await;
}

#[tokio::test]
async fn read_supports_elements_filter_and_marks_subsetted() {
    let (base, shutdown_tx, handle) = start_server().await;
    let client = reqwest::Client::new();
    let fhir_base = format!("{base}/fhir");

    let payload = serde_json::json!({
        "resourceType":"Patient",
        "active": true,
        "name": [{"family": "Subset", "given": ["Test"]}],
        "birthDate": "1990-01-01",
        "telecom": [{"system": "phone", "value": "+10000000000"}]
    });

    let create_resp = client
        .post(format!("{fhir_base}/Patient"))
        .header("accept", "application/json")
        .header("content-type", "application/json")
        .json(&payload)
        .send()
        .await
        .unwrap();
    assert_eq!(create_resp.status(), reqwest::StatusCode::CREATED);
    let created: Value = create_resp.json().await.unwrap();
    let id = created["id"].as_str().unwrap();

    let resp = client
        .get(format!("{fhir_base}/Patient/{id}?_elements=name,birthDate"))
        .header("accept", "application/fhir+json")
        .send()
        .await
        .unwrap();

    assert!(resp.status().is_success());
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["resourceType"], "Patient");
    assert_eq!(body["id"], id);
    assert!(body.get("name").is_some());
    assert!(body.get("birthDate").is_some());
    assert!(body.get("telecom").is_none());

    let tags = body["meta"]["tag"].as_array().cloned().unwrap_or_default();
    assert!(
        tags.iter().any(|tag| {
            tag["system"] == "http://terminology.hl7.org/CodeSystem/v3-ObservationValue"
                && tag["code"] == "SUBSETTED"
        }),
        "expected SUBSETTED tag in meta"
    );

    let _ = shutdown_tx.send(());
    let _ = handle.await;
}

#[tokio::test]
async fn prefer_return_operation_outcome_is_case_insensitive() {
    let (base, shutdown_tx, handle) = start_server().await;
    let client = reqwest::Client::new();
    let fhir_base = format!("{base}/fhir");

    let payload = serde_json::json!({
        "resourceType":"Patient",
        "name": [{"family": "Prefer"}]
    });

    let resp = client
        .post(format!("{fhir_base}/Patient"))
        .header("accept", "application/json")
        .header("content-type", "application/json")
        .header("prefer", "respond-async, RETURN=OperationOutcome")
        .json(&payload)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), reqwest::StatusCode::CREATED);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["resourceType"], "OperationOutcome");

    let _ = shutdown_tx.send(());
    let _ = handle.await;
}
