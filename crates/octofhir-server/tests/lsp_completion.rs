//! LSP completion integration tests against a real PostgreSQL schema.

use async_lsp::lsp_types::{
    CompletionParams, CompletionResponse, DidOpenTextDocumentParams, Position,
    TextDocumentIdentifier, TextDocumentItem, TextDocumentPositionParams, Url,
};
use async_lsp::{ClientSocket, LanguageServer};
use octofhir_fhir_model::provider::FhirVersion as ModelFhirVersion;
use octofhir_server::lsp::PostgresLspServer;
use octofhir_server::model_provider::OctoFhirModelProvider;
use sqlx_core::executor::Executor;
use sqlx_core::migrate::Migrator;
use sqlx_core::query::query;
use sqlx_postgres::PgPoolOptions;
use std::path::Path;
use std::sync::Arc;
use testcontainers::{ContainerAsync, ImageExt, runners::AsyncRunner};
use testcontainers_modules::postgres::Postgres;
use tokio::sync::{Mutex, OnceCell};
use uuid::Uuid;

/// Shared test infrastructure: postgres container, pool, and model provider
struct TestInfra {
    #[allow(dead_code)]
    container: ContainerAsync<Postgres>,
    pool: Arc<sqlx_postgres::PgPool>,
    model_provider: Arc<OctoFhirModelProvider>,
}

static SHARED_INFRA: OnceCell<Arc<Mutex<TestInfra>>> = OnceCell::const_new();

/// Default FHIR version for tests - matches server default
const TEST_FHIR_VERSION: ModelFhirVersion = ModelFhirVersion::R4;

/// Get shared test infrastructure (postgres container, pool, model provider)
async fn get_test_infra() -> Arc<Mutex<TestInfra>> {
    SHARED_INFRA
        .get_or_init(|| async {
            let container = Postgres::default()
                .with_tag("17-alpine")
                .start()
                .await
                .expect("start postgres container");

            let host_port = container.get_host_port_ipv4(5432).await.expect("get port");
            let url = format!(
                "postgres://postgres:postgres@127.0.0.1:{}/postgres",
                host_port
            );

            let pool = Arc::new(
                PgPoolOptions::new()
                    .max_connections(5)
                    .connect(&url)
                    .await
                    .expect("connect to postgres"),
            );

            // Run migrations
            let migrator = Migrator::new(Path::new("../octofhir-db-postgres/migrations"))
                .await
                .expect("load migrations");
            migrator
                .run(pool.as_ref())
                .await
                .expect("run migrations");

            // Insert minimal FHIR schemas for testing
            setup_test_fhir_schemas(pool.as_ref()).await;

            // Create shared model provider - same pattern as server startup
            let model_provider = Arc::new(OctoFhirModelProvider::new(
                pool.as_ref().clone(),
                TEST_FHIR_VERSION,
                500, // Same cache size as server
            ));

            Arc::new(Mutex::new(TestInfra {
                container,
                pool,
                model_provider,
            }))
        })
        .await
        .clone()
}

