//! Metadata import across HTTP, a mock PACS and a disposable PostgreSQL server.
//! Exercises atomic writes, real validation, idempotence and patient isolation.

use std::sync::Arc;

use octofhir_config::ConfigurationManager;
use octofhir_server::{AppConfig, PostgresStorageConfig, build_app};
use serde_json::{Value, json};
use testcontainers::{ImageExt, runners::AsyncRunner};
use testcontainers_modules::postgres::Postgres;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

fn metadata(patient_id: &str) -> Value {
    json!([{
        "0020000D":{"vr":"UI","Value":["1.2.3"]},
        "0020000E":{"vr":"UI","Value":["1.2.3.1"]},
        "00080018":{"vr":"UI","Value":["1.2.3.1.1"]},
        "00080016":{"vr":"UI","Value":["1.2.840.10008.5.1.4.1.1.2"]},
        "00080060":{"vr":"CS","Value":["CT"]},
        "00100020":{"vr":"LO","Value":[patient_id]},
        "00100021":{"vr":"LO","Value":["HOSPITAL"]},
        "00081030":{"vr":"LO","Value":["Test study"]},
        "7FE00010":{"vr":"OW","BulkDataURI":"https://pacs.example/pixels-are-not-imported"}
    }])
}

fn params() -> Value {
    json!({"resourceType":"Parameters","parameter":[
        {"name":"connection","valueString":"pacs"},{"name":"study","valueString":"1.2.3"}
    ]})
}

#[test]
fn imports_only_metadata_and_preserves_fhir_semantics() {
    std::thread::Builder::new()
        .name("dicom-import".into())
        .stack_size(8 * 1024 * 1024)
        .spawn(|| {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(Box::pin(scenario()))
        })
        .unwrap()
        .join()
        .unwrap();
}

