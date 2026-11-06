//! `$sql-export` (SQL on FHIR v3): subjects of every kind as one asynchronous
//! job, the SQL on FHIR delivery pattern (202, 303 to the result, manifest,
//! downloads, cancellation) and the requests rejected at kick-off.
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
    start_configured(postgres_url, export_path, |_| {}).await
}

async fn start_configured(
    postgres_url: &str,
    export_path: &str,
    configure: impl FnOnce(&mut AppConfig),
) -> (String, ServerHandle) {
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
    config.bulk_export.max_resources_per_file = 2;
    config.packages.load = vec![octofhir_server::config::PackageSpec::Table {
        id: Some("hl7.fhir.r4.core".into()),
        version: Some("4.0.1".into()),
        path: None,
        url: Some("https://hl7.org/fhir/R4/hl7.fhir.r4.core.tgz".into()),
    }];
    configure(&mut config);
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

/// POST `$sql-export`; the status, Content-Location and body.
async fn kick_off(
    client: &Client,
    fhir: &str,
    parameters: Vec<Value>,
    respond_async: bool,
) -> (StatusCode, Option<String>, Value) {
    let mut request = client
        .post(format!("{fhir}/$sql-export"))
        .json(&json!({"resourceType": "Parameters", "parameter": parameters}));
    if respond_async {
        request = request.header("Prefer", "respond-async");
    }
    let response = request.send().await.unwrap();
    let status = response.status();
    let location = response
        .headers()
        .get("content-location")
        .map(|value| value.to_str().unwrap().to_owned());
    let body = response.json().await.unwrap_or(Value::Null);
    (status, location, body)
}

/// `url`, an absolute URL the server built from its configured base, on the
/// address the test server listens on.
fn here(fhir: &str, url: &str) -> String {
    let path = &url[url.find("/fhir/").expect("a /fhir/ URL") + "/fhir".len()..];
    format!("{fhir}{path}")
}

/// Poll `status_url` until it redirects; the result URL.
async fn wait(client: &Client, status_url: &str) -> String {
    for _ in 0..120 {
        let response = client.get(status_url).send().await.unwrap();
        match response.status() {
            StatusCode::ACCEPTED => {
                assert!(response.headers().contains_key("retry-after"));
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
            StatusCode::SEE_OTHER => {
                let location = response.headers()["location"].to_str().unwrap().to_owned();
                assert!(response.text().await.unwrap().is_empty());
                return location;
            }
            other => panic!("status {other}: {}", response.text().await.unwrap()),
        }
    }
    panic!("the export did not finish");
}

/// `output` entries of a manifest: name → download URLs.
fn manifest_outputs(manifest: &Value) -> Vec<(String, Vec<String>)> {
    let mut outputs: Vec<(String, Vec<String>)> = manifest["parameter"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|param| param["name"] == "output")
        .map(|output| {
            let parts = output["part"].as_array().unwrap();
            let name = parts.iter().find(|part| part["name"] == "name").unwrap()["valueString"]
                .as_str()
                .unwrap()
                .to_owned();
            let locations = parts
                .iter()
                .filter(|part| part["name"] == "location")
                .map(|part| part["valueUri"].as_str().unwrap().to_owned())
                .collect();
            (name, locations)
        })
        .collect();
    outputs.sort();
    outputs
}

fn manifest_value<'a>(manifest: &'a Value, name: &str) -> &'a Value {
    manifest["parameter"]
        .as_array()
        .unwrap()
        .iter()
        .find(|param| param["name"] == name)
        .unwrap_or(&Value::Null)
}

const LIBRARY_TYPES: &str = "http://hl7.org/fhir/uv/sql-on-fhir/CodeSystem/LibraryTypesCodes";

fn sql_query(
    url: &str,
    name: &str,
    sql: &str,
    depends_on: &[(&str, &str)],
    parameters: Value,
) -> Value {
    use base64::Engine;
    let mut library = json!({
        "resourceType": "Library",
        "url": url,
        "name": name,
        "status": "active",
        "type": {"coding": [{"system": LIBRARY_TYPES, "code": "sql-query"}]},
        "relatedArtifact": depends_on.iter().map(|(label, canonical)| json!({
            "type": "depends-on", "label": label, "resource": canonical
        })).collect::<Vec<_>>(),
        "content": [{
            "contentType": "application/sql",
            "data": base64::engine::general_purpose::STANDARD.encode(sql)
        }]
    });
    if parameters.as_array().is_some_and(|list| !list.is_empty()) {
        library["parameter"] = parameters;
    }
    library
}

