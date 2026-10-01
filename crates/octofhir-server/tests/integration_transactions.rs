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
use testcontainers::{ContainerAsync, ImageExt, runners::AsyncRunner};
use testcontainers_modules::postgres::Postgres;
use tokio::task::JoinHandle;

fn path_patch(operation: &str, path: &str, extra: Vec<Value>) -> Value {
    let mut parts = vec![
        json!({"name":"type","valueCode":operation}),
        json!({"name":"path","valueString":path}),
    ];
    parts.extend(extra);
    json!({"resourceType":"Parameters","parameter":[{"name":"operation","part":parts}]})
}

#[tokio::test]
async fn validate_honors_explicit_profiles_in_query_and_parameters() {
    use reqwest::StatusCode;
    let (_container, postgres_url) = start_postgres().await;
    let config = create_config(&postgres_url);
    let (base, shutdown, _server) = start_server(&config).await;
    let client = reqwest::Client::new();
    let resource = json!({"resourceType":"Patient","active":true});
    let response = client
        .post(format!("{base}/Patient/$validate"))
        .json(&resource)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let outcome: Value = response.json().await.unwrap();
    assert_eq!(outcome["resourceType"], "OperationOutcome");
    assert!(!outcome["issue"].as_array().unwrap().is_empty());
    assert_eq!(outcome["issue"][0]["severity"], "information");

    for (profile, expected) in [
        (
            "http://hl7.org/fhir/StructureDefinition/Patient",
            StatusCode::OK,
        ),
        (
            "http://example.invalid/unavailable-profile",
            StatusCode::BAD_REQUEST,
        ),
    ] {
        let response = client
            .post(format!("{base}/Patient/$validate"))
            .query(&[("profile", profile)])
            .json(&resource)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), expected, "query profile {profile}");
        let response = client
            .post(format!("{base}/Patient/$validate"))
            .json(&json!({"resourceType":"Parameters","parameter":[
                {"name":"resource","resource":resource},
                {"name":"profile","valueUri":profile}
            ]}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), expected, "Parameters profile {profile}");
    }
    let response = client
        .post(format!("{base}/Patient/$validate"))
        .json(&json!({"resourceType":"Patient","active":"wrong"}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let outcome: Value = response.json().await.unwrap();
    assert!(
        outcome["issue"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue["severity"] == "error")
    );
    let _ = shutdown.send(());
}

async fn send_bundle(
    client: &reqwest::Client,
    base: &str,
    kind: &str,
    entries: Value,
) -> (reqwest::StatusCode, Value) {
    let response = client
        .post(base)
        .json(&json!({"resourceType":"Bundle","type":kind,"entry":entries}))
        .send()
        .await
        .unwrap();
    let status = response.status();
    (status, response.json().await.unwrap())
}

#[tokio::test]
async fn conditional_match_cardinality_ignores_result_controls_and_decodes_identifiers() {
    use reqwest::StatusCode;
    let (_container, postgres_url) = start_postgres().await;
    let config = create_config(&postgres_url);
    let (base, shutdown, _server) = start_server(&config).await;
    let client = reqwest::Client::new();
    let mut ids = Vec::new();
    for _ in 0..2 {
        let response = client.post(format!("{base}/Patient"))
            .json(&json!({"resourceType":"Patient","identifier":[{"system":"urn:test","value":"duplicate"}]}))
            .send().await.unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        let body: Value = response.json().await.unwrap();
        ids.push(body["id"].as_str().unwrap().to_owned());
    }
    for controls in [
        "_count=1",
        "_count=0",
        "_offset=100",
        "_summary=count",
        "_elements=id&_count=1",
    ] {
        let url = format!("Patient?identifier=urn%3Atest%7Cduplicate&{controls}");
        let response = client.delete(format!("{base}/{url}")).send().await.unwrap();
        assert_eq!(
            response.status(),
            StatusCode::PRECONDITION_FAILED,
            "{controls}"
        );
        let response = client
            .post(format!("{base}/Patient"))
            .header(
                "If-None-Exist",
                format!("identifier=urn%3Atest%7Cduplicate&{controls}"),
            )
            .json(&json!({"resourceType":"Patient","active":true}))
            .send()
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::PRECONDITION_FAILED,
            "{controls}"
        );
        for kind in ["batch", "transaction"] {
            let (status, body) = send_bundle(
                &client,
                &base,
                kind,
                json!([{"request":{"method":"DELETE","url":url}}]),
            )
            .await;
            if kind == "batch" {
                assert_eq!(
                    body["entry"][0]["response"]["status"], "412 Precondition Failed",
                    "{body}"
                );
            } else {
                assert_eq!(status, StatusCode::PRECONDITION_FAILED, "{body}");
            }
        }
    }
    for id in ids {
        assert!(resource_exists(&client, &base, "Patient", &id).await);
    }

    let response = client.post(format!("{base}/Patient"))
        .json(&json!({"resourceType":"Patient","active":true,"identifier":[{"system":"urn:test","value":"unique"}]}))
        .send().await.unwrap();
    let patient: Value = response.json().await.unwrap();
    let id = patient["id"].as_str().unwrap();
    let (_, body) = send_bundle(&client, &base, "transaction", json!([{
        "request":{"method":"PUT","url":"Patient?identifier=urn%3Atest%7Cunique&_count=0&_offset=100"},
        "resource":{"resourceType":"Patient","id":id,"active":false,"identifier":[{"system":"urn:test","value":"unique"}]}
    }])).await;
    assert_eq!(body["entry"][0]["response"]["status"], "200 OK", "{body}");
    assert_eq!(body["entry"][0]["resource"]["id"], id, "{body}");
    assert_eq!(
        read_resource(&client, &base, "Patient", id).await.unwrap()["active"],
        false
    );
    let _ = shutdown.send(());
}