async fn scenario() {
    let postgres = Postgres::default()
        .with_tag("18-alpine")
        .start()
        .await
        .unwrap();
    let port = postgres.get_host_port_ipv4(5432).await.unwrap();
    let pacs = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/dicom/studies/1.2.3/metadata"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw(metadata("MRN-1").to_string(), "application/dicom+json"),
        )
        .mount(&pacs)
        .await;
    let mut config = AppConfig::default();
    config.fhir.version = "R4".into();
    config.storage.postgres = Some(PostgresStorageConfig {
        url: Some(format!(
            "postgres://postgres:postgres@127.0.0.1:{port}/postgres"
        )),
        pool_size: 8,
        ..Default::default()
    });
    config.packages.load = vec![octofhir_server::config::PackageSpec::Table {
        id: Some("hl7.fhir.r4.core".into()),
        version: Some("4.0.1".into()),
        path: None,
        url: Some("https://hl7.org/fhir/R4/hl7.fhir.r4.core.tgz".into()),
    }];
    config.auth.policy.anonymous_access = true;
    config.bootstrap.backend_client = Some(octofhir_server::config::BackendClientConfig {
        client_id: "dicom-test".into(),
        client_secret: Some("isolated-dicom-secret".into()),
        scopes: vec!["system/*.cruds".into()],
        name: None,
        description: None,
    });
    config.server.outbound_allowlist = vec!["127.0.0.1/32".into()];
    config.dicom = serde_json::from_value(json!({"connections":{"pacs":{
        "wado_url":format!("{}/dicom",pacs.uri()),"allow_http":true,
        "patient_identifier_systems":{"HOSPITAL":"urn:test:mrn"}
    }}}))
    .unwrap();
    let manager = Arc::new(ConfigurationManager::builder().build().await.unwrap());
    let app = build_app(&config, manager).await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/fhir", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = reqwest::Client::new();
    let response = client
        .post(format!("{base}/Patient"))
        .json(&json!({
            "resourceType":"Patient","identifier":[{"system":"urn:test:mrn","value":"MRN-1"}]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 201);
    let patient: Value = response.json().await.unwrap();
    let patient_id = patient["id"].as_str().unwrap();
    let operation = format!("{base}/Patient/{patient_id}/$dicom-import");
    // A narrow authenticated grant must not inherit the PACS service credential.
    let count_before = pacs.received_requests().await.unwrap().len();
    let token_response = client
        .post(format!("{}/auth/token", base.trim_end_matches("/fhir")))
        .basic_auth("dicom-test", Some("isolated-dicom-secret"))
        .form(&[
            ("grant_type", "client_credentials"),
            ("scope", "system/Patient.r"),
        ])
        .send()
        .await
        .unwrap();
    assert_eq!(token_response.status(), 200);
    let token: Value = token_response.json().await.unwrap();
    let denied = client
        .post(&operation)
        .bearer_auth(token["access_token"].as_str().unwrap())
        .json(&params())
        .send()
        .await
        .unwrap();
    assert_eq!(denied.status(), 403, "{}", denied.text().await.unwrap());
    assert_eq!(pacs.received_requests().await.unwrap().len(), count_before);
    let full_token = client
        .post(format!("{}/auth/token", base.trim_end_matches("/fhir")))
        .basic_auth("dicom-test", Some("isolated-dicom-secret"))
        .form(&[
            ("grant_type", "client_credentials"),
            ("scope", "system/*.cruds"),
        ])
        .send()
        .await
        .unwrap();
    assert_eq!(full_token.status(), 200);
    let full_token: Value = full_token.json().await.unwrap();
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        reqwest::header::AUTHORIZATION,
        format!("Bearer {}", full_token["access_token"].as_str().unwrap())
            .parse()
            .unwrap(),
    );
    let client = reqwest::Client::builder()
        .default_headers(headers)
        .build()
        .unwrap();
    let response = client
        .post(&operation)
        .json(&params())
        .send()
        .await
        .unwrap();
    let status = response.status();
    let result: Value = response.json().await.unwrap();
    assert_eq!(status, 200, "{result}");
    assert_eq!(result["parameter"][2]["valueBoolean"], true);
    let study_ref = result["parameter"][0]["valueReference"]["reference"]
        .as_str()
        .unwrap();
    let response = client
        .get(format!("{base}/{study_ref}"))
        .send()
        .await
        .unwrap();
    let study: Value = response.json().await.unwrap();
    assert_eq!(
        study["subject"]["reference"],
        format!("Patient/{patient_id}")
    );
    assert_eq!(study["numberOfInstances"], 1);
    assert!(!study.to_string().contains("pixels-are-not-imported"));
    assert!(study.get("data").is_none());
    let version = study["meta"]["versionId"].clone();
    let response = client
        .post(&operation)
        .json(&params())
        .send()
        .await
        .unwrap();
    let status = response.status();
    let repeated: Value = response.json().await.unwrap();
    assert_eq!(status, 200, "{repeated}");
    assert_eq!(repeated["parameter"][2]["valueBoolean"], false);
    let unchanged: Value = client
        .get(format!("{base}/{study_ref}"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(unchanged["meta"]["versionId"], version);

    // A changed projection updates only importer-owned fields.
    let mut edited = study.clone();
    edited["note"] = json!([{"text":"Keep clinical annotation"}]);
    let response = client
        .put(format!("{base}/{study_ref}"))
        .json(&edited)
        .send()
        .await
        .unwrap();
    assert!(
        response.status().is_success(),
        "{}",
        response.text().await.unwrap()
    );
    pacs.reset().await;
    let mut changed = metadata("MRN-1");
    changed[0]["00081030"]["Value"] = json!(["Updated study"]);
    Mock::given(method("GET"))
        .and(path("/dicom/studies/1.2.3/metadata"))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(changed.to_string(), "application/dicom+json"),
        )
        .mount(&pacs)
        .await;
    let response = client
        .post(&operation)
        .json(&params())
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200, "{}", response.text().await.unwrap());
    let updated: Value = client
        .get(format!("{base}/{study_ref}"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(updated["description"], "Updated study");
    assert_eq!(updated["note"][0]["text"], "Keep clinical annotation");

    // Wrong-patient metadata cannot modify either imported FHIR resource.
    pacs.reset().await;
    Mock::given(method("GET"))
        .and(path("/dicom/studies/1.2.3/metadata"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw(metadata("OTHER").to_string(), "application/dicom+json"),
        )
        .mount(&pacs)
        .await;
    let response = client
        .post(&operation)
        .json(&params())
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 422);
    let after: Value = client
        .get(format!("{base}/{study_ref}"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(after["meta"]["versionId"], updated["meta"]["versionId"]);
    assert!(!after.to_string().contains("OTHER"));
    // Only metadata requests have reached the PACS, no frames or objects.
    assert!(
        pacs.received_requests().await.unwrap().iter().all(
            |r| r.url.path().ends_with("/metadata") && r.headers.get("authorization").is_none()
        )
    );
    server.abort();
    let _ = server.await;
}
