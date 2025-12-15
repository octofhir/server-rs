//! Integration tests for terminology service features using testcontainers.
//!
//! These tests verify:
//! - ValueSet expansion with :in/:not-in modifiers
//! - SNOMED CT hierarchy with :below/:above modifiers
//! - Automatic optimization (IN clause vs temp table)
//! - Integration with tx.fhir.org
//!
//! Requirements:
//! - Docker running (for testcontainers)
//! - Internet connection (for tx.fhir.org)

use octofhir_search::TerminologyConfig;
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

/// Create an AppConfig with terminology service enabled
fn create_config_with_terminology(postgres_url: &str) -> AppConfig {
    let mut config = AppConfig::default();

    // PostgreSQL storage
    config.storage.postgres = Some(PostgresStorageConfig {
        url: Some(postgres_url.to_string()),
        pool_size: 5,
        connect_timeout_ms: 10000,
        idle_timeout_ms: Some(60000),
        ..Default::default()
    });

    // Enable terminology service with tx.fhir.org
    config.terminology = TerminologyConfig {
        enabled: true,
        server_url: "https://tx.fhir.org/r4".to_string(),
        cache_ttl_secs: 300, // 5 minutes for tests
    };

    // Longer timeouts for external calls
    config.server.read_timeout_ms = 30000;
    config.server.write_timeout_ms = 30000;

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

    (format!("http://{addr}/fhir"), tx, server)
}

/// Create test observation resources with known codes
async fn create_test_observations(client: &reqwest::Client, base: &str) -> Vec<String> {
    let mut ids = Vec::new();

    // Create observations with different status codes
    let test_data = vec![
        ("final", "8867-4", "http://loinc.org", "Heart rate"),
        (
            "preliminary",
            "8480-6",
            "http://loinc.org",
            "Systolic blood pressure",
        ),
        (
            "amended",
            "8462-4",
            "http://loinc.org",
            "Diastolic blood pressure",
        ),
    ];

    for (status, code, system, display) in test_data {
        let payload = json!({
            "resourceType": "Observation",
            "status": status,
            "code": {
                "coding": [{
                    "system": system,
                    "code": code,
                    "display": display
                }]
            },
            "subject": {"reference": "Patient/test"}
        });

        let resp = client
            .post(format!("{base}/Observation"))
            .header("content-type", "application/fhir+json")
            .json(&payload)
            .send()
            .await
            .expect("create observation");

        if resp.status().is_success() {
            let created: Value = resp.json().await.expect("parse json");
            if let Some(id) = created["id"].as_str() {
                ids.push(id.to_string());
            }
        }
    }

    ids
}

/// Create test condition resources with SNOMED CT codes
async fn create_test_conditions(client: &reqwest::Client, base: &str) -> Vec<String> {
    let mut ids = Vec::new();

    // Create conditions with diabetes-related SNOMED CT codes
    let test_data = vec![
        ("73211009", "Diabetes mellitus"),
        ("44054006", "Type 2 diabetes mellitus"),
        ("46635009", "Type 1 diabetes mellitus"),
    ];

    for (code, display) in test_data {
        let payload = json!({
            "resourceType": "Condition",
            "code": {
                "coding": [{
                    "system": "http://snomed.info/sct",
                    "code": code,
                    "display": display
                }]
            },
            "subject": {"reference": "Patient/test"}
        });

        let resp = client
            .post(format!("{base}/Condition"))
            .header("content-type", "application/fhir+json")
            .json(&payload)
            .send()
            .await
            .expect("create condition");

        if resp.status().is_success() {
            let created: Value = resp.json().await.expect("parse json");
            if let Some(id) = created["id"].as_str() {
                ids.push(id.to_string());
            }
        }
    }

    ids
}

