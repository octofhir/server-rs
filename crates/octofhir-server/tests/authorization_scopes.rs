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

#[tokio::test]
async fn smart_grants_bound_policies_and_bundle_entries_and_revoked_tokens() {
    let postgres = Postgres::default()
        .with_tag("18-alpine")
        .start()
        .await
        .unwrap();
    let port = postgres.get_host_port_ipv4(5432).await.unwrap();
    let mut config = AppConfig::default();
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
        scopes: vec!["system/*.cruds".into()],
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
        }
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
            StatusCode::NOT_IMPLEMENTED,
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
    for method in ["GET", "POST"] {
        let path = format!("{fhir}/Patient/{id}/$everything?_type=Patient,Observation&_count=1");
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
            next.starts_with(&format!("{configured_fhir}/Patient/{id}/$everything?")),
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
    terminology_server.abort();
}
