use axum::http::HeaderValue;
use serde_json::Value;
use tokio::task::JoinHandle;

use octofhir_server::{AppConfig, build_app, canonical};

async fn start_server_with_packages() -> (String, tokio::sync::oneshot::Sender<()>, JoinHandle<()>)
{
    let cfg = AppConfig {
        packages: octofhir_server::config::PackagesConfig {
            load: vec![
                octofhir_server::config::PackageSpec::Simple("hl7.fhir.r4b.core#4.3.0".into()),
                octofhir_server::config::PackageSpec::Simple("hl7.terminology#5.5.0".into()),
            ],
            path: None,
        },
        ..AppConfig::default()
    };

    // Initialize and set registry explicitly for the test
    let reg = canonical::init_from_config_async(&cfg)
        .await
        .expect("canonical init");
    canonical::set_registry(reg);

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
async fn metadata_includes_loaded_packages_extension() {
    if std::env::var("OCTOFHIR_TEST_CANONICAL_ONLINE")
        .ok()
        .as_deref()
        != Some("1")
    {
        eprintln!(
            "skipping canonical manager online test (set OCTOFHIR_TEST_CANONICAL_ONLINE=1 to run)"
        );
        return;
    }
    let (base, shutdown_tx, handle) = start_server_with_packages().await;
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
    // Expect an extension array with at least two entries for loaded packages
    let ext = body
        .get("extension")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    assert!(
        ext.iter()
            .any(|e| e["url"] == "urn:octofhir:loaded-package")
    );

    let _ = shutdown_tx.send(());
    let _ = handle.await;
}
