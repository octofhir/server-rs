//! End-to-end SMART grant enforcement, including Bundle entries and revocation.
//! Requires Docker; all resources and credentials belong to a disposable DB.

use std::sync::Arc;

use octofhir_auth::token::{AccessTokenClaims, JwtService, SigningAlgorithm, SigningKeyPair};
use octofhir_config::ConfigurationManager;
use octofhir_server::config::BackendClientConfig;
use octofhir_server::{AppConfig, PostgresStorageConfig, build_app};
use reqwest::{Client, StatusCode};
use rsa::pkcs8::{EncodePrivateKey, EncodePublicKey, LineEnding};
use serde_json::{Value, json};
use testcontainers::{ImageExt, runners::AsyncRunner};
use testcontainers_modules::postgres::Postgres;

async fn expand_test_valueset(
    axum::extract::Query(query): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> (axum::http::StatusCode, axum::Json<Value>) {
    let codes: Vec<String> = match query.get("url").map(String::as_str) {
        Some("urn:test:lab") => vec!["lab".into()],
        Some("urn:test:vital") => vec!["vital".into()],
        Some("urn:test:comma") => vec!["lab,vital".into()],
        Some("urn:test:empty") => Vec::new(),
        Some("urn:test:large-a") => (0..6).map(|i| format!("a{i}")).collect(),
        Some("urn:test:large-b") => (0..6).map(|i| format!("b{i}")).collect(),
        _ => {
            return (
                axum::http::StatusCode::NOT_FOUND,
                axum::Json(
                    json!({"resourceType":"OperationOutcome", "issue":[{"severity":"error","code":"not-found"}]}),
                ),
            );
        }
    };
    let contains: Vec<_> = codes.iter().map(|code| json!({"code":code})).collect();
    (
        axum::http::StatusCode::OK,
        axum::Json(
            json!({"resourceType":"ValueSet", "status":"active", "expansion":{"timestamp":"2026-10-01T00:00:00Z", "total":contains.len(), "contains":contains}}),
        ),
    )
}

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

#[test]
fn smart_grants_bound_policies_and_bundle_entries_and_revoked_tokens() {
    // This fixture initializes two complete servers. Reserve enough stack for
    // construction and panic cleanup of their large setup futures, while the
    // scenario itself remains on the heap.
    std::thread::Builder::new()
        .name("smart-grant-scenario".into())
        .stack_size(8 * 1024 * 1024)
        .spawn(|| {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(Box::pin(smart_grant_scenario()));
        })
        .unwrap()
        .join()
        .unwrap();
}

async fn smart_grant_scenario() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_test_writer()
        .try_init();
    let postgres = Postgres::default()
        .with_tag("17-alpine")
        .start()
        .await
        .unwrap();
    let port = postgres.get_host_port_ipv4(5432).await.unwrap();
    let mut config = AppConfig::default();
    let export_dir = tempfile::Builder::new()
        .prefix("octofhir-bulk-http-")
        .tempdir_in("/private/tmp")
        .unwrap();
    config.bulk_export.export_path = export_dir.path().to_str().unwrap().to_owned();
    config.bulk_export.max_resources_per_file = 1;
    config.bulk_export.batch_size = 1;

    // Pin the published archive so each fresh fixture DB can reuse the
    // integrity-checked tarball cache without registry metadata requests.
    config.packages.load = vec![octofhir_server::config::PackageSpec::Table {
        id: Some("hl7.fhir.r4.core".into()),
        version: Some("4.0.1".into()),
        path: None,
        url: Some("https://hl7.org/fhir/R4/hl7.fhir.r4.core.tgz".into()),
    }];
    let terminology_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    config.terminology.server_url =
        format!("http://{}", terminology_listener.local_addr().unwrap());
    config.search.max_valueset_expansion = 10;
    config.validation.check_target_profile = true;
    config.auth.policy.quickjs_enabled = true;
    config.validation.allow_skip_validation = true;
    let terminology_server = tokio::spawn(async move {
        let app = axum::Router::new().route(
            "/ValueSet/$expand",
            axum::routing::get(expand_test_valueset),
        );
        axum::serve(terminology_listener, app).await.unwrap();
    });
    // Disposable signing material exercises real JWT launch context without
    // adding a test-only bypass to server authentication.
    let private_key = rsa::RsaPrivateKey::new(&mut rand::rngs::OsRng, 2048).unwrap();
    let private_pem = private_key
        .to_pkcs8_pem(LineEnding::LF)
        .unwrap()
        .to_string();
    let public_pem = private_key
        .to_public_key()
        .to_public_key_pem(LineEnding::LF)
        .unwrap();
    config.auth.signing.algorithm = "RS256".into();
    config.auth.signing.kid = Some("isolated-test-key".into());
    config.auth.signing.private_key_pem = Some(private_pem.clone());
    config.auth.signing.public_key_pem = Some(public_pem.clone());
    let signing = JwtService::new(
        SigningKeyPair::from_pem(
            "isolated-test-key",
            SigningAlgorithm::RS256,
            &private_pem,
            &public_pem,
        )
        .unwrap(),
        config.auth.issuer.clone(),
    );
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
        scopes: vec!["system/*.cruds".into(), "patient/*.cruds".into()],
        name: None,
        description: None,
    });
    config.search.allow_debug_search_plan = true;
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
    Box::pin(bulk_export_membership_regression(
        &client, &base, &fhir, &full,
    ))
    .await;

    async_bundle_ownership_regression(&client, &fhir, &full, &create, &read, false).await;
    let async_update = token(&base, "system/Patient.u").await;
    async_bundle_cancel_regression(
        &client,
        &fhir,
        &full,
        &async_update,
        config
            .storage
            .postgres
            .as_ref()
            .unwrap()
            .url
            .as_deref()
            .unwrap(),
    )
    .await;

    // Administrative resource searches retain their root service base.
    let response = client
        .get(format!("{base}/AccessPolicy?_count=1&_total=accurate"))
        .bearer_auth(&full)
        .send()
        .await
        .unwrap();
    let status = response.status();
    let body: Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        body["entry"][0]["fullUrl"]
            .as_str()
            .unwrap()
            .starts_with(&format!("{}/AccessPolicy/", config.base_url())),
        "{body}"
    );
    assert!(
        body["link"][0]["url"]
            .as_str()
            .unwrap()
            .starts_with(&format!("{}/AccessPolicy?", config.base_url())),
        "{body}"
    );

    // Bootstrap's broad grant still permits development writes.
    let response = client
        .post(format!("{fhir}/Patient"))
        .bearer_auth(&full)
        .json(&json!({"resourceType":"Patient","active":true,
            "name":[{"family":"meta-validation-private-family"}]}))
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

    // SMART filter predicates apply in SQL to both matches and total, even if
    // client handling is lenient or the search uses POST form parameters.
    let filtered = token(&base, &format!("system/Patient.s?_id={id}&active=true")).await;
    for query in [format!("_id={id}"), "active=true".to_owned()] {
        let body: Value = client
            .get(format!("{fhir}/Patient?{query}&_total=accurate"))
            .bearer_auth(&full)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        if body["total"] != 1 {
            let plan: Value = client
                .get(format!("{fhir}/Patient?{query}&_debug=search-plan"))
                .bearer_auth(&full)
                .send()
                .await
                .unwrap()
                .json()
                .await
                .unwrap();
            let pool = sqlx_postgres::PgPool::connect(
                &config.storage.postgres.as_ref().unwrap().connection_url(),
            )
            .await
            .unwrap();
            let rows: Vec<Value> = sqlx_core::query_scalar::query_scalar(
                "SELECT jsonb_build_object('id',id,'status',status,'resource',resource) FROM patient",
            )
            .fetch_all(&pool)
            .await
            .unwrap();
            let definitions: Vec<Value> = sqlx_core::query_scalar::query_scalar(
                "SELECT jsonb_build_object('url',content->'url','code',content->'code','type',content->'type','expression',content->'expression','base',content->'base') FROM fcm.resources WHERE resource_type='SearchParameter' AND content->>'code'='_id'",
            ).fetch_all(&pool).await.unwrap();
            panic!(
                "unrestricted {query}: {body}; plan: {plan}; rows: {rows:?}; definitions: {definitions:?}"
            );
        }
        assert_eq!(body["entry"][0]["resource"]["active"], true);
    }
    for query in ["_count=1&_total=accurate", "_count=0", "_summary=count"] {
        let response = client
            .get(format!("{fhir}/Patient?{query}"))
            .bearer_auth(&filtered)
            .send()
            .await
            .unwrap();
        let status = response.status();
        let body: Value = response.json().await.unwrap();
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["total"], 1, "{body}");
        if query == "_count=1&_total=accurate" {
            assert_eq!(body["entry"].as_array().unwrap().len(), 1, "{body}");
            assert_eq!(body["entry"][0]["resource"]["id"], id);
        } else {
            assert!(body["entry"].as_array().is_none_or(Vec::is_empty), "{body}");
        }
    }
    for (is_post, query) in [
        (false, "_id=outside&_total=accurate"),
        (true, "active=false&_total=accurate"),
    ] {
        let response = if is_post {
            client
                .post(format!("{fhir}/Patient/_search"))
                .bearer_auth(&filtered)
                .header("Content-Type", "application/x-www-form-urlencoded")
                .body(query)
        } else {
            client
                .get(format!("{fhir}/Patient?{query}"))
                .bearer_auth(&filtered)
        }
        .send()
        .await
        .unwrap();
        let status = response.status();
        let body: Value = response.json().await.unwrap();
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["total"], 0, "{body}");
    }
    let unsupported = token(&base, "system/Patient.s?unknown-grant-filter=value").await;
    assert_eq!(
        client
            .get(format!("{fhir}/Patient"))
            .bearer_auth(&unsupported)
            .header("Prefer", "handling=lenient")
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::BAD_REQUEST
    );
    let response = client
        .get(format!("{fhir}/Patient?_include=Patient:organization"))
        .bearer_auth(&filtered)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["entry"].as_array().unwrap().len(), 1, "{body}");

    let patient_scope = |scope: &str| {
        signing
            .encode(
                &AccessTokenClaims::builder(
                    &config.auth.issuer,
                    "development-backend",
                    "development-backend",
                )
                .audience(vec![config.base_url()])
                .scope(scope)
                .patient(id)
                .build(),
            )
            .unwrap()
    };
    let own_patient = patient_scope("patient/Patient.s");
    let response = client
        .get(format!("{fhir}/Patient?_total=accurate"))
        .bearer_auth(&own_patient)
        .send()
        .await
        .unwrap();
    let status = response.status();
    let body: Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["total"], 1, "{body}");
    assert_eq!(body["entry"][0]["resource"]["id"], id);

    let response = client
        .post(format!("{fhir}/Patient"))
        .bearer_auth(&full)
        .json(&json!({"resourceType":"Patient", "active":false}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let other_patient: Value = response.json().await.unwrap();
    let other_id = other_patient["id"].as_str().unwrap();
    for post in [false, true] {
        let response = if post {
            client
                .post(format!("{fhir}/Patient/_search"))
                .bearer_auth(&full)
                .header("Content-Type", "application/x-www-form-urlencoded")
                .body("_count=1&_total=accurate")
        } else {
            client
                .get(format!("{fhir}/Patient?_count=1&_total=accurate"))
                .bearer_auth(&full)
        }
        .send()
        .await
        .unwrap();
        let status = response.status();
        let body: Value = response.json().await.unwrap();
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["total"], 2, "{body}");
        assert!(
            body["entry"][0]["fullUrl"]
                .as_str()
                .unwrap()
                .starts_with(&format!("{}/fhir/Patient/", config.base_url())),
            "{body}"
        );
        let next = body["link"]
            .as_array()
            .unwrap()
            .iter()
            .find(|link| link["relation"] == "next")
            .unwrap();
        let next = reqwest::Url::parse(next["url"].as_str().unwrap()).unwrap();
        assert_eq!(next.path(), "/fhir/Patient");
        let response = client
            .get(format!("{base}{}?{}", next.path(), next.query().unwrap()))
            .bearer_auth(&full)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let second: Value = response.json().await.unwrap();
        assert_eq!(second["total"], 2, "{second}");
        assert_eq!(second["entry"][0]["resource"]["id"], other_id);
    }
    let mut lab_id = String::new();
    let mut vital_id = String::new();
    for (reference, code, in_scope) in [
        (format!("Patient/{id}"), "lab", true),
        (format!("Patient/{id}"), "vital", true),
        (format!("Patient/{other_id}"), "lab", false),
    ] {
        let response = client.post(format!("{fhir}/Observation")).bearer_auth(&full)
            .json(&json!({"resourceType":"Observation", "status":"final", "code":{"coding":[{"code":code}]}, "subject":{"reference":reference}}))
            .send().await.unwrap();
        let status = response.status();
        let body: Value = response.json().await.unwrap();
        assert_eq!(status, StatusCode::CREATED, "{body}");
        if code == "lab" && in_scope {
            lab_id = body["id"].as_str().unwrap().into();
        }
        if code == "vital" && in_scope {
            vital_id = body["id"].as_str().unwrap().into();
        }
    }
    let labs = patient_scope("patient/Observation.s?code=lab");
    for post in [false, true] {
        let response = if post {
            client
                .post(format!("{fhir}/Observation/_search"))
                .bearer_auth(&labs)
                .header("Content-Type", "application/x-www-form-urlencoded")
                .body("_count=1&_total=accurate")
        } else {
            client
                .get(format!("{fhir}/Observation?_count=1&_total=accurate"))
                .bearer_auth(&labs)
        }
        .send()
        .await
        .unwrap();
        let status = response.status();
        let body: Value = response.json().await.unwrap();
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["total"], 1, "{body}");
        assert_eq!(body["entry"][0]["resource"]["id"], lab_id);
    }

    for (scope, expected_included) in [
        ("patient/Patient.s patient/Observation.s?code=lab", 1),
        ("patient/Patient.s patient/Observation.r", 0),
        ("patient/Patient.s", 0),
    ] {
        let response = client
            .get(format!(
                "{fhir}/Patient?_revinclude=Observation:subject&_total=accurate&_count=1"
            ))
            .bearer_auth(patient_scope(scope))
            .send()
            .await
            .unwrap();
        let status = response.status();
        let body: Value = response.json().await.unwrap();
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["total"], 1, "{body}");
        assert_eq!(
            body["entry"].as_array().unwrap().len(),
            expected_included + 1,
            "{body}"
        );
        if expected_included > 0 {
            assert_eq!(body["entry"][1]["resource"]["id"], lab_id);
            assert_eq!(body["entry"][1]["search"]["mode"], "include");
        }
    }
    let forward = patient_scope("patient/Observation.s?code=lab patient/Patient.s");
    let response = client
        .get(format!(
            "{fhir}/Observation?_include=Observation:subject:Patient&_total=accurate"
        ))
        .bearer_auth(forward)
        .send()
        .await
        .unwrap();
    let status = response.status();
    let body: Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["total"], 1, "{body}");
    assert_eq!(body["entry"].as_array().unwrap().len(), 2, "{body}");
    assert_eq!(body["entry"][1]["resource"]["id"], id);

    // System/compartment searches must authorize each type before one global
    // count and page. POST bodies participate in the same query and policy path.
    let aggregate = patient_scope("patient/Patient.s patient/Observation.s?code=lab");
    for path in [
        "?_type=Patient,Observation&_count=1&_total=accurate".to_string(),
        format!("/Patient/{id}/*?_count=1&_total=accurate"),
    ] {
        let response = client
            .get(format!("{fhir}{path}"))
            .bearer_auth(&aggregate)
            .header("Prefer", "handling=strict")
            .send()
            .await
            .unwrap();
        let status = response.status();
        let body: Value = response.json().await.unwrap();
        assert_eq!(status, StatusCode::OK, "{path}: {body}");
        assert_eq!(body["total"], 2, "{path}: {body}");
        assert_eq!(body["entry"].as_array().unwrap().len(), 1, "{body}");
        assert_eq!(body["entry"][0]["resource"]["id"], lab_id);
        let next = body["link"]
            .as_array()
            .unwrap()
            .iter()
            .find(|link| link["relation"] == "next")
            .unwrap();
        let next = reqwest::Url::parse(next["url"].as_str().unwrap()).unwrap();
        assert!(next.path().starts_with("/fhir"), "{next}");
        if path.starts_with("/Patient") {
            assert!(next.path().contains(&format!("Patient/{id}/*")), "{next}");
        }
        let body: Value = client
            .get(format!("{base}{}?{}", next.path(), next.query().unwrap()))
            .bearer_auth(&aggregate)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(body["total"], 2, "{body}");
        assert_eq!(body["entry"][0]["resource"]["id"], id);
    }
    for path in [
        "?_type=Patient,Observation&_count=0".to_string(),
        "?_summary=count".to_string(),
        format!("/Patient/{id}/*?_summary=count"),
    ] {
        let response = client
            .get(format!("{fhir}{path}"))
            .bearer_auth(&aggregate)
            .send()
            .await
            .unwrap();
        let status = response.status();
        let body: Value = response.json().await.unwrap();
        assert_eq!(status, StatusCode::OK, "{path}: {body}");
        assert_eq!(body["total"], 2, "{path}: {body}");
        assert!(body["entry"].as_array().is_none_or(Vec::is_empty), "{body}");
    }
    for path in [
        "?_type=Patient,Observation&_total=accurate",
        "?_total=accurate",
    ] {
        let response = client
            .get(format!("{fhir}{path}"))
            .bearer_auth(&own_patient)
            .send()
            .await
            .unwrap();
        let status = response.status();
        let body: Value = response.json().await.unwrap();
        assert_eq!(status, StatusCode::OK, "{path}: {body}");
        assert_eq!(body["total"], 1, "{body}");
        assert_eq!(body["entry"][0]["resource"]["id"], id);
    }
    for (path, form, expected) in [
        (
            "/_search".to_string(),
            "_type=Patient,Observation&_count=0",
            2,
        ),
        (
            "/_search?_id=absent".to_string(),
            "_type=Patient,Observation&_total=accurate",
            0,
        ),
        (
            format!("/Patient/{id}/Observation/_search"),
            "code=lab&_count=0",
            1,
        ),
        (
            format!("/Patient/{id}/Observation/_search?code=lab"),
            "code=vital&_count=0",
            0,
        ),
        (format!("/Patient/{id}/*/_search"), "_count=0", 2),
    ] {
        let response = client
            .post(format!("{fhir}{path}"))
            .bearer_auth(&aggregate)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(form)
            .send()
            .await
            .unwrap();
        let status = response.status();
        let body: Value = response.json().await.unwrap();
        assert_eq!(status, StatusCode::OK, "{path}: {body}");
        assert_eq!(body["total"], expected, "{path}: {body}");
    }
    for compartment_id in [other_id, "nonexistent-patient"] {
        let response = client
            .get(format!(
                "{fhir}/Patient/{compartment_id}/Observation?_count=0"
            ))
            .bearer_auth(&labs)
            .send()
            .await
            .unwrap();
        let status = response.status();
        let body: Value = response.json().await.unwrap();
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["total"], 0, "{body}");
    }
    for (path, scope, expected) in [
        ("?_type=Observation", &own_patient, StatusCode::FORBIDDEN),
        ("?_type=Patient,Observation", &read, StatusCode::FORBIDDEN),
        (
            "?_type=Patient,Observation&family=Allowed",
            &aggregate,
            StatusCode::BAD_REQUEST,
        ),
        ("?_type=NotAResource", &aggregate, StatusCode::BAD_REQUEST),
    ] {
        let response = client
            .get(format!("{fhir}{path}"))
            .bearer_auth(scope)
            .send()
            .await
            .unwrap();
        let status = response.status();
        let body: Value = response.json().await.unwrap();
        assert_eq!(status, expected, "{path}: {body}");
    }

    for kind in ["batch", "transaction"] {
        let searches = [
            (
                "Observation?_count=1&_total=accurate&_include=Observation:subject:Patient"
                    .to_string(),
                1,
                2,
            ),
            ("Observation?_count=0".to_string(), 1, 0),
            ("Observation?_summary=count".to_string(), 1, 0),
            ("Observation".to_string(), 1, 1),
            (
                "?_type=Patient,Observation&_count=1&_total=accurate".to_string(),
                2,
                1,
            ),
            (format!("Patient/{id}/*?_total=accurate"), 2, 2),
            (format!("Patient/{other_id}/Observation?_count=0"), 0, 0),
        ];
        let entries: Vec<_> = searches
            .iter()
            .map(|(url, _, _)| json!({"request":{"method":"GET", "url":url}}))
            .collect();
        let response = client
            .post(&fhir)
            .bearer_auth(&aggregate)
            .header("Prefer", "handling=strict")
            .json(&json!({"resourceType":"Bundle", "type":kind, "entry":entries}))
            .send()
            .await
            .unwrap();
        let status = response.status();
        let body: Value = response.json().await.unwrap();
        assert_eq!(status, StatusCode::OK, "{kind}: {body}");
        for (index, (url, expected_total, expected_entries)) in searches.iter().enumerate() {
            let entry = &body["entry"][index];
            assert_eq!(
                entry["response"]["status"], "200 OK",
                "{kind}/{url}: {body}"
            );
            let nested = &entry["resource"];
            assert_eq!(nested["type"], "searchset", "{nested}");
            assert_eq!(nested["total"], *expected_total, "{kind}/{url}: {nested}");
            assert_eq!(
                nested["entry"].as_array().map_or(0, Vec::len),
                *expected_entries,
                "{kind}/{url}: {nested}"
            );
            for resource in nested["entry"].as_array().into_iter().flatten() {
                assert!(
                    resource["resource"]["id"] == id || resource["resource"]["id"] == lab_id,
                    "{resource}"
                );
                assert!(
                    resource["fullUrl"].as_str().unwrap().contains("/fhir/"),
                    "{resource}"
                );
            }
            if index == 0 {
                assert_eq!(nested["entry"][0]["search"]["mode"], "match");
                assert_eq!(nested["entry"][1]["search"]["mode"], "include");
                assert_eq!(nested["entry"][1]["resource"]["id"], id);
            }
        }
    }
    // Strict handling rejects the search before transactional writes, while
    // lenient handling may include an outcome alongside authorized matches.
    for (kind, strict, expected_status) in [
        ("batch", true, StatusCode::OK),
        ("batch", false, StatusCode::OK),
        ("transaction", true, StatusCode::BAD_REQUEST),
    ] {
        let response = client.post(&fhir).bearer_auth(&aggregate)
            .header("Prefer", if strict { "handling=strict" } else { "handling=lenient" })
            .json(&json!({"resourceType":"Bundle", "type":kind, "entry":[
                {"request":{"method":"GET","url":"Observation?unsupported-client-filter=ignored&_total=accurate"}}
            ]})).send().await.unwrap();
        let status = response.status();
        let body: Value = response.json().await.unwrap();
        assert_eq!(status, expected_status, "{kind}/{strict}: {body}");
        if kind == "batch" && strict {
            assert_eq!(
                body["entry"][0]["response"]["status"], "400 Bad Request",
                "{body}"
            );
        } else if !strict {
            let nested = &body["entry"][0]["resource"];
            assert_eq!(nested["total"], 1, "{nested}");
            assert_eq!(nested["entry"][0]["resource"]["id"], lab_id);
            assert_eq!(nested["entry"][1]["search"]["mode"], "outcome");
            let self_link = nested["link"]
                .as_array()
                .unwrap()
                .iter()
                .find(|link| link["relation"] == "self")
                .unwrap();
            let url = reqwest::Url::parse(self_link["url"].as_str().unwrap()).unwrap();
            assert!(
                !url.query_pairs()
                    .any(|(name, _)| name == "unsupported-client-filter"),
                "{url}"
            );
        }
    }

    for post in [false, true] {
        let query = "unsupported-client-filter=ignored&_elements=id&_count=1&_total=accurate";
        let response = if post {
            client
                .post(format!("{fhir}/Observation/_search"))
                .bearer_auth(&aggregate)
                .header("Content-Type", "application/x-www-form-urlencoded")
                .body(query)
                .send()
                .await
                .unwrap()
        } else {
            client
                .get(format!("{fhir}/Observation?{query}"))
                .bearer_auth(&aggregate)
                .send()
                .await
                .unwrap()
        };
        let status = response.status();
        let body: Value = response.json().await.unwrap();
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["total"], 1, "{body}");
        let self_link = body["link"]
            .as_array()
            .unwrap()
            .iter()
            .find(|link| link["relation"] == "self")
            .unwrap();
        let url = reqwest::Url::parse(self_link["url"].as_str().unwrap()).unwrap();
        assert!(
            !url.query_pairs()
                .any(|(name, _)| name == "unsupported-client-filter"),
            "{url}"
        );
        assert!(
            url.query_pairs()
                .any(|(name, value)| name == "_elements" && value == "id"),
            "{url}"
        );
    }

    for code in ["__fhir262_no_match__", "lab,vital"] {
        let response = client.post(format!("{fhir}/Observation")).bearer_auth(&full)
            .json(&json!({"resourceType":"Observation", "status":"final", "code":{"coding":[{"code":code}]}, "subject":{"reference":format!("Patient/{id}")}}))
            .send().await.unwrap();
        let status = response.status();
        let body: Value = response.json().await.unwrap();
        assert_eq!(status, StatusCode::CREATED, "{body}");
    }
    for kind in ["direct", "batch", "transaction"] {
        for (scope, query, expected) in [
            (
                "patient/Observation.s?code:in=urn:test:lab",
                "Observation?_count=0",
                1,
            ),
            (
                "patient/Observation.s?code:in=urn:test:comma",
                "Observation?_count=0",
                1,
            ),
            (
                "patient/Observation.s?code:in=urn:test:empty",
                "Observation?_count=0",
                0,
            ),
            (
                "patient/Observation.s?code:not-in=urn:test:empty",
                "Observation?_count=0",
                4,
            ),
            (
                "patient/Observation.s?code=lab",
                "Observation?code:in=urn:test:empty&_count=0",
                0,
            ),
            (
                "patient/Observation.s?code=lab",
                "Observation?code:not-in=urn:test:lab,urn:test:vital&_count=0",
                1,
            ),
        ] {
            let credential = patient_scope(scope);
            let response = if kind == "direct" {
                client.get(format!("{fhir}/{query}")).bearer_auth(credential)
            } else {
                client.post(&fhir).bearer_auth(credential).json(&json!({"resourceType":"Bundle","type":kind,"entry":[{"request":{"method":"GET","url":query}}]}))
            }.send().await.unwrap();
            let status = response.status();
            let body: Value = response.json().await.unwrap();
            assert_eq!(status, StatusCode::OK, "{kind}/{query}/{scope}: {body}");
            let nested = if kind == "direct" {
                &body
            } else {
                &body["entry"][0]["resource"]
            };
            assert_eq!(nested["total"], expected, "{kind}/{query}/{scope}: {body}");
        }
        let query = "Patient?_revinclude=Observation:subject&_total=accurate";
        let credential =
            patient_scope("patient/Patient.s patient/Observation.s?code:in=urn:test:lab");
        let response = if kind == "direct" {
            client.get(format!("{fhir}/{query}")).bearer_auth(credential)
        } else {
            client.post(&fhir).bearer_auth(credential).json(&json!({"resourceType":"Bundle","type":kind,"entry":[{"request":{"method":"GET","url":query}}]}))
        }.send().await.unwrap();
        let status = response.status();
        let body: Value = response.json().await.unwrap();
        assert_eq!(status, StatusCode::OK, "{kind}: {body}");
        let nested = if kind == "direct" {
            &body
        } else {
            &body["entry"][0]["resource"]
        };
        assert_eq!(nested["total"], 1, "{nested}");
        assert_eq!(nested["entry"].as_array().unwrap().len(), 2, "{nested}");
        assert_eq!(nested["entry"][1]["resource"]["id"], lab_id);
    }
    for query in ["code:in=", "code:in=urn:test:large-a,urn:test:large-b"] {
        let response = client.post(&fhir).bearer_auth(&aggregate)
            .json(&json!({"resourceType":"Bundle","type":"transaction","entry":[{"request":{"method":"GET","url":format!("Observation?{query}")}}]}))
            .send().await.unwrap();
        let status = response.status();
        let body: Value = response.json().await.unwrap();
        assert_eq!(status, StatusCode::BAD_REQUEST, "{query}: {body}");
    }

    // A known unsupported hierarchy modifier must fail even with lenient
    // handling. Empty criteria cannot silently become an unfiltered search.
    for kind in ["direct", "batch", "transaction"] {
        for query in [
            "code:above=",
            "code:below=",
            "code:above=urn:test|lab",
            "code:below=urn:test|lab",
            "code:below=urn:test|lab,",
        ] {
            let path = format!("Observation?{query}");
            let request = if kind == "direct" {
                client.get(format!("{fhir}/{path}"))
            } else {
                client
                    .post(&fhir)
                    .json(&json!({"resourceType":"Bundle","type":kind,"entry":[
                        {"request":{"method":"GET","url":path}}
                    ]}))
            };
            let response = request
                .bearer_auth(&aggregate)
                .header("Prefer", "handling=lenient")
                .send()
                .await
                .unwrap();
            let status = response.status();
            let body: Value = response.json().await.unwrap();
            if kind == "batch" {
                assert_eq!(status, StatusCode::OK, "{query}: {body}");
                assert_eq!(
                    body["entry"][0]["response"]["status"], "400 Bad Request",
                    "{query}: {body}"
                );
            } else {
                assert_eq!(status, StatusCode::BAD_REQUEST, "{kind}/{query}: {body}");
            }
        }
    }

    // Named operations enforce actual resource reads and searches rather than
    // treating the common Operation category as an unrestricted SMART grant.
    for (path, credential, expected) in [
        (format!("Patient/{id}/$meta"), read.clone(), StatusCode::OK),
        (
            format!("Patient/{id}/$meta"),
            search.clone(),
            StatusCode::FORBIDDEN,
        ),
        (
            format!("Patient/{other_id}/$meta"),
            patient_scope("patient/Patient.r"),
            StatusCode::NOT_FOUND,
        ),
        (
            format!("Observation/{lab_id}/$meta"),
            patient_scope("patient/Observation.r?code=lab"),
            StatusCode::OK,
        ),
        (
            format!("Observation/{vital_id}/$meta"),
            patient_scope("patient/Observation.r?code=lab"),
            StatusCode::NOT_FOUND,
        ),
        (
            format!("Patient/{id}/$validate"),
            search.clone(),
            StatusCode::BAD_REQUEST,
        ),
        (
            format!("Patient/{id}/$fhirpath?expression=Patient.active"),
            read.clone(),
            StatusCode::OK,
        ),
    ] {
        let response = client
            .get(format!("{fhir}/{path}"))
            .bearer_auth(credential)
            .send()
            .await
            .unwrap();
        let status = response.status();
        let body: Value = response.json().await.unwrap();
        assert_eq!(status, expected, "{path}: {body}");
    }
    // Validation failures are 200; an unavailable requested check is an HTTP
    // error. Warning-only declared profiles survive the validator adapter.
    let response = client
        .post(format!("{fhir}/Patient/$validate"))
        .bearer_auth(&read)
        .json(&json!({
            "resourceType":"Patient", "meta":{"profile":["urn:test:unknown-declared-profile"]}
        }))
        .send()
        .await
        .unwrap();
    let status = response.status();
    let body: Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        body["issue"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue["severity"] == "warning"
                && issue["diagnostics"]
                    .as_str()
                    .unwrap_or_default()
                    .contains("urn:test:unknown-declared-profile")),
        "{body}"
    );
    let response = client
        .post(format!("{fhir}/Patient/$validate"))
        .bearer_auth(&read)
        .json(&json!({
            "resourceType":"Patient", "birthDate":"not-a-date"
        }))
        .send()
        .await
        .unwrap();
    let status = response.status();
    let body: Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        body["issue"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue["severity"] == "error"),
        "{body}"
    );
    for (path, mode, expected) in [
        (
            "Patient/$validate".to_owned(),
            "strict",
            StatusCode::BAD_REQUEST,
        ),
        (
            "Patient/$validate".to_owned(),
            "update",
            StatusCode::BAD_REQUEST,
        ),
        ("Patient/$validate".to_owned(), "create", StatusCode::OK),
        (format!("Patient/{id}/$validate"), "update", StatusCode::OK),
        (format!("Patient/{id}/$validate"), "delete", StatusCode::OK),
    ] {
        let mut parameters = vec![json!({"name":"mode", "valueCode":mode})];
        if mode != "delete" {
            let mut resource = json!({"resourceType":"Patient", "active":true});
            if mode == "update" {
                resource["id"] = json!(id);
            }
            parameters.push(json!({"name":"resource", "resource":resource}));
        }
        let response = client
            .post(format!("{fhir}/{path}"))
            .bearer_auth(&full)
            .json(&json!({"resourceType":"Parameters", "parameter":parameters}))
            .send()
            .await
            .unwrap();
        let status = response.status();
        let body: Value = response.json().await.unwrap();
        assert_eq!(status, expected, "{path} {mode}: {body}");
        assert_eq!(body["resourceType"], "OperationOutcome", "{body}");
    }

    let response = client.post(format!("{fhir}/Patient/$validate")).bearer_auth(&full).json(&json!({"resourceType":"Parameters", "parameter":[{"name":"mode", "valueCode":"create"}]})).send().await.unwrap();
    let status = response.status();
    let body: Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");

    // Repeated primitive query parameters must reach handler cardinality checks
    // on merged system/type, compartment instance and generic instance routes.
    for (path, resource_type) in [
        ("$validate".to_owned(), "Patient"),
        ("Patient/$validate".to_owned(), "Patient"),
        (format!("Patient/{id}/$validate"), "Patient"),
        (format!("Observation/{lab_id}/$validate"), "Observation"),
    ] {
        let profile = format!("http://hl7.org/fhir/StructureDefinition/{resource_type}");
        for (query, parameter) in [
            ("mode=create&mode=create".to_owned(), "mode"),
            (format!("profile={profile}&%70rofile={profile}"), "profile"),
        ] {
            let response = client
                .get(format!("{fhir}/{path}?{query}"))
                .bearer_auth(&full)
                .send()
                .await
                .unwrap();
            let status = response.status();
            let body: Value = response.json().await.unwrap();
            assert_eq!(status, StatusCode::BAD_REQUEST, "GET {path}: {body}");
            assert!(
                body.to_string()
                    .contains(&format!("{parameter} permits only one value")),
                "GET {path}: {body}"
            );
        }
        let resource = json!({"resourceType":resource_type});
        for (query, content, parameter) in [
            (
                "mode=create&mode=create".to_owned(),
                resource.clone(),
                "mode",
            ),
            (
                format!("profile={profile}&profile={profile}"),
                resource.clone(),
                "profile",
            ),
            (
                format!("profile={profile}"),
                json!({"resourceType":"Parameters", "parameter":[
                    {"name":"profile", "valueUri":profile},
                    {"name":"resource", "resource":resource}
                ]}),
                "profile",
            ),
        ] {
            let response = client
                .post(format!("{fhir}/{path}?{query}"))
                .bearer_auth(&full)
                .json(&content)
                .send()
                .await
                .unwrap();
            let status = response.status();
            let body: Value = response.json().await.unwrap();
            assert_eq!(status, StatusCode::BAD_REQUEST, "POST {path}: {body}");
            assert!(
                body.to_string()
                    .contains(&format!("{parameter} permits only one value")),
                "POST {path}: {body}"
            );
        }
    }

    // Mode results predict the corresponding write, while speculative writes
    // leave the complete resource, history and resource count unchanged.
    let update_only = token(&base, "system/Patient.u").await;
    let delete_only = token(&base, "system/Patient.d").await;
    let own_update = patient_scope("patient/Patient.u");
    let own_delete = patient_scope("patient/Patient.d");
    let write_before: Value = client
        .get(format!("{fhir}/Patient/{id}"))
        .bearer_auth(&full)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let history_before: Value = client
        .get(format!("{fhir}/Patient/{id}/_history"))
        .bearer_auth(&full)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let count_before: Value = client
        .get(format!("{fhir}/Patient?_summary=count"))
        .bearer_auth(&full)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let current_etag = format!(
        "W/\"{}\"",
        write_before["meta"]["versionId"].as_str().unwrap()
    );
    for (path, credential, mode, resource, etag, expected_status, expected_valid) in [
        (
            "Patient/$validate".to_owned(),
            &create,
            "create",
            Some(json!({"resourceType":"Patient", "id":42, "active":true})),
            None,
            StatusCode::OK,
            Some(true),
        ),
        (
            "Patient/$validate".to_owned(),
            &create,
            "create",
            Some(json!({"resourceType":"Patient", "birthDate":"invalid"})),
            None,
            StatusCode::OK,
            Some(false),
        ),
        (
            format!("Patient/{id}/$validate"),
            &update_only,
            "update",
            Some(json!({"resourceType":"Patient", "id":id, "active":false})),
            Some(current_etag.as_str()),
            StatusCode::OK,
            Some(true),
        ),
        (
            format!("Patient/{id}/$validate"),
            &own_update,
            "update",
            Some(json!({"resourceType":"Patient", "id":id, "active":false})),
            None,
            StatusCode::OK,
            Some(true),
        ),
        (
            format!("Patient/{id}/$validate"),
            &update_only,
            "update",
            Some(json!({"resourceType":"Patient", "active":false})),
            None,
            StatusCode::OK,
            Some(false),
        ),
        (
            format!("Patient/{id}/$validate"),
            &update_only,
            "update",
            Some(json!({"resourceType":"Patient", "id":other_id})),
            None,
            StatusCode::OK,
            Some(false),
        ),
        (
            format!("Patient/{id}/$validate"),
            &update_only,
            "update",
            Some(json!({"resourceType":"Patient", "id":id})),
            Some("W/\"999999\""),
            StatusCode::OK,
            Some(false),
        ),
        (
            format!("Patient/{id}/$validate"),
            &update_only,
            "update",
            Some(json!({"resourceType":"Patient", "id":id})),
            Some("42"),
            StatusCode::BAD_REQUEST,
            None,
        ),
        (
            format!("Patient/{id}/$validate"),
            &update_only,
            "update",
            Some(
                json!({"resourceType":"Patient", "id":id, "meta":{"versionId":"stale-body-version"}}),
            ),
            None,
            StatusCode::OK,
            Some(true),
        ),
        (
            "Patient/not-present/$validate".to_owned(),
            &update_only,
            "update",
            Some(json!({"resourceType":"Patient", "id":"not-present"})),
            None,
            StatusCode::OK,
            Some(true),
        ),
        (
            "Patient/not-present/$validate".to_owned(),
            &update_only,
            "update",
            Some(json!({"resourceType":"Patient", "id":"not-present"})),
            Some(current_etag.as_str()),
            StatusCode::OK,
            Some(false),
        ),
        (
            format!("Patient/{id}/$validate"),
            &delete_only,
            "delete",
            Some(
                json!({"resourceType":"Observation", "birthDate":"invalid", "subject":{"reference":"Patient/missing"}}),
            ),
            None,
            StatusCode::OK,
            Some(true),
        ),
        (
            format!("Patient/{id}/$validate"),
            &own_delete,
            "delete",
            None,
            None,
            StatusCode::OK,
            Some(true),
        ),
        (
            "Patient/not-present/$validate".to_owned(),
            &delete_only,
            "delete",
            None,
            None,
            StatusCode::OK,
            Some(true),
        ),
        (
            format!("Patient/{other_id}/$validate"),
            &own_delete,
            "delete",
            None,
            None,
            StatusCode::NOT_FOUND,
            None,
        ),
        (
            "Patient/not-present/$validate".to_owned(),
            &own_delete,
            "delete",
            None,
            None,
            StatusCode::NOT_FOUND,
            None,
        ),
        (
            format!("Patient/{other_id}/$validate"),
            &own_update,
            "update",
            Some(json!({"resourceType":"Patient", "id":other_id})),
            None,
            StatusCode::NOT_FOUND,
            None,
        ),
        (
            format!("Patient/{id}/$validate"),
            &read,
            "update",
            Some(json!({"resourceType":"Patient", "id":id})),
            None,
            StatusCode::FORBIDDEN,
            None,
        ),
        (
            format!("Patient/{id}/$validate"),
            &read,
            "delete",
            None,
            None,
            StatusCode::FORBIDDEN,
            None,
        ),
        (
            "Patient/$validate".to_owned(),
            &read,
            "create",
            Some(json!({"resourceType":"Patient"})),
            None,
            StatusCode::FORBIDDEN,
            None,
        ),
    ] {
        let mut parameters = vec![json!({"name":"mode", "valueCode":mode})];
        if let Some(resource) = resource {
            parameters.push(json!({"name":"resource", "resource":resource}));
        }
        let mut request = client
            .post(format!("{fhir}/{path}"))
            .bearer_auth(credential)
            .json(&json!({"resourceType":"Parameters", "parameter":parameters}));
        if let Some(etag) = etag {
            request = request.header("If-Match", etag);
        }
        let response = request.send().await.unwrap();
        let status = response.status();
        let body: Value = response.json().await.unwrap();
        assert_eq!(status, expected_status, "{path} {mode}: {body}");
        assert_eq!(body["resourceType"], "OperationOutcome", "{body}");
        if let Some(expected_valid) = expected_valid {
            let invalid = body["issue"]
                .as_array()
                .unwrap()
                .iter()
                .any(|issue| matches!(issue["severity"].as_str(), Some("error" | "fatal")));
            assert_eq!(!invalid, expected_valid, "{path} {mode}: {body}");
        }
        assert!(
            !body.to_string().contains("meta-validation-private-family"),
            "{body}"
        );
        let after: Value = client
            .get(format!("{fhir}/Patient/{id}"))
            .bearer_auth(&full)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(after, write_before, "validation changed a stored resource");
    }
    let history_after: Value = client
        .get(format!("{fhir}/Patient/{id}/_history"))
        .bearer_auth(&full)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        history_after["entry"], history_before["entry"],
        "validation changed history"
    );
    let count_after: Value = client
        .get(format!("{fhir}/Patient?_summary=count"))
        .bearer_auth(&full)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        count_after["total"], count_before["total"],
        "validation created a resource"
    );

    // SearchParameter installation constraints precede every mutation.
    let invalid_parameter = json!({"resourceType":"SearchParameter",
        "url":"https://example.test/SearchParameter/validation-no-register",
        "name":"ValidationNoRegister", "status":"active", "description":"Validation fixture",
        "code":"validation-no-register", "base":["Patient"], "type":"string", "expression":"Patient.name("});
    let search_params_before: Value = client
        .get(format!("{fhir}/SearchParameter?_summary=count"))
        .bearer_auth(&full)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let response = client
        .post(format!("{fhir}/SearchParameter/$validate?mode=create"))
        .bearer_auth(&full)
        .json(&invalid_parameter)
        .send()
        .await
        .unwrap();
    let status = response.status();
    let body: Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        body["issue"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue["severity"] == "error"
                && issue["expression"] == json!(["SearchParameter.expression"])),
        "{body}"
    );
    for skip in [false, true] {
        let response = client
            .post(format!("{fhir}/SearchParameter"))
            .bearer_auth(&full)
            .header("X-Skip-Validation", skip.to_string())
            .json(&invalid_parameter)
            .send()
            .await
            .unwrap();
        let status = response.status();
        let body: Value = response.json().await.unwrap();
        assert_eq!(
            status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "skip={skip}: {body}"
        );
    }
    let mut valid_parameter = invalid_parameter;
    valid_parameter["expression"] = json!("Patient.name");
    let response = client
        .post(format!("{fhir}/SearchParameter/$validate?mode=create"))
        .bearer_auth(&full)
        .json(&valid_parameter)
        .send()
        .await
        .unwrap();
    let status = response.status();
    let body: Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        body["issue"]
            .as_array()
            .unwrap()
            .iter()
            .all(|issue| !matches!(issue["severity"].as_str(), Some("error" | "fatal"))),
        "{body}"
    );
    let search_params_after: Value = client
        .get(format!("{fhir}/SearchParameter?_summary=count"))
        .bearer_auth(&full)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(search_params_after["total"], search_params_before["total"]);
    assert_eq!(
        client
            .get(format!("{fhir}/Patient?validation-no-register=anything"))
            .header("Prefer", "handling=strict")
            .bearer_auth(&full)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::BAD_REQUEST,
        "validation must not install a live SearchParameter"
    );

    let response = client
        .post(format!("{base}/AccessPolicy"))
        .bearer_auth(&full)
        .json(&json!({
            "resourceType":"AccessPolicy", "name":"Deny validation-mode Patient update",
            "active":true, "priority":0,
            "matcher":{"resourceTypes":["Patient"], "operations":["update"]},
            "engine":{"type":"deny"}
        }))
        .send()
        .await
        .unwrap();
    let status = response.status();
    let policy: Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::CREATED, "{policy}");
    let mode_update = json!({"resourceType":"Parameters", "parameter":[
        {"name":"mode", "valueCode":"update"}, {"name":"resource", "resource":{"resourceType":"Patient", "id":id}}
    ]});
    let mut denied = false;
    for _ in 0..100 {
        let response = client
            .post(format!("{fhir}/Patient/{id}/$validate"))
            .bearer_auth(&update_only)
            .json(&mode_update)
            .send()
            .await
            .unwrap();
        if response.status() == StatusCode::FORBIDDEN {
            denied = true;
            break;
        }
        assert_eq!(response.status(), StatusCode::OK);
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert!(
        denied,
        "write validation ignored the reloaded update policy"
    );
    assert_eq!(
        client
            .delete(format!(
                "{base}/AccessPolicy/{}",
                policy["id"].as_str().unwrap()
            ))
            .bearer_auth(&full)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NO_CONTENT
    );
    let mut allowed_again = false;
    for _ in 0..100 {
        let response = client
            .post(format!("{fhir}/Patient/{id}/$validate"))
            .bearer_auth(&update_only)
            .json(&mode_update)
            .send()
            .await
            .unwrap();
        if response.status() == StatusCode::OK {
            allowed_again = true;
            break;
        }
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert!(allowed_again, "removed update policy remained active");

    // Concurrent dry runs must not undo a real update or leave their own
    // speculative versions in history.
    let mut checks = tokio::task::JoinSet::new();
    for _ in 0..8 {
        let client = client.clone();
        let target = format!("{fhir}/Patient/{id}/$validate");
        let credential = update_only.clone();
        let etag = current_etag.clone();
        let parameters = mode_update.clone();
        checks.spawn(async move {
            let response = client
                .post(target)
                .bearer_auth(credential)
                .header("If-Match", etag)
                .json(&parameters)
                .send()
                .await
                .unwrap();
            let status = response.status();
            let body: Value = response.json().await.unwrap();
            assert_eq!(status, StatusCode::OK, "{body}");
            assert!(
                body["issue"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|issue| issue["severity"] != "error" || issue["code"] == "conflict"),
                "{body}"
            );
        });
    }
    let response = client
        .put(format!("{fhir}/Patient/{id}"))
        .bearer_auth(&full)
        .header("If-Match", &current_etag)
        .json(&write_before)
        .send()
        .await
        .unwrap();
    let status = response.status();
    let committed: Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::OK, "{committed}");
    while let Some(result) = checks.join_next().await {
        result.unwrap();
    }
    let after: Value = client
        .get(format!("{fhir}/Patient/{id}"))
        .bearer_auth(&full)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        after, committed,
        "dry-run rollback overwrote a committed update"
    );
    assert_ne!(
        after["meta"]["versionId"],
        write_before["meta"]["versionId"]
    );
    let history_after: Value = client
        .get(format!("{fhir}/Patient/{id}/_history"))
        .bearer_auth(&full)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        history_after["entry"].as_array().unwrap().len(),
        history_before["entry"].as_array().unwrap().len() + 1,
        "only the real update may create a history version"
    );

    // Named validation reads references as the invoking caller. A resource
    // supplied for validation does not require read permission for that body.
    let reference_validator = patient_scope("patient/Patient.r");
    for path in [
        "Observation/$validate".to_owned(),
        format!("Observation/{lab_id}/$validate"),
    ] {
        for reference in [
            format!("Patient/{id}"),
            format!("{}/fhir/Patient/{id}", config.base_url()),
        ] {
            let response = client.post(format!("{fhir}/{path}")).bearer_auth(&reference_validator).json(&json!({
                "resourceType":"Observation", "status":"final", "code":{"text":"validation fixture"}, "subject":{"reference":reference}
            })).send().await.unwrap();
            let status = response.status();
            let body: Value = response.json().await.unwrap();
            assert_eq!(status, StatusCode::OK, "{path}: {body}");
            assert_eq!(body["resourceType"], "OperationOutcome", "{body}");
            assert!(
                body["issue"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|issue| issue["severity"] != "error" && issue["severity"] != "fatal"),
                "{body}"
            );
        }
    }
    for (reference, credential, expected) in [
        (
            format!("Patient/{other_id}"),
            reference_validator.clone(),
            StatusCode::NOT_FOUND,
        ),
        (
            format!("{}/fhir/Patient/{other_id}", config.base_url()),
            reference_validator.clone(),
            StatusCode::NOT_FOUND,
        ),
        (
            "Patient/unknown-patient".into(),
            reference_validator.clone(),
            StatusCode::NOT_FOUND,
        ),
        (
            format!("Patient/{id}"),
            patient_scope("patient/Observation.r"),
            StatusCode::FORBIDDEN,
        ),
        (
            format!("Patient/{id}"),
            patient_scope("patient/Patient.s"),
            StatusCode::FORBIDDEN,
        ),
    ] {
        let response = client.post(format!("{fhir}/Observation/$validate")).bearer_auth(credential).json(&json!({
            "resourceType":"Observation", "status":"final", "code":{"text":"validation fixture"}, "subject":{"reference":reference}
        })).send().await.unwrap();
        let status = response.status();
        let body: Value = response.json().await.unwrap();
        assert_eq!(status, expected, "{reference}: {body}");
        assert_eq!(body["resourceType"], "OperationOutcome", "{body}");
    }
    let global_reference_reader = token(&base, "system/Patient.r").await;
    let own_version: Value = client
        .get(format!("{fhir}/Patient/{id}"))
        .bearer_auth(&full)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let version = own_version["meta"]["versionId"].as_str().unwrap();
    for (reference, credential, expected, missing) in [
        (
            "Patient/unknown-patient".to_owned(),
            global_reference_reader.clone(),
            StatusCode::OK,
            true,
        ),
        (
            format!("Patient/{id}/_history/{version}"),
            global_reference_reader.clone(),
            StatusCode::OK,
            false,
        ),
        (
            format!("Patient/{id}/_history/999999"),
            global_reference_reader.clone(),
            StatusCode::OK,
            true,
        ),
        (
            format!("Patient/{id}/_history/{version}"),
            reference_validator.clone(),
            StatusCode::OK,
            false,
        ),
    ] {
        let response = client.post(format!("{fhir}/Observation/$validate")).bearer_auth(credential).json(&json!({
            "resourceType":"Observation", "status":"final", "code":{"text":"validation fixture"}, "subject":{"reference":reference}
        })).send().await.unwrap();
        let status = response.status();
        let body: Value = response.json().await.unwrap();
        assert_eq!(status, expected, "{reference}: {body}");
        if status == StatusCode::OK {
            let has_reference_error = body["issue"].as_array().unwrap().iter().any(|issue| {
                issue["diagnostics"]
                    .as_str()
                    .unwrap_or_default()
                    .contains("does not exist")
            });
            assert_eq!(has_reference_error, missing, "{reference}: {body}");
        }
    }

    let configured_fhir = format!("{}/fhir", config.base_url());
    // CQL secondary retrieves share the invoking token's grants and ordinary
    // search policies. A client-supplied evaluation context can only narrow them.
    let cql_labs = patient_scope("patient/Patient.r patient/Observation.s?code=lab");
    let cql_request = json!({"resourceType":"Parameters", "parameter":[
        {"name":"expression", "valueString":"[Observation]"}
    ]});
    for path in [
        "/$cql".to_owned(),
        "/Observation/$cql".into(),
        format!("/Patient/{id}/$cql"),
    ] {
        let response = client
            .post(format!("{fhir}{path}"))
            .bearer_auth(&cql_labs)
            .json(&cql_request)
            .send()
            .await
            .unwrap();
        let status = response.status();
        let body: Value = response.json().await.unwrap();
        assert_eq!(status, StatusCode::OK, "{path}: {body}");
        let result: Value =
            serde_json::from_str(body["parameter"][1]["valueString"].as_str().unwrap()).unwrap();
        assert_eq!(result.as_array().unwrap().len(), 1, "{body}");
        assert_eq!(result[0]["id"], lab_id, "{body}");
    }
    let response = client
        .get(format!("{fhir}/$cql"))
        .query(&[("expression", "[Observation]")])
        .bearer_auth(&cql_labs)
        .send()
        .await
        .unwrap();
    let status = response.status();
    let body: Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::OK, "{body}");
    let result: Value =
        serde_json::from_str(body["parameter"][1]["valueString"].as_str().unwrap()).unwrap();
    assert_eq!(result[0]["id"], lab_id, "{body}");
    assert_eq!(result.as_array().unwrap().len(), 1, "{body}");

    let response = client
        .post(format!("{fhir}/$cql"))
        .bearer_auth(&cql_labs)
        .json(&json!({"resourceType":"Parameters", "parameter":[
            {"name":"expression", "valueString":"[Observation]"},
            {"name":"context", "valueCode":"Patient"},
            {"name":"contextValue", "resource":other_patient}
        ]}))
        .send()
        .await
        .unwrap();
    let status = response.status();
    let body: Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::OK, "{body}");
    let result: Value =
        serde_json::from_str(body["parameter"][1]["valueString"].as_str().unwrap()).unwrap();
    assert_eq!(result, json!([]), "{body}");

    let response = client.post(format!("{fhir}/$cql")).bearer_auth(&cql_labs).json(&json!({"resourceType":"Parameters", "parameter":[
        {"name":"library", "valueString":"library Scoped version '1.0.0'\nusing FHIR version '4.0.1'\ndefine Observations: [Observation]\ndefine Number: Count([Observation])"}
    ]})).send().await.unwrap();
    let status = response.status();
    let body: Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::OK, "{body}");
    let parts = body["parameter"][0]["part"].as_array().unwrap();
    let count: Value = serde_json::from_str(
        parts.iter().find(|part| part["name"] == "Number").unwrap()["valueString"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(count, 1, "{body}");

    let denied_cql = patient_scope("patient/Patient.r");
    let response = client
        .post(format!("{fhir}/$cql"))
        .bearer_auth(&denied_cql)
        .json(&cql_request)
        .send()
        .await
        .unwrap();
    let status = response.status();
    let body: Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["resourceType"], "OperationOutcome", "{body}");
    // Pure computation does not acquire a resource-search scope.
    let response = client
        .post(format!("{fhir}/$cql"))
        .bearer_auth(&denied_cql)
        .json(&json!({"resourceType":"Parameters", "parameter":[
            {"name":"expression", "valueString":"1 + 1"}
        ]}))
        .send()
        .await
        .unwrap();
    let status = response.status();
    let body: Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["parameter"][1]["valueString"], "2", "{body}");

    let cql_vitals = patient_scope("patient/Observation.s?code=vital");
    let (lab_response, vital_response) = tokio::join!(
        client
            .post(format!("{fhir}/$cql"))
            .bearer_auth(&cql_labs)
            .json(&cql_request)
            .send(),
        client
            .post(format!("{fhir}/$cql"))
            .bearer_auth(&cql_vitals)
            .json(&cql_request)
            .send()
    );
    for (response, expected_id) in [(lab_response, &lab_id), (vital_response, &vital_id)] {
        let response = response.unwrap();
        let status = response.status();
        let body: Value = response.json().await.unwrap();
        assert_eq!(status, StatusCode::OK, "{body}");
        let result: Value =
            serde_json::from_str(body["parameter"][1]["valueString"].as_str().unwrap()).unwrap();
        assert_eq!(result.as_array().unwrap().len(), 1, "{body}");
        assert_eq!(
            result[0]["id"].as_str(),
            Some(expected_id.as_str()),
            "{body}"
        );
    }

    let operation_scope = patient_scope("patient/Patient.r patient/Observation.s?code=lab");
    for operation_path in [
        format!("Patient/{id}/$everything"),
        "Patient/$everything".into(),
    ] {
        for method in ["GET", "POST"] {
            let path = format!("{fhir}/{operation_path}?_type=Patient,Observation&_count=1");
            let request = if method == "GET" {
                client.get(&path)
            } else {
                client
                    .post(&path)
                    .json(&json!({"resourceType":"Parameters","parameter":[]}))
            };
            let response = request.bearer_auth(&operation_scope).send().await.unwrap();
            let status = response.status();
            let body: Value = response.json().await.unwrap();
            assert_eq!(status, StatusCode::OK, "{method}: {body}");
            assert_eq!(body["total"], 2, "{body}");
            assert_eq!(body["entry"].as_array().unwrap().len(), 1, "{body}");
            let next = body["link"]
                .as_array()
                .unwrap()
                .iter()
                .find(|link| link["relation"] == "next")
                .unwrap()["url"]
                .as_str()
                .unwrap();
            assert!(
                next.starts_with(&format!("{configured_fhir}/{operation_path}?")),
                "{next}"
            );
            assert!(next.contains("_type="), "{next}");
            let next = url::Url::parse(next).unwrap();
            let response = client
                .get(format!(
                    "{base}{}?{}",
                    next.path(),
                    next.query().unwrap_or_default()
                ))
                .bearer_auth(&operation_scope)
                .send()
                .await
                .unwrap();
            let status = response.status();
            let second: Value = response.json().await.unwrap();
            assert_eq!(status, StatusCode::OK, "{second}");
            assert_eq!(second["total"], 2, "{second}");
            assert_eq!(second["entry"].as_array().unwrap().len(), 1, "{second}");
            assert_ne!(
                body["entry"][0]["resource"]["id"],
                second["entry"][0]["resource"]["id"]
            );
            for entry in [&body["entry"][0], &second["entry"][0]] {
                assert!(
                    entry["resource"]["id"] == id || entry["resource"]["id"] == lab_id,
                    "{entry}"
                );
                assert_eq!(
                    entry["fullUrl"],
                    format!(
                        "{}/{}/{}",
                        configured_fhir,
                        entry["resource"]["resourceType"].as_str().unwrap(),
                        entry["resource"]["id"].as_str().unwrap()
                    ),
                    "{entry}"
                );
            }
        }
    }
    let group_credential =
        patient_scope("system/Group.r patient/Patient.r patient/Observation.s?code=lab");
    grouped_everything_regression(
        &client,
        &fhir,
        &full,
        &group_credential,
        &[id, other_id],
        &lab_id,
    )
    .await;
    let care_credential = patient_scope(
        "patient/Patient.r patient/Observation.s?code=care-date-probe system/Practitioner.r",
    );
    everything_care_and_supporting_regression(&client, &fhir, &full, &care_credential, id).await;
    let response = client
        .get(format!(
            "{fhir}/Patient/{id}/$everything?_type=Patient,Observation&_count=1&_offset=999999"
        ))
        .bearer_auth(&operation_scope)
        .send()
        .await
        .unwrap();
    let status = response.status();
    let body: Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["total"], 2, "{body}");
    assert!(body["entry"].as_array().is_none_or(Vec::is_empty), "{body}");
    for query in [
        "_count=-1",
        "_since=invalid",
        "_type=NotAResource",
        "start=2027&end=2026",
    ] {
        let response = client
            .get(format!("{fhir}/Patient/{id}/$everything?{query}"))
            .bearer_auth(&operation_scope)
            .send()
            .await
            .unwrap();
        let status = response.status();
        let body: Value = response.json().await.unwrap();
        assert_eq!(status, StatusCode::BAD_REQUEST, "{query}: {body}");
        assert_eq!(body["resourceType"], "OperationOutcome", "{body}");
    }

    let graph = json!({"resourceType":"GraphDefinition", "name":"ScopedGraph", "status":"active", "start":"Patient",
        "link":[{"target":[{"type":"Observation","params":"subject={ref}"}]}]});
    let response = client
        .post(format!("{fhir}/Patient/{id}/$graph"))
        .bearer_auth(&operation_scope)
        .json(&json!({"resourceType":"Parameters","parameter":[
            {"name":"graph", "resource":graph}
        ]}))
        .send()
        .await
        .unwrap();
    let status = response.status();
    let body: Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::OK, "{body}");
    let graph_entries = body["entry"].as_array().unwrap();
    assert_eq!(graph_entries.len(), 2, "{body}");
    assert!(
        graph_entries
            .iter()
            .all(|entry| entry["resource"]["id"] == id || entry["resource"]["id"] == lab_id),
        "{body}"
    );

    let meta = json!({"resourceType":"Parameters","parameter":[{"name":"meta", "valueMeta":{
        "profile":["urn:test:operation-allowed"]
    }}]});
    let response = client
        .post(format!("{fhir}/Patient/{id}/$meta-add"))
        .bearer_auth(&read)
        .json(&meta)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let response = client
        .post(format!("{fhir}/Patient/{id}/$meta-add"))
        .bearer_auth(&full)
        .json(&meta)
        .send()
        .await
        .unwrap();
    let status = response.status();
    let body: Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::OK, "{body}");
    let update_scope = patient_scope("patient/Patient.u?_profile=urn:test:operation-allowed");
    let response = client
        .post(format!("{fhir}/Patient/{id}/$meta-add"))
        .bearer_auth(&update_scope)
        .json(
            &json!({"resourceType":"Parameters","parameter":[{"name":"meta","valueMeta":{
                "tag":[{"system":"urn:test","code":"added-with-update-only"}]
            }}]}),
        )
        .send()
        .await
        .unwrap();
    let status = response.status();
    let body: Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::OK, "{body}");
    let before: Value = client
        .get(format!("{fhir}/Patient/{id}"))
        .bearer_auth(&full)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(
        before["meta"]["profile"]
            .as_array()
            .unwrap()
            .iter()
            .any(|profile| profile == "urn:test:operation-allowed"),
        "{before}"
    );
    let response = client
        .post(format!("{fhir}/Patient/{id}/$meta-delete"))
        .bearer_auth(&update_scope)
        .json(&meta)
        .send()
        .await
        .unwrap();
    let status = response.status();
    let body: Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    let after: Value = client
        .get(format!("{fhir}/Patient/{id}"))
        .bearer_auth(&full)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        after, before,
        "failed grant-changing meta update must roll back"
    );

    // An update-only caller cannot add false conformance claims. Errors must
    // not disclose the stored body, and rejected edits must not alter versions.
    for (metadata, expected) in [
        (
            json!({"profile":["http://hl7.org/fhir/StructureDefinition/Observation"],
            "tag":[{"system":"urn:test", "code":"must-not-be-added"}]}),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (json!({"profile":"not-an-array"}), StatusCode::BAD_REQUEST),
        (json!({"tag":[{"code":42}]}), StatusCode::BAD_REQUEST),
        (
            json!({"security":["not-a-coding"]}),
            StatusCode::BAD_REQUEST,
        ),
    ] {
        let response = client
            .post(format!("{fhir}/Patient/{id}/$meta-add"))
            .bearer_auth(&update_scope)
            .json(&json!({"resourceType":"Parameters", "parameter":[{
                "name":"meta", "valueMeta":metadata
            }]}))
            .send()
            .await
            .unwrap();
        let status = response.status();
        let body: Value = response.json().await.unwrap();
        assert_eq!(status, expected, "{metadata}: {body}");
        assert_eq!(body["resourceType"], "OperationOutcome", "{body}");
        assert!(
            !body.to_string().contains("meta-validation-private-family"),
            "{body}"
        );
        let unchanged: Value = client
            .get(format!("{fhir}/Patient/{id}"))
            .bearer_auth(&full)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(
            unchanged, before,
            "rejected metadata must not create a new version"
        );
    }

    // Addition uses set identity within a single request as well as against
    // existing metadata; deletion omits empty arrays and ignores Coding display.
    let set_metadata = json!({"profile":[
        "http://hl7.org/fhir/StructureDefinition/Patient",
        "http://hl7.org/fhir/StructureDefinition/Patient"
    ], "_profile":[{"id":"first"}, {"id":"second"}], "security":[
        {"system":"http://terminology.hl7.org/CodeSystem/v3-Confidentiality", "code":"N", "display":"first"},
        {"system":"http://terminology.hl7.org/CodeSystem/v3-Confidentiality", "code":"N", "display":"second"}
    ]});
    let response = client.post(format!("{fhir}/Patient/{id}/$meta-add"))
        .bearer_auth(&update_scope)
        .json(&json!({"resourceType":"Parameters", "parameter":[{"name":"meta", "valueMeta":set_metadata}]}))
        .send().await.unwrap();
    let status = response.status();
    let body: Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::OK, "{body}");
    let meta = &body["parameter"][0]["valueMeta"];
    assert_eq!(meta["profile"].as_array().unwrap().len(), 2, "{body}");
    assert_eq!(meta["_profile"], json!([null, {"id":"first"}]), "{body}");
    assert_eq!(meta["security"].as_array().unwrap().len(), 1, "{body}");
    let response = client.post(format!("{fhir}/Patient/{id}/$meta-delete"))
        .bearer_auth(&update_scope)
        .json(&json!({"resourceType":"Parameters", "parameter":[{"name":"meta", "valueMeta":{
            "profile":["http://hl7.org/fhir/StructureDefinition/Patient"],
            "security":[{"system":"http://terminology.hl7.org/CodeSystem/v3-Confidentiality", "code":"N", "display":"ignored"}]
        }}]}))
        .send().await.unwrap();
    let status = response.status();
    let body: Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::OK, "{body}");
    let meta = &body["parameter"][0]["valueMeta"];
    assert_eq!(
        meta["profile"],
        json!(["urn:test:operation-allowed"]),
        "{body}"
    );
    assert!(meta.get("security").is_none(), "{body}");
    assert!(meta.get("_profile").is_none(), "{body}");

    // Metadata validation must not fetch an unchanged subject using read rights.
    let observation_update = patient_scope("patient/Observation.u?code=lab");
    let response = client
        .post(format!("{fhir}/Observation/{lab_id}/$meta-add"))
        .bearer_auth(&observation_update)
        .json(
            &json!({"resourceType":"Parameters", "parameter":[{"name":"meta", "valueMeta":{
                "tag":[{"system":"urn:test", "code":"no-secondary-read"}]
            }}]}),
        )
        .send()
        .await
        .unwrap();
    let status = response.status();
    let body: Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::OK, "{body}");

    // An inline resource also respects policies even when its SMART grant allows it.
    let response = client
        .post(format!("{base}/AccessPolicy"))
        .bearer_auth(&full)
        .json(&json!({
            "resourceType":"AccessPolicy", "name":"Deny inline Observation",
            "active":true, "priority":0,
            "matcher":{"resourceTypes":["Observation"], "operations":["search","read"]},
            "engine":{"type":"deny"}
        }))
        .send()
        .await
        .unwrap();
    let status = response.status();
    let policy: Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::CREATED, "{policy}");
    let policy_id = policy["id"].as_str().unwrap();
    let mut policy_loaded = false;
    for _ in 0..100 {
        let response = client
            .get(format!("{fhir}/Observation"))
            .bearer_auth(&full)
            .send()
            .await
            .unwrap();
        if response.status() == StatusCode::FORBIDDEN {
            policy_loaded = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert!(policy_loaded, "Observation policy did not reload");
    let response = client.post(format!("{fhir}/DiagnosticReport/$validate")).bearer_auth(patient_scope("patient/Observation.r?code=lab")).json(&json!({
        "resourceType":"DiagnosticReport", "status":"final", "code":{"text":"validation fixture"}, "result":[{"reference":format!("Observation/{lab_id}")}]
    })).send().await.unwrap();
    let status = response.status();
    let body: Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["resourceType"], "OperationOutcome", "{body}");
    let response = client
        .post(format!("{fhir}/$cql"))
        .bearer_auth(&cql_labs)
        .json(&cql_request)
        .send()
        .await
        .unwrap();
    let status = response.status();
    let body: Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["resourceType"], "OperationOutcome", "{body}");
    let response = client
        .get(format!("{fhir}/Observation/{lab_id}/$meta"))
        .bearer_auth(patient_scope("patient/Observation.r?code=lab"))
        .send()
        .await
        .unwrap();
    let status = response.status();
    let body: Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    let response = client
        .get(format!(
            "{fhir}/Patient/{id}/$everything?_type=Patient,Observation"
        ))
        .bearer_auth(&operation_scope)
        .send()
        .await
        .unwrap();
    let status = response.status();
    let body: Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["total"], 1, "{body}");
    assert_eq!(body["entry"].as_array().unwrap().len(), 1, "{body}");
    assert_eq!(body["entry"][0]["resource"]["id"], id, "{body}");
    let response = client
        .get(format!(
            "{fhir}/Patient?_revinclude=Observation:subject&_total=accurate"
        ))
        .bearer_auth(patient_scope(
            "patient/Patient.s patient/Observation.s?code=lab",
        ))
        .send()
        .await
        .unwrap();
    let status = response.status();
    let body: Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["total"], 1, "{body}");
    assert_eq!(body["entry"].as_array().unwrap().len(), 1, "{body}");
    for path in [
        "?_type=Patient,Observation&_total=accurate".to_string(),
        format!("/Patient/{id}/*?_total=accurate"),
    ] {
        let response = client
            .get(format!("{fhir}{path}"))
            .bearer_auth(&aggregate)
            .send()
            .await
            .unwrap();
        let status = response.status();
        let body: Value = response.json().await.unwrap();
        assert_eq!(status, StatusCode::OK, "{path}: {body}");
        assert_eq!(body["total"], 1, "{body}");
        assert_eq!(body["entry"][0]["resource"]["id"], id);
    }
    assert_eq!(
        client
            .get(format!("{fhir}?_type=Observation"))
            .bearer_auth(&aggregate)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    for kind in ["batch", "transaction"] {
        let urls = [
            "Patient?_revinclude=Observation:subject&_total=accurate".to_string(),
            "?_type=Patient,Observation&_total=accurate".to_string(),
            format!("Patient/{id}/*?_total=accurate"),
        ];
        let entries: Vec<_> = urls
            .iter()
            .map(|url| json!({"request":{"method":"GET","url":url}}))
            .collect();
        let response = client
            .post(&fhir)
            .bearer_auth(&aggregate)
            .json(&json!({"resourceType":"Bundle","type":kind,"entry":entries}))
            .send()
            .await
            .unwrap();
        let status = response.status();
        let body: Value = response.json().await.unwrap();
        assert_eq!(status, StatusCode::OK, "{kind}: {body}");
        for entry in body["entry"].as_array().unwrap() {
            assert_eq!(entry["response"]["status"], "200 OK", "{entry}");
            let search = &entry["resource"];
            assert_eq!(search["total"], 1, "{search}");
            assert_eq!(search["entry"].as_array().unwrap().len(), 1, "{search}");
            assert_eq!(search["entry"][0]["resource"]["id"], id, "{search}");
        }
        let response = client
            .post(&fhir)
            .bearer_auth(&aggregate)
            .json(&json!({"resourceType":"Bundle","type":kind,"entry":[
                {"request":{"method":"GET","url":"Observation"}}
            ]}))
            .send()
            .await
            .unwrap();
        let status = response.status();
        let body: Value = response.json().await.unwrap();
        if kind == "batch" {
            assert_eq!(status, StatusCode::OK, "{body}");
            assert_eq!(
                body["entry"][0]["response"]["status"], "403 Forbidden",
                "{body}"
            );
        } else {
            assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
        }
    }
    assert_eq!(
        client
            .delete(format!("{base}/AccessPolicy/{policy_id}"))
            .bearer_auth(&full)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NO_CONTENT
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

    // POST ignores even a malformed client id before validating content.
    let response = client
        .post(format!("{fhir}/Patient"))
        .bearer_auth(&full)
        .json(&json!({"resourceType":"Patient", "id":42, "active":true}))
        .send()
        .await
        .unwrap();
    let status = response.status();
    let created: Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert!(created["id"].is_string(), "{created}");

    // Primary writes and speculative modes share native grant constraints.
    // These credentials deliberately omit read/search permissions.
    let filtered_create = patient_scope("system/Patient.c?active=true");
    let filtered_update = patient_scope("system/Patient.u?active=true");
    let filtered_delete = patient_scope("system/Patient.d?active=true");
    let before_count: Value = client
        .get(format!("{fhir}/Patient?_summary=count"))
        .bearer_auth(&full)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    for path in ["Patient", "Patient/$validate?mode=create"] {
        let response = client
            .post(format!("{fhir}/{path}"))
            .bearer_auth(&filtered_create)
            .json(&json!({"resourceType":"Patient", "active":false}))
            .send()
            .await
            .unwrap();
        let status = response.status();
        let body: Value = response.json().await.unwrap();
        assert_eq!(status, StatusCode::FORBIDDEN, "{path}: {body}");
    }
    // A launch Patient ID in POST content cannot grant creation of another ID.
    let response = client
        .post(format!("{fhir}/Patient"))
        .bearer_auth(patient_scope("patient/Patient.c"))
        .json(&json!({"resourceType":"Patient", "id":id, "active":true}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let after_count: Value = client
        .get(format!("{fhir}/Patient?_summary=count"))
        .bearer_auth(&full)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(after_count["total"], before_count["total"]);
    let response = client
        .post(format!("{fhir}/Patient/$validate?mode=create"))
        .bearer_auth(&filtered_create)
        .json(&json!({"resourceType":"Patient", "id":42, "active":true}))
        .send()
        .await
        .unwrap();
    let status = response.status();
    let body: Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        body["issue"]
            .as_array()
            .unwrap()
            .iter()
            .all(|i| i["severity"] != "error"),
        "{body}"
    );
    let response = client
        .post(format!("{fhir}/Patient"))
        .bearer_auth(&filtered_create)
        .json(&json!({"resourceType":"Patient", "id":42, "active":true}))
        .send()
        .await
        .unwrap();
    let status = response.status();
    let atomic_patient: Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::CREATED, "{atomic_patient}");
    let atomic_id = atomic_patient["id"].as_str().unwrap();
    let atomic_url = format!("{fhir}/Patient/{atomic_id}");
    let history: Value = client
        .get(format!("{atomic_url}/_history"))
        .bearer_auth(&full)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    // Ordinary policies must see both the locked previous JSON and the exact
    // proposed stored JSON, including with an unrestricted update-only grant.
    let response = client.post(format!("{base}/AccessPolicy")).bearer_auth(&full)
        .json(&json!({"resourceType":"AccessPolicy", "name":"Protect inactive Patient writes", "active":true,"priority":0,
            "matcher":{"resourceTypes":["Patient"],"operations":["update","patch"]},
            "engine":{"type":"quickjs","script":"if (resource && resource.resource.active === false) return deny('inactive Patient'); return abstain();"}
        })).send().await.unwrap();
    let status = response.status();
    let policy: Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::CREATED, "{policy}");
    let mut denied = false;
    for _ in 0..100 {
        let response = client
            .post(format!("{atomic_url}/$validate?mode=update"))
            .bearer_auth(&update_only)
            .json(&json!({"resourceType":"Patient","id":atomic_id,"active":false}))
            .send()
            .await
            .unwrap();
        if response.status() == StatusCode::FORBIDDEN {
            denied = true;
            break;
        }
        assert_eq!(response.status(), StatusCode::OK);
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert!(denied, "write policies did not inspect proposed content");
    let response = client
        .put(&atomic_url)
        .bearer_auth(&update_only)
        .json(&json!({"resourceType":"Patient","id":atomic_id,"active":false}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let response = client
        .patch(&atomic_url)
        .bearer_auth(&update_only)
        .header("Content-Type", "application/json-patch+json")
        .json(&json!([{"op":"replace","path":"/active","value":false}]))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let current: Value = client
        .get(&atomic_url)
        .bearer_auth(&full)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(current, atomic_patient);
    let current_history: Value = client
        .get(format!("{atomic_url}/_history"))
        .bearer_auth(&full)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(current_history, history);
    let response = client
        .post(format!("{fhir}/Patient"))
        .bearer_auth(&full)
        .json(&json!({"resourceType":"Patient","active":false}))
        .send()
        .await
        .unwrap();
    let status = response.status();
    let inactive: Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::CREATED, "{inactive}");
    let inactive_id = inactive["id"].as_str().unwrap();
    let inactive_url = format!("{fhir}/Patient/{inactive_id}");
    let response = client
        .put(&inactive_url)
        .bearer_auth(&update_only)
        .json(&json!({"resourceType":"Patient","id":inactive_id,"active":true}))
        .send()
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        StatusCode::FORBIDDEN,
        "write policies must inspect previous content as well"
    );
    let current: Value = client
        .get(&inactive_url)
        .bearer_auth(&full)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(current, inactive);
    let response = client
        .patch(&inactive_url)
        .bearer_auth(&update_only)
        .header("Content-Type", "application/json-patch+json")
        .json(&json!([{"op":"replace","path":"/active","value":true}]))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        client
            .delete(format!(
                "{base}/AccessPolicy/{}",
                policy["id"].as_str().unwrap()
            ))
            .bearer_auth(&full)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NO_CONTENT
    );
    let mut allowed = false;
    for _ in 0..100 {
        let response = client
            .post(format!("{atomic_url}/$validate?mode=update"))
            .bearer_auth(&update_only)
            .json(&json!({"resourceType":"Patient","id":atomic_id,"active":false}))
            .send()
            .await
            .unwrap();
        if response.status() == StatusCode::OK {
            allowed = true;
            break;
        }
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert!(allowed, "write policy removal did not reload");

    for (path, resource, expected) in [
        (
            atomic_url.clone(),
            json!({"resourceType":"Patient", "id":atomic_id,"active":false}),
            StatusCode::FORBIDDEN,
        ),
        (
            format!("{atomic_url}/$validate?mode=update"),
            json!({"resourceType":"Patient", "id":atomic_id,"active":false}),
            StatusCode::FORBIDDEN,
        ),
    ] {
        let request = if path.contains("$validate") {
            client.post(path)
        } else {
            client.put(path)
        };
        let response = request
            .bearer_auth(&filtered_update)
            .json(&resource)
            .send()
            .await
            .unwrap();
        let status = response.status();
        let body: Value = response.json().await.unwrap();
        assert_eq!(status, expected, "{body}");
        let current: Value = client
            .get(&atomic_url)
            .bearer_auth(&full)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(current, atomic_patient);
        let current_history: Value = client
            .get(format!("{atomic_url}/_history"))
            .bearer_auth(&full)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(current_history, history);
    }
    for method in [reqwest::Method::PUT, reqwest::Method::DELETE] {
        let suffix = if method == reqwest::Method::PUT {
            "u"
        } else {
            "d"
        };
        let response = client
            .request(method, &atomic_url)
            .bearer_auth(patient_scope(&format!(
                "system/Patient.{suffix}?active=false"
            )))
            .json(&json!({"resourceType":"Patient","id":atomic_id,"active":false}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
    // Valid quoted ETags, including nonnumeric stale versions, fail with 412;
    // malformed and repeated conditions fail with 400 before any mutation.
    for (etag, expected) in [
        ("W/\"not-current\"", StatusCode::PRECONDITION_FAILED),
        ("42", StatusCode::BAD_REQUEST),
        ("*", StatusCode::BAD_REQUEST),
        ("\"1\", \"2\"", StatusCode::BAD_REQUEST),
    ] {
        for method in [reqwest::Method::PUT, reqwest::Method::DELETE] {
            let credential = if method == reqwest::Method::PUT {
                &filtered_update
            } else {
                &filtered_delete
            };
            let response = client
                .request(method, &atomic_url)
                .bearer_auth(credential)
                .header("If-Match", etag)
                .json(&json!({"resourceType":"Patient","id":atomic_id,"active":true}))
                .send()
                .await
                .unwrap();
            let status = response.status();
            let body: Value = response.json().await.unwrap();
            assert_eq!(status, expected, "{etag}: {body}");
        }
    }
    let etag = format!(
        "W/\"{}\"",
        atomic_patient["meta"]["versionId"].as_str().unwrap()
    );
    let mut updates = Vec::new();
    for index in 0..8 {
        let client = client.clone();
        let url = atomic_url.clone();
        let credential = filtered_update.clone();
        let etag = etag.clone();
        let resource = json!({"resourceType":"Patient","id":atomic_id,"active":true,"name":[{"family":format!("atomic-{index}")}]});
        updates.push(tokio::spawn(async move {
            client
                .put(url)
                .bearer_auth(credential)
                .header("If-Match", etag)
                .json(&resource)
                .send()
                .await
                .unwrap()
                .status()
        }));
    }
    let mut successes = 0;
    for update in updates {
        let status = update.await.unwrap();
        if status == StatusCode::OK {
            successes += 1;
        } else {
            assert_eq!(status, StatusCode::PRECONDITION_FAILED);
        }
    }
    assert_eq!(successes, 1);
    let after_history: Value = client
        .get(format!("{atomic_url}/_history"))
        .bearer_auth(&full)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        after_history["entry"].as_array().unwrap().len(),
        history["entry"].as_array().unwrap().len() + 1
    );
    // Update grants allow create-on-update, also for matching filtered candidates.
    let upsert_id = "atomic-filtered-upsert";
    let response = client
        .put(format!("{fhir}/Patient/{upsert_id}"))
        .bearer_auth(&filtered_update)
        .header("Prefer", "return=minimal")
        .json(&json!({"resourceType":"Patient","id":upsert_id,"active":true}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    assert!(response.headers().contains_key("Location"));
    assert!(response.bytes().await.unwrap().is_empty());
    let response = client
        .delete(&atomic_url)
        .bearer_auth(&filtered_delete)
        .header("If-Match", etag)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::PRECONDITION_FAILED);
    let response = client
        .delete(&atomic_url)
        .bearer_auth(&filtered_delete)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let response = client
        .delete(&atomic_url)
        .bearer_auth(&filtered_delete)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let response = client
        .delete(&atomic_url)
        .bearer_auth(&delete_only)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let observation_create = patient_scope("patient/Observation.c?code=atomic-write");
    let observation_update = patient_scope("patient/Observation.u?code=atomic-write");
    Box::pin(primary_reference_patch_regressions(
        &client,
        &fhir,
        &full,
        (
            &filtered_update,
            &filtered_create,
            &patient_scope("system/Patient.u?active=false"),
        ),
        (&observation_create, &observation_update),
        (id, other_id),
    ))
    .await;

    Box::pin(conditional_create_regressions(
        &client,
        &fhir,
        &full,
        &filtered_create,
        &observation_create,
        id,
        other_id,
    ))
    .await;

    Box::pin(conditional_mutation_regressions(
        &client,
        &fhir,
        &base,
        &full,
        &filtered_update,
        &filtered_delete,
        config
            .storage
            .postgres
            .as_ref()
            .unwrap()
            .url
            .as_deref()
            .unwrap(),
    ))
    .await;
    Box::pin(conditional_patient_compartment_mutations(
        &client,
        &fhir,
        &full,
        (
            &observation_update,
            &patient_scope("patient/Observation.d?code=atomic-write"),
        ),
        (id, other_id),
    ))
    .await;
    Box::pin(batch_write_regressions(
        &client,
        &fhir,
        &configured_fhir,
        &full,
        (&filtered_create, &filtered_update, &filtered_delete),
        (
            &observation_update,
            &patient_scope("patient/Observation.d?code=atomic-write"),
        ),
        (id, other_id),
    ))
    .await;
    Box::pin(transaction_write_regressions(
        &client,
        &fhir,
        &base,
        &full,
        (&filtered_create, &filtered_update, &filtered_delete),
        (&observation_create, &observation_update),
        (id, other_id),
    ))
    .await;
    Box::pin(transaction_conditional_grant_regressions(
        &client,
        &fhir,
        &base,
        &full,
        (
            &observation_create,
            &observation_update,
            &patient_scope("patient/Observation.d?code=atomic-write"),
        ),
        (id, other_id),
    ))
    .await;
    Box::pin(transaction_missing_identity_race(
        &client,
        &fhir,
        &full,
        &filtered_update,
        config
            .storage
            .postgres
            .as_ref()
            .unwrap()
            .url
            .as_deref()
            .unwrap(),
    ))
    .await;

    Box::pin(reference_alias_regressions(
        &client,
        &fhir,
        &configured_fhir,
        &full,
        &observation_create,
        id,
        other_id,
    ))
    .await;
    Box::pin(reference_versions_and_profiles(
        &client,
        &fhir,
        &configured_fhir,
        &base,
        &full,
        config
            .storage
            .postgres
            .as_ref()
            .unwrap()
            .url
            .as_deref()
            .unwrap(),
    ))
    .await;

    Box::pin(snapshot_read_regressions(&client, &base, &fhir, &full)).await;
    Box::pin(snapshot_search_policy_regressions(
        &client, &base, &fhir, &full,
    ))
    .await;
    Box::pin(linked_compartment_grants(
        &client,
        &fhir,
        &full,
        (
            &patient_scope("patient/Observation.s"),
            &patient_scope("system/Observation.s"),
        ),
        id,
    ))
    .await;

    let wrong_hint = client
        .post(format!("{base}/auth/introspect"))
        .basic_auth("development-backend", Some("isolated-test-secret"))
        .form(&[
            ("token", read.as_str()),
            ("token_type_hint", "refresh_token"),
        ])
        .send()
        .await
        .unwrap();
    let status = wrong_hint.status();
    let body: Value = wrong_hint.json().await.unwrap();
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["active"], true, "{body}");

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
    Box::pin(current_delegation_regressions(
        &client, &base, &fhir, &config, &signing,
    ))
    .await;
    server.abort();
    Box::pin(anonymous_access_preserves_presented_grants(config)).await;
    terminology_server.abort();
}

async fn current_delegation_regressions(
    client: &Client,
    base: &str,
    fhir: &str,
    config: &AppConfig,
    signing: &JwtService,
) {
    use octofhir_auth::storage::{ClientStorage, RefreshTokenStorage, User, UserStorage};
    use octofhir_auth::types::{GrantType, RefreshToken};
    let pool = Arc::new(
        sqlx_postgres::PgPoolOptions::new()
            .max_connections(2)
            .connect(
                config
                    .storage
                    .postgres
                    .as_ref()
                    .unwrap()
                    .url
                    .as_deref()
                    .unwrap(),
            )
            .await
            .unwrap(),
    );
    let clients = octofhir_auth_postgres::ArcClientStorage::new(pool.clone());
    let users = octofhir_auth_postgres::ArcUserStorage::new(pool.clone());
    let refresh_tokens = octofhir_auth_postgres::ArcRefreshTokenStorage::new(pool);
    let original = clients
        .find_by_client_id("development-backend")
        .await
        .unwrap()
        .unwrap();
    let observer_id = format!("delegation-observer-{}", uuid::Uuid::new_v4());
    let mut observer = original.clone();
    observer.client_id = observer_id.clone();
    clients.create(&observer).await.unwrap();
    async fn introspect(client: &Client, base: &str, observer: &str, credential: &str) -> Value {
        let response = client
            .post(format!("{base}/auth/introspect"))
            .basic_auth(observer, Some("isolated-test-secret"))
            .form(&[("token", credential), ("token_type_hint", "refresh_token")])
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        response.json().await.unwrap()
    }
    let full = token(base, "system/*.cruds").await;
    assert_eq!(
        introspect(client, base, &observer_id, &full).await["active"],
        true
    );
    let response = client
        .post(format!("{fhir}/Patient"))
        .bearer_auth(&full)
        .json(&json!({"resourceType":"Patient","active":true}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let patient: Value = response.json().await.unwrap();
    let url = format!("{fhir}/Patient/{}", patient["id"].as_str().unwrap());
    // Warm both signature and context caches, then change authoritative state.
    assert_eq!(
        client
            .get(&url)
            .bearer_auth(&full)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    let mut changed = original.clone();
    changed.active = false;
    clients.update(&original.client_id, &changed).await.unwrap();
    assert_eq!(
        introspect(client, base, &observer_id, &full).await,
        json!({"active":false})
    );
    assert_eq!(
        client
            .get(&url)
            .bearer_auth(&full)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    clients
        .update(&original.client_id, &original)
        .await
        .unwrap();
    assert_eq!(
        client
            .get(&url)
            .bearer_auth(&full)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    changed = original.clone();
    changed.scopes = vec!["system/Patient.r".into()];
    clients.update(&original.client_id, &changed).await.unwrap();
    assert_eq!(
        introspect(client, base, &observer_id, &full).await,
        json!({"active":false})
    );
    assert_eq!(
        client
            .get(&url)
            .bearer_auth(&full)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let narrow = token(base, "system/Patient.r").await;
    assert_eq!(
        introspect(client, base, &observer_id, &narrow).await["active"],
        true
    );
    assert_eq!(
        client
            .get(&url)
            .bearer_auth(&narrow)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    clients
        .update(&original.client_id, &original)
        .await
        .unwrap();
    assert_eq!(
        client
            .get(&url)
            .bearer_auth(&full)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    for audience in [
        Vec::new(),
        vec!["https://another-resource-server.example/fhir".into()],
    ] {
        let claims = AccessTokenClaims::builder(
            &config.auth.issuer,
            "development-backend",
            "development-backend",
        )
        .audience(audience)
        .scope("system/Patient.r")
        .build();
        let credential = signing.encode(&claims).unwrap();
        assert_eq!(
            introspect(client, base, &observer_id, &credential).await,
            json!({"active":false})
        );
        assert_eq!(
            client
                .get(&url)
                .bearer_auth(&credential)
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
    }
    let mut user = User::builder(format!("live-delegation-{}", uuid::Uuid::new_v4()))
        .roles(vec!["delegated-doctor".into()])
        .build();
    user.fhir_user = Some(url.clone());
    users.create(&user).await.unwrap();
    // Preserve the original FHIR audience through refresh-token rotation, and
    // consult the same current user records before every refresh.
    changed = original.clone();
    changed.grant_types.push(GrantType::RefreshToken);
    clients.update(&original.client_id, &changed).await.unwrap();
    let refresh_value = RefreshToken::generate_token();
    let configured_audience = format!("{}/fhir", config.base_url().trim_end_matches('/'));
    let now = time::OffsetDateTime::now_utc();
    refresh_tokens
        .create(&RefreshToken {
            id: uuid::Uuid::now_v7(),
            token_hash: RefreshToken::hash_token(&refresh_value),
            client_id: original.client_id.clone(),
            user_id: Some(user.id.clone()),
            scope: "system/Patient.r".into(),
            audience: Some(configured_audience.clone()),
            launch_context: None,
            created_at: now,
            expires_at: Some(now + time::Duration::hours(1)),
            revoked_at: None,
        })
        .await
        .unwrap();
    async fn refresh(client: &Client, base: &str, value: &str) -> reqwest::Response {
        client
            .post(format!("{base}/auth/token"))
            .basic_auth("development-backend", Some("isolated-test-secret"))
            .form(&[("grant_type", "refresh_token"), ("refresh_token", value)])
            .send()
            .await
            .unwrap()
    }
    let refreshed = refresh(client, base, &refresh_value).await;
    assert_eq!(refreshed.status(), StatusCode::OK);
    let refreshed: Value = refreshed.json().await.unwrap();
    let refreshed_credential = refreshed["access_token"].as_str().unwrap();
    assert_eq!(
        signing
            .decode::<AccessTokenClaims>(refreshed_credential)
            .unwrap()
            .claims
            .aud,
        vec![configured_audience]
    );
    assert_eq!(
        introspect(client, base, &observer_id, refreshed_credential).await["fhir_user"],
        url
    );
    assert_eq!(
        client
            .get(&url)
            .bearer_auth(refreshed_credential)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        refresh(client, base, &refresh_value).await.status(),
        StatusCode::BAD_REQUEST
    );
    let rotated_refresh = refreshed["refresh_token"].as_str().unwrap();
    let claims = AccessTokenClaims::builder(&config.auth.issuer, &user.id, "development-backend")
        .audience(vec![config.base_url()])
        .scope("system/Patient.r")
        .build();
    let credential = signing.encode(&claims).unwrap();
    assert_eq!(
        client
            .get(&url)
            .bearer_auth(&credential)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    let response = client.post(format!("{base}/AccessPolicy")).bearer_auth(&full).json(&json!({"resourceType":"AccessPolicy", "name":"Current delegated role", "active":true,"priority":0,
        "matcher":{"resourceTypes":["Patient"],"operations":["read"]},
        "engine":{"type":"quickjs","script":format!("if (user && user.id === '{}' && user.roles.indexOf('delegated-doctor') < 0) return deny('Role removed'); return abstain();",user.id)}})).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let policy: Value = response.json().await.unwrap();
    user.roles.clear();
    users.update(&user).await.unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let response = client
                .get(&url)
                .bearer_auth(&credential)
                .send()
                .await
                .unwrap();
            if response.status() == StatusCode::FORBIDDEN {
                break;
            }
            assert_eq!(response.status(), StatusCode::OK);
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("Cached token must use the user's current roles");
    user.active = false;
    users.update(&user).await.unwrap();
    assert_eq!(
        refresh(client, base, rotated_refresh).await.status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        introspect(client, base, &observer_id, refreshed_credential).await,
        json!({"active":false})
    );
    assert_eq!(
        introspect(client, base, &observer_id, &credential).await,
        json!({"active":false})
    );
    assert_eq!(
        client
            .get(&url)
            .bearer_auth(&credential)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    user.active = true;
    user.roles.push("delegated-doctor".into());
    users.update(&user).await.unwrap();
    assert_eq!(
        client
            .get(&url)
            .bearer_auth(&credential)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    assert!(
        users
            .list(100, 0)
            .await
            .unwrap()
            .iter()
            .any(|stored| stored.id == user.id)
    );
    users.delete(&user.id).await.unwrap();
    assert!(
        users
            .list(100, 0)
            .await
            .unwrap()
            .iter()
            .all(|stored| stored.id != user.id)
    );
    assert_eq!(
        refresh(client, base, rotated_refresh).await.status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        introspect(client, base, &observer_id, &credential).await,
        json!({"active":false})
    );
    assert_eq!(
        client
            .get(&url)
            .bearer_auth(&credential)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        client
            .delete(format!(
                "{base}/AccessPolicy/{}",
                policy["id"].as_str().unwrap()
            ))
            .bearer_auth(&full)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NO_CONTENT
    );
    clients
        .update(&original.client_id, &original)
        .await
        .unwrap();
    clients.delete(&observer_id).await.unwrap();
}

async fn anonymous_access_preserves_presented_grants(mut config: AppConfig) {
    config.auth.policy.anonymous_access = true;
    config.auth.cookie.enabled = true;
    config.sql_on_fhir.enabled = true;
    let cookie_name = config.auth.cookie.name.clone();
    let manager = Arc::new(ConfigurationManager::builder().build().await.unwrap());
    let app = Box::pin(build_app(&config, manager)).await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = Client::new();
    let fhir = format!("{base}/fhir");
    let full = token(&base, "system/*.cruds").await;
    let create = token(&base, "system/Patient.c").await;
    let read = token(&base, "system/Patient.r").await;
    let search = token(&base, "system/Patient.s").await;
    let update = token(&base, "system/Patient.u").await;

    let response = client
        .post(format!("{fhir}/Patient"))
        .json(&json!({"resourceType":"Patient", "active":true}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let patient: Value = response.json().await.unwrap();
    let id = patient["id"].as_str().unwrap();
    let target = format!("{fhir}/Patient/{id}");
    for request in [
        client.get(&target),
        client.get(&target).bearer_auth(&full),
        client.get(&target).bearer_auth(&read),
    ] {
        let response = request.send().await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.json::<Value>().await.unwrap(), patient);
    }
    for request in [
        client.get(&target).bearer_auth(&create),
        client.get(&target).bearer_auth(&search),
        client
            .get(&target)
            .header("Cookie", format!("{cookie_name}={create}")),
        client.get(format!("{fhir}/Patient")).bearer_auth(&read),
        client
            .get(format!("{fhir}/Patient/{id}/$everything?_type=Patient"))
            .bearer_auth(&create),
    ] {
        let response = request.send().await.unwrap();
        let status = response.status();
        let body: Value = response.json().await.unwrap();
        assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    }
    for request in [
        client.get(&target).bearer_auth("invalid"),
        client.get(&target).header("Authorization", "Bearer "),
        client.get(&target).header("Authorization", "invalid"),
        client
            .get(&target)
            .header("Cookie", format!("{cookie_name}=")),
    ] {
        assert_eq!(
            request.send().await.unwrap().status(),
            StatusCode::UNAUTHORIZED
        );
    }
    let response = client
        .get(format!("{fhir}/Patient/{id}/$everything?_type=Patient"))
        .bearer_auth(&read)
        .send()
        .await
        .unwrap();
    let status = response.status();
    let everything: Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::OK, "{everything}");
    assert!(
        everything["entry"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["resource"] == patient)
    );
    Box::pin(sof_source_grants_and_snapshot_policies(
        &client, &base, &fhir, &full, &read, id,
    ))
    .await;
    assert_eq!(
        client
            .put(&target)
            .bearer_auth(&update)
            .json(&json!({"resourceType":"Patient", "id":id, "active":false}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    for kind in ["batch", "transaction"] {
        let response = client.post(&fhir).bearer_auth(&create)
            .json(&json!({"resourceType":"Bundle", "type":kind, "entry":[
                {"request":{"method":"POST", "url":"Patient"}, "resource":{"resourceType":"Patient", "active":true}}
            ]})).send().await.unwrap();
        let status = response.status();
        let result: Value = response.json().await.unwrap();
        assert_eq!(status, StatusCode::OK, "{result}");
        assert_eq!(result["entry"][0]["response"]["status"], "201 Created");
        let response = client
            .post(&fhir)
            .bearer_auth(&create)
            .json(&json!({"resourceType":"Bundle", "type":kind, "entry":[
                {"request":{"method":"GET", "url":format!("Patient/{id}")}}
            ]}))
            .send()
            .await
            .unwrap();
        let status = response.status();
        let result: Value = response.json().await.unwrap();
        if kind == "batch" && status == StatusCode::OK {
            assert_eq!(
                result["entry"][0]["response"]["status"], "403 Forbidden",
                "{result}"
            );
            assert_eq!(
                result["entry"][0]["response"]["outcome"]["resourceType"],
                "OperationOutcome"
            );
        } else {
            assert_eq!(status, StatusCode::FORBIDDEN, "{result}");
        }
    }
    async_bundle_ownership_regression(&client, &fhir, &full, &create, &read, true).await;
    Box::pin(native_import_grants_and_metadata(
        &client, &base, &fhir, &full, &create, &update, &read,
    ))
    .await;
    server.abort();
}

async fn native_import_grants_and_metadata(
    client: &Client,
    base: &str,
    fhir: &str,
    full: &str,
    create: &str,
    update: &str,
    read: &str,
) {
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path},
    };
    let source = MockServer::start().await;
    let prefix = uuid::Uuid::new_v4().simple().to_string();
    let id = format!("import-{prefix}");
    let initial = json!({"resourceType":"Patient", "id":id, "active":true,
        "meta":{"profile":["urn:test:unknown-import-profile"],"tag":[{"system":"urn:test","code":"import-preserved"}]}});
    let anonymous = json!({"resourceType":"Patient", "identifier":[{"system":"urn:test:import","value":prefix}],"active":true});
    Mock::given(method("GET"))
        .and(path("/put"))
        .respond_with(ResponseTemplate::new(200).set_body_string(format!("{initial}\n")))
        .mount(&source)
        .await;
    Mock::given(method("GET"))
        .and(path("/post"))
        .respond_with(ResponseTemplate::new(200).set_body_string(format!("{anonymous}\n")))
        .mount(&source)
        .await;
    for (path, credential, expected_created, expected_errors) in [
        ("put", create, 0, 1), // A supplied id invokes update, never a create scope bypass.
        ("put", update, 1, 0), // u without r/s supports update-as-create.
        ("post", create, 1, 0), // c without r/s supports a server-assigned id.
    ] {
        let params = json!({"resourceType":"Parameters", "parameter":[
            {"name":"input", "part":[{"name":"type","valueString":"Patient"},{"name":"url","valueUrl":format!("{}/{path}",source.uri())}]},
            {"name":"batchSize","valueInteger":2},{"name":"parallelism","valueInteger":2}
        ]});
        let response = client
            .post(format!("{fhir}/$import"))
            .bearer_auth(read)
            .header("Prefer", "respond-async")
            .json(&params)
            .send()
            .await
            .unwrap();
        let status = response.status();
        let body: Value = response.json().await.unwrap();
        assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
        let response = client
            .post(format!("{fhir}/$import"))
            .bearer_auth(credential)
            .header("Prefer", "respond-async")
            .json(&params)
            .send()
            .await
            .unwrap();
        let status = response.status();
        let location = response
            .headers()
            .get("Content-Location")
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();
        let body: Value = response.json().await.unwrap();
        assert_eq!(status, StatusCode::ACCEPTED, "{body}");
        let status_url = format!("{base}{}", url::Url::parse(&location).unwrap().path());
        tokio::time::timeout(std::time::Duration::from_secs(20), async {
            loop {
                let response = client
                    .get(&status_url)
                    .bearer_auth(credential)
                    .send()
                    .await
                    .unwrap();
                let status = response.status();
                let body: Value = response.json().await.unwrap();
                if status == StatusCode::OK {
                    assert_eq!(body["status"], "completed", "{body}");
                    break;
                }
                assert_eq!(status, StatusCode::ACCEPTED, "{body}");
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
        })
        .await
        .unwrap();
        let result: Value = client
            .get(format!("{status_url}/result"))
            .bearer_auth(credential)
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(result["total_created"], expected_created, "{result}");
        assert_eq!(result["total_errors"], expected_errors, "{result}");
        if path == "put" && expected_created == 0 {
            assert_eq!(
                client
                    .get(format!("{fhir}/Patient/{id}"))
                    .bearer_auth(full)
                    .send()
                    .await
                    .unwrap()
                    .status(),
                StatusCode::NOT_FOUND
            );
        }
    }
    let saved: Value = client
        .get(format!("{fhir}/Patient/{id}"))
        .bearer_auth(full)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(saved["meta"]["profile"], initial["meta"]["profile"]);
    assert_eq!(saved["meta"]["tag"], initial["meta"]["tag"]);
    assert!(saved["meta"]["versionId"].as_str().is_some());
    assert!(saved["meta"]["lastUpdated"].as_str().is_some());
    let history: Value = client
        .get(format!("{fhir}/Patient/{id}/_history?_total=accurate"))
        .bearer_auth(full)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(history["total"], 1, "{history}");
    assert_eq!(history["entry"][0]["resource"], saved);
    let created: Value = client
        .get(format!("{fhir}/Patient"))
        .query(&[
            ("identifier", format!("urn:test:import|{prefix}")),
            ("_total", "accurate".into()),
        ])
        .bearer_auth(full)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(created["total"], 1, "{created}");
    let created_id = created["entry"][0]["resource"]["id"].as_str().unwrap();
    for id in [id.as_str(), created_id] {
        assert_eq!(
            client
                .get(format!("{fhir}/Patient/{id}"))
                .bearer_auth(create)
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::FORBIDDEN
        );
    }
}

async fn sof_source_grants_and_snapshot_policies(
    client: &Client,
    base: &str,
    fhir: &str,
    full: &str,
    read: &str,
    patient: &str,
) {
    use base64::{Engine, engine::general_purpose::STANDARD};
    let credential = token(base, &format!("system/Patient.s?_id={patient}")).await;
    let params = json!({"resourceType":"Parameters", "parameter":[
        {"name":"viewDefinition", "resource":{"resourceType":"ViewDefinition", "name":"scoped_view",
            "status":"active", "resource":"Patient", "select":[{"column":[
                {"name":"id", "path":"id"}, {"name":"active", "path":"active"}
            ]}]}}
    ]});
    for operation in ["run", "export"] {
        let response = client
            .post(format!("{fhir}/ViewDefinition/${operation}"))
            .bearer_auth(read)
            .json(&params)
            .send()
            .await
            .unwrap();
        let status = response.status();
        let body: Value = response.json().await.unwrap();
        assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    }
    let run = format!("{fhir}/ViewDefinition/$run");
    let response = client
        .post(&run)
        .bearer_auth(&credential)
        .json(&params)
        .send()
        .await
        .unwrap();
    let status = response.status();
    let result: Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::OK, "{result}");
    let binary = &result["parameter"]
        .as_array()
        .unwrap()
        .iter()
        .find(|param| param["name"] == "rows")
        .unwrap()["resource"];
    assert_eq!(binary["resourceType"], "Binary");
    let rows: Vec<Value> =
        serde_json::from_slice(&STANDARD.decode(binary["data"].as_str().unwrap()).unwrap())
            .unwrap();
    assert_eq!(rows, vec![json!({"id":patient, "active":true})]);

    let response = client
        .post(format!("{fhir}/ViewDefinition/$export?_format=ndjson"))
        .bearer_auth(&credential)
        .header("Prefer", "respond-async")
        .json(&params)
        .send()
        .await
        .unwrap();
    let status = response.status();
    let location = response
        .headers()
        .get("Content-Location")
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    let body: Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let location = url::Url::parse(&location).unwrap();
    let status_url = format!("{base}{}", location.path());
    assert_eq!(
        client
            .get(&status_url)
            .bearer_auth(read)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
    let manifest = tokio::time::timeout(std::time::Duration::from_secs(20), async {
        loop {
            let response = client
                .get(&status_url)
                .bearer_auth(&credential)
                .send()
                .await
                .unwrap();
            let status = response.status();
            let body: Value = response.json().await.unwrap();
            if status == StatusCode::OK {
                break body;
            }
            assert_eq!(status, StatusCode::ACCEPTED, "{body}");
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        manifest["output"].as_array().unwrap().len(),
        1,
        "{manifest}"
    );
    assert_eq!(manifest["output"][0]["count"], 1);
    assert!(
        manifest["request"]
            .as_str()
            .unwrap()
            .ends_with("ViewDefinition/$export?_format=ndjson")
    );
    let file = url::Url::parse(manifest["output"][0]["url"].as_str().unwrap()).unwrap();
    let response = client
        .get(format!("{base}{}", file.path()))
        .bearer_auth(&credential)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["Content-Type"], "application/x-ndjson");
    let exported: Vec<Value> = response
        .text()
        .await
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(exported, rows);

    let script = format!(
        "if (resource && resource.resource.id === '{patient}') return deny('protected view source'); return abstain();"
    );
    let response = client.post(format!("{base}/AccessPolicy")).bearer_auth(full)
        .json(&json!({"resourceType":"AccessPolicy", "name":"Protect view source", "active":true, "priority":0,
            "matcher":{"resourceTypes":["Patient"],"operations":["search"]},
            "engine":{"type":"quickjs", "script":script}})).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let policy: Value = response.json().await.unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let response = client
                .post(&run)
                .bearer_auth(&credential)
                .json(&params)
                .send()
                .await
                .unwrap();
            let status = response.status();
            let body: Value = response.json().await.unwrap();
            assert_eq!(status, StatusCode::OK, "{body}");
            if body["parameter"]
                .as_array()
                .unwrap()
                .iter()
                .find(|param| param["name"] == "rowCount")
                .unwrap()["valueInteger"]
                == 0
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("SOF must apply policy to the actual source snapshot");
    assert_eq!(
        client
            .delete(format!(
                "{base}/AccessPolicy/{}",
                policy["id"].as_str().unwrap()
            ))
            .bearer_auth(full)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NO_CONTENT
    );
}

async fn bulk_export_membership_regression(client: &Client, base: &str, fhir: &str, full: &str) {
    let prefix = uuid::Uuid::new_v4().simple().to_string();
    let patient_a = format!("bulk-a-{prefix}");
    let patient_b = format!("bulk-b-{prefix}");
    let observation_a = format!("bulk-oa-{prefix}");
    let observation_b = format!("bulk-ob-{prefix}");
    let orphan = format!("bulk-orphan-{prefix}");
    let nested = format!("bulk-nested-{prefix}");
    let root = format!("bulk-root-{prefix}");
    let mut created = Vec::new();
    let mut snapshots = std::collections::BTreeMap::new();
    for resource in [
        json!({"resourceType":"Patient","id":patient_a}),
        json!({"resourceType":"Patient","id":patient_b}),
        json!({"resourceType":"Observation","id":observation_a,"status":"final","code":{"text":"A"},"subject":{"reference":format!("Patient/{patient_a}")}}),
        json!({"resourceType":"Observation","id":observation_b,"status":"final","code":{"text":"B"},"subject":{"reference":format!("Patient/{patient_b}")}}),
        json!({"resourceType":"Observation","id":orphan,"status":"final","code":{"text":"orphan"}}),
        json!({"resourceType":"Group","id":nested,"type":"person","actual":true,"member":[{"entity":{"reference":format!("Patient/{patient_a}")}}]}),
        json!({"resourceType":"Group","id":root,"type":"person","actual":true,"member":[{"entity":{"reference":format!("Group/{nested}")}},{"entity":{"reference":format!("Patient/{patient_b}")},"inactive":true}]}),
    ] {
        let path = format!(
            "{}/{}",
            resource["resourceType"].as_str().unwrap(),
            resource["id"].as_str().unwrap()
        );
        let response = client
            .put(format!("{fhir}/{path}"))
            .bearer_auth(full)
            .json(&resource)
            .send()
            .await
            .unwrap();
        let status = response.status();
        let saved: Value = response.json().await.unwrap();
        assert_eq!(status, StatusCode::CREATED, "{saved}");
        snapshots.insert(saved["id"].as_str().unwrap().to_owned(), saved);
        created.push(path);
    }
    // Nested groups may form cycles; identity deduplication must terminate
    // traversal while retaining the explicit Patient member.
    let response = client
        .put(format!("{fhir}/Group/{nested}"))
        .bearer_auth(full)
        .json(
            &json!({"resourceType":"Group","id":nested,"type":"person","actual":true,"member":[
                {"entity":{"reference":format!("Patient/{patient_a}")}},
                {"entity":{"reference":format!("Group/{root}")}}
            ]}),
        )
        .send()
        .await
        .unwrap();
    let status = response.status();
    let body: Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::OK, "{body}");
    let response = client
        .get(format!("{fhir}/Group/{root}/$everything"))
        .bearer_auth(full)
        .query(&[("_type", "Observation")])
        .send()
        .await
        .unwrap();
    let status = response.status();
    let body: Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["entry"].as_array().unwrap().len(), 1, "{body}");
    assert_eq!(body["entry"][0]["resource"], snapshots[&observation_a]);
    let metadata: Value = client
        .get(format!("{fhir}/metadata"))
        .bearer_auth(full)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    for (resource_type, canonical) in [
        (
            "Patient",
            "http://hl7.org/fhir/uv/bulkdata/OperationDefinition/patient-export",
        ),
        (
            "Group",
            "http://hl7.org/fhir/uv/bulkdata/OperationDefinition/group-export",
        ),
    ] {
        let resource = metadata["rest"][0]["resource"]
            .as_array()
            .unwrap()
            .iter()
            .find(|resource| resource["type"] == resource_type)
            .unwrap();
        let operation = resource["operation"]
            .as_array()
            .unwrap()
            .iter()
            .find(|operation| operation["name"] == "export")
            .unwrap();
        assert_eq!(operation["definition"], canonical, "{metadata}");
    }
    let export = token(base, "system/Group.r system/Observation.s").await;
    // Each filter is an alternative, and the encoded comma inside a query
    // remains a FHIR search OR. There is no Patient or Observation read grant.
    let filters = [
        format!("Observation?_id={observation_a}"),
        format!("Observation?_id={observation_b},{orphan}"),
    ];
    for (path, expected) in [
        (
            format!("Group/{root}/$export"),
            vec![observation_a.as_str()],
        ),
        (
            "Patient/$export".into(),
            vec![observation_a.as_str(), observation_b.as_str()],
        ),
        (
            "$export".into(),
            vec![
                observation_a.as_str(),
                observation_b.as_str(),
                orphan.as_str(),
            ],
        ),
    ] {
        let request = if path == "Patient/$export" {
            client.post(format!("{fhir}/{path}")).json(
                &json!({"resourceType":"Parameters","parameter":[
                    {"name":"_type","valueString":"Observation"},
                    {"name":"_typeFilter","valueString":filters[0]},
                    {"name":"_typeFilter","valueString":filters[1]}
                ]}),
            )
        } else {
            client.get(format!("{fhir}/{path}")).query(&[
                ("_type", "Observation"),
                ("_typeFilter", filters[0].as_str()),
                ("_typeFilter", filters[1].as_str()),
            ])
        };
        let response = request
            .bearer_auth(&export)
            .header("Prefer", "respond-async")
            .send()
            .await
            .unwrap();
        let status = response.status();
        let location = response
            .headers()
            .get("Content-Location")
            .map(|value| value.to_str().unwrap().to_owned());
        let body: Value = response.json().await.unwrap();
        assert_eq!(status, StatusCode::ACCEPTED, "{path}: {body}");
        let location = url::Url::parse(&location.unwrap()).unwrap();
        let status_url = format!("{base}{}", location.path());
        let mut manifest = None;
        for _ in 0..400 {
            let response = client
                .get(&status_url)
                .bearer_auth(&export)
                .send()
                .await
                .unwrap();
            let status = response.status();
            let body: Value = response.json().await.unwrap();
            if status == StatusCode::OK {
                manifest = Some(body);
                break;
            }
            assert_eq!(status, StatusCode::ACCEPTED, "{path}: {body}");
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        let manifest = manifest.expect("Bulk export did not complete");
        let original = url::Url::parse(manifest["request"].as_str().unwrap()).unwrap();
        assert_eq!(original.path(), format!("/fhir/{path}"));
        if path != "Patient/$export" {
            assert_eq!(
                original
                    .query_pairs()
                    .filter(|(name, _)| name == "_typeFilter")
                    .count(),
                2
            );
        }
        assert_eq!(manifest["requiresAccessToken"], true);
        assert_eq!(manifest["error"], json!([]));
        let files = manifest["output"].as_array().unwrap();
        assert_eq!(files.len(), expected.len(), "{path}: {manifest}");
        let mut actual = std::collections::BTreeSet::new();
        for file in files {
            assert_eq!(file["type"], "Observation");
            assert_eq!(file["count"], 1);
            let url = url::Url::parse(file["url"].as_str().unwrap()).unwrap();
            let response = client
                .get(format!("{base}{}", url.path()))
                .bearer_auth(&export)
                .send()
                .await
                .unwrap();
            let status = response.status();
            let body = response.text().await.unwrap();
            assert_eq!(status, StatusCode::OK, "{body}");
            let resources: Vec<Value> = body
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
            assert_eq!(resources.len(), 1);
            let id = resources[0]["id"].as_str().unwrap();
            assert_eq!(resources[0], snapshots[id]);
            actual.insert(id.to_owned());
        }
        assert_eq!(actual, expected.into_iter().map(str::to_owned).collect());
        assert_eq!(
            client
                .delete(&status_url)
                .bearer_auth(&export)
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::ACCEPTED
        );
        assert_eq!(
            client
                .get(&status_url)
                .bearer_auth(&export)
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::NOT_FOUND
        );
    }
    for path in created.into_iter().rev() {
        assert_eq!(
            client
                .delete(format!("{fhir}/{path}"))
                .bearer_auth(full)
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::NO_CONTENT
        );
    }
}

async fn async_bundle_ownership_regression(
    client: &Client,
    fhir: &str,
    full: &str,
    create: &str,
    read: &str,
    anonymous_access: bool,
) {
    let response = client.post(fhir).bearer_auth(create).header("Prefer", "respond-async")
        .json(&json!({"resourceType":"Bundle","type":"transaction","entry":[
            {"request":{"method":"POST","url":"Patient"},"resource":{"resourceType":"Patient","active":true,"identifier":[{"system":"urn:async-probe","value":"write-only"}]}}
        ]})).send().await.unwrap();
    let status = response.status();
    let location = response
        .headers()
        .get("Content-Location")
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    let body: Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let location = url::Url::parse(&location).unwrap();
    assert!(
        location.path().starts_with("/fhir/_async-status/"),
        "{location}"
    );
    let id = location.path_segments().unwrap().next_back().unwrap();
    let status_url = format!("{fhir}/_async-status/{id}");
    for credential in [None, Some(read)] {
        let request = client.get(&status_url);
        let response = if let Some(credential) = credential {
            request.bearer_auth(credential)
        } else {
            request
        }
        .send()
        .await
        .unwrap();
        assert_eq!(
            response.status(),
            if credential.is_some() || anonymous_access {
                StatusCode::NOT_FOUND
            } else {
                StatusCode::UNAUTHORIZED
            }
        );
    }
    let mut completed = false;
    for _ in 0..200 {
        let response = client
            .get(&status_url)
            .bearer_auth(create)
            .send()
            .await
            .unwrap();
        let status = response.status();
        let body: Value = response.json().await.unwrap();
        if status == StatusCode::OK {
            assert_eq!(body["status"], "completed", "{body}");
            completed = true;
            break;
        }
        assert_eq!(status, StatusCode::ACCEPTED, "{body}");
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert!(completed, "Async Bundle did not complete");
    let response = client
        .get(format!("{status_url}/result"))
        .bearer_auth(create)
        .send()
        .await
        .unwrap();
    let status = response.status();
    let bundle: Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::OK, "{bundle}");
    assert_eq!(bundle["type"], "transaction-response", "{bundle}");
    assert_eq!(
        bundle["entry"][0]["response"]["status"], "201 Created",
        "{bundle}"
    );
    let saved = &bundle["entry"][0]["resource"];
    let id = saved["id"].as_str().unwrap();
    let actual: Value = client
        .get(format!("{fhir}/Patient/{id}"))
        .bearer_auth(full)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(actual, *saved);
    assert_eq!(
        client
            .get(format!("{status_url}/result"))
            .bearer_auth(read)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        client
            .delete(&status_url)
            .bearer_auth(read)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        client
            .delete(&status_url)
            .bearer_auth(create)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        client
            .get(&status_url)
            .bearer_auth(create)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        client
            .delete(format!("{fhir}/Patient/{id}"))
            .bearer_auth(full)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NO_CONTENT
    );
}

async fn async_bundle_cancel_regression(
    client: &Client,
    fhir: &str,
    full: &str,
    update: &str,
    database: &str,
) {
    use sqlx_core::{query::query, query_scalar::query_scalar};
    let id = "async-cancellation-probe";
    let response = client
        .put(format!("{fhir}/Patient/{id}"))
        .bearer_auth(full)
        .json(&json!({"resourceType":"Patient","id":id,"active":true}))
        .send()
        .await
        .unwrap();
    let status = response.status();
    let original: Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::CREATED, "{original}");
    let history: Value = client
        .get(format!("{fhir}/Patient/{id}/_history"))
        .bearer_auth(full)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let pool = sqlx_postgres::PgPoolOptions::new()
        .max_connections(3)
        .connect(database)
        .await
        .unwrap();
    let mut blocker = pool.begin().await.unwrap();
    query("SELECT id FROM patient WHERE id = $1 FOR UPDATE")
        .bind(id)
        .execute(&mut *blocker)
        .await
        .unwrap();
    let response = client.post(fhir).bearer_auth(update).header("Prefer", "respond-async")
        .json(&json!({"resourceType":"Bundle","type":"transaction","entry":[
            {"request":{"method":"PUT","url":format!("Patient/{id}")},"resource":{"resourceType":"Patient","id":id,"active":false}}
        ]})).send().await.unwrap();
    let status = response.status();
    let location = response
        .headers()
        .get("Content-Location")
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    let body: Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let location = url::Url::parse(&location).unwrap();
    let job_id = location.path_segments().unwrap().next_back().unwrap();
    let status_url = format!("{fhir}/_async-status/{job_id}");
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let blocked: bool = query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE wait_event_type='Lock' AND query LIKE '%id=ANY%FOR UPDATE%')")
                .fetch_one(&pool).await.unwrap();
            if blocked { break; }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }).await.expect("Async Bundle must wait on its native previous-row lock");
    assert_eq!(
        client
            .delete(&status_url)
            .bearer_auth(update)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NO_CONTENT
    );
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
    blocker.rollback().await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let actual: Value = client
        .get(format!("{fhir}/Patient/{id}"))
        .bearer_auth(full)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let actual_history: Value = client
        .get(format!("{fhir}/Patient/{id}/_history"))
        .bearer_auth(full)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(actual, original);
    assert_eq!(actual_history, history);
    assert_eq!(
        client
            .get(&status_url)
            .bearer_auth(update)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        client
            .delete(format!("{fhir}/Patient/{id}"))
            .bearer_auth(full)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NO_CONTENT
    );
    pool.close().await;
}

async fn grouped_everything_regression(
    client: &Client,
    fhir: &str,
    full: &str,
    credential: &str,
    patients: &[&str],
    lab_id: &str,
) {
    let response = client.put(format!("{fhir}/Group/everything-group-probe")).bearer_auth(full)
        .json(&json!({"resourceType":"Group","id":"everything-group-probe","type":"person","actual":true,
            "member":patients.iter().map(|id| json!({"entity":{"reference":format!("Patient/{id}")}})).collect::<Vec<_>>() }))
        .send().await.unwrap();
    let status = response.status();
    let group: Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::CREATED, "{group}");
    let response = client
        .put(format!("{fhir}/Group/everything-group-probe"))
        .bearer_auth(full)
        .json(&group)
        .send()
        .await
        .unwrap();
    let status = response.status();
    let updated_group: Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::OK, "{updated_group}");
    assert_ne!(
        updated_group["meta"]["versionId"],
        group["meta"]["versionId"]
    );
    let response = client.get(format!("{fhir}/Group/everything-group-probe/$everything?_type=Group,Patient,Observation&_count=100"))
        .bearer_auth(credential).send().await.unwrap();
    let status = response.status();
    let body: Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["total"], 3, "{body}");
    let entries = body["entry"].as_array().unwrap();
    assert_eq!(
        entries
            .iter()
            .find(|entry| entry["resource"]["resourceType"] == "Group")
            .unwrap()["resource"],
        updated_group
    );
    assert!(
        entries
            .iter()
            .any(|entry| entry["resource"]["id"] == patients[0])
    );
    assert!(
        entries
            .iter()
            .any(|entry| entry["resource"]["id"] == lab_id)
    );
    assert!(
        !entries
            .iter()
            .any(|entry| entry["resource"]["id"] == patients[1])
    );
    assert_eq!(
        client
            .delete(format!("{fhir}/Group/everything-group-probe"))
            .bearer_auth(full)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NO_CONTENT
    );
    let response = client
        .get(format!("{fhir}/Group/everything-group-probe/_history"))
        .bearer_auth(full)
        .send()
        .await
        .unwrap();
    let status = response.status();
    let history: Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::OK, "{history}");
    let entries = history["entry"].as_array().unwrap();
    assert_eq!(entries.len(), 3, "{history}");
    assert!(entries.iter().any(|entry| entry["resource"] == group));
    assert!(
        entries
            .iter()
            .any(|entry| entry["resource"] == updated_group)
    );
    assert!(
        entries
            .iter()
            .any(|entry| entry["request"]["method"] == "DELETE" && entry.get("resource").is_none())
    );
}

async fn everything_care_and_supporting_regression(
    client: &Client,
    fhir: &str,
    full: &str,
    credential: &str,
    patient: &str,
) {
    let practitioner = json!({"resourceType":"Practitioner","id":"care-date-practitioner","active":true,"name":[{"family":"Care"}]});
    let snapshots = [
        practitioner,
        json!({"resourceType":"Observation","id":"care-date-old","status":"final","code":{"coding":[{"code":"care-date-probe"}]},"subject":{"reference":format!("Patient/{patient}")},"effectiveDateTime":"2024-01-01","issued":"2025-02-28T00:00:00Z"}),
        json!({"resourceType":"Observation","id":"care-date-current","status":"final","code":{"coding":[{"code":"care-date-probe"}]},"subject":{"reference":format!("Patient/{patient}")},"effectivePeriod":{"start":"2025-01-01","end":"2025-02-28"},"performer":[{"reference":"Practitioner/care-date-practitioner"}]}),
    ];
    let mut saved = Vec::new();
    for snapshot in &snapshots {
        let response = client
            .put(format!(
                "{fhir}/{}/{}",
                snapshot["resourceType"].as_str().unwrap(),
                snapshot["id"].as_str().unwrap()
            ))
            .bearer_auth(full)
            .json(snapshot)
            .send()
            .await
            .unwrap();
        let status = response.status();
        let body: Value = response.json().await.unwrap();
        assert_eq!(status, StatusCode::CREATED, "{body}");
        saved.push(body);
    }
    for method in ["GET", "POST"] {
        let path = format!(
            "{fhir}/Patient/{patient}/$everything?_type=Patient,Observation,Practitioner&start=2025-02-28&end=2025-02-28&_count=100"
        );
        let request = if method == "GET" {
            client.get(&path)
        } else {
            client
                .post(&path)
                .json(&json!({"resourceType":"Parameters","parameter":[]}))
        };
        let response = request.bearer_auth(credential).send().await.unwrap();
        let status = response.status();
        let body: Value = response.json().await.unwrap();
        assert_eq!(status, StatusCode::OK, "{method}: {body}");
        assert_eq!(body["total"], 3, "{body}");
        let entries = body["entry"].as_array().unwrap();
        assert!(
            !entries
                .iter()
                .any(|entry| entry["resource"]["id"] == "care-date-old"),
            "{body}"
        );
        for expected in [&saved[0], &saved[2]] {
            assert!(
                entries.iter().any(|entry| &entry["resource"] == expected),
                "{body}"
            );
        }
    }
    let response = client.get(format!("{fhir}/Patient/{patient}/$everything?_type=Practitioner&start=2025-02-28&end=2025-02-28&_elements=name&_count=100")).bearer_auth(credential).send().await.unwrap();
    let status = response.status();
    let body: Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["total"], 1, "{body}");
    let projected = &body["entry"][0]["resource"];
    assert_eq!(projected["id"], "care-date-practitioner");
    assert_eq!(projected["name"], saved[0]["name"]);
    assert!(projected.get("active").is_none(), "{projected}");
    assert!(
        projected["meta"]["tag"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tag| tag["code"] == "SUBSETTED")
    );
    // Graph paths are FHIRPath expressions, and * collects Reference values
    // without mistaking URI/string/canonical fields for resource references.
    for path in ["*", "Observation.performer.where(reference.exists())"] {
        let graph = json!({"resourceType":"GraphDefinition","start":"Observation",
            "link":[{"path":path,"min":1,"max":"1","target":[{"type":"Practitioner"}]}]});
        let response = client.post(format!("{fhir}/Observation/care-date-current/$graph"))
            .bearer_auth(full)
            .json(&json!({"resourceType":"Parameters","parameter":[{"name":"graph","resource":graph}]}))
            .send().await.unwrap();
        let status = response.status();
        let body: Value = response.json().await.unwrap();
        assert_eq!(status, StatusCode::OK, "{path}: {body}");
        let entries = body["entry"].as_array().unwrap();
        assert_eq!(entries.len(), 2, "{body}");
        for expected in [&saved[0], &saved[2]] {
            assert!(
                entries.iter().any(|entry| &entry["resource"] == expected),
                "{body}"
            );
        }
    }
    let base = fhir.strip_suffix("/fhir").unwrap();
    let response = client.post(format!("{base}/AccessPolicy")).bearer_auth(full)
        .json(&json!({"resourceType":"AccessPolicy","name":"Everything snapshot selection","active":true,"priority":0,
            "matcher":{"resourceTypes":["Observation"],"operations":["search"]},
            "engine":{"type":"quickjs","script":"if (resource && resource.resource.id === 'care-date-current') return deny('protected record'); return abstain();"}}))
        .send().await.unwrap();
    let status = response.status();
    let policy: Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::CREATED, "{policy}");
    let query = format!(
        "{fhir}/Patient/{patient}/$everything?_type=Patient,Observation,Practitioner&start=2025-02-28&end=2025-02-28&_count=100"
    );
    let mut loaded = false;
    for _ in 0..100 {
        let response = client
            .get(&query)
            .bearer_auth(credential)
            .send()
            .await
            .unwrap();
        let status = response.status();
        let body: Value = response.json().await.unwrap();
        assert_eq!(status, StatusCode::OK, "{body}");
        if body["total"] == 1 {
            loaded = true;
            assert_eq!(
                body["entry"][0]["resource"]["resourceType"], "Patient",
                "{body}"
            );
            break;
        }
        assert_eq!(body["total"], 3, "{body}");
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert!(loaded, "Snapshot policy did not reload");
    let response = client.get(format!("{fhir}/Patient/{patient}/$everything?_type=Practitioner&start=2025-02-28&end=2025-02-28&_count=100")).bearer_auth(credential).send().await.unwrap();
    let status = response.status();
    let body: Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["total"], 0,
        "Supporting references may expand only visible records: {body}"
    );
    assert_eq!(
        client
            .delete(format!(
                "{base}/AccessPolicy/{}",
                policy["id"].as_str().unwrap()
            ))
            .bearer_auth(full)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NO_CONTENT
    );
    for snapshot in snapshots {
        assert_eq!(
            client
                .delete(format!(
                    "{fhir}/{}/{}",
                    snapshot["resourceType"].as_str().unwrap(),
                    snapshot["id"].as_str().unwrap()
                ))
                .bearer_auth(full)
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::NO_CONTENT
        );
    }
}

async fn linked_compartment_grants(
    client: &Client,
    fhir: &str,
    full: &str,
    grants: (&str, &str),
    patient: &str,
) {
    for (id, target) in [
        ("linked-compartment-source", patient),
        ("linked-compartment-ancestor", "linked-compartment-source"),
    ] {
        let response = client.put(format!("{fhir}/Patient/{id}")).bearer_auth(full)
            .json(&json!({"resourceType":"Patient","id":id,"link":[{"other":{"reference":format!("Patient/{target}")},"type":"replaced-by"}]}))
            .send().await.unwrap();
        let status = response.status();
        let body: Value = response.json().await.unwrap();
        assert_eq!(status, StatusCode::CREATED, "{body}");
    }
    for (index, target) in [
        patient,
        "linked-compartment-source",
        "linked-compartment-ancestor",
    ]
    .into_iter()
    .enumerate()
    {
        let id = format!("linked-compartment-observation-{index}");
        let response = client.put(format!("{fhir}/Observation/{id}")).bearer_auth(full)
            .json(&json!({"resourceType":"Observation","id":id,"status":"final","code":{"coding":[{"code":"linked-compartment-probe"}]},"subject":{"reference":format!("Patient/{target}")}}))
            .send().await.unwrap();
        let status = response.status();
        let body: Value = response.json().await.unwrap();
        assert_eq!(status, StatusCode::CREATED, "{body}");
    }
    for (grant, total) in [(grants.0, 1), (grants.1, 3)] {
        let path = format!("Patient/{patient}/Observation?code=linked-compartment-probe&_count=0");
        for kind in ["direct", "batch", "transaction"] {
            let response = if kind == "direct" {
                client
                    .get(format!("{fhir}/{path}"))
                    .bearer_auth(grant)
                    .send()
                    .await
                    .unwrap()
            } else {
                client.post(fhir).bearer_auth(grant)
                    .json(&json!({"resourceType":"Bundle","type":kind,"entry":[{"request":{"method":"GET","url":path}}]}))
                    .send().await.unwrap()
            };
            let status = response.status();
            let body: Value = response.json().await.unwrap();
            assert_eq!(status, StatusCode::OK, "{kind}: {body}");
            let result = if kind == "direct" {
                &body
            } else {
                assert_eq!(body["entry"][0]["response"]["status"], "200 OK", "{body}");
                &body["entry"][0]["resource"]
            };
            assert_eq!(result["total"], total, "{kind}: {body}");
        }
    }
    for index in 0..3 {
        assert_eq!(
            client
                .delete(format!(
                    "{fhir}/Observation/linked-compartment-observation-{index}"
                ))
                .bearer_auth(full)
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::NO_CONTENT
        );
    }
    for id in ["linked-compartment-ancestor", "linked-compartment-source"] {
        assert_eq!(
            client
                .delete(format!("{fhir}/Patient/{id}"))
                .bearer_auth(full)
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::NO_CONTENT
        );
    }
}

async fn conditional_create_regressions(
    client: &Client,
    fhir: &str,
    full: &str,
    patient_create: &str,
    observation_create: &str,
    patient_id: &str,
    other_patient_id: &str,
) {
    Box::pin(conditional_create_cardinality(
        client,
        fhir,
        full,
        patient_create,
    ))
    .await;
    Box::pin(conditional_create_compartment(
        client,
        fhir,
        full,
        observation_create,
        patient_id,
        other_patient_id,
    ))
    .await;
    Box::pin(conditional_create_races(
        client,
        fhir,
        full,
        observation_create,
        patient_id,
    ))
    .await;
    Box::pin(conditional_create_policies(
        client,
        fhir,
        full,
        patient_create,
    ))
    .await;
}

async fn conditional_create_cardinality(
    client: &Client,
    fhir: &str,
    full: &str,
    patient_create: &str,
) {
    let patients = format!("{fhir}/Patient");
    let condition = "identifier=urn:conditional%7Cpatient";
    let content = json!({"resourceType":"Patient","active":true,
        "identifier":[{"system":"urn:conditional","value":"patient"}]});
    let response = client
        .post(&patients)
        .bearer_auth(patient_create)
        .header("If-None-Exist", condition)
        .json(&content)
        .send()
        .await
        .unwrap();
    let status = response.status();
    assert!(response.headers().contains_key("Location"));
    let original: Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::CREATED, "{original}");
    let url = format!("{patients}/{}", original["id"].as_str().unwrap());
    let history_before: Value = client
        .get(format!("{url}/_history"))
        .bearer_auth(full)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    // Even invalid schema and nonexistent references in ignored content cannot
    // turn a successful match into a validation error.
    for preference in [
        "return=representation",
        "return=minimal",
        "return=OperationOutcome",
    ] {
        let response = client
            .post(&patients)
            .bearer_auth(patient_create)
            .header(
                "If-None-Exist",
                format!("{condition}&_count=0&_offset=100&_summary=count"),
            )
            .header("Prefer", preference)
            .json(
                &json!({"resourceType":"Patient","active":false,"birthDate":42,
                "managingOrganization":{"reference":"Organization/conditional-missing"}}),
            )
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{preference}");
        assert!(response.headers().contains_key("Content-Location"));
        assert_eq!(
            response.headers().get("ETag").unwrap().to_str().unwrap(),
            format!("W/\"{}\"", original["meta"]["versionId"].as_str().unwrap())
        );
        if preference == "return=minimal" {
            assert!(response.bytes().await.unwrap().is_empty());
        } else {
            let result: Value = response.json().await.unwrap();
            if preference == "return=representation" {
                assert_eq!(result, original);
            } else {
                assert_eq!(result["resourceType"], "OperationOutcome");
            }
        }
    }
    let history_after: Value = client
        .get(format!("{url}/_history"))
        .bearer_auth(full)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(history_after, history_before);
    // Repeated headers must be rejected instead of silently selecting one.
    let response = client
        .post(&patients)
        .bearer_auth(patient_create)
        .header("If-None-Exist", condition)
        .header("If-None-Exist", "active=false")
        .json(&content)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let response = client
        .post(&patients)
        .bearer_auth(patient_create)
        .header(
            "If-None-Exist",
            reqwest::header::HeaderValue::from_bytes(&[0xff]).unwrap(),
        )
        .json(&content)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let response = client
        .post(&patients)
        .bearer_auth(full)
        .json(&content)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let response = client
        .post(&patients)
        .bearer_auth(patient_create)
        .header("If-None-Exist", condition)
        .json(&content)
        .send()
        .await
        .unwrap();
    let status = response.status();
    let outcome: Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::PRECONDITION_FAILED, "{outcome}");
}

async fn conditional_create_compartment(
    client: &Client,
    fhir: &str,
    full: &str,
    observation_create: &str,
    patient_id: &str,
    other_patient_id: &str,
) {
    let observations = format!("{fhir}/Observation");
    let observation = |identifier: &str, subject: &str| {
        json!({"resourceType":"Observation",
        "status":"final", "code":{"coding":[{"code":"atomic-write"}]},
        "identifier":[{"system":"urn:conditional","value":identifier}],
        "subject":{"reference":format!("Patient/{subject}")}})
    };
    let foreign = observation("compartment", other_patient_id);
    let response = client
        .post(&observations)
        .bearer_auth(full)
        .json(&foreign)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let own = observation("compartment", patient_id);
    let condition = "identifier=urn:conditional%7Ccompartment";
    let response = client
        .post(&observations)
        .bearer_auth(observation_create)
        .header("If-None-Exist", condition)
        .json(&own)
        .send()
        .await
        .unwrap();
    let status = response.status();
    let original: Value = response.json().await.unwrap();
    assert_eq!(
        status,
        StatusCode::CREATED,
        "hidden foreign match must not be returned: {original}"
    );
    let response = client
        .post(&observations)
        .bearer_auth(observation_create)
        .header("If-None-Exist", condition)
        .json(&foreign)
        .send()
        .await
        .unwrap();
    let status = response.status();
    let matched: Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::OK, "{matched}");
    assert_eq!(matched, original);
    for (subject, expected) in [
        (other_patient_id, StatusCode::FORBIDDEN),
        ("conditional-missing", StatusCode::UNPROCESSABLE_ENTITY),
    ] {
        let response = client
            .post(&observations)
            .bearer_auth(observation_create)
            .header("If-None-Exist", "identifier=urn:conditional%7Cdenied")
            .json(&observation("denied", subject))
            .send()
            .await
            .unwrap();
        let status = response.status();
        let outcome: Value = response.json().await.unwrap();
        assert_eq!(status, expected, "{outcome}");
    }
    let rejected: Value = client
        .get(format!(
            "{observations}?identifier=urn:conditional%7Cdenied&_summary=count"
        ))
        .bearer_auth(full)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(rejected["total"], 0);
}

async fn conditional_create_races(
    client: &Client,
    fhir: &str,
    full: &str,
    observation_create: &str,
    patient_id: &str,
) {
    let observations = format!("{fhir}/Observation");
    let observation = |identifier: &str, subject: &str| {
        json!({"resourceType":"Observation",
        "status":"final", "code":{"coding":[{"code":"atomic-write"}]},
        "identifier":[{"system":"urn:conditional","value":identifier}],
        "subject":{"reference":format!("Patient/{subject}")}})
    };
    // Different predicates that overlap must coordinate, including a chained
    // dependency on Patient. Whichever creates first, the other must match it.
    for (identifier, first_condition, second_condition) in [
        (
            "race-same",
            "identifier=urn:conditional%7Crace-same".to_owned(),
            "identifier=urn:conditional%7Crace-same".to_owned(),
        ),
        (
            "race-overlap",
            "identifier=urn:conditional%7Crace-overlap".to_owned(),
            format!("identifier=urn:conditional%7Crace-overlap&subject:Patient._id={patient_id}"),
        ),
    ] {
        let content = observation(identifier, patient_id);
        let first = client
            .post(&observations)
            .bearer_auth(observation_create)
            .header("If-None-Exist", first_condition)
            .json(&content)
            .send();
        let second = client
            .post(&observations)
            .bearer_auth(observation_create)
            .header("If-None-Exist", second_condition)
            .json(&content)
            .send();
        let (first, second) = tokio::join!(first, second);
        let first = first.unwrap();
        let second = second.unwrap();
        let statuses = [first.status(), second.status()];
        let first: Value = first.json().await.unwrap();
        let second: Value = second.json().await.unwrap();
        assert_eq!(
            statuses
                .iter()
                .filter(|&&s| s == StatusCode::CREATED)
                .count(),
            1,
            "{first} {second}"
        );
        assert_eq!(
            statuses.iter().filter(|&&s| s == StatusCode::OK).count(),
            1,
            "{first} {second}"
        );
        assert_eq!(first, second);
        let history: Value = client
            .get(format!(
                "{observations}/{}/_history",
                first["id"].as_str().unwrap()
            ))
            .bearer_auth(full)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(history["entry"].as_array().unwrap().len(), 1);
        let count: Value = client
            .get(format!(
                "{observations}?identifier=urn:conditional%7C{identifier}&_summary=count"
            ))
            .bearer_auth(full)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(count["total"], 1);
    }
}

async fn conditional_create_policies(
    client: &Client,
    fhir: &str,
    full: &str,
    patient_create: &str,
) {
    let patients = format!("{fhir}/Patient");
    let content = json!({"resourceType":"Patient","active":true,
        "identifier":[{"system":"urn:conditional","value":"policy"}]});
    let response = client
        .post(&patients)
        .bearer_auth(full)
        .json(&content)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let original: Value = response.json().await.unwrap();
    let url = format!("{patients}/{}", original["id"].as_str().unwrap());
    let history_before: Value = client
        .get(format!("{url}/_history"))
        .bearer_auth(full)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let base = fhir.strip_suffix("/fhir").unwrap();
    let response = client.post(format!("{base}/AccessPolicy")).bearer_auth(full)
        .json(&json!({"resourceType":"AccessPolicy","name":"Protect conditional create matches","active":true,"priority":0,
            "matcher":{"resourceTypes":["Patient"],"operations":["create"]},
            "engine":{"type":"quickjs","script":"if (resource && resource.resource.identifier && resource.resource.identifier[0].value === 'policy') return deny('protected conditional Patient'); return abstain();"}}))
        .send().await.unwrap();
    let status = response.status();
    let policy: Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::CREATED, "{policy}");
    let mut denied = false;
    for _ in 0..100 {
        let response = client
            .post(&patients)
            .bearer_auth(patient_create)
            .header("If-None-Exist", "identifier=urn:conditional%7Cpolicy")
            .json(&json!({"resourceType":"Patient","birthDate":42}))
            .send()
            .await
            .unwrap();
        let status = response.status();
        let outcome: Value = response.json().await.unwrap();
        if status == StatusCode::FORBIDDEN {
            denied = true;
            break;
        }
        assert_eq!(status, StatusCode::OK, "{outcome}");
        assert_eq!(outcome, original);
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert!(
        denied,
        "matching conditional POST must authorize the actual stored row"
    );
    let transaction_create = token(base, "system/Patient.c").await;
    for (condition, resource) in [
        (
            "identifier=urn:conditional%7Cpolicy",
            json!({"resourceType":"Patient","birthDate":42}),
        ),
        (
            "identifier=urn:conditional%7Cpolicy-no-match",
            content.clone(),
        ),
    ] {
        for credential in [transaction_create.as_str(), patient_create] {
            Box::pin(scoped_transaction(client, fhir, credential, json!([
                {"request":{"method":"POST","url":"Patient","ifNoneExist":condition},"resource":resource}
            ]), StatusCode::FORBIDDEN, false)).await;
        }
    }
    let response = client.post(&patients).bearer_auth(full)
        .json(&json!({"resourceType":"Patient","active":true,"identifier":[{"system":"urn:conditional","value":"policy-allowed"}]}))
        .send().await.unwrap();
    let allowed: Value = response.json().await.unwrap();
    let matched = Box::pin(scoped_transaction(client, fhir, &transaction_create, json!([
        {"request":{"method":"POST","url":"Patient","ifNoneExist":"identifier=urn:conditional%7Cpolicy-allowed"},
         "resource":{"resourceType":"Patient","birthDate":42}}
    ]), StatusCode::OK, false)).await;
    assert_eq!(
        matched["entry"][0]["response"]["status"], "200 OK",
        "{matched}"
    );
    assert_eq!(
        matched["entry"][0]["resource"], allowed,
        "matching POST must ignore the candidate and preserve saved JSON"
    );
    let response = client
        .post(&patients)
        .bearer_auth(patient_create)
        .header(
            "If-None-Exist",
            "identifier=urn:conditional%7Cpolicy-no-match",
        )
        .json(&content)
        .send()
        .await
        .unwrap();
    let status = response.status();
    let outcome: Value = response.json().await.unwrap();
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "resulting conditional create must be checked: {outcome}"
    );
    let count: Value = client
        .get(format!(
            "{patients}?identifier=urn:conditional%7Cpolicy&_summary=count"
        ))
        .bearer_auth(full)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(count["total"], 1, "rejected candidate must roll back");
    let history_after: Value = client
        .get(format!("{url}/_history"))
        .bearer_auth(full)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(history_after, history_before);
    // Ambiguous scoped matches must run their policies before revealing 412.
    let duplicate_id = "conditional-policy-duplicate";
    let mut duplicate = content.clone();
    duplicate["id"] = json!(duplicate_id);
    let response = client
        .put(format!("{patients}/{duplicate_id}"))
        .bearer_auth(full)
        .json(&duplicate)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    for credential in [transaction_create.as_str(), patient_create] {
        Box::pin(scoped_transaction(client, fhir, credential, json!([
            {"request":{"method":"POST","url":"Patient","ifNoneExist":"identifier=urn:conditional%7Cpolicy&_count=1"},
             "resource":{"resourceType":"Patient","birthDate":42}}
        ]), StatusCode::FORBIDDEN, false)).await;
    }
    assert_eq!(
        client
            .delete(format!("{patients}/{duplicate_id}"))
            .bearer_auth(full)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        client
            .delete(format!(
                "{base}/AccessPolicy/{}",
                policy["id"].as_str().unwrap()
            ))
            .bearer_auth(full)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NO_CONTENT
    );
}

async fn checked_validation(
    client: &Client,
    url: &str,
    credential: &str,
    content: &Value,
    valid: bool,
) {
    let response = client
        .post(url)
        .bearer_auth(credential)
        .json(content)
        .send()
        .await
        .unwrap();
    let status = response.status();
    let outcome: Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::OK, "{url}: {outcome}");
    let invalid = outcome["issue"]
        .as_array()
        .unwrap()
        .iter()
        .any(|issue| matches!(issue["severity"].as_str(), Some("error" | "fatal")));
    assert_eq!(!invalid, valid, "{url}: {outcome}");
}

async fn reference_alias_regressions(
    client: &Client,
    fhir: &str,
    configured: &str,
    full: &str,
    create: &str,
    patient: &str,
    other_patient: &str,
) {
    let alternate = configured.replacen("http://", "https://", 1);
    let content = |reference: String| {
        json!({"resourceType":"Observation", "status":"final",
        "code":{"coding":[{"code":"atomic-write"}]}, "subject":{"reference":reference}})
    };
    for reference in [
        format!("Patient/{patient}"),
        format!("{configured}/Patient/{patient}"),
        format!("{alternate}/Patient/{patient}"),
    ] {
        let resource = content(reference);
        let before = count_resources(client, fhir, full, "Observation").await;
        checked_validation(
            client,
            &format!("{fhir}/Observation/$validate?mode=create"),
            create,
            &resource,
            true,
        )
        .await;
        assert_eq!(
            count_resources(client, fhir, full, "Observation").await,
            before
        );
        let response = client
            .post(format!("{fhir}/Observation"))
            .bearer_auth(create)
            .json(&resource)
            .send()
            .await
            .unwrap();
        let status = response.status();
        let stored: Value = response.json().await.unwrap();
        assert_eq!(status, StatusCode::CREATED, "{stored}");
        assert_eq!(stored["subject"], resource["subject"]);
    }
    for reference in [
        format!("{configured}/Patient/reference-absent"),
        format!("{alternate}/Patient/reference-absent"),
        format!("Patient/{patient}/_history/missing.version"),
    ] {
        let resource = content(reference);
        let before = count_resources(client, fhir, full, "Observation").await;
        checked_validation(
            client,
            &format!("{fhir}/Observation/$validate?mode=create"),
            create,
            &resource,
            false,
        )
        .await;
        let response = client
            .post(format!("{fhir}/Observation"))
            .bearer_auth(create)
            .json(&resource)
            .send()
            .await
            .unwrap();
        let status = response.status();
        let outcome: Value = response.json().await.unwrap();
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{outcome}");
        assert_eq!(
            count_resources(client, fhir, full, "Observation").await,
            before
        );
    }
    for reference in [
        format!("Patient/{patient}/extra"),
        format!("Patient/{patient}/_history/1/extra"),
        format!("Patient\"unsafe/{patient}"),
        format!("UnknownResource/{patient}"),
    ] {
        let resource = content(reference);
        checked_validation(
            client,
            &format!("{fhir}/Observation/$validate?mode=create"),
            create,
            &resource,
            false,
        )
        .await;
        let response = client
            .post(format!("{fhir}/Observation"))
            .bearer_auth(create)
            .json(&resource)
            .send()
            .await
            .unwrap();
        let status = response.status();
        let outcome: Value = response.json().await.unwrap();
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{outcome}");
    }
    let response = client
        .post(format!("{fhir}/Observation"))
        .bearer_auth(create)
        .json(&content(format!("{alternate}/Patient/{other_patient}")))
        .send()
        .await
        .unwrap();
    let status = response.status();
    let outcome: Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::FORBIDDEN, "{outcome}");
}

async fn count_resources(client: &Client, fhir: &str, full: &str, resource_type: &str) -> Value {
    let response = client
        .get(format!("{fhir}/{resource_type}?_summary=count"))
        .bearer_auth(full)
        .send()
        .await
        .unwrap();
    let status = response.status();
    let bundle: Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::OK, "{bundle}");
    bundle["total"].clone()
}