/// Setup minimal FHIR schemas for LSP completion testing.
async fn setup_test_fhir_schemas(pool: &sqlx_postgres::PgPool) {
    // Create a test package first
    pool.execute(
        r#"
        INSERT INTO fcm.packages (name, version, fhir_version, manifest_hash, priority)
        VALUES ('hl7.fhir.r4.core', '4.0.1', 'R4', 'test-hash', 0)
        ON CONFLICT (name, version) DO NOTHING
        "#,
    )
    .await
    .expect("insert test package");

    // Insert Patient schema with essential elements
    // Note: kind and class are required fields in FhirSchema
    let patient_schema = serde_json::json!({
        "name": "Patient",
        "type": "Patient",
        "url": "http://hl7.org/fhir/StructureDefinition/Patient",
        "kind": "resource",
        "class": "DomainResource",
        "derivation": "specialization",
        "elements": {
            "id": {"type": "id", "short": "Logical id of this artifact"},
            "name": {"type": "HumanName", "array": true, "short": "A name associated with the patient"},
            "active": {"type": "boolean", "short": "Whether this patient record is in active use"},
            "identifier": {"type": "Identifier", "array": true, "short": "An identifier for this patient"},
            "birthDate": {"type": "date", "short": "The date of birth for the individual"}
        }
    });

    query(
        r#"
        INSERT INTO fcm.fhirschemas (url, version, package_name, package_version, fhir_version, schema_type, content, content_hash)
        VALUES ($1, '4.0.1', 'hl7.fhir.r4.core', '4.0.1', 'R4', 'resource', $2, 'test-hash-patient')
        ON CONFLICT (url, package_name, package_version) DO NOTHING
        "#,
    )
    .bind("http://hl7.org/fhir/StructureDefinition/Patient")
    .bind(&patient_schema)
    .execute(pool)
    .await
    .expect("insert Patient schema");

    // Insert HumanName schema for nested path testing
    let humanname_schema = serde_json::json!({
        "name": "HumanName",
        "type": "HumanName",
        "url": "http://hl7.org/fhir/StructureDefinition/HumanName",
        "kind": "complex-type",
        "class": "Element",
        "derivation": "specialization",
        "elements": {
            "use": {"type": "code", "short": "usual | official | temp | nickname | anonymous | old | maiden"},
            "family": {"type": "string", "short": "Family name (often called 'Surname')"},
            "given": {"type": "string", "array": true, "short": "Given names (not always 'first')"},
            "prefix": {"type": "string", "array": true, "short": "Parts that come before the name"},
            "suffix": {"type": "string", "array": true, "short": "Parts that come after the name"},
            "period": {"type": "Period", "short": "Time period when name was/is in use"}
        }
    });

    query(
        r#"
        INSERT INTO fcm.fhirschemas (url, version, package_name, package_version, fhir_version, schema_type, content, content_hash)
        VALUES ($1, '4.0.1', 'hl7.fhir.r4.core', '4.0.1', 'R4', 'complex-type', $2, 'test-hash-humanname')
        ON CONFLICT (url, package_name, package_version) DO NOTHING
        "#,
    )
    .bind("http://hl7.org/fhir/StructureDefinition/HumanName")
    .bind(&humanname_schema)
    .execute(pool)
    .await
    .expect("insert HumanName schema");
}

/// Get shared pool and model provider for tests
async fn get_shared_resources() -> (Arc<sqlx_postgres::PgPool>, Arc<OctoFhirModelProvider>) {
    let infra = get_test_infra().await;
    let guard = infra.lock().await;
    (guard.pool.clone(), guard.model_provider.clone())
}

fn completion_items(response: CompletionResponse) -> Vec<async_lsp::lsp_types::CompletionItem> {
    match response {
        CompletionResponse::List(list) => list.items,
        CompletionResponse::Array(items) => items,
    }
}

#[tokio::test]
async fn model_provider_loads_patient_schema() {
    let (_, model_provider) = get_shared_resources().await;

    // Verify we can load the Patient schema from the database
    let schema = model_provider.get_schema("Patient").await;
    assert!(
        schema.is_some(),
        "Expected to load Patient schema from database, fhir_version_str = {}",
        model_provider.fhir_version_str()
    );

    let schema = schema.unwrap();
    assert_eq!(schema.name, "Patient");

    // Check that elements are present
    let elements = schema.elements.as_ref().expect("Patient should have elements");
    assert!(elements.contains_key("name"), "Patient should have 'name' element");

    // Check that the name element has type HumanName
    let name_element = elements.get("name").expect("name element");
    assert_eq!(name_element.type_name.as_deref(), Some("HumanName"));

    // Verify we can also load HumanName
    let humanname = model_provider.get_schema("HumanName").await;
    assert!(
        humanname.is_some(),
        "Expected to load HumanName schema from database"
    );
    let humanname = humanname.unwrap();
    let elements = humanname.elements.as_ref().expect("HumanName should have elements");
    assert!(elements.contains_key("family"), "HumanName should have 'family' element");
}

