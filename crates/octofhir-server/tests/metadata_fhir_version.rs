use axum::http::HeaderValue;
use serde_json::Value;
use tokio::task::JoinHandle;

use octofhir_config::ConfigurationManager;
use octofhir_server::{AppConfig, PostgresStorageConfig, build_app, config};
use testcontainers::{ContainerAsync, ImageExt, runners::AsyncRunner};
use testcontainers_modules::postgres::Postgres;
use tokio::sync::{Mutex, MutexGuard};

// Shared config and the canonical manager are process-global; one server at a time.
static SERIAL: Mutex<()> = Mutex::const_new(());

/// Keeps the database and the serial lock alive for the duration of a test.
struct Fixture {
    _serial: MutexGuard<'static, ()>,
    _postgres: ContainerAsync<Postgres>,
}

async fn start_server_with_cfg(
    mut cfg: AppConfig,
) -> (
    String,
    tokio::sync::oneshot::Sender<()>,
    JoinHandle<()>,
    Fixture,
) {
    let serial = SERIAL.lock().await;
    let postgres = Postgres::default()
        .with_tag("17-alpine")
        .start()
        .await
        .expect("start postgres container");
    let port = postgres.get_host_port_ipv4(5432).await.expect("get port");
    cfg.storage.postgres = Some(PostgresStorageConfig {
        url: Some(format!(
            "postgres://postgres:postgres@127.0.0.1:{port}/postgres"
        )),
        pool_size: 5,
        connect_timeout_ms: 10000,
        ..Default::default()
    });
    cfg.auth.policy.anonymous_access = true;
    if octofhir_server::canonical::get_manager().is_some() {
        octofhir_server::canonical::rebuild_from_config_async(&cfg)
            .await
            .expect("rebuild canonical registry for fixture database");
    }

    // Set shared config so handlers can read fhir.version
    let shared = std::sync::Arc::new(std::sync::RwLock::new(cfg.clone()));
    config::shared::set_shared(shared);

    let config_manager = std::sync::Arc::new(
        ConfigurationManager::builder()
            .build()
            .await
            .expect("build config manager"),
    );
    let app = build_app(&cfg, config_manager).await.expect("build app");
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
    let fixture = Fixture {
        _serial: serial,
        _postgres: postgres,
    };
    (format!("http://{addr}"), tx, server, fixture)
}

#[tokio::test]
async fn metadata_reports_configured_fhir_version_r4() {
    let mut cfg = AppConfig::default();
    cfg.fhir.version = "R4".to_string();
    let (base, shutdown_tx, handle, _fixture) = start_server_with_cfg(cfg).await;
    let fhir_base = format!("{base}/fhir");

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{fhir_base}/metadata"))
        .header("accept", HeaderValue::from_static("application/fhir+json"))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["resourceType"], "CapabilityStatement");
    assert_eq!(body["fhirVersion"], "4.0.1");
    let interactions = body["rest"][0]["interaction"].as_array().unwrap();
    for code in ["transaction", "batch", "search-system", "history-system"] {
        assert!(
            interactions
                .iter()
                .any(|interaction| interaction["code"] == code),
            "missing rest interaction {code}"
        );
    }

    let _ = shutdown_tx.send(());
    let _ = handle.await;
}

#[tokio::test]
async fn metadata_reports_configured_fhir_version_r5() {
    let mut cfg = AppConfig::default();
    cfg.fhir.version = "R5".to_string();
    let (base, shutdown_tx, handle, _fixture) = start_server_with_cfg(cfg).await;
    let fhir_base = format!("{base}/fhir");

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{fhir_base}/metadata"))
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
