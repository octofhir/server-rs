use axum::http::HeaderValue;
use serde_json::Value;
use tokio::task::JoinHandle;

use octofhir_server::{AppConfig, build_app, config};

async fn start_server_with_cfg(
    cfg: AppConfig,
) -> (String, tokio::sync::oneshot::Sender<()>, JoinHandle<()>) {
    // Set shared config so handlers can read fhir.version
    let shared = std::sync::Arc::new(std::sync::RwLock::new(cfg.clone()));
    config::shared::set_shared(shared);

    let app = build_app(&cfg).await.expect("build app");
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
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
async fn metadata_reports_configured_fhir_version_r4() {
    let mut cfg = AppConfig::default();
    cfg.fhir.version = "R4".to_string();
    let (base, shutdown_tx, handle) = start_server_with_cfg(cfg).await;

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{base}/metadata"))
        .header("accept", HeaderValue::from_static("application/fhir+json"))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["resourceType"], "CapabilityStatement");
    assert_eq!(body["fhirVersion"], "4.0.1");

    let _ = shutdown_tx.send(());
    let _ = handle.await;
}

#[tokio::test]
async fn metadata_reports_configured_fhir_version_r5() {
    let mut cfg = AppConfig::default();
    cfg.fhir.version = "R5".to_string();
    let (base, shutdown_tx, handle) = start_server_with_cfg(cfg).await;

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{base}/metadata"))
        .header("accept", HeaderValue::from_static("application/fhir+json"))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["fhirVersion"], "5.0.0");

    let _ = shutdown_tx.send(());
    let _ = handle.await;
}
