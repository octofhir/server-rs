//! Integration tests for GraphQL query implementation.
//!
//! These tests verify the complete query flow from GraphQL schema
//! to storage and back.

use std::sync::Arc;

use async_graphql::dynamic::Schema;
use octofhir_auth::policy::cache::PolicyCache;
use octofhir_auth::policy::engine::{PolicyEvaluator, PolicyEvaluatorConfig};
use octofhir_auth::storage::PolicyStorage;
use octofhir_auth::AuthResult;
use octofhir_graphql::{
    FhirSchemaBuilder, GraphQLContext, GraphQLContextBuilder, SchemaBuilderConfig,
};
use octofhir_search::{SearchConfig, SearchParameter, SearchParameterRegistry, SearchParameterType};
use octofhir_storage::{
    DynStorage, FhirStorage, HistoryParams, HistoryResult, SearchParams, SearchResult,
    StorageError, StoredResource, Transaction,
};
use serde_json::json;
use time::Duration;

// =============================================================================
// Mock Storage
// =============================================================================

/// Mock storage for testing.
#[derive(Clone)]
struct MockStorage {
    resources: Vec<StoredResource>,
}

impl MockStorage {
    fn new() -> Self {
        Self {
            resources: Vec::new(),
        }
    }

    fn with_resources(resources: Vec<StoredResource>) -> Self {
        Self { resources }
    }
}

#[async_trait::async_trait]
impl FhirStorage for MockStorage {
    async fn create(&self, _resource: &serde_json::Value) -> Result<StoredResource, StorageError> {
        Err(StorageError::internal("create not supported in mock"))
    }

    async fn read(
        &self,
        resource_type: &str,
        id: &str,
    ) -> Result<Option<StoredResource>, StorageError> {
        let found = self.resources.iter().find(|r| {
            r.resource.get("resourceType") == Some(&json!(resource_type))
                && r.resource.get("id") == Some(&json!(id))
        });
        Ok(found.cloned())
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
        Err(StorageError::internal("vread not supported in mock"))
    }

    async fn history(
        &self,
        _resource_type: &str,
        _id: Option<&str>,
        _params: &HistoryParams,
    ) -> Result<HistoryResult, StorageError> {
        Err(StorageError::internal("history not supported in mock"))
    }

