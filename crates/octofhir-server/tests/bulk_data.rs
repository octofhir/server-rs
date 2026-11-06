//! Bulk Data Access: `$export` in both IG versions, and `$bulk-submit`.
//!
//! Requirements: Docker running (testcontainers).

use std::sync::Arc;
use std::time::Duration;

use octofhir_config::ConfigurationManager;
use octofhir_server::config::{BulkSpecVersion, BulkSubmitter};
use octofhir_server::{AppConfig, PostgresStorageConfig, build_app};
use reqwest::{Client, Response, StatusCode};
use serde_json::{Value, json};
use testcontainers::{ImageExt, runners::AsyncRunner};
use testcontainers_modules::postgres::Postgres;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{header, method, path},
};

const ASYNC: &str = "respond-async";
const EVENT_STATUS: &str = "http://hl7.org/fhir/event-status";
const SUBMITTER: (&str, &str) = ("urn:test:submitters", "provider-a");

struct ServerHandle(tokio::task::JoinHandle<()>);

impl Drop for ServerHandle {
    fn drop(&mut self) {
        self.0.abort();
    }
}

async fn start_server(
    postgres_url: &str,
    export_path: &str,
    spec: BulkSpecVersion,
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
    config.bulk_export.enabled = true;
    config.bulk_export.export_path = export_path.to_owned();
    config.bulk_export.max_resources_per_file = 2;
    config.bulk_export.max_files_per_manifest = 1;
    config.bulk_export.min_poll_interval_ms = 0;
    config.bulk_export.spec_version = spec;
    config.bulk_submit.submitters = vec![BulkSubmitter {
        system: SUBMITTER.0.into(),
        value: SUBMITTER.1.into(),
        client_id: None,
        provider_client_id: None,
        provider_scope: "system/*.read".into(),
    }];
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
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("bind");
    let addr = listener.local_addr().unwrap();
    config.server.base_url = Some(format!("http://{addr}"));
    let app = build_app(&config, config_manager).await.expect("build app");
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

fn parameters(parameter: Value) -> Value {
    json!({"resourceType": "Parameters", "parameter": parameter})
}

async fn kick_off(client: &Client, url: &str, body: Value, prefer: &str) -> Response {
    client
        .post(url)
        .header("Accept", "application/fhir+json")
        .header("Prefer", prefer)
        .json(&body)
        .send()
        .await
        .unwrap()
}

fn location(response: &Response) -> String {
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    response.headers()["content-location"]
        .to_str()
        .unwrap()
        .to_owned()
}

/// Polls until the job is over; the response's status, headers and body.
async fn wait(
    client: &Client,
    status_url: &str,
) -> (StatusCode, reqwest::header::HeaderMap, Value) {
    for _ in 0..240 {
        let response = client
            .get(status_url)
            .header("Accept", "application/json")
            .send()
            .await
            .unwrap();
        if response.status() != StatusCode::ACCEPTED {
            let status = response.status();
            let headers = response.headers().clone();
            return (
                status,
                headers,
                response.json().await.unwrap_or(Value::Null),
            );
        }
        assert!(response.headers().contains_key("retry-after"));
        assert!(response.headers().contains_key("x-progress"));
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("the job did not finish");
}

/// Starts an export and returns its manifest.
async fn export(client: &Client, url: &str, parameter: Value) -> Value {
    let response = kick_off(client, url, parameters(parameter), ASYNC).await;
    let status = location(&response);
    let (code, _, manifest) = wait(client, &status).await;
    assert_eq!(code, StatusCode::OK, "{manifest}");
    manifest
}

async fn lines(client: &Client, url: &str) -> Vec<Value> {
    let response = client
        .get(url)
        .header("Accept", "application/fhir+ndjson")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK, "{url}");
    assert_eq!(
        response.headers()["content-type"],
        "application/fhir+ndjson"
    );
    response
        .text()
        .await
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

/// Every resource of a manifest section.
async fn section(client: &Client, manifest: &Value, name: &str) -> Vec<Value> {
    let mut all = Vec::new();
    for file in manifest[name].as_array().into_iter().flatten() {
        all.extend(lines(client, file["url"].as_str().unwrap()).await);
    }
    all
}

fn ids<'a>(resources: &'a [Value], resource_type: &str) -> Vec<&'a str> {
    let mut found: Vec<_> = resources
        .iter()
        .filter(|resource| resource["resourceType"] == resource_type)
        .map(|resource| resource["id"].as_str().unwrap())
        .collect();
    found.sort_unstable();
    found
}

