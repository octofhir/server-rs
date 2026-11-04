//! `$sql-describe` (the console's view of a `$sql-run` subject): result
//! columns, dependencies resolved the way `$sql-run` resolves them (stored
//! and inline ViewDefinitions, a SQLView, a stored ValueSet with its
//! provenance), a SQL that does not prepare reported as `check`, and an
//! unresolvable dependency answered 404 as `$sql-run` does.
//!
//! Requirements: Docker running (testcontainers).

use std::sync::Arc;

use octofhir_config::ConfigurationManager;
use octofhir_server::{AppConfig, PostgresStorageConfig, build_app};
use reqwest::{Client, StatusCode};
use serde_json::{Value, json};
use testcontainers::{ImageExt, runners::AsyncRunner};
use testcontainers_modules::postgres::Postgres;

struct ServerHandle(tokio::task::JoinHandle<()>);

impl Drop for ServerHandle {
    fn drop(&mut self) {
        self.0.abort();
    }
}

async fn start_server(postgres_url: &str) -> (String, ServerHandle) {
    let mut config = AppConfig::default();
    config.storage.postgres = Some(PostgresStorageConfig {
        url: Some(postgres_url.to_owned()),
        pool_size: 5,
        connect_timeout_ms: 10_000,
        idle_timeout_ms: Some(60_000),
        ..Default::default()
    });
    config.auth.policy.anonymous_access = true;
    config.sql_on_fhir.enabled = true;
    config.packages.load = vec![octofhir_server::config::PackageSpec::Table {
        id: Some("hl7.fhir.r4.core".into()),
        version: Some("4.0.1".into()),
        path: None,
        url: Some("https://hl7.org/fhir/R4/hl7.fhir.r4.core.tgz".into()),
    }];
    let config_manager = Arc::new(
        ConfigurationManager::builder()
            .build()
            .await
            .expect("config manager"),
    );
    let app = build_app(&config, config_manager).await.expect("build app");
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("bind");
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (format!("http://{addr}/fhir"), ServerHandle(server))
}

async fn put(client: &Client, fhir: &str, resource: Value) {
    let path = format!(
        "{fhir}/{}/{}",
        resource["resourceType"].as_str().unwrap(),
        resource["id"].as_str().unwrap()
    );
    let response = client.put(path).json(&resource).send().await.unwrap();
    let status = response.status();
    let body = response.text().await.unwrap();
    assert!(status.is_success(), "{status}: {body}");
}

async fn describe(client: &Client, fhir: &str, parameters: Vec<Value>) -> (StatusCode, Value) {
    let response = client
        .post(format!("{fhir}/$sql-describe"))
        .json(&json!({"resourceType": "Parameters", "parameter": parameters}))
        .send()
        .await
        .unwrap();
    let status = response.status();
    (status, response.json().await.unwrap())
}

const LIBRARY_TYPES: &str = "http://hl7.org/fhir/uv/sql-on-fhir/CodeSystem/LibraryTypesCodes";

fn sql_library(code: &str, url: &str, sql: &str, depends_on: &[(&str, &str)]) -> Value {
    use base64::Engine;
    json!({
        "resourceType": "Library",
        "url": url,
        "status": "active",
        "type": {"coding": [{"system": LIBRARY_TYPES, "code": code}]},
        "relatedArtifact": depends_on.iter().map(|(label, canonical)| json!({
            "type": "depends-on", "label": label, "resource": canonical
        })).collect::<Vec<_>>(),
        "content": [{
            "contentType": "application/sql",
            "data": base64::engine::general_purpose::STANDARD.encode(sql)
        }]
    })
}

fn subject(resource: Value) -> Value {
    json!({"name": "subjectResource", "resource": resource})
}

fn context(resource: Value) -> Value {
    json!({"name": "context", "resource": resource})
}

/// The `(name, type)` pairs of the `column` parts of `parts`.
fn columns(parts: &[Value]) -> Vec<(String, String)> {
    parts
        .iter()
        .filter(|part| part["name"] == "column")
        .map(|column| {
            let field = |name: &str| {
                column["part"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|part| part["name"] == name)
                    .and_then(|part| part["valueString"].as_str())
                    .unwrap()
                    .to_owned()
            };
            (field("name"), field("type"))
        })
        .collect()
}

fn parameters(answer: &Value) -> Vec<Value> {
    answer["parameter"].as_array().cloned().unwrap_or_default()
}

/// The `dependency` part labelled `label`.
fn dependency(answer: &Value, label: &str) -> Vec<Value> {
    parameters(answer)
        .into_iter()
        .filter(|param| param["name"] == "dependency")
        .map(|param| param["part"].as_array().unwrap().clone())
        .find(|parts| parts.iter().any(|part| part["valueString"] == label))
        .unwrap_or_else(|| panic!("no dependency {label} in {answer}"))
}

fn value<'a>(parts: &'a [Value], name: &str) -> &'a Value {
    parts
        .iter()
        .find(|part| part["name"] == name)
        .map(|part| {
            part.get("valueString")
                .or_else(|| part.get("valueCode"))
                .unwrap()
        })
        .unwrap_or_else(|| panic!("no {name} in {parts:?}"))
}

