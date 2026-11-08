//! Real private S3 package loading against pinned RustFS and PostgreSQL containers.
//! A child process receives fixture credentials through its environment, so tests
//! never mutate process-wide environment variables while Rust threads are running.

use aws_credential_types::Credentials;
use aws_sigv4::{
    http_request::{
        PayloadChecksumKind, PercentEncodingMode, SignableBody, SignableRequest, SigningSettings,
        UriPathNormalizationMode, sign,
    },
    sign::v4,
};
use octofhir_config::ConfigurationManager;
use octofhir_server::{
    AppConfig, PostgresStorageConfig, build_app, canonical,
    config::{PackageSpec, S3PackageSource},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{sync::Arc, time::Duration};
use testcontainers::{GenericImage, ImageExt, core::IntoContainerPort, runners::AsyncRunner};
use testcontainers_modules::postgres::Postgres;

const RUSTFS_IMAGE: &str =
    "latest@sha256:ed9210d6411d77ffceb70d94b964a3cb7d225983368bc25bebe01e995293dcc2";
const PACKAGE: &[u8] = include_bytes!("fixtures/ru-core/package.tgz");
const PROFILE: &str = "https://fhir.ru/ig/core/StructureDefinition/core-patient";

#[test]
fn rustfs_private_ig_boots_without_registry_and_enforces_ru_core() {
    std::thread::Builder::new()
        .name("rustfs-s3-packages".into())
        .stack_size(8 * 1024 * 1024)
        .spawn(|| {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(Box::pin(run()))
        })
        .unwrap()
        .join()
        .unwrap();
}

async fn run() {
    if std::env::var("OCTOFHIR_S3_CHILD").as_deref() == Ok("1") {
        child().await;
        return;
    }
    let key = format!(
        "igtest-{}",
        &uuid::Uuid::new_v4().simple().to_string()[..12]
    );
    let secret = uuid::Uuid::new_v4().simple().to_string();
    let rustfs = GenericImage::new("rustfs/rustfs", RUSTFS_IMAGE)
        .with_exposed_port(9000.tcp())
        .with_env_var("RUSTFS_ACCESS_KEY", &key)
        .with_env_var("RUSTFS_SECRET_KEY", &secret)
        .with_env_var("RUSTFS_CONSOLE_ENABLE", "false")
        .with_env_var("RUSTFS_ADDRESS", ":9000")
        .with_env_var("RUSTFS_OBS_LOGGER_LEVEL", "error")
        .with_cmd(["/data"])
        .with_startup_timeout(Duration::from_secs(90))
        .start()
        .await
        .unwrap();
    let endpoint = format!(
        "http://127.0.0.1:{}",
        rustfs.get_host_port_ipv4(9000).await.unwrap()
    );
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(1))
        .build()
        .unwrap();
    let mut healthy = false;
    for _ in 0..180 {
        if client
            .get(format!("{endpoint}/health"))
            .send()
            .await
            .is_ok_and(|r| r.status().is_success())
        {
            healthy = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    assert!(healthy, "RustFS did not become healthy");
    let postgres = Postgres::default()
        .with_tag("18-alpine")
        .start()
        .await
        .unwrap();
    let database = format!(
        "postgres://postgres:postgres@127.0.0.1:{}/postgres",
        postgres.get_host_port_ipv4(5432).await.unwrap()
    );
    let status = tokio::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "rustfs_private_ig_boots_without_registry_and_enforces_ru_core",
            "--nocapture",
        ])
        .env("OCTOFHIR_S3_CHILD", "1")
        .env("OCTOFHIR_S3_TEST_ENDPOINT", &endpoint)
        .env("OCTOFHIR_S3_TEST_DATABASE", database)
        .env("IG_TEST_ACCESS_KEY", &key)
        .env("IG_TEST_SECRET_KEY", &secret)
        .env("IG_TEST_BAD_SECRET", "deliberately-wrong-fixture-secret")
        .env("OCTOFHIR_CONFIG_KEY", "1a".repeat(32))
        .status()
        .await
        .unwrap();
    assert!(
        status.success(),
        "S3 package child scenario failed: {status}"
    );
}

