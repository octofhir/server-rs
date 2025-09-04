use octofhir_server::{build_app, AppConfig};
use serde_json::Value;
use tokio::task::JoinHandle;

async fn start_server() -> (String, tokio::sync::oneshot::Sender<()>, JoinHandle<()>) {
    let app = build_app(&AppConfig::default());

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

    (format!("http://{}", addr), tx, server)
}

#[tokio::test]
async fn server_endpoints_work() {
    let (base, shutdown_tx, handle) = start_server().await;
    let client = reqwest::Client::new();

    // GET /
    let resp = client
        .get(format!("{}/", base))
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
        .get(format!("{}/healthz", base))
        .header("accept", "application/fhir+json")
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "ok");

    // GET /readyz
    let resp = client
        .get(format!("{}/readyz", base))
        .header("accept", "application/fhir+json")
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "ready");

    // GET /metadata
    let resp = client
        .get(format!("{}/metadata", base))
        .header("accept", "application/fhir+json")
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["resourceType"], "CapabilityStatement");

    // GET /Patient (search placeholder)
    let resp = client
        .get(format!("{}/Patient", base))
        .header("accept", "application/fhir+json")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::NOT_IMPLEMENTED);

    // shutdown
    let _ = shutdown_tx.send(());
    let _ = handle.await;
}