async fn reference_version_regressions(
    client: &Client,
    fhir: &str,
    configured: &str,
    full: &str,
    create: &str,
    update: &str,
) {
    let response = client
        .post(format!("{fhir}/Patient"))
        .bearer_auth(full)
        .json(&json!({"resourceType":"Patient", "active":true}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let original: Value = response.json().await.unwrap();
    let patient = original["id"].as_str().unwrap();
    let version = original["meta"]["versionId"].as_str().unwrap();
    let patient_url = format!("{fhir}/Patient/{patient}");
    let response = client
        .put(&patient_url)
        .bearer_auth(full)
        .json(&json!({"resourceType":"Patient", "id":patient, "active":false}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let current: Value = response.json().await.unwrap();
    let current_version = current["meta"]["versionId"].as_str().unwrap();
    let content = |reference: String| {
        json!({"resourceType":"Observation", "status":"final",
        "code":{"coding":[{"code":"reference-version"}]}, "subject":{"reference":reference}})
    };
    for reference in [
        format!("Patient/{patient}/_history/{version}"),
        format!("{configured}/Patient/{patient}/_history/{current_version}"),
    ] {
        let resource = content(reference);
        checked_validation(
            client,
            &format!("{fhir}/Observation/$validate?mode=create"),
            create,
            &resource,
            true,
        )
        .await;
        let response = client
            .post(format!("{fhir}/Observation"))
            .bearer_auth(create)
            .json(&resource)
            .send()
            .await
            .unwrap();
        let status = response.status();
        let stored: Value = response.json().await.unwrap();
        assert_eq!(status, StatusCode::CREATED, "{stored}");
        assert_eq!(stored["subject"], resource["subject"]);
        let url = format!("{fhir}/Observation/{}", stored["id"].as_str().unwrap());
        let before: Value = client
            .get(format!("{url}/_history"))
            .bearer_auth(full)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let mut updated = resource.clone();
        updated["id"] = stored["id"].clone();
        checked_validation(
            client,
            &format!("{url}/$validate?mode=update"),
            update,
            &updated,
            true,
        )
        .await;
        let after: Value = client
            .get(format!("{url}/_history"))
            .bearer_auth(full)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(after["entry"], before["entry"]);
        // The same credential cannot use a read-only validation to fetch targets.
        let response = client
            .post(format!("{fhir}/Observation/$validate"))
            .bearer_auth(create)
            .json(&resource)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
    for wrong in [format!("0{version}"), "unknown.version".into()] {
        let resource = content(format!("Patient/{patient}/_history/{wrong}"));
        checked_validation(
            client,
            &format!("{fhir}/Observation/$validate?mode=create"),
            create,
            &resource,
            false,
        )
        .await;
        let response = client
            .post(format!("{fhir}/Observation"))
            .bearer_auth(create)
            .json(&resource)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let batch = Box::pin(scoped_batch(
            client,
            fhir,
            create,
            json!([
                {"request":{"method":"POST","url":"Observation"},"resource":resource}
            ]),
        ))
        .await;
        assert_eq!(
            batch["entry"][0]["response"]["status"], "422 Unprocessable Entity",
            "{batch}"
        );
        // Valid opaque IDs are unknown versions, not malformed numeric versions.
        let response = client
            .get(format!("{patient_url}/_history/{wrong}"))
            .bearer_auth(full)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
    assert_eq!(
        client
            .delete(&patient_url)
            .bearer_auth(full)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NO_CONTENT
    );
    for (reference, expected) in [
        (
            format!("Patient/{patient}"),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            format!("Patient/{patient}/_history/{version}"),
            StatusCode::CREATED,
        ),
    ] {
        let resource = content(reference);
        checked_validation(
            client,
            &format!("{fhir}/Observation/$validate?mode=create"),
            create,
            &resource,
            expected == StatusCode::CREATED,
        )
        .await;
        let response = client
            .post(format!("{fhir}/Observation"))
            .bearer_auth(create)
            .json(&resource)
            .send()
            .await
            .unwrap();
        let status = response.status();
        let stored: Value = response.json().await.unwrap();
        assert_eq!(status, expected, "{stored}");
        let batch = Box::pin(scoped_batch(
            client,
            fhir,
            create,
            json!([
                {"request":{"method":"POST","url":"Observation"},"resource":resource}
            ]),
        ))
        .await;
        assert_eq!(
            batch["entry"][0]["response"]["status"],
            if expected == StatusCode::CREATED {
                "201 Created"
            } else {
                "422 Unprocessable Entity"
            },
            "{batch}"
        );
    }
    let retained: Value = client
        .get(format!("{patient_url}/_history/{version}"))
        .bearer_auth(full)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(retained, original);

    let self_url = format!("{fhir}/Observation/reference-self");
    let resource = json!({"resourceType":"Observation", "id":"reference-self", "status":"final",
        "code":{"coding":[{"code":"reference-version"}]}, "derivedFrom":[{"reference":"Observation/reference-self"}]});
    checked_validation(
        client,
        &format!("{self_url}/$validate?mode=update"),
        update,
        &resource,
        true,
    )
    .await;
    assert_eq!(
        client
            .get(&self_url)
            .bearer_auth(full)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
    let response = client
        .put(&self_url)
        .bearer_auth(update)
        .json(&resource)
        .send()
        .await
        .unwrap();
    let status = response.status();
    let stored: Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::CREATED, "{stored}");
    assert_eq!(stored["derivedFrom"], resource["derivedFrom"]);
}

async fn reference_profile_regressions(
    client: &Client,
    fhir: &str,
    full: &str,
    create: &str,
    update: &str,
    database: &str,
) {
    use octofhir_fhirschema::embedded::{FhirVersion, get_schemas};
    let patient_profile = "urn:test:reference-patient-birth";
    let observation_profile = "urn:test:reference-profiled-observation";
    // A development validation bypass can store a false profile claim. Such a
    // claim must not make the target count as conforming to a loaded profile.
    let response = client
        .post(format!("{fhir}/Patient"))
        .bearer_auth(full)
        .header("X-Skip-Validation", "true")
        .json(&json!({"resourceType":"Patient", "meta":{"profile":[patient_profile]}}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let invalid: Value = response.json().await.unwrap();
    let response = client
        .post(format!("{fhir}/Patient"))
        .bearer_auth(full)
        .json(&json!({"resourceType":"Patient", "birthDate":"2000-01-01"}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let valid: Value = response.json().await.unwrap();
    let pool = sqlx_postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(database)
        .await
        .unwrap();
    let store = octofhir_db_postgres::PostgresPackageStore::new(pool.clone());
    let schemas = get_schemas(FhirVersion::R4);
    let mut patient = schemas.get("Patient").unwrap().clone();
    patient.name = "ReferencePatientWithBirthDate".into();
    patient.url = patient_profile.into();
    patient.base = Some("http://hl7.org/fhir/StructureDefinition/Patient".into());
    patient.derivation = Some("constraint".into());
    patient
        .required
        .get_or_insert_with(Vec::new)
        .push("birthDate".into());
    patient
        .elements
        .as_mut()
        .unwrap()
        .get_mut("birthDate")
        .unwrap()
        .min = Some(1);
    let mut observation = schemas.get("Observation").unwrap().clone();
    observation.name = "ReferenceProfiledObservation".into();
    observation.url = observation_profile.into();
    observation.base = Some("http://hl7.org/fhir/StructureDefinition/Observation".into());
    observation.derivation = Some("constraint".into());
    observation
        .elements
        .as_mut()
        .unwrap()
        .get_mut("subject")
        .unwrap()
        .refers = Some(vec![patient_profile.into()]);
    for schema in [patient, observation] {
        store
            .store_fhirschema(
                &schema.url,
                None,
                "hl7.fhir.r4.core",
                "4.0.1",
                "R4",
                "resource",
                &serde_json::to_value(&schema).unwrap(),
            )
            .await
            .unwrap();
    }
    let good_id = valid["id"].as_str().unwrap();
    let good_version = valid["meta"]["versionId"].as_str().unwrap();
    let transaction_create =
        token(fhir.strip_suffix("/fhir").unwrap(), "system/Observation.c").await;
    assert_eq!(
        client
            .put(format!("{fhir}/Patient/{good_id}"))
            .bearer_auth(full)
            .json(&json!({"resourceType":"Patient", "id":good_id}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    for (reference, conforms) in [
        (
            format!("Patient/{}", invalid["id"].as_str().unwrap()),
            false,
        ),
        (format!("Patient/{good_id}"), false),
        (format!("Patient/{good_id}/_history/{good_version}"), true),
    ] {
        let resource = json!({"resourceType":"Observation", "meta":{"profile":[observation_profile]},
            "status":"final", "code":{"coding":[{"code":"reference-version"}]},
            "subject":{"reference":reference}});
        let count = count_resources(client, fhir, full, "Observation").await;
        let batch = Box::pin(scoped_batch(
            client,
            fhir,
            create,
            json!([
                {"request":{"method":"POST","url":"Observation"},"resource":resource}
            ]),
        ))
        .await;
        assert_eq!(
            batch["entry"][0]["response"]["status"],
            if conforms {
                "201 Created"
            } else {
                "422 Unprocessable Entity"
            },
            "{batch}"
        );
        let after_batch = count_resources(client, fhir, full, "Observation").await;
        assert_eq!(
            after_batch.as_u64().unwrap(),
            count.as_u64().unwrap() + u64::from(conforms)
        );
        let response = client
            .post(fhir)
            .bearer_auth(&transaction_create)
            .json(
                &json!({"resourceType":"Bundle","type":"transaction","entry":[
                    {"request":{"method":"POST","url":"Observation"},"resource":resource}
                ]}),
            )
            .send()
            .await
            .unwrap();
        let status = response.status();
        let transaction: Value = response.json().await.unwrap();
        assert_eq!(
            status,
            if conforms {
                StatusCode::OK
            } else {
                StatusCode::UNPROCESSABLE_ENTITY
            },
            "{transaction}"
        );
        let count = count_resources(client, fhir, full, "Observation").await;
        assert_eq!(
            count.as_u64().unwrap(),
            after_batch.as_u64().unwrap() + u64::from(conforms)
        );
        checked_validation(
            client,
            &format!("{fhir}/Observation/$validate?mode=create"),
            create,
            &resource,
            conforms,
        )
        .await;
        assert_eq!(
            count_resources(client, fhir, full, "Observation").await,
            count
        );
        let response = client
            .post(format!("{fhir}/Observation"))
            .bearer_auth(create)
            .json(&resource)
            .send()
            .await
            .unwrap();
        let status = response.status();
        let stored: Value = response.json().await.unwrap();
        assert_eq!(
            status,
            if conforms {
                StatusCode::CREATED
            } else {
                StatusCode::UNPROCESSABLE_ENTITY
            },
            "{stored}"
        );
        if conforms {
            let id = stored["id"].as_str().unwrap();
            let mut updated = resource.clone();
            updated["id"] = stored["id"].clone();
            checked_validation(
                client,
                &format!("{fhir}/Observation/{id}/$validate?mode=update"),
                update,
                &updated,
                true,
            )
            .await;
        } else {
            assert!(
                stored["issue"].as_array().unwrap().iter().any(|issue| {
                    issue["code"] == "invalid"
                        && issue["expression"] == json!(["Observation.subject.reference"])
                        && issue["diagnostics"].as_str().is_some_and(|message| {
                            message.contains("does not conform")
                                && message.contains(patient_profile)
                        })
                }),
                "{stored}"
            );
            assert_eq!(
                count_resources(client, fhir, full, "Observation").await,
                count
            );
        }
    }
    Box::pin(transaction_profile_graphs(
        client,
        fhir,
        full,
        patient_profile,
        observation_profile,
    ))
    .await;
    Box::pin(reference_profile_races(
        client,
        fhir,
        full,
        create,
        database,
        observation_profile,
    ))
    .await;
    pool.close().await;
}

async fn transaction_profile_graphs(
    client: &Client,
    fhir: &str,
    full: &str,
    patient_profile: &str,
    observation_profile: &str,
) {
    let create = token(
        fhir.strip_suffix("/fhir").unwrap(),
        "system/Patient.c system/Observation.c",
    )
    .await;
    let observation = |reference: &str| {
        json!({"resourceType":"Observation","meta":{"profile":[observation_profile]},
        "status":"final","code":{"coding":[{"code":"reference-version"}]},"subject":{"reference":reference}})
    };
    let mut patient_id = String::new();
    for conforms in [false, true] {
        let before_patient = count_resources(client, fhir, full, "Patient").await;
        let before_observation = count_resources(client, fhir, full, "Observation").await;
        let mut parent = json!({"resourceType":"Patient"});
        if conforms {
            parent["birthDate"] = json!("2000-01-01");
            parent["meta"] = json!({"profile":[patient_profile]});
        }
        let response = client.post(fhir).bearer_auth(&create).json(&json!({
            "resourceType":"Bundle","type":"transaction","entry":[
                {"request":{"method":"POST","url":"Observation"},"resource":observation("urn:uuid:graph-parent")},
                {"fullUrl":"urn:uuid:graph-parent","request":{"method":"POST","url":"Patient"},"resource":parent}
            ]})).send().await.unwrap();
        let status = response.status();
        let body: Value = response.json().await.unwrap();
        assert_eq!(
            status,
            if conforms {
                StatusCode::OK
            } else {
                StatusCode::UNPROCESSABLE_ENTITY
            },
            "{body}"
        );
        assert_eq!(
            count_resources(client, fhir, full, "Patient")
                .await
                .as_u64()
                .unwrap(),
            before_patient.as_u64().unwrap() + u64::from(conforms)
        );
        assert_eq!(
            count_resources(client, fhir, full, "Observation")
                .await
                .as_u64()
                .unwrap(),
            before_observation.as_u64().unwrap() + u64::from(conforms)
        );
        if conforms {
            patient_id = body["entry"][1]["resource"]["id"]
                .as_str()
                .unwrap()
                .to_owned();
            assert_eq!(
                body["entry"][0]["resource"]["subject"]["reference"],
                format!("Patient/{patient_id}")
            );
        }
    }
    let update = token(
        fhir.strip_suffix("/fhir").unwrap(),
        "system/Patient.u system/Observation.c",
    )
    .await;
    let target = format!("Patient/{patient_id}");
    let previous: Value = client
        .get(format!("{fhir}/{target}"))
        .bearer_auth(full)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let history: Value = client
        .get(format!("{fhir}/{target}/_history"))
        .bearer_auth(full)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    for method in ["PUT", "PATCH"] {
        let candidate = if method == "PUT" {
            json!({"resourceType":"Patient","id":patient_id})
        } else {
            json!({"resourceType":"Parameters","parameter":[
                {"name":"operation","part":[{"name":"type","valueCode":"delete"},{"name":"path","valueString":"Patient.birthDate"}]},
                {"name":"operation","part":[{"name":"type","valueCode":"delete"},{"name":"path","valueString":"Patient.meta.profile"}]}
            ]})
        };
        let before = count_resources(client, fhir, full, "Observation").await;
        let response = client.post(fhir).bearer_auth(&update).json(&json!({"resourceType":"Bundle","type":"transaction","entry":[
            {"request":{"method":method,"url":target},"resource":candidate},
            {"request":{"method":"POST","url":"Observation"},"resource":observation(&target)}
        ]})).send().await.unwrap();
        let status = response.status();
        let body: Value = response.json().await.unwrap();
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{method}: {body}");
        assert_eq!(
            count_resources(client, fhir, full, "Observation").await,
            before
        );
        let after: Value = client
            .get(format!("{fhir}/{target}"))
            .bearer_auth(full)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let after_history: Value = client
            .get(format!("{fhir}/{target}/_history"))
            .bearer_auth(full)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(after, previous);
        assert_eq!(after_history, history);
    }
}

async fn reference_profile_races(
    client: &Client,
    fhir: &str,
    full: &str,
    create: &str,
    database: &str,
    profile: &str,
) {
    use sqlx_core::{query::query, query_scalar::query_scalar};
    let pool = sqlx_postgres::PgPoolOptions::new()
        .max_connections(3)
        .connect(database)
        .await
        .unwrap();
    let transaction_create =
        token(fhir.strip_suffix("/fhir").unwrap(), "system/Observation.c").await;
    for suffix in ["", "/$validate?mode=create", "batch", "transaction"] {
        let response = client
            .post(format!("{fhir}/Patient"))
            .bearer_auth(full)
            .json(&json!({"resourceType":"Patient", "birthDate":"2000-01-01"}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        let patient: Value = response.json().await.unwrap();
        let id = patient["id"].as_str().unwrap();
        let content = json!({"resourceType":"Observation", "meta":{"profile":[profile]}, "status":"final",
            "code":{"coding":[{"code":"reference-version"}]}, "subject":{"reference":format!("Patient/{id}")}});
        let count = count_resources(client, fhir, full, "Observation").await;
        let mut blocker = pool.begin().await.unwrap();
        query("SELECT id FROM patient WHERE id=$1 FOR UPDATE")
            .bind(id)
            .execute(&mut *blocker)
            .await
            .unwrap();
        let request = if matches!(suffix, "batch" | "transaction") {
            client
                .post(fhir)
                .bearer_auth(if suffix == "transaction" {
                    &transaction_create
                } else {
                    create
                })
                .json(&json!({"resourceType":"Bundle", "type":suffix, "entry":[
                    {"request":{"method":"POST","url":"Observation"},"resource":content}
                ]}))
        } else {
            client
                .post(format!("{fhir}/Observation{suffix}"))
                .bearer_auth(create)
                .json(&content)
        };
        let pending = tokio::spawn(async move { request.send().await.unwrap() });
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            loop {
                let waiting: i64 = query_scalar("SELECT count(*) FROM pg_stat_activity WHERE wait_event_type='Lock' AND query LIKE '%FROM%patient%FOR SHARE%'")
                    .fetch_one(&pool).await.unwrap();
                if waiting > 0 { break; }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        }).await.expect("write must reach the reference row lock after validating its previous body");
        if suffix == "transaction" {
            let independent = tokio::time::timeout(std::time::Duration::from_secs(10), client.post(fhir)
                .bearer_auth(full).json(&json!({"resourceType":"Bundle","type":"transaction","entry":[
                    {"request":{"method":"POST","url":"Patient"},"resource":{"resourceType":"Patient","birthDate":"2000-01-01"}}
                ]})).send()).await.expect("an unrelated transaction must commit while another reference is row-locked").unwrap();
            assert_eq!(independent.status(), StatusCode::OK);
        }
        // Even raw SQL cannot change a saved version in place. Verify that
        // protection, then commit a legitimate changed revision while the
        // request waits; the exact reference snapshot must still be rechecked.
        query("SAVEPOINT snapshot_guard_probe")
            .execute(&mut *blocker)
            .await
            .unwrap();
        let rejected = query("UPDATE patient SET resource=resource - 'birthDate' WHERE id=$1")
            .bind(id)
            .execute(&mut *blocker)
            .await
            .unwrap_err();
        assert_eq!(
            rejected.as_database_error().unwrap().code().as_deref(),
            Some("40001")
        );
        query("ROLLBACK TO SAVEPOINT snapshot_guard_probe")
            .execute(&mut *blocker)
            .await
            .unwrap();
        query("UPDATE patient SET txid=nextval('_transaction_txid_seq'), resource=resource - 'birthDate', status='updated' WHERE id=$1")
            .bind(id)
            .execute(&mut *blocker)
            .await
            .unwrap();
        blocker.commit().await.unwrap();
        let response = tokio::time::timeout(std::time::Duration::from_secs(10), pending)
            .await
            .unwrap()
            .unwrap();
        let status = response.status();
        let outcome: Value = response.json().await.unwrap();
        if suffix == "batch" {
            assert_eq!(status, StatusCode::OK, "{outcome}");
            assert_eq!(
                outcome["entry"][0]["response"]["status"], "409 Conflict",
                "{outcome}"
            );
        } else {
            assert_eq!(status, StatusCode::CONFLICT, "{suffix}: {outcome}");
        }
        assert_eq!(
            count_resources(client, fhir, full, "Observation").await,
            count
        );
    }
    pool.close().await;
}

async fn primary_reference_patch_regressions(
    client: &Client,
    fhir: &str,
    full: &str,
    patient_grants: (&str, &str, &str),
    grants: (&str, &str),
    patients: (&str, &str),
) {
    let (observation_create, observation_update) = grants;
    let (filtered_update, filtered_create, hidden_update) = patient_grants;
    let (id, other_id) = patients;
    // Reference-bearing primary writes still require c/u, independently of r.
    let observation = json!({"resourceType":"Observation","status":"final", "code":{"coding":[{"code":"atomic-write"}]}, "subject":{"reference":format!("Patient/{id}")}});
    for subject in [
        format!("Patient/{other_id}"),
        "Patient/atomic-missing".into(),
    ] {
        let mut content = observation.clone();
        content["subject"]["reference"] = json!(subject);
        let response = client
            .post(format!("{fhir}/Observation"))
            .bearer_auth(observation_create)
            .json(&content)
            .send()
            .await
            .unwrap();
        let status = response.status();
        let body: Value = response.json().await.unwrap();
        let expected = if subject.ends_with("atomic-missing") {
            StatusCode::UNPROCESSABLE_ENTITY
        } else {
            StatusCode::FORBIDDEN
        };
        assert_eq!(status, expected, "{body}");
    }
    let response = client
        .post(format!("{fhir}/Observation"))
        .bearer_auth(observation_create)
        .json(&observation)
        .send()
        .await
        .unwrap();
    let status = response.status();
    let atomic_observation: Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::CREATED, "{atomic_observation}");
    let observation_id = atomic_observation["id"].as_str().unwrap();
    let observation_url = format!("{fhir}/Observation/{observation_id}");
    let mut escaped = atomic_observation.clone();
    escaped["subject"]["reference"] = json!(format!("Patient/{other_id}"));
    let response = client
        .put(&observation_url)
        .bearer_auth(observation_update)
        .json(&escaped)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let response = client
        .get(&observation_url)
        .bearer_auth(full)
        .send()
        .await
        .unwrap();
    let status = response.status();
    let unchanged: Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::OK, "{unchanged}");
    assert_eq!(unchanged, atomic_observation);

    // PATCH has the same transaction/grant contract and needs u independently
    // of r. Reference failures and grant escapes leave JSON and history intact.
    let observation_history: Value = client
        .get(format!("{observation_url}/_history"))
        .bearer_auth(full)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    for (patch, expected) in [
        (
            json!([{"op":"replace","path":"/subject/reference","value":format!("Patient/{other_id}")}]),
            StatusCode::FORBIDDEN,
        ),
        (
            json!([{"op":"replace","path":"/subject/reference","value":"Patient/atomic-missing"}]),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            json!([{"op":"replace","path":"/code","value":{"text":"outside grant"}}]),
            StatusCode::FORBIDDEN,
        ),
    ] {
        let response = client
            .patch(&observation_url)
            .bearer_auth(observation_update)
            .header("Content-Type", "application/json-patch+json")
            .json(&patch)
            .send()
            .await
            .unwrap();
        let status = response.status();
        let outcome: Value = response.json().await.unwrap();
        assert_eq!(status, expected, "{outcome}");
        let current: Value = client
            .get(&observation_url)
            .bearer_auth(full)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(current, atomic_observation);
        let current_history: Value = client
            .get(format!("{observation_url}/_history"))
            .bearer_auth(full)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(current_history, observation_history);
    }
    let response = client.patch(&observation_url).bearer_auth(observation_update)
        .header("Content-Type", "application/fhir+json")
        .json(&json!({"resourceType":"Parameters","parameter":[{"name":"operation","part":[
            {"name":"type","valueCode":"replace"},{"name":"path","valueString":"Observation.status"},
            {"name":"value","valueCode":"amended"}
        ]}]})).send().await.unwrap();
    let status = response.status();
    let patched: Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::OK, "{patched}");
    assert_eq!(patched["status"], "amended");

    let response = client.post(format!("{fhir}/Patient")).bearer_auth(filtered_create)
        .json(&json!({"resourceType":"Patient", "active":true,
            "text":{"status":"generated","div":"<div xmlns=\"http://www.w3.org/1999/xhtml\">Active patient</div>"}}))
        .send().await.unwrap();
    let status = response.status();
    let patch_patient: Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::CREATED, "{patch_patient}");
    let patch_url = format!("{fhir}/Patient/{}", patch_patient["id"].as_str().unwrap());
    for (etag, expected) in [
        ("42", StatusCode::BAD_REQUEST),
        ("*", StatusCode::BAD_REQUEST),
        ("W/\"not-current\"", StatusCode::PRECONDITION_FAILED),
    ] {
        let response = client
            .patch(&patch_url)
            .bearer_auth(filtered_update)
            .header("Content-Type", "application/json-patch+json")
            .header("If-Match", etag)
            .json(&json!([{"op":"add","path":"/birthDate","value":"2000-01-01"}]))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
    }
    let response = client
        .patch(&patch_url)
        .bearer_auth(filtered_update)
        .header("Content-Type", "application/json-patch+json")
        .header("If-Match", "W/\"1\"")
        .header("If-Match", "W/\"2\"")
        .json(&json!([]))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let response = client
        .patch(&patch_url)
        .bearer_auth(hidden_update)
        .header("Content-Type", "application/json-patch+json")
        .json(&json!([]))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let response = client
        .patch(format!("{fhir}/Patient/patch-missing"))
        .bearer_auth(filtered_update)
        .header("Content-Type", "application/json-patch+json")
        .json(&json!([]))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let response = client
        .patch(&patch_url)
        .bearer_auth(filtered_update)
        .header("Content-Type", "application/json-patch+json; charset=utf-8")
        .json(&json!([{"op":"add","path":"/birthDate","value":"2000-01-01"}]))
        .send()
        .await
        .unwrap();
    let status = response.status();
    let etag = response
        .headers()
        .get("ETag")
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    let patched: Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::OK, "{patched}");
    assert_eq!(patched["birthDate"], "2000-01-01");
    assert!(
        patched.get("text").is_none(),
        "generated narrative must not become stale"
    );
    let current: Value = client
        .get(&patch_url)
        .bearer_auth(full)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(current, patched);
    let first = client
        .patch(&patch_url)
        .bearer_auth(filtered_update)
        .header("Content-Type", "application/json-patch+json")
        .header("If-Match", &etag)
        .json(&json!([{"op":"replace","path":"/birthDate","value":"2001-01-01"}]))
        .send();
    let second = client
        .patch(&patch_url)
        .bearer_auth(filtered_update)
        .header("Content-Type", "application/json-patch+json")
        .header("If-Match", &etag)
        .json(&json!([{"op":"replace","path":"/birthDate","value":"2002-01-01"}]))
        .send();
    let (first, second) = tokio::join!(first, second);
    let statuses = [first.unwrap().status(), second.unwrap().status()];
    assert_eq!(
        statuses
            .iter()
            .filter(|&&status| status == StatusCode::OK)
            .count(),
        1
    );
    assert_eq!(
        statuses
            .iter()
            .filter(|&&status| status == StatusCode::PRECONDITION_FAILED)
            .count(),
        1
    );
    let response = client
        .patch(&patch_url)
        .bearer_auth(filtered_update)
        .header("Content-Type", "application/json-patch+json")
        .header("Prefer", "return=minimal")
        .json(&json!([]))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response.bytes().await.unwrap().is_empty());
}

async fn reference_versions_and_profiles(
    client: &Client,
    fhir: &str,
    configured: &str,
    base: &str,
    full: &str,
    database: &str,
) {
    let reference_create = token(base, "system/Observation.c?code=reference-version").await;
    let reference_update = token(base, "system/Observation.u?code=reference-version").await;
    Box::pin(reference_version_regressions(
        client,
        fhir,
        configured,
        full,
        &reference_create,
        &reference_update,
    ))
    .await;
    Box::pin(reference_profile_regressions(
        client,
        fhir,
        full,
        &reference_create,
        &reference_update,
        database,
    ))
    .await;
}

async fn conditional_mutation_regressions(
    client: &Client,
    fhir: &str,
    base: &str,
    full: &str,
    update: &str,
    delete: &str,
    database: &str,
) {
    Box::pin(conditional_patient_mutations(
        client, fhir, full, update, delete,
    ))
    .await;
    let unrestricted_update = token(base, "system/Patient.u").await;
    let unrestricted_delete = token(base, "system/Patient.d").await;
    Box::pin(conditional_mutation_policies(
        client,
        fhir,
        full,
        &unrestricted_update,
        &unrestricted_delete,
    ))
    .await;
    let observation_update = token(base, "system/Observation.u?code=conditional-chain").await;
    let observation_delete = token(base, "system/Observation.d?code=conditional-chain").await;
    Box::pin(conditional_chain_mutation_races(
        client,
        fhir,
        full,
        (&observation_update, &observation_delete),
        database,
    ))
    .await;
}

async fn scoped_batch(client: &Client, fhir: &str, credential: &str, entries: Value) -> Value {
    let response = client
        .post(fhir)
        .bearer_auth(credential)
        .json(&json!({"resourceType":"Bundle","type":"batch","entry":entries}))
        .send()
        .await
        .unwrap();
    let status = response.status();
    let body: Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["type"], "batch-response");
    body
}

async fn scoped_transaction(
    client: &Client,
    fhir: &str,
    credential: &str,
    entries: Value,
    expected: StatusCode,
    minimal: bool,
) -> Value {
    let mut request = client
        .post(fhir)
        .bearer_auth(credential)
        .json(&json!({"resourceType":"Bundle","type":"transaction","entry":entries}));
    if minimal {
        request = request.header("Prefer", "return=minimal");
    }
    let response = request.send().await.unwrap();
    let status = response.status();
    let body: Value = response.json().await.unwrap();
    assert_eq!(status, expected, "{body}");
    if expected == StatusCode::OK {
        assert_eq!(body["type"], "transaction-response", "{body}");
    }
    body
}

async fn transaction_write_regressions(
    client: &Client,
    fhir: &str,
    base: &str,
    full: &str,
    credentials: (&str, &str, &str),
    observations: (&str, &str),
    patients: (&str, &str),
) {
    let (create, update, delete) = credentials;
    let before: Value = client
        .get(format!("{fhir}/Patient?_summary=count"))
        .bearer_auth(full)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    Box::pin(scoped_transaction(client, fhir, create, json!([
        {"request":{"method":"POST","url":"Patient"},"resource":{"resourceType":"Patient","active":true}},
        {"request":{"method":"POST","url":"Patient"},"resource":{"resourceType":"Patient","active":false}}
    ]), StatusCode::FORBIDDEN, false)).await;
    let after: Value = client
        .get(format!("{fhir}/Patient?_summary=count"))
        .bearer_auth(full)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        after["total"], before["total"],
        "denied sibling must roll back every create"
    );
    let created = Box::pin(scoped_transaction(client, fhir, create, json!([
        {"request":{"method":"POST","url":"Patient"},"resource":{"resourceType":"Patient","active":true}}
    ]), StatusCode::OK, false)).await;
    let patient = &created["entry"][0]["resource"];
    let id = patient["id"].as_str().unwrap();
    let path = format!("Patient/{id}");
    let history: Value = client
        .get(format!("{fhir}/{path}/_history"))
        .bearer_auth(full)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let hidden = token(base, "system/Patient.u?active=false").await;
    for (credential, body, expected) in [
        (
            update,
            json!({"resourceType":"Patient","id":id,"active":false}),
            StatusCode::FORBIDDEN,
        ),
        (
            hidden.as_str(),
            json!({"resourceType":"Patient","id":id,"active":true}),
            StatusCode::NOT_FOUND,
        ),
    ] {
        Box::pin(scoped_transaction(
            client,
            fhir,
            credential,
            json!([
                {"request":{"method":"PUT","url":path},"resource":body}
            ]),
            expected,
            false,
        ))
        .await;
        let current: Value = client
            .get(format!("{fhir}/{path}"))
            .bearer_auth(full)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(&current, patient);
        let current_history: Value = client
            .get(format!("{fhir}/{path}/_history"))
            .bearer_auth(full)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(current_history, history);
    }
    // Result grants must run even when there is no returned representation.
    let named_update = token(base, "system/Patient.u?family=TransactionAllowed").await;
    let upsert_id = "transaction-filtered-upsert";
    Box::pin(scoped_transaction(client, fhir, &named_update, json!([
        {"request":{"method":"PUT","url":format!("Patient/{upsert_id}")},
         "resource":{"resourceType":"Patient","id":upsert_id,"active":true,"name":[{"family":"TransactionOutside"}]}}
    ]), StatusCode::NOT_FOUND, true)).await;
    let minimal = Box::pin(scoped_transaction(client, fhir, &named_update, json!([
        {"request":{"method":"PUT","url":format!("Patient/{upsert_id}")},
         "resource":{"resourceType":"Patient","id":upsert_id,"active":true,"name":[{"family":"TransactionAllowed"}]}}
    ]), StatusCode::OK, true)).await;
    assert_eq!(
        minimal["entry"][0]["response"]["status"], "201 Created",
        "{minimal}"
    );
    assert!(minimal["entry"][0].get("resource").is_none(), "{minimal}");
    Box::pin(scoped_transaction(client, fhir, &named_update, json!([
        {"request":{"method":"PUT","url":format!("Patient/{upsert_id}")},
         "resource":{"resourceType":"Patient","id":upsert_id,"active":true,"name":[{"family":"TransactionOutside"}]}}
    ]), StatusCode::FORBIDDEN, true)).await;
    let patched = Box::pin(scoped_transaction(client, fhir, update, json!([
        {"request":{"method":"PATCH","url":path},"resource":{"resourceType":"Parameters","parameter":[{"name":"operation","part":[
            {"name":"type","valueCode":"add"},{"name":"path","valueString":"Patient"},{"name":"name","valueString":"birthDate"},{"name":"value","valueDate":"2000-01-01"}
        ]}]}}
    ]), StatusCode::OK, false)).await;
    assert_eq!(
        patched["entry"][0]["resource"]["birthDate"], "2000-01-01",
        "{patched}"
    );
    let etag = patched["entry"][0]["response"]["etag"].as_str().unwrap();
    let entries = json!([{ "request":{"method":"PUT","url":path,"ifMatch":etag},
        "resource":{"resourceType":"Patient","id":id,"active":true,"birthDate":"2001-01-01"}}]);
    let first = client
        .post(fhir)
        .bearer_auth(update)
        .json(&json!({"resourceType":"Bundle","type":"transaction","entry":entries}))
        .send();
    let second = client
        .post(fhir)
        .bearer_auth(update)
        .json(&json!({"resourceType":"Bundle","type":"transaction","entry":entries}))
        .send();
    let (first, second) = tokio::join!(first, second);
    let statuses = [first.unwrap().status(), second.unwrap().status()];
    assert_eq!(
        statuses
            .iter()
            .filter(|&&status| status == StatusCode::OK)
            .count(),
        1
    );
    assert_eq!(
        statuses
            .iter()
            .filter(|&&status| status == StatusCode::PRECONDITION_FAILED)
            .count(),
        1
    );
    Box::pin(scoped_transaction(
        client,
        fhir,
        delete,
        json!([
            {"request":{"method":"DELETE","url":path}}
        ]),
        StatusCode::OK,
        true,
    ))
    .await;
    Box::pin(scoped_transaction(
        client,
        fhir,
        delete,
        json!([
            {"request":{"method":"DELETE","url":path}}
        ]),
        StatusCode::NOT_FOUND,
        true,
    ))
    .await;
    let unrestricted_delete = token(base, "system/Patient.d").await;
    Box::pin(scoped_transaction(
        client,
        fhir,
        &unrestricted_delete,
        json!([
            {"request":{"method":"DELETE","url":path}}
        ]),
        StatusCode::OK,
        true,
    ))
    .await;
    for (subject, expected) in [
        (patients.0, StatusCode::OK),
        (patients.1, StatusCode::FORBIDDEN),
    ] {
        Box::pin(scoped_transaction(client, fhir, observations.0, json!([
            {"request":{"method":"POST","url":"Observation"},"resource":{"resourceType":"Observation","status":"final","code":{"coding":[{"code":"atomic-write"}]},"subject":{"reference":format!("Patient/{subject}")}}}
        ]), expected, false)).await;
    }
    // Exercise grouped c predicates under concurrent authenticated load.
    for concurrency in [1, 4, 16] {
        let started = std::time::Instant::now();
        let mut requests = Vec::new();
        for _ in 0..concurrency {
            let client = client.clone();
            let fhir = fhir.to_owned();
            let credential = create.to_owned();
            requests.push(tokio::spawn(async move {
                let entries: Vec<_> = (0..50).map(|_| json!({"request":{"method":"POST","url":"Patient"},"resource":{"resourceType":"Patient","active":true}})).collect();
                let body = scoped_transaction(&client, &fhir, &credential, json!(entries), StatusCode::OK, true).await;
                assert_eq!(body["entry"].as_array().unwrap().len(), 50);
                for entry in body["entry"].as_array().unwrap() {
                    assert_eq!(entry["response"]["status"], "201 Created", "{entry}");
                    assert!(entry.get("resource").is_none(), "{entry}");
                }
            }));
        }
        for request in requests {
            request.await.unwrap();
        }
        let elapsed = started.elapsed();
        println!("Filtered transaction grants: {concurrency} concurrent x 50 entries: {elapsed:?}");
        assert!(elapsed < std::time::Duration::from_secs(30), "{elapsed:?}");
    }
}

async fn transaction_conditional_grant_regressions(
    client: &Client,
    fhir: &str,
    base: &str,
    full: &str,
    observations: (&str, &str, &str),
    patients: (&str, &str),
) {
    let graph_rights = token(
        base,
        "system/Patient.c?active=true system/Observation.c?subject:Patient.active=true",
    )
    .await;
    let graph_bundle = |allowed: bool, marker: &str| {
        json!([
            {"request":{"method":"POST","url":"Patient"},"resource":{"resourceType":"Patient","active":allowed,
             "identifier":[{"system":"urn:scoped-graph","value":marker}]}},
            {"request":{"method":"POST","url":"Observation"},"resource":{"resourceType":"Observation","status":"final",
             "code":{"text":"scoped graph"},"subject":{"reference":format!("Patient?identifier=urn:scoped-graph|{marker}")}}}
        ])
    };
    let graph_result = Box::pin(scoped_transaction(
        client,
        fhir,
        &graph_rights,
        graph_bundle(true, "allowed"),
        StatusCode::OK,
        false,
    ))
    .await;
    let graph_id = graph_result["entry"][0]["resource"]["id"].as_str().unwrap();
    assert_eq!(
        graph_result["entry"][1]["resource"]["subject"]["reference"],
        format!("Patient/{graph_id}")
    );
    Box::pin(scoped_transaction(
        client,
        fhir,
        &graph_rights,
        graph_bundle(false, "denied"),
        StatusCode::FORBIDDEN,
        true,
    ))
    .await;
    let denied_count: Value = client
        .get(format!(
            "{fhir}/Patient?identifier=urn:scoped-graph|denied&_summary=count"
        ))
        .bearer_auth(full)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        denied_count["total"], 0,
        "denied own-graph grant must roll back all siblings"
    );

    let graph_started = std::time::Instant::now();
    let mut graph_entries = Vec::new();
    for index in 0..50 {
        graph_entries.push(json!({"request":{"method":"POST","url":"Patient"},
            "resource":{"resourceType":"Patient","active":true,"identifier":[{"system":"urn:scoped-graph-load","value":index.to_string()}]}}));
        graph_entries.push(json!({"request":{"method":"POST","url":"Observation"},
            "resource":{"resourceType":"Observation","status":"final","code":{"text":"graph load"},
            "subject":{"reference":format!("Patient?identifier=urn:scoped-graph-load|{index}")}}}));
    }
    let loaded = Box::pin(scoped_transaction(
        client,
        fhir,
        &graph_rights,
        json!(graph_entries),
        StatusCode::OK,
        true,
    ))
    .await;
    assert_eq!(loaded["entry"].as_array().unwrap().len(), 100);
    assert!(
        loaded["entry"]
            .as_array()
            .unwrap()
            .iter()
            .all(|entry| entry["response"]["status"] == "201 Created")
    );
    println!(
        "Proposed graph: 100 entries, 50 conditional references and native c grants: {:?}",
        graph_started.elapsed()
    );

    let condition =
        "identifier=http://example.org/transaction-conditional|rights&_count=1&_offset=999";
    let mut originals = Vec::new();
    for (family, active) in [
        ("CondCreate", true),
        ("CondUpdate", false),
        ("CondDelete", false),
    ] {
        let response = client
            .post(format!("{fhir}/Patient"))
            .bearer_auth(full)
            .json(
                &json!({"resourceType":"Patient","active":active,"name":[{"family":family}],
                "identifier":[{"system":"http://example.org/transaction-conditional","value":"rights"}]}),
            )
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        originals.push(response.json::<Value>().await.unwrap());
    }
    // Identical client predicates must select different targets under c/u/d.
    // Neither read nor search is granted to this token.
    let rights = token(base, "system/Patient.c?active=true system/Patient.u?family=CondUpdate system/Patient.d?family=CondDelete").await;
    let result = Box::pin(scoped_transaction(client, fhir, &rights, json!([
        {"request":{"method":"POST","url":"Patient","ifNoneExist":condition},"resource":{"resourceType":"Patient","birthDate":42}},
        {"request":{"method":"PUT","url":format!("Patient?{condition}")},"resource":{"resourceType":"Patient","active":false,
            "name":[{"family":"CondUpdate"}],"birthDate":"2001-01-01","identifier":[{"system":"http://example.org/transaction-conditional","value":"rights"}]}},
        {"request":{"method":"DELETE","url":format!("Patient?{condition}")}}
    ]), StatusCode::OK, false)).await;
    assert_eq!(result["entry"][0]["resource"], originals[0]);
    assert_eq!(result["entry"][1]["resource"]["id"], originals[1]["id"]);
    assert_eq!(result["entry"][1]["resource"]["birthDate"], "2001-01-01");
    assert_eq!(result["entry"][2]["response"]["status"], "204 No Content");
    assert!(
        result["entry"][2]["fullUrl"]
            .as_str()
            .unwrap()
            .ends_with(originals[2]["id"].as_str().unwrap())
    );

    let create = token(base, "system/Patient.c?family=CondCreate").await;
    let duplicates: Vec<_> = (0..64)
        .map(|_| {
            json!({
                "request":{"method":"POST","url":"Patient","ifNoneExist":condition},
                "resource":{"resourceType":"Patient","birthDate":42}
            })
        })
        .collect();
    Box::pin(scoped_transaction(
        client,
        fhir,
        &create,
        json!(duplicates),
        StatusCode::BAD_REQUEST,
        false,
    ))
    .await;
    let retained: Value = client
        .get(format!(
            "{fhir}/Patient/{}",
            originals[0]["id"].as_str().unwrap()
        ))
        .bearer_auth(full)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        retained, originals[0],
        "duplicate matched identities must preserve the exact snapshot"
    );
    let update = token(base, "system/Patient.u?family=CondUpdate").await;
    // Transaction conditional PATCH uses u independently of r/s. Its policies
    // and final grant inspect the actual patched Patient, never the Binary.
    let patch = |value: Value| {
        use base64::{Engine, engine::general_purpose::STANDARD};
        json!({"resourceType":"Binary","contentType":"application/json-patch+json",
            "data":STANDARD.encode(serde_json::to_vec(&value).unwrap())})
    };
    let patched = Box::pin(scoped_transaction(
        client,
        fhir,
        &update,
        json!([
            {"request":{"method":"PATCH","url":format!("Patient?{condition}")},
                "resource":patch(json!([{"op":"add","path":"/birthDate","value":"2002-02-02"}]))}
        ]),
        StatusCode::OK,
        false,
    ))
    .await;
    assert_eq!(patched["entry"][0]["resource"]["id"], originals[1]["id"]);
    assert_eq!(patched["entry"][0]["resource"]["birthDate"], "2002-02-02");
    let patch_target = format!("{fhir}/Patient/{}", originals[1]["id"].as_str().unwrap());
    let before: Value = client
        .get(format!("{patch_target}/_history"))
        .bearer_auth(full)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    Box::pin(scoped_transaction(client, fhir, &update, json!([
        {"request":{"method":"PATCH","url":format!("Patient?{condition}")},
            "resource":patch(json!([{"op":"replace","path":"/name/0/family","value":"OutsideGrant"}]))}
    ]), StatusCode::FORBIDDEN, false)).await;
    Box::pin(scoped_transaction(client, fhir, &update, json!([
        {"request":{"method":"PATCH","url":"Patient?_id=missing-conditional-patch"},"resource":patch(json!([]))}
    ]), StatusCode::NOT_FOUND, false)).await;
    Box::pin(scoped_transaction(client, fhir, &update, json!([
        {"request":{"method":"PATCH","url":format!("Patient?{condition}"),"ifMatch":"W/\"stale\""},"resource":patch(json!([]))}
    ]), StatusCode::PRECONDITION_FAILED, false)).await;
    let after: Value = client
        .get(format!("{patch_target}/_history"))
        .bearer_auth(full)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        before, after,
        "Denied conditional PATCH must preserve exact history"
    );

    let delete = token(base, "system/Patient.d?family=CondDelete").await;
    // A hidden match is not included in visible cardinality. Empty d selection
    // is a no-op; empty c/u selection checks the actual inserted result.
    let no_match = "identifier=urn:transaction-conditional%7Chidden";
    let response = client
        .post(format!("{fhir}/Patient"))
        .bearer_auth(full)
        .json(
            &json!({"resourceType":"Patient","active":false,"name":[{"family":"CondHidden"}],
            "identifier":[{"system":"urn:transaction-conditional","value":"hidden"}]}),
        )
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let hidden = response.json::<Value>().await.unwrap();
    Box::pin(scoped_transaction(
        client,
        fhir,
        &delete,
        json!([
            {"request":{"method":"DELETE","url":format!("Patient?{no_match}")}}
        ]),
        StatusCode::OK,
        false,
    ))
    .await;
    for (method, credential, family, expected) in [
        ("POST", &create, "CondOutside", StatusCode::FORBIDDEN),
        ("PUT", &update, "CondOutside", StatusCode::NOT_FOUND),
        ("POST", &create, "CondCreate", StatusCode::OK),
        ("PUT", &update, "CondUpdate", StatusCode::OK),
    ] {
        let request = if method == "POST" {
            json!({"method":method,"url":"Patient","ifNoneExist":no_match})
        } else {
            json!({"method":method,"url":format!("Patient?{no_match}")})
        };
        let result = Box::pin(scoped_transaction(client, fhir, credential, json!([
            {"request":request,"resource":{"resourceType":"Patient","active":true,"name":[{"family":family}],
                "identifier":[{"system":"urn:transaction-conditional","value":"hidden"}]}}
        ]), expected, false)).await;
        if expected == StatusCode::OK {
            assert_eq!(result["entry"][0]["response"]["status"], "201 Created");
            assert_ne!(result["entry"][0]["resource"]["id"], hidden["id"]);
        }
    }
    // An explicit body ID cannot turn a grant-hidden no-match into an update.
    Box::pin(scoped_transaction(client, fhir, &update, json!([
        {"request":{"method":"PUT","url":"Patient?identifier=urn:transaction-conditional%7Cabsent"},
            "resource":{"resourceType":"Patient","id":hidden["id"],"name":[{"family":"CondUpdate"}]}}
    ]), StatusCode::NOT_FOUND, false)).await;
    let current: Value = client
        .get(format!("{fhir}/Patient/{}", hidden["id"].as_str().unwrap()))
        .bearer_auth(full)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(current, hidden);

    let response = client
        .post(format!("{fhir}/Patient"))
        .bearer_auth(full)
        .json(
            &json!({"resourceType":"Patient","active":true,"name":[{"family":"CondCreate"}],
            "identifier":[{"system":"http://example.org/transaction-conditional","value":"rights"}]}),
        )
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    Box::pin(scoped_transaction(client, fhir, &create, json!([
        {"request":{"method":"POST","url":"Patient","ifNoneExist":condition},"resource":{"resourceType":"Patient","active":true}}
    ]), StatusCode::PRECONDITION_FAILED, false)).await;
    // Result checks remain active when the Bundle requests minimal responses.
    let before: Value = client
        .get(format!(
            "{fhir}/Patient/{}",
            originals[1]["id"].as_str().unwrap()
        ))
        .bearer_auth(full)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let history: Value = client
        .get(format!(
            "{fhir}/Patient/{}/_history",
            originals[1]["id"].as_str().unwrap()
        ))
        .bearer_auth(full)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let create_update = token(
        base,
        "system/Patient.c?family=CondUpdate system/Patient.u?family=CondUpdate",
    )
    .await;
    Box::pin(scoped_transaction(client, fhir, &create_update, json!([
        {"request":{"method":"PUT","url":format!("Patient?{condition}")},
            "resource":{"resourceType":"Patient","name":[{"family":"CondOutside"}]}},
        {"request":{"method":"POST","url":"Patient"},"resource":{"resourceType":"Patient","name":[{"family":"CondUpdate"}]}}
    ]), StatusCode::FORBIDDEN, true)).await;
    let after: Value = client
        .get(format!(
            "{fhir}/Patient/{}",
            originals[1]["id"].as_str().unwrap()
        ))
        .bearer_auth(full)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let history_after: Value = client
        .get(format!(
            "{fhir}/Patient/{}/_history",
            originals[1]["id"].as_str().unwrap()
        ))
        .bearer_auth(full)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(after, before);
    assert_eq!(history_after, history);

    let race_bundle = json!({"resourceType":"Bundle","type":"transaction","entry":[
        {"request":{"method":"POST","url":"Patient","ifNoneExist":"identifier=urn:transaction-conditional%7Crace"},
         "resource":{"resourceType":"Patient","name":[{"family":"CondCreate"}],"identifier":[{"system":"urn:transaction-conditional","value":"race"}]}}
    ]});
    let (first, second) = tokio::join!(
        client
            .post(fhir)
            .bearer_auth(&create)
            .json(&race_bundle)
            .send(),
        client
            .post(fhir)
            .bearer_auth(&create)
            .json(&race_bundle)
            .send(),
    );
    let mut created = 0;
    for response in [first.unwrap(), second.unwrap()] {
        let status = response.status();
        let body: Value = response.json().await.unwrap();
        if status == StatusCode::OK {
            match body["entry"][0]["response"]["status"].as_str().unwrap() {
                "201 Created" => created += 1,
                "200 OK" => {}
                _ => panic!("{body}"),
            }
        } else {
            assert_eq!(status, StatusCode::CONFLICT, "{body}");
        }
    }
    assert_eq!(created, 1);
    let count: Value = client
        .get(format!(
            "{fhir}/Patient?identifier=urn:transaction-conditional%7Crace&_summary=count"
        ))
        .bearer_auth(full)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        count["total"], 1,
        "concurrent scoped conditions must not create duplicates"
    );
    let started = std::time::Instant::now();
    let entries: Vec<_> = (0..50).map(|index| json!({
        "request":{"method":"POST","url":"Patient","ifNoneExist":format!("identifier=urn:transaction-conditional-load%7C{index}")},
        "resource":{"resourceType":"Patient","name":[{"family":"CondCreate"}],"identifier":[{"system":"urn:transaction-conditional-load","value":index.to_string()}]}
    })).collect();
    let result = Box::pin(scoped_transaction(
        client,
        fhir,
        &create,
        json!(entries),
        StatusCode::OK,
        true,
    ))
    .await;
    assert_eq!(result["entry"].as_array().unwrap().len(), 50);
    for entry in result["entry"].as_array().unwrap() {
        assert_eq!(entry["response"]["status"], "201 Created", "{entry}");
    }
    println!(
        "Filtered conditional transaction: 50 independent predicates: {:?}",
        started.elapsed()
    );

    let (patient, foreign_patient) = patients;
    let obs_condition = "identifier=urn:transaction-conditional%7Cpatient";
    let observation = |subject: &str| {
        json!({"resourceType":"Observation","status":"final","code":{"coding":[{"code":"atomic-write"}]},
        "subject":{"reference":format!("Patient/{subject}")},"identifier":[{"system":"urn:transaction-conditional","value":"patient"}]})
    };
    let mut bodies = Vec::new();
    for subject in [patient, foreign_patient] {
        let response = client
            .post(format!("{fhir}/Observation"))
            .bearer_auth(full)
            .json(&observation(subject))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        bodies.push(response.json::<Value>().await.unwrap());
    }
    let result = Box::pin(scoped_transaction(client, fhir, observations.0, json!([
        {"request":{"method":"POST","url":"Observation","ifNoneExist":obs_condition},"resource":{"resourceType":"Observation"}}
    ]), StatusCode::OK, false)).await;
    assert_eq!(result["entry"][0]["resource"], bodies[0]);
    Box::pin(scoped_transaction(client, fhir, observations.1, json!([
        {"request":{"method":"PUT","url":format!("Observation?{obs_condition}")},"resource":observation(foreign_patient)}
    ]), StatusCode::FORBIDDEN, true)).await;
    let result = Box::pin(scoped_transaction(client, fhir, observations.1, json!([
        {"request":{"method":"PUT","url":format!("Observation?{obs_condition}")},"resource":observation(patient)}
    ]), StatusCode::OK, false)).await;
    assert_eq!(result["entry"][0]["resource"]["id"], bodies[0]["id"]);
    Box::pin(scoped_transaction(
        client,
        fhir,
        observations.2,
        json!([
            {"request":{"method":"DELETE","url":format!("Observation?{obs_condition}")}}
        ]),
        StatusCode::OK,
        true,
    ))
    .await;
    let foreign: Value = client
        .get(format!(
            "{fhir}/Observation/{}",
            bodies[1]["id"].as_str().unwrap()
        ))
        .bearer_auth(full)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(foreign, bodies[1]);
}

async fn transaction_missing_identity_race(
    client: &Client,
    fhir: &str,
    full: &str,
    update: &str,
    database: &str,
) {
    use sqlx_core::{query::query, query_scalar::query_scalar};
    let pool = sqlx_postgres::PgPoolOptions::new()
        .max_connections(4)
        .connect(database)
        .await
        .unwrap();
    let locked_id = "transaction-race-a-existing";
    let missing_id = "transaction-race-b-missing";
    let response = client
        .put(format!("{fhir}/Patient/{locked_id}"))
        .bearer_auth(full)
        .json(&json!({"resourceType":"Patient","id":locked_id,"active":true}))
        .send()
        .await
        .unwrap();
    let original: Value = response.json().await.unwrap();
    let history: Value = client
        .get(format!("{fhir}/Patient/{locked_id}/_history"))
        .bearer_auth(full)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let mut blocker = pool.begin().await.unwrap();
    query("SELECT id FROM patient WHERE id=$1 FOR UPDATE")
        .bind(locked_id)
        .execute(&mut *blocker)
        .await
        .unwrap();
    let task_client = client.clone();
    let task_fhir = fhir.to_owned();
    let credential = update.to_owned();
    let task = tokio::spawn(async move {
        task_client.post(task_fhir).bearer_auth(credential)
            .json(&json!({"resourceType":"Bundle","type":"transaction","entry":[
                {"request":{"method":"PUT","url":format!("Patient/{locked_id}")},"resource":{"resourceType":"Patient","id":locked_id,"active":true,"birthDate":"2000-01-01"}},
                {"request":{"method":"PUT","url":format!("Patient/{missing_id}")},"resource":{"resourceType":"Patient","id":missing_id,"active":true}}
            ]})).send().await.unwrap()
    });
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let blocked: bool = query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE wait_event_type='Lock' AND query LIKE '%id=ANY%FOR UPDATE%')")
                .fetch_one(&pool).await.unwrap();
            if blocked { break; }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }).await.expect("Bundle must establish its missing-row snapshot before the concurrent insert");
    let resource: Value = query_scalar("WITH version AS (SELECT nextval('_transaction_txid_seq') AS txid) INSERT INTO patient (id,txid,resource,status) SELECT $1,version.txid,jsonb_build_object('resourceType','Patient','id',$1,'active',false,'meta',jsonb_build_object('versionId',version.txid::text,'lastUpdated',to_char(NOW() AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"'))),'created' FROM version RETURNING resource")
        .bind(missing_id).fetch_one(&pool).await.unwrap();
    blocker.rollback().await.unwrap();
    let response = tokio::time::timeout(std::time::Duration::from_secs(10), task)
        .await
        .unwrap()
        .unwrap();
    let status = response.status();
    let outcome: Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::CONFLICT, "{outcome}");
    let current: Value = client
        .get(format!("{fhir}/Patient/{missing_id}"))
        .bearer_auth(full)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        current, resource,
        "a previously missing target cannot become an unchecked update"
    );
    let current: Value = client
        .get(format!("{fhir}/Patient/{locked_id}"))
        .bearer_auth(full)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        current, original,
        "the successful sibling update must roll back"
    );
    let current_history: Value = client
        .get(format!("{fhir}/Patient/{locked_id}/_history"))
        .bearer_auth(full)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(current_history, history);
}

