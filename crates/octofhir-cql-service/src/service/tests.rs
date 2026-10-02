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

    async fn history_raw(
        &self,
        _resource_type: &str,
        _id: Option<&str>,
        _params: &octofhir_storage::HistoryParams,
    ) -> Result<octofhir_storage::RawHistoryResult, octofhir_storage::StorageError> {
        Ok(octofhir_storage::RawHistoryResult {
            entries: vec![],
            total: Some(0),
        })
    }

    async fn system_history_raw(
        &self,
        _params: &octofhir_storage::HistoryParams,
    ) -> Result<octofhir_storage::RawHistoryResult, octofhir_storage::StorageError> {
        Ok(octofhir_storage::RawHistoryResult {
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
    create_test_service_with_search(None, 10000).await
}

async fn create_test_service_with_search(
    search: Option<Arc<dyn crate::data_provider::ResourceSearch>>,
    limit: usize,
) -> CqlService {
    let storage: octofhir_storage::DynStorage = Arc::new(TestStorage);

    // Create minimal FhirPathEngine for tests
    let model_provider = Arc::new(octofhir_fhir_model::EmptyModelProvider);
    let registry = Arc::new(octofhir_fhirpath::create_function_registry());
    let fhirpath_engine = Arc::new(
        octofhir_fhirpath::FhirPathEngine::new(registry, model_provider.clone())
            .await
            .unwrap(),
    );

    let data_provider = Arc::new(match search {
        Some(search) => FhirServerDataProvider::with_search(search, fhirpath_engine, limit),
        None => FhirServerDataProvider::new(storage.clone(), fhirpath_engine, limit),
    });
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
async fn test_evaluate_library_source_multiple_defines() {
    let service = create_test_service().await;

    let source = r#"library Playground version '1.0.0'

define Sum: 1 + 2 + 3
define Product: 6 * 7
define Comparison: 5 > 3"#;

    let result = service
        .evaluate_library_source(source, None, None, HashMap::new())
        .await;

    assert!(result.is_ok(), "Failed to evaluate: {:?}", result.err());
    let defines = result.unwrap();

    // Every define is returned, in source order.
    assert_eq!(
        defines.keys().collect::<Vec<_>>(),
        vec!["Sum", "Product", "Comparison"]
    );
    assert_eq!(defines.get("Sum"), Some(&json!(6)));
    assert_eq!(defines.get("Product"), Some(&json!(42)));
    assert_eq!(defines.get("Comparison"), Some(&json!(true)));
}

#[tokio::test]
async fn test_evaluate_library_source_empty_fails() {
    let service = create_test_service().await;

    let result = service
        .evaluate_library_source("   ", None, None, HashMap::new())
        .await;

    assert!(matches!(result, Err(CqlError::InvalidParameter(_))));
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

struct ScriptedSearch {
    pages: std::sync::Mutex<std::collections::VecDeque<CqlResult<octofhir_storage::SearchResult>>>,
    calls: std::sync::Mutex<Vec<(String, octofhir_storage::SearchParams)>>,
}

#[async_trait::async_trait]
impl crate::data_provider::ResourceSearch for ScriptedSearch {
    async fn search(
        &self,
        resource_type: &str,
        params: &octofhir_storage::SearchParams,
    ) -> CqlResult<octofhir_storage::SearchResult> {
        self.calls
            .lock()
            .unwrap()
            .push((resource_type.into(), params.clone()));
        self.pages
            .lock()
            .unwrap()
            .pop_front()
            .expect("unexpected retrieve")
    }
}

fn page(id: &str, has_more: bool) -> CqlResult<octofhir_storage::SearchResult> {
    Ok(octofhir_storage::SearchResult {
        entries: vec![octofhir_storage::StoredResource {
            id: id.into(),
            version_id: "1".into(),
            resource_type: "Observation".into(),
            resource: json!({"resourceType":"Observation", "id":id, "status":"final"}),
            last_updated: time::OffsetDateTime::now_utc(),
            created_at: time::OffsetDateTime::now_utc(),
        }],
        total: None,
        has_more,
    })
}

fn scripted(pages: Vec<CqlResult<octofhir_storage::SearchResult>>) -> Arc<ScriptedSearch> {
    Arc::new(ScriptedSearch {
        pages: std::sync::Mutex::new(pages.into()),
        calls: std::sync::Mutex::new(Vec::new()),
    })
}

// Default Tokio test runtime is single-threaded. These tests actually retrieve
// data, unlike arithmetic-only tests that never exercise synchronous callbacks.
#[tokio::test]
async fn retrieves_all_pages_on_single_thread_runtime() {
    let search = scripted(vec![page("first", true), page("second", false)]);
    let service = create_test_service_with_search(Some(search.clone()), 3).await;
    let result = service
        .evaluate_expression("[Observation]", None, None, HashMap::new())
        .await
        .unwrap();
    assert_eq!(result.as_array().unwrap().len(), 2);
    assert_eq!(result[0]["id"], "first");
    assert_eq!(result[1]["id"], "second");
    let calls = search.calls.lock().unwrap();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].0, "Observation");
    assert_eq!(calls[1].1.offset, Some(1));
    assert_eq!(calls[1].1.count, Some(2));
}

#[tokio::test]
async fn retrieve_failure_rejects_entire_evaluation_and_next_request_is_independent() {
    let search = scripted(vec![
        page("partial", true),
        Err(CqlError::ResourceAccess(Box::new(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "resource policy denied",
        )))),
        page("next-request", false),
    ]);
    let service = create_test_service_with_search(Some(search.clone()), 10).await;
    let error = service
        .evaluate_expression("[Observation]", None, None, HashMap::new())
        .await
        .unwrap_err();
    match error {
        CqlError::ResourceAccess(error) => assert_eq!(
            error.downcast::<std::io::Error>().unwrap().kind(),
            std::io::ErrorKind::PermissionDenied
        ),
        other => panic!("Access error was lost: {other:?}"),
    }
    let result = service
        .evaluate_expression("[Observation]", None, None, HashMap::new())
        .await
        .unwrap();
    assert_eq!(result[0]["id"], "next-request");
    assert_eq!(search.calls.lock().unwrap().len(), 3);
}

#[tokio::test]
async fn retrieve_limits_and_no_progress_are_errors() {
    for (pages, limit) in [
        (vec![page("overflow", true)], 1),
        (
            vec![Ok(octofhir_storage::SearchResult {
                entries: vec![],
                total: None,
                has_more: true,
            })],
            10,
        ),
        (
            vec![
                page("partial", true),
                Err(CqlError::DataProviderError("SQL failed".into())),
            ],
            10,
        ),
    ] {
        let service = create_test_service_with_search(Some(scripted(pages)), limit).await;
        assert!(matches!(
            service
                .evaluate_expression("Count([Observation])", None, None, HashMap::new())
                .await,
            Err(CqlError::DataProviderError(_))
        ));
    }
}

#[tokio::test]
async fn patient_context_resource_and_patient_id_criteria_are_retained() {
    for (resource_type, expected_key, expected_value) in [
        ("Observation", "subject", "Patient/patient-1"),
        ("Patient", "_id", "patient-1"),
    ] {
        let search = scripted(vec![page("match", false)]);
        let service = create_test_service_with_search(Some(search.clone()), 10).await;
        service
            .evaluate_expression(
                &format!("[{resource_type}]"),
                Some("Patient"),
                Some(json!({"resourceType":"Patient", "id":"patient-1"})),
                HashMap::new(),
            )
            .await
            .unwrap();
        let calls = search.calls.lock().unwrap();
        assert_eq!(
            calls[0].1.parameters.get(expected_key),
            Some(&vec![expected_value.into()])
        );
    }
}

struct SlowSearch;

#[async_trait::async_trait]
impl crate::data_provider::ResourceSearch for SlowSearch {
    async fn search(
        &self,
        _resource_type: &str,
        _params: &octofhir_storage::SearchParams,
    ) -> CqlResult<octofhir_storage::SearchResult> {
        tokio::time::sleep(Duration::from_secs(5)).await;
        page("too-late", false)
    }
}

#[tokio::test]
async fn async_retrieve_obeys_evaluation_deadline() {
    let mut service = create_test_service_with_search(Some(Arc::new(SlowSearch)), 10).await;
    service.config.evaluation_timeout_ms = 50;
    let result = tokio::time::timeout(
        Duration::from_secs(1),
        service.evaluate_expression("[Observation]", None, None, HashMap::new()),
    )
    .await
    .expect("evaluation blocked the runtime");
    assert!(matches!(result, Err(CqlError::Timeout(_))));
}