#[tokio::test]
async fn jsonb_schema_fields_at_path_test() {
    use mold_completion::types::{JsonbField, JsonbFieldType, JsonbSchema};

    // Create a mock JSONB schema that mimics what would be built from FHIR schemas
    let mut humanname_schema = JsonbSchema::new();
    humanname_schema = humanname_schema.with_field(JsonbField::new("family".to_string(), JsonbFieldType::String));
    humanname_schema = humanname_schema.with_field(JsonbField::new("given".to_string(), JsonbFieldType::Array));
    humanname_schema = humanname_schema.with_field(JsonbField::new("use".to_string(), JsonbFieldType::String));

    let mut patient_schema = JsonbSchema::new();
    patient_schema = patient_schema.with_field(JsonbField::new("id".to_string(), JsonbFieldType::String));
    patient_schema = patient_schema.with_field(JsonbField::new("active".to_string(), JsonbFieldType::Boolean));
    patient_schema = patient_schema.with_field(
        JsonbField::new("name".to_string(), JsonbFieldType::Array)
            .with_nested(humanname_schema)
    );

    // Test 1: Empty path returns top-level fields (Patient fields)
    let fields = patient_schema.fields_at_path(&[]);
    let names: Vec<_> = fields.iter().map(|f| f.name.as_str()).collect();
    assert!(names.contains(&"name"), "Empty path should return Patient fields: {:?}", names);

    // Test 2: Path ["name"] should return HumanName fields (nested)
    let fields = patient_schema.fields_at_path(&["name".to_string()]);
    let names: Vec<_> = fields.iter().map(|f| f.name.as_str()).collect();
    assert!(names.contains(&"family"), "Path ['name'] should return HumanName fields: {:?}", names);
    assert!(names.contains(&"given"), "Path ['name'] should return HumanName fields: {:?}", names);

    // Test 3: Path ["name", "0"] should skip numeric index and return HumanName fields
    let fields = patient_schema.fields_at_path(&["name".to_string(), "0".to_string()]);
    let names: Vec<_> = fields.iter().map(|f| f.name.as_str()).collect();
    assert!(names.contains(&"family"), "Path ['name', '0'] should return HumanName fields: {:?}", names);
}

#[tokio::test]
async fn lsp_completion_suggests_tables_after_from() {
    let (pool, model_provider) = get_shared_resources().await;

    let table_name = format!("lsp_table_{}", Uuid::new_v4().simple());
    let create_table = format!(
        "CREATE TABLE IF NOT EXISTS {table} (id text, resource jsonb)",
        table = table_name
    );
    pool.as_ref()
        .execute(create_table.as_str())
        .await
        .expect("create table");

    let client = ClientSocket::new_closed();
    let mut server = PostgresLspServer::new(client, pool.clone(), model_provider);

    let uri = Url::parse("file:///lsp_completion_from.sql").expect("uri");
    let sql = "SELECT * FROM ".to_string();

    let _ = server.did_open(DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "sql".to_string(),
            version: 1,
            text: sql.clone(),
        },
    });

    let table_completion = server
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position: Position {
                    line: 0,
                    character: sql.len() as u32,
                },
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            context: None,
        })
        .await
        .expect("completion")
        .expect("completion response");

    let items = completion_items(table_completion);
    assert!(
        items.iter().any(|item| item.label == table_name),
        "expected table completion for {table_name}"
    );
}

#[tokio::test]
async fn lsp_completion_filters_tables_by_prefix() {
    let (pool, model_provider) = get_shared_resources().await;

    let table_pat = format!("lsp_pat_{}", Uuid::new_v4().simple());
    let table_other = format!("lsp_other_{}", Uuid::new_v4().simple());

    let create_table = format!(
        "CREATE TABLE IF NOT EXISTS {table} (id text, resource jsonb)",
        table = table_pat
    );
    pool.as_ref()
        .execute(create_table.as_str())
        .await
        .expect("create table");

    let create_table = format!(
        "CREATE TABLE IF NOT EXISTS {table} (id text, resource jsonb)",
        table = table_other
    );
    pool.as_ref()
        .execute(create_table.as_str())
        .await
        .expect("create table");

    let client = ClientSocket::new_closed();
    let mut server = PostgresLspServer::new(client, pool.clone(), model_provider);

    let uri = Url::parse("file:///lsp_completion_prefix.sql").expect("uri");
    let sql = format!("SELECT * FROM {table}", table = "lsp_pat_");

    let _ = server.did_open(DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "sql".to_string(),
            version: 1,
            text: sql.clone(),
        },
    });

    let completion = server
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position: Position {
                    line: 0,
                    character: sql.len() as u32,
                },
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            context: None,
        })
        .await
        .expect("completion")
        .expect("completion response");

    let items = completion_items(completion);
    assert!(
        items.iter().any(|item| item.label == table_pat),
        "expected table completion for {table_pat}"
    );
    assert!(
        !items.iter().any(|item| item.label == table_other),
        "did not expect table completion for {table_other}"
    );
}