async fn s3_request(method: reqwest::Method, url: &str, body: &[u8]) -> reqwest::Response {
    let credentials = Credentials::new(
        std::env::var("IG_TEST_ACCESS_KEY").unwrap(),
        std::env::var("IG_TEST_SECRET_KEY").unwrap(),
        None,
        None,
        "rustfs-fixture",
    );
    let identity = credentials.into();
    let mut settings = SigningSettings::default();
    settings.percent_encoding_mode = PercentEncodingMode::Single;
    settings.uri_path_normalization_mode = UriPathNormalizationMode::Disabled;
    settings.payload_checksum_kind = PayloadChecksumKind::XAmzSha256;
    let params = v4::SigningParams::builder()
        .identity(&identity)
        .region("us-east-1")
        .name("s3")
        .time(std::time::SystemTime::now())
        .settings(settings)
        .build()
        .unwrap()
        .into();
    let signable = SignableRequest::new(
        method.as_str(),
        url,
        std::iter::empty(),
        SignableBody::Bytes(body),
    )
    .unwrap();
    let (instructions, _) = sign(signable, &params).unwrap().into_parts();
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(30))
        .build()
        .unwrap();
    let mut request = client.request(method, url).body(body.to_vec());
    for (name, value) in instructions.headers() {
        request = request.header(name, value);
    }
    let response = request.send().await.unwrap();
    assert!(
        response.status().is_success(),
        "RustFS fixture request: {}",
        response.status()
    );
    response
}

