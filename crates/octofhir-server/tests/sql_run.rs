//! `$sql-run` (SQL on FHIR v3) against the server's own data.
//!
//! A SQLQuery over a SQLView and ViewDefinitions (inline `context` and stored,
//! resolved by canonical), bound parameters, the data filters applied before
//! the SQL, `_format=fhir`, a ViewDefinition subject, the sandbox (other
//! tables, functions, qualified names, statements) and the 400/404/422
//! answers.
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

async fn create(client: &Client, fhir: &str, resource: Value) -> Value {
    let resource_type = resource["resourceType"].as_str().unwrap().to_owned();
    let response = client
        .post(format!("{fhir}/{resource_type}"))
        .json(&resource)
        .send()
        .await
        .unwrap();
    let status = response.status();
    let body: Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::CREATED, "{body}");
    body
}

/// POST `$viewdefinition-run` with `parameters`; the status, content type and body.
async fn run(client: &Client, url: &str, parameters: Value) -> (StatusCode, String, String) {
    let response = client
        .post(url)
        .json(&json!({"resourceType": "Parameters", "parameter": parameters}))
        .send()
        .await
        .unwrap();
    let status = response.status();
    let content_type = response
        .headers()
        .get("content-type")
        .map(|value| value.to_str().unwrap().to_owned())
        .unwrap_or_default();
    (status, content_type, response.text().await.unwrap())
}

const LIBRARY_TYPES: &str = "http://hl7.org/fhir/uv/sql-on-fhir/CodeSystem/LibraryTypesCodes";

fn sql_library(
    code: &str,
    url: &str,
    sql: &str,
    depends_on: &[(&str, &str)],
    parameters: Value,
) -> Value {
    use base64::Engine;
    let mut library = json!({
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
    });
    if parameters
        .as_array()
        .is_some_and(|parameters| !parameters.is_empty())
    {
        library["parameter"] = parameters;
    }
    if library["relatedArtifact"]
        .as_array()
        .is_some_and(Vec::is_empty)
    {
        library.as_object_mut().unwrap().remove("relatedArtifact");
    }
    library
}

