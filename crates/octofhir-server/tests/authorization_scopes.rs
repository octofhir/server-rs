//! End-to-end SMART grant enforcement, including Bundle entries and revocation.
//! Requires Docker; all resources and credentials belong to a disposable DB.

use std::sync::Arc;

use octofhir_config::ConfigurationManager;
use octofhir_server::config::BackendClientConfig;
use octofhir_server::{AppConfig, PostgresStorageConfig, build_app};
use reqwest::{Client, StatusCode};
use serde_json::{Value, json};
use testcontainers::{ImageExt, runners::AsyncRunner};
use testcontainers_modules::postgres::Postgres;

async fn token(base: &str, scope: &str) -> String {
    let response = Client::new()
        .post(format!("{base}/auth/token"))
        .basic_auth("development-backend", Some("isolated-test-secret"))
        .form(&[("grant_type", "client_credentials"), ("scope", scope)])
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["scope"], scope);
    body["access_token"].as_str().unwrap().to_owned()
}

#[tokio::test]
async fn smart_grants_bound_policies_and_bundle_entries_and_revoked_tokens() {
    let postgres = Postgres::default()
        .with_tag("18-alpine")
        .start()
        .await
        .unwrap();
    let port = postgres.get_host_port_ipv4(5432).await.unwrap();
    let mut config = AppConfig::default();
    config.storage.postgres = Some(PostgresStorageConfig {
        url: Some(format!(
            "postgres://postgres:postgres@127.0.0.1:{port}/postgres"
        )),
        pool_size: 10,
        connect_timeout_ms: 30000,
        ..Default::default()
    });
    config.bootstrap.backend_client = Some(BackendClientConfig {
        client_id: "development-backend".into(),
        client_secret: Some("isolated-test-secret".into()),
        scopes: vec!["system/*.cruds".into()],
        name: None,
        description: None,
    });
    assert!(!config.auth.policy.anonymous_access);
    let manager = Arc::new(ConfigurationManager::builder().build().await.unwrap());
    let app = build_app(&config, manager).await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = Client::new();
    let full = token(&base, "system/*.cruds").await;
    let read = token(&base, "system/Patient.r").await;
    let search = token(&base, "system/Patient.s").await;
    let create = token(&base, "system/Patient.c").await;
    let fhir = format!("{base}/fhir");

    // Bootstrap's broad grant still permits development writes.
    let response = client
        .post(format!("{fhir}/Patient"))
        .bearer_auth(&full)
        .json(&json!({"resourceType":"Patient","active":true}))
        .send()
        .await
        .unwrap();
    let status = response.status();
    let patient: Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::CREATED, "{patient}");
    let id = patient["id"].as_str().unwrap();
    let version = patient["meta"]["versionId"].as_str().unwrap();

    for path in [
        format!("Patient/{id}"),
        format!("Patient/{id}/_history"),
        format!("Patient/{id}/_history/{version}"),
    ] {
        assert_eq!(
            client
                .get(format!("{fhir}/{path}"))
                .bearer_auth(&read)
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::OK,
            "read scope: {path}"
        );
    }
    for path in ["Patient", "Patient/_history"] {
        assert_eq!(
            client
                .get(format!("{fhir}/{path}"))
                .bearer_auth(&read)
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::FORBIDDEN,
            "read must not grant search: {path}"
        );
        assert_eq!(
            client
                .get(format!("{fhir}/{path}"))
                .bearer_auth(&search)
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::OK,
            "search scope: {path}"
        );
    }
    assert_eq!(
        client
            .get(format!("{fhir}/Patient/{id}"))
            .bearer_auth(&search)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        client
            .post(format!("{fhir}/Patient/_search"))
            .bearer_auth(&search)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body("active=true")
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        client
            .post(format!("{fhir}/Patient"))
            .bearer_auth(&read)
            .json(&json!({"resourceType":"Patient"}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );

    let batch = json!({"resourceType":"Bundle","type":"batch","entry":[
        {"request":{"method":"GET","url":format!("Patient/{id}")}},
        {"resource":{"resourceType":"Patient"},"request":{"method":"POST","url":"Patient"}},
        {"request":{"method":"GET","url":"Observation"}}
    ]});
    let response = client
        .post(&fhir)
        .bearer_auth(&read)
        .json(&batch)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = response.json().await.unwrap();
    assert!(
        body["entry"][0]["response"]["status"]
            .as_str()
            .unwrap()
            .starts_with("200")
    );
    for index in [1, 2] {
        assert!(
            body["entry"][index]["response"]["status"]
                .as_str()
                .unwrap()
                .starts_with("403"),
            "{body}"
        );
    }

    // A forbidden sibling must prevent every transaction write.
    let transaction = json!({"resourceType":"Bundle","type":"transaction","entry":[
        {"resource":{"resourceType":"Patient","identifier":[{"system":"urn:test","value":"must-not-exist"}]},"request":{"method":"POST","url":"Patient"}},
        {"resource":{"resourceType":"Observation","status":"final","code":{"text":"test"}},"request":{"method":"POST","url":"Observation"}}
    ]});
    assert_eq!(
        client
            .post(&fhir)
            .bearer_auth(&create)
            .json(&transaction)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    let response = client
        .get(format!(
            "{fhir}/Patient?identifier=urn:test%7Cmust-not-exist"
        ))
        .bearer_auth(&full)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = response.json().await.unwrap();
    assert!(
        body["entry"]
            .as_array()
            .is_none_or(|entries| entries.is_empty()),
        "{body}"
    );

    let response = client
        .post(format!("{base}/auth/introspect"))
        .basic_auth("development-backend", Some("isolated-test-secret"))
        .form(&[("token", read.as_str())])
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["active"], true);
    assert_eq!(
        client
            .post(format!("{base}/auth/revoke"))
            .basic_auth("development-backend", Some("isolated-test-secret"))
            .form(&[
                ("token", read.as_str()),
                // A wrong hint must not suppress access-token revocation.
                ("token_type_hint", "refresh_token")
            ])
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        client
            .get(format!("{fhir}/Patient/{id}"))
            .bearer_auth(&read)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );

    assert_eq!(
        client
            .get(format!("{fhir}/Patient/{id}"))
            .bearer_auth(&full)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        client
            .post(format!("{base}/auth/logout"))
            .bearer_auth(&full)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        client
            .get(format!("{fhir}/Patient/{id}"))
            .bearer_auth(&full)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    server.abort();
}