async fn child() {
    let endpoint = std::env::var("OCTOFHIR_S3_TEST_ENDPOINT").unwrap();
    let database = std::env::var("OCTOFHIR_S3_TEST_DATABASE").unwrap();
    s3_request(
        reqwest::Method::PUT,
        &format!("{endpoint}/private-igs"),
        &[],
    )
    .await;
    s3_request(reqwest::Method::PUT,&format!("{endpoint}/private-igs?versioning"),br#"<VersioningConfiguration xmlns="http://s3.amazonaws.com/doc/2006-03-01/"><Status>Enabled</Status></VersioningConfiguration>"#).await;
    let object = format!("{endpoint}/private-igs/private/ru%20core+%25.tgz");
    let response = s3_request(reqwest::Method::PUT, &object, PACKAGE).await;
    let version = response
        .headers()
        .get("x-amz-version-id")
        .expect("RustFS must return an object version")
        .to_str()
        .unwrap()
        .to_owned();
    // Retrieving the configured version must work even if the current object changes.
    s3_request(
        reqwest::Method::PUT,
        &object,
        b"new object version; not the pinned IG",
    )
    .await;
    let unsigned = reqwest::Client::new().get(&object).send().await.unwrap();
    assert!(
        matches!(unsigned.status().as_u16(), 401 | 403),
        "test bucket must be private"
    );
    let source = S3PackageSource {
        bucket: "private-igs".into(),
        key: "private/ru core+%.tgz".into(),
        provider: "test-rustfs".into(),
        sha256: hex::encode(Sha256::digest(PACKAGE)),
        version_id: Some(version),
        ..Default::default()
    };
    let directory = tempfile::tempdir().unwrap();
    let fixture = directory.path().join("ru-core.tgz");
    std::fs::write(&fixture, PACKAGE).unwrap();
    let terminology = wiremock::MockServer::start().await;
    let mut config = AppConfig::default();
    config.fhir.version = "R4B".into();
    config.storage_providers=vec![serde_json::from_value(json!({"id":"test-rustfs","endpoint":endpoint,"allowHttp":true,"credentials":{"type":"stored","accessKeyId":std::env::var("IG_TEST_ACCESS_KEY").unwrap(),"secretAccessKey":std::env::var("IG_TEST_SECRET_KEY").unwrap()}})).unwrap()];
    config.server.outbound_allowlist.push("127.0.0.1".into());
    config.storage.postgres = Some(PostgresStorageConfig {
        url: Some(database),
        pool_size: 10,
        ..Default::default()
    });
    config.packages.path = Some(directory.path().join("manager").display().to_string());
    config.terminology.server_url = terminology.uri();
    config.auth.policy.anonymous_access = true;
    // Warm a portable dependency archive set using the same published local IG.
    config.packages.load = vec![
        PackageSpec::Table {
            id: Some("hl7.fhir.r4b.core".into()),
            version: Some("4.3.0".into()),
            path: None,
            url: Some("https://hl7.org/fhir/R4B/hl7.fhir.r4b.core.tgz".into()),
        },
        PackageSpec::Table {
            id: None,
            version: None,
            path: Some(fixture.display().to_string()),
            url: None,
        },
    ];
    let _warm = canonical::init_from_config_async(&config).await.unwrap();
    canonical::download_registry_package(&config, "hl7.fhir.r4.core", "4.0.1")
        .await
        .unwrap();
    let pool = sqlx_postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(&config.storage.postgres.as_ref().unwrap().connection_url())
        .await
        .unwrap();
    sqlx_core::query::query("CREATE DATABASE private_s3_packages")
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    let mut cold_database =
        url::Url::parse(&config.storage.postgres.as_ref().unwrap().connection_url()).unwrap();
    cold_database.set_path("/private_s3_packages");
    config.storage.postgres.as_mut().unwrap().url = Some(cold_database.into());
    config.packages.offline = true;
    config.packages.load[1] = PackageSpec::S3 {
        id: None,
        version: None,
        s3: source.clone(),
    };
    config.validate().unwrap();
    let manager = Arc::new(ConfigurationManager::builder().build().await.unwrap());
    let app = build_app(&config, manager).await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/fhir", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = reqwest::Client::new();
    let valid = json!({"resourceType":"Patient","meta":{"profile":[PROFILE]},"identifier":[{"system":"http://fhir.ru/core/systems/snils","value":"123-456-789 00","type":{"coding":[{"code":"SB"}]}}]});
    let response = client
        .post(format!("{base}/Patient"))
        .json(&valid)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 201, "{}", response.text().await.unwrap());
    let mut invalid = valid;
    invalid["identifier"][0]
        .as_object_mut()
        .unwrap()
        .remove("type");
    let response = client
        .post(format!("{base}/Patient"))
        .json(&invalid)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 422, "{}", response.text().await.unwrap());
    let before = terminology.received_requests().await.unwrap().len();
    for (code, expected) in [("1", true), ("unknown", false)] {
        let response=client.post(format!("{base}/ValueSet/$validate-code")).json(&json!({"resourceType":"Parameters","parameter":[
            {"name":"url","valueUri":"https://fhir.ru/ig/core/ValueSet/core-vs-nsi-sources-of-payment"},
            {"name":"system","valueUri":"https://fhir.ru/ig/core/CodeSystem/core-cs-nsi-sources-of-payment"},{"name":"code","valueCode":code}
        ]})).send().await.unwrap();
        assert_eq!(response.status(), 200);
        let body: Value = response.json().await.unwrap();
        assert_eq!(
            body["parameter"]
                .as_array()
                .unwrap()
                .iter()
                .find(|p| p["name"] == "result")
                .unwrap()["valueBoolean"],
            expected,
            "{body}"
        );
    }
    assert_eq!(terminology.received_requests().await.unwrap().len(), before);
    // Real auth and digest failures must stop acquisition before installation.
    for bad_secret in [true, false] {
        let mut bad = config.clone();
        bad.packages.path = Some(
            directory
                .path()
                .join(if bad_secret { "bad-auth" } else { "bad-hash" })
                .display()
                .to_string(),
        );
        let mut source = source.clone();
        if bad_secret {
            source.provider = "bad-auth".into();
            let mut provider = bad.storage_providers[0].clone();
            provider.id = source.provider.clone();
            provider.credentials = serde_json::from_value(json!({"type":"environment","accessKeyIdEnv":"IG_TEST_ACCESS_KEY","secretAccessKeyEnv":"IG_TEST_BAD_SECRET"})).unwrap();
            bad.storage_providers.push(provider);
        } else {
            source.sha256 = "0".repeat(64);
        }
        bad.packages.load[1] = PackageSpec::S3 {
            id: None,
            version: None,
            s3: source,
        };
        let error = canonical::init_from_config_async(&bad).await.unwrap_err();
        assert!(
            error.contains(if bad_secret {
                "HTTP 403"
            } else {
                "SHA-256 mismatch"
            }),
            "{error}"
        );
        assert!(!error.contains(&std::env::var("IG_TEST_BAD_SECRET").unwrap()));
    }
    // Reboot with a cached pinned source without consulting credential providers.
    let mut cached = source;
    cached.provider = "cache-only".into();
    let mut provider = config.storage_providers[0].clone();
    provider.id = cached.provider.clone();
    provider.credentials = serde_json::from_value(json!({"type":"environment","accessKeyIdEnv":"IG_TEST_CREDENTIAL_MUST_NOT_BE_READ","secretAccessKeyEnv":"IG_TEST_CREDENTIAL_MUST_NOT_BE_READ"})).unwrap();
    config.storage_providers.push(provider);
    config.packages.load[1] = PackageSpec::S3 {
        id: None,
        version: None,
        s3: cached,
    };
    canonical::rebuild_from_config_async(&config).await.unwrap();
    server.abort();
    let _ = server.await;
}
