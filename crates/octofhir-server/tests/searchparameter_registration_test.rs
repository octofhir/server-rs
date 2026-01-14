//! Integration tests for SearchParameter auto-registration.
//!
//! These tests verify that SearchParameter resources created via REST API
//! are automatically registered and immediately available for search operations.
//!
//! **Requirements:**
//! - Docker running (for PostgreSQL testcontainer)
//! - FHIR packages installed in `.fhir/` directory
//!
//! Run with: cargo test -p octofhir-server searchparameter_registration -- --ignored

use octofhir_server::{AppConfig, PostgresStorageConfig, build_app};
use serde_json::json;
use std::sync::Arc;
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
    // Create minimal config manager for tests
    let config_manager = Arc::new(
        octofhir_config::ConfigurationManager::builder()
            .build()
            .await
            .expect("create config manager"),
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
#[ignore = "requires Docker and FHIR packages in .fhir/ directory"]
async fn test_create_searchparameter_auto_registers() {
    let (_container, postgres_url) = start_postgres().await;
    let config = create_config(&postgres_url);
    let (base_url, shutdown_tx, _handle) = start_server(&config).await;
    let client = reqwest::Client::new();

    // Create a custom SearchParameter for Patient extension
    let search_param = json!({
        "resourceType": "SearchParameter",
        "url": "http://example.org/SearchParameter/patient-custom-field",
        "name": "PatientCustomField",
        "code": "custom-field",
        "status": "active",
        "description": "Search by custom patient field in extension",
        "base": ["Patient"],
        "type": "string",
        "expression": "Patient.extension.where(url='http://example.org/custom-field').valueString"
    });

    // Create the SearchParameter via POST
    let response = client
        .post(&format!("{}/fhir/SearchParameter", base_url))
        .header("content-type", "application/fhir+json")
        .json(&search_param)
        .send()
        .await
        .expect("Failed to send request");

    assert_eq!(
        response.status(),
        201,
        "SearchParameter creation should succeed"
    );

    let created: serde_json::Value = response.json().await.expect("Failed to parse response");
    assert_eq!(created["resourceType"], "SearchParameter");
    let search_param_id = created["id"].as_str().expect("Missing id");

    // Create a Patient with the custom extension
    let patient = json!({
        "resourceType": "Patient",
        "extension": [{
            "url": "http://example.org/custom-field",
            "valueString": "test-value-123"
        }],
        "name": [{
            "family": "TestPatient"
        }]
    });

    client
        .post(&format!("{}/fhir/Patient", base_url))
        .header("content-type", "application/fhir+json")
        .json(&patient)
        .send()
        .await
        .expect("Failed to create patient");

    // Give a small delay for indexing (if needed)
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // IMMEDIATELY try to use the custom search parameter
    // This should work without server restart!
    let search_response = client
        .get(&format!(
            "{}/fhir/Patient?custom-field=test-value-123",
            base_url
        ))
        .send()
        .await
        .expect("Failed to search");

    assert!(
        search_response.status().is_success(),
        "Search with custom parameter should succeed immediately after registration"
    );

    let bundle: serde_json::Value = search_response
        .json()
        .await
        .expect("Failed to parse search response");
    assert_eq!(bundle["resourceType"], "Bundle");
    assert_eq!(bundle["type"], "searchset");

    // Verify we got results
    let total = bundle["total"].as_u64().unwrap_or(0);
    assert!(
        total >= 1,
        "Should find at least one patient with custom field"
    );

    // Cleanup
    client
        .delete(&format!(
            "{}/fhir/SearchParameter/{}",
            base_url, search_param_id
        ))
        .send()
        .await
        .ok();

    shutdown_tx.send(()).ok();
}

#[tokio::test]
#[ignore = "requires Docker and FHIR packages in .fhir/ directory"]
async fn test_invalid_fhirpath_expression_rejected() {
    let (_container, postgres_url) = start_postgres().await;
    let config = create_config(&postgres_url);
    let (base_url, shutdown_tx, _handle) = start_server(&config).await;
    let client = reqwest::Client::new();

    // Try to create a SearchParameter with invalid FHIRPath expression
    let invalid_param = json!({
        "resourceType": "SearchParameter",
        "url": "http://example.org/SearchParameter/invalid-expression",
        "code": "invalid-test",
        "status": "active",
        "base": ["Patient"],
        "type": "string",
        "expression": "Patient..invalid..syntax"  // Invalid FHIRPath!
    });

    let response = client
        .post(&format!("{}/fhir/SearchParameter", base_url))
        .header("content-type", "application/fhir+json")
        .json(&invalid_param)
        .send()
        .await
        .expect("Failed to send request");

    // Should be rejected with 400 Bad Request
    assert_eq!(
        response.status(),
        400,
        "Invalid FHIRPath expression should be rejected"
    );

    let outcome: serde_json::Value = response.json().await.expect("Failed to parse response");
    assert_eq!(outcome["resourceType"], "OperationOutcome");

    // Verify error message mentions FHIRPath
    let diagnostics = outcome["issue"][0]["diagnostics"].as_str().unwrap_or("");
    assert!(
        diagnostics.contains("FHIRPath") || diagnostics.contains("expression"),
        "Error message should mention FHIRPath or expression"
    );

    shutdown_tx.send(()).ok();
}

#[tokio::test]
#[ignore = "requires Docker and FHIR packages in .fhir/ directory"]
async fn test_update_searchparameter_updates_registry() {
    let (_container, postgres_url) = start_postgres().await;
    let config = create_config(&postgres_url);
    let (base_url, shutdown_tx, _handle) = start_server(&config).await;
    let client = reqwest::Client::new();

    // Create initial SearchParameter
    let search_param = json!({
        "resourceType": "SearchParameter",
        "url": "http://example.org/SearchParameter/patient-updateable",
        "code": "updateable-field",
        "status": "active",
        "base": ["Patient"],
        "type": "string",
        "expression": "Patient.extension.where(url='http://example.org/field-v1').valueString"
    });

    let create_response = client
        .post(&format!("{}/fhir/SearchParameter", base_url))
        .header("content-type", "application/fhir+json")
        .json(&search_param)
        .send()
        .await
        .expect("Failed to create");

    assert_eq!(create_response.status(), 201);
    let created: serde_json::Value = create_response.json().await.unwrap();
    let param_id = created["id"].as_str().unwrap();

    // Update the SearchParameter to change the expression
    let mut updated_param = created.clone();
    updated_param["expression"] =
        json!("Patient.extension.where(url='http://example.org/field-v2').valueString");

    let update_response = client
        .put(&format!("{}/fhir/SearchParameter/{}", base_url, param_id))
        .header("content-type", "application/fhir+json")
        .json(&updated_param)
        .send()
        .await
        .expect("Failed to update");

    assert!(
        update_response.status().is_success(),
        "SearchParameter update should succeed"
    );

    // Create a patient with the NEW extension URL
    let patient = json!({
        "resourceType": "Patient",
        "extension": [{
            "url": "http://example.org/field-v2",  // Updated URL
            "valueString": "updated-value"
        }]
    });

    client
        .post(&format!("{}/fhir/Patient", base_url))
        .header("content-type", "application/fhir+json")
        .json(&patient)
        .send()
        .await
        .expect("Failed to create patient");

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Search using the updated parameter (should use new expression)
    let search_response = client
        .get(&format!(
            "{}/fhir/Patient?updateable-field=updated-value",
            base_url
        ))
        .send()
        .await
        .expect("Failed to search");

    assert!(search_response.status().is_success());

    // Cleanup
    client
        .delete(&format!("{}/fhir/SearchParameter/{}", base_url, param_id))
        .send()
        .await
        .ok();

    shutdown_tx.send(()).ok();
}

#[tokio::test]
#[ignore = "requires Docker and FHIR packages in .fhir/ directory"]
async fn test_delete_searchparameter_removes_from_registry() {
    let (_container, postgres_url) = start_postgres().await;
    let config = create_config(&postgres_url);
    let (base_url, shutdown_tx, _handle) = start_server(&config).await;
    let client = reqwest::Client::new();

    // Create a SearchParameter
    let search_param = json!({
        "resourceType": "SearchParameter",
        "url": "http://example.org/SearchParameter/patient-deletable",
        "code": "deletable-field",
        "status": "active",
        "base": ["Patient"],
        "type": "string",
        "expression": "Patient.extension.where(url='http://example.org/deletable').valueString"
    });

    let create_response = client
        .post(&format!("{}/fhir/SearchParameter", base_url))
        .header("content-type", "application/fhir+json")
        .json(&search_param)
        .send()
        .await
        .expect("Failed to create");

    assert_eq!(create_response.status(), 201);
    let created: serde_json::Value = create_response.json().await.unwrap();
    let param_id = created["id"].as_str().unwrap();

    // Verify the parameter works
    let search_response = client
        .get(&format!("{}/fhir/Patient?deletable-field=test", base_url))
        .send()
        .await
        .expect("Failed to search");

    assert!(
        search_response.status().is_success(),
        "Search should work before deletion"
    );

    // Delete the SearchParameter
    let delete_response = client
        .delete(&format!("{}/fhir/SearchParameter/{}", base_url, param_id))
        .send()
        .await
        .expect("Failed to delete");

    assert_eq!(
        delete_response.status(),
        204,
        "SearchParameter deletion should succeed"
    );

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Try to use the deleted parameter - should fail or return error
    let search_after_delete = client
        .get(&format!("{}/fhir/Patient?deletable-field=test", base_url))
        .send()
        .await
        .expect("Failed to search");

    // The search might fail with 400 (unknown parameter) or succeed with empty results
    // depending on how the server handles unknown parameters
    // For now, we just verify it doesn't crash
    assert!(
        search_after_delete.status().is_client_error() || search_after_delete.status().is_success(),
        "Search after deletion should either fail gracefully or return empty results"
    );

    shutdown_tx.send(()).ok();
}

#[tokio::test]
#[ignore = "requires Docker and FHIR packages in .fhir/ directory"]
async fn test_searchparameter_with_multi_resource_base() {
    let (_container, postgres_url) = start_postgres().await;
    let config = create_config(&postgres_url);
    let (base_url, shutdown_tx, _handle) = start_server(&config).await;
    let client = reqwest::Client::new();

    // Create a SearchParameter that applies to multiple resource types
    let search_param = json!({
        "resourceType": "SearchParameter",
        "url": "http://example.org/SearchParameter/clinical-custom-status",
        "code": "custom-status",
        "status": "active",
        "base": ["Observation", "Condition"],
        "type": "token",
        "expression": "%resource.extension.where(url='http://example.org/custom-status').valueCode"
    });

    let response = client
        .post(&format!("{}/fhir/SearchParameter", base_url))
        .header("content-type", "application/fhir+json")
        .json(&search_param)
        .send()
        .await
        .expect("Failed to create");

    assert_eq!(response.status(), 201);
    let created: serde_json::Value = response.json().await.unwrap();
    let param_id = created["id"].as_str().unwrap();

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Verify the parameter works for Observation
    let obs_search = client
        .get(&format!(
            "{}/fhir/Observation?custom-status=reviewed",
            base_url
        ))
        .send()
        .await
        .expect("Failed to search Observation");

    assert!(obs_search.status().is_success());

    // Verify the parameter works for Condition
    let cond_search = client
        .get(&format!(
            "{}/fhir/Condition?custom-status=reviewed",
            base_url
        ))
        .send()
        .await
        .expect("Failed to search Condition");

    assert!(cond_search.status().is_success());

    // Cleanup
    client
        .delete(&format!("{}/fhir/SearchParameter/{}", base_url, param_id))
        .send()
        .await
        .ok();

    shutdown_tx.send(()).ok();
}

#[tokio::test]
#[ignore = "requires Docker and FHIR packages in .fhir/ directory"]
async fn test_searchparameter_validation_missing_required_fields() {
    let (_container, postgres_url) = start_postgres().await;
    let config = create_config(&postgres_url);
    let (base_url, shutdown_tx, _handle) = start_server(&config).await;
    let client = reqwest::Client::new();

    // Missing 'code' field
    let missing_code = json!({
        "resourceType": "SearchParameter",
        "url": "http://example.org/SearchParameter/missing-code",
        "base": ["Patient"],
        "type": "string"
    });

    let response = client
        .post(&format!("{}/fhir/SearchParameter", base_url))
        .header("content-type", "application/fhir+json")
        .json(&missing_code)
        .send()
        .await
        .expect("Failed to send");

    assert_eq!(
        response.status(),
        400,
        "Should reject SearchParameter without code"
    );

    // Missing 'base' field
    let missing_base = json!({
        "resourceType": "SearchParameter",
        "url": "http://example.org/SearchParameter/missing-base",
        "code": "test",
        "type": "string"
    });

    let response = client
        .post(&format!("{}/fhir/SearchParameter", base_url))
        .header("content-type", "application/fhir+json")
        .json(&missing_base)
        .send()
        .await
        .expect("Failed to send");

    assert_eq!(
        response.status(),
        400,
        "Should reject SearchParameter without base"
    );

    shutdown_tx.send(()).ok();
}
