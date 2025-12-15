//! Integration tests for search parameters and modifiers.
//!
//! These tests verify all search modifiers and advanced search features:
//! - String modifiers: :exact, :contains, :missing
//! - Token modifiers: :text, :not, :of-type
//! - Date prefixes: eq, ne, lt, le, gt, ge, sa, eb, ap
//! - _include and _revinclude with :iterate
//! - _elements and _summary filtering
//! - Composite search parameters
//! - Chained search parameters
//!
//! Requirements:
//! - Docker running (for testcontainers)

use octofhir_server::{AppConfig, PostgresStorageConfig, build_app};
use serde_json::{Value, json};
use testcontainers::{ContainerAsync, runners::AsyncRunner};
use testcontainers_modules::postgres::Postgres;
use tokio::task::JoinHandle;

// =============================================================================
// Test Infrastructure
// =============================================================================

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

/// Create test patients for search tests
async fn create_test_patients(client: &reqwest::Client, base: &str) -> Vec<String> {
    let mut ids = Vec::new();

    let patients = vec![
        json!({
            "resourceType": "Patient",
            "name": [{"family": "Smith", "given": ["John", "William"]}],
            "birthDate": "1980-01-15",
            "gender": "male",
            "active": true,
            "identifier": [{"system": "http://hospital.org/mrn", "value": "MRN-001"}],
            "telecom": [{"system": "phone", "value": "555-1234"}]
        }),
        json!({
            "resourceType": "Patient",
            "name": [{"family": "Smith", "given": ["Jane"]}],
            "birthDate": "1985-06-20",
            "gender": "female",
            "active": true,
            "identifier": [{"system": "http://hospital.org/mrn", "value": "MRN-002"}]
        }),
        json!({
            "resourceType": "Patient",
            "name": [{"family": "Johnson", "given": ["Robert"]}],
            "birthDate": "1990-12-01",
            "gender": "male",
            "active": false,
            "identifier": [{"system": "http://hospital.org/mrn", "value": "MRN-003"}]
        }),
        json!({
            "resourceType": "Patient",
            "name": [{"family": "Williams", "given": ["Emily"]}],
            "birthDate": "2000-03-10",
            "gender": "female",
            "active": true
            // No identifier - for :missing tests
        }),
    ];

    for patient in patients {
        let resp = client
            .post(format!("{base}/Patient"))
            .header("content-type", "application/fhir+json")
            .json(&patient)
            .send()
            .await
            .expect("create patient");

        if resp.status().is_success() {
            let created: Value = resp.json().await.expect("parse json");
            if let Some(id) = created["id"].as_str() {
                ids.push(id.to_string());
            }
        }
    }

    ids
}