async fn batch_write_regressions(
    client: &Client,
    fhir: &str,
    configured_fhir: &str,
    full: &str,
    patient_credentials: (&str, &str, &str),
    observation_credentials: (&str, &str),
    patients: (&str, &str),
) {
    let (create, update, delete) = patient_credentials;
    let patient = Box::pin(batch_create_contracts(
        client,
        fhir,
        configured_fhir,
        full,
        create,
    ))
    .await;
    Box::pin(batch_update_delete_contracts(
        client,
        fhir,
        full,
        (&patient, update, delete),
    ))
    .await;
    Box::pin(batch_compartment_contracts(
        client,
        fhir,
        full,
        observation_credentials,
        patients,
    ))
    .await;
}

async fn batch_create_contracts(
    client: &Client,
    fhir: &str,
    configured_fhir: &str,
    full: &str,
    create: &str,
) -> Value {
    let entries = json!([
        {"request":{"method":"POST","url":"Patient"},"resource":{"resourceType":"Patient", "id":"ignored-batch-id", "active":true,"identifier":[{"system":"urn:batch-atomic","value":"created"}]}},
        {"request":{"method":"POST","url":"Patient"},"resource":{"resourceType":"Patient", "active":false}},
        {"request":{"method":"POST","url":"Patient"},"resource":{"resourceType":"Patient", "birthDate":42}}
    ]);
    let body = scoped_batch(client, fhir, create, entries).await;
    let statuses: Vec<_> = body["entry"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["response"]["status"].as_str().unwrap())
        .collect();
    assert_eq!(
        statuses,
        ["201 Created", "403 Forbidden", "422 Unprocessable Entity"],
        "{body}"
    );
    let entry = &body["entry"][0];
    let patient = entry["resource"].clone();
    let id = patient["id"].as_str().unwrap();
    assert_ne!(id, "ignored-batch-id");
    assert_eq!(entry["fullUrl"], format!("{configured_fhir}/Patient/{id}"));
    assert_eq!(
        entry["response"]["location"],
        format!(
            "{configured_fhir}/Patient/{id}/_history/{}",
            patient["meta"]["versionId"].as_str().unwrap()
        )
    );
    assert_eq!(
        entry["response"]["etag"],
        format!("W/\"{}\"", patient["meta"]["versionId"].as_str().unwrap())
    );
    assert_eq!(
        entry["response"]["lastModified"],
        patient["meta"]["lastUpdated"]
    );
    let direct = format!("{fhir}/Patient/{id}");
    let original_history: Value = client
        .get(format!("{direct}/_history"))
        .bearer_auth(full)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let matched = scoped_batch(client, fhir, create, json!([
        {"request":{"method":"POST","url":"Patient","ifNoneExist":"identifier=urn:batch-atomic%7Ccreated"},"resource":{"resourceType":"Patient","birthDate":42}}
    ])).await;
    assert_eq!(
        matched["entry"][0]["response"]["status"], "200 OK",
        "{matched}"
    );
    assert_eq!(matched["entry"][0]["resource"], patient);
    let current_history: Value = client
        .get(format!("{direct}/_history"))
        .bearer_auth(full)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(current_history, original_history);
    let response = client.post(fhir).bearer_auth(create).header("Prefer", "return=minimal")
        .json(&json!({"resourceType":"Bundle", "type":"batch", "entry":[
            {"request":{"method":"POST","url":"Patient"},"resource":{"resourceType":"Patient","active":true}}
        ]})).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let minimal: Value = response.json().await.unwrap();
    assert!(minimal["entry"][0].get("resource").is_none(), "{minimal}");
    assert!(
        minimal["entry"][0]["response"]["location"]
            .as_str()
            .unwrap()
            .starts_with(configured_fhir),
        "{minimal}"
    );
    patient
}

