//! Durable QIDO discovery and source notifications through real FHIR authorization.
//! Requires Docker for PostgreSQL. PACS transport is an isolated HTTP mock.

use octofhir_config::ConfigurationManager;
use octofhir_server::{AppConfig, PostgresStorageConfig, build_app};
use serde_json::{Value, json};
use sqlx_core::{query::query, row::Row};
use std::{sync::Arc, time::Duration};
use testcontainers::{ImageExt, runners::AsyncRunner};
use testcontainers_modules::postgres::Postgres;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path, query_param},
};

fn dataset(uid: &str, mrn: &str) -> Value {
    json!([{
        "0020000D":{"vr":"UI","Value":[uid]},"0020000E":{"vr":"UI","Value":[format!("{uid}.1")]},
        "00080018":{"vr":"UI","Value":[format!("{uid}.1.1")]},"00080016":{"vr":"UI","Value":["1.2.840.10008.5.1.4.1.1.2"]},
        "00080060":{"vr":"CS","Value":["CT"]},"00100020":{"vr":"LO","Value":[mrn]},"00100021":{"vr":"LO","Value":["HOSPITAL"]},
        "00080020":{"vr":"DA","Value":["19990101"]},"7FE00010":{"vr":"OW","BulkDataURI":"https://never-fetch.example/pixels"}
    }])
}
async fn pacs(server: &MockServer, late: bool, mrn: &str, status: u16) {
    server.reset().await;
    for (offset, uid) in [(0, "1.2.3"), (1, "1.2.4")] {
        let body = if offset == 0 || late {
            json!([{
                "0020000D":{"vr":"UI","Value":[uid]},"00100020":{"vr":"LO","Value":[mrn]},"00100021":{"vr":"LO","Value":["HOSPITAL"]},
                "00201208":{"vr":"IS","Value":[1]},"00080020":{"vr":"DA","Value":["19990101"]}
            }])
        } else {
            json!([])
        };
        Mock::given(method("GET"))
            .and(path("/qido/studies"))
            .and(query_param("offset", offset.to_string()))
            .respond_with(
                ResponseTemplate::new(200).set_body_raw(body.to_string(), "application/dicom+json"),
            )
            .with_priority(1)
            .mount(server)
            .await;
        Mock::given(method("GET"))
            .and(path(format!("/wado/studies/{uid}/metadata")))
            .respond_with(
                ResponseTemplate::new(status)
                    .set_body_raw(dataset(uid, mrn).to_string(), "application/dicom+json"),
            )
            .mount(server)
            .await;
    }
    Mock::given(method("GET"))
        .and(path("/qido/studies"))
        .respond_with(ResponseTemplate::new(200).set_body_raw("[]", "application/dicom+json"))
        .with_priority(10)
        .mount(server)
        .await;
}
async fn serve(config: &AppConfig) -> (String, tokio::task::JoinHandle<()>) {
    let manager = Arc::new(ConfigurationManager::builder().build().await.unwrap());
    let app = build_app(config, manager).await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/fhir", listener.local_addr().unwrap());
    let handle = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (base, handle)
}
async fn token(client: &reqwest::Client, base: &str, scope: &str) -> String {
    let response = client
        .post(format!("{}/auth/token", base.trim_end_matches("/fhir")))
        .basic_auth("sync-service", Some("isolated-sync-secret"))
        .form(&[("grant_type", "client_credentials"), ("scope", scope)])
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    response.json::<Value>().await.unwrap()["access_token"]
        .as_str()
        .unwrap()
        .to_owned()
}
async fn patient(client: &reqwest::Client, base: &str, mrn: &str) -> String {
    let response = client
        .post(format!("{base}/Patient"))
        .json(
            &json!({"resourceType":"Patient","identifier":[{"system":"urn:test:mrn","value":mrn}]}),
        )
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 201);
    response.json::<Value>().await.unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned()
}
async fn wait_study(client: &reqwest::Client, base: &str, uid: &str, status: &str) -> Value {
    let mut last = json!(null);
    for _ in 0..160 {
        let response = client
            .get(format!("{base}/ImagingStudy"))
            .query(&[("_count", "100")])
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        last = response.json().await.unwrap();
        if let Some(resource) = last["entry"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|entry| &entry["resource"])
            .find(|resource| {
                resource["identifier"]
                    .as_array()
                    .is_some_and(|ids| ids.iter().any(|id| id["value"] == format!("urn:oid:{uid}")))
            })
            && resource["status"] == status
        {
            return resource.clone();
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    panic!("Study {uid} did not reach {status}: {last}");
}
async fn notify(
    client: &reqwest::Client,
    base: &str,
    token: &str,
    sequence: u32,
    change: &str,
) -> Value {
    let response=client.post(format!("{base}/$dicom-notify")).bearer_auth(token).json(&json!({"resourceType":"Parameters","parameter":[
        {"name":"connection","valueString":"pacs"},{"name":"study","valueString":"1.2.3"},
        {"name":"sequence","valueString":sequence.to_string()},{"name":"change","valueString":change}
    ]})).send().await.unwrap();
    let status = response.status();
    let body: Value = response.json().await.unwrap();
    assert_eq!(status, 200, "{body}");
    body
}

#[test]
fn qido_reconciles_late_priors_orders_notifications_and_recovers_after_restart() {
    std::thread::Builder::new()
        .name("dicom-sync".into())
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
    let database = format!(
        "postgres://postgres:postgres@127.0.0.1:{}/postgres",
        postgres.get_host_port_ipv4(5432).await.unwrap()
    );
    let archive = MockServer::start().await;
    pacs(&archive, false, "MRN-1", 200).await;
    let mut config = AppConfig::default();
    config.storage.postgres = Some(PostgresStorageConfig {
        url: Some(database.clone()),
        pool_size: 10,
        ..Default::default()
    });
    config.packages.load = vec![octofhir_server::config::PackageSpec::Table {
        id: Some("hl7.fhir.r4.core".into()),
        version: Some("4.0.1".into()),
        path: None,
        url: Some("https://hl7.org/fhir/R4/hl7.fhir.r4.core.tgz".into()),
    }];
    config.auth.policy.anonymous_access = true;
    config.server.outbound_allowlist = vec!["127.0.0.1/32".into()];
    config.bootstrap.backend_client = Some(octofhir_server::config::BackendClientConfig {
        client_id: "sync-service".into(),
        client_secret: Some("isolated-sync-secret".into()),
        scopes: vec!["system/*.cruds".into()],
        name: None,
        description: None,
    });
    config.dicom=serde_json::from_value(json!({"connections":{"pacs":{
        "wado_url":format!("{}/wado",archive.uri()),"qido_url":format!("{}/qido",archive.uri()),"allow_http":true,
        "patient_identifier_systems":{"HOSPITAL":"urn:test:mrn"},
        "sync":{"client_id":"sync-service","scopes":["system/*.cruds"],"rescan_seconds":1,"page_size":1,"pages_per_run":2,"retry_seconds":1}
    }}})).unwrap();
    let (base, server) = serve(&config).await;
    let client = reqwest::Client::new();
    let patient_id = patient(&client, &base, "MRN-1").await;
    let full = token(&client, &base, "system/*.cruds").await;
    let first = wait_study(&client, &base, "1.2.3", "available").await;
    assert_eq!(
        first["subject"]["reference"],
        format!("Patient/{patient_id}")
    );
    assert_eq!(first["started"], "1999-01-01");
    assert!(!first.to_string().contains("never-fetch"));
    let version = first["meta"]["versionId"].clone();
    tokio::time::sleep(Duration::from_secs(3)).await;
    let same = wait_study(&client, &base, "1.2.3", "available").await;
    assert_eq!(same["meta"]["versionId"], version);
    // Only the explicitly configured service principal can enumerate or enqueue.
    let params = json!({"resourceType":"Parameters","parameter":[{"name":"connection","valueString":"pacs"}]});
    let denied = client
        .post(format!("{base}/Patient/{patient_id}/$dicom-discover"))
        .json(&params)
        .send()
        .await
        .unwrap();
    assert_eq!(denied.status(), 403);
    let response = client
        .post(format!("{base}/Patient/{patient_id}/$dicom-discover"))
        .bearer_auth(&full)
        .json(&params)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200, "{}", response.text().await.unwrap());
    let narrow = token(&client, &base, "system/Patient.r").await;
    let denied = client
        .post(format!("{base}/$dicom-sync-status"))
        .bearer_auth(narrow)
        .json(&params)
        .send()
        .await
        .unwrap();
    assert_eq!(denied.status(), 403);
    // A prior with an old acquisition date must still be found on the next full scan.
    pacs(&archive, true, "MRN-1", 200).await;
    wait_study(&client, &base, "1.2.4", "available").await;
    assert!(
        archive
            .received_requests()
            .await
            .unwrap()
            .iter()
            .all(|r| !r.url.query_pairs().any(|(key, _)| key == "StudyDate"))
    );
    let accepted = notify(&client, &base, &full, 5, "rejected").await;
    assert_eq!(accepted["parameter"][0]["valueBoolean"], true);
    let rejected = wait_study(&client, &base, "1.2.3", "unknown").await;
    assert!(
        rejected["extension"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["valueCode"] == "rejected")
    );
    assert_eq!(
        notify(&client, &base, &full, 4, "restored").await["parameter"][0]["valueBoolean"],
        false
    );
    assert_eq!(
        notify(&client, &base, &full, 5, "rejected").await["parameter"][0]["valueBoolean"],
        false
    );
    tokio::time::sleep(Duration::from_secs(2)).await;
    let unchanged = wait_study(&client, &base, "1.2.3", "unknown").await;
    assert_eq!(
        unchanged["meta"]["versionId"],
        rejected["meta"]["versionId"]
    );
    notify(&client, &base, &full, 6, "restored").await;
    wait_study(&client, &base, "1.2.3", "available").await;
    // Network outages are retryable; they do not become delete/rejection events.
    pacs(&archive, true, "MRN-1", 503).await;
    notify(&client, &base, &full, 7, "changed").await;
    let pool = sqlx_postgres::PgPoolOptions::new()
        .max_connections(2)
        .connect(&database)
        .await
        .unwrap();
    let mut failed = false;
    for _ in 0..80 {
        let row=query("SELECT last_error FROM dicom_sync_studies WHERE connection_name='pacs' AND study_uid='1.2.3'").fetch_one(&pool).await.unwrap();
        if row.get::<Option<String>, _>("last_error").is_some() {
            failed = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(failed, "Source failures must remain visible and retryable");
    wait_study(&client, &base, "1.2.3", "available").await;
    server.abort();
    let _ = server.await;
    pacs(&archive, true, "MRN-1", 200).await;
    let (base, server) = serve(&config).await;
    let recovered = wait_study(&client, &base, "1.2.3", "available").await;
    assert_eq!(
        recovered["subject"]["reference"],
        format!("Patient/{patient_id}")
    );
    let mut caught_up = false;
    for _ in 0..120 {
        let row=query("SELECT desired_revision=applied_revision AS caught_up FROM dicom_sync_studies WHERE connection_name='pacs' AND study_uid='1.2.3'").fetch_one(&pool).await.unwrap();
        if row.get::<bool, _>("caught_up") {
            caught_up = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(caught_up, "A restarted server must finish persisted work");
    // Corrected archive identity cannot silently reassign an existing study.
    patient(&client, &base, "MRN-2").await;
    pacs(&archive, true, "MRN-2", 200).await;
    let full = token(&client, &base, "system/*.cruds").await;
    notify(&client, &base, &full, 8, "changed").await;
    tokio::time::sleep(Duration::from_secs(3)).await;
    let pinned = wait_study(&client, &base, "1.2.3", "available").await;
    assert_eq!(
        pinned["subject"]["reference"],
        format!("Patient/{patient_id}")
    );
    assert!(
        archive.received_requests().await.unwrap().iter().all(|r| r
            .url
            .path()
            .ends_with("/metadata")
            || r.url.path() == "/qido/studies")
    );
    server.abort();
    let _ = server.await;
    pool.close().await;
}
