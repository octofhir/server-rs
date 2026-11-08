//! Real ru.core R4B package installation from a mounted-style directory and
//! validation of profile slicing and package-local terminology. Requires Docker.

use std::sync::Arc;

use octofhir_config::ConfigurationManager;
use octofhir_server::{AppConfig, PostgresStorageConfig, build_app};
use serde_json::{Value, json};
use testcontainers::{ImageExt, runners::AsyncRunner};
use testcontainers_modules::postgres::Postgres;

const PACKAGE: &[u8] = include_bytes!("fixtures/ru-core/package.tgz");
const PATIENT_PROFILE: &str = "https://fhir.ru/ig/core/StructureDefinition/core-patient";
const VALUES: &str = "https://fhir.ru/ig/core/ValueSet/core-vs-nsi-sources-of-payment";
const SYSTEM: &str = "https://fhir.ru/ig/core/CodeSystem/core-cs-nsi-sources-of-payment";

#[test]
fn local_ru_core_installs_and_enforces_profile_and_terminology() {
    std::thread::Builder::new()
        .name("ru-core-validation".into())
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

fn errors(outcome: &Value) -> bool {
    outcome["issue"].as_array().is_some_and(|issues| {
        issues
            .iter()
            .any(|issue| issue["severity"] == "error" || issue["severity"] == "fatal")
    })
}

async fn validate(client: &reqwest::Client, base: &str, resource: Value) -> Value {
    let response = client
        .post(format!("{base}/Patient/$validate"))
        .json(&json!({"resourceType":"Parameters","parameter":[
            {"name":"resource","resource":resource},{"name":"profile","valueUri":PATIENT_PROFILE}
        ]}))
        .send()
        .await
        .unwrap();
    let status = response.status();
    let outcome: Value = response.json().await.unwrap();
    assert_eq!(status, 200, "{outcome}");
    assert_eq!(outcome["resourceType"], "OperationOutcome");
    outcome
}

async fn validate_payment_codes(client: &reqwest::Client, base: &str) {
    for (code, expected) in [("1", true), ("not-a-payment-code", false)] {
        let response=client.post(format!("{base}/ValueSet/$validate-code"))
            .json(&json!({"resourceType":"Parameters","parameter":[
                {"name":"url","valueUri":VALUES},{"name":"system","valueUri":SYSTEM},{"name":"code","valueCode":code}
            ]})).send().await.unwrap();
        let status = response.status();
        let body: Value = response.json().await.unwrap();
        assert_eq!(status, 200, "{body}");
        let result = body["parameter"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == "result")
            .unwrap();
        assert_eq!(result["valueBoolean"], expected, "{body}");
    }
}

async fn scenario() {
    let mount = tempfile::tempdir().unwrap();
    tar::Archive::new(flate2::read::GzDecoder::new(PACKAGE))
        .unpack(mount.path())
        .unwrap();
    let package_dir = mount.path().join("package");
    let manifest_before = std::fs::read(package_dir.join("package.json")).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&package_dir, std::fs::Permissions::from_mode(0o555)).unwrap();
    }
    let postgres = Postgres::default()
        .with_tag("18-alpine")
        .start()
        .await
        .unwrap();
    let port = postgres.get_host_port_ipv4(5432).await.unwrap();
    let terminology = wiremock::MockServer::start().await;
    let mut config = AppConfig::default();
    config.terminology.server_url = terminology.uri();
    config.fhir.version = "R4B".into();
    config.packages.path = Some(mount.path().join("manager").display().to_string());
    config.storage.postgres = Some(PostgresStorageConfig {
        url: Some(format!(
            "postgres://postgres:postgres@127.0.0.1:{port}/postgres"
        )),
        pool_size: 10,
        ..Default::default()
    });
    config.auth.policy.anonymous_access = true;
    config.packages.load = vec![
        octofhir_server::config::PackageSpec::Table {
            id: Some("hl7.fhir.r4b.core".into()),
            version: Some("4.3.0".into()),
            path: None,
            url: Some("https://hl7.org/fhir/R4B/hl7.fhir.r4b.core.tgz".into()),
        },
        octofhir_server::config::PackageSpec::Table {
            id: None,
            version: None,
            path: Some(mount.path().display().to_string()),
            url: None,
        },
    ];
    let manager = Arc::new(ConfigurationManager::builder().build().await.unwrap());
    let app = build_app(&config, manager).await.unwrap();
    assert_eq!(
        std::fs::read(package_dir.join("package.json")).unwrap(),
        manifest_before
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/fhir", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = reqwest::Client::new();
    let valid = json!({"resourceType":"Patient","identifier":[{
        "system":"http://fhir.ru/core/systems/snils","value":"123-456-789 00","type":{"coding":[{"code":"SB"}]}
    }]});
    let outcome = validate(&client, &base, valid.clone()).await;
    assert!(!errors(&outcome), "valid RuCore Patient: {outcome}");
    // This is valid base FHIR, but violates the RuCore SNILS slice's mandatory type.
    let mut missing_type = valid.clone();
    missing_type["identifier"][0]
        .as_object_mut()
        .unwrap()
        .remove("type");
    let outcome = validate(&client, &base, missing_type.clone()).await;
    assert!(
        errors(&outcome),
        "RuCore mandatory slice type was ignored: {outcome}"
    );
    // Normal writes must enforce the declared loaded profile too.
    let mut declared = valid.clone();
    declared["meta"] = json!({"profile":[PATIENT_PROFILE]});
    let response = client
        .post(format!("{base}/Patient"))
        .json(&declared)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 201, "{}", response.text().await.unwrap());
    missing_type["meta"] = json!({"profile":[PATIENT_PROFILE]});
    let response = client
        .post(format!("{base}/Patient"))
        .json(&missing_type)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 422, "{}", response.text().await.unwrap());
    let mut wrong_type = valid.clone();
    wrong_type["identifier"][0]["type"]["coding"][0]["code"] = json!("TAX");
    let outcome = validate(&client, &base, wrong_type).await;
    assert!(
        errors(&outcome),
        "RuCore slice pattern was ignored: {outcome}"
    );
    // Package-local terminology must work without a national registry-specific adapter.
    let remote_before = terminology.received_requests().await.unwrap().len();
    validate_payment_codes(&client, &base).await;
    assert_eq!(
        terminology.received_requests().await.unwrap().len(),
        remote_before,
        "package-local terminology must not need a remote terminology server"
    );
    server.abort();
    let _ = server.await;
    // The R4 extensions manifest also declares an R4 core dependency. The online
    // registry metadata resolver may omit this cross-release core, but a portable
    // offline set must include the dependencies from the published manifests.
    octofhir_server::canonical::download_registry_package(&config, "hl7.fhir.r4.core", "4.0.1")
        .await
        .unwrap();
    // Cold bootstrap into a new database using the actual ru.core archive and
    // the exact dependency archives obtained above. Registry access is disabled.
    let pool = sqlx_postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(&config.storage.postgres.as_ref().unwrap().connection_url())
        .await
        .unwrap();
    sqlx_core::query::query("CREATE DATABASE offline_packages")
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    let trap = wiremock::MockServer::start().await;
    let archive_path = mount.path().join("ru-core.tgz");
    std::fs::write(&archive_path, PACKAGE).unwrap();
    config.storage.postgres.as_mut().unwrap().url = Some(format!(
        "postgres://postgres:postgres@127.0.0.1:{port}/offline_packages"
    ));
    config.packages.offline = true;
    config.packages.load = vec![
        octofhir_server::config::PackageSpec::Table {
            id: Some("hl7.fhir.r4b.core".into()),
            version: Some("4.3.0".into()),
            path: None,
            url: Some(format!("{}/must-not-be-requested", trap.uri())),
        },
        octofhir_server::config::PackageSpec::Table {
            id: None,
            version: None,
            path: Some(archive_path.display().to_string()),
            url: None,
        },
    ];
    octofhir_server::canonical::rebuild_from_config_async(&config)
        .await
        .unwrap();
    let manager = Arc::new(ConfigurationManager::builder().build().await.unwrap());
    let app = build_app(&config, manager).await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/fhir", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let outcome = validate(&client, &base, valid.clone()).await;
    assert!(!errors(&outcome), "valid offline RuCore Patient: {outcome}");
    let mut invalid = valid;
    invalid["identifier"][0]
        .as_object_mut()
        .unwrap()
        .remove("type");
    let outcome = validate(&client, &base, invalid).await;
    assert!(
        errors(&outcome),
        "offline archive must retain profile enforcement: {outcome}"
    );
    let remote_before = terminology.received_requests().await.unwrap().len();
    validate_payment_codes(&client, &base).await;
    assert_eq!(
        terminology.received_requests().await.unwrap().len(),
        remote_before,
        "offline package-local terminology must not call the remote server"
    );
    assert!(
        trap.received_requests().await.unwrap().is_empty(),
        "offline startup contacted a direct package URL"
    );
    // A warm restart uses installed packages even after the download cache is removed.
    std::fs::remove_dir_all(mount.path().join("manager/cache")).unwrap();
    octofhir_server::canonical::rebuild_from_config_async(&config)
        .await
        .unwrap();
    server.abort();
    let _ = server.await;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&package_dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}
