//! Integration tests for PostgreSQL native transactions.
//!
//! These tests verify ACID guarantees for FHIR Bundle transactions:
//! - Atomicity: All-or-nothing execution
//! - Consistency: Database remains in valid state
//! - Isolation: Concurrent transactions don't interfere
//! - Durability: Committed changes persist
//!
//! Requirements:
//! - Docker running (for testcontainers)

use std::sync::Arc;

use octofhir_config::ConfigurationManager;
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
        pool_size: 10, // Higher for concurrent transaction tests
        connect_timeout_ms: 10000,
        idle_timeout_ms: Some(60000),
        ..Default::default()
    });
    config
}

async fn start_server(
    config: &AppConfig,
) -> (String, tokio::sync::oneshot::Sender<()>, JoinHandle<()>) {
    let config_manager = Arc::new(
        ConfigurationManager::builder()
            .build()
            .await
            .expect("build config manager"),
    );
    let app = build_app(config, config_manager).await.expect("build app");

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

/// Helper to check if a resource exists
async fn resource_exists(
    client: &reqwest::Client,
    base: &str,
    resource_type: &str,
    id: &str,
) -> bool {
    let resp = client
        .get(format!("{base}/{resource_type}/{id}"))
        .header("accept", "application/fhir+json")
        .send()
        .await
        .expect("read request");

    resp.status().is_success()
}

/// Helper to read a resource
async fn read_resource(
    client: &reqwest::Client,
    base: &str,
    resource_type: &str,
    id: &str,
) -> Option<Value> {
    let resp = client
        .get(format!("{base}/{resource_type}/{id}"))
        .header("accept", "application/fhir+json")
        .send()
        .await
        .expect("read request");

    if resp.status().is_success() {
        resp.json().await.ok()
    } else {
        None
    }
}

// =============================================================================
// Transaction Commit Tests (Atomicity - Success Case)
// =============================================================================

#[tokio::test]
async fn test_transaction_commit_all_creates() {
    let (_container, postgres_url) = start_postgres().await;
    let config = create_config(&postgres_url);
    let (base, shutdown_tx, _handle) = start_server(&config).await;
    let client = reqwest::Client::new();

    // Transaction bundle with multiple CREATE operations
    let bundle = json!({
        "resourceType": "Bundle",
        "type": "transaction",
        "entry": [
            {
                "fullUrl": "urn:uuid:patient-1",
                "resource": {
                    "resourceType": "Patient",
                    "name": [{"family": "Transaction", "given": ["Test"]}]
                },
                "request": {
                    "method": "POST",
                    "url": "Patient"
                }
            },
            {
                "fullUrl": "urn:uuid:obs-1",
                "resource": {
                    "resourceType": "Observation",
                    "status": "final",
                    "code": {"coding": [{"system": "http://loinc.org", "code": "8867-4"}]},
                    "subject": {"reference": "urn:uuid:patient-1"}
                },
                "request": {
                    "method": "POST",
                    "url": "Observation"
                }
            }
        ]
    });

    let resp = client
        .post(format!("{base}/"))
        .header("content-type", "application/fhir+json")
        .json(&bundle)
        .send()
        .await
        .expect("transaction request");

    assert!(resp.status().is_success(), "Transaction should succeed");

    let response_bundle: Value = resp.json().await.expect("parse response");
    assert_eq!(response_bundle["type"], "transaction-response");

    // Extract created IDs from response
    let entries = response_bundle["entry"].as_array().expect("entries");
    assert_eq!(entries.len(), 2);

    // Verify both resources exist
    for entry in entries {
        let location = entry["response"]["location"].as_str();
        if let Some(loc) = location {
            let parts: Vec<&str> = loc.split('/').collect();
            if parts.len() >= 2 {
                let resource_type = parts[0];
                let id = parts[1].split('?').next().unwrap_or(parts[1]);
                assert!(
                    resource_exists(&client, &base, resource_type, id).await,
                    "Resource {} should exist after commit",
                    loc
                );
            }
        }
    }

    let _ = shutdown_tx.send(());
}

#[tokio::test]
async fn test_transaction_commit_mixed_operations() {
    let (_container, postgres_url) = start_postgres().await;
    let config = create_config(&postgres_url);
    let (base, shutdown_tx, _handle) = start_server(&config).await;
    let client = reqwest::Client::new();

    // First create a patient
    let patient = json!({
        "resourceType": "Patient",
        "name": [{"family": "ToUpdate", "given": ["Initial"]}]
    });

    let resp = client
        .post(format!("{base}/Patient"))
        .header("content-type", "application/fhir+json")
        .json(&patient)
        .send()
        .await
        .expect("create patient");

    assert!(resp.status().is_success());
    let created: Value = resp.json().await.expect("parse");
    let patient_id = created["id"].as_str().expect("id");

    // Transaction with UPDATE and CREATE
    let bundle = json!({
        "resourceType": "Bundle",
        "type": "transaction",
        "entry": [
            {
                "resource": {
                    "resourceType": "Patient",
                    "id": patient_id,
                    "name": [{"family": "ToUpdate", "given": ["Updated"]}]
                },
                "request": {
                    "method": "PUT",
                    "url": format!("Patient/{}", patient_id)
                }
            },
            {
                "fullUrl": "urn:uuid:new-obs",
                "resource": {
                    "resourceType": "Observation",
                    "status": "final",
                    "code": {"coding": [{"system": "http://loinc.org", "code": "8867-4"}]},
                    "subject": {"reference": format!("Patient/{}", patient_id)}
                },
                "request": {
                    "method": "POST",
                    "url": "Observation"
                }
            }
        ]
    });

    let resp = client
        .post(format!("{base}/"))
        .header("content-type", "application/fhir+json")
        .json(&bundle)
        .send()
        .await
        .expect("transaction request");

    assert!(
        resp.status().is_success(),
        "Mixed transaction should succeed"
    );

    // Verify patient was updated
    let updated_patient = read_resource(&client, &base, "Patient", patient_id)
        .await
        .expect("patient should exist");

    let given = &updated_patient["name"][0]["given"][0];
    assert_eq!(given, "Updated", "Patient name should be updated");

    let _ = shutdown_tx.send(());
}

// =============================================================================
// Transaction Rollback Tests (Atomicity - Failure Case)
// =============================================================================

#[tokio::test]
async fn test_transaction_rollback_on_invalid_reference() {
    let (_container, postgres_url) = start_postgres().await;
    let config = create_config(&postgres_url);
    let (base, shutdown_tx, _handle) = start_server(&config).await;
    let client = reqwest::Client::new();

    // Transaction with invalid reference - should rollback all
    let bundle = json!({
        "resourceType": "Bundle",
        "type": "transaction",
        "entry": [
            {
                "fullUrl": "urn:uuid:patient-rollback",
                "resource": {
                    "resourceType": "Patient",
                    "name": [{"family": "ShouldRollback"}]
                },
                "request": {
                    "method": "POST",
                    "url": "Patient"
                }
            },
            {
                "resource": {
                    "resourceType": "Observation",
                    "status": "final",
                    "code": {"coding": [{"system": "http://loinc.org", "code": "8867-4"}]},
                    "subject": {"reference": "Patient/nonexistent-id-12345"}
                },
                "request": {
                    "method": "PUT",
                    "url": "Observation/nonexistent-obs-12345"
                }
            }
        ]
    });

    let resp = client
        .post(format!("{base}/"))
        .header("content-type", "application/fhir+json")
        .json(&bundle)
        .send()
        .await
        .expect("transaction request");

    // Transaction should fail
    assert!(
        !resp.status().is_success(),
        "Transaction should fail on invalid reference"
    );

    // Search for the patient that should have been rolled back
    let search_resp = client
        .get(format!("{base}/Patient?family=ShouldRollback"))
        .header("accept", "application/fhir+json")
        .send()
        .await
        .expect("search request");

    let search_bundle: Value = search_resp.json().await.expect("parse");
    let total = search_bundle["total"].as_u64().unwrap_or(0);

    assert_eq!(
        total, 0,
        "Patient should be rolled back when transaction fails"
    );

    let _ = shutdown_tx.send(());
}

#[tokio::test]
async fn test_transaction_rollback_preserves_existing_data() {
    let (_container, postgres_url) = start_postgres().await;
    let config = create_config(&postgres_url);
    let (base, shutdown_tx, _handle) = start_server(&config).await;
    let client = reqwest::Client::new();

    // First create a patient that should NOT be affected by failed transaction
    let patient = json!({
        "resourceType": "Patient",
        "name": [{"family": "PreserveMe", "given": ["Original"]}]
    });

    let resp = client
        .post(format!("{base}/Patient"))
        .header("content-type", "application/fhir+json")
        .json(&patient)
        .send()
        .await
        .expect("create patient");

    assert!(resp.status().is_success());
    let created: Value = resp.json().await.expect("parse");
    let patient_id = created["id"].as_str().expect("id");

    // Try a failing transaction that attempts to update this patient
    let bundle = json!({
        "resourceType": "Bundle",
        "type": "transaction",
        "entry": [
            {
                "resource": {
                    "resourceType": "Patient",
                    "id": patient_id,
                    "name": [{"family": "PreserveMe", "given": ["Modified"]}]
                },
                "request": {
                    "method": "PUT",
                    "url": format!("Patient/{}", patient_id)
                }
            },
            {
                "resource": {
                    "resourceType": "InvalidResourceType",
                    "someField": "value"
                },
                "request": {
                    "method": "POST",
                    "url": "InvalidResourceType"
                }
            }
        ]
    });

    let resp = client
        .post(format!("{base}/"))
        .header("content-type", "application/fhir+json")
        .json(&bundle)
        .send()
        .await
        .expect("transaction request");

    // Transaction should fail
    assert!(
        !resp.status().is_success(),
        "Transaction should fail on invalid resource"
    );

    // Verify original patient is unchanged
    let preserved = read_resource(&client, &base, "Patient", patient_id)
        .await
        .expect("patient should still exist");

    let given = &preserved["name"][0]["given"][0];
    assert_eq!(
        given, "Original",
        "Patient should be unchanged after rollback"
    );

    let _ = shutdown_tx.send(());
}

// =============================================================================
// Batch vs Transaction Tests (Isolation Difference)
// =============================================================================

#[tokio::test]
async fn test_batch_partial_failure() {
    let (_container, postgres_url) = start_postgres().await;
    let config = create_config(&postgres_url);
    let (base, shutdown_tx, _handle) = start_server(&config).await;
    let client = reqwest::Client::new();

    // Batch with one valid and one invalid operation
    // Unlike transaction, valid operation should succeed
    let bundle = json!({
        "resourceType": "Bundle",
        "type": "batch",
        "entry": [
            {
                "resource": {
                    "resourceType": "Patient",
                    "name": [{"family": "BatchSuccess"}]
                },
                "request": {
                    "method": "POST",
                    "url": "Patient"
                }
            },
            {
                "resource": {
                    "resourceType": "Patient",
                    "id": "nonexistent-for-update"
                },
                "request": {
                    "method": "PUT",
                    "url": "Patient/nonexistent-for-update",
                    "ifMatch": "W/\"999\""
                }
            }
        ]
    });

    let resp = client
        .post(format!("{base}/"))
        .header("content-type", "application/fhir+json")
        .json(&bundle)
        .send()
        .await
        .expect("batch request");

    // Batch might return 200 even with partial failures
    let response_bundle: Value = resp.json().await.expect("parse response");
    assert_eq!(response_bundle["type"], "batch-response");

    // First entry should succeed, second might fail
    let entries = response_bundle["entry"].as_array().expect("entries");

    // Check first entry succeeded
    let first_status = entries[0]["response"]["status"].as_str().unwrap_or("");
    assert!(
        first_status.starts_with("201") || first_status.starts_with("200"),
        "First batch entry should succeed"
    );

    // Verify the successful patient exists
    let search_resp = client
        .get(format!("{base}/Patient?family=BatchSuccess"))
        .header("accept", "application/fhir+json")
        .send()
        .await
        .expect("search request");

    let search_bundle: Value = search_resp.json().await.expect("parse");
    let total = search_bundle["total"].as_u64().unwrap_or(0);

    assert!(
        total > 0,
        "Successful batch entry should persist despite other failures"
    );

    let _ = shutdown_tx.send(());
}

// =============================================================================
// Concurrent Transaction Tests (Isolation)
// =============================================================================

#[tokio::test]
async fn test_concurrent_transactions_isolation() {
    let (_container, postgres_url) = start_postgres().await;
    let config = create_config(&postgres_url);
    let (base, shutdown_tx, _handle) = start_server(&config).await;
    let client = reqwest::Client::new();

    // Create initial patient
    let patient = json!({
        "resourceType": "Patient",
        "name": [{"family": "ConcurrentTest", "given": ["Initial"]}],
        "birthDate": "2000-01-01"
    });

    let resp = client
        .post(format!("{base}/Patient"))
        .header("content-type", "application/fhir+json")
        .json(&patient)
        .send()
        .await
        .expect("create patient");

    let created: Value = resp.json().await.expect("parse");
    let patient_id = created["id"].as_str().expect("id").to_string();
    let base_clone = base.clone();

    // Launch two concurrent transactions updating different fields
    let id1 = patient_id.clone();
    let base1 = base_clone.clone();
    let tx1 = tokio::spawn(async move {
        let client = reqwest::Client::new();
        let bundle = json!({
            "resourceType": "Bundle",
            "type": "transaction",
            "entry": [{
                "resource": {
                    "resourceType": "Patient",
                    "id": id1,
                    "name": [{"family": "ConcurrentTest", "given": ["FromTx1"]}],
                    "birthDate": "2000-01-01"
                },
                "request": {
                    "method": "PUT",
                    "url": format!("Patient/{}", id1)
                }
            }]
        });

        client
            .post(format!("{base1}/"))
            .header("content-type", "application/fhir+json")
            .json(&bundle)
            .send()
            .await
    });

    let id2 = patient_id.clone();
    let base2 = base_clone;
    let tx2 = tokio::spawn(async move {
        let client = reqwest::Client::new();
        let bundle = json!({
            "resourceType": "Bundle",
            "type": "transaction",
            "entry": [{
                "resource": {
                    "resourceType": "Patient",
                    "id": id2,
                    "name": [{"family": "ConcurrentTest", "given": ["FromTx2"]}],
                    "birthDate": "2000-01-01"
                },
                "request": {
                    "method": "PUT",
                    "url": format!("Patient/{}", id2)
                }
            }]
        });

        client
            .post(format!("{base2}/"))
            .header("content-type", "application/fhir+json")
            .json(&bundle)
            .send()
            .await
    });

    // Wait for both transactions
    let (result1, result2) = tokio::join!(tx1, tx2);

    let resp1 = result1.expect("tx1").expect("tx1 response");
    let resp2 = result2.expect("tx2").expect("tx2 response");

    // At least one should succeed (the other might fail due to optimistic locking)
    let success_count = [resp1.status().is_success(), resp2.status().is_success()]
        .iter()
        .filter(|&&x| x)
        .count();

    assert!(
        success_count >= 1,
        "At least one concurrent transaction should succeed"
    );

    // Final state should be consistent (one of the two values)
    let final_patient = read_resource(&client, &base, "Patient", &patient_id)
        .await
        .expect("patient should exist");

    let given = final_patient["name"][0]["given"][0].as_str().unwrap_or("");
    assert!(
        given == "FromTx1" || given == "FromTx2",
        "Final state should reflect one of the transactions"
    );

    let _ = shutdown_tx.send(());
}

// =============================================================================
// Reference Resolution Tests
// =============================================================================

#[tokio::test]
async fn test_transaction_internal_reference_resolution() {
    let (_container, postgres_url) = start_postgres().await;
    let config = create_config(&postgres_url);
    let (base, shutdown_tx, _handle) = start_server(&config).await;
    let client = reqwest::Client::new();

    // Transaction with internal references (urn:uuid)
    let bundle = json!({
        "resourceType": "Bundle",
        "type": "transaction",
        "entry": [
            {
                "fullUrl": "urn:uuid:new-patient",
                "resource": {
                    "resourceType": "Patient",
                    "name": [{"family": "ReferenceTest"}]
                },
                "request": {
                    "method": "POST",
                    "url": "Patient"
                }
            },
            {
                "fullUrl": "urn:uuid:new-encounter",
                "resource": {
                    "resourceType": "Encounter",
                    "status": "finished",
                    "class": {
                        "system": "http://terminology.hl7.org/CodeSystem/v3-ActCode",
                        "code": "AMB"
                    },
                    "subject": {"reference": "urn:uuid:new-patient"}
                },
                "request": {
                    "method": "POST",
                    "url": "Encounter"
                }
            },
            {
                "fullUrl": "urn:uuid:new-condition",
                "resource": {
                    "resourceType": "Condition",
                    "subject": {"reference": "urn:uuid:new-patient"},
                    "encounter": {"reference": "urn:uuid:new-encounter"},
                    "code": {
                        "coding": [{"system": "http://snomed.info/sct", "code": "386661006"}]
                    }
                },
                "request": {
                    "method": "POST",
                    "url": "Condition"
                }
            }
        ]
    });

    let resp = client
        .post(format!("{base}/"))
        .header("content-type", "application/fhir+json")
        .json(&bundle)
        .send()
        .await
        .expect("transaction request");

    assert!(
        resp.status().is_success(),
        "Transaction with internal refs should succeed"
    );

    let response_bundle: Value = resp.json().await.expect("parse response");
    let entries = response_bundle["entry"].as_array().expect("entries");

    // Extract created IDs
    let mut patient_id = String::new();
    let mut encounter_id = String::new();
    let mut condition_id = String::new();

    for entry in entries {
        if let Some(location) = entry["response"]["location"].as_str() {
            if location.starts_with("Patient/") {
                patient_id = location
                    .split('/')
                    .nth(1)
                    .unwrap_or("")
                    .split('?')
                    .next()
                    .unwrap_or("")
                    .to_string();
            } else if location.starts_with("Encounter/") {
                encounter_id = location
                    .split('/')
                    .nth(1)
                    .unwrap_or("")
                    .split('?')
                    .next()
                    .unwrap_or("")
                    .to_string();
            } else if location.starts_with("Condition/") {
                condition_id = location
                    .split('/')
                    .nth(1)
                    .unwrap_or("")
                    .split('?')
                    .next()
                    .unwrap_or("")
                    .to_string();
            }
        }
    }

    assert!(!patient_id.is_empty(), "Patient should be created");
    assert!(!encounter_id.is_empty(), "Encounter should be created");
    assert!(!condition_id.is_empty(), "Condition should be created");

    // Verify references were resolved
    let condition = read_resource(&client, &base, "Condition", &condition_id)
        .await
        .expect("condition should exist");

    let subject_ref = condition["subject"]["reference"].as_str().unwrap_or("");
    let encounter_ref = condition["encounter"]["reference"].as_str().unwrap_or("");

    assert!(
        subject_ref.contains(&patient_id),
        "Subject reference should be resolved to Patient/{}",
        patient_id
    );
    assert!(
        encounter_ref.contains(&encounter_id),
        "Encounter reference should be resolved to Encounter/{}",
        encounter_id
    );

    let _ = shutdown_tx.send(());
}

// =============================================================================
// Transaction with Delete Tests
// =============================================================================

#[tokio::test]
async fn test_transaction_with_delete() {
    let (_container, postgres_url) = start_postgres().await;
    let config = create_config(&postgres_url);
    let (base, shutdown_tx, _handle) = start_server(&config).await;
    let client = reqwest::Client::new();

    // First create a patient
    let patient = json!({
        "resourceType": "Patient",
        "name": [{"family": "ToDelete"}]
    });

    let resp = client
        .post(format!("{base}/Patient"))
        .header("content-type", "application/fhir+json")
        .json(&patient)
        .send()
        .await
        .expect("create patient");

    let created: Value = resp.json().await.expect("parse");
    let patient_id = created["id"].as_str().expect("id");

    // Transaction that deletes the patient and creates a new one
    let bundle = json!({
        "resourceType": "Bundle",
        "type": "transaction",
        "entry": [
            {
                "request": {
                    "method": "DELETE",
                    "url": format!("Patient/{}", patient_id)
                }
            },
            {
                "resource": {
                    "resourceType": "Patient",
                    "name": [{"family": "ReplacementPatient"}]
                },
                "request": {
                    "method": "POST",
                    "url": "Patient"
                }
            }
        ]
    });

    let resp = client
        .post(format!("{base}/"))
        .header("content-type", "application/fhir+json")
        .json(&bundle)
        .send()
        .await
        .expect("transaction request");

    assert!(
        resp.status().is_success(),
        "Transaction with delete should succeed"
    );

    // Verify deleted patient no longer exists
    assert!(
        !resource_exists(&client, &base, "Patient", patient_id).await,
        "Deleted patient should not exist"
    );

    // Verify new patient exists
    let search_resp = client
        .get(format!("{base}/Patient?family=ReplacementPatient"))
        .header("accept", "application/fhir+json")
        .send()
        .await
        .expect("search request");

    let search_bundle: Value = search_resp.json().await.expect("parse");
    let total = search_bundle["total"].as_u64().unwrap_or(0);

    assert_eq!(total, 1, "New patient should exist");

    let _ = shutdown_tx.send(());
}

// =============================================================================
// Conditional Operations Tests
// =============================================================================

#[tokio::test]
async fn test_transaction_conditional_create() {
    let (_container, postgres_url) = start_postgres().await;
    let config = create_config(&postgres_url);
    let (base, shutdown_tx, _handle) = start_server(&config).await;
    let client = reqwest::Client::new();

    // First create a patient with a specific identifier
    let patient = json!({
        "resourceType": "Patient",
        "identifier": [{"system": "http://example.org/mrn", "value": "COND-001"}],
        "name": [{"family": "ConditionalFirst"}]
    });

    let resp = client
        .post(format!("{base}/Patient"))
        .header("content-type", "application/fhir+json")
        .json(&patient)
        .send()
        .await
        .expect("create patient");

    assert!(resp.status().is_success());

    // Transaction with conditional create (if-none-exist)
    // Should NOT create a duplicate
    let bundle = json!({
        "resourceType": "Bundle",
        "type": "transaction",
        "entry": [
            {
                "resource": {
                    "resourceType": "Patient",
                    "identifier": [{"system": "http://example.org/mrn", "value": "COND-001"}],
                    "name": [{"family": "ConditionalSecond"}]
                },
                "request": {
                    "method": "POST",
                    "url": "Patient",
                    "ifNoneExist": "identifier=http://example.org/mrn|COND-001"
                }
            }
        ]
    });

    let resp = client
        .post(format!("{base}/"))
        .header("content-type", "application/fhir+json")
        .json(&bundle)
        .send()
        .await
        .expect("transaction request");

    // Should succeed (either created or found existing)
    assert!(
        resp.status().is_success(),
        "Conditional create transaction should succeed"
    );

    // Verify only one patient with this identifier
    let search_resp = client
        .get(format!(
            "{base}/Patient?identifier=http://example.org/mrn|COND-001"
        ))
        .header("accept", "application/fhir+json")
        .send()
        .await
        .expect("search request");

    let search_bundle: Value = search_resp.json().await.expect("parse");
    let total = search_bundle["total"].as_u64().unwrap_or(0);

    assert_eq!(
        total, 1,
        "Should have exactly one patient with this identifier"
    );

    let _ = shutdown_tx.send(());
}

// =============================================================================
// Large Transaction Tests
// =============================================================================

#[tokio::test]
async fn test_large_transaction_performance() {
    let (_container, postgres_url) = start_postgres().await;
    let config = create_config(&postgres_url);
    let (base, shutdown_tx, _handle) = start_server(&config).await;
    let client = reqwest::Client::new();

    // Create a transaction with many entries
    let mut entries = Vec::new();
    for i in 0..50 {
        entries.push(json!({
            "resource": {
                "resourceType": "Patient",
                "name": [{"family": format!("BulkPatient{}", i)}],
                "birthDate": "2000-01-01"
            },
            "request": {
                "method": "POST",
                "url": "Patient"
            }
        }));
    }

    let bundle = json!({
        "resourceType": "Bundle",
        "type": "transaction",
        "entry": entries
    });

    let start = std::time::Instant::now();

    let resp = client
        .post(format!("{base}/"))
        .header("content-type", "application/fhir+json")
        .json(&bundle)
        .send()
        .await
        .expect("transaction request");

    let duration = start.elapsed();

    assert!(
        resp.status().is_success(),
        "Large transaction should succeed"
    );

    let response_bundle: Value = resp.json().await.expect("parse response");
    let response_entries = response_bundle["entry"].as_array().expect("entries");

    assert_eq!(
        response_entries.len(),
        50,
        "All entries should be processed"
    );

    // Performance check: 50 entries should complete in reasonable time
    // This is a soft assertion - mainly for benchmarking
    println!(
        "Large transaction (50 entries) completed in: {:?}",
        duration
    );
    assert!(
        duration.as_secs() < 30,
        "Large transaction should complete within 30 seconds"
    );

    // Verify all patients exist
    let search_resp = client
        .get(format!(
            "{base}/Patient?name:contains=BulkPatient&_count=100"
        ))
        .header("accept", "application/fhir+json")
        .send()
        .await
        .expect("search request");

    let search_bundle: Value = search_resp.json().await.expect("parse");
    let total = search_bundle["total"].as_u64().unwrap_or(0);

    assert_eq!(total, 50, "All 50 bulk patients should exist");

    let _ = shutdown_tx.send(());
}