fn subject(parts: Vec<Value>) -> Value {
    json!({"name": "subject", "part": parts})
}

fn context(resource: &Value) -> Value {
    json!({"name": "context", "resource": resource})
}

#[tokio::test]
async fn sql_export_runs_subjects_as_one_job() {
    let postgres = Postgres::default()
        .with_tag("18-alpine")
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

    for (id, gender, code) in [
        ("p1", "male", "22298006"),
        ("p2", "female", "I21"),
        ("p3", "other", "73211009"),
    ] {
        put(
            &client,
            &fhir,
            json!({"resourceType": "Patient", "id": id, "gender": gender,
                "name": [{"family": format!("Family, {id}")}]}),
        )
        .await;
        put(
            &client,
            &fhir,
            json!({"resourceType": "Condition", "id": format!("c-{id}"),
                "subject": {"reference": format!("Patient/{id}")},
                "code": {"coding": [{"system": "http://snomed.info/sct", "code": code}]}}),
        )
        .await;
    }
    let patients = json!({"resourceType": "ViewDefinition", "status": "active",
        "url": "http://example.org/ViewDefinition/patients", "name": "patients",
        "resource": "Patient",
        "select": [{"column": [
            {"name": "id", "path": "getResourceKey()", "type": "id"},
            {"name": "family", "path": "name.family.first()", "type": "string"},
            {"name": "gender", "path": "gender", "type": "code"}]}]});
    let conditions = json!({"resourceType": "ViewDefinition", "status": "active",
        "url": "http://example.org/ViewDefinition/conditions", "name": "conditions",
        "resource": "Condition",
        "select": [
            {"column": [{"name": "patient_id", "path": "subject.getReferenceKey(Patient)", "type": "id"}]},
            {"forEach": "code.coding", "column": [
                {"name": "system", "path": "system", "type": "uri"},
                {"name": "code", "path": "code", "type": "code"}]}]});
    let mut stored_conditions = conditions.clone();
    stored_conditions["id"] = json!("conditions");
    put(&client, &fhir, stored_conditions).await;
    let cardiac = json!({"resourceType": "ValueSet", "status": "active",
        "url": "http://example.org/ValueSet/cardiac", "version": "1",
        "expansion": {"identifier": "urn:uuid:cardiac-1", "timestamp": "2026-11-01T00:00:00Z",
            "contains": [{"system": "http://snomed.info/sct", "code": "22298006"}]}});
    let cardiac_patients = sql_query(
        "http://example.org/Library/cardiac-patients",
        "cardiac_patients",
        "SELECT p.id, p.gender FROM patients p \
         WHERE p.gender <> :skip_gender AND EXISTS (SELECT 1 FROM conditions c JOIN cardiac v \
           ON v.system = c.system AND v.code = c.code WHERE c.patient_id = p.id) ORDER BY p.id",
        &[
            ("patients", "http://example.org/ViewDefinition/patients"),
            ("conditions", "http://example.org/ViewDefinition/conditions"),
            ("cardiac", "http://example.org/ValueSet/cardiac|1"),
        ],
        json!([{"name": "skip_gender", "use": "in", "type": "string", "min": 1}]),
    );
    let skip = |gender: &str| {
        json!({"name": "parameters", "resource": {"resourceType": "Parameters",
            "parameter": [{"name": "skip_gender", "valueString": gender}]}})
    };

    // Three subjects named three ways, one shared view supplied once.
    let (status, location, body) = kick_off(
        &client,
        &fhir,
        vec![
            json!({"name": "clientTrackingId", "valueString": "batch-7"}),
            subject(vec![
                json!({"name": "name", "valueString": "demographics"}),
                json!({"name": "subjectResource", "resource": patients}),
            ]),
            subject(vec![
                json!({"name": "subjectResource", "resource": cardiac_patients}),
                skip("female"),
            ]),
            subject(vec![json!({"name": "subjectReference",
                "valueReference": {"reference": "ViewDefinition/conditions"}})]),
            context(&patients),
            context(&cardiac),
            json!({"name": "_format", "valueCode": "csv"}),
        ],
        true,
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let status_url = location.expect("Content-Location");
    assert_eq!(manifest_value(&body, "status")["valueCode"], "accepted");
    assert_eq!(
        manifest_value(&body, "clientTrackingId")["valueString"],
        "batch-7"
    );
    assert_eq!(manifest_value(&body, "location")["valueUri"], status_url);

    let result_url = here(&fhir, &wait(&client, &here(&fhir, &status_url)).await);
    let response = client.get(&result_url).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let provenance: Value =
        serde_json::from_str(response.headers()["x-sof-terminology"].to_str().unwrap()).unwrap();
    assert_eq!(
        provenance[0]["expansion"]["identifier"],
        "urn:uuid:cardiac-1"
    );
    let manifest: Value = response.json().await.unwrap();
    assert_eq!(
        manifest_value(&manifest, "status")["valueCode"],
        "completed"
    );
    assert_eq!(manifest_value(&manifest, "_format")["valueCode"], "csv");
    assert_eq!(
        manifest_value(&manifest, "clientTrackingId")["valueString"],
        "batch-7"
    );
    let outputs = manifest_outputs(&manifest);
    let names: Vec<&str> = outputs.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names, ["cardiac_patients", "conditions", "demographics"]);

    let download = |url: String| {
        let client = client.clone();
        let fhir = fhir.clone();
        async move {
            let response = client.get(here(&fhir, &url)).send().await.unwrap();
            assert_eq!(response.status(), StatusCode::OK, "{url}");
            assert_eq!(response.headers()["content-type"], "text/csv");
            response.text().await.unwrap()
        }
    };
    // Three patients, two rows per file: two parts, each with its header.
    let demographics = &outputs[2].1;
    assert_eq!(demographics.len(), 2);
    assert!(
        demographics[0].ends_with("/demographics.part1.csv"),
        "{demographics:?}"
    );
    assert!(
        demographics[1].ends_with("/demographics.part2.csv"),
        "{demographics:?}"
    );
    let mut rows = Vec::new();
    for url in demographics {
        let text = download(url.clone()).await;
        let mut lines = text.lines();
        assert_eq!(lines.next(), Some("id,family,gender"));
        rows.extend(lines.map(str::to_owned));
    }
    rows.sort();
    assert_eq!(
        rows,
        [
            "p1,\"Family, p1\",male",
            "p2,\"Family, p2\",female",
            "p3,\"Family, p3\",other"
        ]
    );
    // The SQLQuery with its bound parameter and value set relation.
    assert_eq!(
        download(outputs[0].1[0].clone()).await,
        "id,gender\r\np1,male\r\n"
    );
    // The stored view, by reference: one condition per patient.
    let conditions_rows: usize = {
        let mut count = 0;
        for url in &outputs[1].1 {
            count += download(url.clone()).await.lines().count() - 1;
        }
        count
    };
    assert_eq!(conditions_rows, 3);

    // ndjson by default.
    let (status, location, body) = kick_off(
        &client,
        &fhir,
        vec![
            subject(vec![
                json!({"name": "name", "valueString": "genders"}),
                json!({"name": "subjectResource", "resource": sql_query(
                "http://example.org/Library/genders", "genders",
                "SELECT gender, count(*) AS n FROM patients GROUP BY gender ORDER BY gender",
                &[("patients", "http://example.org/ViewDefinition/patients")], json!([]))}),
            ]),
            context(&patients),
        ],
        true,
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let result_url = here(
        &fhir,
        &wait(&client, &here(&fhir, &location.unwrap())).await,
    );
    let manifest: Value = client
        .get(&result_url)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let url = manifest_outputs(&manifest)[0].1[0].clone();
    assert!(url.ends_with("/genders.part1.ndjson"), "{url}");
    let response = client.get(here(&fhir, &url)).send().await.unwrap();
    assert_eq!(response.headers()["content-type"], "application/x-ndjson");
    assert_eq!(
        response.text().await.unwrap(),
        "{\"gender\":\"female\",\"n\":1}\n{\"gender\":\"male\",\"n\":1}\n"
    );

    // Cancelled: DELETE answers 202, the status URL is then a 404.
    let (status, location, _) = kick_off(
        &client,
        &fhir,
        vec![subject(vec![
            json!({"name": "subjectResource", "resource": patients}),
        ])],
        true,
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let status_url = here(&fhir, &location.unwrap());
    let response = client.delete(&status_url).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let response = client.get(&status_url).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    // Rejected at kick-off, never through the status URL.
    let view_subject = subject(vec![
        json!({"name": "subjectResource", "resource": patients}),
    ]);
    let broken = sql_query(
        "http://example.org/Library/broken",
        "broken",
        "SELECT p.no_such_column FROM patients p",
        &[("patients", "http://example.org/ViewDefinition/patients")],
        json!([]),
    );
    for (parameters, respond_async, expected, text) in [
        (
            vec![view_subject.clone()],
            false,
            StatusCode::BAD_REQUEST,
            "respond-async",
        ),
        (vec![], true, StatusCode::BAD_REQUEST, "subject"),
        (
            vec![
                view_subject.clone(),
                json!({"name": "_format", "valueCode": "fhir"}),
            ],
            true,
            StatusCode::BAD_REQUEST,
            "fhir",
        ),
        (
            vec![
                view_subject.clone(),
                json!({"name": "_limit", "valueInteger": 1}),
            ],
            true,
            StatusCode::BAD_REQUEST,
            "_limit",
        ),
        (
            vec![view_subject.clone(), view_subject.clone()],
            true,
            StatusCode::BAD_REQUEST,
            "output name",
        ),
        (
            vec![subject(vec![
                json!({"name": "subjectResource", "resource": patients}),
                skip("male"),
            ])],
            true,
            StatusCode::BAD_REQUEST,
            "ViewDefinition declares none",
        ),
        (
            vec![view_subject.clone(), context(&cardiac)],
            true,
            StatusCode::BAD_REQUEST,
            "matches no dependency",
        ),
        (
            vec![subject(vec![json!({"name": "subjectCanonical",
                "valueCanonical": "http://example.org/Library/missing"})])],
            true,
            StatusCode::NOT_FOUND,
            "missing",
        ),
        (
            vec![
                subject(vec![json!({"name": "subjectResource", "resource": broken})]),
                context(&patients),
            ],
            true,
            StatusCode::UNPROCESSABLE_ENTITY,
            "no_such_column",
        ),
        (
            vec![
                view_subject.clone(),
                json!({"name": "patient", "valueReference": {"reference": "Patient/nobody"}}),
            ],
            true,
            StatusCode::BAD_REQUEST,
            "nobody",
        ),
    ] {
        let (status, _, body) = kick_off(&client, &fhir, parameters.clone(), respond_async).await;
        assert_eq!(status, expected, "{parameters:?}: {body}");
        assert!(body.to_string().contains(text), "{text}: {body}");
    }

    // ViewDefinition/$export, the export of 2.0, is there next to $sql-export.
    let response = client
        .post(format!("{fhir}/ViewDefinition/$export"))
        .header("Prefer", "respond-async")
        .json(&view_export())
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
}

fn view_export() -> Value {
    json!({"resourceType": "Parameters", "parameter": [{"name": "viewDefinition", "resource": {
        "resourceType": "ViewDefinition", "status": "active", "resource": "Patient",
        "select": [{"column": [{"name": "id", "path": "id", "type": "id"}]}]}}]})
}

#[tokio::test]
async fn the_export_of_2_0_can_be_switched_off() {
    let postgres = Postgres::default()
        .with_tag("18-alpine")
        .start()
        .await
        .expect("postgres");
    let port = postgres.get_host_port_ipv4(5432).await.unwrap();
    let exports = tempfile::tempdir().unwrap();
    let (fhir, _server) = start_configured(
        &format!("postgres://postgres:postgres@127.0.0.1:{port}/postgres"),
        exports.path().to_str().unwrap(),
        |config| config.sql_on_fhir.viewdefinition_export = false,
    )
    .await;
    let client = Client::builder().redirect(Policy::none()).build().unwrap();

    let response = client
        .post(format!("{fhir}/ViewDefinition/$export"))
        .header("Prefer", "respond-async")
        .json(&view_export())
        .send()
        .await
        .unwrap();
    assert_ne!(response.status(), StatusCode::ACCEPTED);

    // $sql-export stays.
    let statement: Value = client
        .get(format!("{fhir}/metadata"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let operations = statement["rest"][0]["operation"].as_array().unwrap();
    assert!(
        operations
            .iter()
            .any(|operation| operation["name"] == "$sql-export")
    );
    let (status, _, body) = kick_off(&client, &fhir, vec![], true).await;
    assert_ne!(status, StatusCode::NOT_IMPLEMENTED, "{body}");
    assert!(!body.to_string().contains("not enabled"), "{body}");
}