async fn seed(client: &Client, fhir: &str) {
    for id in ["p1", "p2", "p3"] {
        put(
            client,
            fhir,
            json!({"resourceType":"Patient","id":id,"gender":"male","name":[{"family":id}]}),
        )
        .await;
    }
    for (id, patient) in [("o1", "p1"), ("o2", "p2")] {
        put(
            client,
            fhir,
            json!({"resourceType":"Observation","id":id,"status":"final",
                "code":{"text":"x"},"subject":{"reference":format!("Patient/{patient}")}}),
        )
        .await;
    }
    for (id, recorded) in [
        ("prov-old", "2024-01-01T00:00:00Z"),
        ("prov-new", "2025-01-01T00:00:00Z"),
    ] {
        put(
            client,
            fhir,
            json!({"resourceType":"Provenance","id":id,"recorded":recorded,
                "target":[{"reference":"Observation/o1"}],
                "agent":[{"who":{"display":"x"}}]}),
        )
        .await;
    }
    put(
        client,
        fhir,
        json!({"resourceType":"Group","id":"g1","type":"person","actual":true,
            "member":[{"entity":{"reference":"Patient/p1"}},{"entity":{"reference":"Patient/p2"}}]}),
    )
    .await;
}

async fn stu3_export(client: &Client, fhir: &str) {
    let system = format!("{fhir}/$export");
    let patients = format!("{fhir}/Patient/$export");
    let group = format!("{fhir}/Group/g1/$export");

    // GET stays in STU 3, with a manifest of its shape.
    let response = client
        .get(format!("{system}?_type=Patient,Observation"))
        .header("Accept", "application/fhir+json")
        .header("Prefer", ASYNC)
        .send()
        .await
        .unwrap();
    let status_url = location(&response);
    let (code, headers, manifest) = wait(client, &status_url).await;
    assert_eq!(code, StatusCode::OK);
    assert!(headers.contains_key("expires"));
    assert!(
        headers["content-type"]
            .to_str()
            .unwrap()
            .starts_with("application/json")
    );
    assert!(manifest["request"].as_str().unwrap().contains("$export"));
    assert_eq!(manifest["error"], json!([]));
    assert!(manifest.get("outcome").is_none());
    assert_eq!(manifest["requiresAccessToken"], false);
    let output = section(client, &manifest, "output").await;
    assert_eq!(ids(&output, "Patient"), ["p1", "p2", "p3"]);
    assert_eq!(ids(&output, "Observation"), ["o1", "o2"]);
    assert_eq!(
        manifest["output"].as_array().unwrap().len(),
        3,
        "two Patient files of at most two, one Observation file"
    );

    // POST with Parameters, repeated values.
    let manifest = export(
        client,
        &system,
        json!([{"name":"_type","valueString":"Patient"},
               {"name":"_type","valueString":"Observation"},
               {"name":"_typeFilter","valueString":"Observation?status=final"}]),
    )
    .await;
    assert_eq!(section(client, &manifest, "output").await.len(), 5);

    // _until: nothing was changed before the epoch of this test.
    let manifest = export(
        client,
        &system,
        json!([{"name":"_type","valueString":"Patient"},
               {"name":"_until","valueInstant":"2001-01-01T00:00:00Z"}]),
    )
    .await;
    assert_eq!(manifest["output"], json!([]));

    // patient narrows a Patient and a Group export; a non-member is an error.
    let only = json!([{"name":"_type","valueString":"Observation"},
        {"name":"patient","valueReference":{"reference":"Patient/p1"}}]);
    let manifest = export(client, &patients, only.clone()).await;
    assert_eq!(
        ids(&section(client, &manifest, "output").await, "Observation"),
        ["o1"]
    );
    let manifest = export(client, &group, only).await;
    assert_eq!(
        ids(&section(client, &manifest, "output").await, "Observation"),
        ["o1"]
    );
    let outsider =
        parameters(json!([{"name":"patient","valueReference":{"reference":"Patient/p3"}}]));
    let refused = kick_off(client, &group, outsider.clone(), ASYNC).await;
    assert_eq!(refused.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        refused.json::<Value>().await.unwrap()["resourceType"],
        "OperationOutcome"
    );
    let refused = kick_off(
        client,
        &system,
        parameters(json!([{"name":"patient","valueReference":{"reference":"Patient/p1"}}])),
        ASYNC,
    )
    .await;
    assert_eq!(refused.status(), StatusCode::BAD_REQUEST);

    // Lenient: the rest goes ahead, and the manifest tells what was ignored.
    let response = kick_off(
        client,
        &group,
        parameters(json!([
            {"name":"patient","valueReference":{"reference":"Patient/p3"}},
            {"name":"patient","valueReference":{"reference":"Patient/p1"}},
            {"name":"organizeOutputBy","valueString":"Encounter"},
            {"name":"_type","valueString":"Observation"}
        ])),
        "respond-async, handling=lenient",
    )
    .await;
    let (_, _, manifest) = wait(client, &location(&response)).await;
    let messages = section(client, &manifest, "error").await;
    assert_eq!(messages.len(), 2, "{messages:?}");
    assert!(
        messages
            .iter()
            .all(|m| m["resourceType"] == "OperationOutcome")
    );
    assert_eq!(
        ids(&section(client, &manifest, "output").await, "Observation"),
        ["o1"]
    );

    // _elements: named and mandatory elements stay, the rest goes, and it says so.
    let manifest = export(
        client,
        &system,
        json!([{"name":"_type","valueString":"Observation"},
               {"name":"_type","valueString":"Patient"},
               {"name":"_elements","valueString":"Patient.gender"}]),
    )
    .await;
    let output = section(client, &manifest, "output").await;
    let patient = output
        .iter()
        .find(|r| r["resourceType"] == "Patient")
        .unwrap();
    assert_eq!(patient["gender"], "male");
    assert!(patient.get("name").is_none());
    assert_eq!(patient["meta"]["tag"][0]["code"], "SUBSETTED");
    let observation = output
        .iter()
        .find(|r| r["resourceType"] == "Observation")
        .unwrap();
    assert!(
        observation.get("status").is_some(),
        "mandatory: {observation}"
    );
    assert!(observation.get("subject").is_none());
    let invalid = parameters(json!([{"name":"_elements","valueString":"Patient.name.family"}]));
    assert_eq!(
        kick_off(client, &system, invalid, ASYNC).await.status(),
        StatusCode::BAD_REQUEST
    );

    // Provenance: default in a Patient export follows the compartment, and
    // includeAssociatedData selects from the resources exported.
    let manifest = export(client, &patients, json!([])).await;
    let output = section(client, &manifest, "output").await;
    assert_eq!(ids(&output, "Provenance"), ["prov-new", "prov-old"]);
    let manifest = export(
        client,
        &patients,
        json!([{"name":"_type","valueString":"Observation"},
               {"name":"includeAssociatedData","valueCode":"LatestProvenanceResources"}]),
    )
    .await;
    assert_eq!(
        ids(&section(client, &manifest, "output").await, "Provenance"),
        ["prov-new"]
    );
    let manifest = export(
        client,
        &group,
        json!([{"name":"_type","valueString":"Observation"},
               {"name":"includeAssociatedData","valueCode":"LatestProvenanceResources"},
               {"name":"includeAssociatedData","valueCode":"RelevantProvenanceResources"}]),
    )
    .await;
    assert_eq!(
        ids(&section(client, &manifest, "output").await, "Provenance"),
        ["prov-new", "prov-old"],
        "the least restrictive value wins"
    );

    // organizeOutputBy=Patient: a header, the Patient, what references it.
    let manifest = export(
        client,
        &group,
        json!([{"name":"organizeOutputBy","valueString":"Patient"}]),
    )
    .await;
    assert_eq!(manifest["outputOrganizedBy"], "Patient");
    assert!(
        manifest["output"]
            .as_array()
            .unwrap()
            .iter()
            .all(|f| f.get("type").is_none())
    );
    let output = section(client, &manifest, "output").await;
    assert_eq!(output[0]["parameter"][0]["name"], "header");
    assert_eq!(
        output[0]["parameter"][0]["valueReference"]["reference"],
        "Patient/p1"
    );
    assert_eq!(output[1]["id"], "p1");
    let second = output
        .iter()
        .position(|line| line["parameter"][0]["valueReference"]["reference"] == "Patient/p2")
        .expect("a block for p2");
    let first_block = &output[..second];
    assert_eq!(ids(first_block, "Observation"), ["o1"]);
    assert_eq!(ids(first_block, "Provenance"), ["prov-new", "prov-old"]);
    assert_eq!(output[second + 1]["id"], "p2");
    assert!(
        ids(&output, "Patient").iter().all(|id| *id != "p3"),
        "p3 is no member"
    );

    // Partial manifests: one file per page, linked, all fields but the files equal.
    let manifest = export(
        client,
        &system,
        json!([{"name":"_type","valueString":"Patient"},
               {"name":"allowPartialManifests","valueBoolean":true}]),
    )
    .await;
    let mut page = manifest;
    let mut seen = Vec::new();
    let mut pages = 1;
    let transaction_time = page["transactionTime"].clone();
    loop {
        seen.extend(section(client, &page, "output").await);
        assert_eq!(page["transactionTime"], transaction_time);
        let Some(next) = page["link"][0]["url"].as_str().map(str::to_owned) else {
            break;
        };
        assert_eq!(page["link"][0]["relation"], "next");
        page = client.get(next).send().await.unwrap().json().await.unwrap();
        pages += 1;
    }
    assert_eq!(ids(&seen, "Patient"), ["p1", "p2", "p3"]);
    assert_eq!(pages, 2, "three Patients are two files");

    // separate-export-status moves the job's status to a header.
    let response = kick_off(
        client,
        &system,
        parameters(json!([{"name":"_type","valueString":"Patient"}])),
        "respond-async, separate-export-status",
    )
    .await;
    assert_eq!(
        response.headers()["preference-applied"],
        "respond-async, separate-export-status"
    );
    let status_url = location(&response);
    loop {
        let response = client.get(&status_url).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        match response.headers()["x-export-status"].to_str().unwrap() {
            "202 Accepted" => tokio::time::sleep(Duration::from_millis(100)).await,
            "200 OK" => break,
            other => panic!("unexpected job status {other}"),
        }
    }

    // DELETE: 202, then 404 with an OperationOutcome, and the files are gone.
    let response = kick_off(
        client,
        &system,
        parameters(json!([{"name":"_type","valueString":"Patient"}])),
        ASYNC,
    )
    .await;
    let status_url = location(&response);
    let (_, _, manifest) = wait(client, &status_url).await;
    let file = manifest["output"][0]["url"].as_str().unwrap().to_owned();
    let deleted = client.delete(&status_url).send().await.unwrap();
    assert_eq!(deleted.status(), StatusCode::ACCEPTED);
    let gone = client.get(&status_url).send().await.unwrap();
    assert_eq!(gone.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        gone.json::<Value>().await.unwrap()["resourceType"],
        "OperationOutcome"
    );
    assert_eq!(
        client.get(&file).send().await.unwrap().status(),
        StatusCode::NOT_FOUND
    );

    // deleted: a transaction Bundle per resource removed since _since.
    let before = chrono::Utc::now();
    tokio::time::sleep(Duration::from_millis(50)).await;
    put(client, fhir, json!({"resourceType":"Patient","id":"gone"})).await;
    let response = client
        .delete(format!("{fhir}/Patient/gone"))
        .send()
        .await
        .unwrap();
    assert!(response.status().is_success());
    let manifest = export(
        client,
        &system,
        json!([{"name":"_type","valueString":"Patient"},
               {"name":"_since","valueInstant":before.to_rfc3339()}]),
    )
    .await;
    assert_eq!(manifest["output"], json!([]));
    let bundles = section(client, &manifest, "deleted").await;
    assert_eq!(bundles.len(), 1);
    assert_eq!(bundles[0]["type"], "transaction");
    assert_eq!(bundles[0]["entry"][0]["request"]["method"], "DELETE");
    assert_eq!(bundles[0]["entry"][0]["request"]["url"], "Patient/gone");
}