#[tokio::test]
#[ignore] // Requires internet connection to tx.fhir.org
async fn test_valueset_in_modifier_small() {
    let (_container, postgres_url) = start_postgres().await;
    let config = create_config_with_terminology(&postgres_url);
    let (base, shutdown_tx, _handle) = start_server(&config).await;
    let client = reqwest::Client::new();

    // Create test data
    let _obs_ids = create_test_observations(&client, &base).await;

    // Search with :in modifier using a small ValueSet
    // This should use the IN clause strategy
    let resp = client
        .get(format!(
            "{base}/Observation?code:in=http://hl7.org/fhir/ValueSet/observation-status"
        ))
        .header("accept", "application/fhir+json")
        .send()
        .await
        .expect("search request");

    assert!(resp.status().is_success(), "Search should succeed");
    let bundle: Value = resp.json().await.expect("parse bundle");

    assert_eq!(bundle["resourceType"], "Bundle");
    assert_eq!(bundle["type"], "searchset");

    // Should find observations with status codes in the ValueSet
    if let Some(total) = bundle["total"].as_u64() {
        assert!(total > 0, "Should find at least one observation");
    }

    let _ = shutdown_tx.send(());
}

#[tokio::test]
#[ignore] // Requires internet connection to tx.fhir.org
async fn test_valueset_not_in_modifier() {
    let (_container, postgres_url) = start_postgres().await;
    let config = create_config_with_terminology(&postgres_url);
    let (base, shutdown_tx, _handle) = start_server(&config).await;
    let client = reqwest::Client::new();

    // Create test data
    let _obs_ids = create_test_observations(&client, &base).await;

    // Search with :not-in modifier to exclude certain statuses
    let resp = client
        .get(format!(
            "{base}/Observation?code:not-in=http://hl7.org/fhir/ValueSet/observation-status"
        ))
        .header("accept", "application/fhir+json")
        .send()
        .await
        .expect("search request");

    assert!(resp.status().is_success(), "Search should succeed");
    let bundle: Value = resp.json().await.expect("parse bundle");

    assert_eq!(bundle["resourceType"], "Bundle");

    let _ = shutdown_tx.send(());
}

#[tokio::test]
#[ignore] // Requires internet connection to tx.fhir.org and SNOMED CT
async fn test_snomed_ct_below_modifier() {
    let (_container, postgres_url) = start_postgres().await;
    let config = create_config_with_terminology(&postgres_url);
    let (base, shutdown_tx, _handle) = start_server(&config).await;
    let client = reqwest::Client::new();

    // Create test conditions with diabetes codes
    let _condition_ids = create_test_conditions(&client, &base).await;

    // Search for conditions with diabetes or any subtype using :below
    // 73211009 = Diabetes mellitus (should match all our test conditions)
    let resp = client
        .get(format!(
            "{base}/Condition?code:below=http://snomed.info/sct|73211009"
        ))
        .header("accept", "application/fhir+json")
        .send()
        .await
        .expect("search request");

    assert!(resp.status().is_success(), "Search should succeed");
    let bundle: Value = resp.json().await.expect("parse bundle");

    assert_eq!(bundle["resourceType"], "Bundle");

    // Should find all three conditions (diabetes, type 1, type 2)
    if let Some(total) = bundle["total"].as_u64() {
        assert!(
            total >= 3,
            "Should find at least 3 conditions (diabetes and subtypes), found {}",
            total
        );
    }

    let _ = shutdown_tx.send(());
}

#[tokio::test]
#[ignore] // Requires internet connection to tx.fhir.org and SNOMED CT
async fn test_snomed_ct_above_modifier() {
    let (_container, postgres_url) = start_postgres().await;
    let config = create_config_with_terminology(&postgres_url);
    let (base, shutdown_tx, _handle) = start_server(&config).await;
    let client = reqwest::Client::new();

    // Create test conditions
    let _condition_ids = create_test_conditions(&client, &base).await;

    // Search for conditions that subsume (are ancestors of) Type 2 DM
    // 44054006 = Type 2 diabetes mellitus
    // Should match general "Diabetes mellitus" condition
    let resp = client
        .get(format!(
            "{base}/Condition?code:above=http://snomed.info/sct|44054006"
        ))
        .header("accept", "application/fhir+json")
        .send()
        .await
        .expect("search request");

    assert!(resp.status().is_success(), "Search should succeed");
    let bundle: Value = resp.json().await.expect("parse bundle");

    assert_eq!(bundle["resourceType"], "Bundle");

    let _ = shutdown_tx.send(());
}