#[tokio::test]
async fn bundle_writes_follow_identity_validation_patch_and_version_contracts() {
    use reqwest::StatusCode;
    let (_container, postgres_url) = start_postgres().await;
    let config = create_config(&postgres_url);
    let (base, shutdown, _server) = start_server(&config).await;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .unwrap();
    let created: Value = client
        .post(format!("{base}/Patient"))
        .json(&json!({"resourceType":"Patient","active":true,"name":[
            {"use":"official","family":"One","given":["Same","Same","Third"]},
            {"use":"usual","family":"Two"}
        ]}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let id = created["id"].as_str().unwrap();
    let version = created["meta"]["versionId"].as_str().unwrap();
    let url = format!("Patient/{id}");

    for resource in [
        json!({"resourceType":"Patient","active":false}),
        json!({"resourceType":"Patient","id":"different","active":false}),
    ] {
        let response = client
            .put(format!("{base}/{url}"))
            .json(&resource)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        for kind in ["batch", "transaction"] {
            let (status, body) = send_bundle(
                &client,
                &base,
                kind,
                json!([{"resource":resource,"request":{"method":"PUT","url":url}}]),
            )
            .await;
            if kind == "batch" {
                assert_eq!(
                    body["entry"][0]["response"]["status"], "400 Bad Request",
                    "{body}"
                );
            } else {
                assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
            }
        }
    }
    let invalid = json!({"resourceType":"Patient","id":id,"active":"not-a-boolean"});
    let response = client
        .put(format!("{base}/{url}"))
        .json(&invalid)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let (_, body) = send_bundle(
        &client,
        &base,
        "batch",
        json!([{"resource":invalid,"request":{"method":"PUT","url":url}}]),
    )
    .await;
    assert!(
        body["entry"][0]["response"]["status"]
            .as_str()
            .unwrap()
            .starts_with("422"),
        "{body}"
    );

    // Two mutations of the same identity cannot share one history version.
    let (_, body) = send_bundle(&client, &base, "transaction", json!([
        {"resource":{"resourceType":"Patient","id":id,"active":false},"request":{"method":"PUT","url":url}},
        {"resource":{"resourceType":"Patient","id":id,"active":true},"request":{"method":"PUT","url":url}}
    ])).await;
    assert_eq!(body["resourceType"], "OperationOutcome", "{body}");
    assert_eq!(
        read_resource(&client, &base, "Patient", id).await.unwrap()["meta"]["versionId"],
        version
    );

    // Indices distinguish equal values; where selects the physical array member.
    let patch = path_patch(
        "replace",
        "Patient.name.where(use = 'official').given[1]",
        vec![json!({"name":"value","valueString":"Changed"})],
    );
    let (_, body) = send_bundle(
        &client,
        &base,
        "batch",
        json!([{"resource":patch,"request":{"method":"PATCH","url":url}}]),
    )
    .await;
    assert_eq!(body["entry"][0]["response"]["status"], "200 OK", "{body}");
    let current = read_resource(&client, &base, "Patient", id).await.unwrap();
    assert_eq!(
        current["name"][0]["given"],
        json!(["Same", "Changed", "Third"])
    );
    let current_version = current["meta"]["versionId"].clone();

    let invalid_patch = path_patch(
        "replace",
        "Patient.active",
        vec![json!({"name":"value","valueString":"wrong"})],
    );
    for kind in ["batch", "transaction"] {
        let (status, body) = send_bundle(
            &client,
            &base,
            kind,
            json!([{"resource":invalid_patch,"request":{"method":"PATCH","url":url}}]),
        )
        .await;
        if kind == "batch" {
            assert!(
                body["entry"][0]["response"]["status"]
                    .as_str()
                    .unwrap()
                    .starts_with("422"),
                "{body}"
            );
        } else {
            assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
        }
    }
    let valid_patch = path_patch(
        "replace",
        "Patient.active",
        vec![json!({"name":"value","valueBoolean":false})],
    );
    for kind in ["batch", "transaction"] {
        let (status, body) = send_bundle(&client, &base, kind, json!([{"resource":valid_patch,"request":{"method":"PATCH","url":url,"ifMatch":format!("W/\"{version}\"")}}])).await;
        if kind == "batch" {
            assert_eq!(
                body["entry"][0]["response"]["status"], "412 Precondition Failed",
                "{body}"
            );
        } else {
            assert_eq!(status, StatusCode::PRECONDITION_FAILED, "{body}");
        }
    }
    assert_eq!(
        read_resource(&client, &base, "Patient", id).await.unwrap()["meta"]["versionId"],
        current_version
    );

    for patch in [
        path_patch(
            "replace",
            "Patient.name.family",
            vec![json!({"name":"value","valueString":"Ambiguous"})],
        ),
        path_patch(
            "insert",
            "Patient.name[0].given",
            vec![
                json!({"name":"index","valueInteger":-1}),
                json!({"name":"value","valueString":"Negative"}),
            ],
        ),
        path_patch(
            "move",
            "Patient.name[0].given",
            vec![
                json!({"name":"source","valueInteger":0}),
                json!({"name":"destination","valueInteger":99}),
            ],
        ),
        json!([{"op":"replace","path":"/active","value":false}]),
    ] {
        let (_, body) = send_bundle(
            &client,
            &base,
            "batch",
            json!([{"resource":patch,"request":{"method":"PATCH","url":url}}]),
        )
        .await;
        assert_eq!(
            body["entry"][0]["response"]["status"], "400 Bad Request",
            "{body}"
        );
    }

    let move_patch = path_patch(
        "move",
        "Patient.name[0].given",
        vec![
            json!({"name":"source","valueInteger":0}),
            json!({"name":"destination","valueInteger":2}),
        ],
    );
    let (_, body) = send_bundle(
        &client,
        &base,
        "transaction",
        json!([{"resource":move_patch,"request":{"method":"PATCH","url":url}}]),
    )
    .await;
    assert_eq!(body["entry"][0]["response"]["status"], "200 OK", "{body}");
    assert_eq!(
        read_resource(&client, &base, "Patient", id).await.unwrap()["name"][0]["given"],
        json!(["Changed", "Third", "Same"])
    );

    for kind in ["batch", "transaction"] {
        let (_, body) = send_bundle(&client, &base, kind, json!([{"resource":{"resourceType":"Patient","id":"client-chosen-id","active":true},"request":{"method":"POST","url":"Patient"}}])).await;
        assert_eq!(
            body["entry"][0]["response"]["status"], "201 Created",
            "{body}"
        );
        assert_ne!(
            body["entry"][0]["resource"]["id"], "client-chosen-id",
            "{body}"
        );
    }
    let _ = shutdown.send(());
}

// =============================================================================
// Test Infrastructure
// =============================================================================

async fn start_postgres() -> (ContainerAsync<Postgres>, String) {
    // Match production PG (docker-compose postgres:18). The migration needs
    // PG14+ for tstzmultirange and the jsonpath[] date-extraction overloads;
    // the testcontainers default (11-alpine) lacks both.
    let container = Postgres::default()
        .with_tag("18-alpine")
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
    config.auth.policy.anonymous_access = true;
    config
}

async fn start_server(
    config: &AppConfig,
) -> (String, tokio::sync::oneshot::Sender<()>, JoinHandle<()>) {
    // Canonical packages are process-global. Every fixture uses a different DB;
    // serialize fixture lifetimes and rebuild the registry for that database.
    // Request concurrency inside an individual test is unaffected.
    static FIXTURE_LOCK: std::sync::OnceLock<Arc<tokio::sync::Mutex<()>>> =
        std::sync::OnceLock::new();
    let isolation = FIXTURE_LOCK
        .get_or_init(|| Arc::new(tokio::sync::Mutex::new(())))
        .clone()
        .lock_owned()
        .await;
    if octofhir_server::canonical::get_manager().is_some() {
        octofhir_server::canonical::rebuild_from_config_async(config)
            .await
            .expect("rebuild canonical registry for fixture database");
    }
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
        let _isolation = isolation;
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
        .post(&base)
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
        .post(&base)
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
                    "id": "nonexistent-obs-12345",
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
        .post(&base)
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
        .post(&base)
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
        .post(&base)
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

#[tokio::test]
async fn test_transaction_prefer_return_minimal_omits_mutation_resources() {
    let (_container, postgres_url) = start_postgres().await;
    let config = create_config(&postgres_url);
    let (base, shutdown_tx, _handle) = start_server(&config).await;
    let client = reqwest::Client::new();

    let patient = json!({
        "resourceType": "Patient",
        "name": [{"family": "PreferMinimal", "given": ["Before"]}]
    });

    let resp = client
        .post(format!("{base}/Patient"))
        .header("content-type", "application/fhir+json")
        .json(&patient)
        .send()
        .await
        .expect("create patient");
    let create_status = resp.status();
    let created: Value = resp.json().await.expect("parse create response");
    assert!(create_status.is_success(), "create failed: {created}");
    let patient_id = created["id"].as_str().expect("id");

    let bundle = json!({
        "resourceType": "Bundle",
        "type": "transaction",
        "entry": [
            {
                "resource": {
                    "resourceType": "Patient",
                    "id": patient_id,
                    "name": [{"family": "PreferMinimal", "given": ["After"]}]
                },
                "request": {
                    "method": "PUT",
                    "url": format!("Patient/{}", patient_id)
                }
            },
            {
                "request": {
                    "method": "GET",
                    "url": "Patient?family=PreferMinimal"
                }
            }
        ]
    });

    let resp = client
        .post(&base)
        .header("content-type", "application/fhir+json")
        .header("prefer", "return=minimal")
        .json(&bundle)
        .send()
        .await
        .expect("transaction request");
    let status = resp.status();
    let response_bundle: Value = resp.json().await.expect("parse response");
    assert!(status.is_success(), "transaction failed: {response_bundle}");
    let entries = response_bundle["entry"].as_array().expect("entries");
    assert_eq!(entries.len(), 2);
    assert!(
        entries[0].get("resource").is_none(),
        "return=minimal must omit mutation response resources"
    );
    assert_eq!(entries[1]["resource"]["resourceType"], "Bundle");
    assert_eq!(
        entries[1]["resource"]["entry"][0]["resource"]["name"][0]["given"][0],
        "After"
    );

    let _ = shutdown_tx.send(());
}

#[tokio::test]
async fn test_batch_prefer_return_minimal_omits_mutation_resources() {
    let (_container, postgres_url) = start_postgres().await;
    let config = create_config(&postgres_url);
    let (base, shutdown_tx, _handle) = start_server(&config).await;
    let client = reqwest::Client::new();

    let bundle = json!({
        "resourceType": "Bundle",
        "type": "batch",
        "entry": [
            {
                "resource": {
                    "resourceType": "Patient",
                    "name": [{"family": "BatchPreferMinimal"}]
                },
                "request": {
                    "method": "POST",
                    "url": "Patient"
                }
            }
        ]
    });

    let resp = client
        .post(&base)
        .header("content-type", "application/fhir+json")
        .header("prefer", "return=minimal")
        .json(&bundle)
        .send()
        .await
        .expect("batch request");
    assert!(resp.status().is_success());

    let response_bundle: Value = resp.json().await.expect("parse response");
    let entries = response_bundle["entry"].as_array().expect("entries");
    assert_eq!(entries.len(), 1);
    assert!(
        entries[0].get("resource").is_none(),
        "batch return=minimal must omit mutation response resources"
    );
    assert!(
        entries[0]["response"]["status"]
            .as_str()
            .unwrap_or("")
            .starts_with("201")
    );

    let search_resp = client
        .get(format!("{base}/Patient?family=BatchPreferMinimal"))
        .header("accept", "application/fhir+json")
        .send()
        .await
        .expect("search request");
    let search_bundle: Value = search_resp.json().await.expect("parse search response");
    assert_eq!(search_bundle["total"].as_u64().unwrap_or(0), 1);

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
            .post(&base1)
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
            .post(&base2)
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
        "At least one concurrent transaction should succeed: {}, {}",
        resp1.status(),
        resp2.status()
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
        .post(&base)
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
        .post(&base)
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
        .post(&base)
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

#[tokio::test]
async fn test_transaction_conditional_update() {
    let (_container, postgres_url) = start_postgres().await;
    let config = create_config(&postgres_url);
    let (base, shutdown_tx, _handle) = start_server(&config).await;
    let client = reqwest::Client::new();

    let patient = json!({
        "resourceType": "Patient",
        "identifier": [{"system": "http://example.org/mrn", "value": "COND-UPD-001"}],
        "name": [{"family": "ConditionalUpdate", "given": ["Before"]}]
    });

    let resp = client
        .post(format!("{base}/Patient"))
        .header("content-type", "application/fhir+json")
        .json(&patient)
        .send()
        .await
        .expect("create patient");

    assert!(resp.status().is_success());

    let bundle = json!({
        "resourceType": "Bundle",
        "type": "transaction",
        "entry": [
            {
                "resource": {
                    "resourceType": "Patient",
                    "identifier": [{"system": "http://example.org/mrn", "value": "COND-UPD-001"}],
                    "name": [{"family": "ConditionalUpdate", "given": ["After"]}]
                },
                "request": {
                    "method": "PUT",
                    "url": "Patient?identifier=http://example.org/mrn|COND-UPD-001"
                }
            }
        ]
    });

    let resp = client
        .post(&base)
        .header("content-type", "application/fhir+json")
        .json(&bundle)
        .send()
        .await
        .expect("transaction request");

    assert!(
        resp.status().is_success(),
        "Conditional update should succeed"
    );

    let search_resp = client
        .get(format!(
            "{base}/Patient?identifier=http://example.org/mrn|COND-UPD-001"
        ))
        .header("accept", "application/fhir+json")
        .send()
        .await
        .expect("search request");

    let search_bundle: Value = search_resp.json().await.expect("parse");
    assert_eq!(search_bundle["total"].as_u64().unwrap_or(0), 1);
    assert_eq!(
        search_bundle["entry"][0]["resource"]["name"][0]["given"][0],
        "After"
    );

    let _ = shutdown_tx.send(());
}

#[tokio::test]
async fn test_transaction_conditional_delete() {
    let (_container, postgres_url) = start_postgres().await;
    let config = create_config(&postgres_url);
    let (base, shutdown_tx, _handle) = start_server(&config).await;
    let client = reqwest::Client::new();

    let patient = json!({
        "resourceType": "Patient",
        "identifier": [{"system": "http://example.org/mrn", "value": "COND-DEL-001"}],
        "name": [{"family": "ConditionalDelete"}]
    });

    let resp = client
        .post(format!("{base}/Patient"))
        .header("content-type", "application/fhir+json")
        .json(&patient)
        .send()
        .await
        .expect("create patient");

    assert!(resp.status().is_success());

    let bundle = json!({
        "resourceType": "Bundle",
        "type": "transaction",
        "entry": [
            {
                "request": {
                    "method": "DELETE",
                    "url": "Patient?identifier=http://example.org/mrn|COND-DEL-001"
                }
            }
        ]
    });

    let resp = client
        .post(&base)
        .header("content-type", "application/fhir+json")
        .json(&bundle)
        .send()
        .await
        .expect("transaction request");

    assert!(
        resp.status().is_success(),
        "Conditional delete should succeed"
    );

    let search_resp = client
        .get(format!(
            "{base}/Patient?identifier=http://example.org/mrn|COND-DEL-001"
        ))
        .header("accept", "application/fhir+json")
        .send()
        .await
        .expect("search request");

    let search_bundle: Value = search_resp.json().await.expect("parse");
    assert_eq!(search_bundle["total"].as_u64().unwrap_or(0), 0);

    let _ = shutdown_tx.send(());
}

#[tokio::test]
async fn test_transaction_patch_entry() {
    let (_container, postgres_url) = start_postgres().await;
    let config = create_config(&postgres_url);
    let (base, shutdown_tx, _handle) = start_server(&config).await;
    let client = reqwest::Client::new();

    let patient = json!({
        "resourceType": "Patient",
        "name": [{"family": "PatchInTransaction", "given": ["Before"]}]
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

    let bundle = json!({
        "resourceType": "Bundle",
        "type": "transaction",
        "entry": [
            {
                "resource": {
                    "resourceType": "Parameters",
                    "parameter": [{"name": "operation", "part": [
                        {"name": "type", "valueCode": "replace"},
                        {"name": "path", "valueString": "Patient.name[0].given[0]"},
                        {"name": "value", "valueString": "After"}
                    ]}]
                },
                "request": {
                    "method": "PATCH",
                    "url": format!("Patient/{}", patient_id)
                }
            }
        ]
    });

    let resp = client
        .post(&base)
        .header("content-type", "application/fhir+json")
        .json(&bundle)
        .send()
        .await
        .expect("transaction request");

    assert!(
        resp.status().is_success(),
        "PATCH in transaction should succeed"
    );

    let patched = read_resource(&client, &base, "Patient", patient_id)
        .await
        .expect("patient should exist");

    assert_eq!(patched["name"][0]["given"][0], "After");

    let _ = shutdown_tx.send(());
}

#[tokio::test]
async fn test_transaction_get_search_sees_uncommitted_create() {
    let (_container, postgres_url) = start_postgres().await;
    let config = create_config(&postgres_url);
    let (base, shutdown_tx, _handle) = start_server(&config).await;
    let client = reqwest::Client::new();

    let bundle = json!({
        "resourceType": "Bundle",
        "type": "transaction",
        "entry": [
            {
                "resource": {
                    "resourceType": "Patient",
                    "identifier": [{"system": "http://example.org/mrn", "value": "TX-SEARCH-001"}],
                    "name": [{"family": "TxSearch"}]
                },
                "request": {
                    "method": "POST",
                    "url": "Patient"
                }
            },
            {
                "request": {
                    "method": "GET",
                    "url": "Patient?identifier=http://example.org/mrn|TX-SEARCH-001"
                }
            }
        ]
    });

    let resp = client
        .post(&base)
        .header("content-type", "application/fhir+json")
        .json(&bundle)
        .send()
        .await
        .expect("transaction request");

    assert!(
        resp.status().is_success(),
        "GET search in transaction should succeed"
    );

    let response_bundle: Value = resp.json().await.expect("parse response");
    let entries = response_bundle["entry"].as_array().expect("entries");
    let search_bundle = &entries[1]["resource"];

    assert_eq!(search_bundle["resourceType"], "Bundle");
    assert_eq!(search_bundle["type"], "searchset");
    assert_eq!(search_bundle["total"].as_u64().unwrap_or(0), 1);

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
        .post(&base)
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

/// A transaction Bundle with an invalid POST entry must be rejected with 422 +
/// OperationOutcome (same as a single POST create), and — being atomic — must NOT
/// commit the valid sibling. A Patient without `name` fails validation.
#[tokio::test]
async fn test_transaction_invalid_resource_returns_422_and_rolls_back() {
    let (_container, postgres_url) = start_postgres().await;
    let config = create_config(&postgres_url);
    let (base, shutdown_tx, _handle) = start_server(&config).await;
    let client = reqwest::Client::new();

    let bundle = json!({
        "resourceType": "Bundle",
        "type": "transaction",
        "entry": [
            {
                "fullUrl": "urn:uuid:valid-1",
                "resource": {"resourceType": "Patient", "name": [{"family": "TxValid"}]},
                "request": {"method": "POST", "url": "Patient"}
            },
            {
                // Invalid: Patient with no required name element.
                "fullUrl": "urn:uuid:invalid-1",
                "resource": {"resourceType": "Patient", "active": "not-a-boolean"},
                "request": {"method": "POST", "url": "Patient"}
            }
        ]
    });

    let resp = client
        .post(&base)
        .header("content-type", "application/fhir+json")
        .json(&bundle)
        .send()
        .await
        .expect("transaction request");

    assert_eq!(
        resp.status(),
        reqwest::StatusCode::UNPROCESSABLE_ENTITY,
        "invalid resource in a transaction Bundle must return 422"
    );
    let body: Value = resp.json().await.expect("parse response");
    assert_eq!(
        body["resourceType"], "OperationOutcome",
        "422 body should be an OperationOutcome"
    );

    // Atomic rollback: the valid sibling must not have been committed.
    let search = client
        .get(format!("{base}/Patient?family=TxValid"))
        .header("accept", "application/fhir+json")
        .send()
        .await
        .expect("search request");
    let sb: Value = search.json().await.expect("parse search");
    assert_eq!(
        sb["total"].as_u64().unwrap_or(0),
        0,
        "transaction must roll back; the valid sibling must not persist"
    );

    let _ = shutdown_tx.send(());
}

/// A batch Bundle is non-atomic: an invalid POST entry returns a per-entry 422
/// OperationOutcome while valid siblings still persist.
#[tokio::test]
async fn test_batch_invalid_resource_per_entry_422_siblings_persist() {
    let (_container, postgres_url) = start_postgres().await;
    let config = create_config(&postgres_url);
    let (base, shutdown_tx, _handle) = start_server(&config).await;
    let client = reqwest::Client::new();

    let bundle = json!({
        "resourceType": "Bundle",
        "type": "batch",
        "entry": [
            {
                "resource": {"resourceType": "Patient", "name": [{"family": "BatchValid"}]},
                "request": {"method": "POST", "url": "Patient"}
            },
            {
                // Invalid: Patient with no required name element.
                "resource": {"resourceType": "Patient", "active": "not-a-boolean"},
                "request": {"method": "POST", "url": "Patient"}
            }
        ]
    });

    let resp = client
        .post(&base)
        .header("content-type", "application/fhir+json")
        .json(&bundle)
        .send()
        .await
        .expect("batch request");

    let response_bundle: Value = resp.json().await.expect("parse response");
    assert_eq!(response_bundle["type"], "batch-response");
    let entries = response_bundle["entry"].as_array().expect("entries");
    assert_eq!(entries.len(), 2);

    let first_status = entries[0]["response"]["status"].as_str().unwrap_or("");
    assert!(
        first_status.starts_with("201"),
        "valid batch entry should be created, got {first_status}"
    );
    let second_status = entries[1]["response"]["status"].as_str().unwrap_or("");
    assert!(
        second_status.starts_with("422"),
        "invalid batch entry should return 422, got {second_status}"
    );
    assert_eq!(
        entries[1]["response"]["outcome"]["resourceType"], "OperationOutcome",
        "invalid batch entry should carry an OperationOutcome"
    );

    // Valid sibling persisted despite the invalid one failing.
    let search = client
        .get(format!("{base}/Patient?family=BatchValid"))
        .header("accept", "application/fhir+json")
        .send()
        .await
        .expect("search request");
    let sb: Value = search.json().await.expect("parse search");
    assert!(
        sb["total"].as_u64().unwrap_or(0) > 0,
        "valid batch sibling must persist"
    );

    let _ = shutdown_tx.send(());
}