async fn capabilities(client: &Client, fhir: &str) {
    let metadata: Value = client
        .get(format!("{fhir}/metadata"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        metadata["instantiates"][0],
        "http://hl7.org/fhir/uv/bulkdata/CapabilityStatement/bulk-data"
    );
    let operations = metadata["rest"][0]["operation"].as_array().unwrap();
    for (name, definition) in [
        ("export", "export"),
        ("bulk-submit", "bulk-submit"),
        ("bulk-submit-status", "bulk-submit-status"),
    ] {
        let operation = operations
            .iter()
            .find(|operation| operation["name"] == name)
            .unwrap_or_else(|| panic!("{name} is declared"));
        assert_eq!(
            operation["definition"],
            format!("http://hl7.org/fhir/uv/bulkdata/OperationDefinition/{definition}")
        );
    }
    let resource = |name: &str| {
        metadata["rest"][0]["resource"]
            .as_array()
            .unwrap()
            .iter()
            .find(|resource| resource["type"] == name)
            .cloned()
            .unwrap()
    };
    for (name, definition) in [("Patient", "patient-export"), ("Group", "group-export")] {
        let operation = resource(name)["operation"]
            .as_array()
            .unwrap()
            .iter()
            .find(|operation| operation["name"] == "export")
            .cloned()
            .unwrap();
        assert_eq!(
            operation["definition"],
            format!("http://hl7.org/fhir/uv/bulkdata/OperationDefinition/{definition}")
        );
    }
}

fn submit_request(submission: &str, extra: Value) -> Value {
    let mut parameter = vec![
        json!({"name":"submitter","valueIdentifier":{"system":SUBMITTER.0,"value":SUBMITTER.1}}),
        json!({"name":"submissionId","valueString":submission}),
    ];
    parameter.extend(extra.as_array().cloned().unwrap_or_default());
    parameters(json!(parameter))
}

fn status_parameter(code: &str) -> Value {
    json!({"name":"submissionStatus","valueCoding":{"system":EVENT_STATUS,"code":code}})
}

/// Mounts a manifest and its files at `name`; returns the manifest URL.
async fn provide(provider: &MockServer, name: &str, files: &[(&str, Vec<Value>)]) -> String {
    let mut output = Vec::new();
    for (file, resources) in files {
        let body: String = resources
            .iter()
            .map(|resource| format!("{resource}\n"))
            .collect();
        let file_path = format!("/{name}/{file}.ndjson");
        Mock::given(method("GET"))
            .and(path(file_path.clone()))
            .and(header("x-file-key", "secret"))
            .respond_with(ResponseTemplate::new(200).set_body_string(body))
            .mount(provider)
            .await;
        output.push(json!({"type":file,"url":format!("{}{file_path}", provider.uri())}));
    }
    let manifest_path = format!("/{name}/manifest.json");
    Mock::given(method("GET"))
        .and(path(manifest_path.clone()))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "transactionTime": "2026-01-01T00:00:00Z",
            "requiresAccessToken": false,
            "output": output,
        })))
        .mount(provider)
        .await;
    format!("{}{manifest_path}", provider.uri())
}

