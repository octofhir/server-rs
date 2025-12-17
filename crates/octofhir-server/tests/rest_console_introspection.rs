//! Integration tests for REST console introspection endpoint.
//!
//! These tests verify the REST console metadata generation.

use std::sync::Arc;

use axum::{
    extract::State,
    http::{HeaderValue, StatusCode},
    response::IntoResponse,
};
use octofhir_search::{
    parameters::{SearchModifier, SearchParameter, SearchParameterType},
    registry::SearchParameterRegistry,
};
use octofhir_server::operations::definition::{OperationDefinition, OperationKind};
use octofhir_server::operations::registry::OperationRegistry;
use octofhir_server::rest_console::{self, RestConsoleState};
use octofhir_storage::{
    DynStorage, FhirStorage, HistoryParams, HistoryResult, SearchParams, SearchResult,
    StorageError, StoredResource, Transaction,
};
use sqlx_postgres::PgPool;

// =============================================================================
// Mock Storage
// =============================================================================

/// Mock storage for testing - returns empty results for all operations.
#[derive(Clone)]
struct MockStorage;

#[async_trait::async_trait]
impl FhirStorage for MockStorage {
    async fn create(&self, _resource: &serde_json::Value) -> Result<StoredResource, StorageError> {
        Err(StorageError::internal("create not supported in mock"))
    }

    async fn read(
        &self,
        _resource_type: &str,
        _id: &str,
    ) -> Result<Option<StoredResource>, StorageError> {
        Ok(None)
    }

    async fn update(
        &self,
        _resource: &serde_json::Value,
        _if_match: Option<&str>,
    ) -> Result<StoredResource, StorageError> {
        Err(StorageError::internal("update not supported in mock"))
    }

    async fn delete(&self, _resource_type: &str, _id: &str) -> Result<(), StorageError> {
        Err(StorageError::internal("delete not supported in mock"))
    }

    async fn vread(
        &self,
        _resource_type: &str,
        _id: &str,
        _version: &str,
    ) -> Result<Option<StoredResource>, StorageError> {
        Ok(None)
    }

    async fn history(
        &self,
        _resource_type: &str,
        _id: Option<&str>,
        _params: &HistoryParams,
    ) -> Result<HistoryResult, StorageError> {
        Ok(HistoryResult {
            entries: vec![],
            total: Some(0),
        })
    }

    async fn system_history(&self, _params: &HistoryParams) -> Result<HistoryResult, StorageError> {
        Ok(HistoryResult {
            entries: vec![],
            total: Some(0),
        })
    }

    async fn search(
        &self,
        _resource_type: &str,
        _params: &SearchParams,
    ) -> Result<SearchResult, StorageError> {
        // Return empty results - no Apps or CustomOperations
        Ok(SearchResult {
            entries: vec![],
            total: Some(0),
            has_more: false,
        })
    }

    async fn begin_transaction(&self) -> Result<Box<dyn Transaction>, StorageError> {
        Err(StorageError::internal("transactions not supported in mock"))
    }

    fn supports_transactions(&self) -> bool {
        false
    }

    fn backend_name(&self) -> &'static str {
        "mock"
    }
}

// =============================================================================
// Tests
// =============================================================================

/// Get the test database URL from environment or use default.
fn get_test_db_url() -> String {
    std::env::var("TEST_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5450/octofhir".to_string())
}

/// Create a test database pool.
async fn create_test_pool() -> Option<Arc<PgPool>> {
    let db_url = get_test_db_url();
    match PgPool::connect(&db_url).await {
        Ok(pool) => Some(Arc::new(pool)),
        Err(e) => {
            eprintln!("Warning: Could not connect to test database: {}", e);
            None
        }
    }
}

#[tokio::test]
async fn rest_console_introspection_exposes_metadata() {
    // Create test database pool
    let db_pool = match create_test_pool().await {
        Some(pool) => pool,
        None => {
            eprintln!("Skipping test: database not available");
            return;
        }
    };

    let state = test_state(db_pool);

    // Test the introspect handler
    let response = rest_console::introspect(State(state.clone()))
        .await
        .into_response();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response.headers().contains_key("etag"));
    assert_eq!(
        response.headers().get("cache-control"),
        Some(&HeaderValue::from_static("public, max-age=60"))
    );

    // Test build_payload directly
    let payload = serde_json::to_value(rest_console::build_payload(&state).await).unwrap();
    assert_eq!(payload["base_path"], "/fhir");
    assert_eq!(payload["fhir_version"], "R4");

    // Check suggestions structure exists
    assert!(payload["suggestions"].is_object());
    assert!(payload["suggestions"]["resources"].is_array());
    assert!(payload["suggestions"]["system_operations"].is_array());
    assert!(payload["suggestions"]["type_operations"].is_array());
    assert!(payload["suggestions"]["instance_operations"].is_array());
    assert!(payload["suggestions"]["api_endpoints"].is_array());

    // Check search_params structure exists
    assert!(payload["search_params"].is_object());

    // Check Patient search params exist
    let patient_params = &payload["search_params"]["Patient"];
    assert!(patient_params.is_array());
    assert!(!patient_params.as_array().unwrap().is_empty());

    // Check first param has expected structure
    let first_param = &patient_params[0];
    assert!(first_param["code"].is_string());
    assert!(first_param["type"].is_string());
    assert!(first_param["modifiers"].is_array());
    assert!(first_param["comparators"].is_array());
}