async fn batch_update_delete_contracts(
    client: &Client,
    fhir: &str,
    full: &str,
    context: (&Value, &str, &str),
) {
    use base64::{Engine, engine::general_purpose::STANDARD};
    let (patient, update, delete) = context;
    let id = patient["id"].as_str().unwrap();
    let target = format!("Patient/{id}");
    let direct = format!("{fhir}/{target}");
    let patch = |value: Value| json!({"resourceType":"Binary", "contentType":"application/json-patch+json", "data":STANDARD.encode(serde_json::to_vec(&value).unwrap())});
    let history: Value = client
        .get(format!("{direct}/_history"))
        .bearer_auth(full)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let body = scoped_batch(client, fhir, update, json!([
        {"request":{"method":"PUT","url":target},"resource":{"resourceType":"Patient","id":id,"active":false}},
        {"request":{"method":"PATCH","url":target},"resource":patch(json!([{"op":"replace","path":"/active","value":false}]))},
        {"request":{"method":"PATCH","url":target},"resource":patch(json!([{"op":"add","path":"/managingOrganization","value":{"reference":"Organization/batch-absent"}}]))},
        {"request":{"method":"PUT","url":target,"ifMatch":"*"},"resource":patient},
        {"request":{"method":"PUT","url":target,"ifMatch":42},"resource":patient},
        {"request":{"method":"PATCH","url":target,"ifMatch":"W/\"stale\""},"resource":patch(json!([]))}
    ])).await;
    let statuses: Vec<_> = body["entry"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["response"]["status"].as_str().unwrap())
        .collect();
    assert_eq!(
        statuses,
        [
            "403 Forbidden",
            "403 Forbidden",
            "422 Unprocessable Entity",
            "400 Bad Request",
            "400 Bad Request",
            "412 Precondition Failed"
        ],
        "{body}"
    );
    let after: Value = client
        .get(format!("{direct}/_history"))
        .bearer_auth(full)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(history, after);
    let body = scoped_batch(client, fhir, update, json!([
        {"request":{"method":"PUT","url":target,"ifMatch":format!("W/\"{}\"", patient["meta"]["versionId"].as_str().unwrap())},"resource":patient}
    ])).await;
    assert_eq!(body["entry"][0]["response"]["status"], "200 OK", "{body}");
    let current = &body["entry"][0]["resource"];
    let etag = format!("W/\"{}\"", current["meta"]["versionId"].as_str().unwrap());
    let requests = ["2001-01-01", "2002-01-01"].map(|date| {
        client.post(fhir).bearer_auth(update).json(&json!({"resourceType":"Bundle","type":"batch","entry":[
            {"request":{"method":"PATCH","url":target,"ifMatch":etag},"resource":patch(json!([{"op":"add","path":"/birthDate","value":date}]))}
        ]})).send()
    });
    let [first, second] = requests;
    let (first, second) = tokio::join!(first, second);
    let first: Value = first.unwrap().json().await.unwrap();
    let second: Value = second.unwrap().json().await.unwrap();
    let statuses = [
        &first["entry"][0]["response"]["status"],
        &second["entry"][0]["response"]["status"],
    ];
    assert_eq!(
        statuses
            .iter()
            .filter(|&&status| status == "200 OK")
            .count(),
        1,
        "{first} {second}"
    );
    assert_eq!(
        statuses
            .iter()
            .filter(|&&status| status == "412 Precondition Failed")
            .count(),
        1,
        "{first} {second}"
    );
    let body = scoped_batch(client, fhir, update, json!([
        {"request":{"method":"PATCH","url":"Patient?identifier=urn:batch-atomic%7Ccreated"},"resource":{"resourceType":"Parameters","parameter":[{"name":"operation","part":[
            {"name":"type","valueCode":"replace"}, {"name":"path","valueString":"Patient.birthDate"}, {"name":"value","valueDate":"1990-01-01"}
        ]}]}}
    ])).await;
    assert_eq!(body["entry"][0]["response"]["status"], "200 OK", "{body}");
    assert_eq!(body["entry"][0]["resource"]["birthDate"], "1990-01-01");
    let delete_etag = body["entry"][0]["response"]["etag"].clone();
    let history: Value = client
        .get(format!("{direct}/_history"))
        .bearer_auth(full)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let denied = scoped_batch(
        client,
        fhir,
        delete,
        json!([
            {"request":{"method":"DELETE","url":target,"ifMatch":etag}}
        ]),
    )
    .await;
    assert_eq!(
        denied["entry"][0]["response"]["status"], "412 Precondition Failed",
        "{denied}"
    );
    let after: Value = client
        .get(format!("{direct}/_history"))
        .bearer_auth(full)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(history, after);
    let deleted = scoped_batch(client, fhir, delete, json!([
        {"request":{"method":"DELETE","url":"Patient?identifier=urn:batch-atomic%7Ccreated","ifMatch":delete_etag}}
    ])).await;
    assert_eq!(
        deleted["entry"][0]["response"]["status"], "204 No Content",
        "{deleted}"
    );
    let repeated = scoped_batch(
        client,
        fhir,
        delete,
        json!([
            {"request":{"method":"DELETE","url":"Patient?identifier=urn:batch-atomic%7Ccreated"}}
        ]),
    )
    .await;
    assert_eq!(
        repeated["entry"][0]["response"]["status"], "204 No Content",
        "{repeated}"
    );
    let conditional = scoped_batch(client, fhir, update, json!([
        {"request":{"method":"PUT","url":"Patient?identifier=urn:batch-atomic%7Cupsert"},"resource":{"resourceType":"Patient","active":true,"identifier":[{"system":"urn:batch-atomic","value":"upsert"}]}}
    ])).await;
    assert_eq!(
        conditional["entry"][0]["response"]["status"], "201 Created",
        "{conditional}"
    );
}