fn manifest_parameters(url: &str, provider: &MockServer) -> Vec<Value> {
    vec![
        json!({"name":"manifestUrl","valueUrl":url}),
        json!({"name":"fhirBaseUrl","valueUrl":format!("{}/fhir", provider.uri())}),
        json!({"name":"fileRequestHeader","part":[
            {"name":"headerName","valueString":"x-file-key"},
            {"name":"headerValue","valueString":"secret"}]}),
    ]
}

async fn submit(client: &Client, fhir: &str, body: Value) -> Response {
    client
        .post(format!("{fhir}/$bulk-submit"))
        .header("Accept", "application/fhir+json")
        .json(&body)
        .send()
        .await
        .unwrap()
}

/// `$bulk-submit-status` until the submission is finished; its manifest.
async fn submission_status(client: &Client, fhir: &str, submission: &str) -> Value {
    let response = kick_off(
        client,
        &format!("{fhir}/$bulk-submit-status"),
        submit_request(submission, json!([])),
        ASYNC,
    )
    .await;
    let (code, _, manifest) = wait(client, &location(&response)).await;
    assert_eq!(code, StatusCode::OK, "{manifest}");
    manifest
}

async fn read_status(client: &Client, fhir: &str, path: &str) -> StatusCode {
    client
        .get(format!("{fhir}/{path}"))
        .send()
        .await
        .unwrap()
        .status()
}