#[tokio::test]
async fn lsp_completion_filters_tables_by_schema_prefix() {
    let (pool, model_provider) = get_shared_resources().await;

    let table_name = format!("lsp_schema_pat_{}", Uuid::new_v4().simple());
    let create_table = format!(
        "CREATE TABLE IF NOT EXISTS public.{table} (id text, resource jsonb)",
        table = table_name
    );
    pool.as_ref()
        .execute(create_table.as_str())
        .await
        .expect("create table");

    let client = ClientSocket::new_closed();
    let mut server = PostgresLspServer::new(client, pool.clone(), model_provider);

    let uri = Url::parse("file:///lsp_completion_schema_prefix.sql").expect("uri");
    let sql = "SELECT * FROM public.lsp_schema_".to_string();

    let _ = server.did_open(DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "sql".to_string(),
            version: 1,
            text: sql.clone(),
        },
    });

    let completion = server
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position: Position {
                    line: 0,
                    character: sql.len() as u32,
                },
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            context: None,
        })
        .await
        .expect("completion")
        .expect("completion response");

    let items = completion_items(completion);
    assert!(
        items.iter().any(|item| item.label == table_name),
        "expected table completion for {table_name}"
    );
}

#[tokio::test]
async fn lsp_completion_resolves_alias_columns() {
    let (pool, model_provider) = get_shared_resources().await;

    let table_name = format!("lsp_alias_{}", Uuid::new_v4().simple());
    let create_table = format!(
        "CREATE TABLE IF NOT EXISTS {table} (id text, name text, resource jsonb)",
        table = table_name
    );
    pool.as_ref()
        .execute(create_table.as_str())
        .await
        .expect("create table");

    let client = ClientSocket::new_closed();
    let mut server = PostgresLspServer::new(client, pool.clone(), model_provider);

    let uri = Url::parse("file:///lsp_completion_alias.sql").expect("uri");
    let sql = format!("SELECT p. FROM {table} p", table = table_name);
    let dot_pos = sql.find("p.").expect("dot position") + 2;

    let _ = server.did_open(DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "sql".to_string(),
            version: 1,
            text: sql.clone(),
        },
    });

    let completion = server
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position: Position {
                    line: 0,
                    character: dot_pos as u32,
                },
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            context: None,
        })
        .await
        .expect("completion")
        .expect("completion response");

    let items = completion_items(completion);
    assert!(items.iter().any(|item| item.label == "id"));
    assert!(items.iter().any(|item| item.label == "name"));
}