async fn batch_compartment_contracts(
    client: &Client,
    fhir: &str,
    full: &str,
    credentials: (&str, &str),
    patients: (&str, &str),
) {
    let (update, delete) = credentials;
    let (patient, foreign) = patients;
    let content = |id: &str, owner: &str| json!({"resourceType":"Observation","id":id,"status":"final","code":{"coding":[{"code":"atomic-write"}]},"subject":{"reference":format!("Patient/{owner}")}});
    let created = scoped_batch(client, fhir, update, json!([
        {"request":{"method":"PUT","url":"Observation/batch-compartment"},"resource":content("batch-compartment", patient)},
        {"request":{"method":"PUT","url":"Observation/batch-foreign"},"resource":content("batch-foreign", foreign)}
    ])).await;
    assert_eq!(
        created["entry"][0]["response"]["status"], "201 Created",
        "{created}"
    );
    assert_eq!(
        created["entry"][1]["response"]["status"], "404 Not Found",
        "{created}"
    );
    let hidden = scoped_batch(client, fhir, full, json!([
        {"request":{"method":"PUT","url":"Observation/batch-foreign"},"resource":content("batch-foreign", foreign)}
    ])).await;
    assert_eq!(
        hidden["entry"][0]["response"]["status"], "201 Created",
        "{hidden}"
    );
    let denied = scoped_batch(client, fhir, update, json!([
        {"request":{"method":"PUT","url":"Observation/batch-foreign"},"resource":content("batch-foreign", patient)},
        {"request":{"method":"PUT","url":"Observation/batch-compartment"},"resource":content("batch-compartment", foreign)}
    ])).await;
    assert_eq!(
        denied["entry"][0]["response"]["status"], "404 Not Found",
        "{denied}"
    );
    assert_eq!(
        denied["entry"][1]["response"]["status"], "403 Forbidden",
        "{denied}"
    );
    let deleted = scoped_batch(
        client,
        fhir,
        delete,
        json!([
            {"request":{"method":"DELETE","url":"Observation/batch-compartment"}},
            {"request":{"method":"DELETE","url":"Observation/batch-foreign"}}
        ]),
    )
    .await;
    assert_eq!(
        deleted["entry"][0]["response"]["status"], "204 No Content",
        "{deleted}"
    );
    assert_eq!(
        deleted["entry"][1]["response"]["status"], "404 Not Found",
        "{deleted}"
    );
    let current: Value = client
        .get(format!("{fhir}/Observation/batch-foreign"))
        .bearer_auth(full)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(current, hidden["entry"][0]["resource"]);
}

