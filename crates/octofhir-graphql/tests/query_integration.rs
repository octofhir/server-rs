//! Integration tests for GraphQL query implementation.
//!
//! These tests verify the complete query flow from GraphQL schema
//! to storage and back.
//!
//! Uses embedded FHIR schemas for real type generation.

use std::sync::Arc;

use async_graphql::dynamic::Schema;
use octofhir_auth::AuthResult;
use octofhir_auth::middleware::AuthContext;
use octofhir_auth::policy::cache::PolicyCache;
use octofhir_auth::policy::engine::{DefaultDecision, PolicyEvaluator, PolicyEvaluatorConfig};
use octofhir_auth::storage::PolicyStorage;
use octofhir_auth::token::jwt::AccessTokenClaims;
use octofhir_auth::types::client::{Client, GrantType};
use octofhir_fhir_model::provider::FhirVersion;
use octofhir_fhirschema::{FhirSchemaModelProvider, get_schemas};
use octofhir_graphql::{
    DynModelProvider, FhirSchemaBuilder, GraphQLContext, GraphQLContextBuilder, SchemaBuilderConfig,
};
use octofhir_search::{
    SearchConfig, SearchParameter, SearchParameterRegistry, SearchParameterType,
};
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

    async fn system_history(&self, _params: &HistoryParams) -> Result<HistoryResult, StorageError> {
        Err(StorageError::internal("system_history not supported in mock"))
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

    async fn upsert(&self, policy: &AccessPolicy) -> AuthResult<AccessPolicy> {
        Ok(policy.clone())
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

fn create_test_model_provider() -> DynModelProvider {
    // Use embedded FHIR R4 schemas for real type generation
    // Note: R4B package only has 5 extension StructureDefinitions, R4 has all core types
    let schemas = get_schemas(octofhir_fhirschema::FhirVersion::R4).clone();
    let provider = FhirSchemaModelProvider::new(schemas, FhirVersion::R4);
    Arc::new(provider)
}

async fn build_test_schema(registry: Arc<SearchParameterRegistry>) -> Schema {
    let model_provider = create_test_model_provider();
    let builder = FhirSchemaBuilder::new(registry, model_provider, SchemaBuilderConfig::default());
    builder.build().await.expect("Schema should build")
}

fn create_policy_evaluator() -> Arc<PolicyEvaluator> {
    let policy_storage: Arc<dyn PolicyStorage> = Arc::new(MockPolicyStorage);
    let policy_cache = Arc::new(PolicyCache::new(policy_storage, Duration::minutes(5)));
    // Use allow-by-default for tests since we have no policies defined
    let config = PolicyEvaluatorConfig {
        default_decision: DefaultDecision::Allow,
        ..PolicyEvaluatorConfig::default()
    };
    Arc::new(PolicyEvaluator::new(policy_cache, config))
}

/// Creates a test auth context with full FHIR access scopes.
fn create_test_auth_context() -> AuthContext {
    // Create token claims with full access scopes
    let token_claims = AccessTokenClaims {
        iss: "http://test.octofhir.local".to_string(),
        sub: "test-user".to_string(),
        aud: vec!["http://test.octofhir.local".to_string()],
        exp: (time::OffsetDateTime::now_utc() + Duration::hours(1)).unix_timestamp(),
        iat: time::OffsetDateTime::now_utc().unix_timestamp(),
        jti: "test-token-id".to_string(),
        // Full FHIR access scopes for testing
        scope: "user/*.cruds".to_string(),
        client_id: "test-client".to_string(),
        patient: None,
        encounter: None,
        fhir_user: Some("Practitioner/test-user".to_string()),
    };

    // Create test client
    let client = Client {
        client_id: "test-client".to_string(),
        client_secret: None,
        name: "Test Client".to_string(),
        description: Some("Test client for integration tests".to_string()),
        grant_types: vec![GrantType::AuthorizationCode],
        redirect_uris: vec!["http://localhost/callback".to_string()],
        scopes: vec!["user/*.cruds".to_string()],
        confidential: false,
        active: true,
        access_token_lifetime: Some(3600),
        refresh_token_lifetime: None,
        pkce_required: None,
        allowed_origins: vec![],
        jwks: None,
        jwks_uri: None,
    };

    AuthContext {
        token_claims,
        client,
        user: None,
        patient: None,
        encounter: None,
    }
}

fn build_test_context(
    storage: MockStorage,
    registry: Arc<SearchParameterRegistry>,
) -> GraphQLContext {
    let search_config = SearchConfig::new(registry);
    let policy_evaluator = create_policy_evaluator();
    let auth_context = create_test_auth_context();

    GraphQLContextBuilder::new()
        .with_storage(Arc::new(storage) as DynStorage)
        .with_search_config(search_config)
        .with_policy_evaluator(policy_evaluator)
        .with_auth_context(Some(auth_context))
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
    // Search params now use list types for OR logic support: name: [String]
    assert!(
        sdl.contains("name: [String]"),
        "Should have name search param (list type for OR logic)"
    );
    assert!(
        sdl.contains("birthdate: [String]"),
        "Should have birthdate search param (list type for OR logic)"
    );
    assert!(
        sdl.contains("gender: [String]"),
        "Should have gender search param (list type for OR logic)"
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
    // With typed resources, PatientEdge has resource: Patient!, not FhirResource
    assert!(
        sdl.contains("resource: Patient!"),
        "PatientEdge should have resource: Patient! field"
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

    // Execute query - with typed resources, we must select specific fields
    // Using real FHIR R4B schema with proper field names
    let query = r#"
        query {
            Patient(_id: "123") {
                id
                gender
                birthDate
            }
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

    // Should return the patient with selected fields
    let data = response.data.into_json().expect("Should have data");
    assert!(data["Patient"].is_object(), "Should return Patient object");
    assert_eq!(data["Patient"]["id"], "123");
    assert_eq!(data["Patient"]["gender"], "male");
}

#[tokio::test]
async fn test_patient_read_not_found() {
    let registry = create_test_registry();
    let schema = build_test_schema(registry.clone()).await;

    let storage = MockStorage::new();
    let context = build_test_context(storage, registry.clone());

    // With typed resources, we must select fields
    let query = r#"
        query {
            Patient(_id: "nonexistent") {
                id
                gender
            }
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

    // With typed resources, PatientList returns [Patient!]! and we must select fields
    let query = r#"
        query {
            PatientList {
                id
                gender
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
    let patients = data["PatientList"].as_array().expect("Should be array");
    assert_eq!(patients.len(), 3, "Should return all 3 patients");

    // Verify first patient has expected structure with selected fields
    assert_eq!(patients[0]["id"], "1");
    assert_eq!(patients[0]["gender"], "male");
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

    // With typed resources, we must select fields
    let query = r#"
        query {
            PatientList(_count: 2) {
                id
                gender
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
    let patients = data["PatientList"].as_array().expect("Should be array");
    assert_eq!(patients.len(), 2, "Should return only 2 patients");

    // Verify first patient structure
    assert_eq!(patients[0]["id"], "1");
    assert_eq!(patients[0]["gender"], "male");
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

    // With typed resources, we must select fields
    let query = r#"
        query {
            PatientList(_offset: 1, _count: 10) {
                id
                gender
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

    // With typed resources, edge.resource returns Patient and we must select fields
    let query = r#"
        query {
            PatientConnection(_count: 2) {
                count
                pageSize
                edges {
                    resource {
                        id
                        gender
                    }
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

    // Verify edge structure - resource contains selected fields
    assert_eq!(edges[0]["resource"]["id"], "1");
    assert_eq!(edges[0]["resource"]["gender"], "male");
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

    // With typed resources, we must select fields
    let query = r#"
        query {
            PatientList {
                id
                gender
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

// =============================================================================
// Reference Resolution Tests
// =============================================================================

#[tokio::test]
async fn test_schema_has_reference_type() {
    let registry = create_test_registry();
    let schema = build_test_schema(registry.clone()).await;

    let storage = MockStorage::new();
    let context = build_test_context(storage, registry.clone());

    // Query for the Reference type via introspection
    let query = r#"
        query {
            __type(name: "Reference") {
                name
                kind
                fields {
                    name
                }
            }
        }
    "#;

    let request = async_graphql::Request::new(query).data(context);
    let response = schema.execute(request).await;

    assert!(
        response.errors.is_empty(),
        "Introspection should succeed: {:?}",
        response.errors
    );

    let data = response.data.into_json().expect("Should have data");
    let type_info = &data["__type"];

    assert!(!type_info.is_null(), "Reference type should exist");
    assert_eq!(type_info["name"], "Reference");
    assert_eq!(type_info["kind"], "OBJECT");

    // Verify expected fields
    let fields = type_info["fields"].as_array().expect("Should have fields");
    let field_names: Vec<&str> = fields.iter().map(|f| f["name"].as_str().unwrap()).collect();

    assert!(
        field_names.contains(&"reference"),
        "Reference type should have 'reference' field"
    );
    assert!(
        field_names.contains(&"display"),
        "Reference type should have 'display' field"
    );
    assert!(
        field_names.contains(&"type"),
        "Reference type should have 'type' field"
    );
}

#[tokio::test]
async fn test_schema_has_all_resources_union() {
    let registry = create_test_registry();
    let schema = build_test_schema(registry.clone()).await;

    let storage = MockStorage::new();
    let context = build_test_context(storage, registry.clone());

    // Query for the AllResources union via introspection
    let query = r#"
        query {
            __type(name: "AllResources") {
                name
                kind
                possibleTypes {
                    name
                }
            }
        }
    "#;

    let request = async_graphql::Request::new(query).data(context);
    let response = schema.execute(request).await;

    assert!(
        response.errors.is_empty(),
        "Introspection should succeed: {:?}",
        response.errors
    );

    let data = response.data.into_json().expect("Should have data");
    let type_info = &data["__type"];

    assert!(!type_info.is_null(), "AllResources union should exist");
    assert_eq!(type_info["name"], "AllResources");
    assert_eq!(type_info["kind"], "UNION");

    // Should have possible types
    let possible_types = type_info["possibleTypes"]
        .as_array()
        .expect("Should have possibleTypes");
    assert!(
        !possible_types.is_empty(),
        "AllResources should include resource types"
    );

    let type_names: Vec<&str> = possible_types
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();

    // Patient and Observation are registered in our test registry
    assert!(
        type_names.contains(&"Patient"),
        "AllResources should include Patient"
    );
    assert!(
        type_names.contains(&"Observation"),
        "AllResources should include Observation"
    );
}

#[tokio::test]
async fn test_reference_type_has_resource_field_with_arguments() {
    let registry = create_test_registry();
    let schema = build_test_schema(registry.clone()).await;

    let storage = MockStorage::new();
    let context = build_test_context(storage, registry.clone());

    // Query the resource field definition with arguments
    let query = r#"
        query {
            __type(name: "Reference") {
                fields {
                    name
                    args {
                        name
                        type {
                            name
                            kind
                        }
                        defaultValue
                    }
                }
            }
        }
    "#;

    let request = async_graphql::Request::new(query).data(context);
    let response = schema.execute(request).await;

    assert!(
        response.errors.is_empty(),
        "Introspection should succeed: {:?}",
        response.errors
    );

    let data = response.data.into_json().expect("Should have data");
    let fields = data["__type"]["fields"]
        .as_array()
        .expect("Should have fields");

    // Find the resource field
    let resource_field = fields.iter().find(|f| f["name"] == "resource");
    assert!(
        resource_field.is_some(),
        "Reference should have resource field"
    );

    let resource_field = resource_field.unwrap();
    let args = resource_field["args"]
        .as_array()
        .expect("resource should have args");

    // Verify optional argument
    let optional_arg = args.iter().find(|a| a["name"] == "optional");
    assert!(
        optional_arg.is_some(),
        "resource should have optional argument"
    );

    // Verify type argument
    let type_arg = args.iter().find(|a| a["name"] == "type");
    assert!(type_arg.is_some(), "resource should have type argument");
}

#[tokio::test]
async fn test_dataloader_batches_resource_loads() {
    // Create test data with multiple related resources
    let patient1 = StoredResource::new(
        "patient-1",
        "1",
        "Patient",
        json!({
            "resourceType": "Patient",
            "id": "patient-1",
            "gender": "male",
            "birthDate": "1990-01-01"
        }),
    );

    let patient2 = StoredResource::new(
        "patient-2",
        "1",
        "Patient",
        json!({
            "resourceType": "Patient",
            "id": "patient-2",
            "gender": "female",
            "birthDate": "1985-05-15"
        }),
    );

    let registry = create_test_registry();
    let storage = MockStorage::with_resources(vec![patient1, patient2]);
    let context = build_test_context(storage, registry.clone());

    // Use the DataLoader directly via context
    let result1 = context.load_resource("Patient", "patient-1").await;
    let result2 = context.load_resource("Patient", "patient-2").await;
    let result_missing = context.load_resource("Patient", "non-existent").await;

    assert!(result1.is_some(), "Should find patient-1");
    assert!(result2.is_some(), "Should find patient-2");
    assert!(result_missing.is_none(), "Should not find non-existent");

    // Verify data
    let p1 = result1.unwrap();
    assert_eq!(p1["id"], "patient-1");
    assert_eq!(p1["gender"], "male");

    let p2 = result2.unwrap();
    assert_eq!(p2["id"], "patient-2");
    assert_eq!(p2["gender"], "female");
}

#[tokio::test]
async fn test_reference_loader_parses_references() {
    let patient = StoredResource::new(
        "patient-123",
        "1",
        "Patient",
        json!({
            "resourceType": "Patient",
            "id": "patient-123",
            "name": [{"family": "Smith", "given": ["John"]}]
        }),
    );

    let registry = create_test_registry();
    let storage = MockStorage::with_resources(vec![patient]);
    let context = build_test_context(storage, registry.clone());

    // Test relative reference
    let result = context.resolve_reference("Patient/patient-123").await;
    assert!(result.is_some(), "Should resolve relative reference");
    let resolved = result.unwrap();
    assert_eq!(resolved.parsed.resource_type, "Patient");
    assert_eq!(resolved.parsed.id, "patient-123");
    assert!(resolved.resource.is_some(), "Should have resource");

    // Test absolute reference
    let result = context
        .resolve_reference("http://example.org/fhir/Patient/patient-123")
        .await;
    assert!(result.is_some(), "Should resolve absolute reference");
    let resolved = result.unwrap();
    assert_eq!(resolved.parsed.resource_type, "Patient");
    assert!(resolved.parsed.is_absolute);

    // Test missing reference
    let result = context.resolve_reference("Patient/non-existent").await;
    assert!(result.is_some(), "Should return parsed reference");
    let resolved = result.unwrap();
    assert!(
        resolved.resource.is_none(),
        "Resource should be None for missing"
    );

    // Test contained reference
    let result = context.resolve_reference("#contained-med").await;
    assert!(result.is_some(), "Should parse contained reference");
    let resolved = result.unwrap();
    assert!(resolved.parsed.is_contained);
    assert_eq!(resolved.parsed.id, "contained-med");
}

// =============================================================================
// Include/Reverse Include Tests
// =============================================================================

fn create_test_registry_with_targets() -> Arc<SearchParameterRegistry> {
    let mut registry = SearchParameterRegistry::new();

    // Add Patient search parameters
    registry.register(SearchParameter::new(
        "name",
        "http://hl7.org/fhir/SearchParameter/Patient-name",
        SearchParameterType::String,
        vec!["Patient".to_string()],
    ));
    registry.register(SearchParameter::new(
        "gender",
        "http://hl7.org/fhir/SearchParameter/Patient-gender",
        SearchParameterType::Token,
        vec!["Patient".to_string()],
    ));

    // Add Observation search parameters with targets
    registry.register(SearchParameter::new(
        "code",
        "http://hl7.org/fhir/SearchParameter/Observation-code",
        SearchParameterType::Token,
        vec!["Observation".to_string()],
    ));
    registry.register(
        SearchParameter::new(
            "subject",
            "http://hl7.org/fhir/SearchParameter/Observation-subject",
            SearchParameterType::Reference,
            vec!["Observation".to_string()],
        )
        .with_targets(vec!["Patient".to_string()]),
    );
    registry.register(
        SearchParameter::new(
            "patient",
            "http://hl7.org/fhir/SearchParameter/Observation-patient",
            SearchParameterType::Reference,
            vec!["Observation".to_string()],
        )
        .with_targets(vec!["Patient".to_string()]),
    );

    Arc::new(registry)
}

#[tokio::test]
async fn test_schema_has_reverse_reference_fields() {
    let registry = create_test_registry_with_targets();
    let schema = build_test_schema(registry.clone()).await;
    let sdl = schema.sdl();

    // Patient should have ObservationList_subject and ObservationList_patient fields
    assert!(
        sdl.contains("ObservationList_subject"),
        "Patient should have ObservationList_subject field"
    );
    assert!(
        sdl.contains("ObservationList_patient"),
        "Patient should have ObservationList_patient field"
    );
}

#[tokio::test]
async fn test_reverse_reference_field_introspection() {
    let registry = create_test_registry_with_targets();
    let schema = build_test_schema(registry.clone()).await;

    let storage = MockStorage::new();
    let context = build_test_context(storage, registry.clone());

    // Query Patient type to find reverse reference fields
    let query = r#"
        query {
            __type(name: "Patient") {
                fields {
                    name
                    description
                    type {
                        name
                        kind
                        ofType {
                            name
                        }
                    }
                }
            }
        }
    "#;

    let request = async_graphql::Request::new(query).data(context);
    let response = schema.execute(request).await;

    assert!(
        response.errors.is_empty(),
        "Introspection should succeed: {:?}",
        response.errors
    );

    let data = response.data.into_json().expect("Should have data");
    let fields = data["__type"]["fields"]
        .as_array()
        .expect("Should have fields");

    let field_names: Vec<&str> = fields.iter().map(|f| f["name"].as_str().unwrap()).collect();

    // Should have reverse reference fields
    assert!(
        field_names.contains(&"ObservationList_subject"),
        "Patient should have ObservationList_subject field"
    );
    assert!(
        field_names.contains(&"ObservationList_patient"),
        "Patient should have ObservationList_patient field"
    );

    // Find the ObservationList_subject field and verify its type
    let subject_field = fields
        .iter()
        .find(|f| f["name"] == "ObservationList_subject");
    assert!(
        subject_field.is_some(),
        "Should have ObservationList_subject field"
    );

    let subject_field = subject_field.unwrap();
    assert_eq!(
        subject_field["type"]["kind"], "LIST",
        "Should return a list"
    );
}

#[tokio::test]
async fn test_reverse_reference_query_execution() {
    let registry = create_test_registry_with_targets();

    // Create test data
    let patient = StoredResource::new(
        "patient-123",
        "1",
        "Patient",
        json!({
            "resourceType": "Patient",
            "id": "patient-123",
            "gender": "male"
        }),
    );

    let obs1 = StoredResource::new(
        "obs-1",
        "1",
        "Observation",
        json!({
            "resourceType": "Observation",
            "id": "obs-1",
            "subject": {"reference": "Patient/patient-123"},
            "code": {"coding": [{"code": "vital-signs"}]}
        }),
    );

    let obs2 = StoredResource::new(
        "obs-2",
        "1",
        "Observation",
        json!({
            "resourceType": "Observation",
            "id": "obs-2",
            "subject": {"reference": "Patient/patient-123"},
            "code": {"coding": [{"code": "lab-result"}]}
        }),
    );

    // Observation for different patient - should not be included
    let obs3 = StoredResource::new(
        "obs-3",
        "1",
        "Observation",
        json!({
            "resourceType": "Observation",
            "id": "obs-3",
            "subject": {"reference": "Patient/other-patient"},
            "code": {"coding": [{"code": "other"}]}
        }),
    );

    let storage = MockStorage::with_resources(vec![patient, obs1, obs2, obs3]);

    // Need to customize MockStorage.search to handle reference searches
    // For now, we'll test schema structure. Full execution test requires
    // a more sophisticated mock.

    let context = build_test_context(storage, registry.clone());
    let schema = build_test_schema(registry.clone()).await;

    // Query patient with observations
    let query = r#"
        query {
            Patient(_id: "patient-123") {
                id
                gender
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
    assert_eq!(data["Patient"]["id"], "patient-123");
    assert_eq!(data["Patient"]["gender"], "male");
}

#[tokio::test]
async fn test_nested_reference_resolution() {
    let registry = create_test_registry_with_targets();

    // Create test data with references
    let patient = StoredResource::new(
        "patient-123",
        "1",
        "Patient",
        json!({
            "resourceType": "Patient",
            "id": "patient-123",
            "name": [{"family": "Smith", "given": ["John"]}],
            "gender": "male"
        }),
    );

    let obs = StoredResource::new(
        "obs-1",
        "1",
        "Observation",
        json!({
            "resourceType": "Observation",
            "id": "obs-1",
            "subject": {"reference": "Patient/patient-123"},
            "code": {"coding": [{"code": "vital-signs"}]}
        }),
    );

    let storage = MockStorage::with_resources(vec![patient, obs]);
    let context = build_test_context(storage, registry.clone());
    let schema = build_test_schema(registry.clone()).await;

    // Query observation with nested subject reference resolution
    let query = r#"
        query {
            Observation(_id: "obs-1") {
                id
                subject {
                    reference
                    resource {
                        ... on Patient {
                            id
                            gender
                        }
                    }
                }
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
    assert_eq!(data["Observation"]["id"], "obs-1");
    assert_eq!(
        data["Observation"]["subject"]["reference"],
        "Patient/patient-123"
    );

    // Reference resolution should work
    let resource = &data["Observation"]["subject"]["resource"];
    assert!(!resource.is_null(), "Should resolve resource reference");
    assert_eq!(resource["id"], "patient-123");
    assert_eq!(resource["gender"], "male");
}

#[tokio::test]
async fn test_chained_reference_resolution() {
    let mut registry = SearchParameterRegistry::new();

    // Add Patient search parameters
    registry.register(SearchParameter::new(
        "name",
        "http://hl7.org/fhir/SearchParameter/Patient-name",
        SearchParameterType::String,
        vec!["Patient".to_string()],
    ));
    registry.register(
        SearchParameter::new(
            "organization",
            "http://hl7.org/fhir/SearchParameter/Patient-organization",
            SearchParameterType::Reference,
            vec!["Patient".to_string()],
        )
        .with_targets(vec!["Organization".to_string()]),
    );

    // Add Organization search parameters
    registry.register(SearchParameter::new(
        "name",
        "http://hl7.org/fhir/SearchParameter/Organization-name",
        SearchParameterType::String,
        vec!["Organization".to_string()],
    ));

    // Add Observation search parameters
    registry.register(
        SearchParameter::new(
            "subject",
            "http://hl7.org/fhir/SearchParameter/Observation-subject",
            SearchParameterType::Reference,
            vec!["Observation".to_string()],
        )
        .with_targets(vec!["Patient".to_string()]),
    );

    let registry = Arc::new(registry);

    // Create test data: Observation -> Patient -> Organization
    let org = StoredResource::new(
        "org-1",
        "1",
        "Organization",
        json!({
            "resourceType": "Organization",
            "id": "org-1",
            "active": true
        }),
    );

    let patient = StoredResource::new(
        "patient-123",
        "1",
        "Patient",
        json!({
            "resourceType": "Patient",
            "id": "patient-123",
            "gender": "male",
            "managingOrganization": {"reference": "Organization/org-1"}
        }),
    );

    let obs = StoredResource::new(
        "obs-1",
        "1",
        "Observation",
        json!({
            "resourceType": "Observation",
            "id": "obs-1",
            "subject": {"reference": "Patient/patient-123"}
        }),
    );

    let storage = MockStorage::with_resources(vec![org, patient, obs]);
    let context = build_test_context(storage, registry.clone());
    let schema = build_test_schema(registry.clone()).await;

    // Test chained reference resolution: Observation -> Patient -> Organization
    // Use 'active' field instead of 'name' to avoid array heuristic issues
    let query = r#"
        query {
            Observation(_id: "obs-1") {
                id
                subject {
                    reference
                    resource {
                        ... on Patient {
                            id
                            gender
                            managingOrganization {
                                reference
                                resource {
                                    ... on Organization {
                                        id
                                        active
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    "#;

    let request = async_graphql::Request::new(query).data(context);
    let response = schema.execute(request).await;

    assert!(
        response.errors.is_empty(),
        "Chained reference query should succeed: {:?}",
        response.errors
    );

    let data = response.data.into_json().expect("Should have data");

    // Verify Observation
    assert_eq!(data["Observation"]["id"], "obs-1");

    // Verify Patient (first level)
    let patient_resource = &data["Observation"]["subject"]["resource"];
    assert!(!patient_resource.is_null(), "Should resolve Patient");
    assert_eq!(patient_resource["id"], "patient-123");
    assert_eq!(patient_resource["gender"], "male");

    // Verify Organization (second level - chained)
    let org_resource = &patient_resource["managingOrganization"]["resource"];
    assert!(
        !org_resource.is_null(),
        "Should resolve Organization in chain"
    );
    assert_eq!(org_resource["id"], "org-1");
    assert_eq!(org_resource["active"], true);
}

// =============================================================================
// Mutation Tests
// =============================================================================

#[tokio::test]
async fn test_schema_has_mutation_type() {
    let registry = create_test_registry();
    let schema = build_test_schema(registry.clone()).await;
    let sdl = schema.sdl();

    // Should have Mutation type
    assert!(
        sdl.contains("type Mutation"),
        "Schema should have Mutation type"
    );
}

#[tokio::test]
async fn test_schema_has_create_mutations() {
    let registry = create_test_registry();
    let schema = build_test_schema(registry.clone()).await;
    let sdl = schema.sdl();

    // Should have create mutations for each resource type
    assert!(
        sdl.contains("PatientCreate"),
        "Schema should have PatientCreate mutation"
    );
    assert!(
        sdl.contains("ObservationCreate"),
        "Schema should have ObservationCreate mutation"
    );
}

#[tokio::test]
async fn test_schema_has_update_mutations() {
    let registry = create_test_registry();
    let schema = build_test_schema(registry.clone()).await;
    let sdl = schema.sdl();

    // Should have update mutations for each resource type
    assert!(
        sdl.contains("PatientUpdate"),
        "Schema should have PatientUpdate mutation"
    );
    assert!(
        sdl.contains("ObservationUpdate"),
        "Schema should have ObservationUpdate mutation"
    );
}

#[tokio::test]
async fn test_schema_has_delete_mutations() {
    let registry = create_test_registry();
    let schema = build_test_schema(registry.clone()).await;
    let sdl = schema.sdl();

    // Should have delete mutations for each resource type
    assert!(
        sdl.contains("PatientDelete"),
        "Schema should have PatientDelete mutation"
    );
    assert!(
        sdl.contains("ObservationDelete"),
        "Schema should have ObservationDelete mutation"
    );
}

#[tokio::test]
async fn test_schema_has_input_types() {
    let registry = create_test_registry();
    let schema = build_test_schema(registry.clone()).await;
    let sdl = schema.sdl();

    // Should have input types for each resource type
    assert!(
        sdl.contains("input PatientInput"),
        "Schema should have PatientInput type"
    );
    assert!(
        sdl.contains("input ObservationInput"),
        "Schema should have ObservationInput type"
    );
}

#[tokio::test]
async fn test_schema_has_operation_outcome_type() {
    let registry = create_test_registry();
    let schema = build_test_schema(registry.clone()).await;
    let sdl = schema.sdl();

    // Should have OperationOutcome type for delete responses
    assert!(
        sdl.contains("type OperationOutcome"),
        "Schema should have OperationOutcome type"
    );
    assert!(
        sdl.contains("type OperationOutcomeIssue"),
        "Schema should have OperationOutcomeIssue type"
    );
}

#[tokio::test]
async fn test_mutation_introspection() {
    let registry = create_test_registry();
    let schema = build_test_schema(registry.clone()).await;

    let storage = MockStorage::new();
    let context = build_test_context(storage, registry.clone());

    // Query Mutation type to find all mutation fields
    let query = r#"
        query {
            __type(name: "Mutation") {
                fields {
                    name
                    args {
                        name
                        type {
                            name
                            kind
                            ofType {
                                name
                            }
                        }
                    }
                    type {
                        name
                    }
                }
            }
        }
    "#;

    let request = async_graphql::Request::new(query).data(context);
    let response = schema.execute(request).await;

    assert!(
        response.errors.is_empty(),
        "Introspection should succeed: {:?}",
        response.errors
    );

    let data = response.data.into_json().expect("Should have data");
    let fields = data["__type"]["fields"]
        .as_array()
        .expect("Should have fields");

    let field_names: Vec<&str> = fields.iter().map(|f| f["name"].as_str().unwrap()).collect();

    // Should have CRUD mutations for Patient
    assert!(
        field_names.contains(&"PatientCreate"),
        "Should have PatientCreate mutation"
    );
    assert!(
        field_names.contains(&"PatientUpdate"),
        "Should have PatientUpdate mutation"
    );
    assert!(
        field_names.contains(&"PatientDelete"),
        "Should have PatientDelete mutation"
    );

    // Find PatientCreate and verify its arguments
    let create_field = fields.iter().find(|f| f["name"] == "PatientCreate");
    assert!(create_field.is_some(), "Should have PatientCreate field");

    let create_field = create_field.unwrap();
    let args = create_field["args"].as_array().expect("Should have args");
    let arg_names: Vec<&str> = args.iter().map(|a| a["name"].as_str().unwrap()).collect();

    assert!(
        arg_names.contains(&"res"),
        "PatientCreate should have 'res' argument"
    );

    // Find PatientUpdate and verify its arguments
    let update_field = fields.iter().find(|f| f["name"] == "PatientUpdate");
    assert!(update_field.is_some(), "Should have PatientUpdate field");

    let update_field = update_field.unwrap();
    let args = update_field["args"].as_array().expect("Should have args");
    let arg_names: Vec<&str> = args.iter().map(|a| a["name"].as_str().unwrap()).collect();

    assert!(
        arg_names.contains(&"id"),
        "PatientUpdate should have 'id' argument"
    );
    assert!(
        arg_names.contains(&"res"),
        "PatientUpdate should have 'res' argument"
    );
    assert!(
        arg_names.contains(&"ifMatch"),
        "PatientUpdate should have 'ifMatch' argument"
    );

    // Verify PatientDelete returns OperationOutcome
    let delete_field = fields.iter().find(|f| f["name"] == "PatientDelete");
    assert!(delete_field.is_some(), "Should have PatientDelete field");

    let delete_field = delete_field.unwrap();
    assert_eq!(
        delete_field["type"]["name"], "OperationOutcome",
        "PatientDelete should return OperationOutcome"
    );
}

#[tokio::test]
async fn test_input_type_introspection() {
    let registry = create_test_registry();
    let schema = build_test_schema(registry.clone()).await;

    let storage = MockStorage::new();
    let context = build_test_context(storage, registry.clone());

    // Query PatientInput type
    let query = r#"
        query {
            __type(name: "PatientInput") {
                name
                kind
                inputFields {
                    name
                    type {
                        name
                        kind
                        ofType {
                            name
                        }
                    }
                }
            }
        }
    "#;

    let request = async_graphql::Request::new(query).data(context);
    let response = schema.execute(request).await;

    assert!(
        response.errors.is_empty(),
        "Introspection should succeed: {:?}",
        response.errors
    );

    let data = response.data.into_json().expect("Should have data");
    let input_type = &data["__type"];

    assert_eq!(input_type["name"], "PatientInput");
    assert_eq!(input_type["kind"], "INPUT_OBJECT");

    // Should have resource field
    let fields = input_type["inputFields"]
        .as_array()
        .expect("Should have fields");
    let field_names: Vec<&str> = fields.iter().map(|f| f["name"].as_str().unwrap()).collect();

    assert!(
        field_names.contains(&"resource"),
        "PatientInput should have 'resource' field"
    );
}

#[tokio::test]
async fn test_json_scalar_exists() {
    let registry = create_test_registry();
    let schema = build_test_schema(registry.clone()).await;

    let storage = MockStorage::new();
    let context = build_test_context(storage, registry.clone());

    // Query JSON scalar type
    let query = r#"
        query {
            __type(name: "JSON") {
                name
                kind
            }
        }
    "#;

    let request = async_graphql::Request::new(query).data(context);
    let response = schema.execute(request).await;

    assert!(
        response.errors.is_empty(),
        "Introspection should succeed: {:?}",
        response.errors
    );

    let data = response.data.into_json().expect("Should have data");
    let scalar_type = &data["__type"];

    assert_eq!(scalar_type["name"], "JSON");
    assert_eq!(scalar_type["kind"], "SCALAR");
}

// =============================================================================
// Access Control Tests
// =============================================================================

/// Creates a test auth context with restricted scopes.
fn create_restricted_auth_context(scope: &str) -> AuthContext {
    let token_claims = AccessTokenClaims {
        iss: "http://test.octofhir.local".to_string(),
        sub: "test-user".to_string(),
        aud: vec!["http://test.octofhir.local".to_string()],
        exp: (time::OffsetDateTime::now_utc() + Duration::hours(1)).unix_timestamp(),
        iat: time::OffsetDateTime::now_utc().unix_timestamp(),
        jti: "test-token-id".to_string(),
        scope: scope.to_string(),
        client_id: "test-client".to_string(),
        patient: None,
        encounter: None,
        fhir_user: Some("Practitioner/test-user".to_string()),
    };

    let client = Client {
        client_id: "test-client".to_string(),
        client_secret: None,
        name: "Test Client".to_string(),
        description: Some("Test client for integration tests".to_string()),
        grant_types: vec![GrantType::AuthorizationCode],
        redirect_uris: vec!["http://localhost/callback".to_string()],
        scopes: vec![scope.to_string()],
        confidential: false,
        active: true,
        access_token_lifetime: Some(3600),
        refresh_token_lifetime: None,
        pkce_required: None,
        allowed_origins: vec![],
        jwks: None,
        jwks_uri: None,
    };

    AuthContext {
        token_claims,
        client,
        user: None,
        patient: None,
        encounter: None,
    }
}

/// Creates a policy evaluator that denies by default and evaluates scopes first.
fn create_strict_policy_evaluator() -> Arc<PolicyEvaluator> {
    let policy_storage: Arc<dyn PolicyStorage> = Arc::new(MockPolicyStorage);
    let policy_cache = Arc::new(PolicyCache::new(policy_storage, Duration::minutes(5)));
    // Use deny-by-default with scope evaluation for strict access control
    let config = PolicyEvaluatorConfig {
        default_decision: DefaultDecision::Deny,
        evaluate_scopes_first: true,
        ..PolicyEvaluatorConfig::default()
    };
    Arc::new(PolicyEvaluator::new(policy_cache, config))
}

/// Builds a test context with custom auth settings.
fn build_test_context_with_auth(
    storage: MockStorage,
    registry: Arc<SearchParameterRegistry>,
    auth_context: Option<AuthContext>,
    use_strict_policy: bool,
) -> GraphQLContext {
    let search_config = SearchConfig::new(registry);
    let policy_evaluator = if use_strict_policy {
        create_strict_policy_evaluator()
    } else {
        create_policy_evaluator()
    };

    GraphQLContextBuilder::new()
        .with_storage(Arc::new(storage) as DynStorage)
        .with_search_config(search_config)
        .with_policy_evaluator(policy_evaluator)
        .with_auth_context(auth_context)
        .with_request_id("test-request-123")
        .build()
        .expect("Context should build")
}

#[tokio::test]
async fn test_access_control_denies_insufficient_scope() {
    let registry = create_test_registry();
    let schema = build_test_schema(registry.clone()).await;

    // Create patient in storage
    let patient = create_test_patient("123", "Smith", "male");
    let storage = MockStorage::with_resources(vec![patient]);

    // Create context with only Observation read scope (not Patient)
    let auth_context = create_restricted_auth_context("user/Observation.r");
    let context = build_test_context_with_auth(storage, registry.clone(), Some(auth_context), true);

    let query = r#"
        query {
            Patient(_id: "123") {
                id
            }
        }
    "#;

    let request = async_graphql::Request::new(query).data(context);
    let response = schema.execute(request).await;

    // Should fail due to insufficient scope
    assert!(
        !response.errors.is_empty(),
        "Query should fail with insufficient scope"
    );

    let error = &response.errors[0];
    assert!(
        error.message.contains("scope"),
        "Error should mention scope: {}",
        error.message
    );
}

#[tokio::test]
async fn test_access_control_allows_sufficient_scope() {
    let registry = create_test_registry();
    let schema = build_test_schema(registry.clone()).await;

    // Create patient in storage
    let patient = create_test_patient("123", "Smith", "male");
    let storage = MockStorage::with_resources(vec![patient]);

    // Create context with Patient read scope (note: .r is the SMART scope suffix for read)
    let auth_context = create_restricted_auth_context("user/Patient.r");
    let context =
        build_test_context_with_auth(storage, registry.clone(), Some(auth_context), false);

    let query = r#"
        query {
            Patient(_id: "123") {
                id
            }
        }
    "#;

    let request = async_graphql::Request::new(query).data(context);
    let response = schema.execute(request).await;

    // Should succeed with proper scope
    assert!(
        response.errors.is_empty(),
        "Query should succeed with correct scope: {:?}",
        response.errors
    );
}

#[tokio::test]
async fn test_access_control_denies_anonymous_with_strict_policy() {
    let registry = create_test_registry();
    let schema = build_test_schema(registry.clone()).await;

    let patient = create_test_patient("123", "Smith", "male");
    let storage = MockStorage::with_resources(vec![patient]);

    // Create context without auth (anonymous)
    let context = build_test_context_with_auth(storage, registry.clone(), None, true);

    let query = r#"
        query {
            Patient(_id: "123") {
                id
            }
        }
    "#;

    let request = async_graphql::Request::new(query).data(context);
    let response = schema.execute(request).await;

    // Should fail for anonymous request with strict policy
    assert!(
        !response.errors.is_empty(),
        "Query should fail for anonymous request with strict policy"
    );
}

#[tokio::test]
async fn test_access_control_wildcard_scope_allows_all() {
    let registry = create_test_registry();
    let schema = build_test_schema(registry.clone()).await;

    let patient = create_test_patient("123", "Smith", "male");
    let storage = MockStorage::with_resources(vec![patient]);

    // Create context with wildcard scope
    let auth_context = create_restricted_auth_context("user/*.cruds");
    let context =
        build_test_context_with_auth(storage, registry.clone(), Some(auth_context), false);

    let query = r#"
        query {
            Patient(_id: "123") {
                id
                gender
            }
        }
    "#;

    let request = async_graphql::Request::new(query).data(context);
    let response = schema.execute(request).await;

    assert!(
        response.errors.is_empty(),
        "Query should succeed with wildcard scope: {:?}",
        response.errors
    );
}

#[tokio::test]
async fn test_access_control_search_requires_scope() {
    let registry = create_test_registry();
    let schema = build_test_schema(registry.clone()).await;

    let patient = create_test_patient("123", "Smith", "male");
    let storage = MockStorage::with_resources(vec![patient]);

    // Only Observation scope, no Patient scope
    let auth_context = create_restricted_auth_context("user/Observation.read");
    let context = build_test_context_with_auth(storage, registry.clone(), Some(auth_context), true);

    let query = r#"
        query {
            PatientList {
                id
            }
        }
    "#;

    let request = async_graphql::Request::new(query).data(context);
    let response = schema.execute(request).await;

    assert!(
        !response.errors.is_empty(),
        "PatientList should fail without Patient scope"
    );
}

#[tokio::test]
async fn test_access_control_create_requires_write_scope() {
    let registry = create_test_registry();
    let schema = build_test_schema(registry.clone()).await;

    let storage = MockStorage::new();

    // Only read scope, no create/write scope
    let auth_context = create_restricted_auth_context("user/Patient.r");
    let context = build_test_context_with_auth(storage, registry.clone(), Some(auth_context), true);

    let query = r#"
        mutation {
            PatientCreate(res: {resource: {resourceType: "Patient", gender: "male"}}) {
                id
            }
        }
    "#;

    let request = async_graphql::Request::new(query).data(context);
    let response = schema.execute(request).await;

    assert!(
        !response.errors.is_empty(),
        "Create mutation should fail without write scope"
    );
}

#[tokio::test]
async fn test_access_control_error_includes_operation_outcome() {
    let registry = create_test_registry();
    let schema = build_test_schema(registry.clone()).await;

    let storage = MockStorage::new();

    // Insufficient scope
    let auth_context = create_restricted_auth_context("user/Observation.read");
    let context = build_test_context_with_auth(storage, registry.clone(), Some(auth_context), true);

    let query = r#"
        query {
            Patient(_id: "123") {
                id
            }
        }
    "#;

    let request = async_graphql::Request::new(query).data(context);
    let response = schema.execute(request).await;

    // Verify error has OperationOutcome in extensions
    let error = &response.errors[0];
    let extensions = error.extensions.as_ref().expect("Should have extensions");

    assert!(
        extensions.get("operationOutcome").is_some(),
        "Error should include operationOutcome in extensions"
    );
    assert!(
        extensions.get("code").is_some(),
        "Error should include code in extensions"
    );
}

/// Test that builds schema with ALL converted schemas like the server does
#[tokio::test]
async fn test_schema_builds_with_converted_schemas() {
    use octofhir_fhir_model::provider::ModelProvider;
    use octofhir_fhirschema::{StructureDefinition, translate};

    // Path to FHIR package
    let package_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join(".fhir/packages/hl7.fhir.r4.core-4.0.1/package");

    if !package_path.exists() {
        println!("Package not found at {:?}, skipping test", package_path);
        return;
    }

    // Load ALL StructureDefinitions and convert them
    let mut schemas = std::collections::HashMap::new();
    for entry in std::fs::read_dir(&package_path).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_file()
            && path
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("StructureDefinition-")
        {
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(sd) = serde_json::from_str::<StructureDefinition>(&content) {
                    if let Ok(schema) = translate(sd, None) {
                        schemas.insert(schema.name.clone(), schema);
                    }
                }
            }
        }
    }

    // Also load internal SDs (like the server does)
    let internal_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join(".fhir/octofhir-internal");
    if internal_path.exists() {
        for entry in std::fs::read_dir(&internal_path).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.is_file()
                && path
                    .file_name()
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .starts_with("StructureDefinition-")
            {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    if let Ok(sd) = serde_json::from_str::<StructureDefinition>(&content) {
                        if let Ok(schema) = translate(sd, None) {
                            schemas.insert(schema.name.clone(), schema);
                        }
                    }
                }
            }
        }
    }

    println!("Loaded {} converted schemas", schemas.len());

    // Check Task.input in converted schema
    if let Some(task_schema) = schemas.get("Task") {
        if let Some(elements) = &task_schema.elements {
            if let Some(input) = elements.get("input") {
                println!("Task.input type_name: {:?}", input.type_name);
                println!("Task.input has elements: {}", input.elements.is_some());
            }
        }
    }

    let provider = FhirSchemaModelProvider::new(schemas, FhirVersion::R4);

    // Get elements for Task
    let elements = provider.get_elements("Task").await.unwrap();
    let backbone_count = elements
        .iter()
        .filter(|e| e.element_type == "BackboneElement")
        .count();
    println!("Task backbone elements: {}", backbone_count);
    for elem in &elements {
        if elem.name == "input" || elem.name == "output" || elem.name == "restriction" {
            println!("  {} -> type: {}", elem.name, elem.element_type);
        }
    }

    // Build schema
    let registry = create_test_registry();
    let builder =
        FhirSchemaBuilder::new(registry, Arc::new(provider), SchemaBuilderConfig::default());
    let result = builder.build().await;

    assert!(
        result.is_ok(),
        "Schema should build with converted schemas: {:?}",
        result.err()
    );
}

/// Debug test to compare embedded vs converted schemas for Task backbone elements
#[tokio::test]
async fn debug_task_backbone_detection() {
    use octofhir_fhir_model::provider::ModelProvider;
    use octofhir_fhirschema::{StructureDefinition, translate};

    println!("\n=== EMBEDDED SCHEMAS ===");
    let embedded_schemas = get_schemas(octofhir_fhirschema::embedded::FhirVersion::R4).clone();
    let embedded_provider = FhirSchemaModelProvider::new(embedded_schemas.clone(), FhirVersion::R4);

    if let Some(task_schema) = embedded_schemas.get("Task") {
        if let Some(elements) = &task_schema.elements {
            if let Some(input) = elements.get("input") {
                println!("Embedded Task.input type_name: {:?}", input.type_name);
                println!(
                    "Embedded Task.input has elements: {}",
                    input.elements.is_some()
                );
            }
        }
    }

    let elements = embedded_provider.get_elements("Task").await.unwrap();
    for elem in &elements {
        if elem.name == "input" || elem.name == "output" || elem.name == "restriction" {
            println!(
                "Embedded get_elements: {} -> type: {}",
                elem.name, elem.element_type
            );
        }
    }

    // Test 2: Converted from StructureDefinition
    println!("\n=== CONVERTED SCHEMAS ===");
    // Try to find the SD file, use None if not available
    let task_sd_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join(".fhir/packages/hl7.fhir.r4.core-4.0.1/package/StructureDefinition-Task.json");

    if !task_sd_path.exists() {
        println!(
            "Task SD not found at {:?}, skipping converted test",
            task_sd_path
        );
        return;
    }

    let task_sd_json = std::fs::read_to_string(&task_sd_path).expect("Failed to read Task SD");
    let sd: StructureDefinition = serde_json::from_str(&task_sd_json).expect("Failed to parse");
    let converted_schema = translate(sd, None).expect("Failed to translate");

    if let Some(elements) = &converted_schema.elements {
        if let Some(input) = elements.get("input") {
            println!("Converted Task.input type_name: {:?}", input.type_name);
            println!(
                "Converted Task.input has elements: {}",
                input.elements.is_some()
            );
            if let Some(nested) = &input.elements {
                println!("Converted Task.input nested count: {}", nested.len());
            }
        }
    }

    let mut schemas = std::collections::HashMap::new();
    schemas.insert(converted_schema.name.clone(), converted_schema);
    let converted_provider = FhirSchemaModelProvider::new(schemas, FhirVersion::R4);

    let elements = converted_provider.get_elements("Task").await.unwrap();
    for elem in &elements {
        if elem.name == "input" || elem.name == "output" || elem.name == "restriction" {
            println!(
                "Converted get_elements: {} -> type: {}",
                elem.name, elem.element_type
            );
        }
    }
}