#[tokio::test(flavor = "multi_thread")]
async fn lsp_completion_suggests_tables_after_join_update_insert() {
    let (pool, model_provider) = get_shared_resources().await;

    let table_left = format!("lsp_join_left_{}", Uuid::new_v4().simple());
    let table_right = format!("lsp_join_right_{}", Uuid::new_v4().simple());

    let create_table = format!(
        "CREATE TABLE IF NOT EXISTS {table} (id text, resource jsonb)",
        table = table_left
    );
    pool.as_ref()
        .execute(create_table.as_str())
        .await
        .expect("create table");

    let create_table = format!(
        "CREATE TABLE IF NOT EXISTS {table} (id text, resource jsonb)",
        table = table_right
    );
    pool.as_ref()
        .execute(create_table.as_str())
        .await
        .expect("create table");

    let client = ClientSocket::new_closed();
    let mut server = PostgresLspServer::new(client, pool.clone(), model_provider);

    let uri = Url::parse("file:///lsp_completion_join.sql").expect("uri");
    let sql = format!("SELECT * FROM {table} JOIN ", table = table_left);

    let _ = server.did_open(DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "sql".to_string(),
            version: 1,
            text: sql.clone(),
        },
    });

    let completion = server
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position: Position {
                    line: 0,
                    character: sql.len() as u32,
                },
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            context: None,
        })
        .await
        .expect("completion")
        .expect("completion response");

    let items = completion_items(completion);
    assert!(
        items.iter().any(|item| item.label == table_right),
        "expected table completion for {table_right}"
    );

    let uri = Url::parse("file:///lsp_completion_update.sql").expect("uri");
    let sql = "UPDATE ".to_string();

    let _ = server.did_open(DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "sql".to_string(),
            version: 1,
            text: sql.clone(),
        },
    });

    let completion = server
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position: Position {
                    line: 0,
                    character: sql.len() as u32,
                },
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            context: None,
        })
        .await
        .expect("completion")
        .expect("completion response");

    let items = completion_items(completion);
    assert!(
        items.iter().any(|item| item.label == table_left),
        "expected table completion for {table_left}"
    );

    let uri = Url::parse("file:///lsp_completion_insert.sql").expect("uri");
    let sql = "INSERT INTO ".to_string();

    let _ = server.did_open(DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "sql".to_string(),
            version: 1,
            text: sql.clone(),
        },
    });

    let completion = server
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position: Position {
                    line: 0,
                    character: sql.len() as u32,
                },
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            context: None,
        })
        .await
        .expect("completion")
        .expect("completion response");

    let items = completion_items(completion);
    assert!(
        items.iter().any(|item| item.label == table_left),
        "expected table completion for {table_left}"
    );
}

#[tokio::test]
async fn lsp_completion_suggests_columns_in_where_and_order_by() {
    let (pool, model_provider) = get_shared_resources().await;

    let table_name = format!("lsp_where_{}", Uuid::new_v4().simple());
    let create_table = format!(
        "CREATE TABLE IF NOT EXISTS {table} (id text, name text, resource jsonb)",
        table = table_name
    );
    pool.as_ref()
        .execute(create_table.as_str())
        .await
        .expect("create table");

    let client = ClientSocket::new_closed();
    let mut server = PostgresLspServer::new(client, pool.clone(), model_provider);

    let uri = Url::parse("file:///lsp_completion_where.sql").expect("uri");
    let sql = format!("SELECT * FROM {table} WHERE ", table = table_name);

    let _ = server.did_open(DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "sql".to_string(),
            version: 1,
            text: sql.clone(),
        },
    });

    let completion = server
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position: Position {
                    line: 0,
                    character: sql.len() as u32,
                },
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            context: None,
        })
        .await
        .expect("completion")
        .expect("completion response");

    let items = completion_items(completion);
    assert!(items.iter().any(|item| item.label == "id"));
    assert!(items.iter().any(|item| item.label == "name"));

    let uri = Url::parse("file:///lsp_completion_order_by.sql").expect("uri");
    let sql = format!("SELECT * FROM {table} ORDER BY ", table = table_name);

    let _ = server.did_open(DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "sql".to_string(),
            version: 1,
            text: sql.clone(),
        },
    });

    let completion = server
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position: Position {
                    line: 0,
                    character: sql.len() as u32,
                },
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            context: None,
        })
        .await
        .expect("completion")
        .expect("completion response");

    let items = completion_items(completion);
    assert!(items.iter().any(|item| item.label == "id"));
    assert!(items.iter().any(|item| item.label == "name"));
}