async fn bulk_submit(client: &Client, fhir: &str) {
    let provider = MockServer::start().await;
    let base = format!("{}/fhir", provider.uri());
    let url = provide(
        &provider,
        "first",
        &[
            // The Observation file comes first and points at a Patient of a later file.
            (
                "Observation",
                vec![
                    json!({"resourceType":"Observation","id":"sub-o1","status":"final",
                        "code":{"text":"x"},"subject":{"reference":format!("{base}/Patient/sub-p1")}}),
                    json!({"resourceType":"Observation","id":"sub-bad","status":"not-a-status",
                        "code":{"text":"x"}}),
                ],
            ),
            (
                "Patient",
                vec![json!({"resourceType":"Patient","id":"sub-p1"}), json!({"resourceType":"Observation","id":"misfiled"})],
            ),
        ],
    )
    .await;

    // Refusals.
    let unknown = parameters(json!([
        {"name":"submitter","valueIdentifier":{"system":"urn:other","value":"x"}},
        {"name":"submissionId","valueString":"s0"},
        status_parameter("completed"),
    ]));
    assert_eq!(
        submit(client, fhir, unknown).await.status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        submit(
            client,
            fhir,
            submit_request("s0", json!([{"name":"manifestUrl","valueUrl":url}]))
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST,
        "fhirBaseUrl goes with manifestUrl"
    );
    let get = client
        .get(format!("{fhir}/$bulk-submit"))
        .send()
        .await
        .unwrap();
    assert_eq!(get.status(), StatusCode::METHOD_NOT_ALLOWED);

    // A submission: resources are stored, bad ones are reported, references resolved.
    let response = submit(
        client,
        fhir,
        submit_request("s1", json!(manifest_parameters(&url, &provider))),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let again = submit(
        client,
        fhir,
        submit_request("s1", json!(manifest_parameters(&url, &provider))),
    )
    .await;
    assert_eq!(
        again.status(),
        StatusCode::BAD_REQUEST,
        "a manifest URL is unique"
    );

    // 202 until the Data Provider closes the submission.
    let polling = kick_off(
        client,
        &format!("{fhir}/$bulk-submit-status"),
        submit_request("s1", json!([])),
        ASYNC,
    )
    .await;
    let status_url = location(&polling);
    let poll = client.get(&status_url).send().await.unwrap();
    assert_eq!(poll.status(), StatusCode::ACCEPTED);
    assert_eq!(poll.json::<Value>().await.unwrap()["submissionId"], "s1");

    let closed = submit(
        client,
        fhir,
        submit_request("s1", json!([status_parameter("completed")])),
    )
    .await;
    assert_eq!(closed.status(), StatusCode::OK);
    let (code, _, manifest) = wait(client, &status_url).await;
    assert_eq!(code, StatusCode::OK, "{manifest}");
    assert_eq!(manifest["submissionId"], "s1");
    assert_eq!(
        manifest["manifestType"],
        "http://hl7.org/fhir/uv/bulkdata/StructureDefinition/BulkSubmitStatusManifest"
    );
    assert_eq!(manifest["outcome"][0]["manifestUrl"], url);
    assert!(manifest["outcome"][0]["fileSize"].as_u64().unwrap() > 0);
    let outcomes = section(client, &manifest, "outcome").await;
    let source = outcomes
        .iter()
        .find(|outcome| {
            outcome["extension"][0]["valueReference"]["reference"]
                == format!("{base}/Observation/sub-bad")
        })
        .expect("the invalid resource is named");
    assert_eq!(source["issue"][0]["severity"], "error");
    assert!(outcomes.iter().any(|outcome| {
        outcome["issue"][0]["diagnostics"]
            .as_str()
            .is_some_and(|text| text.contains("Expected Patient"))
    }));
    assert!(
        outcomes
            .iter()
            .any(|outcome| outcome["issue"][0]["severity"] == "information")
    );

    assert_eq!(
        read_status(client, fhir, "Patient/sub-p1").await,
        StatusCode::OK
    );
    assert_eq!(
        read_status(client, fhir, "Observation/sub-bad").await,
        StatusCode::NOT_FOUND
    );
    let stored: Value = client
        .get(format!("{fhir}/Observation/sub-o1"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(stored["subject"]["reference"], "Patient/sub-p1");

    // A closed submission takes nothing more.
    let late = submit(
        client,
        fhir,
        submit_request("s1", json!([status_parameter("completed")])),
    )
    .await;
    assert_eq!(late.status(), StatusCode::CONFLICT);

    // stopped takes back what was created and restores what was changed.
    put(
        client,
        fhir,
        json!({"resourceType":"Patient","id":"sub-old","gender":"female"}),
    )
    .await;
    let url = provide(
        &provider,
        "stopped",
        &[(
            "Patient",
            vec![
                json!({"resourceType":"Patient","id":"sub-old","gender":"male"}),
                json!({"resourceType":"Patient","id":"sub-new"}),
            ],
        )],
    )
    .await;
    submit(
        client,
        fhir,
        submit_request("s2", json!(manifest_parameters(&url, &provider))),
    )
    .await;
    wait_for(client, fhir, "Patient/sub-new", StatusCode::OK).await;
    let stopped = submit(
        client,
        fhir,
        submit_request("s2", json!([status_parameter("stopped")])),
    )
    .await;
    assert_eq!(stopped.status(), StatusCode::OK);
    let manifest = submission_status(client, fhir, "s2").await;
    assert!(
        section(client, &manifest, "outcome")
            .await
            .iter()
            .any(|outcome| {
                outcome["issue"][0]["diagnostics"]
                    .as_str()
                    .is_some_and(|text| text.contains("taken back"))
            })
    );
    assert_eq!(
        read_status(client, fhir, "Patient/sub-new").await,
        StatusCode::GONE
    );
    let restored: Value = client
        .get(format!("{fhir}/Patient/sub-old"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(restored["gender"], "female");

    // replacesManifestUrl swaps one manifest's data and leaves the others alone.
    let keep = provide(
        &provider,
        "keep",
        &[(
            "Patient",
            vec![json!({"resourceType":"Patient","id":"kept"})],
        )],
    )
    .await;
    let wrong = provide(
        &provider,
        "wrong",
        &[(
            "Patient",
            vec![json!({"resourceType":"Patient","id":"wrong"})],
        )],
    )
    .await;
    let right = provide(
        &provider,
        "right",
        &[(
            "Patient",
            vec![json!({"resourceType":"Patient","id":"right"})],
        )],
    )
    .await;
    submit(
        client,
        fhir,
        submit_request("s3", json!(manifest_parameters(&keep, &provider))),
    )
    .await;
    submit(
        client,
        fhir,
        submit_request("s3", json!(manifest_parameters(&wrong, &provider))),
    )
    .await;
    wait_for(client, fhir, "Patient/wrong", StatusCode::OK).await;
    let mut replacing = manifest_parameters(&right, &provider);
    replacing.push(json!({"name":"replacesManifestUrl","valueUrl":wrong}));
    let response = submit(client, fhir, submit_request("s3", json!(replacing))).await;
    assert_eq!(response.status(), StatusCode::OK);
    submit(
        client,
        fhir,
        submit_request("s3", json!([status_parameter("completed")])),
    )
    .await;
    submission_status(client, fhir, "s3").await;
    assert_eq!(
        read_status(client, fhir, "Patient/kept").await,
        StatusCode::OK
    );
    assert_eq!(
        read_status(client, fhir, "Patient/right").await,
        StatusCode::OK
    );
    assert_eq!(
        read_status(client, fhir, "Patient/wrong").await,
        StatusCode::GONE
    );

    // What cannot be honoured is refused.
    let oauth = submit_request(
        "s4",
        json!([status_parameter("completed"),
            {"name":"oauthMetadataUrl","valueUrl":"https://example.org/.well-known/smart-configuration"}]),
    );
    assert!(submit(client, fhir, oauth).await.status().is_client_error());
}

/// `$expunge` lives here for the server it shares: it removes what history keeps.
async fn expunge(client: &Client, fhir: &str) {
    put(
        client,
        fhir,
        json!({"resourceType":"Patient","id":"exp","gender":"male"}),
    )
    .await;
    put(
        client,
        fhir,
        json!({"resourceType":"Patient","id":"exp","gender":"female"}),
    )
    .await;
    let url = format!("{fhir}/Patient/exp/$expunge");
    let preview: Value = client
        .post(&url)
        .json(&parameters(json!([{"name":"dryRun","valueBoolean":true}])))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(preview["parameter"][2]["valueInteger"], 2, "{preview}");
    assert_eq!(
        read_status(client, fhir, "Patient/exp").await,
        StatusCode::OK
    );

    let removed = client
        .post(&url)
        .json(&parameters(json!([])))
        .send()
        .await
        .unwrap();
    assert_eq!(removed.status(), StatusCode::OK);
    assert_eq!(
        read_status(client, fhir, "Patient/exp").await,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        read_status(client, fhir, "Patient/exp/_history").await,
        StatusCode::NOT_FOUND,
        "no version may stay behind"
    );
    let again = client
        .post(&url)
        .json(&parameters(json!([])))
        .send()
        .await
        .unwrap();
    assert_eq!(again.status(), StatusCode::NOT_FOUND);
}

async fn wait_for(client: &Client, fhir: &str, path: &str, expected: StatusCode) {
    for _ in 0..240 {
        if read_status(client, fhir, path).await == expected {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("{path} never became {expected}");
}

async fn stu4_export(client: &Client, fhir: &str) {
    let system = format!("{fhir}/$export");
    let get = client
        .get(&system)
        .header("Prefer", ASYNC)
        .send()
        .await
        .unwrap();
    assert_eq!(get.status(), StatusCode::METHOD_NOT_ALLOWED);
    let comma = parameters(json!([{"name":"_type","valueString":"Patient,Observation"}]));
    assert_eq!(
        kick_off(client, &system, comma, ASYNC).await.status(),
        StatusCode::BAD_REQUEST
    );
    let response = kick_off(
        client,
        &system,
        parameters(json!([
            {"name":"_type","valueString":"Patient"},
            {"name":"unknown","valueString":"x"}
        ])),
        "respond-async, handling=lenient",
    )
    .await;
    let (_, _, manifest) = wait(client, &location(&response)).await;
    assert_eq!(
        manifest["manifestType"],
        "http://hl7.org/fhir/uv/bulkdata/StructureDefinition/BulkDataManifest"
    );
    assert_eq!(manifest["outputFormat"], "application/fhir+ndjson");
    assert!(manifest.get("request").is_none());
    assert!(manifest.get("error").is_none());
    assert!(manifest["output"][0]["fileSize"].as_u64().unwrap() > 0);
    assert_eq!(
        manifest["outcome"][0]["countSeverity"][0]["code"],
        "warning"
    );
    assert_eq!(section(client, &manifest, "outcome").await.len(), 1);
}

async fn postgres_url() -> (testcontainers::ContainerAsync<Postgres>, String) {
    let container = Postgres::default()
        .with_tag("18-alpine")
        .start()
        .await
        .expect("postgres container");
    let port = container.get_host_port_ipv4(5432).await.expect("port");
    (
        container,
        format!("postgres://postgres:postgres@127.0.0.1:{port}/postgres"),
    )
}

#[tokio::test]
async fn stu3_server_exports_and_accepts_submissions() {
    let (_postgres, url) = postgres_url().await;
    let exports = tempfile::tempdir().unwrap();
    let (fhir, _server) = start_server(
        &url,
        exports.path().to_str().unwrap(),
        BulkSpecVersion::Stu3,
    )
    .await;
    let client = Client::new();
    seed(&client, &fhir).await;
    capabilities(&client, &fhir).await;
    stu3_export(&client, &fhir).await;
    bulk_submit(&client, &fhir).await;
    expunge(&client, &fhir).await;
}

#[tokio::test]
async fn stu4_server_follows_the_ballot() {
    let (_postgres, url) = postgres_url().await;
    let exports = tempfile::tempdir().unwrap();
    let (fhir, _server) = start_server(
        &url,
        exports.path().to_str().unwrap(),
        BulkSpecVersion::Stu4,
    )
    .await;
    let client = Client::new();
    seed(&client, &fhir).await;
    stu4_export(&client, &fhir).await;
}
