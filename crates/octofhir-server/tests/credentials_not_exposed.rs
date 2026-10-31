//! Credential hashes stored in `User` must never appear in API responses, and an update that
//! sends a user back without its hash must keep the stored one.
//!
//! Requirements:
//! - Docker running (for testcontainers)

use std::sync::Arc;

use octofhir_config::ConfigurationManager;
use octofhir_server::{AppConfig, PostgresStorageConfig, build_app};
use serde_json::{Value, json};
use testcontainers::{ImageExt, runners::AsyncRunner};
use testcontainers_modules::postgres::Postgres;

struct TestServerHandle(tokio::task::JoinHandle<()>);

impl Drop for TestServerHandle {
    fn drop(&mut self) {
        self.0.abort();
    }
}

async fn start_server(postgres_url: &str) -> (String, TestServerHandle) {
    let mut config = AppConfig::default();
    config.storage.postgres = Some(PostgresStorageConfig {
        url: Some(postgres_url.to_string()),
        pool_size: 5,
        connect_timeout_ms: 10000,
        idle_timeout_ms: Some(60000),
        ..Default::default()
    });
    config.auth.policy.anonymous_access = true;

    let config_manager = Arc::new(
        ConfigurationManager::builder()
            .build()
            .await
            .expect("build config manager"),
    );
    let app = build_app(&config, config_manager).await.expect("build app");
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("bind");
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (format!("http://{addr}"), TestServerHandle(server))
}

async fn get_text(client: &reqwest::Client, url: &str) -> String {
    let response = client
        .get(url)
        .header("accept", "application/fhir+json")
        .send()
        .await
        .expect("request");
    assert!(
        response.status().is_success(),
        "{url}: {}",
        response.status()
    );
    response.text().await.expect("body")
}

async fn stored_password_hash(postgres_url: &str, id: &str) -> Option<String> {
    let pool = sqlx_postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(postgres_url)
        .await
        .expect("connect postgres");
    let hash: Option<String> = sqlx_core::query_scalar::query_scalar(
        r#"SELECT COALESCE(resource->>'passwordHash', resource->>'password_hash') FROM "user" WHERE id::text = $1"#,
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .expect("read stored user");
    pool.close().await;
    hash
}

#[tokio::test]
async fn user_password_hashes_never_leave_the_server() {
    let container = Postgres::default()
        .with_tag("17-alpine")
        .start()
        .await
        .expect("start postgres container");
    let port = container.get_host_port_ipv4(5432).await.expect("port");
    let postgres_url = format!("postgres://postgres:postgres@127.0.0.1:{port}/postgres");
    let (root, _handle) = start_server(&postgres_url).await;
    let fhir = format!("{root}/fhir");
    let client = reqwest::Client::new();

    let created: Value = client
        .post(format!("{fhir}/User"))
        .header("content-type", "application/fhir+json")
        .json(&json!({
            "resourceType": "User",
            "username": "dana.nurse",
            "password": "integration-test-password",
            "email": "dana@example.org",
            "active": true
        }))
        .send()
        .await
        .expect("create user")
        .json()
        .await
        .expect("created body");
    let id = created["id"].as_str().expect("user id").to_owned();
    let version = created["meta"]["versionId"]
        .as_str()
        .expect("version")
        .to_owned();
    let original_hash = stored_password_hash(&postgres_url, &id)
        .await
        .expect("password hashed on create");
    assert!(original_hash.starts_with("$argon2"));

    for url in [
        format!("{fhir}/User/{id}"),
        format!("{fhir}/User/{id}/_history/{version}"),
        format!("{fhir}/User?username=dana.nurse"),
        format!("{fhir}/User/_history"),
        format!("{fhir}/User/{id}/_history"),
        format!("{fhir}/_history"),
        format!("{root}/User/{id}"),
        format!("{root}/User"),
    ] {
        let body = get_text(&client, &url).await;
        assert!(body.contains(&id), "{url} should return the user");
        for leaked in ["passwordHash", "password_hash", "$argon2"] {
            assert!(!body.contains(leaked), "{url} exposes {leaked}");
        }
    }

    // A client that read the (scrubbed) user and saves it back must not wipe the password.
    let mut edited: Value =
        serde_json::from_str(&get_text(&client, &format!("{fhir}/User/{id}")).await)
            .expect("user json");
    edited["email"] = json!("dana.n@example.org");
    let response = client
        .put(format!("{fhir}/User/{id}"))
        .header("content-type", "application/fhir+json")
        .json(&edited)
        .send()
        .await
        .expect("update user");
    assert!(
        response.status().is_success(),
        "update: {}",
        response.status()
    );
    let body = response.text().await.expect("update body");
    assert!(!body.contains("argon2"), "update response exposes the hash");
    assert_eq!(
        stored_password_hash(&postgres_url, &id).await.as_deref(),
        Some(original_hash.as_str()),
        "update without a hash keeps the stored password"
    );

    // Client secrets are hashed on write and must not come back either.
    let response = client
        .post(format!("{fhir}/Client"))
        .header("content-type", "application/fhir+json")
        .json(&json!({
            "resourceType": "Client",
            "clientId": "leak-test",
            "name": "Leak test",
            "clientSecret": "plain-secret-for-the-test",
            "grantTypes": ["client_credentials"],
            "confidential": true,
            "active": true
        }))
        .send()
        .await
        .expect("create client");
    let status = response.status();
    let created_client: Value = response.json().await.expect("client json");
    assert!(
        status.is_success(),
        "create client: {status} {created_client}"
    );
    let client_id = created_client["id"].as_str().expect("client id").to_owned();
    for url in [
        format!("{fhir}/Client/{client_id}"),
        format!("{fhir}/Client"),
        format!("{root}/Client/{client_id}"),
    ] {
        let body = get_text(&client, &url).await;
        assert!(
            !body.contains("$argon2"),
            "{url} exposes the client secret hash"
        );
    }
}