#[tokio::test(flavor = "multi_thread")]
async fn lsp_completion_suggests_jsonb_paths() {
    let (pool, model_provider) = get_shared_resources().await;

    let create_table = "CREATE TABLE IF NOT EXISTS patient (id text, resource jsonb)";
    pool.as_ref()
        .execute(create_table)
        .await
        .expect("create table");

    let client = ClientSocket::new_closed();
    let mut server = PostgresLspServer::new(client, pool.clone(), model_provider);

    let uri = Url::parse("file:///lsp_completion_jsonb.sql").expect("uri");
    let sql = "SELECT resource -> 'na' FROM patient".to_string();
    let offset = sql.find("'na'").expect("offset") + 3;

    let _ = server.did_open(DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "sql".to_string(),
            version: 1,
            text: sql.clone(),
        },
    });

    let completion = server
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position: Position {
                    line: 0,
                    character: offset as u32,
                },
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            context: None,
        })
        .await
        .expect("completion")
        .expect("completion response");

    let items = completion_items(completion);
    assert!(
        items.iter().any(|item| item.label == "name"),
        "expected JSONB path completion for name"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn lsp_completion_suggests_nested_jsonb_paths() {
    let (pool, model_provider) = get_shared_resources().await;

    let create_table = "CREATE TABLE IF NOT EXISTS patient (id text, resource jsonb)";
    pool.as_ref()
        .execute(create_table)
        .await
        .expect("create table");

    let client = ClientSocket::new_closed();
    let mut server = PostgresLspServer::new(client, pool.clone(), model_provider);

    let uri = Url::parse("file:///lsp_completion_jsonb_nested.sql").expect("uri");
    let sql = "SELECT patient.resource->'name'->0->'fa' FROM patient".to_string();
    let offset = sql.find("'fa'").expect("offset") + 3;

    let _ = server.did_open(DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "sql".to_string(),
            version: 1,
            text: sql.clone(),
        },
    });

    let completion = server
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position: Position {
                    line: 0,
                    character: offset as u32,
                },
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            context: None,
        })
        .await
        .expect("completion")
        .expect("completion response");

    let items = completion_items(completion);
    let labels: Vec<_> = items.iter().map(|item| item.label.as_str()).collect();
    assert!(
        items.iter().any(|item| item.label == "family"),
        "expected JSONB path completion for family, got: {:?}",
        labels
    );
}

/// Tests completion for #> operator with PostgreSQL array literal syntax.
/// Query: SELECT * FROM patient WHERE resource#>'{na}'
#[tokio::test(flavor = "multi_thread")]
async fn lsp_completion_suggests_jsonb_paths_with_hash_arrow() {
    let (pool, model_provider) = get_shared_resources().await;

    let create_table = "CREATE TABLE IF NOT EXISTS patient (id text, resource jsonb)";
    pool.as_ref()
        .execute(create_table)
        .await
        .expect("create table");

    let client = ClientSocket::new_closed();
    let mut server = PostgresLspServer::new(client, pool.clone(), model_provider);

    let uri = Url::parse("file:///lsp_completion_jsonb_hash_arrow.sql").expect("uri");
    // Test #> operator with array literal syntax
    let sql = "SELECT * FROM patient WHERE resource#>'{na}'".to_string();
    // Position cursor inside the array literal, after 'na'
    let offset = sql.find("na}'").expect("offset") + 2;

    let _ = server.did_open(DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "sql".to_string(),
            version: 1,
            text: sql.clone(),
        },
    });

    let completion = server
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position: Position {
                    line: 0,
                    character: offset as u32,
                },
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            context: None,
        })
        .await
        .expect("completion")
        .expect("completion response");

    let items = completion_items(completion);
    let labels: Vec<_> = items.iter().map(|item| item.label.as_str()).collect();

    // Should suggest 'name' field from Patient schema
    assert!(
        items.iter().any(|item| item.label == "name"),
        "expected JSONB path completion for 'name' with #> operator, got: {:?}",
        labels
    );
}

