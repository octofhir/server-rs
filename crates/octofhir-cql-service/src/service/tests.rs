//! Unit tests for CQL service
//!
//! These tests verify basic CQL evaluation functionality.
//! Integration tests with actual FHIR storage are in the server crate.

use super::*;
use serde_json::json;
use std::collections::HashMap;

/// Simple mock storage for tests (minimal implementation)
struct TestStorage;

#[async_trait::async_trait]
impl octofhir_storage::FhirStorage for TestStorage {
    async fn create(
        &self,
        _resource: &serde_json::Value,
    ) -> Result<octofhir_storage::StoredResource, octofhir_storage::StorageError> {
        unimplemented!("Test storage: create not needed")
    }

    async fn read(
        &self,
        _resource_type: &str,
        _id: &str,
    ) -> Result<Option<octofhir_storage::StoredResource>, octofhir_storage::StorageError> {
        Ok(None)
    }

    async fn update(
        &self,
        _resource: &serde_json::Value,
        _if_match: Option<&str>,
    ) -> Result<octofhir_storage::StoredResource, octofhir_storage::StorageError> {
        unimplemented!("Test storage: update not needed")
    }

    async fn delete(
        &self,
        _resource_type: &str,
        _id: &str,
    ) -> Result<(), octofhir_storage::StorageError> {
        Ok(())
    }

    async fn vread(
        &self,
        _resource_type: &str,
        _id: &str,
        _version: &str,
    ) -> Result<Option<octofhir_storage::StoredResource>, octofhir_storage::StorageError> {
        Ok(None)
    }

    async fn search(
        &self,
        _resource_type: &str,
        _params: &octofhir_storage::SearchParams,
    ) -> Result<octofhir_storage::SearchResult, octofhir_storage::StorageError> {
        Ok(octofhir_storage::SearchResult {
            entries: vec![],
            total: Some(0),
            has_more: false,
        })
    }

    async fn history(
        &self,
        _resource_type: &str,
        _id: Option<&str>,
        _params: &octofhir_storage::HistoryParams,
    ) -> Result<octofhir_storage::HistoryResult, octofhir_storage::StorageError> {
        Ok(octofhir_storage::HistoryResult {
            entries: vec![],
            total: Some(0),
        })
    }

    async fn system_history(
        &self,
        _params: &octofhir_storage::HistoryParams,
    ) -> Result<octofhir_storage::HistoryResult, octofhir_storage::StorageError> {
        Ok(octofhir_storage::HistoryResult {
            entries: vec![],
            total: Some(0),
        })
    }

    async fn begin_transaction(
        &self,
    ) -> Result<Box<dyn octofhir_storage::Transaction>, octofhir_storage::StorageError> {
        Err(octofhir_storage::StorageError::transaction_error(
            "Test storage does not support transactions",
        ))
    }

    fn supports_transactions(&self) -> bool {
        false
    }

    fn backend_name(&self) -> &'static str {
        "test"
    }
}

/// Create a test CQL service with minimal dependencies
async fn create_test_service() -> CqlService {
    let storage: octofhir_storage::DynStorage = Arc::new(TestStorage);

    // Create minimal FhirPathEngine for tests
    let model_provider = Arc::new(octofhir_fhir_model::EmptyModelProvider);
    let registry = Arc::new(octofhir_fhirpath::create_function_registry());
    let fhirpath_engine = Arc::new(
        octofhir_fhirpath::FhirPathEngine::new(registry, model_provider.clone())
            .await
            .unwrap(),
    );

    let data_provider = Arc::new(FhirServerDataProvider::new(
        storage.clone(),
        fhirpath_engine,
        10000,
    ));
    let terminology_provider = Arc::new(CqlTerminologyProvider::new());
    let library_cache = Arc::new(LibraryCache::new(100));
    let config = CqlConfig::default();

    CqlService::new(
        data_provider,
        terminology_provider,
        library_cache,
        storage,
        config,
    )
}

#[tokio::test]
async fn test_evaluate_simple_arithmetic() {
    let service = create_test_service().await;

    let result = service
        .evaluate_expression("1 + 1", None, None, HashMap::new())
        .await;

    assert!(result.is_ok(), "Failed to evaluate: {:?}", result.err());
    let value = result.unwrap();
    assert_eq!(value, json!(2));
}

#[tokio::test]
async fn test_evaluate_empty_expression_fails() {
    let service = create_test_service().await;

    let result = service
        .evaluate_expression("", None, None, HashMap::new())
        .await;

    assert!(result.is_err());
    match result {
        Err(CqlError::InvalidParameter(_)) => (),
        _ => panic!("Expected InvalidParameter error, got: {:?}", result),
    }
}

#[tokio::test]
async fn test_evaluate_boolean_logic() {
    let service = create_test_service().await;

    let result = service
        .evaluate_expression("true and false", None, None, HashMap::new())
        .await;

    assert!(result.is_ok());
    let value = result.unwrap();
    assert_eq!(value, json!(false));
}

#[tokio::test]
async fn test_evaluate_string_concatenation() {
    let service = create_test_service().await;

    let result = service
        .evaluate_expression("'Hello' + ' ' + 'World'", None, None, HashMap::new())
        .await;

    assert!(result.is_ok());
    let value = result.unwrap();
    assert_eq!(value, json!("Hello World"));
}

#[tokio::test]
async fn test_evaluate_comparison() {
    let service = create_test_service().await;

    let result = service
        .evaluate_expression("5 > 3", None, None, HashMap::new())
        .await;

    assert!(result.is_ok());
    let value = result.unwrap();
    assert_eq!(value, json!(true));
}

#[tokio::test]
async fn test_evaluate_multiplication() {
    let service = create_test_service().await;

    let result = service
        .evaluate_expression("6 * 7", None, None, HashMap::new())
        .await;

    assert!(result.is_ok());
    let value = result.unwrap();
    assert_eq!(value, json!(42));
}

#[tokio::test]
async fn test_cache_stats() {
    let service = create_test_service().await;

    let stats = service.cache_stats();
    assert_eq!(stats.size, 0);
    assert_eq!(stats.capacity, 100);
}

#[tokio::test]
async fn test_clear_cache() {
    let service = create_test_service().await;

    // This should not panic
    service.clear_cache();

    let stats = service.cache_stats();
    assert_eq!(stats.size, 0);
}

#[tokio::test]
async fn test_invalid_cql_syntax() {
    let service = create_test_service().await;

    let result = service
        .evaluate_expression("this is not valid CQL !!!", None, None, HashMap::new())
        .await;

    assert!(result.is_err());
    match result {
        Err(CqlError::ParseError(_)) => (),
        _ => panic!("Expected ParseError, got: {:?}", result),
    }
}
