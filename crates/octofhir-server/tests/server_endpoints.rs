use octofhir_server::{AppConfig, build_app};
use serde_json::Value;
use tokio::task::JoinHandle;

async fn start_server() -> (String, tokio::sync::oneshot::Sender<()>, JoinHandle<()>) {
    let app = build_app(&AppConfig::default()).await.expect("build app");

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
async fn server_endpoints_work() {
    let (base, shutdown_tx, handle) = start_server().await;
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