/// Tests completion for nested paths with #> operator.
/// Query: SELECT * FROM patient WHERE resource#>'{name,0,fa}'
#[tokio::test(flavor = "multi_thread")]
async fn lsp_completion_suggests_nested_jsonb_paths_with_hash_arrow() {
    let (pool, model_provider) = get_shared_resources().await;

    let create_table = "CREATE TABLE IF NOT EXISTS patient (id text, resource jsonb)";
    pool.as_ref()
        .execute(create_table)
        .await
        .expect("create table");

    let client = ClientSocket::new_closed();
    let mut server = PostgresLspServer::new(client, pool.clone(), model_provider);

    let uri = Url::parse("file:///lsp_completion_jsonb_hash_arrow_nested.sql").expect("uri");
    // Test nested path with #> operator
    let sql = "SELECT * FROM patient WHERE resource#>'{name,0,fa}'".to_string();
    // Position cursor inside the array literal, after 'fa'
    let offset = sql.find("fa}'").expect("offset") + 2;

    let _ = server.did_open(DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "sql".to_string(),
            version: 1,
            text: sql.clone(),
        },
    });

    let completion = server
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position: Position {
                    line: 0,
                    character: offset as u32,
                },
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            context: None,
        })
        .await
        .expect("completion")
        .expect("completion response");

    let items = completion_items(completion);
    let labels: Vec<_> = items.iter().map(|item| item.label.as_str()).collect();

    // Should suggest 'family' field from HumanName schema (nested under name)
    assert!(
        items.iter().any(|item| item.label == "family"),
        "expected JSONB path completion for 'family' with nested #> path, got: {:?}",
        labels
    );
}

#[tokio::test]
async fn lsp_completion_returns_tables_columns_and_functions() {
    let (pool, model_provider) = get_shared_resources().await;

    let table_name = format!("lsp_patient_{}", Uuid::new_v4().simple());
    let function_name = format!("lsp_fn_{}", Uuid::new_v4().simple());

    let create_table = format!(
        "CREATE TABLE IF NOT EXISTS {table} (id text, name text, resource jsonb)",
        table = table_name
    );
    pool.as_ref()
        .execute(create_table.as_str())
        .await
        .expect("create table");

    let create_function = format!(
        "CREATE OR REPLACE FUNCTION {func}(input text) RETURNS text LANGUAGE sql AS $$ SELECT input $$",
        func = function_name
    );
    pool.as_ref()
        .execute(create_function.as_str())
        .await
        .expect("create function");

    let client = ClientSocket::new_closed();
    let mut server = PostgresLspServer::new(client, pool.clone(), model_provider);

    let uri = Url::parse("file:///lsp_completion_tables.sql").expect("uri");
    let sql = "SELECT * FROM ".to_string();

    let _ = server.did_open(DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "sql".to_string(),
            version: 1,
            text: sql.clone(),
        },
    });

    let table_completion = server
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position: Position {
                    line: 0,
                    character: sql.len() as u32,
                },
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            context: None,
        })
        .await
        .expect("completion")
        .expect("completion response");

    let items = completion_items(table_completion);
    assert!(
        items.iter().any(|item| item.label == table_name),
        "expected table completion for {table_name}"
    );

    let uri = Url::parse("file:///lsp_completion_columns.sql").expect("uri");
    let sql = format!("SELECT {table}. FROM {table}", table = table_name);
    let dot_pos = sql
        .find(&format!("{table}.", table = table_name))
        .expect("dot position")
        + table_name.len()
        + 1;

    let _ = server.did_open(DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "sql".to_string(),
            version: 1,
            text: sql.clone(),
        },
    });

    let column_completion = server
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position: Position {
                    line: 0,
                    character: dot_pos as u32,
                },
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            context: None,
        })
        .await
        .expect("completion")
        .expect("completion response");

    let items = completion_items(column_completion);
    assert!(items.iter().any(|item| item.label == "id"));
    assert!(items.iter().any(|item| item.label == "name"));
    assert!(items.iter().any(|item| item.label == "resource"));

    let uri = Url::parse("file:///lsp_completion_functions.sql").expect("uri");
    let sql = "SELECT ".to_string();

    let _ = server.did_open(DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "sql".to_string(),
            version: 1,
            text: sql.clone(),
        },
    });

    let function_completion = server
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position: Position {
                    line: 0,
                    character: sql.len() as u32,
                },
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            context: None,
        })
        .await
        .expect("completion")
        .expect("completion response");

    let items = completion_items(function_completion);
    assert!(
        items.iter().any(|item| item.label == function_name),
        "expected function completion for {function_name}"
    );
}
