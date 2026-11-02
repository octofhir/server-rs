//! SQL on FHIR 3.0.0 conformance details of `$sql-run` / `$sql-export`:
//! the CapabilityStatement, ViewDefinition search, content negotiation
//! (`_format`, `Accept`, the Binary envelope, parquet), GET, the rejection
//! codes and expressions, canonical version selection, the `fhir` format's
//! value mapping, and views computed in memory feeding SQL.
//!
//! Requirements: Docker running (testcontainers).

use std::sync::Arc;
use std::time::Duration;

use octofhir_config::ConfigurationManager;
use octofhir_server::{AppConfig, PostgresStorageConfig, build_app};
use reqwest::{Client, StatusCode, redirect::Policy};
use serde_json::{Value, json};
use testcontainers::{ImageExt, runners::AsyncRunner};
use testcontainers_modules::postgres::Postgres;

struct ServerHandle(tokio::task::JoinHandle<()>);

impl Drop for ServerHandle {
    fn drop(&mut self) {
        self.0.abort();
    }
}

async fn start_server(postgres_url: &str, export_path: &str) -> (String, ServerHandle) {
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
    config.bulk_export.enabled = true;
    config.bulk_export.export_path = export_path.to_owned();
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

struct Answer {
    status: StatusCode,
    content_type: String,
    bytes: Vec<u8>,
}

impl Answer {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.bytes).into_owned()
    }

    fn json(&self) -> Value {
        serde_json::from_slice(&self.bytes)
            .unwrap_or_else(|error| panic!("{error}: {}", self.text()))
    }

    /// The issues of an OperationOutcome answer: (code, expression).
    fn issues(&self) -> Vec<(String, String)> {
        self.json()["issue"]
            .as_array()
            .unwrap()
            .iter()
            .map(|issue| {
                (
                    issue["code"].as_str().unwrap_or_default().to_owned(),
                    issue["expression"][0]
                        .as_str()
                        .unwrap_or_default()
                        .to_owned(),
                )
            })
            .collect()
    }
}

async fn answer(response: reqwest::Response) -> Answer {
    Answer {
        status: response.status(),
        content_type: response
            .headers()
            .get("content-type")
            .map(|value| value.to_str().unwrap().to_owned())
            .unwrap_or_default(),
        bytes: response.bytes().await.unwrap().to_vec(),
    }
}

async fn run(client: &Client, fhir: &str, accept: Option<&str>, parameters: Vec<Value>) -> Answer {
    let mut request = client
        .post(format!("{fhir}/$sql-run"))
        .json(&json!({"resourceType": "Parameters", "parameter": parameters}));
    if let Some(accept) = accept {
        request = request.header("Accept", accept);
    }
    answer(request.send().await.unwrap()).await
}

const LIBRARY_TYPES: &str = "http://hl7.org/fhir/uv/sql-on-fhir/CodeSystem/LibraryTypesCodes";

fn sql_query(sql: &str, depends_on: &[(&str, &str)]) -> Value {
    use base64::Engine;
    json!({
        "resourceType": "Library",
        "status": "active",
        "type": {"coding": [{"system": LIBRARY_TYPES, "code": "sql-query"}]},
        "relatedArtifact": depends_on.iter().map(|(label, canonical)| json!({
            "type": "depends-on", "label": label, "resource": canonical
        })).collect::<Vec<_>>(),
        "content": [{
            "contentType": "application/sql",
            "data": base64::engine::general_purpose::STANDARD.encode(sql)
        }]
    })
}

fn subject(resource: &Value) -> Value {
    json!({"name": "subjectResource", "resource": resource})
}

fn format(code: &str) -> Value {
    json!({"name": "_format", "valueCode": code})
}

const PATIENTS: &str = "http://example.org/ViewDefinition/patients";