const PATIENTS: &str = "http://example.org/ViewDefinition/patients";
const ADULTS: &str = "http://example.org/Library/adults";
const CODES: &str = "http://example.org/ValueSet/cardiovascular-disease|2026";

#[tokio::test]
async fn describe_reports_columns_dependencies_and_terminology() {
    let postgres = Postgres::default()
        .with_tag("17-alpine")
        .start()
        .await
        .expect("postgres");
    let port = postgres.get_host_port_ipv4(5432).await.unwrap();
    let (fhir, _server) = start_server(&format!(
        "postgres://postgres:postgres@127.0.0.1:{port}/postgres"
    ))
    .await;
    let client = Client::new();

    let patients = json!({"resourceType": "ViewDefinition", "id": "patients", "status": "active",
        "url": PATIENTS, "name": "patients", "resource": "Patient",
        "select": [{"column": [
            {"name": "id", "path": "getResourceKey()", "type": "id"},
            {"name": "gender", "path": "gender", "type": "code"},
            {"name": "birth_date", "path": "birthDate", "type": "date"}]}]});
    put(&client, &fhir, patients.clone()).await;
    put(
        &client,
        &fhir,
        json!({
            "resourceType": "ValueSet", "id": "cvd", "status": "active",
            "url": "http://example.org/ValueSet/cardiovascular-disease", "version": "2026",
            "expansion": {"timestamp": "2026-09-16T00:00:00Z", "total": 2, "contains": [
                {"system": "http://snomed.info/sct", "code": "22298006"},
                {"system": "http://hl7.org/fhir/sid/icd-10", "code": "I21"}]}
        }),
    )
    .await;
    let adults = sql_library(
        "sql-view",
        ADULTS,
        "SELECT patient.id, patient.gender FROM patient WHERE patient.birth_date < '2008-01-01'",
        &[("patient", PATIENTS)],
    );
    let query = |sql: &str| {
        let mut library = sql_library(
            "sql-query",
            "http://example.org/Library/q",
            sql,
            &[("adults", ADULTS), ("codes", CODES)],
        );
        library["parameter"] = json!([{"name": "g", "use": "in", "type": "code"}]);
        library
    };

    // A query over a SQLView (inline context) over a stored view, and a value set.
    let (status, answer) = describe(
        &client,
        &fhir,
        vec![
            subject(query(
                "SELECT adults.id, adults.gender, codes.code FROM adults CROSS JOIN codes \
                 WHERE adults.gender = :g",
            )),
            context(adults.clone()),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert_eq!(
        columns(&parameters(&answer)),
        [
            ("id".to_owned(), "text".to_owned()),
            ("gender".to_owned(), "text".to_owned()),
            // CHARACTER VARYING, as Terminology in SQL types the relation.
            ("code".to_owned(), "varchar".to_owned()),
        ]
    );
    let view = dependency(&answer, "adults");
    assert_eq!(value(&view, "kind"), "SQLView");
    assert_eq!(value(&view, "canonical"), ADULTS);
    assert_eq!(
        columns(&view)
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>(),
        ["id", "gender"]
    );
    let codes = dependency(&answer, "codes");
    assert_eq!(value(&codes, "kind"), "ValueSet");
    assert_eq!(value(&codes, "resolved"), CODES);
    let provenance: Value =
        serde_json::from_str(value(&codes, "provenance").as_str().unwrap()).unwrap();
    assert_eq!(provenance["members"], 2, "{provenance}");
    assert_eq!(
        columns(&codes)
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>(),
        ["system", "version", "code", "display", "inactive"]
    );
    assert!(
        !parameters(&answer)
            .iter()
            .any(|param| param["name"] == "check"),
        "{answer}"
    );

    // SQL that does not prepare is a `check`, the dependencies still described.
    let (status, answer) = describe(
        &client,
        &fhir,
        vec![
            subject(query("SELECT adults.nope FROM adults")),
            context(adults.clone()),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    let check = parameters(&answer)
        .into_iter()
        .find(|param| param["name"] == "check")
        .unwrap_or_else(|| panic!("no check in {answer}"));
    assert!(
        check["valueString"].as_str().unwrap().contains("nope"),
        "{check}"
    );
    assert_eq!(value(&dependency(&answer, "codes"), "kind"), "ValueSet");

    // A construct the sandbox refuses is a `check` too.
    let (status, answer) = describe(
        &client,
        &fhir,
        vec![
            subject(query("SELECT pg_sleep(1) AS pause FROM adults")),
            context(adults),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert!(answer.to_string().contains("pg_sleep"), "{answer}");

    // A dependency that does not resolve is answered as $sql-run answers it.
    let (status, answer) = describe(&client, &fhir, vec![subject(query("SELECT 1 AS one"))]).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{answer}");

    // A ViewDefinition subject: its columns.
    let (status, answer) = describe(&client, &fhir, vec![subject(patients)]).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert_eq!(
        columns(&parameters(&answer))
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>(),
        ["id", "gender", "birth_date"]
    );
}
