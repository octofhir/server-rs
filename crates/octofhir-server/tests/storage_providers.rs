//! Provider administration, encrypted write-only keys and create-only restart seeds.
//! The child owns its encryption-key environment; PostgreSQL is disposable.
use octofhir_config::{ConfigurationManager, SecretValue, Secrets};
use octofhir_server::{
    AppConfig, PostgresStorageConfig, build_app,
    config::{AdminUserConfig, BackendClientConfig, PackageSpec},
};
use serde_json::{Value, json};
use sqlx_core::{query::query, row::Row};
use std::sync::Arc;
use testcontainers::{ImageExt, runners::AsyncRunner};
use testcontainers_modules::postgres::Postgres;
#[test]
fn storage_provider_admin_lifecycle() {
    if std::env::var("OCTOFHIR_PROVIDER_CHILD").as_deref() != Ok("1") {
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "storage_provider_admin_lifecycle", "--nocapture"])
            .env("OCTOFHIR_PROVIDER_CHILD", "1")
            .env("OCTOFHIR_CONFIG_KEY", "2b".repeat(32))
            .status()
            .unwrap();
        assert!(status.success());
        return;
    }
    std::thread::Builder::new()
        .name("storage-providers".into())
        .stack_size(8 * 1024 * 1024)
        .spawn(|| {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(Box::pin(scenario(false)))
        })
        .unwrap()
        .join()
        .unwrap();
}
#[test]
fn stored_credentials_require_encryption_key() {
    if std::env::var("OCTOFHIR_PROVIDER_NO_KEY").as_deref() != Ok("1") {
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "stored_credentials_require_encryption_key",
                "--nocapture",
            ])
            .env("OCTOFHIR_PROVIDER_NO_KEY", "1")
            .env_remove("OCTOFHIR_CONFIG_KEY")
            .status()
            .unwrap();
        assert!(status.success());
        return;
    }
    std::thread::Builder::new()
        .stack_size(8 * 1024 * 1024)
        .spawn(|| {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(Box::pin(scenario(true)))
        })
        .unwrap()
        .join()
        .unwrap();
}
async fn serve(config: &AppConfig) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let mut deployment = config.clone();
    deployment.server.base_url = Some(base.clone());
    deployment.auth.issuer = base.clone();
    let app = build_app(
        &deployment,
        Arc::new(ConfigurationManager::builder().build().await.unwrap()),
    )
    .await
    .unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (base, server)
}
async fn admin(client: &reqwest::Client, base: &str) -> String {
    let value: Value = client
        .post(format!("{base}/auth/token"))
        .form(&[
            ("grant_type", "password"),
            ("client_id", "octofhir-ui"),
            ("username", "admin"),
            ("password", "provider-test-password"),
            ("scope", "openid user/*.cruds system/*.cruds"),
        ])
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    value["access_token"].as_str().unwrap().into()
}
async fn scenario(no_key: bool) {
    let pg = Postgres::default()
        .with_tag("18-alpine")
        .start()
        .await
        .unwrap();
    let database = format!(
        "postgres://postgres:postgres@127.0.0.1:{}/postgres",
        pg.get_host_port_ipv4(5432).await.unwrap()
    );
    let pool = sqlx_postgres::PgPoolOptions::new()
        .max_connections(2)
        .connect(&database)
        .await
        .unwrap();
    let mut config = AppConfig::default();
    config.fhir.version = "R4".into();
    config.storage.postgres = Some(PostgresStorageConfig {
        url: Some(database),
        pool_size: 10,
        ..Default::default()
    });
    config.packages.load = vec![PackageSpec::Table {
        id: Some("hl7.fhir.r4.core".into()),
        version: Some("4.0.1".into()),
        path: None,
        url: Some("https://hl7.org/fhir/R4/hl7.fhir.r4.core.tgz".into()),
    }];
    config.auth.policy.anonymous_access = true;
    config.bootstrap.admin_user = Some(AdminUserConfig {
        username: "admin".into(),
        password: "provider-test-password".into(),
        email: None,
        fhir_user: None,
    });
    config.bootstrap.backend_client = Some(BackendClientConfig {
        client_id: "not-an-administrator".into(),
        client_secret: Some("backend-test-password".into()),
        scopes: vec!["system/*.cruds".into()],
        name: None,
        description: None,
    });
    config.storage_providers = vec![
        serde_json::from_value(json!({"id":"seed","name":"Initial name","region":"us-east-1"}))
            .unwrap(),
    ];
    let (base, server) = serve(&config).await;
    let client = reqwest::Client::new();
    let token = admin(&client, &base).await;
    let root = format!("{base}/admin/StorageProvider");
    if no_key {
        let response=client.post(&root).bearer_auth(&token).json(&json!({"id":"insecure","credentials":{"type":"stored","accessKeyId":"access","secretAccessKey":"must-not-persist"}})).send().await.unwrap();
        assert_eq!(response.status(), 400);
        let count:i64=sqlx_core::query_scalar::query_scalar("SELECT (SELECT count(*) FROM _storage_providers WHERE id='insecure')+(SELECT count(*) FROM _storage_provider_history WHERE provider_id='insecure')").fetch_one(&pool).await.unwrap();
        assert_eq!(
            count, 0,
            "Missing encryption must not persist plaintext or history"
        );
        server.abort();
        pool.close().await;
        return;
    }

    assert_eq!(client.get(&root).send().await.unwrap().status(), 401);
    let backend: Value = client
        .post(format!("{base}/auth/token"))
        .basic_auth("not-an-administrator", Some("backend-test-password"))
        .form(&[
            ("grant_type", "client_credentials"),
            ("scope", "system/*.cruds"),
        ])
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        client
            .get(&root)
            .bearer_auth(backend["access_token"].as_str().unwrap())
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    let response = client
        .get(format!("{root}/seed"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(response.headers()["etag"], "W/\"1\"");
    let mut seed: Value = response.json().await.unwrap();
    seed["name"] = json!("Database wins");
    seed["region"] = json!("auto");
    seed["endpoint"] = json!("https://account.r2.cloudflarestorage.com");
    let response = client
        .put(format!("{root}/seed"))
        .bearer_auth(&token)
        .header("if-match", "W/\"1\"")
        .json(&seed)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200, "{}", response.text().await.unwrap());
    assert_eq!(
        client
            .put(format!("{root}/seed"))
            .bearer_auth(&token)
            .header("if-match", "W/\"1\"")
            .json(&seed)
            .send()
            .await
            .unwrap()
            .status(),
        409
    );
    assert_eq!(
        client
            .put(format!("{root}/seed"))
            .bearer_auth(&token)
            .json(&seed)
            .send()
            .await
            .unwrap()
            .status(),
        400
    );
    let secret = json!({"id":"encrypted","name":"Private connection","region":"eu-west-1","addressingStyle":"virtual_hosted","credentials":{"type":"stored","accessKeyId":"fixture-access-marker","secretAccessKey":"fixture-secret-marker","sessionToken":"fixture-token-marker"}});
    let response = client
        .post(&root)
        .bearer_auth(&token)
        .json(&secret)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 201);
    let public = response.text().await.unwrap();
    for marker in [
        "fixture-access-marker",
        "fixture-secret-marker",
        "fixture-token-marker",
    ] {
        assert!(!public.contains(marker));
    }
    let mut public: Value = serde_json::from_str(&public).unwrap();
    let row =
        query("SELECT resource,encrypted_credentials FROM _storage_providers WHERE id='encrypted'")
            .fetch_one(&pool)
            .await
            .unwrap();
    let encrypted: Value = row.get("encrypted_credentials");
    assert_eq!(encrypted["encrypted"], true);
    assert!(!encrypted.to_string().contains("fixture-secret-marker"));
    assert!(
        !row.get::<Value, _>("resource")
            .to_string()
            .contains("fixture-secret-marker")
    );
    let sealed: SecretValue = serde_json::from_value(encrypted).unwrap();
    let clear = Secrets::from_env()
        .unwrap()
        .unwrap()
        .decrypt(&sealed)
        .await
        .unwrap();
    let material: Value = serde_json::from_str(&clear).unwrap();
    assert_eq!(material["secretAccessKey"], "fixture-secret-marker");
    public["name"] = json!("Renamed without replacing keys");
    let response = client
        .put(format!("{root}/encrypted"))
        .bearer_auth(&token)
        .header("if-match", "W/\"1\"")
        .json(&public)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    public = response.json().await.unwrap();
    public["credentials"] = json!({"type":"stored","accessKeyId":"rotated-access-marker","secretAccessKey":"rotated-secret-marker"});
    let response = client
        .put(format!("{root}/encrypted"))
        .bearer_auth(&token)
        .header("if-match", "W/\"2\"")
        .json(&public)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    public = response.json().await.unwrap();
    let sealed: Value = sqlx_core::query_scalar::query_scalar(
        "SELECT encrypted_credentials FROM _storage_providers WHERE id='encrypted'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let sealed: SecretValue = serde_json::from_value(sealed).unwrap();
    let clear = Secrets::from_env()
        .unwrap()
        .unwrap()
        .decrypt(&sealed)
        .await
        .unwrap();
    let material: Value = serde_json::from_str(&clear).unwrap();
    assert_eq!(material["secretAccessKey"], "rotated-secret-marker");
    assert!(material["sessionToken"].is_null());
    let history = client
        .get(format!("{root}/encrypted/_history"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(history.status(), 200);
    let history = history.text().await.unwrap();
    for marker in [
        "fixture-access-marker",
        "fixture-secret-marker",
        "rotated-secret-marker",
        "ciphertext",
    ] {
        assert!(!history.contains(marker));
    }
    // Disabling changes only future operations; the resource and audit trail remain.
    public["enabled"] = json!(false);
    let response = client
        .put(format!("{root}/encrypted"))
        .bearer_auth(&token)
        .header("if-match", "W/\"3\"")
        .json(&public)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    server.abort();
    let mut stale = config.clone();
    stale.storage_providers[0].endpoint = Some("http://127.0.0.1:1".into());
    stale.storage_providers[0].allow_http = true;
    stale.validate().unwrap();
    let (base, server) = serve(&stale).await;
    let token = admin(&client, &base).await;
    let root = format!("{base}/admin/StorageProvider");
    let seed: Value = client
        .get(format!("{root}/seed"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(seed["name"], "Database wins");
    assert_eq!(seed["meta"]["versionId"], "2");
    assert_eq!(
        client
            .delete(format!("{root}/encrypted"))
            .bearer_auth(&token)
            .header("if-match", "W/\"3\"")
            .send()
            .await
            .unwrap()
            .status(),
        409
    );
    assert_eq!(
        client
            .delete(format!("{root}/encrypted"))
            .bearer_auth(&token)
            .header("if-match", "W/\"4\"")
            .send()
            .await
            .unwrap()
            .status(),
        204
    );
    let count: i64 = sqlx_core::query_scalar::query_scalar(
        "SELECT count(*) FROM _storage_provider_history WHERE provider_id='encrypted'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count, 5);
    // Recreating an alias never reuses a prior version/ETag.
    let response = client
        .post(&root)
        .bearer_auth(&token)
        .json(&json!({"id":"encrypted"}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 201);
    assert_eq!(response.headers()["etag"], "W/\"6\"");
    assert_eq!(
        client
            .delete(format!("{root}/seed"))
            .bearer_auth(&token)
            .header("if-match", "W/\"2\"")
            .send()
            .await
            .unwrap()
            .status(),
        204
    );
    server.abort();
    let (base, server) = serve(&config).await;
    let token = admin(&client, &base).await;
    assert_eq!(
        client
            .get(format!("{base}/admin/StorageProvider/seed"))
            .bearer_auth(&token)
            .send()
            .await
            .unwrap()
            .status(),
        404,
        "An API deletion must survive its stale TOML seed"
    );
    if let Ok(path) = std::env::var("OCTOFHIR_PROVIDER_UI_READY") {
        std::fs::write(path, json!({"base":base,"token":token}).to_string()).unwrap();
        tokio::time::sleep(std::time::Duration::from_secs(120)).await;
    }
    server.abort();
    pool.close().await;
}
