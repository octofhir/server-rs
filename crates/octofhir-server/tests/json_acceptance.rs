use octofhir_server::{build_app, AppConfig};
use serde_json::Value;
use tokio::task::JoinHandle;

async fn start_server() -> (String, tokio::sync::oneshot::Sender<()>, JoinHandle<()>) {
    let app = build_app(&AppConfig::default());

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
async fn accepts_application_json_in_accept_header() {
    let (base, shutdown_tx, handle) = start_server().await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{}/healthz", base))
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

    let payload = serde_json::json!({"resourceType":"Patient"});

    let resp = client
        .post(format!("{}/Patient", base))
        .header("accept", "application/json")
        .header("content-type", "application/json")
        .json(&payload)
        .send()
        .await
        .unwrap();

    // Handler returns 501 Not Implemented, which means Content-Type was accepted by middleware
    assert_eq!(resp.status(), reqwest::StatusCode::NOT_IMPLEMENTED);

    let _ = shutdown_tx.send(());
    let _ = handle.await;
}