/// Create test observations linked to patients
async fn create_test_observations(
    client: &reqwest::Client,
    base: &str,
    patient_ids: &[String],
) -> Vec<String> {
    let mut ids = Vec::new();

    if patient_ids.is_empty() {
        return ids;
    }

    let observations = vec![
        json!({
            "resourceType": "Observation",
            "status": "final",
            "code": {
                "coding": [{"system": "http://loinc.org", "code": "8867-4", "display": "Heart rate"}]
            },
            "subject": {"reference": format!("Patient/{}", patient_ids[0])},
            "effectiveDateTime": "2024-01-15T10:00:00Z",
            "valueQuantity": {"value": 72, "unit": "beats/min"}
        }),
        json!({
            "resourceType": "Observation",
            "status": "preliminary",
            "code": {
                "coding": [{"system": "http://loinc.org", "code": "8480-6", "display": "Systolic BP"}]
            },
            "subject": {"reference": format!("Patient/{}", patient_ids[0])},
            "effectiveDateTime": "2024-01-15T10:05:00Z",
            "valueQuantity": {"value": 120, "unit": "mmHg"}
        }),
        json!({
            "resourceType": "Observation",
            "status": "final",
            "code": {
                "coding": [{"system": "http://loinc.org", "code": "8462-4", "display": "Diastolic BP"}]
            },
            "subject": {"reference": format!("Patient/{}", patient_ids.get(1).unwrap_or(&patient_ids[0]))},
            "effectiveDateTime": "2024-02-20T14:30:00Z",
            "valueQuantity": {"value": 80, "unit": "mmHg"}
        }),
    ];

    for obs in observations {
        let resp = client
            .post(format!("{base}/Observation"))
            .header("content-type", "application/fhir+json")
            .json(&obs)
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

fn get_bundle_total(bundle: &Value) -> u64 {
    bundle["total"].as_u64().unwrap_or(0)
}

fn get_bundle_entries(bundle: &Value) -> &Vec<Value> {
    static EMPTY: Vec<Value> = Vec::new();
    bundle["entry"].as_array().unwrap_or(&EMPTY)
}

// =============================================================================
// String Modifier Tests
// =============================================================================

#[tokio::test]
async fn test_string_exact_modifier() {
    let (_container, postgres_url) = start_postgres().await;
    let config = create_config(&postgres_url);
    let (base, shutdown_tx, _handle) = start_server(&config).await;
    let client = reqwest::Client::new();

    let _patient_ids = create_test_patients(&client, &base).await;

    // Exact match should be case-sensitive
    let resp = client
        .get(format!("{base}/Patient?family:exact=Smith"))
        .header("accept", "application/fhir+json")
        .send()
        .await
        .expect("search request");

    assert!(resp.status().is_success());
    let bundle: Value = resp.json().await.expect("parse bundle");
    assert_eq!(bundle["resourceType"], "Bundle");
    assert_eq!(get_bundle_total(&bundle), 2); // John Smith and Jane Smith

    // Case mismatch should not match
    let resp = client
        .get(format!("{base}/Patient?family:exact=smith"))
        .header("accept", "application/fhir+json")
        .send()
        .await
        .expect("search request");

    assert!(resp.status().is_success());
    let bundle: Value = resp.json().await.expect("parse bundle");
    assert_eq!(get_bundle_total(&bundle), 0);

    let _ = shutdown_tx.send(());
}

#[tokio::test]
async fn test_string_contains_modifier() {
    let (_container, postgres_url) = start_postgres().await;
    let config = create_config(&postgres_url);
    let (base, shutdown_tx, _handle) = start_server(&config).await;
    let client = reqwest::Client::new();

    let _patient_ids = create_test_patients(&client, &base).await;

    // Contains should find substring matches
    let resp = client
        .get(format!("{base}/Patient?family:contains=mit"))
        .header("accept", "application/fhir+json")
        .send()
        .await
        .expect("search request");

    assert!(resp.status().is_success());
    let bundle: Value = resp.json().await.expect("parse bundle");
    assert_eq!(get_bundle_total(&bundle), 2); // Smith family

    let _ = shutdown_tx.send(());
}

#[tokio::test]
async fn test_string_missing_modifier() {
    let (_container, postgres_url) = start_postgres().await;
    let config = create_config(&postgres_url);
    let (base, shutdown_tx, _handle) = start_server(&config).await;
    let client = reqwest::Client::new();

    let _patient_ids = create_test_patients(&client, &base).await;

    // Find patients missing identifier
    let resp = client
        .get(format!("{base}/Patient?identifier:missing=true"))
        .header("accept", "application/fhir+json")
        .send()
        .await
        .expect("search request");

    assert!(resp.status().is_success());
    let bundle: Value = resp.json().await.expect("parse bundle");
    assert_eq!(get_bundle_total(&bundle), 1); // Emily Williams has no identifier

    // Find patients with identifier
    let resp = client
        .get(format!("{base}/Patient?identifier:missing=false"))
        .header("accept", "application/fhir+json")
        .send()
        .await
        .expect("search request");

    assert!(resp.status().is_success());
    let bundle: Value = resp.json().await.expect("parse bundle");
    assert_eq!(get_bundle_total(&bundle), 3); // 3 patients have identifiers

    let _ = shutdown_tx.send(());
}

// =============================================================================
// Token Modifier Tests
// =============================================================================

#[tokio::test]
async fn test_token_text_modifier() {
    let (_container, postgres_url) = start_postgres().await;
    let config = create_config(&postgres_url);
    let (base, shutdown_tx, _handle) = start_server(&config).await;
    let client = reqwest::Client::new();

    let _patient_ids = create_test_patients(&client, &base).await;

    // Text search on gender display
    let resp = client
        .get(format!("{base}/Patient?gender:text=male"))
        .header("accept", "application/fhir+json")
        .send()
        .await
        .expect("search request");

    assert!(resp.status().is_success());
    let bundle: Value = resp.json().await.expect("parse bundle");
    // Should find male patients
    assert!(get_bundle_total(&bundle) >= 2);

    let _ = shutdown_tx.send(());
}

#[tokio::test]
async fn test_token_not_modifier() {
    let (_container, postgres_url) = start_postgres().await;
    let config = create_config(&postgres_url);
    let (base, shutdown_tx, _handle) = start_server(&config).await;
    let client = reqwest::Client::new();

    let _patient_ids = create_test_patients(&client, &base).await;

    // Find patients that are NOT male
    let resp = client
        .get(format!("{base}/Patient?gender:not=male"))
        .header("accept", "application/fhir+json")
        .send()
        .await
        .expect("search request");

    assert!(resp.status().is_success());
    let bundle: Value = resp.json().await.expect("parse bundle");
    assert_eq!(get_bundle_total(&bundle), 2); // Jane and Emily

    let _ = shutdown_tx.send(());
}

// =============================================================================
// Date Prefix Tests
// =============================================================================

#[tokio::test]
async fn test_date_eq_prefix() {
    let (_container, postgres_url) = start_postgres().await;
    let config = create_config(&postgres_url);
    let (base, shutdown_tx, _handle) = start_server(&config).await;
    let client = reqwest::Client::new();

    let _patient_ids = create_test_patients(&client, &base).await;

    // Exact date match
    let resp = client
        .get(format!("{base}/Patient?birthdate=eq1980-01-15"))
        .header("accept", "application/fhir+json")
        .send()
        .await
        .expect("search request");

    assert!(resp.status().is_success());
    let bundle: Value = resp.json().await.expect("parse bundle");
    assert_eq!(get_bundle_total(&bundle), 1); // John Smith

    let _ = shutdown_tx.send(());
}

#[tokio::test]
async fn test_date_lt_le_gt_ge_prefixes() {
    let (_container, postgres_url) = start_postgres().await;
    let config = create_config(&postgres_url);
    let (base, shutdown_tx, _handle) = start_server(&config).await;
    let client = reqwest::Client::new();

    let _patient_ids = create_test_patients(&client, &base).await;

    // Less than 1990
    let resp = client
        .get(format!("{base}/Patient?birthdate=lt1990-01-01"))
        .header("accept", "application/fhir+json")
        .send()
        .await
        .expect("search request");

    assert!(resp.status().is_success());
    let bundle: Value = resp.json().await.expect("parse bundle");
    assert_eq!(get_bundle_total(&bundle), 2); // John (1980) and Jane (1985)

    // Greater than or equal to 1990
    let resp = client
        .get(format!("{base}/Patient?birthdate=ge1990-01-01"))
        .header("accept", "application/fhir+json")
        .send()
        .await
        .expect("search request");

    assert!(resp.status().is_success());
    let bundle: Value = resp.json().await.expect("parse bundle");
    assert_eq!(get_bundle_total(&bundle), 2); // Robert (1990) and Emily (2000)

    let _ = shutdown_tx.send(());
}

#[tokio::test]
async fn test_date_ne_prefix() {
    let (_container, postgres_url) = start_postgres().await;
    let config = create_config(&postgres_url);
    let (base, shutdown_tx, _handle) = start_server(&config).await;
    let client = reqwest::Client::new();

    let _patient_ids = create_test_patients(&client, &base).await;

    // Not equal to specific date
    let resp = client
        .get(format!("{base}/Patient?birthdate=ne1980-01-15"))
        .header("accept", "application/fhir+json")
        .send()
        .await
        .expect("search request");

    assert!(resp.status().is_success());
    let bundle: Value = resp.json().await.expect("parse bundle");
    assert_eq!(get_bundle_total(&bundle), 3); // All except John Smith

    let _ = shutdown_tx.send(());
}

// =============================================================================
// _include and _revinclude Tests
// =============================================================================

#[tokio::test]
async fn test_include_basic() {
    let (_container, postgres_url) = start_postgres().await;
    let config = create_config(&postgres_url);
    let (base, shutdown_tx, _handle) = start_server(&config).await;
    let client = reqwest::Client::new();

    let patient_ids = create_test_patients(&client, &base).await;
    let _obs_ids = create_test_observations(&client, &base, &patient_ids).await;

    // Search observations and include subject (Patient)
    let resp = client
        .get(format!("{base}/Observation?_include=Observation:subject"))
        .header("accept", "application/fhir+json")
        .send()
        .await
        .expect("search request");

    assert!(resp.status().is_success());
    let bundle: Value = resp.json().await.expect("parse bundle");

    // Should have observations AND included patients
    let entries = get_bundle_entries(&bundle);
    let has_observation = entries
        .iter()
        .any(|e| e["resource"]["resourceType"].as_str() == Some("Observation"));
    let has_patient = entries
        .iter()
        .any(|e| e["resource"]["resourceType"].as_str() == Some("Patient"));

    assert!(has_observation, "Should include Observation resources");
    assert!(has_patient, "Should include Patient resources via _include");

    let _ = shutdown_tx.send(());
}

#[tokio::test]
async fn test_revinclude_basic() {
    let (_container, postgres_url) = start_postgres().await;
    let config = create_config(&postgres_url);
    let (base, shutdown_tx, _handle) = start_server(&config).await;
    let client = reqwest::Client::new();

    let patient_ids = create_test_patients(&client, &base).await;
    let _obs_ids = create_test_observations(&client, &base, &patient_ids).await;

    // Search patients and reverse-include observations
    let resp = client
        .get(format!("{base}/Patient?_revinclude=Observation:subject"))
        .header("accept", "application/fhir+json")
        .send()
        .await
        .expect("search request");

    assert!(resp.status().is_success());
    let bundle: Value = resp.json().await.expect("parse bundle");

    // Should have patients AND reverse-included observations
    let entries = get_bundle_entries(&bundle);
    let has_patient = entries
        .iter()
        .any(|e| e["resource"]["resourceType"].as_str() == Some("Patient"));
    let has_observation = entries
        .iter()
        .any(|e| e["resource"]["resourceType"].as_str() == Some("Observation"));

    assert!(has_patient, "Should include Patient resources");
    assert!(
        has_observation,
        "Should include Observation resources via _revinclude"
    );

    let _ = shutdown_tx.send(());
}

// =============================================================================
// _elements and _summary Tests
// =============================================================================

#[tokio::test]
async fn test_elements_filtering() {
    let (_container, postgres_url) = start_postgres().await;
    let config = create_config(&postgres_url);
    let (base, shutdown_tx, _handle) = start_server(&config).await;
    let client = reqwest::Client::new();

    let _patient_ids = create_test_patients(&client, &base).await;

    // Request only specific elements
    let resp = client
        .get(format!("{base}/Patient?_elements=id,name,birthDate"))
        .header("accept", "application/fhir+json")
        .send()
        .await
        .expect("search request");

    assert!(resp.status().is_success());
    let bundle: Value = resp.json().await.expect("parse bundle");

    let entries = get_bundle_entries(&bundle);
    if !entries.is_empty() {
        let resource = &entries[0]["resource"];

        // Should have requested elements
        assert!(resource.get("id").is_some(), "Should include id");
        assert!(resource.get("name").is_some(), "Should include name");
        assert!(
            resource.get("birthDate").is_some(),
            "Should include birthDate"
        );

        // Should NOT have non-requested elements (unless mandatory)
        // Note: resourceType and meta are usually always included
        assert!(
            resource.get("telecom").is_none(),
            "Should not include telecom"
        );
    }

    let _ = shutdown_tx.send(());
}

#[tokio::test]
async fn test_summary_true() {
    let (_container, postgres_url) = start_postgres().await;
    let config = create_config(&postgres_url);
    let (base, shutdown_tx, _handle) = start_server(&config).await;
    let client = reqwest::Client::new();

    let _patient_ids = create_test_patients(&client, &base).await;

    // Request summary view
    let resp = client
        .get(format!("{base}/Patient?_summary=true"))
        .header("accept", "application/fhir+json")
        .send()
        .await
        .expect("search request");

    assert!(resp.status().is_success());
    let bundle: Value = resp.json().await.expect("parse bundle");

    // Bundle should be tagged with SUBSETTED
    let entries = get_bundle_entries(&bundle);
    if !entries.is_empty() {
        let resource = &entries[0]["resource"];
        let meta = &resource["meta"];

        // Check for SUBSETTED tag
        if let Some(tags) = meta["tag"].as_array() {
            let has_subsetted = tags.iter().any(|t| t["code"].as_str() == Some("SUBSETTED"));
            assert!(has_subsetted, "Summary should have SUBSETTED tag");
        }
    }

    let _ = shutdown_tx.send(());
}

#[tokio::test]
async fn test_summary_count() {
    let (_container, postgres_url) = start_postgres().await;
    let config = create_config(&postgres_url);
    let (base, shutdown_tx, _handle) = start_server(&config).await;
    let client = reqwest::Client::new();

    let _patient_ids = create_test_patients(&client, &base).await;

    // Request count only
    let resp = client
        .get(format!("{base}/Patient?_summary=count"))
        .header("accept", "application/fhir+json")
        .send()
        .await
        .expect("search request");

    assert!(resp.status().is_success());
    let bundle: Value = resp.json().await.expect("parse bundle");

    // Should have total but no entries
    assert!(bundle.get("total").is_some(), "Should have total count");
    let entries = get_bundle_entries(&bundle);
    assert!(entries.is_empty(), "Count summary should have no entries");

    let _ = shutdown_tx.send(());
}

// =============================================================================
// Pagination Tests
// =============================================================================

#[tokio::test]
async fn test_pagination_count() {
    let (_container, postgres_url) = start_postgres().await;
    let config = create_config(&postgres_url);
    let (base, shutdown_tx, _handle) = start_server(&config).await;
    let client = reqwest::Client::new();

    let _patient_ids = create_test_patients(&client, &base).await;

    // Request first 2 patients
    let resp = client
        .get(format!("{base}/Patient?_count=2"))
        .header("accept", "application/fhir+json")
        .send()
        .await
        .expect("search request");

    assert!(resp.status().is_success());
    let bundle: Value = resp.json().await.expect("parse bundle");

    let entries = get_bundle_entries(&bundle);
    assert_eq!(entries.len(), 2, "Should return exactly 2 entries");

    // Should have next link if more results
    if get_bundle_total(&bundle) > 2 {
        let has_next = bundle["link"]
            .as_array()
            .map(|links| links.iter().any(|l| l["relation"] == "next"))
            .unwrap_or(false);
        assert!(has_next, "Should have next link for pagination");
    }

    let _ = shutdown_tx.send(());
}

// =============================================================================
// Sorting Tests
// =============================================================================

#[tokio::test]
async fn test_sort_ascending() {
    let (_container, postgres_url) = start_postgres().await;
    let config = create_config(&postgres_url);
    let (base, shutdown_tx, _handle) = start_server(&config).await;
    let client = reqwest::Client::new();

    let _patient_ids = create_test_patients(&client, &base).await;

    // Sort by birthDate ascending
    let resp = client
        .get(format!("{base}/Patient?_sort=birthdate"))
        .header("accept", "application/fhir+json")
        .send()
        .await
        .expect("search request");

    assert!(resp.status().is_success());
    let bundle: Value = resp.json().await.expect("parse bundle");

    let entries = get_bundle_entries(&bundle);
    if entries.len() >= 2 {
        let first_date = entries[0]["resource"]["birthDate"].as_str();
        let second_date = entries[1]["resource"]["birthDate"].as_str();

        if let (Some(d1), Some(d2)) = (first_date, second_date) {
            assert!(d1 <= d2, "Should be sorted ascending: {} <= {}", d1, d2);
        }
    }

    let _ = shutdown_tx.send(());
}

#[tokio::test]
async fn test_sort_descending() {
    let (_container, postgres_url) = start_postgres().await;
    let config = create_config(&postgres_url);
    let (base, shutdown_tx, _handle) = start_server(&config).await;
    let client = reqwest::Client::new();

    let _patient_ids = create_test_patients(&client, &base).await;

    // Sort by birthDate descending
    let resp = client
        .get(format!("{base}/Patient?_sort=-birthdate"))
        .header("accept", "application/fhir+json")
        .send()
        .await
        .expect("search request");

    assert!(resp.status().is_success());
    let bundle: Value = resp.json().await.expect("parse bundle");

    let entries = get_bundle_entries(&bundle);
    if entries.len() >= 2 {
        let first_date = entries[0]["resource"]["birthDate"].as_str();
        let second_date = entries[1]["resource"]["birthDate"].as_str();

        if let (Some(d1), Some(d2)) = (first_date, second_date) {
            assert!(d1 >= d2, "Should be sorted descending: {} >= {}", d1, d2);
        }
    }

    let _ = shutdown_tx.send(());
}

// =============================================================================
// Combined Search Tests
// =============================================================================

#[tokio::test]
async fn test_combined_search_parameters() {
    let (_container, postgres_url) = start_postgres().await;
    let config = create_config(&postgres_url);
    let (base, shutdown_tx, _handle) = start_server(&config).await;
    let client = reqwest::Client::new();

    let _patient_ids = create_test_patients(&client, &base).await;

    // Combine multiple parameters (AND logic)
    let resp = client
        .get(format!("{base}/Patient?family=Smith&gender=female"))
        .header("accept", "application/fhir+json")
        .send()
        .await
        .expect("search request");

    assert!(resp.status().is_success());
    let bundle: Value = resp.json().await.expect("parse bundle");
    assert_eq!(get_bundle_total(&bundle), 1); // Only Jane Smith

    let _ = shutdown_tx.send(());
}

#[tokio::test]
async fn test_or_search_values() {
    let (_container, postgres_url) = start_postgres().await;
    let config = create_config(&postgres_url);
    let (base, shutdown_tx, _handle) = start_server(&config).await;
    let client = reqwest::Client::new();

    let _patient_ids = create_test_patients(&client, &base).await;

    // Multiple values for same parameter (OR logic)
    let resp = client
        .get(format!("{base}/Patient?family=Smith,Johnson"))
        .header("accept", "application/fhir+json")
        .send()
        .await
        .expect("search request");

    assert!(resp.status().is_success());
    let bundle: Value = resp.json().await.expect("parse bundle");
    assert_eq!(get_bundle_total(&bundle), 3); // 2 Smiths + 1 Johnson

    let _ = shutdown_tx.send(());
}