#[tokio::test]
async fn sql_run_queries_views_in_a_sandbox() {
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
    let sql_run = format!("{fhir}/$sql-run");

    let mut ids = Vec::new();
    for (family, active, codes) in [
        ("Alice", true, vec!["8867-4", "8480-6"]),
        ("Bob", false, vec!["8867-4"]),
        ("Carol", true, vec![]),
    ] {
        let patient = create(
            &client,
            &fhir,
            json!({"resourceType":"Patient","active":active,"name":[{"family":family}]}),
        )
        .await;
        let id = patient["id"].as_str().unwrap().to_owned();
        for code in codes {
            create(
                &client,
                &fhir,
                json!({"resourceType":"Observation","status":"final",
                "code":{"coding":[{"system":"http://loinc.org","code":code}]},
                "subject":{"reference":format!("Patient/{id}")}}),
            )
            .await;
        }
        ids.push((family, id));
    }
    let id_of = |family: &str| ids.iter().find(|(f, _)| *f == family).unwrap().1.clone();

    let patient_view = json!({"resourceType":"ViewDefinition","status":"active",
        "url":"http://example.org/ViewDefinition/patient","name":"patient","resource":"Patient",
        "select":[{"column":[
            {"name":"id","path":"getResourceKey()","type":"id"},
            {"name":"family","path":"name.family.first()","type":"string"},
            {"name":"active","path":"active","type":"boolean"}]}]});
    let obs_view = json!({"resourceType":"ViewDefinition","status":"active",
        "url":"http://example.org/ViewDefinition/obs","name":"obs","resource":"Observation",
        "select":[{"column":[
            {"name":"subject","path":"subject.getReferenceKey(Patient)","type":"id"},
            {"name":"code","path":"code.coding.code.first()","type":"code"}]}]});
    let active_view = sql_library(
        "sql-view",
        "http://example.org/Library/ActivePatients",
        "SELECT id, family FROM patient WHERE active",
        &[("patient", "http://example.org/ViewDefinition/patient")],
        json!([]),
    );
    let query = sql_library(
        "sql-query",
        "http://example.org/Library/ObservationCounts",
        "SELECT a.family, count(o.code) AS observations \
         FROM active_patients a LEFT JOIN obs o ON o.subject = a.id \
         WHERE count_from <= :min_count OR true \
         GROUP BY a.family HAVING count(o.code) >= :min_count ORDER BY a.family",
        &[
            (
                "active_patients",
                "http://example.org/Library/ActivePatients",
            ),
            ("obs", "http://example.org/ViewDefinition/obs"),
        ],
        json!([{"name":"min_count","use":"in","type":"integer","min":1}]),
    );
    let context = |extra: Vec<Value>| -> Vec<Value> {
        let mut parameters = vec![
            json!({"name":"context","resource":patient_view}),
            json!({"name":"context","resource":obs_view}),
            json!({"name":"context","resource":active_view}),
        ];
        parameters.extend(extra);
        parameters
    };
    let min_count = |n: i64| {
        json!({"name":"parameters","resource":{"resourceType":"Parameters",
            "parameter":[{"name":"min_count","valueInteger":n}]}})
    };

    // The query references a column the dependency does not have: 422.
    let (status, _, body) = run(
        &client,
        &sql_run,
        Value::Array(context(vec![
            json!({"name":"subjectResource","resource":query}),
            min_count(0),
        ])),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(body.contains("count_from"), "{body}");

    // SQLQuery over a SQLView and a ViewDefinition, with a bound parameter.
    let query = sql_library(
        "sql-query",
        "http://example.org/Library/ObservationCounts",
        "SELECT a.family, count(o.code) AS observations \
         FROM active_patients a LEFT JOIN obs o ON o.subject = a.id \
         GROUP BY a.family HAVING count(o.code) >= :min_count ORDER BY a.family",
        &[
            (
                "active_patients",
                "http://example.org/Library/ActivePatients",
            ),
            ("obs", "http://example.org/ViewDefinition/obs"),
        ],
        json!([{"name":"min_count","use":"in","type":"integer","min":1}]),
    );
    let (status, content_type, body) = run(
        &client,
        &sql_run,
        Value::Array(context(vec![
            json!({"name":"subjectResource","resource":query}),
            min_count(0),
        ])),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(content_type, "application/x-ndjson");
    assert_eq!(
        body.lines().collect::<Vec<_>>(),
        [
            r#"{"family":"Alice","observations":2}"#,
            r#"{"family":"Carol","observations":0}"#
        ]
    );

    // _format=fhir: a Parameters with a row per row, typed by the SQL type.
    let (status, _, body) = run(
        &client,
        &sql_run,
        Value::Array(context(vec![
            json!({"name":"subjectResource","resource":query}),
            min_count(1),
            json!({"name":"_format","valueCode":"fhir"}),
        ])),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let parameters: Value = serde_json::from_str(&body).unwrap();
    // An R4 server has no integer64: a BIGINT count is a decimal.
    assert_eq!(
        parameters,
        json!({"resourceType":"Parameters","parameter":[{"name":"row","part":[
            {"name":"family","valueString":"Alice"},
            {"name":"observations","valueDecimal":2}
        ]}]})
    );

    // patient filters the dependencies before the SQL runs.
    let (status, _, body) = run(
        &client,
        &sql_run,
        Value::Array(context(vec![
            json!({"name":"subjectResource","resource":query}),
            min_count(0),
            json!({"name":"patient","valueReference":{"reference":format!("Patient/{}", id_of("Carol"))}}),
        ])),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body.trim(), r#"{"family":"Carol","observations":0}"#);

    // A stored Library, by reference and by canonical; its dependencies
    // still come from context.
    let stored = create(&client, &fhir, query.clone()).await;
    let stored_id = stored["id"].as_str().unwrap();
    for subject in [
        json!({"name":"subjectReference","valueReference":{"reference":format!("Library/{stored_id}")}}),
        json!({"name":"subjectCanonical","valueCanonical":"http://example.org/Library/ObservationCounts"}),
    ] {
        let (status, _, body) = run(
            &client,
            &sql_run,
            Value::Array(context(vec![subject.clone(), min_count(2)])),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{subject}: {body}");
        assert_eq!(
            body.trim(),
            r#"{"family":"Alice","observations":2}"#,
            "{subject}"
        );
    }

    // Everything stored on the server: dependencies resolve by canonical.
    for artifact in [&patient_view, &obs_view, &active_view] {
        create(&client, &fhir, artifact.clone()).await;
    }
    let (status, _, body) = run(
        &client,
        &sql_run,
        json!([
            {"name":"subjectCanonical","valueCanonical":"http://example.org/Library/ObservationCounts"},
            min_count(2)
        ]),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body.trim(), r#"{"family":"Alice","observations":2}"#);

    // A ViewDefinition subject runs like $viewdefinition-run.
    let (status, _, body) = run(
        &client,
        &sql_run,
        json!([{"name":"subjectResource","resource":patient_view},
               {"name":"_format","valueCode":"csv"}]),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body.lines().next(), Some("id,family,active"));
    assert_eq!(body.lines().count(), 4, "{body}");

    // The sandbox: other tables, functions outside the allowlist, a qualified
    // name, more than one statement.
    for (sql, expected) in [
        ("SELECT id FROM patient", "not a dependency"),
        (
            "SELECT * FROM obs JOIN \"Patient\" ON true",
            "not a dependency",
        ),
        (
            "SELECT set_config('role', 'postgres', true) FROM obs",
            "not allowed",
        ),
        ("SELECT * FROM public.patient", "Qualified"),
        ("SELECT 1 FROM obs; DROP TABLE patient", "single statement"),
    ] {
        let library = sql_library(
            "sql-query",
            "http://example.org/Library/Probe",
            sql,
            &[("obs", "http://example.org/ViewDefinition/obs")],
            json!([]),
        );
        let (status, _, body) = run(
            &client,
            &sql_run,
            Value::Array(context(vec![
                json!({"name":"subjectResource","resource":library}),
            ])),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{sql}: {body}");
        assert!(body.contains(expected), "{sql}: {body}");
    }

    // Input errors: 400; a dependency nobody can resolve: 404.
    let missing_dependency = sql_library(
        "sql-query",
        "http://example.org/Library/Missing",
        "SELECT * FROM nowhere",
        &[("nowhere", "http://example.org/ViewDefinition/nowhere")],
        json!([]),
    );
    for (parameters, expected) in [
        (json!([]), StatusCode::BAD_REQUEST),
        (
            json!([{"name":"subjectResource","resource":query},
                   {"name":"subjectCanonical","valueCanonical":"http://example.org/Library/ObservationCounts"}]),
            StatusCode::BAD_REQUEST,
        ),
        (
            Value::Array(context(vec![
                json!({"name":"subjectResource","resource":query}),
            ])),
            StatusCode::BAD_REQUEST,
        ),
        (
            Value::Array(context(vec![
                json!({"name":"subjectResource","resource":query}),
                min_count(0),
                json!({"name":"resource","resource":{"resourceType":"Patient","id":"x"}}),
            ])),
            StatusCode::BAD_REQUEST,
        ),
        (
            json!([{"name":"subjectResource","resource":patient_view}, min_count(1)]),
            StatusCode::BAD_REQUEST,
        ),
        (
            json!([{"name":"subjectResource","resource":missing_dependency}]),
            StatusCode::NOT_FOUND,
        ),
        (
            json!([{"name":"subjectCanonical","valueCanonical":"http://example.org/Library/Unknown"}]),
            StatusCode::NOT_FOUND,
        ),
        (
            json!([{"name":"subjectResource","resource":{"resourceType":"Patient","id":"p"}}]),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
    ] {
        let (status, _, body) = run(&client, &sql_run, parameters.clone()).await;
        assert_eq!(status, expected, "{parameters}: {body}");
    }
}