    async fn search(
        &self,
        resource_type: &str,
        params: &SearchParams,
    ) -> Result<SearchResult, StorageError> {
        // Filter by resource type
        let mut results: Vec<StoredResource> = self
            .resources
            .iter()
            .filter(|r| r.resource.get("resourceType") == Some(&json!(resource_type)))
            .cloned()
            .collect();

        // Apply simple filtering based on params
        for (key, values) in params.parameters.iter() {
            if key.starts_with('_') {
                continue;
            }
            results.retain(|r| {
                let field_value = r.resource.get(key);
                values.iter().any(|v| {
                    if let Some(fv) = field_value {
                        fv.as_str() == Some(v.as_str())
                    } else {
                        false
                    }
                })
            });
        }

        // Apply pagination
        let total = results.len() as u32;
        let offset = params.offset.unwrap_or(0) as usize;
        let count = params.count.unwrap_or(100) as usize;

        let entries: Vec<StoredResource> = results.into_iter().skip(offset).take(count).collect();
        let has_more = offset + entries.len() < total as usize;

        Ok(SearchResult {
            entries,
            total: Some(total),
            has_more,
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
// Mock Policy Storage
// =============================================================================

use octofhir_auth::policy::resources::AccessPolicy;
use octofhir_auth::smart::scopes::FhirOperation;
use octofhir_auth::storage::PolicySearchParams;

/// Mock policy storage that returns empty policies (allow-all for testing).
struct MockPolicyStorage;

#[async_trait::async_trait]
impl PolicyStorage for MockPolicyStorage {
    async fn get(&self, _id: &str) -> AuthResult<Option<AccessPolicy>> {
        Ok(None)
    }

    async fn list_active(&self) -> AuthResult<Vec<AccessPolicy>> {
        Ok(vec![])
    }

    async fn list_all(&self) -> AuthResult<Vec<AccessPolicy>> {
        Ok(vec![])
    }

    async fn create(&self, policy: &AccessPolicy) -> AuthResult<AccessPolicy> {
        Ok(policy.clone())
    }

    async fn update(&self, _id: &str, policy: &AccessPolicy) -> AuthResult<AccessPolicy> {
        Ok(policy.clone())
    }

    async fn delete(&self, _id: &str) -> AuthResult<()> {
        Ok(())
    }

    async fn find_applicable(
        &self,
        _resource_type: &str,
        _operation: FhirOperation,
    ) -> AuthResult<Vec<AccessPolicy>> {
        Ok(vec![])
    }

    async fn get_by_ids(&self, _ids: &[String]) -> AuthResult<Vec<AccessPolicy>> {
        Ok(vec![])
    }

    async fn search(&self, _params: &PolicySearchParams) -> AuthResult<Vec<AccessPolicy>> {
        Ok(vec![])
    }

    async fn find_for_client(&self, _client_id: &str) -> AuthResult<Vec<AccessPolicy>> {
        Ok(vec![])
    }

    async fn find_for_user(&self, _user_id: &str) -> AuthResult<Vec<AccessPolicy>> {
        Ok(vec![])
    }

    async fn find_for_role(&self, _role: &str) -> AuthResult<Vec<AccessPolicy>> {
        Ok(vec![])
    }
}

// =============================================================================
// Test Helpers
// =============================================================================

fn create_test_registry() -> Arc<SearchParameterRegistry> {
    let mut registry = SearchParameterRegistry::new();

    // Add Patient search parameters
    registry.register(SearchParameter::new(
        "name",
        "http://hl7.org/fhir/SearchParameter/Patient-name",
        SearchParameterType::String,
        vec!["Patient".to_string()],
    ));
    registry.register(SearchParameter::new(
        "birthdate",
        "http://hl7.org/fhir/SearchParameter/Patient-birthdate",
        SearchParameterType::Date,
        vec!["Patient".to_string()],
    ));
    registry.register(SearchParameter::new(
        "gender",
        "http://hl7.org/fhir/SearchParameter/Patient-gender",
        SearchParameterType::Token,
        vec!["Patient".to_string()],
    ));

    // Add Observation search parameters
    registry.register(SearchParameter::new(
        "code",
        "http://hl7.org/fhir/SearchParameter/Observation-code",
        SearchParameterType::Token,
        vec!["Observation".to_string()],
    ));
    registry.register(SearchParameter::new(
        "patient",
        "http://hl7.org/fhir/SearchParameter/Observation-patient",
        SearchParameterType::Reference,
        vec!["Observation".to_string()],
    ));

    Arc::new(registry)
}

fn create_test_patient(id: &str, name: &str, gender: &str) -> StoredResource {
    StoredResource::new(
        id,
        "1",
        "Patient",
        json!({
            "resourceType": "Patient",
            "id": id,
            "name": [{"family": name}],
            "gender": gender
        }),
    )
}

#[allow(dead_code)]
fn create_test_observation(id: &str, patient_id: &str, code: &str) -> StoredResource {
    StoredResource::new(
        id,
        "1",
        "Observation",
        json!({
            "resourceType": "Observation",
            "id": id,
            "subject": {"reference": format!("Patient/{}", patient_id)},
            "code": {"coding": [{"code": code}]}
        }),
    )
}

async fn build_test_schema(registry: Arc<SearchParameterRegistry>) -> Schema {
    let builder = FhirSchemaBuilder::new(registry, SchemaBuilderConfig::default());
    builder.build().await.expect("Schema should build")
}

fn create_policy_evaluator() -> Arc<PolicyEvaluator> {
    let policy_storage: Arc<dyn PolicyStorage> = Arc::new(MockPolicyStorage);
    let policy_cache = Arc::new(PolicyCache::new(policy_storage, Duration::minutes(5)));
    let config = PolicyEvaluatorConfig::default();
    Arc::new(PolicyEvaluator::new(policy_cache, config))
}

fn build_test_context(
    storage: MockStorage,
    registry: Arc<SearchParameterRegistry>,
) -> GraphQLContext {
    let search_config = SearchConfig::new(registry);
    let policy_evaluator = create_policy_evaluator();

    GraphQLContextBuilder::new()
        .with_storage(Arc::new(storage) as DynStorage)
        .with_search_config(search_config)
        .with_policy_evaluator(policy_evaluator)
        .with_request_id("test-request-123")
        .build()
        .expect("Context should build")
}

// =============================================================================
// Schema Tests (no storage needed)
// =============================================================================

#[tokio::test]
async fn test_schema_has_patient_queries() {
    let registry = create_test_registry();
    let schema = build_test_schema(registry).await;
    let sdl = schema.sdl();

    // Verify Patient queries exist
    assert!(sdl.contains("Patient("), "Should have Patient read query");
    assert!(
        sdl.contains("PatientList("),
        "Should have PatientList query"
    );
    assert!(
        sdl.contains("PatientConnection("),
        "Should have PatientConnection query"
    );

    // Verify Patient connection types exist
    assert!(
        sdl.contains("type PatientConnection"),
        "Should have PatientConnection type"
    );
    assert!(
        sdl.contains("type PatientEdge"),
        "Should have PatientEdge type"
    );
}

#[tokio::test]
async fn test_schema_has_observation_queries() {
    let registry = create_test_registry();
    let schema = build_test_schema(registry).await;
    let sdl = schema.sdl();

    // Verify Observation queries exist
    assert!(
        sdl.contains("Observation("),
        "Should have Observation read query"
    );
    assert!(
        sdl.contains("ObservationList("),
        "Should have ObservationList query"
    );
    assert!(
        sdl.contains("ObservationConnection("),
        "Should have ObservationConnection query"
    );
}

#[tokio::test]
async fn test_schema_has_search_parameters() {
    let registry = create_test_registry();
    let schema = build_test_schema(registry).await;
    let sdl = schema.sdl();

    // Patient search parameters should be GraphQL-safe
    assert!(
        sdl.contains("name: String"),
        "Should have name search param"
    );
    assert!(
        sdl.contains("birthdate: String"),
        "Should have birthdate search param"
    );
    assert!(
        sdl.contains("gender: String"),
        "Should have gender search param"
    );

    // Common pagination params
    assert!(sdl.contains("_count: Int"), "Should have _count param");
    assert!(sdl.contains("_offset: Int"), "Should have _offset param");
    assert!(sdl.contains("_sort: String"), "Should have _sort param");

    // _reference param for reverse reference queries
    assert!(
        sdl.contains("_reference: String"),
        "Should have _reference param"
    );
}

#[tokio::test]
async fn test_schema_connection_has_pagination_fields() {
    let registry = create_test_registry();
    let schema = build_test_schema(registry).await;
    let sdl = schema.sdl();

    // Connection should have pagination cursors
    assert!(
        sdl.contains("first: String"),
        "Connection should have first cursor"
    );
    assert!(
        sdl.contains("previous: String"),
        "Connection should have previous cursor"
    );
    assert!(
        sdl.contains("next: String"),
        "Connection should have next cursor"
    );
    assert!(
        sdl.contains("last: String"),
        "Connection should have last cursor"
    );

    // Connection should have count and pagination info
    assert!(sdl.contains("count: Int"), "Connection should have count");
    assert!(sdl.contains("offset: Int"), "Connection should have offset");
    assert!(
        sdl.contains("pageSize: Int"),
        "Connection should have pageSize"
    );
    assert!(sdl.contains("edges: ["), "Connection should have edges");
}

#[tokio::test]
async fn test_schema_edge_has_required_fields() {
    let registry = create_test_registry();
    let schema = build_test_schema(registry).await;
    let sdl = schema.sdl();

    // Edge should have resource and metadata
    assert!(
        sdl.contains("resource: FhirResource!"),
        "Edge should have resource field"
    );
    assert!(sdl.contains("mode: String"), "Edge should have mode field");
    assert!(sdl.contains("score: Float"), "Edge should have score field");
}

// =============================================================================
// Query Execution Tests (with storage)
// =============================================================================

#[tokio::test]
async fn test_patient_read_query_with_storage() {
    let registry = create_test_registry();
    let schema = build_test_schema(registry.clone()).await;

    // Create storage with test patient
    let storage = MockStorage::with_resources(vec![create_test_patient("123", "Smith", "male")]);
    let context = build_test_context(storage, registry.clone());

    // Execute query
    let query = r#"
        query {
            Patient(_id: "123")
        }
    "#;

    let request = async_graphql::Request::new(query).data(context);
    let response = schema.execute(request).await;

    // Should not have errors
    assert!(
        response.errors.is_empty(),
        "Query should succeed: {:?}",
        response.errors
    );

    // Should return the patient
    let data = response.data.into_json().expect("Should have data");
    assert!(data["Patient"].is_object(), "Should return Patient object");
    assert_eq!(data["Patient"]["id"], "123");
    assert_eq!(data["Patient"]["resourceType"], "Patient");
}

#[tokio::test]
async fn test_patient_read_not_found() {
    let registry = create_test_registry();
    let schema = build_test_schema(registry.clone()).await;

    let storage = MockStorage::new();
    let context = build_test_context(storage, registry.clone());

    let query = r#"
        query {
            Patient(_id: "nonexistent")
        }
    "#;

    let request = async_graphql::Request::new(query).data(context);
    let response = schema.execute(request).await;

    // Should not have errors (null is valid for optional field)
    assert!(
        response.errors.is_empty(),
        "Query should succeed even if not found"
    );

    // Should return null
    let data = response.data.into_json().expect("Should have data");
    assert!(
        data["Patient"].is_null(),
        "Should return null for not found"
    );
}

#[tokio::test]
async fn test_patient_list_query() {
    let registry = create_test_registry();
    let schema = build_test_schema(registry.clone()).await;

    let storage = MockStorage::with_resources(vec![
        create_test_patient("1", "Smith", "male"),
        create_test_patient("2", "Jones", "female"),
        create_test_patient("3", "Brown", "male"),
    ]);
    let context = build_test_context(storage, registry.clone());

    // Note: PatientList returns [FhirResource!]! where FhirResource is a scalar (JSON),
    // so we don't select individual fields - the full resource JSON is returned
    let query = r#"
        query {
            PatientList
        }
    "#;

    let request = async_graphql::Request::new(query).data(context);
    let response = schema.execute(request).await;

    assert!(
        response.errors.is_empty(),
        "Query should succeed: {:?}",
        response.errors
    );

    let data = response.data.into_json().expect("Should have data");
    let patients = data["PatientList"].as_array().expect("Should be array");
    assert_eq!(patients.len(), 3, "Should return all 3 patients");

    // Verify first patient has expected structure
    assert_eq!(patients[0]["id"], "1");
    assert_eq!(patients[0]["resourceType"], "Patient");
}

#[tokio::test]
async fn test_patient_list_with_count() {
    let registry = create_test_registry();
    let schema = build_test_schema(registry.clone()).await;

    let storage = MockStorage::with_resources(vec![
        create_test_patient("1", "Smith", "male"),
        create_test_patient("2", "Jones", "female"),
        create_test_patient("3", "Brown", "male"),
    ]);
    let context = build_test_context(storage, registry.clone());

    // FhirResource is a scalar (returns full JSON), so no field selection
    let query = r#"
        query {
            PatientList(_count: 2)
        }
    "#;

    let request = async_graphql::Request::new(query).data(context);
    let response = schema.execute(request).await;

    assert!(
        response.errors.is_empty(),
        "Query should succeed: {:?}",
        response.errors
    );

    let data = response.data.into_json().expect("Should have data");
    let patients = data["PatientList"].as_array().expect("Should be array");
    assert_eq!(patients.len(), 2, "Should return only 2 patients");

    // Verify first patient structure
    assert_eq!(patients[0]["id"], "1");
    assert_eq!(patients[0]["resourceType"], "Patient");
}

#[tokio::test]
async fn test_patient_list_with_offset() {
    let registry = create_test_registry();
    let schema = build_test_schema(registry.clone()).await;

    let storage = MockStorage::with_resources(vec![
        create_test_patient("1", "Smith", "male"),
        create_test_patient("2", "Jones", "female"),
        create_test_patient("3", "Brown", "male"),
    ]);
    let context = build_test_context(storage, registry.clone());

    // FhirResource is a scalar (returns full JSON), so no field selection
    let query = r#"
        query {
            PatientList(_offset: 1, _count: 10)
        }
    "#;

    let request = async_graphql::Request::new(query).data(context);
    let response = schema.execute(request).await;

    assert!(
        response.errors.is_empty(),
        "Query should succeed: {:?}",
        response.errors
    );

    let data = response.data.into_json().expect("Should have data");
    let patients = data["PatientList"].as_array().expect("Should be array");
    assert_eq!(patients.len(), 2, "Should return 2 patients after offset");

    // Verify offset worked - should start from id "2"
    assert_eq!(patients[0]["id"], "2");
    assert_eq!(patients[1]["id"], "3");
}

#[tokio::test]
async fn test_patient_connection_query() {
    let registry = create_test_registry();
    let schema = build_test_schema(registry.clone()).await;

    let storage = MockStorage::with_resources(vec![
        create_test_patient("1", "Smith", "male"),
        create_test_patient("2", "Jones", "female"),
        create_test_patient("3", "Brown", "male"),
    ]);
    let context = build_test_context(storage, registry.clone());

    // Note: resource is FhirResource scalar - no field selection allowed
    let query = r#"
        query {
            PatientConnection(_count: 2) {
                count
                pageSize
                edges {
                    resource
                    mode
                }
                first
                next
            }
        }
    "#;

    let request = async_graphql::Request::new(query).data(context);
    let response = schema.execute(request).await;

    assert!(
        response.errors.is_empty(),
        "Query should succeed: {:?}",
        response.errors
    );

    let data = response.data.into_json().expect("Should have data");
    let connection = &data["PatientConnection"];

    assert_eq!(connection["count"], 3, "Count should be total");
    assert_eq!(connection["pageSize"], 2, "Page size should be 2");

    let edges = connection["edges"].as_array().expect("Should have edges");
    assert_eq!(edges.len(), 2, "Should return 2 edges");

    // Verify edge structure - resource is the full FHIR resource JSON
    assert_eq!(edges[0]["resource"]["id"], "1");
    assert_eq!(edges[0]["resource"]["resourceType"], "Patient");
    assert_eq!(edges[0]["mode"], "match");

    // Should have cursors for navigation
    assert!(!connection["first"].is_null(), "Should have first cursor");
    assert!(!connection["next"].is_null(), "Should have next cursor");
}

#[tokio::test]
async fn test_empty_search_returns_empty_array() {
    let registry = create_test_registry();
    let schema = build_test_schema(registry.clone()).await;

    let storage = MockStorage::new();
    let context = build_test_context(storage, registry.clone());

    // FhirResource is a scalar (returns full JSON), so no field selection
    let query = r#"
        query {
            PatientList
        }
    "#;

    let request = async_graphql::Request::new(query).data(context);
    let response = schema.execute(request).await;

    assert!(
        response.errors.is_empty(),
        "Query should succeed: {:?}",
        response.errors
    );

    let data = response.data.into_json().expect("Should have data");
    let patients = data["PatientList"].as_array().expect("Should be array");
    assert!(patients.is_empty(), "Should return empty array, not null");
}

#[tokio::test]
async fn test_health_check_query() {
    let registry = create_test_registry();
    let schema = build_test_schema(registry.clone()).await;

    let storage = MockStorage::new();
    let context = build_test_context(storage, registry.clone());

    let query = r#"
        query {
            _health
        }
    "#;

    let request = async_graphql::Request::new(query).data(context);
    let response = schema.execute(request).await;

    assert!(response.errors.is_empty());

    let data = response.data.into_json().expect("Should have data");
    assert_eq!(data["_health"], "ok", "Health check should return ok");
}

#[tokio::test]
async fn test_version_query() {
    let registry = create_test_registry();
    let schema = build_test_schema(registry.clone()).await;

    let storage = MockStorage::new();
    let context = build_test_context(storage, registry.clone());

    let query = r#"
        query {
            _version
        }
    "#;

    let request = async_graphql::Request::new(query).data(context);
    let response = schema.execute(request).await;

    assert!(response.errors.is_empty());

    let data = response.data.into_json().expect("Should have data");
    assert!(data["_version"].is_string(), "Version should return string");
}