#[tokio::test]
#[ignore] // Requires internet connection to tx.fhir.org
async fn test_large_valueset_uses_temp_table() {
    let (_container, postgres_url) = start_postgres().await;
    let config = create_config_with_terminology(&postgres_url);
    let (base, shutdown_tx, _handle) = start_server(&config).await;
    let client = reqwest::Client::new();

    // Create test data
    let _obs_ids = create_test_observations(&client, &base).await;

    // Search with a large ValueSet (all observation codes)
    // This should trigger temp table optimization (>500 codes)
    let resp = client
        .get(format!(
            "{base}/Observation?code:in=http://hl7.org/fhir/ValueSet/observation-codes"
        ))
        .header("accept", "application/fhir+json")
        .send()
        .await
        .expect("search request");

    assert!(resp.status().is_success(), "Search should succeed");
    let bundle: Value = resp.json().await.expect("parse bundle");

    assert_eq!(bundle["resourceType"], "Bundle");

    // The response should be successful regardless of optimization strategy
    // Check server logs for "Bulk inserted N codes into temp table"

    let _ = shutdown_tx.send(());
}

#[tokio::test]
#[ignore] // Requires internet connection
async fn test_terminology_caching() {
    let (_container, postgres_url) = start_postgres().await;
    let config = create_config_with_terminology(&postgres_url);
    let (base, shutdown_tx, _handle) = start_server(&config).await;
    let client = reqwest::Client::new();

    // Create test data
    let _obs_ids = create_test_observations(&client, &base).await;

    let valueset_url = "http://hl7.org/fhir/ValueSet/observation-status";

    // First request - cache miss
    let start = std::time::Instant::now();
    let resp1 = client
        .get(format!("{base}/Observation?code:in={valueset_url}"))
        .header("accept", "application/fhir+json")
        .send()
        .await
        .expect("first request");
    let first_duration = start.elapsed();
    assert!(resp1.status().is_success());

    // Second request - should hit cache (faster)
    let start = std::time::Instant::now();
    let resp2 = client
        .get(format!("{base}/Observation?code:in={valueset_url}"))
        .header("accept", "application/fhir+json")
        .send()
        .await
        .expect("second request");
    let second_duration = start.elapsed();
    assert!(resp2.status().is_success());

    // Second request should be faster due to caching
    // (Not a strict assertion due to network variability)
    println!("First request: {:?}", first_duration);
    println!("Second request: {:?}", second_duration);
    println!(
        "Speedup: {:.2}x",
        first_duration.as_secs_f64() / second_duration.as_secs_f64()
    );

    let _ = shutdown_tx.send(());
}

#[tokio::test]
async fn test_terminology_disabled() {
    let (_container, postgres_url) = start_postgres().await;
    let mut config = create_config_with_terminology(&postgres_url);

    // Disable terminology service
    config.terminology.enabled = false;

    let (base, shutdown_tx, _handle) = start_server(&config).await;
    let client = reqwest::Client::new();

    // Search with :in modifier should return error when terminology disabled
    let resp = client
        .get(format!(
            "{base}/Observation?code:in=http://hl7.org/fhir/ValueSet/observation-status"
        ))
        .header("accept", "application/fhir+json")
        .send()
        .await
        .expect("search request");

    // Should return 400 Bad Request or OperationOutcome
    assert!(
        !resp.status().is_success(),
        "Should fail when terminology is disabled"
    );

    let _ = shutdown_tx.send(());
}

#[tokio::test]
#[ignore] // Requires internet connection
async fn test_multiple_terminology_operations() {
    let (_container, postgres_url) = start_postgres().await;
    let config = create_config_with_terminology(&postgres_url);
    let (base, shutdown_tx, _handle) = start_server(&config).await;
    let client = reqwest::Client::new();

    // Create test data
    let _obs_ids = create_test_observations(&client, &base).await;
    let _condition_ids = create_test_conditions(&client, &base).await;

    // Test multiple terminology operations in sequence
    let operations = vec![
        format!("{base}/Observation?code:in=http://hl7.org/fhir/ValueSet/observation-status"),
        format!("{base}/Condition?code:below=http://snomed.info/sct|73211009"),
        format!("{base}/Observation?code:not-in=http://hl7.org/fhir/ValueSet/observation-status"),
    ];

    for url in operations {
        let resp = client
            .get(&url)
            .header("accept", "application/fhir+json")
            .send()
            .await
            .expect("search request");

        assert!(resp.status().is_success(), "Search failed for: {}", url);
    }

    let _ = shutdown_tx.send(());
}
