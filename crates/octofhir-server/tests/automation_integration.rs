//! Integration tests for the automation system.
//!
//! These tests spin up a PostgreSQL container and test:
//! - Automation CRUD operations
//! - Automation triggers (resource event)
//! - Automation execution
//! - Execution logging
//!
//! **Requirements:**
//! - Docker running
//! - FHIR packages installed in `.fhir/` directory
//!
//! Run with: cargo test -p octofhir-server --test automation_integration -- --ignored

use std::sync::Arc;

use octofhir_config::ConfigurationManager;
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

/// Create an AppConfig that uses the given PostgreSQL URL
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
    // Create a minimal ConfigurationManager for tests
    let config_manager = Arc::new(
        ConfigurationManager::builder()
            .build()
            .await
            .expect("build config manager"),
    );
    let app = build_app(config, config_manager).await.expect("build app");

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
#[ignore = "requires FHIR packages in .fhir/ directory and Docker"]
async fn test_automation_crud_operations() {
    let (_container, postgres_url) = start_postgres().await;
    let config = create_config(&postgres_url);
    let (base, shutdown_tx, _handle) = start_server(&config).await;
    let client = reqwest::Client::new();

    // Create an automation
    let create_payload = json!({
        "name": "Test Automation",
        "description": "A test automation for integration testing",
        "sourceCode": "console.log('Hello from automation!'); return { success: true };",
        "timeoutMs": 5000
    });

    let resp = client
        .post(format!("{base}/api/automations"))
        .header("content-type", "application/json")
        .json(&create_payload)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let created: Value = resp.json().await.unwrap();
    let automation_id = created["id"].as_str().expect("automation id").to_string();

    assert_eq!(created["name"], "Test Automation");
    assert_eq!(created["status"], "inactive");
    assert_eq!(created["version"], 1);

    // Read the automation
    let resp = client
        .get(format!("{base}/api/automations/{automation_id}"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let read: Value = resp.json().await.unwrap();
    assert_eq!(read["id"], automation_id);
    assert_eq!(read["name"], "Test Automation");

    // Update the automation
    let update_payload = json!({
        "name": "Updated Automation",
        "description": "Updated description"
    });

    let resp = client
        .put(format!("{base}/api/automations/{automation_id}"))
        .header("content-type", "application/json")
        .json(&update_payload)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let updated: Value = resp.json().await.unwrap();
    assert_eq!(updated["name"], "Updated Automation");
    assert_eq!(updated["version"], 2);

    // List all automations
    let resp = client.get(format!("{base}/api/automations")).send().await.unwrap();

    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let list: Value = resp.json().await.unwrap();
    assert!(list.as_array().unwrap().len() >= 1);

    // Delete the automation
    let resp = client
        .delete(format!("{base}/api/automations/{automation_id}"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), reqwest::StatusCode::NO_CONTENT);

    // Verify deletion
    let resp = client
        .get(format!("{base}/api/automations/{automation_id}"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), reqwest::StatusCode::NOT_FOUND);

    // Cleanup
    let _ = shutdown_tx.send(());
}

#[tokio::test]
#[ignore = "requires FHIR packages in .fhir/ directory and Docker"]
async fn test_automation_manual_execution() {
    let (_container, postgres_url) = start_postgres().await;
    let config = create_config(&postgres_url);
    let (base, shutdown_tx, _handle) = start_server(&config).await;
    let client = reqwest::Client::new();

    // Create an automation that returns the event data
    let create_payload = json!({
        "name": "Echo Automation",
        "description": "Returns the event data",
        "sourceCode": r#"
            console.log('Event type:', event.type);
            console.log('Resource:', JSON.stringify(event.resource));
            return {
                receivedType: event.type,
                resourceType: event.resource.resourceType,
                resourceId: event.resource.id
            };
        "#,
        "timeoutMs": 5000
    });

    let resp = client
        .post(format!("{base}/api/automations"))
        .header("content-type", "application/json")
        .json(&create_payload)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let created: Value = resp.json().await.unwrap();
    let automation_id = created["id"].as_str().expect("automation id").to_string();

    // Deploy the automation (activate it)
    let resp = client
        .post(format!("{base}/api/automations/{automation_id}/deploy"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), reqwest::StatusCode::OK);

    // Execute the automation manually
    let execute_payload = json!({
        "resource": {
            "resourceType": "Patient",
            "id": "test-123",
            "active": true
        }
    });

    let resp = client
        .post(format!("{base}/api/automations/{automation_id}/execute"))
        .header("content-type", "application/json")
        .json(&execute_payload)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let result: Value = resp.json().await.unwrap();

    // Check execution result
    assert_eq!(result["success"], true);
    assert!(result["executionId"].is_string());

    // The output should contain our returned data
    let output = &result["output"];
    assert_eq!(output["receivedType"], "manual");
    assert_eq!(output["resourceType"], "Patient");
    assert_eq!(output["resourceId"], "test-123");

    // Check execution logs
    let resp = client
        .get(format!("{base}/api/automations/{automation_id}/logs"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let logs: Value = resp.json().await.unwrap();
    let logs_array = logs.as_array().unwrap();
    assert!(logs_array.len() >= 1);

    // Most recent execution should be completed
    let latest_log = &logs_array[0];
    assert_eq!(latest_log["status"], "completed");

    // Cleanup
    let _ = shutdown_tx.send(());
}

#[tokio::test]
#[ignore = "requires FHIR packages in .fhir/ directory and Docker"]
async fn test_automation_trigger_configuration() {
    let (_container, postgres_url) = start_postgres().await;
    let config = create_config(&postgres_url);
    let (base, shutdown_tx, _handle) = start_server(&config).await;
    let client = reqwest::Client::new();

    // Create an automation
    let create_payload = json!({
        "name": "Triggered Automation",
        "sourceCode": "return { triggered: true };",
    });

    let resp = client
        .post(format!("{base}/api/automations"))
        .header("content-type", "application/json")
        .json(&create_payload)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let created: Value = resp.json().await.unwrap();
    let automation_id = created["id"].as_str().expect("automation id").to_string();

    // Add a resource event trigger
    let trigger_payload = json!({
        "triggerType": "resource_event",
        "resourceType": "Patient",
        "eventTypes": ["created", "updated"]
    });

    let resp = client
        .post(format!("{base}/api/automations/{automation_id}/triggers"))
        .header("content-type", "application/json")
        .json(&trigger_payload)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let trigger: Value = resp.json().await.unwrap();
    let trigger_id = trigger["id"].as_str().expect("trigger id").to_string();

    assert_eq!(trigger["triggerType"], "resource_event");
    assert_eq!(trigger["resourceType"], "Patient");

    // Get automation with triggers
    let resp = client
        .get(format!("{base}/api/automations/{automation_id}"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let automation_with_triggers: Value = resp.json().await.unwrap();
    let triggers = automation_with_triggers["triggers"].as_array().unwrap();
    assert_eq!(triggers.len(), 1);

    // Add a cron trigger
    let cron_trigger_payload = json!({
        "triggerType": "cron",
        "cronExpression": "0 * * * *"  // Every hour
    });

    let resp = client
        .post(format!("{base}/api/automations/{automation_id}/triggers"))
        .header("content-type", "application/json")
        .json(&cron_trigger_payload)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), reqwest::StatusCode::OK);

    // Delete the first trigger
    let resp = client
        .delete(format!("{base}/api/automations/{automation_id}/triggers/{trigger_id}"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), reqwest::StatusCode::NO_CONTENT);

    // Verify trigger was deleted
    let resp = client
        .get(format!("{base}/api/automations/{automation_id}"))
        .send()
        .await
        .unwrap();

    let automation_after_delete: Value = resp.json().await.unwrap();
    let triggers_after = automation_after_delete["triggers"].as_array().unwrap();
    assert_eq!(triggers_after.len(), 1);
    assert_eq!(triggers_after[0]["triggerType"], "cron");

    // Cleanup
    let _ = shutdown_tx.send(());
}

#[tokio::test]
#[ignore = "requires FHIR packages in .fhir/ directory and Docker"]
async fn test_automation_execution_error_handling() {
    let (_container, postgres_url) = start_postgres().await;
    let config = create_config(&postgres_url);
    let (base, shutdown_tx, _handle) = start_server(&config).await;
    let client = reqwest::Client::new();

    // Create an automation with a runtime error
    let create_payload = json!({
        "name": "Error Automation",
        "sourceCode": "throw new Error('Intentional error for testing');",
    });

    let resp = client
        .post(format!("{base}/api/automations"))
        .header("content-type", "application/json")
        .json(&create_payload)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let created: Value = resp.json().await.unwrap();
    let automation_id = created["id"].as_str().expect("automation id").to_string();

    // Deploy the automation
    let _ = client
        .post(format!("{base}/api/automations/{automation_id}/deploy"))
        .send()
        .await
        .unwrap();

    // Execute the automation - should capture the error
    let execute_payload = json!({
        "resource": { "resourceType": "Patient", "id": "123" }
    });

    let resp = client
        .post(format!("{base}/api/automations/{automation_id}/execute"))
        .header("content-type", "application/json")
        .json(&execute_payload)
        .send()
        .await
        .unwrap();

    // Execution endpoint should still return OK (execution was attempted)
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let result: Value = resp.json().await.unwrap();

    // But the execution should have failed
    assert_eq!(result["success"], false);
    assert!(result["error"].is_string());

    // Check execution logs show failure
    let resp = client
        .get(format!("{base}/api/automations/{automation_id}/logs"))
        .send()
        .await
        .unwrap();

    let logs: Value = resp.json().await.unwrap();
    let logs_array = logs.as_array().unwrap();
    assert!(!logs_array.is_empty());
    assert_eq!(logs_array[0]["status"], "failed");

    // Cleanup
    let _ = shutdown_tx.send(());
}

#[tokio::test]
#[ignore = "requires FHIR packages in .fhir/ directory and Docker"]
async fn test_automation_fhir_client_operations() {
    let (_container, postgres_url) = start_postgres().await;
    let config = create_config(&postgres_url);
    let (base, shutdown_tx, _handle) = start_server(&config).await;
    let client = reqwest::Client::new();

    // Create an automation that uses fhir.* operations
    let create_payload = json!({
        "name": "FHIR Client Automation",
        "sourceCode": r#"
            // Create a patient
            const patient = fhir.create({
                resourceType: 'Patient',
                active: true,
                name: [{ family: 'AutomationCreated', given: ['Test'] }]
            });
            console.log('Created patient:', patient.id);

            // Read it back
            const readPatient = fhir.read('Patient', patient.id);
            console.log('Read patient:', readPatient.id);

            // Update it
            readPatient.active = false;
            const updated = fhir.update(readPatient);

            return {
                createdId: patient.id,
                wasActive: patient.active,
                isNowActive: updated.active
            };
        "#,
        "timeoutMs": 10000
    });

    let resp = client
        .post(format!("{base}/api/automations"))
        .header("content-type", "application/json")
        .json(&create_payload)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let created: Value = resp.json().await.unwrap();
    let automation_id = created["id"].as_str().expect("automation id").to_string();

    // Deploy the automation
    let _ = client
        .post(format!("{base}/api/automations/{automation_id}/deploy"))
        .send()
        .await
        .unwrap();

    // Execute the automation
    let execute_payload = json!({
        "resource": { "resourceType": "Parameters" }
    });

    let resp = client
        .post(format!("{base}/api/automations/{automation_id}/execute"))
        .header("content-type", "application/json")
        .json(&execute_payload)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let result: Value = resp.json().await.unwrap();

    assert_eq!(result["success"], true);
    let output = &result["output"];
    assert!(output["createdId"].is_string());
    assert_eq!(output["wasActive"], true);
    assert_eq!(output["isNowActive"], false);

    // Verify the patient actually exists in the database
    let patient_id = output["createdId"].as_str().unwrap();
    let resp = client
        .get(format!("{base}/fhir/Patient/{patient_id}"))
        .header("accept", "application/fhir+json")
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let patient: Value = resp.json().await.unwrap();
    assert_eq!(patient["active"], false);

    // Cleanup
    let _ = shutdown_tx.send(());
}