#[tokio::test]
async fn sql_on_fhir_conformance_details() {
    let postgres = Postgres::default()
        .with_tag("17-alpine")
        .start()
        .await
        .expect("postgres");
    let port = postgres.get_host_port_ipv4(5432).await.unwrap();
    let exports = tempfile::tempdir().unwrap();
    let (fhir, _server) = start_server(
        &format!("postgres://postgres:postgres@127.0.0.1:{port}/postgres"),
        exports.path().to_str().unwrap(),
    )
    .await;
    let client = Client::builder().redirect(Policy::none()).build().unwrap();

    for (id, birth) in [("p1", "2012"), ("p2", "1980-05-01"), ("p3", "1975-01-02")] {
        put(
            &client,
            &fhir,
            json!({"resourceType": "Patient", "id": id, "birthDate": birth,
                "gender": if id == "p2" { "female" } else { "male" }}),
        )
        .await;
    }
    let patients = json!({"resourceType": "ViewDefinition", "id": "patients", "status": "active",
        "url": PATIENTS, "version": "1", "name": "patients", "resource": "Patient",
        "select": [{"column": [
            {"name": "id", "path": "getResourceKey()", "type": "id"},
            {"name": "gender", "path": "gender", "type": "code"}]}]});
    put(&client, &fhir, patients.clone()).await;
    let mut newer = patients.clone();
    newer["id"] = json!("patients-2");
    newer["version"] = json!("2");
    newer["select"][0]["column"]
        .as_array_mut()
        .unwrap()
        .push(json!({"name": "birth", "path": "birthDate", "type": "date"}));
    put(&client, &fhir, newer).await;

    // CapabilityStatement: the data operations by `$` name and this server's
    // definitions; ViewDefinition declared, without operations.
    let metadata = answer(client.get(format!("{fhir}/metadata")).send().await.unwrap())
        .await
        .json();
    let operations = metadata["rest"][0]["operation"].as_array().unwrap();
    let operation = |name: &str| operations.iter().find(|op| op["name"] == name).cloned();
    assert_eq!(
        operation("$sql-run").unwrap()["definition"],
        "http://octofhir.io/OperationDefinition/sql-run"
    );
    assert_eq!(
        operation("$sql-export").unwrap()["definition"],
        "http://octofhir.io/OperationDefinition/sql-export"
    );
    assert!(operation("viewdefinition-run").is_none());
    let view_entry = metadata["rest"][0]["resource"]
        .as_array()
        .unwrap()
        .iter()
        .find(|resource| resource["type"] == "ViewDefinition")
        .expect("ViewDefinition entry")
        .clone();
    assert!(view_entry.get("operation").is_none(), "{view_entry}");

    // ViewDefinition is searchable by its metadata (url, version).
    let bundle = answer(
        client
            .get(format!("{fhir}/ViewDefinition"))
            .query(&[("url", PATIENTS), ("version", "2")])
            .send()
            .await
            .unwrap(),
    )
    .await
    .json();
    assert_eq!(
        bundle["entry"].as_array().map(Vec::len),
        Some(1),
        "{bundle}"
    );

    // An unversioned canonical subject: the latest version.
    let canonical = json!({"name": "subjectCanonical", "valueCanonical": PATIENTS});
    let latest = run(
        &client,
        &fhir,
        None,
        vec![canonical.clone(), format("json")],
    )
    .await;
    assert_eq!(latest.status, StatusCode::OK, "{}", latest.text());
    assert!(latest.json()[0].get("birth").is_some(), "{}", latest.text());

    // Accept selects the format without _format; _format wins over it.
    let csv = run(&client, &fhir, Some("text/csv"), vec![canonical.clone()]).await;
    assert_eq!(csv.status, StatusCode::OK, "{}", csv.text());
    assert!(
        csv.content_type.starts_with("text/csv"),
        "{}",
        csv.content_type
    );
    assert!(csv.text().starts_with("id,gender,birth"), "{}", csv.text());
    let ndjson = run(
        &client,
        &fhir,
        Some("text/csv"),
        vec![canonical.clone(), format("ndjson")],
    )
    .await;
    assert_eq!(ndjson.content_type, "application/x-ndjson");
    // A FHIR media type asks for the Binary envelope.
    let envelope = run(
        &client,
        &fhir,
        Some("application/fhir+json"),
        vec![canonical.clone(), format("csv")],
    )
    .await;
    assert_eq!(envelope.status, StatusCode::OK, "{}", envelope.text());
    let binary = envelope.json();
    assert_eq!(binary["resourceType"], "Binary");
    assert!(
        binary["contentType"]
            .as_str()
            .unwrap()
            .starts_with("text/csv"),
        "{binary}"
    );
    assert!(binary.get("__sofBinaryEnvelope").is_none());
    // Parquet.
    let parquet = run(
        &client,
        &fhir,
        Some("application/vnd.apache.parquet"),
        vec![canonical.clone()],
    )
    .await;
    assert_eq!(parquet.status, StatusCode::OK, "{}", parquet.text());
    assert_eq!(parquet.content_type, "application/vnd.apache.parquet");
    assert_eq!(&parquet.bytes[..4], b"PAR1");

    // GET, with only primitive parameters.
    let get = answer(
        client
            .get(format!("{fhir}/$sql-run"))
            .query(&[
                ("subjectCanonical", format!("{PATIENTS}|1")),
                ("_format", "csv".to_owned()),
            ])
            .send()
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(get.status, StatusCode::OK, "{}", get.text());
    assert_eq!(get.text().lines().next(), Some("id,gender"));

    // Rejections: status, issue code and the parameter at fault.
    let cases: Vec<(Vec<Value>, StatusCode, &str, &str)> = vec![
        (vec![], StatusCode::BAD_REQUEST, "required", "subject"),
        (
            vec![
                canonical.clone(),
                json!({"name": "_formt", "valueCode": "csv"}),
            ],
            StatusCode::BAD_REQUEST,
            "not-supported",
            "_formt",
        ),
        (
            vec![canonical.clone(), format("xlsx")],
            StatusCode::BAD_REQUEST,
            "not-supported",
            "_format",
        ),
        (
            vec![
                canonical.clone(),
                json!({"name": "source", "valueString": "s3://x"}),
            ],
            StatusCode::BAD_REQUEST,
            "not-supported",
            "source",
        ),
        (
            vec![
                canonical.clone(),
                json!({"name": "patient", "valueReference": {"reference": "Patient/nobody"}}),
            ],
            StatusCode::BAD_REQUEST,
            "not-found",
            "patient",
        ),
        (
            vec![
                json!({"name": "subjectReference", "valueReference": {"reference": "Patient/p1"}}),
            ],
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid",
            "subject",
        ),
        (
            vec![subject(
                &json!({"resourceType": "ViewDefinition", "status": "active",
                "resource": "Patient", "select": [{"forEach": "name", "forEachOrNull": "name",
                "column": [{"name": "f", "path": "family"}]}]}),
            )],
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid",
            "subject",
        ),
    ];
    for (parameters, status, code, expression) in cases {
        let answer = run(&client, &fhir, None, parameters.clone()).await;
        assert_eq!(answer.status, status, "{parameters:?}: {}", answer.text());
        assert_eq!(
            answer.issues()[0],
            (code.to_owned(), expression.to_owned()),
            "{}",
            answer.text()
        );
    }
    // GET cannot carry a resource-valued parameter.
    let get_parameters = answer(
        client
            .get(format!("{fhir}/$sql-run"))
            .query(&[("subjectCanonical", PATIENTS), ("parameters", "x")])
            .send()
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(get_parameters.status, StatusCode::BAD_REQUEST);
    assert_eq!(get_parameters.issues()[0].1, "parameters");
    // An unresolvable subject with an unresolvable filter: 404, both issues.
    let both = run(
        &client,
        &fhir,
        None,
        vec![
            json!({"name": "subjectCanonical", "valueCanonical": "http://example.org/nothing"}),
            json!({"name": "group", "valueReference": {"reference": "Group/nobody"}}),
        ],
    )
    .await;
    assert_eq!(both.status, StatusCode::NOT_FOUND, "{}", both.text());
    assert_eq!(
        both.issues(),
        [
            ("not-found".to_owned(), "subject".to_owned()),
            ("not-found".to_owned(), "group".to_owned())
        ]
    );

    // _format=fhir: on R4 a BIGINT is a decimal; a timestamp keeps no zone.
    let typed = run(
        &client,
        &fhir,
        None,
        vec![
            subject(&sql_query(
                "SELECT count(*) AS n, timestamp '2026-01-02 03:04:05' AS local FROM patients",
                &[("patients", &format!("{PATIENTS}|1"))],
            )),
            format("fhir"),
        ],
    )
    .await;
    assert_eq!(typed.status, StatusCode::OK, "{}", typed.text());
    let parts = &typed.json()["parameter"][0]["part"];
    assert_eq!(parts[0], json!({"name": "n", "valueDecimal": 3}));
    assert_eq!(
        parts[1],
        json!({"name": "local", "valueDateTime": "2026-01-02T03:04:05"})
    );

    // A view computed in memory (memberOf) feeds the SQL with the types the
    // SQL path gives: a partial date stays text.
    let in_memory = json!({"resourceType": "ViewDefinition", "status": "active",
        "url": "http://example.org/ViewDefinition/births", "name": "births", "resource": "Patient",
        "select": [{"column": [
            {"name": "id", "path": "getResourceKey()", "type": "id"},
            {"name": "birth", "path": "birthDate", "type": "date"},
            {"name": "known", "type": "boolean",
             "path": "gender.memberOf('http://hl7.org/fhir/ValueSet/administrative-gender')"}]}]});
    let births = run(
        &client,
        &fhir,
        None,
        vec![
            subject(&sql_query(
                "SELECT id, birth FROM births ORDER BY id",
                &[("births", "http://example.org/ViewDefinition/births")],
            )),
            json!({"name": "context", "resource": in_memory}),
            format("json"),
        ],
    )
    .await;
    assert_eq!(births.status, StatusCode::OK, "{}", births.text());
    assert_eq!(births.json()[0], json!({"id": "p1", "birth": "2012"}));

    // $sql-export writes parquet.
    let kickoff = client
        .post(format!("{fhir}/$sql-export"))
        .header("Prefer", "respond-async")
        .json(&json!({"resourceType": "Parameters", "parameter": [
            {"name": "subject", "part": [
                {"name": "subjectCanonical", "valueCanonical": format!("{PATIENTS}|1")}]},
            format("parquet")
        ]}))
        .send()
        .await
        .unwrap();
    assert_eq!(kickoff.status(), StatusCode::ACCEPTED);
    let here = |url: &str| {
        let path = &url[url.find("/fhir/").unwrap() + "/fhir".len()..];
        format!("{fhir}{path}")
    };
    let status_url = here(kickoff.headers()["content-location"].to_str().unwrap());
    let mut result_url = None;
    for _ in 0..120 {
        let response = client.get(&status_url).send().await.unwrap();
        if response.status() == StatusCode::SEE_OTHER {
            result_url = Some(here(response.headers()["location"].to_str().unwrap()));
            break;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    let manifest = answer(
        client
            .get(result_url.expect("finished"))
            .send()
            .await
            .unwrap(),
    )
    .await
    .json();
    let output = manifest["parameter"]
        .as_array()
        .unwrap()
        .iter()
        .find(|param| param["name"] == "output")
        .unwrap()
        .clone();
    let location = output["part"]
        .as_array()
        .unwrap()
        .iter()
        .find(|part| part["name"] == "location")
        .unwrap()["valueUri"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(location.ends_with(".parquet"), "{location}");
    let file = answer(
        client
            .get(here(&location))
            .header("Accept", "application/vnd.apache.parquet")
            .send()
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(file.status, StatusCode::OK, "{}", file.text());
    assert_eq!(file.content_type, "application/vnd.apache.parquet");
    assert_eq!(&file.bytes[..4], b"PAR1");
}