#[tokio::test]
async fn rest_console_payload_contains_search_param_details() {
    let db_pool = match create_test_pool().await {
        Some(pool) => pool,
        None => {
            eprintln!("Skipping test: database not available");
            return;
        }
    };

    let state = test_state(db_pool);
    let payload = serde_json::to_value(rest_console::build_payload(&state).await).unwrap();

    // Check the Patient "name" search param has modifiers and comparators
    let patient_params = payload["search_params"]["Patient"].as_array().unwrap();
    let name_param = patient_params
        .iter()
        .find(|p| p["code"] == "name")
        .expect("Patient should have 'name' search param");

    assert!(
        name_param["modifiers"]
            .as_array()
            .map(|a| !a.is_empty())
            .unwrap_or(false),
        "name param should have modifiers"
    );
    assert!(
        name_param["comparators"]
            .as_array()
            .map(|a| !a.is_empty())
            .unwrap_or(false),
        "name param should have comparators"
    );
}

/// Create test state with search parameters and operations.
fn test_state(db_pool: Arc<PgPool>) -> RestConsoleState {
    let mut registry = SearchParameterRegistry::new();

    // Register Patient name search param
    let name_param = SearchParameter::new(
        "name",
        "http://hl7.org/fhir/SearchParameter/Patient-name",
        SearchParameterType::String,
        vec!["Patient".to_string()],
    )
    .with_expression("Patient.name")
    .with_description("A patient's name")
    .with_modifiers(vec![SearchModifier::Exact, SearchModifier::Contains])
    .with_comparators(vec!["eq".to_string()]);
    registry.register(name_param);

    // Register Patient identifier search param
    let id_param = SearchParameter::new(
        "identifier",
        "http://hl7.org/fhir/SearchParameter/Patient-identifier",
        SearchParameterType::Token,
        vec!["Patient".to_string()],
    )
    .with_expression("Patient.identifier")
    .with_description("A patient identifier")
    .with_modifiers(vec![SearchModifier::Exact])
    .with_comparators(vec!["eq".to_string()]);
    registry.register(id_param);

    // Register operation
    let mut operation_registry = OperationRegistry::new();
    operation_registry.register(OperationDefinition {
        code: "validate".to_string(),
        url: "http://hl7.org/fhir/OperationDefinition/validate".to_string(),
        kind: OperationKind::Operation,
        system: true,
        type_level: true,
        instance: true,
        resource: vec!["Patient".to_string()],
        parameters: vec![],
        affects_state: false,
    });

    // Create mock storage for Gateway operations
    let storage: DynStorage = Arc::new(MockStorage);

    RestConsoleState::new(
        Arc::new(registry),
        Arc::new(operation_registry),
        "R4".to_string(),
        db_pool,
        storage,
    )
}

/// Test that search parameter registry correctly stores and retrieves parameters.
#[test]
fn test_search_parameter_registry() {
    let mut registry = SearchParameterRegistry::new();
    let param = SearchParameter::new(
        "name",
        "http://hl7.org/fhir/SearchParameter/Patient-name",
        SearchParameterType::String,
        vec!["Patient".to_string()],
    )
    .with_expression("Patient.name")
    .with_description("A patient's name")
    .with_modifiers(vec![SearchModifier::Exact])
    .with_comparators(vec!["eq".to_string()]);
    registry.register(param);

    let params = registry.get_all_for_type("Patient");
    assert_eq!(params.len(), 1);
    assert_eq!(params[0].code, "name");
}

/// Test that operation registry correctly stores and retrieves operations.
#[test]
fn test_operation_registry() {
    let mut operation_registry = OperationRegistry::new();
    operation_registry.register(OperationDefinition {
        code: "validate".to_string(),
        url: "http://hl7.org/fhir/OperationDefinition/validate".to_string(),
        kind: OperationKind::Operation,
        system: true,
        type_level: true,
        instance: true,
        resource: vec!["Patient".to_string()],
        parameters: vec![],
        affects_state: false,
    });

    let ops = operation_registry.all();
    assert_eq!(ops.len(), 1);
    assert_eq!(ops[0].code, "validate");
}