async fn conditional_mutation_policies(
    client: &Client,
    fhir: &str,
    full: &str,
    update: &str,
    delete: &str,
) {
    let mut originals = Vec::new();
    for (name, active) in [("allowed", true), ("protected", false)] {
        let response = client
            .post(format!("{fhir}/Patient"))
            .bearer_auth(full)
            .json(&json!({"resourceType":"Patient", "active":active,
                "identifier":[{"system":"urn:conditional-policy", "value":name}]}))
            .send()
            .await
            .unwrap();
        let status = response.status();
        let original: Value = response.json().await.unwrap();
        assert_eq!(status, StatusCode::CREATED, "{original}");
        originals.push(original);
    }
    let base = fhir.strip_suffix("/fhir").unwrap();
    let script = r#"
if (resource && resource.resource.identifier && resource.resource.identifier[0].system === 'urn:conditional-policy') {
    if (resource.resource.active === false) return deny('protected previous or resulting Patient');
    if (request.method !== 'DELETE' && (!request.body || request.resourceId !== resource.resource.id || request.body.id !== resource.resource.id)) return deny('missing resolved write context');
}
return abstain();
"#;
    let response = client.post(format!("{base}/AccessPolicy")).bearer_auth(full)
        .json(&json!({"resourceType":"AccessPolicy", "name":"Conditional mutation context",
            "active":true,"priority":0,"matcher":{"resourceTypes":["Patient"],"operations":["update","patch","delete"]},
            "engine":{"type":"quickjs","script":script}}))
        .send().await.unwrap();
    let status = response.status();
    let policy: Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::CREATED, "{policy}");
    let conditional = |name| format!("{fhir}/Patient?identifier=urn:conditional-policy%7C{name}");
    let mut loaded = false;
    for _ in 0..100 {
        // Invalid patches never commit while the policy cache reloads.
        let response = client
            .patch(conditional("protected"))
            .bearer_auth(update)
            .header("Content-Type", "application/json-patch+json")
            .json(&json!([{"op":"invalid","path":"/active"}]))
            .send()
            .await
            .unwrap();
        if response.status() == StatusCode::FORBIDDEN {
            loaded = true;
            break;
        }
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert!(loaded, "conditional mutation policy did not reload");
    let mut histories = Vec::new();
    for original in &originals {
        let history: Value = client
            .get(format!(
                "{fhir}/Patient/{}/_history",
                original["id"].as_str().unwrap()
            ))
            .bearer_auth(full)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        histories.push(history);
    }
    for name in ["allowed", "protected"] {
        let active = name == "protected";
        let original = &originals[usize::from(name == "protected")];
        let id = original["id"].as_str().unwrap();
        for method in ["PUT", "PATCH"] {
            let resource = if method == "PUT" {
                json!({"resourceType":"Patient","id":id,"active":active,"identifier":[{"system":"urn:conditional-policy","value":name}]})
            } else {
                json!({"resourceType":"Parameters","parameter":[{"name":"operation","part":[
                    {"name":"type","valueCode":"replace"},{"name":"path","valueString":"Patient.active"},{"name":"value","valueBoolean":active}
                ]}]})
            };
            Box::pin(scoped_transaction(
                client,
                fhir,
                update,
                json!([
                    {"request":{"method":method,"url":format!("Patient/{id}")},"resource":resource}
                ]),
                StatusCode::FORBIDDEN,
                false,
            ))
            .await;
        }
        Box::pin(scoped_transaction(client, fhir, update, json!([
            {"request":{"method":"PUT","url":format!("Patient?identifier=urn:conditional-policy%7C{name}")},
             "resource":{"resourceType":"Patient","active":active,"identifier":[{"system":"urn:conditional-policy","value":name}]}}
        ]), StatusCode::FORBIDDEN, false)).await;
        let batch = Box::pin(scoped_batch(client, fhir, update, json!([
            {"request":{"method":"PUT","url":format!("Patient?identifier=urn:conditional-policy%7C{name}")},
             "resource":{"resourceType":"Patient", "active":active,"identifier":[{"system":"urn:conditional-policy","value":name}]}},
            {"request":{"method":"PATCH","url":format!("Patient?identifier=urn:conditional-policy%7C{name}")},
             "resource":{"resourceType":"Parameters","parameter":[{"name":"operation","part":[
                 {"name":"type","valueCode":"replace"},{"name":"path","valueString":"Patient.active"},{"name":"value","valueBoolean":active}
             ]}]}}
        ]))).await;
        for entry in batch["entry"].as_array().unwrap() {
            assert_eq!(entry["response"]["status"], "403 Forbidden", "{batch}");
        }
        let response = client
            .put(conditional(name))
            .bearer_auth(update)
            .json(&json!({"resourceType":"Patient", "active":active,
                "identifier":[{"system":"urn:conditional-policy", "value":name}]}))
            .send()
            .await
            .unwrap();
        let status = response.status();
        let outcome: Value = response.json().await.unwrap();
        assert_eq!(status, StatusCode::FORBIDDEN, "{name}: {outcome}");
        let response = client
            .patch(conditional(name))
            .bearer_auth(update)
            .header("Content-Type", "application/json-patch+json")
            .json(&json!([{"op":"replace","path":"/active","value":active}]))
            .send()
            .await
            .unwrap();
        let status = response.status();
        let outcome: Value = response.json().await.unwrap();
        assert_eq!(status, StatusCode::FORBIDDEN, "{name}: {outcome}");
    }
    assert_eq!(
        client
            .delete(conditional("protected"))
            .bearer_auth(delete)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    Box::pin(scoped_transaction(client, fhir, delete, json!([
        {"request":{"method":"DELETE","url":format!("Patient/{}", originals[1]["id"].as_str().unwrap())}}
    ]), StatusCode::FORBIDDEN, true)).await;
    Box::pin(scoped_transaction(client, fhir, delete, json!([
        {"request":{"method":"DELETE","url":"Patient?identifier=urn:conditional-policy%7Cprotected"}}
    ]), StatusCode::FORBIDDEN, true)).await;
    let response = client
        .put(conditional("created-denied"))
        .bearer_auth(update)
        .json(&json!({"resourceType":"Patient", "active":false,
            "identifier":[{"system":"urn:conditional-policy", "value":"created-denied"}]}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let count: Value = client
        .get(format!("{}&_summary=count", conditional("created-denied")))
        .bearer_auth(full)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(count["total"], 0);
    Box::pin(scoped_transaction(client, fhir, update, json!([
        {"request":{"method":"PUT","url":"Patient?identifier=urn:conditional-policy%7Ctransaction-denied"},
         "resource":{"resourceType":"Patient","active":false,"identifier":[{"system":"urn:conditional-policy","value":"transaction-denied"}]}}
    ]), StatusCode::FORBIDDEN, true)).await;
    for (original, history) in originals.iter().zip(histories) {
        let direct = format!("{fhir}/Patient/{}", original["id"].as_str().unwrap());
        let current: Value = client
            .get(&direct)
            .bearer_auth(full)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(&current, original);
        let current_history: Value = client
            .get(format!("{direct}/_history"))
            .bearer_auth(full)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(current_history, history);
    }
    // Policies requiring a complete proposed body must allow both conditional
    // patch previews and the final locked write, without any read scope.
    let transaction = Box::pin(scoped_transaction(client, fhir, update, json!([
        {"request":{"method":"PUT","url":"Patient?identifier=urn:conditional-policy%7Callowed"},
         "resource":{"resourceType":"Patient","active":true,"identifier":[{"system":"urn:conditional-policy","value":"allowed"}]}}
    ]), StatusCode::OK, false)).await;
    assert_eq!(
        transaction["entry"][0]["response"]["status"], "200 OK",
        "{transaction}"
    );
    let batch = Box::pin(scoped_batch(client, fhir, update, json!([
        {"request":{"method":"PATCH","url":"Patient?identifier=urn:conditional-policy%7Callowed"},
         "resource":{"resourceType":"Parameters","parameter":[{"name":"operation","part":[
             {"name":"type","valueCode":"add"},{"name":"path","valueString":"Patient"},{"name":"name","valueString":"birthDate"},{"name":"value","valueDate":"2000-01-01"}
         ]}]}}
    ]))).await;
    assert_eq!(batch["entry"][0]["response"]["status"], "200 OK", "{batch}");
    for body in [
        json!([]),
        json!([{"op":"add","path":"/birthDate","value":"2000-01-01"}]),
    ] {
        let response = client
            .patch(conditional("allowed"))
            .bearer_auth(update)
            .header("Content-Type", "application/json-patch+json")
            .json(&body)
            .send()
            .await
            .unwrap();
        let status = response.status();
        let result: Value = response.json().await.unwrap();
        assert_eq!(status, StatusCode::OK, "{result}");
    }
    let response = client
        .put(conditional("created-allowed"))
        .bearer_auth(update)
        .json(&json!({"resourceType":"Patient", "active":true,
            "identifier":[{"system":"urn:conditional-policy", "value":"created-allowed"}]}))
        .send()
        .await
        .unwrap();
    let status = response.status();
    let result: Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::CREATED, "{result}");
    assert_eq!(
        client
            .delete(format!(
                "{base}/AccessPolicy/{}",
                policy["id"].as_str().unwrap()
            ))
            .bearer_auth(full)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NO_CONTENT
    );
}

async fn conditional_patient_mutations(
    client: &Client,
    fhir: &str,
    full: &str,
    update: &str,
    delete: &str,
) {
    let content = |value: &str| {
        json!({"resourceType":"Patient", "active":true,
        "identifier":[{"system":"urn:conditional-update", "value":value}]})
    };
    let url = format!("{fhir}/Patient?identifier=urn:conditional-update%7Csingle");
    let response = client
        .put(&url)
        .bearer_auth(update)
        .json(&content("single"))
        .send()
        .await
        .unwrap();
    let status = response.status();
    let first: Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::CREATED, "{first}");
    let id = first["id"].as_str().unwrap();
    let direct = format!("{fhir}/Patient/{id}");
    assert_eq!(
        client
            .get(&direct)
            .bearer_auth(update)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    let response = client
        .put(&url)
        .bearer_auth(update)
        .json(&content("single"))
        .send()
        .await
        .unwrap();
    let status = response.status();
    let second: Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::OK, "{second}");
    assert_eq!(second["id"], first["id"]);
    let before: Value = client
        .get(format!("{direct}/_history"))
        .bearer_auth(full)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let mut wrong = content("single");
    wrong["id"] = json!("conditional-mismatched-id");
    let response = client
        .put(&url)
        .bearer_auth(update)
        .json(&wrong)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    for (body, expected) in [
        (
            json!({"resourceType":"Patient", "active":false}),
            StatusCode::FORBIDDEN,
        ),
        (
            json!({"resourceType":"Patient", "active":true, "managingOrganization":{"reference":"Organization/conditional-absent"}}),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
    ] {
        let response = client
            .put(&url)
            .bearer_auth(update)
            .json(&body)
            .send()
            .await
            .unwrap();
        let status = response.status();
        let outcome: Value = response.json().await.unwrap();
        assert_eq!(status, expected, "{outcome}");
    }
    for header in ["42", "*", "\"1\", \"2\""] {
        for method in [reqwest::Method::PUT, reqwest::Method::DELETE] {
            let credential = if method == reqwest::Method::PUT {
                update
            } else {
                delete
            };
            assert_eq!(
                client
                    .request(method, &url)
                    .bearer_auth(credential)
                    .header("If-Match", header)
                    .json(&content("single"))
                    .send()
                    .await
                    .unwrap()
                    .status(),
                StatusCode::BAD_REQUEST
            );
        }
    }
    assert_eq!(
        client
            .delete(&url)
            .bearer_auth(delete)
            .header("If-Match", "W/\"stale\"")
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::PRECONDITION_FAILED
    );
    let after: Value = client
        .get(format!("{direct}/_history"))
        .bearer_auth(full)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(after["entry"], before["entry"]);
    let response = client
        .put(format!(
            "{fhir}/Patient?identifier=urn:conditional-update%7Cexplicit"
        ))
        .bearer_auth(update)
        .json(
            &json!({"resourceType":"Patient", "id":"conditional-explicit", "active":true,
            "identifier":[{"system":"urn:conditional-update", "value":"explicit"}]}),
        )
        .send()
        .await
        .unwrap();
    let status = response.status();
    let explicit: Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::CREATED, "{explicit}");
    assert_eq!(explicit["id"], "conditional-explicit");
    // R4's zero-match/body-ID rule delegates to ordinary Update as Create,
    // including updating a visible ID that does not match the search criterion.
    let response = client
        .put(format!(
            "{fhir}/Patient?identifier=urn:conditional-update%7Cno-match"
        ))
        .bearer_auth(update)
        .json(&explicit)
        .send()
        .await
        .unwrap();
    let status = response.status();
    let updated: Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["id"], explicit["id"]);
    let response = client
        .put(format!(
            "{fhir}/Patient?identifier=urn:conditional-update%7Cexplicit"
        ))
        .bearer_auth(update)
        .json(&updated)
        .send()
        .await
        .unwrap();
    let status = response.status();
    let matched: Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::OK, "{matched}");
    assert_eq!(matched["id"], explicit["id"]);
    let response = client
        .put(format!(
            "{fhir}/Patient?identifier=urn:conditional-update%7Chidden"
        ))
        .bearer_auth(full)
        .json(&json!({"resourceType":"Patient", "id":"conditional-hidden", "active":false}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let hidden: Value = response.json().await.unwrap();
    let response = client
        .put(format!(
            "{fhir}/Patient?identifier=urn:conditional-update%7Cdoes-not-match"
        ))
        .bearer_auth(update)
        .json(&json!({"resourceType":"Patient", "id":"conditional-hidden", "active":true}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let still_hidden: Value = client
        .get(format!("{fhir}/Patient/conditional-hidden"))
        .bearer_auth(full)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(still_hidden, hidden);

    for patch in [
        json!([{"op":"replace", "path":"/active", "value":false}]),
        json!([{"op":"add", "path":"/managingOrganization", "value":{"reference":"Organization/conditional-absent"}}]),
    ] {
        let response = client
            .patch(&url)
            .bearer_auth(update)
            .header("Content-Type", "application/json-patch+json")
            .json(&patch)
            .send()
            .await
            .unwrap();
        let status = response.status();
        let outcome: Value = response.json().await.unwrap();
        assert_eq!(
            status,
            if patch[0]["path"] == "/active" {
                StatusCode::FORBIDDEN
            } else {
                StatusCode::UNPROCESSABLE_ENTITY
            },
            "{outcome}"
        );
    }
    assert_eq!(
        client
            .patch(&url)
            .bearer_auth(update)
            .header("Content-Type", "application/json-patch+json-invalid")
            .json(&json!([]))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNSUPPORTED_MEDIA_TYPE
    );
    assert_eq!(
        client
            .patch(format!(
                "{fhir}/Patient?identifier=urn:conditional-update%7Cabsent"
            ))
            .bearer_auth(update)
            .header("Content-Type", "application/json-patch+json")
            .json(&json!([]))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
    let after: Value = client
        .get(format!("{direct}/_history"))
        .bearer_auth(full)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(after["entry"], before["entry"]);
    let etag = format!("W/\"{}\"", second["meta"]["versionId"].as_str().unwrap());
    let requests = ["2001-01-01", "2002-01-01"].map(|birth| {
        client
            .patch(&url)
            .bearer_auth(update)
            .header("Content-Type", "application/json-patch+json; charset=utf-8")
            .header("If-Match", &etag)
            .json(&json!([{"op":"add", "path":"/birthDate", "value":birth}]))
            .send()
    });
    let [first, second] = requests;
    let (first, second) = tokio::join!(first, second);
    let statuses = [first.unwrap().status(), second.unwrap().status()];
    assert_eq!(statuses.iter().filter(|&&s| s == StatusCode::OK).count(), 1);
    assert_eq!(
        statuses
            .iter()
            .filter(|&&s| s == StatusCode::PRECONDITION_FAILED)
            .count(),
        1
    );
    let response = client
        .patch(&url)
        .bearer_auth(update)
        .header("Content-Type", "application/fhir+json")
        .json(
            &json!({"resourceType":"Parameters","parameter":[{"name":"operation","part":[
            {"name":"type","valueCode":"replace"},{"name":"path","valueString":"Patient.birthDate"},
            {"name":"value","valueDate":"1990-01-01"}]}]}),
        )
        .send()
        .await
        .unwrap();
    let status = response.status();
    let patched: Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::OK, "{patched}");
    assert_eq!(patched["birthDate"], "1990-01-01");
    let response = client
        .put(&url)
        .bearer_auth(update)
        .header("Prefer", "return=minimal")
        .json(&content("single"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let etag = response.headers()["ETag"].clone();
    assert!(response.bytes().await.unwrap().is_empty());
    assert_eq!(
        client
            .delete(&url)
            .bearer_auth(delete)
            .header("If-Match", etag)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        client
            .delete(&url)
            .bearer_auth(delete)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NO_CONTENT
    );

    for _ in 0..2 {
        assert_eq!(
            client
                .post(format!("{fhir}/Patient"))
                .bearer_auth(full)
                .json(&content("multiple"))
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::CREATED
        );
    }
    let multiple =
        format!("{fhir}/Patient?identifier=urn:conditional-update%7Cmultiple&_count=1&_offset=100");
    assert_eq!(
        client
            .put(&multiple)
            .bearer_auth(update)
            .json(&content("multiple"))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::PRECONDITION_FAILED
    );
    assert_eq!(
        client
            .patch(&multiple)
            .bearer_auth(update)
            .header("Content-Type", "application/json-patch+json")
            .json(&json!([]))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::PRECONDITION_FAILED
    );
    assert_eq!(
        client
            .delete(&multiple)
            .bearer_auth(delete)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::PRECONDITION_FAILED
    );
    let mut tasks = Vec::new();
    for _ in 0..4 {
        let request = client
            .put(format!(
                "{fhir}/Patient?identifier=urn:conditional-update%7Crace"
            ))
            .bearer_auth(update)
            .json(&content("race"));
        tasks.push(tokio::spawn(async move { request.send().await.unwrap() }));
    }
    let mut created = 0;
    let mut ids = std::collections::HashSet::new();
    for task in tasks {
        let response = task.await.unwrap();
        let status = response.status();
        let resource: Value = response.json().await.unwrap();
        assert!(
            matches!(status, StatusCode::OK | StatusCode::CREATED),
            "{status}: {resource}"
        );
        created += usize::from(status == StatusCode::CREATED);
        ids.insert(resource["id"].clone().to_string());
    }
    assert_eq!(created, 1);
    assert_eq!(ids.len(), 1);
}

async fn conditional_patient_compartment_mutations(
    client: &Client,
    fhir: &str,
    full: &str,
    credentials: (&str, &str),
    patients: (&str, &str),
) {
    let (update, delete) = credentials;
    let (patient, foreign_patient) = patients;
    let make = |value: &str, subject: &str| {
        json!({"resourceType":"Observation", "status":"final",
        "code":{"coding":[{"code":"atomic-write"}]}, "subject":{"reference":format!("Patient/{subject}")},
        "identifier":[{"system":"urn:conditional-compartment","value":value}]})
    };
    let url = format!("{fhir}/Observation?identifier=urn:conditional-compartment%7Cown");
    let response = client
        .put(&url)
        .bearer_auth(update)
        .json(&make("own", patient))
        .send()
        .await
        .unwrap();
    let status = response.status();
    let own: Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::CREATED, "{own}");
    let direct = format!("{fhir}/Observation/{}", own["id"].as_str().unwrap());
    let before: Value = client
        .get(format!("{direct}/_history"))
        .bearer_auth(full)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let response = client
        .put(&url)
        .bearer_auth(update)
        .json(&make("own", foreign_patient))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let response = client.patch(&url).bearer_auth(update).header("Content-Type", "application/json-patch+json")
        .json(&json!([{"op":"replace","path":"/subject/reference","value":format!("Patient/{foreign_patient}")}]))
        .send().await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let after: Value = client
        .get(format!("{direct}/_history"))
        .bearer_auth(full)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(before, after);
    let foreign = make("hidden", foreign_patient);
    let response = client
        .post(format!("{fhir}/Observation"))
        .bearer_auth(full)
        .json(&foreign)
        .send()
        .await
        .unwrap();
    let status = response.status();
    let hidden: Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::CREATED, "{hidden}");
    let hidden_url = format!("{fhir}/Observation?identifier=urn:conditional-compartment%7Chidden");
    assert_eq!(
        client
            .patch(&hidden_url)
            .bearer_auth(update)
            .header("Content-Type", "application/json-patch+json")
            .json(&json!([]))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        client
            .delete(&hidden_url)
            .bearer_auth(delete)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NO_CONTENT
    );
    let response = client
        .put(&hidden_url)
        .bearer_auth(update)
        .json(&foreign)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let current: Value = client
        .get(format!(
            "{fhir}/Observation/{}",
            hidden["id"].as_str().unwrap()
        ))
        .bearer_auth(full)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(current, hidden);
    assert_eq!(
        client
            .delete(&url)
            .bearer_auth(delete)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NO_CONTENT
    );
}

async fn conditional_chain_mutation_races(
    client: &Client,
    fhir: &str,
    full: &str,
    grants: (&str, &str),
    database: &str,
) {
    use sqlx_core::{query::query, query_scalar::query_scalar};
    let pool = sqlx_postgres::PgPoolOptions::new()
        .max_connections(3)
        .connect(database)
        .await
        .unwrap();
    for (index, (method, transaction)) in [
        (reqwest::Method::PUT, false),
        (reqwest::Method::PATCH, false),
        (reqwest::Method::DELETE, false),
        (reqwest::Method::PUT, true),
        (reqwest::Method::DELETE, true),
    ]
    .into_iter()
    .enumerate()
    {
        let response = client
            .post(format!("{fhir}/Patient"))
            .bearer_auth(full)
            .json(&json!({"resourceType":"Patient", "active":true}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        let patient: Value = response.json().await.unwrap();
        let patient_id = patient["id"].as_str().unwrap();
        let content = json!({"resourceType":"Observation", "status":"final", "code":{"coding":[{"code":"conditional-chain"}]},
            "identifier":[{"system":"urn:conditional-chain","value":index.to_string()}], "subject":{"reference":format!("Patient/{patient_id}")}});
        let response = client
            .post(format!("{fhir}/Observation"))
            .bearer_auth(full)
            .json(&content)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        let original: Value = response.json().await.unwrap();
        let direct = format!("{fhir}/Observation/{}", original["id"].as_str().unwrap());
        let before: Value = client
            .get(format!("{direct}/_history"))
            .bearer_auth(full)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let mut blocker = pool.begin().await.unwrap();
        query("SELECT pg_advisory_xact_lock(1330006098, 1)")
            .execute(&mut *blocker)
            .await
            .unwrap();
        let credential = if method == reqwest::Method::DELETE {
            grants.1
        } else {
            grants.0
        };
        let mut request = client.request(method.clone(), format!("{fhir}/Observation?identifier=urn:conditional-chain%7C{index}&subject:Patient.active=true"))
            .bearer_auth(credential);
        if method == reqwest::Method::PATCH {
            request = request
                .header("Content-Type", "application/json-patch+json")
                .json(&json!([{"op":"replace","path":"/status","value":"amended"}]));
        } else if method == reqwest::Method::PUT {
            request = request.json(&content);
        }
        if transaction {
            let mut entry = json!({"request":{"method":method.as_str(),"url":format!("Observation?identifier=urn:conditional-chain%7C{index}&subject:Patient.active=true")}});
            if method == reqwest::Method::PUT {
                entry["resource"] = content.clone();
            }
            request = client
                .post(fhir)
                .bearer_auth(credential)
                .json(&json!({"resourceType":"Bundle","type":"transaction","entry":[entry]}));
        }
        let pending = tokio::spawn(async move { request.send().await.unwrap() });
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            loop {
                let waiting: i64 = query_scalar("SELECT count(*) FROM pg_stat_activity WHERE wait_event_type='Lock' AND query LIKE '%pg_advisory_xact_lock%'")
                    .fetch_one(&pool).await.unwrap();
                if waiting > 0 { break; }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        }).await.expect("conditional resolution must wait at the write gate before selecting its target");
        query("UPDATE patient SET txid=nextval('_transaction_txid_seq'), resource=jsonb_set(resource, '{active}', 'false'::jsonb), status='updated' WHERE id=$1")
            .bind(patient_id).execute(&mut *blocker).await.unwrap();
        blocker.commit().await.unwrap();
        let response = tokio::time::timeout(std::time::Duration::from_secs(10), pending)
            .await
            .unwrap()
            .unwrap();
        let expected = if method == reqwest::Method::PUT {
            StatusCode::CREATED
        } else if method == reqwest::Method::PATCH {
            StatusCode::NOT_FOUND
        } else {
            StatusCode::NO_CONTENT
        };
        let status = response.status();
        let bytes = response.bytes().await.unwrap();
        if transaction {
            assert_eq!(
                status,
                StatusCode::OK,
                "{method}: {}",
                String::from_utf8_lossy(&bytes)
            );
            let body: Value = serde_json::from_slice(&bytes).unwrap();
            let entry_status = if method == reqwest::Method::PUT {
                "201 Created"
            } else {
                "204 No Content"
            };
            assert_eq!(
                body["entry"][0]["response"]["status"], entry_status,
                "{body}"
            );
        } else {
            assert_eq!(
                status,
                expected,
                "{method}: {}",
                String::from_utf8_lossy(&bytes)
            );
        }
        let current: Value = client
            .get(&direct)
            .bearer_auth(full)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(
            current, original,
            "condition stopped matching before resolution; previous target must be untouched"
        );
        let after: Value = client
            .get(format!("{direct}/_history"))
            .bearer_auth(full)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(after["entry"], before["entry"]);
    }
    pool.close().await;
}

async fn snapshot_read_regressions(client: &Client, base: &str, fhir: &str, full: &str) {
    let original: Value = client
        .post(format!("{fhir}/Patient"))
        .bearer_auth(full)
        .json(
            &json!({"resourceType":"Patient","active":true,"name":[{"family":"SnapshotAllowed"}]}),
        )
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let id = original["id"].as_str().unwrap();
    let old_version = original["meta"]["versionId"].as_str().unwrap();
    let url = format!("{fhir}/Patient/{id}");
    let mut changed = original.clone();
    changed["name"][0]["family"] = json!("SnapshotHidden");
    let changed: Value = client
        .put(&url)
        .bearer_auth(full)
        .json(&changed)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let current_version = changed["meta"]["versionId"].as_str().unwrap();
    // Populate the shared cache with a snapshot that the narrow reader cannot see.
    let cached: Value = client
        .get(&url)
        .bearer_auth(full)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(cached, changed);
    let allowed = token(base, "system/Patient.r?family=SnapshotAllowed").await;
    let hidden = token(base, "system/Patient.r?family=SnapshotHidden").await;
    for (path, credential, expected, snapshot) in [
        (
            format!("Patient/{id}"),
            &allowed,
            StatusCode::NOT_FOUND,
            None,
        ),
        (
            format!("Patient/{id}/_history/{old_version}"),
            &allowed,
            StatusCode::OK,
            Some(&original),
        ),
        (
            format!("Patient/{id}/_history/{current_version}"),
            &allowed,
            StatusCode::NOT_FOUND,
            None,
        ),
        (
            format!("Patient/{id}"),
            &hidden,
            StatusCode::OK,
            Some(&changed),
        ),
        (
            format!("Patient/{id}/_history/{old_version}"),
            &hidden,
            StatusCode::NOT_FOUND,
            None,
        ),
    ] {
        let response = client
            .get(format!("{fhir}/{path}"))
            .bearer_auth(credential)
            .send()
            .await
            .unwrap();
        let status = response.status();
        let body: Value = response.json().await.unwrap();
        assert_eq!(status, expected, "{path}: {body}");
        if let Some(snapshot) = snapshot {
            assert_eq!(body, *snapshot);
        }
    }
    for (credential, expected) in [
        (&allowed, StatusCode::NOT_FOUND),
        (&hidden, StatusCode::NOT_MODIFIED),
    ] {
        let response = client
            .get(format!("{url}?_elements=id"))
            .bearer_auth(credential)
            .header("If-None-Match", format!("W/\"{current_version}\""))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
    }
    let allowed_search = token(base, "system/Patient.s?family=SnapshotAllowed").await;
    for (path, credential) in [
        (format!("Patient/{id}/_history"), &allowed),
        ("Patient/_history".into(), &allowed_search),
        ("_history".into(), &allowed_search),
    ] {
        let response = client
            .get(format!("{fhir}/{path}?_total=accurate&_count=1"))
            .bearer_auth(credential)
            .send()
            .await
            .unwrap();
        let status = response.status();
        let body: Value = response.json().await.unwrap();
        assert_eq!(status, StatusCode::OK, "{path}: {body}");
        assert_eq!(body["total"], 1, "{path}: {body}");
        assert_eq!(body["entry"][0]["resource"], original, "{path}: {body}");
        assert!(
            body["entry"][0]["fullUrl"]
                .as_str()
                .unwrap()
                .contains("/fhir/Patient/"),
            "{body}"
        );
        let response = client
            .get(format!("{fhir}/{path}?_count=0"))
            .bearer_auth(credential)
            .send()
            .await
            .unwrap();
        let status = response.status();
        let body: Value = response.json().await.unwrap();
        assert_eq!(status, StatusCode::OK, "{path}: {body}");
        assert_eq!(body["total"], 1, "{path}: {body}");
        assert!(body["entry"].as_array().is_none_or(Vec::is_empty), "{body}");
    }
    for bundle_type in ["batch", "transaction"] {
        let entries = json!([
            {"request":{"method":"GET","url":format!("Patient/{id}/_history/{old_version}")}},
            {"request":{"method":"GET","url":format!("Patient/{id}/_history/{old_version}?_elements=id")}},
            {"request":{"method":"GET","url":format!("Patient/{id}/_history/{old_version}"),"ifNoneMatch":format!("W/\"{old_version}\"")}},
            {"request":{"method":"HEAD","url":format!("Patient/{id}/_history/{old_version}")}}
        ]);
        let response = client
            .post(fhir)
            .bearer_auth(&allowed)
            .json(&json!({"resourceType":"Bundle","type":bundle_type,"entry":entries}))
            .send()
            .await
            .unwrap();
        let status = response.status();
        let body: Value = response.json().await.unwrap();
        assert_eq!(status, StatusCode::OK, "{bundle_type}: {body}");
        assert_eq!(body["entry"][0]["resource"], original);
        assert_eq!(body["entry"][1]["resource"]["id"], id);
        assert!(body["entry"][1]["resource"].get("name").is_none(), "{body}");
        assert_eq!(body["entry"][2]["response"]["status"], "304 Not Modified");
        assert!(body["entry"][2].get("resource").is_none(), "{body}");
        assert_eq!(body["entry"][3]["response"]["status"], "200 OK");
        assert!(body["entry"][3].get("resource").is_none(), "{body}");
        let response = client.post(fhir).bearer_auth(&allowed)
            .json(&json!({"resourceType":"Bundle","type":bundle_type,"entry":[
                {"request":{"method":"GET","url":format!("Patient/{id}?_elements=id"),"ifNoneMatch":format!("W/\"{current_version}\"")}}
            ]})).send().await.unwrap();
        let status = response.status();
        let body: Value = response.json().await.unwrap();
        if bundle_type == "transaction" {
            assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
        } else {
            assert_eq!(
                body["entry"][0]["response"]["status"], "404 Not Found",
                "{body}"
            );
        }
    }
    let page: Value = client
        .get(format!("{url}/_history"))
        .query(&[
            ("_count", "1"),
            ("_since", original["meta"]["lastUpdated"].as_str().unwrap()),
        ])
        .bearer_auth(full)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(page.get("total").is_none(), "{page}");
    let next = page["link"]
        .as_array()
        .unwrap()
        .iter()
        .find(|link| link["relation"] == "next")
        .unwrap()["url"]
        .as_str()
        .unwrap();
    assert!(
        url::Url::parse(next)
            .unwrap()
            .query_pairs()
            .any(|(name, value)| name == "_since"
                && value == original["meta"]["lastUpdated"].as_str().unwrap()),
        "{page}"
    );
    let next_url = url::Url::parse(next).unwrap();
    assert_eq!(next_url.path(), format!("/fhir/Patient/{id}/_history"));
    let next_page: Value = client
        .get(format!(
            "{base}{}?{}",
            next_url.path(),
            next_url.query().unwrap()
        ))
        .bearer_auth(full)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(next_page["entry"][0]["resource"], original);
    let historical: Value = client
        .get(format!("{fhir}/Patient/_history"))
        .query(&[
            ("_at", changed["meta"]["lastUpdated"].as_str().unwrap()),
            ("_total", "accurate"),
        ])
        .bearer_auth(&allowed_search)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(historical["total"], 0, "{historical}");
    let deleted = client.delete(&url).bearer_auth(full).send().await.unwrap();
    assert_eq!(deleted.status(), StatusCode::NO_CONTENT);
    let deletion_version = deleted.headers()["etag"]
        .to_str()
        .unwrap()
        .trim_start_matches("W/\"")
        .trim_end_matches('"')
        .to_owned();
    assert_ne!(deletion_version, current_version);
    assert!(deleted.headers().contains_key("last-modified"));
    for credential in [&allowed, &hidden] {
        for path in [url.clone(), format!("{url}/_history/{deletion_version}")] {
            let response = client
                .get(&path)
                .bearer_auth(credential)
                .send()
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                if credential == &allowed {
                    StatusCode::NOT_FOUND
                } else {
                    StatusCode::GONE
                },
                "{path}"
            );
        }
        for bundle_type in ["batch", "transaction"] {
            let response = client.post(fhir).bearer_auth(credential).json(&json!({"resourceType":"Bundle", "type":bundle_type, "entry":[{"request":{"method":"GET","url":format!("Patient/{id}/_history/{deletion_version}")}}]})).send().await.unwrap();
            let status = response.status();
            let body: Value = response.json().await.unwrap();
            let expected = if credential == &allowed {
                StatusCode::NOT_FOUND
            } else {
                StatusCode::GONE
            };
            if bundle_type == "transaction" {
                assert_eq!(status, expected, "{body}");
            } else {
                assert_eq!(
                    body["entry"][0]["response"]["status"],
                    format!(
                        "{} {}",
                        expected.as_u16(),
                        expected.canonical_reason().unwrap()
                    ),
                    "{body}"
                );
            }
        }
    }
    let after: Value = client
        .get(format!("{url}/_history?_total=accurate"))
        .bearer_auth(full)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(after["total"], 3, "{after}");
    assert_eq!(after["entry"][0]["request"]["method"], "DELETE");
    assert!(after["entry"][0].get("resource").is_none(), "{after}");
    let repeated = client.delete(&url).bearer_auth(full).send().await.unwrap();
    assert_eq!(repeated.status(), StatusCode::NO_CONTENT);
    assert!(!repeated.headers().contains_key("etag"));
    let historical: Value = client
        .get(format!("{url}/_history/{old_version}"))
        .bearer_auth(&allowed)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(historical, original);
}

async fn snapshot_search_policy_regressions(client: &Client, base: &str, fhir: &str, full: &str) {
    use octofhir_auth::smart::scopes::FhirOperation;
    let suffix = uuid::Uuid::new_v4();
    let a = format!("snapshot-search-a-{suffix}");
    let b = format!("snapshot-search-b-{suffix}");
    let c = format!("snapshot-search-c-{suffix}");
    let root = format!("snapshot-root-{suffix}");
    let bridge = format!("snapshot-bridge-{suffix}");
    let tail = format!("snapshot-tail-{suffix}");
    let mut saved = std::collections::HashMap::new();
    for resource in [
        json!({"resourceType":"Patient","id":a,"active":true}),
        json!({"resourceType":"Patient","id":b,"active":false,"link":[{"type":"seealso","other":{"reference":format!("Patient/{a}")}}]}),
        json!({"resourceType":"Patient","id":c,"active":true,"link":[{"type":"seealso","other":{"reference":format!("Patient/{a}")}}]}),
        json!({"resourceType":"Observation","id":tail,"status":"final","code":{"text":"snapshot policy"},"subject":{"reference":format!("Patient/{a}")}}),
        json!({"resourceType":"Observation","id":bridge,"status":"preliminary","code":{"text":"snapshot policy"},"subject":{"reference":format!("Patient/{a}")},"hasMember":[{"reference":format!("Observation/{tail}")}]}),
        json!({"resourceType":"Observation","id":root,"status":"final","code":{"text":"snapshot policy"},"subject":{"reference":format!("Patient/{a}")},"hasMember":[{"reference":format!("Observation/{bridge}")}]}),
    ] {
        let response = client
            .put(format!(
                "{fhir}/{}/{}",
                resource["resourceType"].as_str().unwrap(),
                resource["id"].as_str().unwrap()
            ))
            .bearer_auth(full)
            .json(&resource)
            .send()
            .await
            .unwrap();
        let status = response.status();
        let body: Value = response.json().await.unwrap();
        assert_eq!(status, StatusCode::CREATED, "{body}");
        saved.insert(body["id"].as_str().unwrap().to_owned(), body);
    }
    let ids = format!("{a},{b},{c}");
    let scoped = token(
        base,
        &format!("system/Patient.s?_id={ids} system/Observation.s"),
    )
    .await;
    let operations = [
        FhirOperation::Search,
        FhirOperation::SearchType,
        FhirOperation::SearchSystem,
        FhirOperation::Read,
        FhirOperation::VRead,
        FhirOperation::HistoryInstance,
        FhirOperation::HistoryType,
        FhirOperation::HistorySystem,
    ]
    .map(|operation| operation.to_string());
    let response = client.post(format!("{base}/AccessPolicy")).bearer_auth(full).json(&json!({
        "resourceType":"AccessPolicy","name":"Actual search snapshot","active":true,"priority":0,
        "matcher":{"resourceTypes":["Patient","Observation"],"operations":operations},
        "engine":{"type":"quickjs","script":"if (resource && (resource.resource.active === false || resource.resource.status === 'preliminary')) return deny('Hidden snapshot'); return abstain();"}
    })).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let policy: Value = response.json().await.unwrap();
    let query = format!("_id={ids}&_sort=-_id&_count=1&_offset=1&_total=accurate");
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let response = client
                .get(format!("{fhir}/Patient?{query}"))
                .bearer_auth(&scoped)
                .send()
                .await
                .unwrap();
            let status = response.status();
            let body: Value = response.json().await.unwrap();
            assert_eq!(status, StatusCode::OK, "{body}");
            if body["total"] == 2 {
                break;
            }
            assert_eq!(body["total"], 3, "{body}");
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap();
    for path in [
        "Patient".to_owned(),
        "?_type=Patient".to_owned(),
        format!("Patient/{a}/Patient"),
    ] {
        let separator = if path.contains('?') { '&' } else { '?' };
        let endpoint = if path.starts_with('?') {
            format!("{fhir}{path}")
        } else {
            format!("{fhir}/{path}")
        };
        let response = client
            .get(format!("{endpoint}{separator}{query}"))
            .bearer_auth(&scoped)
            .send()
            .await
            .unwrap();
        let status = response.status();
        let body: Value = response.json().await.unwrap();
        assert_eq!(status, StatusCode::OK, "{path}: {body}");
        assert_eq!(body["total"], 2, "{path}: {body}");
        assert_eq!(body["entry"].as_array().unwrap().len(), 1, "{body}");
        assert_eq!(body["entry"][0]["resource"], saved[&a], "{path}: {body}");
    }
    for path in ["Patient/_search", "_search"] {
        let response = client
            .post(format!("{fhir}/{path}"))
            .bearer_auth(&scoped)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(format!("{query}&_type=Patient&_elements=id"))
            .send()
            .await
            .unwrap();
        let status = response.status();
        let body: Value = response.json().await.unwrap();
        assert_eq!(status, StatusCode::OK, "{path}: {body}");
        assert_eq!(body["total"], 2, "{body}");
        assert_eq!(body["entry"][0]["resource"]["id"], a);
        assert!(body["entry"][0]["resource"].get("active").is_none());
    }
    let count: Value = client
        .get(format!("{fhir}/Patient?_id={ids}&_summary=count"))
        .bearer_auth(&scoped)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(count["total"], 2, "{count}");
    assert!(
        count
            .get("entry")
            .is_none_or(|entries| entries.as_array().unwrap().is_empty())
    );
    let inline_query = format!("_id={root}&_include:iterate=Observation:has-member");
    let inline: Value = client
        .get(format!("{fhir}/Observation?{inline_query}"))
        .bearer_auth(&scoped)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(inline["entry"].as_array().unwrap().len(), 1, "{inline}");
    assert_eq!(inline["entry"][0]["resource"], saved[&root]);
    let reverse: Value = client
        .get(format!(
            "{fhir}/Patient?_id={a}&_revinclude=Observation:subject"
        ))
        .bearer_auth(&scoped)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let included: Vec<_> = reverse["entry"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["search"]["mode"] == "include")
        .map(|entry| &entry["resource"])
        .collect();
    assert_eq!(included.len(), 2, "{reverse}");
    assert!(
        included
            .iter()
            .all(|body| *body == &saved[&root] || *body == &saved[&tail])
    );
    assert_eq!(
        client
            .get(format!("{fhir}/Patient/{a}"))
            .bearer_auth(&scoped)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    for bundle_type in ["batch", "transaction"] {
        let response = client
            .post(fhir)
            .bearer_auth(&scoped)
            .json(
                &json!({"resourceType":"Bundle", "type":bundle_type, "entry":[
                    {"request":{"method":"GET","url":format!("Patient?{query}")}},
                    {"request":{"method":"GET","url":format!("Observation?{inline_query}")}}
                ]}),
            )
            .send()
            .await
            .unwrap();
        let status = response.status();
        let body: Value = response.json().await.unwrap();
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["entry"][0]["response"]["status"], "200 OK", "{body}");
        assert_eq!(body["entry"][0]["resource"]["total"], 2, "{body}");
        assert_eq!(
            body["entry"][0]["resource"]["entry"][0]["resource"],
            saved[&a]
        );
        assert_eq!(
            body["entry"][1]["resource"]["entry"]
                .as_array()
                .unwrap()
                .len(),
            1,
            "{body}"
        );
    }
    let response = client.post(format!("{base}/AccessPolicy")).bearer_auth(full).json(&json!({
        "resourceType":"AccessPolicy","name":"Snapshot runtime failure","active":true,"priority":0,
        "matcher":{"resourceTypes":["Patient"],"operations":operations},
        "engine":{"type":"quickjs","script":"throw new Error('deliberate snapshot failure');"}
    })).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let failing: Value = response.json().await.unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let response = client
                .get(format!("{fhir}/Patient/{a}"))
                .bearer_auth(full)
                .send()
                .await
                .unwrap();
            if response.status() == StatusCode::INTERNAL_SERVER_ERROR {
                break;
            }
            assert_eq!(response.status(), StatusCode::OK);
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap();
    for path in [
        format!("Patient/{a}/_history?_total=accurate"),
        format!("Patient?_id={a}&_summary=count"),
    ] {
        assert_eq!(
            client
                .get(format!("{fhir}/{path}"))
                .bearer_auth(full)
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::INTERNAL_SERVER_ERROR,
            "{path}"
        );
    }
    let rollback_marker = format!("snapshot-rollback-{suffix}");
    let response = client.post(fhir).bearer_auth(full).json(&json!({"resourceType":"Bundle","type":"transaction","entry":[
        {"resource":{"resourceType":"Patient","identifier":[{"system":"urn:snapshot-rollback","value":rollback_marker}]},"request":{"method":"POST","url":"Patient"}},
        {"request":{"method":"GET","url":format!("Patient?_id={a}")}}
    ]})).send().await.unwrap();
    let status = response.status();
    let body: Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
    for definition in [&failing, &policy] {
        assert_eq!(
            client
                .delete(format!(
                    "{base}/AccessPolicy/{}",
                    definition["id"].as_str().unwrap()
                ))
                .bearer_auth(full)
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::NO_CONTENT
        );
    }
    let response = client
        .get(format!("{fhir}/Patient"))
        .bearer_auth(full)
        .query(&[
            (
                "identifier",
                format!("urn:snapshot-rollback|{rollback_marker}"),
            ),
            ("_summary", "count".to_owned()),
        ])
        .send()
        .await
        .unwrap();
    let status = response.status();
    let remaining: Value = response.json().await.unwrap();
    assert_eq!(status, StatusCode::OK, "{remaining}");
    assert_eq!(remaining["total"], 0, "{remaining}");
}
