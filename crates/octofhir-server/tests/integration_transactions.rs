//! Integration tests for PostgreSQL native transactions.
//!
//! These tests verify ACID guarantees for FHIR Bundle transactions:
//! - Atomicity: All-or-nothing execution
//! - Consistency: Database remains in valid state
//! - Isolation: Concurrent transactions don't interfere
//! - Durability: Committed changes persist
//!
//! Requirements:
//! - Docker running (for testcontainers)

use std::sync::Arc;

use octofhir_config::ConfigurationManager;
use octofhir_server::{AppConfig, PostgresStorageConfig, build_app};
use serde_json::{Value, json};
use testcontainers::{ContainerAsync, ImageExt, runners::AsyncRunner};
use testcontainers_modules::postgres::Postgres;
use tokio::task::JoinHandle;

fn path_patch(operation: &str, path: &str, extra: Vec<Value>) -> Value {
    let mut parts = vec![
        json!({"name":"type","valueCode":operation}),
        json!({"name":"path","valueString":path}),
    ];
    parts.extend(extra);
    json!({"resourceType":"Parameters","parameter":[{"name":"operation","part":parts}]})
}

#[tokio::test]
async fn auth_helpers_preserve_global_fhir_revisions_and_atomic_code_consumption() {
    use octofhir_auth::oauth::session::AuthorizationSession;
    use octofhir_auth::storage::{RefreshTokenStorage, SessionStorage as SessionStorageTrait};
    use octofhir_auth::types::RefreshToken;
    use octofhir_auth_postgres::{
        ArcRefreshTokenStorage, ArcSessionStorage, ClientStorage, IdentityProviderStorage,
        PolicyStorage, RoleStorage, SessionStorage, TokenStorage, UserStorage,
    };
    use octofhir_db_postgres::PostgresStorage;
    use octofhir_storage::{FhirStorage, HistoryParams};
    use time::OffsetDateTime;
    use uuid::Uuid;

    let (_container, postgres_url) = start_postgres().await;
    let config = create_config(&postgres_url);
    let (_base, shutdown, handle) = start_server(&config).await;
    let pool = Arc::new(
        sqlx_postgres::PgPoolOptions::new()
            .max_connections(5)
            .connect(&postgres_url)
            .await
            .unwrap(),
    );
    let native = PostgresStorage::from_pool(pool.as_ref().clone());
    macro_rules! revisions {
        ($helper:expr, $resource_type:literal) => {{
            let helper = $helper;
            let id = Uuid::new_v4();
            let created = helper.create(id, json!({"resourceType":$resource_type,
                "id":id,"active":true,"name":"revision-fixture"})).await.unwrap();
            assert_eq!(created.resource["meta"]["versionId"], created.txid.to_string());
            let first_native = native.update(&created.resource, None).await.unwrap();
            let updated = helper.update(id, first_native.resource.clone()).await.unwrap();
            assert!(updated.txid > first_native.version_id.parse::<i64>().unwrap());
            let second_native = native.update(&updated.resource, None).await.unwrap();
            assert!(second_native.version_id.parse::<i64>().unwrap() > updated.txid);
            assert_eq!(native.vread($resource_type, &id.to_string(), &updated.txid.to_string())
                .await.unwrap().unwrap().resource, updated.resource);
            helper.delete(id).await.unwrap();
            // Deleted resources read as Gone (410), not as never-existing (404)
            assert!(matches!(
                native.read($resource_type, &id.to_string()).await,
                Err(octofhir_storage::StorageError::Deleted { .. })
            ));
            let tombstone = native.current_snapshot($resource_type, &id.to_string())
                .await.unwrap().unwrap();
            assert_eq!(tombstone.method, octofhir_storage::HistoryMethod::Delete);
            assert!(tombstone.resource.version_id.parse::<i64>().unwrap() > second_native.version_id.parse::<i64>().unwrap());
            let history = native.history($resource_type, Some(&id.to_string()),
                &HistoryParams { total: Some(octofhir_storage::TotalMode::Accurate), ..Default::default() })
                .await.unwrap();
            assert_eq!(history.total, Some(5));
            assert_eq!(history.entries[0].resource.resource, serde_json::from_str::<Value>(&tombstone.resource.resource_json).unwrap());
            assert_eq!(history.entries[1].resource.resource, second_native.resource);
        }};
    }
    revisions!(UserStorage::new(&pool), "User");
    revisions!(RoleStorage::new(&pool), "Role");
    revisions!(ClientStorage::new(&pool), "Client");
    revisions!(PolicyStorage::new(&pool), "AccessPolicy");
    revisions!(IdentityProviderStorage::new(&pool), "IdentityProvider");
    revisions!(SessionStorage::new(&pool), "Session");

    let now = OffsetDateTime::now_utc();
    let session = AuthorizationSession {
        id: Uuid::new_v4(),
        code: AuthorizationSession::generate_code(),
        client_id: "code-fixture".into(),
        redirect_uri: "https://client.example/callback".into(),
        scope: "openid".into(),
        state: "state".into(),
        code_challenge: None,
        code_challenge_method: None,
        user_id: None,
        launch_context: None,
        nonce: None,
        aud: config.base_url(),
        created_at: now,
        expires_at: now + time::Duration::minutes(5),
        consumed_at: None,
    };
    let sessions = ArcSessionStorage::new(pool.clone());
    sessions.create(&session).await.unwrap();
    let (first, second) = tokio::join!(
        sessions.consume(&session.code),
        sessions.consume(&session.code)
    );
    assert_eq!(usize::from(first.is_ok()) + usize::from(second.is_ok()), 1);
    let successful = first
        .as_ref()
        .ok()
        .or_else(|| second.as_ref().ok())
        .unwrap();
    let failed = first
        .as_ref()
        .err()
        .or_else(|| second.as_ref().err())
        .unwrap();
    assert!(
        matches!(failed, octofhir_auth::AuthError::InvalidGrant { .. }),
        "{failed}"
    );
    assert!(successful.consumed_at.is_some());
    let stored = sessions.find_by_code(&session.code).await.unwrap().unwrap();
    assert_eq!(stored.consumed_at, successful.consumed_at);
    assert!(sessions.consume(&session.code).await.is_err());
    let mut expired = session.clone();
    expired.id = Uuid::new_v4();
    expired.code = AuthorizationSession::generate_code();
    expired.expires_at = now - time::Duration::seconds(1);
    sessions.create(&expired).await.unwrap();
    assert!(sessions.consume(&expired.code).await.is_err());
    assert!(
        sessions
            .find_by_code(&expired.code)
            .await
            .unwrap()
            .unwrap()
            .consumed_at
            .is_none()
    );

    let refresh = RefreshToken {
        id: Uuid::new_v4(),
        token_hash: RefreshToken::hash_token("fixture-refresh"),
        client_id: "refresh-fixture".into(),
        user_id: None,
        scope: "system/Patient.c".into(),
        audience: Some(config.base_url()),
        launch_context: None,
        created_at: now,
        expires_at: Some(now + time::Duration::days(1)),
        revoked_at: None,
    };
    let tokens = ArcRefreshTokenStorage::new(pool.clone());
    tokens.create(&refresh).await.unwrap();
    TokenStorage::new(&pool)
        .update_last_used(refresh.id)
        .await
        .unwrap();
    tokens.revoke(&refresh.token_hash).await.unwrap();
    let revoked = tokens
        .find_by_hash(&refresh.token_hash)
        .await
        .unwrap()
        .unwrap();
    assert!(revoked.is_revoked());
    let row = TokenStorage::new(&pool)
        .find_by_id(refresh.id)
        .await
        .unwrap()
        .unwrap();
    time::OffsetDateTime::parse(
        row.resource["lastUsedAt"].as_str().unwrap(),
        &time::format_description::well_known::Rfc3339,
    )
    .unwrap();
    let previous = row.resource.clone();
    assert!(tokens.revoke(&refresh.token_hash).await.is_err());
    assert_eq!(
        TokenStorage::new(&pool)
            .find_by_id(refresh.id)
            .await
            .unwrap()
            .unwrap()
            .resource,
        previous
    );
    shutdown.send(()).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(10), handle)
        .await
        .unwrap()
        .unwrap();
    pool.close().await;
}

#[tokio::test]
async fn configured_fhir_version_aliases_preserve_models_search_and_warm_caches() {
    use octofhir_db_postgres::PostgresPackageStore;
    use sqlx_core::{query::query, query_scalar::query_scalar};

    let (_container, postgres_url) = start_postgres().await;
    let mut config = create_config(&postgres_url);
    config.fhir.version = "4.0.1".into();
    config.validate().unwrap();
    let (base, shutdown, handle) = start_server(&config).await;
    let client = reqwest::Client::new();
    let response = client
        .post(format!("{base}/Patient"))
        .json(&json!({"resourceType":"Patient","active":true,
            "identifier":[{"system":"urn:version-fixture","value":"kept"}]}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::CREATED);
    let etag = response.headers()["etag"].clone();
    let last_modified = response.headers()["last-modified"].clone();
    let saved: Value = response.json().await.unwrap();
    shutdown.send(()).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(10), handle)
        .await
        .unwrap()
        .unwrap();

    let pool = sqlx_postgres::PgPoolOptions::new()
        .max_connections(3)
        .connect(&postgres_url)
        .await
        .unwrap();
    let store = PostgresPackageStore::new(pool.clone());
    // A short-form embedded package must be included even with a numeric config.
    store
        .load_from_embedded(
            "version-fixture",
            "1.0.0",
            "R4",
            vec![(
                "SearchParameter".into(),
                json!({"resourceType":"SearchParameter","url":"urn:version-fixture:marker",
            "name":"VersionMarker","status":"active","code":"version-marker",
            "type":"token","base":["Patient"],"expression":"Patient.identifier"}),
            )],
        )
        .await
        .unwrap();
    // Reproduce a partial cache left by the old numeric version key. A warm
    // package with other valid schemas must not skip repair of this one.
    let updated = query("UPDATE fcm.fhirschemas SET fhir_version='4.0.1' WHERE package_name='hl7.fhir.r4.core' AND content->>'name'='Patient'")
        .execute(&pool).await.unwrap();
    assert_eq!(updated.rows_affected(), 1);
    assert_eq!(
        store
            .count_fhirschemas_for_package_version("hl7.fhir.r4.core", "4.0.1", "R4")
            .await
            .unwrap(),
        0
    );

    for alias in ["4.0.1", "r4"] {
        config.fhir.version = alias.into();
        config.validate().unwrap();
        let (base, shutdown, handle) = start_server(&config).await;
        let response = client
            .get(format!("{base}/Patient/{}", saved["id"].as_str().unwrap()))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        assert_eq!(response.headers()["etag"], etag);
        assert_eq!(response.headers()["last-modified"], last_modified);
        assert_eq!(response.json::<Value>().await.unwrap(), saved);
        let metadata: Value = client
            .get(format!("{base}/metadata"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(metadata["fhirVersion"], "4.0.1");
        let patient = metadata["rest"][0]["resource"]
            .as_array()
            .unwrap()
            .iter()
            .find(|resource| resource["type"] == "Patient")
            .unwrap();
        assert!(
            patient["searchParam"]
                .as_array()
                .unwrap()
                .iter()
                .any(|param| param["name"] == "version-marker"),
            "{patient}"
        );
        let response = client
            .get(format!("{base}/Patient"))
            .query(&[
                ("version-marker", "urn:version-fixture|kept"),
                ("_total", "accurate"),
            ])
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        let matches: Value = response.json().await.unwrap();
        assert_eq!(matches["total"], 1, "{matches}");
        assert_eq!(matches["entry"][0]["resource"], saved);
        let wrong_keys: i64 =
            query_scalar("SELECT COUNT(*) FROM fcm.fhirschemas WHERE fhir_version <> 'R4'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(wrong_keys, 0);
        shutdown.send(()).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(10), handle)
            .await
            .unwrap()
            .unwrap();
    }
    pool.close().await;
}

#[tokio::test]
async fn validate_honors_explicit_profiles_in_query_and_parameters() {
    use reqwest::StatusCode;
    let (_container, postgres_url) = start_postgres().await;
    let mut config = create_config(&postgres_url);
    config.cql.enabled = true;
    let (base, shutdown, _server) = start_server(&config).await;
    let client = reqwest::Client::new();
    let resource = json!({"resourceType":"Patient","active":true});
    let response = client
        .post(format!("{base}/Patient/$validate"))
        .json(&resource)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let outcome: Value = response.json().await.unwrap();
    assert_eq!(outcome["resourceType"], "OperationOutcome");
    assert!(!outcome["issue"].as_array().unwrap().is_empty());
    assert_eq!(outcome["issue"][0]["severity"], "information");

    for method in [reqwest::Method::GET, reqwest::Method::POST] {
        let mut request = client.request(
            method.clone(),
            format!("{base}/Measure/unavailable/$evaluate-measure"),
        );
        if method == reqwest::Method::POST {
            request = request.json(&json!({"resourceType":"Parameters","parameter":[
                {"name":"periodStart","valueDate":"2024-01-01"},
                {"name":"periodEnd","valueDate":"2024-12-31"},
                {"name":"reportType","valueCode":"summary"}
            ]}));
        } else {
            request = request.query(&[
                ("periodStart", "2024-01-01"),
                ("periodEnd", "2024-12-31"),
                ("reportType", "summary"),
            ]);
        }
        let response = request.send().await.unwrap();
        let status = response.status();
        let body: Value = response.json().await.unwrap();
        assert_eq!(status, StatusCode::NOT_IMPLEMENTED, "{body}");
        assert_eq!(body["resourceType"], "OperationOutcome", "{body}");
        assert!(
            body.get("status").is_none(),
            "unavailable scoring must not fabricate a complete report: {body}"
        );
    }
    let metadata: Value = client
        .get(format!("{base}/metadata"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(metadata["software"]["name"], "OctoFHIR");
    assert_eq!(metadata["fhirVersion"], "4.0.1");
    assert!(
        metadata["implementation"]["url"]
            .as_str()
            .unwrap()
            .ends_with("/fhir")
    );
    let patient_capability = metadata["rest"][0]["resource"]
        .as_array()
        .unwrap()
        .iter()
        .find(|resource| resource["type"] == "Patient")
        .unwrap();
    assert!(
        patient_capability["operation"]
            .as_array()
            .unwrap()
            .iter()
            .any(|op| op["name"] == "everything"),
        "Patient $everything must remain supported: {patient_capability}"
    );
    assert!(
        !metadata["rest"][0]["operation"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|op| op["name"] == "everything")
    );
    assert!(patient_capability.get("supportedProfile").is_none());

    assert!(metadata.get("instantiates").is_none());
    for rest in metadata["rest"].as_array().unwrap() {
        for operation in rest["operation"].as_array().into_iter().flatten().chain(
            rest["resource"]
                .as_array()
                .into_iter()
                .flatten()
                .flat_map(|resource| resource["operation"].as_array().into_iter().flatten()),
        ) {
            assert_ne!(operation["name"], "evaluate-measure");
            assert_ne!(operation["name"], "$evaluate-measure");
        }
    }

    for (profile, expected) in [
        (
            "http://hl7.org/fhir/StructureDefinition/Patient",
            StatusCode::OK,
        ),
        (
            "http://example.invalid/unavailable-profile",
            StatusCode::BAD_REQUEST,
        ),
    ] {
        let response = client
            .post(format!("{base}/Patient/$validate"))
            .query(&[("profile", profile)])
            .json(&resource)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), expected, "query profile {profile}");
        let response = client
            .post(format!("{base}/Patient/$validate"))
            .json(&json!({"resourceType":"Parameters","parameter":[
                {"name":"resource","resource":resource},
                {"name":"profile","valueUri":profile}
            ]}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), expected, "Parameters profile {profile}");
    }
    let response = client
        .post(format!("{base}/Patient/$validate"))
        .json(&json!({"resourceType":"Patient","active":"wrong"}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let outcome: Value = response.json().await.unwrap();
    assert!(
        outcome["issue"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue["severity"] == "error")
    );
    let _ = shutdown.send(());
}

async fn send_bundle(
    client: &reqwest::Client,
    base: &str,
    kind: &str,
    entries: Value,
) -> (reqwest::StatusCode, Value) {
    let response = client
        .post(base)
        .json(&json!({"resourceType":"Bundle","type":kind,"entry":entries}))
        .send()
        .await
        .unwrap();
    let status = response.status();
    (status, response.json().await.unwrap())
}

#[tokio::test]
async fn conditional_match_cardinality_ignores_result_controls_and_decodes_identifiers() {
    use reqwest::StatusCode;
    let (_container, postgres_url) = start_postgres().await;
    let config = create_config(&postgres_url);
    let (base, shutdown, _server) = start_server(&config).await;
    let client = reqwest::Client::new();
    let mut ids = Vec::new();
    for _ in 0..2 {
        let response = client.post(format!("{base}/Patient"))
            .json(&json!({"resourceType":"Patient","identifier":[{"system":"urn:test","value":"duplicate"}]}))
            .send().await.unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        let body: Value = response.json().await.unwrap();
        ids.push(body["id"].as_str().unwrap().to_owned());
    }
    for controls in [
        "_count=1",
        "_count=0",
        "_offset=100",
        "_summary=count",
        "_elements=id&_count=1",
    ] {
        let url = format!("Patient?identifier=urn%3Atest%7Cduplicate&{controls}");
        let response = client.delete(format!("{base}/{url}")).send().await.unwrap();
        assert_eq!(
            response.status(),
            StatusCode::PRECONDITION_FAILED,
            "{controls}"
        );
        let response = client
            .post(format!("{base}/Patient"))
            .header(
                "If-None-Exist",
                format!("identifier=urn%3Atest%7Cduplicate&{controls}"),
            )
            .json(&json!({"resourceType":"Patient","active":true}))
            .send()
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::PRECONDITION_FAILED,
            "{controls}"
        );
        for kind in ["batch", "transaction"] {
            let (status, body) = send_bundle(
                &client,
                &base,
                kind,
                json!([{"request":{"method":"DELETE","url":url}}]),
            )
            .await;
            if kind == "batch" {
                assert_eq!(
                    body["entry"][0]["response"]["status"], "412 Precondition Failed",
                    "{body}"
                );
            } else {
                assert_eq!(status, StatusCode::PRECONDITION_FAILED, "{body}");
            }
        }
    }
    for id in ids {
        assert!(resource_exists(&client, &base, "Patient", &id).await);
    }

    let response = client.post(format!("{base}/Patient"))
        .json(&json!({"resourceType":"Patient","active":true,"identifier":[{"system":"urn:test","value":"unique"}]}))
        .send().await.unwrap();
    let patient: Value = response.json().await.unwrap();
    let id = patient["id"].as_str().unwrap();
    let (_, body) = send_bundle(&client, &base, "transaction", json!([{
        "request":{"method":"PUT","url":"Patient?identifier=urn%3Atest%7Cunique&_count=0&_offset=100"},
        "resource":{"resourceType":"Patient","id":id,"active":false,"identifier":[{"system":"urn:test","value":"unique"}]}
    }])).await;
    assert_eq!(body["entry"][0]["response"]["status"], "200 OK", "{body}");
    assert_eq!(body["entry"][0]["resource"]["id"], id, "{body}");
    assert_eq!(
        read_resource(&client, &base, "Patient", id).await.unwrap()["active"],
        false
    );
    let _ = shutdown.send(());
}

#[tokio::test]
async fn bundle_writes_follow_identity_validation_patch_and_version_contracts() {
    use reqwest::StatusCode;
    let (_container, postgres_url) = start_postgres().await;
    let config = create_config(&postgres_url);
    let (base, shutdown, _server) = start_server(&config).await;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .unwrap();
    let created: Value = client
        .post(format!("{base}/Patient"))
        .json(&json!({"resourceType":"Patient","active":true,"name":[
            {"use":"official","family":"One","given":["Same","Same","Third"]},
            {"use":"usual","family":"Two"}
        ]}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let id = created["id"].as_str().unwrap();
    let version = created["meta"]["versionId"].as_str().unwrap();
    let url = format!("Patient/{id}");

    for resource in [
        json!({"resourceType":"Patient","active":false}),
        json!({"resourceType":"Patient","id":"different","active":false}),
    ] {
        let response = client
            .put(format!("{base}/{url}"))
            .json(&resource)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        for kind in ["batch", "transaction"] {
            let (status, body) = send_bundle(
                &client,
                &base,
                kind,
                json!([{"resource":resource,"request":{"method":"PUT","url":url}}]),
            )
            .await;
            if kind == "batch" {
                assert_eq!(
                    body["entry"][0]["response"]["status"], "400 Bad Request",
                    "{body}"
                );
            } else {
                assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
            }
        }
    }
    let invalid = json!({"resourceType":"Patient","id":id,"active":"not-a-boolean"});
    let response = client
        .put(format!("{base}/{url}"))
        .json(&invalid)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let (_, body) = send_bundle(
        &client,
        &base,
        "batch",
        json!([{"resource":invalid,"request":{"method":"PUT","url":url}}]),
    )
    .await;
    assert!(
        body["entry"][0]["response"]["status"]
            .as_str()
            .unwrap()
            .starts_with("422"),
        "{body}"
    );

    // Two mutations of the same identity cannot share one history version.
    let (_, body) = send_bundle(&client, &base, "transaction", json!([
        {"resource":{"resourceType":"Patient","id":id,"active":false},"request":{"method":"PUT","url":url}},
        {"resource":{"resourceType":"Patient","id":id,"active":true},"request":{"method":"PUT","url":url}}
    ])).await;
    assert_eq!(body["resourceType"], "OperationOutcome", "{body}");
    assert_eq!(
        read_resource(&client, &base, "Patient", id).await.unwrap()["meta"]["versionId"],
        version
    );

    // Indices distinguish equal values; where selects the physical array member.
    let patch = path_patch(
        "replace",
        "Patient.name.where(use = 'official').given[1]",
        vec![json!({"name":"value","valueString":"Changed"})],
    );
    let (_, body) = send_bundle(
        &client,
        &base,
        "batch",
        json!([{"resource":patch,"request":{"method":"PATCH","url":url}}]),
    )
    .await;
    assert_eq!(body["entry"][0]["response"]["status"], "200 OK", "{body}");
    let current = read_resource(&client, &base, "Patient", id).await.unwrap();
    assert_eq!(
        current["name"][0]["given"],
        json!(["Same", "Changed", "Third"])
    );
    let current_version = current["meta"]["versionId"].clone();

    let invalid_patch = path_patch(
        "replace",
        "Patient.active",
        vec![json!({"name":"value","valueString":"wrong"})],
    );
    for kind in ["batch", "transaction"] {
        let (status, body) = send_bundle(
            &client,
            &base,
            kind,
            json!([{"resource":invalid_patch,"request":{"method":"PATCH","url":url}}]),
        )
        .await;
        if kind == "batch" {
            assert!(
                body["entry"][0]["response"]["status"]
                    .as_str()
                    .unwrap()
                    .starts_with("422"),
                "{body}"
            );
        } else {
            assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
        }
    }
    let valid_patch = path_patch(
        "replace",
        "Patient.active",
        vec![json!({"name":"value","valueBoolean":false})],
    );
    for kind in ["batch", "transaction"] {
        let (status, body) = send_bundle(&client, &base, kind, json!([{"resource":valid_patch,"request":{"method":"PATCH","url":url,"ifMatch":format!("W/\"{version}\"")}}])).await;
        if kind == "batch" {
            assert_eq!(
                body["entry"][0]["response"]["status"], "412 Precondition Failed",
                "{body}"
            );
        } else {
            assert_eq!(status, StatusCode::PRECONDITION_FAILED, "{body}");
        }
    }
    assert_eq!(
        read_resource(&client, &base, "Patient", id).await.unwrap()["meta"]["versionId"],
        current_version
    );

    for patch in [
        path_patch(
            "replace",
            "Patient.name.family",
            vec![json!({"name":"value","valueString":"Ambiguous"})],
        ),
        path_patch(
            "insert",
            "Patient.name[0].given",
            vec![
                json!({"name":"index","valueInteger":-1}),
                json!({"name":"value","valueString":"Negative"}),
            ],
        ),
        path_patch(
            "move",
            "Patient.name[0].given",
            vec![
                json!({"name":"source","valueInteger":0}),
                json!({"name":"destination","valueInteger":99}),
            ],
        ),
        json!([{"op":"replace","path":"/active","value":false}]),
    ] {
        let (_, body) = send_bundle(
            &client,
            &base,
            "batch",
            json!([{"resource":patch,"request":{"method":"PATCH","url":url}}]),
        )
        .await;
        assert_eq!(
            body["entry"][0]["response"]["status"], "400 Bad Request",
            "{body}"
        );
    }

    let move_patch = path_patch(
        "move",
        "Patient.name[0].given",
        vec![
            json!({"name":"source","valueInteger":0}),
            json!({"name":"destination","valueInteger":2}),
        ],
    );
    let (_, body) = send_bundle(
        &client,
        &base,
        "transaction",
        json!([{"resource":move_patch,"request":{"method":"PATCH","url":url}}]),
    )
    .await;
    assert_eq!(body["entry"][0]["response"]["status"], "200 OK", "{body}");
    assert_eq!(
        read_resource(&client, &base, "Patient", id).await.unwrap()["name"][0]["given"],
        json!(["Changed", "Third", "Same"])
    );

    for kind in ["batch", "transaction"] {
        let (_, body) = send_bundle(&client, &base, kind, json!([{"resource":{"resourceType":"Patient","id":"client-chosen-id","active":true},"request":{"method":"POST","url":"Patient"}}])).await;
        assert_eq!(
            body["entry"][0]["response"]["status"], "201 Created",
            "{body}"
        );
        assert_ne!(
            body["entry"][0]["resource"]["id"], "client-chosen-id",
            "{body}"
        );
    }
    Box::pin(transaction_etag_contracts(
        &client,
        &base,
        &format!("{}/fhir", config.base_url()),
    ))
    .await;
    let _ = shutdown.send(());
}

async fn transaction_etag_contracts(client: &reqwest::Client, base: &str, advertised_base: &str) {
    use reqwest::StatusCode;
    let resource = json!({"resourceType":"Patient", "id":"transaction-etag", "active":true,
        "identifier":[{"system":"urn:tx-etag","value":"protected"}]});
    let direct = format!("{base}/Patient/transaction-etag");
    let response = client.put(&direct).json(&resource).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let original: Value = response.json().await.unwrap();
    let version = original["meta"]["versionId"].as_str().unwrap();
    let history: Value = client
        .get(format!("{direct}/_history"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let patch = path_patch(
        "replace",
        "Patient.active",
        vec![json!({"name":"value","valueBoolean":false})],
    );
    let sentinel = json!({"request":{"method":"POST","url":"Patient"}, "resource":{"resourceType":"Patient", "identifier":[{"system":"urn:tx-etag","value":"rollback-sibling"}]}});
    for (value, expected) in [
        (json!(42), StatusCode::BAD_REQUEST),
        (json!("*"), StatusCode::BAD_REQUEST),
        (json!("1"), StatusCode::BAD_REQUEST),
        (json!("W/\"1\", W/\"2\""), StatusCode::BAD_REQUEST),
        (
            json!("W/\"missing.version\""),
            StatusCode::PRECONDITION_FAILED,
        ),
        (
            json!(format!("W/\"0{version}\"")),
            StatusCode::PRECONDITION_FAILED,
        ),
    ] {
        for method in ["PUT", "PATCH", "DELETE"] {
            let mut entry = json!({"request":{"method":method,"url":"Patient/transaction-etag","ifMatch":value}});
            if method == "PUT" {
                entry["resource"] = resource.clone();
            }
            if method == "PATCH" {
                entry["resource"] = patch.clone();
            }
            let (status, body) =
                send_bundle(client, base, "transaction", json!([sentinel, entry])).await;
            assert_eq!(status, expected, "{method}/{value}: {body}");
        }
    }
    for target in [
        "Patient/transaction-absent",
        "Patient?identifier=urn:tx-etag%7Cabsent",
    ] {
        let mut absent = resource.clone();
        absent["id"] = json!("transaction-absent");
        let (status, body) = send_bundle(client, base, "transaction", json!([
            sentinel,
            {"request":{"method":"PUT","url":target,"ifMatch":"W/\"missing.version\""},"resource":absent}
        ])).await;
        assert_eq!(status, StatusCode::PRECONDITION_FAILED, "{body}");
    }
    let unchanged: Value = client
        .get(&direct)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(unchanged, original);
    let unchanged_history: Value = client
        .get(format!("{direct}/_history"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(unchanged_history, history);
    let count: Value = client
        .get(format!(
            "{base}/Patient?identifier=urn:tx-etag%7Crollback-sibling&_summary=count"
        ))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        count["total"], 0,
        "failed version checks must roll back sibling creates"
    );

    let (status, body) = send_bundle(client, base, "transaction", json!([
        {"request":{"method":"PUT","url":"Patient?identifier=urn:tx-etag%7Cprotected"},"resource":{"resourceType":"Patient","id":"mismatched-conditional","active":true}}
    ])).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    let (status, body) = send_bundle(client, base, "transaction", json!([
        {"fullUrl":"urn:uuid:upsert-existing", "request":{"method":"PUT","url":"Patient?identifier=urn:tx-etag%7Cno-match","ifMatch":format!("W/\"{version}\"")},"resource":resource}
    ])).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["entry"][0]["response"]["status"], "200 OK", "{body}");
    assert_eq!(body["entry"][0]["resource"]["id"], "transaction-etag");
    let etag = body["entry"][0]["response"]["etag"].clone();
    let before_delete = body["entry"][0]["resource"].clone();
    let (status, body) = send_bundle(
        client,
        base,
        "transaction",
        json!([
            {"request":{"method":"DELETE","url":"Patient/transaction-etag","ifMatch":etag}}
        ]),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["entry"][0]["response"]["status"], "204 No Content",
        "{body}"
    );
    let deleted_etag = body["entry"][0]["response"]["etag"].as_str().unwrap();
    let deletion_version = deleted_etag
        .strip_prefix("W/\"")
        .unwrap()
        .strip_suffix('"')
        .unwrap();
    assert_ne!(
        deletion_version,
        before_delete["meta"]["versionId"].as_str().unwrap()
    );
    assert!(
        time::OffsetDateTime::parse(
            body["entry"][0]["response"]["lastModified"]
                .as_str()
                .unwrap(),
            &time::format_description::well_known::Rfc3339
        )
        .is_ok()
    );
    assert_eq!(
        client
            .get(format!("{direct}/_history/{deletion_version}"))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::GONE
    );
    assert!(body["entry"][0]["response"].get("location").is_none());
    assert_eq!(
        body["entry"][0]["fullUrl"],
        format!("{advertised_base}/Patient/transaction-etag")
    );
    let historical: Value = client
        .get(format!(
            "{direct}/_history/{}",
            before_delete["meta"]["versionId"].as_str().unwrap()
        ))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(historical, before_delete);
    let (status, body) = send_bundle(client, base, "transaction", json!([
        {"request":{"method":"DELETE","url":"Patient/transaction-etag","ifMatch":etag}}, sentinel
    ])).await;
    assert_eq!(status, StatusCode::PRECONDITION_FAILED, "{body}");
    let (status, body) = send_bundle(
        client,
        base,
        "transaction",
        json!([
            {"request":{"method":"DELETE","url":"Patient/transaction-etag"}}
        ]),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, body) = send_bundle(client, base, "transaction", json!([
        {"fullUrl":"urn:uuid:upsert-new", "request":{"method":"PUT","url":"Patient?identifier=urn:tx-etag%7Cnew"},
            "resource":{"resourceType":"Patient","id":"transaction-explicit-created","active":true}},
        {"request":{"method":"POST","url":"Observation"},"resource":{"resourceType":"Observation","status":"final",
            "code":{"text":"upsert link"},"subject":{"reference":"urn:uuid:upsert-new"}}}
    ])).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["entry"][0]["response"]["status"], "201 Created",
        "{body}"
    );
    assert_eq!(
        body["entry"][0]["resource"]["id"],
        "transaction-explicit-created"
    );
    assert_eq!(
        body["entry"][1]["resource"]["subject"]["reference"],
        "Patient/transaction-explicit-created"
    );
    let cached_url = format!("{base}/Patient/transaction-explicit-created");
    let cached: Value = client
        .get(&cached_url)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let (status, body) = send_bundle(
        client,
        base,
        "transaction",
        json!([
            {"request":{"method":"DELETE","url":"Patient?_id=transaction-explicit-created",
                "ifMatch":format!("W/\"{}\"", cached["meta"]["versionId"].as_str().unwrap())}}
        ]),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["entry"][0]["fullUrl"],
        format!("{advertised_base}/Patient/transaction-explicit-created")
    );
    let deleted_etag = body["entry"][0]["response"]["etag"].as_str().unwrap();
    let deletion_version = deleted_etag
        .strip_prefix("W/\"")
        .unwrap()
        .strip_suffix('"')
        .unwrap();
    assert_ne!(
        deletion_version,
        cached["meta"]["versionId"].as_str().unwrap()
    );
    assert_eq!(
        client
            .get(format!("{cached_url}/_history/{deletion_version}"))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::GONE
    );
    assert!(body["entry"][0]["response"].get("location").is_none());
    assert_eq!(
        client.get(&cached_url).send().await.unwrap().status(),
        StatusCode::GONE
    );
}

// =============================================================================
// Test Infrastructure
// =============================================================================

struct PostgresFixture {
    _container: ContainerAsync<Postgres>,
    _isolation: tokio::sync::OwnedMutexGuard<()>,
}

async fn start_postgres() -> (PostgresFixture, String) {
    // Canonical packages are process-global. Hold isolation for the whole
    // fixture, including container startup and teardown, rather than starting
    // every database while its test waits to initialize the server.
    static FIXTURE_LOCK: std::sync::OnceLock<Arc<tokio::sync::Mutex<()>>> =
        std::sync::OnceLock::new();
    let isolation = FIXTURE_LOCK
        .get_or_init(|| Arc::new(tokio::sync::Mutex::new(())))
        .clone()
        .lock_owned()
        .await;
    // Match production PG (docker-compose postgres:18). The migration needs
    // PG14+ for tstzmultirange and the jsonpath[] date-extraction overloads;
    // the testcontainers default (11-alpine) lacks both.
    let container = Postgres::default()
        .with_tag("17-alpine")
        .start()
        .await
        .expect("start postgres container");

    let host_port = container.get_host_port_ipv4(5432).await.expect("get port");
    let url = format!(
        "postgres://postgres:postgres@127.0.0.1:{}/postgres",
        host_port
    );

    (
        PostgresFixture {
            _container: container,
            _isolation: isolation,
        },
        url,
    )
}

fn create_config(postgres_url: &str) -> AppConfig {
    let mut config = AppConfig::default();
    // Pin the published archive so each fresh fixture DB can reuse the
    // integrity-checked tarball cache without registry metadata requests.
    config.packages.load = vec![octofhir_server::config::PackageSpec::Table {
        id: Some("hl7.fhir.r4.core".into()),
        version: Some("4.0.1".into()),
        path: None,
        url: Some("https://hl7.org/fhir/R4/hl7.fhir.r4.core.tgz".into()),
    }];
    config.storage.postgres = Some(PostgresStorageConfig {
        url: Some(postgres_url.to_string()),
        pool_size: 10, // Higher for concurrent transaction tests
        connect_timeout_ms: 10000,
        idle_timeout_ms: Some(60000),
        ..Default::default()
    });
    config.auth.policy.anonymous_access = true;
    config
}

async fn start_server(
    config: &AppConfig,
) -> (String, tokio::sync::oneshot::Sender<()>, JoinHandle<()>) {
    // Every isolated fixture uses a different database; rebuild its canonical
    // registry. Concurrent requests inside a fixture still run concurrently.
    if octofhir_server::canonical::get_manager().is_some() {
        octofhir_server::canonical::rebuild_from_config_async(config)
            .await
            .expect("rebuild canonical registry for fixture database");
    }
    let config_manager = Arc::new(
        ConfigurationManager::builder()
            .build()
            .await
            .expect("build config manager"),
    );
    let app = build_app(config, config_manager).await.expect("build app");

    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("bind");
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();

    let server = tokio::spawn(async move {
        let _ = axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = rx.await;
            })
            .await;
    });

    (format!("http://{addr}/fhir"), tx, server)
}

/// Helper to check if a resource exists
async fn resource_exists(
    client: &reqwest::Client,
    base: &str,
    resource_type: &str,
    id: &str,
) -> bool {
    let resp = client
        .get(format!("{base}/{resource_type}/{id}"))
        .header("accept", "application/fhir+json")
        .send()
        .await
        .expect("read request");

    resp.status().is_success()
}

/// Helper to read a resource
async fn read_resource(
    client: &reqwest::Client,
    base: &str,
    resource_type: &str,
    id: &str,
) -> Option<Value> {
    let resp = client
        .get(format!("{base}/{resource_type}/{id}"))
        .header("accept", "application/fhir+json")
        .send()
        .await
        .expect("read request");

    if resp.status().is_success() {
        resp.json().await.ok()
    } else {
        None
    }
}

// =============================================================================
// Transaction Commit Tests (Atomicity - Success Case)
// =============================================================================

#[tokio::test]
async fn test_transaction_commit_all_creates() {
    let (_container, postgres_url) = start_postgres().await;
    let config = create_config(&postgres_url);
    let (base, shutdown_tx, _handle) = start_server(&config).await;
    let client = reqwest::Client::new();

    // Transaction bundle with multiple CREATE operations
    let bundle = json!({
        "resourceType": "Bundle",
        "type": "transaction",
        "entry": [
            {
                "fullUrl": "urn:uuid:patient-1",
                "resource": {
                    "resourceType": "Patient",
                    "name": [{"family": "Transaction", "given": ["Test"]}]
                },
                "request": {
                    "method": "POST",
                    "url": "Patient"
                }
            },
            {
                "fullUrl": "urn:uuid:obs-1",
                "resource": {
                    "resourceType": "Observation",
                    "status": "final",
                    "code": {"coding": [{"system": "http://loinc.org", "code": "8867-4"}]},
                    "subject": {"reference": "urn:uuid:patient-1"}
                },
                "request": {
                    "method": "POST",
                    "url": "Observation"
                }
            }
        ]
    });

    let resp = client
        .post(&base)
        .header("content-type", "application/fhir+json")
        .json(&bundle)
        .send()
        .await
        .expect("transaction request");

    assert!(resp.status().is_success(), "Transaction should succeed");

    let response_bundle: Value = resp.json().await.expect("parse response");
    assert_eq!(response_bundle["type"], "transaction-response");

    // Extract created IDs from response
    let entries = response_bundle["entry"].as_array().expect("entries");
    assert_eq!(entries.len(), 2);

    // Each committed response identifies the saved resource at the FHIR base.
    for entry in entries {
        let resource = &entry["resource"];
        let resource_type = resource["resourceType"].as_str().unwrap();
        let id = resource["id"].as_str().unwrap();
        let version = resource["meta"]["versionId"].as_str().unwrap();
        assert_eq!(
            entry["fullUrl"],
            format!("{}/fhir/{resource_type}/{id}", config.base_url())
        );
        assert_eq!(
            entry["response"]["location"],
            format!(
                "{}/fhir/{resource_type}/{id}/_history/{version}",
                config.base_url()
            )
        );
        assert_eq!(
            entry["response"]["lastModified"],
            resource["meta"]["lastUpdated"]
        );
        assert!(resource_exists(&client, &base, resource_type, id).await);
    }

    let _ = shutdown_tx.send(());
}

#[tokio::test]
async fn test_transaction_commit_mixed_operations() {
    let (_container, postgres_url) = start_postgres().await;
    let config = create_config(&postgres_url);
    let (base, shutdown_tx, _handle) = start_server(&config).await;
    let client = reqwest::Client::new();

    // First create a patient
    let patient = json!({
        "resourceType": "Patient",
        "name": [{"family": "ToUpdate", "given": ["Initial"]}]
    });

    let resp = client
        .post(format!("{base}/Patient"))
        .header("content-type", "application/fhir+json")
        .json(&patient)
        .send()
        .await
        .expect("create patient");

    assert!(resp.status().is_success());
    let created: Value = resp.json().await.expect("parse");
    let patient_id = created["id"].as_str().expect("id");

    // Transaction with UPDATE and CREATE
    let bundle = json!({
        "resourceType": "Bundle",
        "type": "transaction",
        "entry": [
            {
                "resource": {
                    "resourceType": "Patient",
                    "id": patient_id,
                    "name": [{"family": "ToUpdate", "given": ["Updated"]}]
                },
                "request": {
                    "method": "PUT",
                    "url": format!("Patient/{}", patient_id)
                }
            },
            {
                "fullUrl": "urn:uuid:new-obs",
                "resource": {
                    "resourceType": "Observation",
                    "status": "final",
                    "code": {"coding": [{"system": "http://loinc.org", "code": "8867-4"}]},
                    "subject": {"reference": format!("Patient/{}", patient_id)}
                },
                "request": {
                    "method": "POST",
                    "url": "Observation"
                }
            }
        ]
    });

    let resp = client
        .post(&base)
        .header("content-type", "application/fhir+json")
        .json(&bundle)
        .send()
        .await
        .expect("transaction request");

    assert!(
        resp.status().is_success(),
        "Mixed transaction should succeed"
    );

    // Verify patient was updated
    let updated_patient = read_resource(&client, &base, "Patient", patient_id)
        .await
        .expect("patient should exist");

    let given = &updated_patient["name"][0]["given"][0];
    assert_eq!(given, "Updated", "Patient name should be updated");

    let _ = shutdown_tx.send(());
}

// =============================================================================
// Transaction Rollback Tests (Atomicity - Failure Case)
// =============================================================================

#[tokio::test]
async fn test_transaction_rollback_on_invalid_reference() {
    let (_container, postgres_url) = start_postgres().await;
    let config = create_config(&postgres_url);
    let (base, shutdown_tx, _handle) = start_server(&config).await;
    let client = reqwest::Client::new();

    // Transaction with invalid reference - should rollback all
    let bundle = json!({
        "resourceType": "Bundle",
        "type": "transaction",
        "entry": [
            {
                "fullUrl": "urn:uuid:patient-rollback",
                "resource": {
                    "resourceType": "Patient",
                    "name": [{"family": "ShouldRollback"}]
                },
                "request": {
                    "method": "POST",
                    "url": "Patient"
                }
            },
            {
                "resource": {
                    "resourceType": "Observation",
                    "status": "final",
                    "code": {"coding": [{"system": "http://loinc.org", "code": "8867-4"}]},
                    "id": "nonexistent-obs-12345",
                    "subject": {"reference": "Patient/nonexistent-id-12345"}
                },
                "request": {
                    "method": "PUT",
                    "url": "Observation/nonexistent-obs-12345"
                }
            }
        ]
    });

    let resp = client
        .post(&base)
        .header("content-type", "application/fhir+json")
        .json(&bundle)
        .send()
        .await
        .expect("transaction request");

    // Transaction should fail
    assert!(
        !resp.status().is_success(),
        "Transaction should fail on invalid reference"
    );

    // Search for the patient that should have been rolled back
    let search_resp = client
        .get(format!("{base}/Patient?family=ShouldRollback"))
        .header("accept", "application/fhir+json")
        .send()
        .await
        .expect("search request");

    let search_bundle: Value = search_resp.json().await.expect("parse");
    let total = search_bundle["total"].as_u64().unwrap_or(0);

    assert_eq!(
        total, 0,
        "Patient should be rolled back when transaction fails"
    );

    let _ = shutdown_tx.send(());
}

#[tokio::test]
async fn test_transaction_rollback_preserves_existing_data() {
    let (_container, postgres_url) = start_postgres().await;
    let config = create_config(&postgres_url);
    let (base, shutdown_tx, _handle) = start_server(&config).await;
    let client = reqwest::Client::new();

    // First create a patient that should NOT be affected by failed transaction
    let patient = json!({
        "resourceType": "Patient",
        "name": [{"family": "PreserveMe", "given": ["Original"]}]
    });

    let resp = client
        .post(format!("{base}/Patient"))
        .header("content-type", "application/fhir+json")
        .json(&patient)
        .send()
        .await
        .expect("create patient");

    assert!(resp.status().is_success());
    let created: Value = resp.json().await.expect("parse");
    let patient_id = created["id"].as_str().expect("id");

    // Try a failing transaction that attempts to update this patient
    let bundle = json!({
        "resourceType": "Bundle",
        "type": "transaction",
        "entry": [
            {
                "resource": {
                    "resourceType": "Patient",
                    "id": patient_id,
                    "name": [{"family": "PreserveMe", "given": ["Modified"]}]
                },
                "request": {
                    "method": "PUT",
                    "url": format!("Patient/{}", patient_id)
                }
            },
            {
                "resource": {
                    "resourceType": "InvalidResourceType",
                    "someField": "value"
                },
                "request": {
                    "method": "POST",
                    "url": "InvalidResourceType"
                }
            }
        ]
    });

    let resp = client
        .post(&base)
        .header("content-type", "application/fhir+json")
        .json(&bundle)
        .send()
        .await
        .expect("transaction request");

    // Transaction should fail
    assert!(
        !resp.status().is_success(),
        "Transaction should fail on invalid resource"
    );

    // Verify original patient is unchanged
    let preserved = read_resource(&client, &base, "Patient", patient_id)
        .await
        .expect("patient should still exist");

    let given = &preserved["name"][0]["given"][0];
    assert_eq!(
        given, "Original",
        "Patient should be unchanged after rollback"
    );

    let _ = shutdown_tx.send(());
}

// =============================================================================
// Batch vs Transaction Tests (Isolation Difference)
// =============================================================================

#[tokio::test]
async fn test_batch_partial_failure() {
    let (_container, postgres_url) = start_postgres().await;
    let config = create_config(&postgres_url);
    let (base, shutdown_tx, _handle) = start_server(&config).await;
    let client = reqwest::Client::new();

    // Batch with one valid and one invalid operation
    // Unlike transaction, valid operation should succeed
    let bundle = json!({
        "resourceType": "Bundle",
        "type": "batch",
        "entry": [
            {
                "resource": {
                    "resourceType": "Patient",
                    "name": [{"family": "BatchSuccess"}]
                },
                "request": {
                    "method": "POST",
                    "url": "Patient"
                }
            },
            {
                "resource": {
                    "resourceType": "Patient",
                    "id": "nonexistent-for-update"
                },
                "request": {
                    "method": "PUT",
                    "url": "Patient/nonexistent-for-update",
                    "ifMatch": "W/\"999\""
                }
            }
        ]
    });

    let resp = client
        .post(&base)
        .header("content-type", "application/fhir+json")
        .json(&bundle)
        .send()
        .await
        .expect("batch request");

    // Batch might return 200 even with partial failures
    let response_bundle: Value = resp.json().await.expect("parse response");
    assert_eq!(response_bundle["type"], "batch-response");

    // First entry should succeed, second might fail
    let entries = response_bundle["entry"].as_array().expect("entries");

    // Check first entry succeeded
    let first_status = entries[0]["response"]["status"].as_str().unwrap_or("");
    assert!(
        first_status.starts_with("201") || first_status.starts_with("200"),
        "First batch entry should succeed"
    );

    // Verify the successful patient exists
    let search_resp = client
        .get(format!("{base}/Patient?family=BatchSuccess"))
        .header("accept", "application/fhir+json")
        .send()
        .await
        .expect("search request");

    let search_bundle: Value = search_resp.json().await.expect("parse");
    let total = search_bundle["total"].as_u64().unwrap_or(0);

    assert!(
        total > 0,
        "Successful batch entry should persist despite other failures"
    );

    let _ = shutdown_tx.send(());
}

#[tokio::test]
async fn test_transaction_prefer_return_minimal_omits_mutation_resources() {
    let (_container, postgres_url) = start_postgres().await;
    let config = create_config(&postgres_url);
    let (base, shutdown_tx, _handle) = start_server(&config).await;
    let client = reqwest::Client::new();

    let patient = json!({
        "resourceType": "Patient",
        "name": [{"family": "PreferMinimal", "given": ["Before"]}]
    });

    let resp = client
        .post(format!("{base}/Patient"))
        .header("content-type", "application/fhir+json")
        .json(&patient)
        .send()
        .await
        .expect("create patient");
    let create_status = resp.status();
    let created: Value = resp.json().await.expect("parse create response");
    assert!(create_status.is_success(), "create failed: {created}");
    let patient_id = created["id"].as_str().expect("id");

    let bundle = json!({
        "resourceType": "Bundle",
        "type": "transaction",
        "entry": [
            {
                "resource": {
                    "resourceType": "Patient",
                    "id": patient_id,
                    "name": [{"family": "PreferMinimal", "given": ["After"]}]
                },
                "request": {
                    "method": "PUT",
                    "url": format!("Patient/{}", patient_id)
                }
            },
            {
                "request": {
                    "method": "GET",
                    "url": "Patient?family=PreferMinimal"
                }
            }
        ]
    });

    let resp = client
        .post(&base)
        .header("content-type", "application/fhir+json")
        .header("prefer", "return=minimal")
        .json(&bundle)
        .send()
        .await
        .expect("transaction request");
    let status = resp.status();
    let response_bundle: Value = resp.json().await.expect("parse response");
    assert!(status.is_success(), "transaction failed: {response_bundle}");
    let entries = response_bundle["entry"].as_array().expect("entries");
    assert_eq!(entries.len(), 2);
    assert!(
        entries[0].get("resource").is_none(),
        "return=minimal must omit mutation response resources"
    );
    assert_eq!(entries[1]["resource"]["resourceType"], "Bundle");
    assert_eq!(
        entries[1]["resource"]["entry"][0]["resource"]["name"][0]["given"][0],
        "After"
    );

    let _ = shutdown_tx.send(());
}

#[tokio::test]
async fn test_batch_prefer_return_minimal_omits_mutation_resources() {
    let (_container, postgres_url) = start_postgres().await;
    let config = create_config(&postgres_url);
    let (base, shutdown_tx, _handle) = start_server(&config).await;
    let client = reqwest::Client::new();

    let bundle = json!({
        "resourceType": "Bundle",
        "type": "batch",
        "entry": [
            {
                "resource": {
                    "resourceType": "Patient",
                    "name": [{"family": "BatchPreferMinimal"}]
                },
                "request": {
                    "method": "POST",
                    "url": "Patient"
                }
            }
        ]
    });

    let resp = client
        .post(&base)
        .header("content-type", "application/fhir+json")
        .header("prefer", "return=minimal")
        .json(&bundle)
        .send()
        .await
        .expect("batch request");
    assert!(resp.status().is_success());

    let response_bundle: Value = resp.json().await.expect("parse response");
    let entries = response_bundle["entry"].as_array().expect("entries");
    assert_eq!(entries.len(), 1);
    assert!(
        entries[0].get("resource").is_none(),
        "batch return=minimal must omit mutation response resources"
    );
    assert!(
        entries[0]["response"]["status"]
            .as_str()
            .unwrap_or("")
            .starts_with("201")
    );

    let search_resp = client
        .get(format!("{base}/Patient?family=BatchPreferMinimal"))
        .header("accept", "application/fhir+json")
        .send()
        .await
        .expect("search request");
    let search_bundle: Value = search_resp.json().await.expect("parse search response");
    assert_eq!(search_bundle["total"].as_u64().unwrap_or(0), 1);

    let _ = shutdown_tx.send(());
}

// =============================================================================
// Concurrent Transaction Tests (Isolation)
// =============================================================================

#[tokio::test]
async fn test_concurrent_transactions_isolation() {
    let (_container, postgres_url) = start_postgres().await;
    let config = create_config(&postgres_url);
    let (base, shutdown_tx, _handle) = start_server(&config).await;
    let client = reqwest::Client::new();

    // Create initial patient
    let patient = json!({
        "resourceType": "Patient",
        "name": [{"family": "ConcurrentTest", "given": ["Initial"]}],
        "birthDate": "2000-01-01"
    });

    let resp = client
        .post(format!("{base}/Patient"))
        .header("content-type", "application/fhir+json")
        .json(&patient)
        .send()
        .await
        .expect("create patient");

    let created: Value = resp.json().await.expect("parse");
    let patient_id = created["id"].as_str().expect("id").to_string();
    let base_clone = base.clone();

    // Launch two concurrent transactions updating different fields
    let id1 = patient_id.clone();
    let base1 = base_clone.clone();
    let tx1 = tokio::spawn(async move {
        let client = reqwest::Client::new();
        let bundle = json!({
            "resourceType": "Bundle",
            "type": "transaction",
            "entry": [{
                "resource": {
                    "resourceType": "Patient",
                    "id": id1,
                    "name": [{"family": "ConcurrentTest", "given": ["FromTx1"]}],
                    "birthDate": "2000-01-01"
                },
                "request": {
                    "method": "PUT",
                    "url": format!("Patient/{}", id1)
                }
            }]
        });

        client
            .post(&base1)
            .header("content-type", "application/fhir+json")
            .json(&bundle)
            .send()
            .await
    });

    let id2 = patient_id.clone();
    let base2 = base_clone;
    let tx2 = tokio::spawn(async move {
        let client = reqwest::Client::new();
        let bundle = json!({
            "resourceType": "Bundle",
            "type": "transaction",
            "entry": [{
                "resource": {
                    "resourceType": "Patient",
                    "id": id2,
                    "name": [{"family": "ConcurrentTest", "given": ["FromTx2"]}],
                    "birthDate": "2000-01-01"
                },
                "request": {
                    "method": "PUT",
                    "url": format!("Patient/{}", id2)
                }
            }]
        });

        client
            .post(&base2)
            .header("content-type", "application/fhir+json")
            .json(&bundle)
            .send()
            .await
    });

    // Wait for both transactions
    let (result1, result2) = tokio::join!(tx1, tx2);

    let resp1 = result1.expect("tx1").expect("tx1 response");
    let resp2 = result2.expect("tx2").expect("tx2 response");

    // At least one should succeed (the other might fail due to optimistic locking)
    let success_count = [resp1.status().is_success(), resp2.status().is_success()]
        .iter()
        .filter(|&&x| x)
        .count();

    assert!(
        success_count >= 1,
        "At least one concurrent transaction should succeed: {}, {}",
        resp1.status(),
        resp2.status()
    );

    // Final state should be consistent (one of the two values)
    let final_patient = read_resource(&client, &base, "Patient", &patient_id)
        .await
        .expect("patient should exist");

    let given = final_patient["name"][0]["given"][0].as_str().unwrap_or("");
    assert!(
        given == "FromTx1" || given == "FromTx2",
        "Final state should reflect one of the transactions"
    );

    let _ = shutdown_tx.send(());
}

// =============================================================================
// Reference Resolution Tests
// =============================================================================

#[tokio::test]
async fn test_transaction_internal_reference_resolution() {
    let (_container, postgres_url) = start_postgres().await;
    let config = create_config(&postgres_url);
    let (base, shutdown_tx, _handle) = start_server(&config).await;
    let client = reqwest::Client::new();

    // Transaction with internal references (urn:uuid)
    let bundle = json!({
        "resourceType": "Bundle",
        "type": "transaction",
        "entry": [
            {
                "fullUrl": "urn:uuid:new-patient",
                "resource": {
                    "resourceType": "Patient",
                    "name": [{"family": "ReferenceTest"}]
                },
                "request": {
                    "method": "POST",
                    "url": "Patient"
                }
            },
            {
                "fullUrl": "urn:uuid:new-encounter",
                "resource": {
                    "resourceType": "Encounter",
                    "status": "finished",
                    "class": {
                        "system": "http://terminology.hl7.org/CodeSystem/v3-ActCode",
                        "code": "AMB"
                    },
                    "subject": {"reference": "urn:uuid:new-patient"}
                },
                "request": {
                    "method": "POST",
                    "url": "Encounter"
                }
            },
            {
                "fullUrl": "urn:uuid:new-condition",
                "resource": {
                    "resourceType": "Condition",
                    "subject": {"reference": "urn:uuid:new-patient"},
                    "encounter": {"reference": "urn:uuid:new-encounter"},
                    "code": {
                        "coding": [{"system": "http://snomed.info/sct", "code": "386661006"}]
                    }
                },
                "request": {
                    "method": "POST",
                    "url": "Condition"
                }
            }
        ]
    });

    let resp = client
        .post(&base)
        .header("content-type", "application/fhir+json")
        .json(&bundle)
        .send()
        .await
        .expect("transaction request");

    assert!(
        resp.status().is_success(),
        "Transaction with internal refs should succeed"
    );

    let response_bundle: Value = resp.json().await.expect("parse response");
    let entries = response_bundle["entry"].as_array().expect("entries");

    // Extract created IDs
    let mut patient_id = String::new();
    let mut encounter_id = String::new();
    let mut condition_id = String::new();

    for entry in entries {
        let resource = &entry["resource"];
        let id = resource["id"].as_str().unwrap();
        match resource["resourceType"].as_str().unwrap() {
            "Patient" => patient_id = id.to_owned(),
            "Encounter" => encounter_id = id.to_owned(),
            "Condition" => condition_id = id.to_owned(),
            resource_type => panic!("Unexpected transaction resource: {resource_type}"),
        }
        assert_eq!(
            entry["fullUrl"],
            format!(
                "{}/fhir/{}/{}",
                config.base_url(),
                resource["resourceType"].as_str().unwrap(),
                id
            )
        );
    }

    assert!(!patient_id.is_empty(), "Patient should be created");
    assert!(!encounter_id.is_empty(), "Encounter should be created");
    assert!(!condition_id.is_empty(), "Condition should be created");

    // Verify references were resolved
    let condition = read_resource(&client, &base, "Condition", &condition_id)
        .await
        .expect("condition should exist");

    let subject_ref = condition["subject"]["reference"].as_str().unwrap_or("");
    let encounter_ref = condition["encounter"]["reference"].as_str().unwrap_or("");

    assert!(
        subject_ref.contains(&patient_id),
        "Subject reference should be resolved to Patient/{}",
        patient_id
    );
    assert!(
        encounter_ref.contains(&encounter_id),
        "Encounter reference should be resolved to Encounter/{}",
        encounter_id
    );

    let _ = shutdown_tx.send(());
}

// =============================================================================
// Transaction with Delete Tests
// =============================================================================

#[tokio::test]
async fn test_transaction_with_delete() {
    let (_container, postgres_url) = start_postgres().await;
    let config = create_config(&postgres_url);
    let (base, shutdown_tx, _handle) = start_server(&config).await;
    let client = reqwest::Client::new();

    // First create a patient
    let patient = json!({
        "resourceType": "Patient",
        "name": [{"family": "ToDelete"}]
    });

    let resp = client
        .post(format!("{base}/Patient"))
        .header("content-type", "application/fhir+json")
        .json(&patient)
        .send()
        .await
        .expect("create patient");

    let created: Value = resp.json().await.expect("parse");
    let patient_id = created["id"].as_str().expect("id");

    // Transaction that deletes the patient and creates a new one
    let bundle = json!({
        "resourceType": "Bundle",
        "type": "transaction",
        "entry": [
            {
                "request": {
                    "method": "DELETE",
                    "url": format!("Patient/{}", patient_id)
                }
            },
            {
                "resource": {
                    "resourceType": "Patient",
                    "name": [{"family": "ReplacementPatient"}]
                },
                "request": {
                    "method": "POST",
                    "url": "Patient"
                }
            }
        ]
    });

    let resp = client
        .post(&base)
        .header("content-type", "application/fhir+json")
        .json(&bundle)
        .send()
        .await
        .expect("transaction request");

    assert!(
        resp.status().is_success(),
        "Transaction with delete should succeed"
    );

    // Verify deleted patient no longer exists
    assert!(
        !resource_exists(&client, &base, "Patient", patient_id).await,
        "Deleted patient should not exist"
    );

    // Verify new patient exists
    let search_resp = client
        .get(format!("{base}/Patient?family=ReplacementPatient"))
        .header("accept", "application/fhir+json")
        .send()
        .await
        .expect("search request");

    let search_bundle: Value = search_resp.json().await.expect("parse");
    let total = search_bundle["total"].as_u64().unwrap_or(0);

    assert_eq!(total, 1, "New patient should exist");

    let _ = shutdown_tx.send(());
}

// =============================================================================
// Conditional Operations Tests
// =============================================================================

#[tokio::test]
async fn test_transaction_conditional_create() {
    let (_container, postgres_url) = start_postgres().await;
    let config = create_config(&postgres_url);
    let (base, shutdown_tx, _handle) = start_server(&config).await;
    let client = reqwest::Client::new();

    // First create a patient with a specific identifier
    let patient = json!({
        "resourceType": "Patient",
        "identifier": [{"system": "http://example.org/mrn", "value": "COND-001"}],
        "name": [{"family": "ConditionalFirst"}]
    });

    let resp = client
        .post(format!("{base}/Patient"))
        .header("content-type", "application/fhir+json")
        .json(&patient)
        .send()
        .await
        .expect("create patient");

    assert!(resp.status().is_success());

    // Transaction with conditional create (if-none-exist)
    // Should NOT create a duplicate
    let bundle = json!({
        "resourceType": "Bundle",
        "type": "transaction",
        "entry": [
            {
                "resource": {
                    "resourceType": "Patient",
                    "identifier": [{"system": "http://example.org/mrn", "value": "COND-001"}],
                    "name": [{"family": "ConditionalSecond"}]
                },
                "request": {
                    "method": "POST",
                    "url": "Patient",
                    "ifNoneExist": "identifier=http://example.org/mrn|COND-001"
                }
            }
        ]
    });

    let resp = client
        .post(&base)
        .header("content-type", "application/fhir+json")
        .json(&bundle)
        .send()
        .await
        .expect("transaction request");

    // Should succeed (either created or found existing)
    assert!(
        resp.status().is_success(),
        "Conditional create transaction should succeed"
    );

    // Verify only one patient with this identifier
    let search_resp = client
        .get(format!(
            "{base}/Patient?identifier=http://example.org/mrn|COND-001"
        ))
        .header("accept", "application/fhir+json")
        .send()
        .await
        .expect("search request");

    let search_bundle: Value = search_resp.json().await.expect("parse");
    let total = search_bundle["total"].as_u64().unwrap_or(0);

    assert_eq!(
        total, 1,
        "Should have exactly one patient with this identifier"
    );

    let _ = shutdown_tx.send(());
}

#[tokio::test]
async fn test_transaction_conditional_update() {
    let (_container, postgres_url) = start_postgres().await;
    let config = create_config(&postgres_url);
    let (base, shutdown_tx, _handle) = start_server(&config).await;
    let client = reqwest::Client::new();

    let patient = json!({
        "resourceType": "Patient",
        "identifier": [{"system": "http://example.org/mrn", "value": "COND-UPD-001"}],
        "name": [{"family": "ConditionalUpdate", "given": ["Before"]}]
    });

    let resp = client
        .post(format!("{base}/Patient"))
        .header("content-type", "application/fhir+json")
        .json(&patient)
        .send()
        .await
        .expect("create patient");

    assert!(resp.status().is_success());

    let bundle = json!({
        "resourceType": "Bundle",
        "type": "transaction",
        "entry": [
            {
                "resource": {
                    "resourceType": "Patient",
                    "identifier": [{"system": "http://example.org/mrn", "value": "COND-UPD-001"}],
                    "name": [{"family": "ConditionalUpdate", "given": ["After"]}]
                },
                "request": {
                    "method": "PUT",
                    "url": "Patient?identifier=http://example.org/mrn|COND-UPD-001"
                }
            }
        ]
    });

    let resp = client
        .post(&base)
        .header("content-type", "application/fhir+json")
        .json(&bundle)
        .send()
        .await
        .expect("transaction request");

    let status = resp.status();
    let outcome: Value = resp.json().await.expect("conditional update response");
    assert!(
        status.is_success(),
        "Conditional update should succeed: {status} {outcome}"
    );

    let search_resp = client
        .get(format!(
            "{base}/Patient?identifier=http://example.org/mrn|COND-UPD-001"
        ))
        .header("accept", "application/fhir+json")
        .send()
        .await
        .expect("search request");

    let search_bundle: Value = search_resp.json().await.expect("parse");
    assert_eq!(search_bundle["total"].as_u64().unwrap_or(0), 1);
    assert_eq!(
        search_bundle["entry"][0]["resource"]["name"][0]["given"][0],
        "After"
    );

    let _ = shutdown_tx.send(());
}

#[tokio::test]
async fn test_transaction_conditional_delete() {
    let (_container, postgres_url) = start_postgres().await;
    let config = create_config(&postgres_url);
    let (base, shutdown_tx, _handle) = start_server(&config).await;
    let client = reqwest::Client::new();

    let patient = json!({
        "resourceType": "Patient",
        "identifier": [{"system": "http://example.org/mrn", "value": "COND-DEL-001"}],
        "name": [{"family": "ConditionalDelete"}]
    });

    let resp = client
        .post(format!("{base}/Patient"))
        .header("content-type", "application/fhir+json")
        .json(&patient)
        .send()
        .await
        .expect("create patient");

    assert!(resp.status().is_success());

    let bundle = json!({
        "resourceType": "Bundle",
        "type": "transaction",
        "entry": [
            {
                "request": {
                    "method": "DELETE",
                    "url": "Patient?identifier=http://example.org/mrn|COND-DEL-001"
                }
            }
        ]
    });

    let resp = client
        .post(&base)
        .header("content-type", "application/fhir+json")
        .json(&bundle)
        .send()
        .await
        .expect("transaction request");

    let status = resp.status();
    let outcome: Value = resp.json().await.expect("conditional delete response");
    assert!(
        status.is_success(),
        "Conditional delete should succeed: {status} {outcome}"
    );

    let search_resp = client
        .get(format!(
            "{base}/Patient?identifier=http://example.org/mrn|COND-DEL-001"
        ))
        .header("accept", "application/fhir+json")
        .send()
        .await
        .expect("search request");

    let search_bundle: Value = search_resp.json().await.expect("parse");
    assert_eq!(search_bundle["total"].as_u64().unwrap_or(0), 0);

    let _ = shutdown_tx.send(());
}

#[tokio::test]
async fn test_transaction_patch_entry() {
    let (_container, postgres_url) = start_postgres().await;
    let config = create_config(&postgres_url);
    let (base, shutdown_tx, _handle) = start_server(&config).await;
    let client = reqwest::Client::new();

    let patient = json!({
        "resourceType": "Patient",
        "name": [{"family": "PatchInTransaction", "given": ["Before"]}]
    });

    let resp = client
        .post(format!("{base}/Patient"))
        .header("content-type", "application/fhir+json")
        .json(&patient)
        .send()
        .await
        .expect("create patient");

    assert!(resp.status().is_success());
    let created: Value = resp.json().await.expect("parse");
    let patient_id = created["id"].as_str().expect("id");

    let bundle = json!({
        "resourceType": "Bundle",
        "type": "transaction",
        "entry": [
            {
                "resource": {
                    "resourceType": "Parameters",
                    "parameter": [{"name": "operation", "part": [
                        {"name": "type", "valueCode": "replace"},
                        {"name": "path", "valueString": "Patient.name[0].given[0]"},
                        {"name": "value", "valueString": "After"}
                    ]}]
                },
                "request": {
                    "method": "PATCH",
                    "url": format!("Patient/{}", patient_id)
                }
            }
        ]
    });

    let resp = client
        .post(&base)
        .header("content-type", "application/fhir+json")
        .json(&bundle)
        .send()
        .await
        .expect("transaction request");

    assert!(
        resp.status().is_success(),
        "PATCH in transaction should succeed"
    );

    let patched = read_resource(&client, &base, "Patient", patient_id)
        .await
        .expect("patient should exist");

    assert_eq!(patched["name"][0]["given"][0], "After");

    let _ = shutdown_tx.send(());
}

#[tokio::test]
async fn test_transaction_get_search_sees_uncommitted_create() {
    let (_container, postgres_url) = start_postgres().await;
    let config = create_config(&postgres_url);
    let (base, shutdown_tx, _handle) = start_server(&config).await;
    let client = reqwest::Client::new();

    let bundle = json!({
        "resourceType": "Bundle",
        "type": "transaction",
        "entry": [
            {
                "resource": {
                    "resourceType": "Patient",
                    "identifier": [{"system": "http://example.org/mrn", "value": "TX-SEARCH-001"}],
                    "name": [{"family": "TxSearch"}]
                },
                "request": {
                    "method": "POST",
                    "url": "Patient"
                }
            },
            {
                "request": {
                    "method": "GET",
                    "url": "Patient?identifier=http://example.org/mrn|TX-SEARCH-001"
                }
            }
        ]
    });

    let resp = client
        .post(&base)
        .header("content-type", "application/fhir+json")
        .json(&bundle)
        .send()
        .await
        .expect("transaction request");

    assert!(
        resp.status().is_success(),
        "GET search in transaction should succeed"
    );

    let response_bundle: Value = resp.json().await.expect("parse response");
    let entries = response_bundle["entry"].as_array().expect("entries");
    let search_bundle = &entries[1]["resource"];

    assert_eq!(search_bundle["resourceType"], "Bundle");
    assert_eq!(search_bundle["type"], "searchset");
    assert_eq!(search_bundle["total"].as_u64().unwrap_or(0), 1);

    let _ = shutdown_tx.send(());
}

// =============================================================================
// Large Transaction Tests
// =============================================================================

#[tokio::test]
async fn test_large_transaction_performance() {
    let (_container, postgres_url) = start_postgres().await;
    let config = create_config(&postgres_url);
    let (base, shutdown_tx, _handle) = start_server(&config).await;
    let client = reqwest::Client::new();

    // Create a transaction with many entries
    let mut entries = Vec::new();
    for i in 0..50 {
        entries.push(json!({
            "resource": {
                "resourceType": "Patient",
                "name": [{"family": format!("BulkPatient{}", i)}],
                "birthDate": "2000-01-01"
            },
            "request": {
                "method": "POST",
                "url": "Patient"
            }
        }));
    }

    let bundle = json!({
        "resourceType": "Bundle",
        "type": "transaction",
        "entry": entries
    });

    let start = std::time::Instant::now();

    let resp = client
        .post(&base)
        .header("content-type", "application/fhir+json")
        .json(&bundle)
        .send()
        .await
        .expect("transaction request");

    let duration = start.elapsed();

    assert!(
        resp.status().is_success(),
        "Large transaction should succeed"
    );

    let response_bundle: Value = resp.json().await.expect("parse response");
    let response_entries = response_bundle["entry"].as_array().expect("entries");

    assert_eq!(
        response_entries.len(),
        50,
        "All entries should be processed"
    );

    // Performance check: 50 entries should complete in reasonable time
    // This is a soft assertion - mainly for benchmarking
    println!(
        "Large transaction (50 entries) completed in: {:?}",
        duration
    );
    assert!(
        duration.as_secs() < 30,
        "Large transaction should complete within 30 seconds"
    );

    // Verify all patients exist
    let search_resp = client
        .get(format!(
            "{base}/Patient?name:contains=BulkPatient&_count=100"
        ))
        .header("accept", "application/fhir+json")
        .send()
        .await
        .expect("search request");

    let search_bundle: Value = search_resp.json().await.expect("parse");
    let total = search_bundle["total"].as_u64().unwrap_or(0);

    assert_eq!(total, 50, "All 50 bulk patients should exist");

    let parent = response_entries[0]["resource"]["id"].as_str().unwrap();
    let linked_entries: Vec<_> = (0..50).map(|_| json!({
        "request":{"method":"POST","url":"Observation"},
        "resource":{"resourceType":"Observation","status":"final","code":{"text":"load regression"},
            "subject":{"reference":format!("Patient/{parent}")}}
    })).collect();
    let linked = json!({"resourceType":"Bundle","type":"transaction","entry":linked_entries});
    for concurrency in [1, 4, 16] {
        let start = std::time::Instant::now();
        let results = futures_util::future::join_all((0..concurrency).map(|_| async {
            let response = client.post(&base).json(&linked).send().await.unwrap();
            let status = response.status();
            let body: Value = response.json().await.unwrap();
            assert!(status.is_success(), "{body}");
            assert_eq!(body["entry"].as_array().unwrap().len(), 50);
        }))
        .await;
        let elapsed = start.elapsed();
        println!(
            "Reference-heavy transaction: {} concurrent requests x 50 entries, one unique target: {:?}",
            results.len(),
            elapsed
        );
        assert!(
            elapsed.as_secs() < 30,
            "reference-heavy concurrency regression: {elapsed:?}"
        );
    }

    let _ = shutdown_tx.send(());
}

/// A transaction Bundle with an invalid POST entry must be rejected with 422 +
/// OperationOutcome (same as a single POST create), and — being atomic — must NOT
/// commit the valid sibling. A Patient without `name` fails validation.
#[tokio::test]
async fn test_transaction_invalid_resource_returns_422_and_rolls_back() {
    let (_container, postgres_url) = start_postgres().await;
    let config = create_config(&postgres_url);
    let (base, shutdown_tx, _handle) = start_server(&config).await;
    let client = reqwest::Client::new();

    let bundle = json!({
        "resourceType": "Bundle",
        "type": "transaction",
        "entry": [
            {
                "fullUrl": "urn:uuid:valid-1",
                "resource": {"resourceType": "Patient", "name": [{"family": "TxValid"}]},
                "request": {"method": "POST", "url": "Patient"}
            },
            {
                // Invalid: Patient with no required name element.
                "fullUrl": "urn:uuid:invalid-1",
                "resource": {"resourceType": "Patient", "active": "not-a-boolean"},
                "request": {"method": "POST", "url": "Patient"}
            }
        ]
    });

    let resp = client
        .post(&base)
        .header("content-type", "application/fhir+json")
        .json(&bundle)
        .send()
        .await
        .expect("transaction request");

    assert_eq!(
        resp.status(),
        reqwest::StatusCode::UNPROCESSABLE_ENTITY,
        "invalid resource in a transaction Bundle must return 422"
    );
    let body: Value = resp.json().await.expect("parse response");
    assert_eq!(
        body["resourceType"], "OperationOutcome",
        "422 body should be an OperationOutcome"
    );

    // Atomic rollback: the valid sibling must not have been committed.
    let search = client
        .get(format!("{base}/Patient?family=TxValid"))
        .header("accept", "application/fhir+json")
        .send()
        .await
        .expect("search request");
    let sb: Value = search.json().await.expect("parse search");
    assert_eq!(
        sb["total"].as_u64().unwrap_or(0),
        0,
        "transaction must roll back; the valid sibling must not persist"
    );

    let _ = shutdown_tx.send(());
}

/// A batch Bundle is non-atomic: an invalid POST entry returns a per-entry 422
/// OperationOutcome while valid siblings still persist.
#[tokio::test]
async fn test_batch_invalid_resource_per_entry_422_siblings_persist() {
    let (_container, postgres_url) = start_postgres().await;
    let config = create_config(&postgres_url);
    let (base, shutdown_tx, _handle) = start_server(&config).await;
    let client = reqwest::Client::new();

    let bundle = json!({
        "resourceType": "Bundle",
        "type": "batch",
        "entry": [
            {
                "resource": {"resourceType": "Patient", "name": [{"family": "BatchValid"}]},
                "request": {"method": "POST", "url": "Patient"}
            },
            {
                // Invalid: Patient with no required name element.
                "resource": {"resourceType": "Patient", "active": "not-a-boolean"},
                "request": {"method": "POST", "url": "Patient"}
            }
        ]
    });

    let resp = client
        .post(&base)
        .header("content-type", "application/fhir+json")
        .json(&bundle)
        .send()
        .await
        .expect("batch request");

    let response_bundle: Value = resp.json().await.expect("parse response");
    assert_eq!(response_bundle["type"], "batch-response");
    let entries = response_bundle["entry"].as_array().expect("entries");
    assert_eq!(entries.len(), 2);

    let first_status = entries[0]["response"]["status"].as_str().unwrap_or("");
    assert!(
        first_status.starts_with("201"),
        "valid batch entry should be created, got {first_status}"
    );
    let second_status = entries[1]["response"]["status"].as_str().unwrap_or("");
    assert!(
        second_status.starts_with("422"),
        "invalid batch entry should return 422, got {second_status}"
    );
    assert_eq!(
        entries[1]["response"]["outcome"]["resourceType"], "OperationOutcome",
        "invalid batch entry should carry an OperationOutcome"
    );

    // Valid sibling persisted despite the invalid one failing.
    let search = client
        .get(format!("{base}/Patient?family=BatchValid"))
        .header("accept", "application/fhir+json")
        .send()
        .await
        .expect("search request");
    let sb: Value = search.json().await.expect("parse search");
    assert!(
        sb["total"].as_u64().unwrap_or(0) > 0,
        "valid batch sibling must persist"
    );

    let _ = shutdown_tx.send(());
}

#[tokio::test]
async fn transaction_search_counts_and_includes_observe_own_writes_with_one_connection() {
    use reqwest::StatusCode;
    let (_container, postgres_url) = start_postgres().await;
    let mut config = create_config(&postgres_url);
    config.storage.postgres.as_mut().unwrap().pool_size = 1;
    let (base, shutdown, _server) = start_server(&config).await;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .unwrap();
    for rollback in [false, true] {
        let identifier = if rollback {
            "rollback-search"
        } else {
            "commit-search"
        };
        let mut entries = vec![
            json!({"fullUrl":"urn:uuid:new-patient", "resource":{"resourceType":"Patient", "identifier":[{"system":"urn:test:tx-search", "value":identifier}]}, "request":{"method":"POST", "url":"Patient"}}),
            json!({"resource":{"resourceType":"Observation", "status":"final", "code":{"coding":[{"system":"urn:test:tx-search", "code":identifier}]}, "subject":{"reference":"urn:uuid:new-patient"}}, "request":{"method":"POST", "url":"Observation"}}),
            json!({"request":{"method":"GET", "url":format!("Observation?code=urn%3Atest%3Atx-search%7C{identifier}&_total=accurate&_count=1&_include=Observation:subject:Patient")}}),
            json!({"request":{"method":"GET", "url":format!("Patient?identifier=urn%3Atest%3Atx-search%7C{identifier}&_summary=count")}}),
        ];
        if rollback {
            entries
                .push(json!({"request":{"method":"GET", "url":"Patient/missing-search-rollback"}}));
        }
        let (status, body) = send_bundle(&client, &base, "transaction", json!(entries)).await;
        if rollback {
            assert!(status.is_client_error(), "{body}");
        } else {
            assert_eq!(status, StatusCode::OK, "{body}");
            let patient_id = body["entry"][0]["resource"]["id"].as_str().unwrap();
            let nested = &body["entry"][2]["resource"];
            assert_eq!(nested["total"], 1, "{nested}");
            assert_eq!(nested["entry"].as_array().unwrap().len(), 2, "{nested}");
            assert_eq!(nested["entry"][0]["search"]["mode"], "match");
            assert_eq!(nested["entry"][1]["search"]["mode"], "include");
            assert_eq!(nested["entry"][1]["resource"]["id"], patient_id);
            assert_eq!(body["entry"][3]["resource"]["total"], 1);
            assert!(
                body["entry"][3]["resource"]["entry"]
                    .as_array()
                    .is_none_or(Vec::is_empty)
            );
        }
        let response = client
            .get(format!(
                "{base}/Patient?identifier=urn%3Atest%3Atx-search%7C{identifier}&_total=accurate"
            ))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let stored: Value = response.json().await.unwrap();
        assert_eq!(stored["total"], usize::from(!rollback), "{stored}");
    }
    let _ = shutdown.send(());
}

#[tokio::test]
async fn transaction_conditional_references_resolve_proposed_graph_and_roll_back_ambiguity() {
    use reqwest::StatusCode;
    let (_container, postgres_url) = start_postgres().await;
    let config = create_config(&postgres_url);
    let (base, shutdown, _server) = start_server(&config).await;
    let client = reqwest::Client::new();
    let observation = |criterion: &str| {
        json!({"resourceType":"Observation","status":"final",
        "code":{"text":"graph reference"},"subject":{"reference":criterion}})
    };
    // The result must be independent of entry order. No pool search can find
    // these targets before their Bundle commits.
    for reverse in [false, true] {
        let value = format!("new-{reverse}");
        let criterion = format!("Patient?identifier=urn:graph|{value}&_count=0&_offset=999");
        let mut entries = vec![
            json!({"request":{"method":"POST","url":"Patient"},
                "resource":{"resourceType":"Patient","identifier":[{"system":"urn:graph","value":value}]}}),
            json!({"request":{"method":"POST","url":"Observation"},"resource":observation(&criterion)}),
        ];
        if reverse {
            entries.reverse();
        }
        let (status, body) = send_bundle(&client, &base, "transaction", json!(entries)).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let patient_index = usize::from(reverse);
        let observation_index = usize::from(!reverse);
        let id = body["entry"][patient_index]["resource"]["id"]
            .as_str()
            .unwrap();
        let obs_id = body["entry"][observation_index]["resource"]["id"]
            .as_str()
            .unwrap();
        let saved = read_resource(&client, &base, "Observation", obs_id)
            .await
            .unwrap();
        assert_eq!(saved["subject"]["reference"], format!("Patient/{id}"));
    }
    // A reference predicate may depend on another conditional reference.
    let (status, body) = send_bundle(&client, &base, "transaction", json!([
        {"request":{"method":"POST","url":"DiagnosticReport"},
         "resource":{"resourceType":"DiagnosticReport","status":"final","code":{"text":"dependency"},
          "result":[{"reference":"Observation?subject:Patient.identifier=urn:graph|dependency&code=graph-code"}]}},
        {"request":{"method":"POST","url":"Observation"},
         "resource":{"resourceType":"Observation","status":"final","code":{"coding":[{"code":"graph-code"}]},
          "subject":{"reference":"Patient?identifier=urn:graph|dependency"}}},
        {"request":{"method":"POST","url":"Patient"},
         "resource":{"resourceType":"Patient","identifier":[{"system":"urn:graph","value":"dependency"}]}}
    ])).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["entry"][0]["resource"]["result"][0]["reference"],
        format!(
            "Observation/{}",
            body["entry"][1]["resource"]["id"].as_str().unwrap()
        )
    );

    let old: Value = client.post(format!("{base}/Patient"))
        .json(&json!({"resourceType":"Patient","identifier":[{"system":"urn:graph","value":"replace-old"}]}))
        .send().await.unwrap().json().await.unwrap();
    let id = old["id"].as_str().unwrap();
    let (status, body) = send_bundle(&client,&base,"transaction",json!([
        {"request":{"method":"PUT","url":format!("Patient/{id}")},
         "resource":{"resourceType":"Patient","id":id,"identifier":[{"system":"urn:graph","value":"replace-new"}]}},
        {"request":{"method":"POST","url":"Observation"},
         "resource":observation("Patient?identifier=urn:graph|replace-new")}
    ])).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["entry"][1]["resource"]["subject"]["reference"],
        format!("Patient/{id}")
    );
    let before = read_resource(&client, &base, "Patient", id).await.unwrap();
    for entries in [
        json!([
            {"request":{"method":"DELETE","url":format!("Patient/{id}")}},
            {"request":{"method":"POST","url":"Observation"},"resource":observation("Patient?identifier=urn:graph|replace-new")}
        ]),
        json!([
            {"request":{"method":"POST","url":"Patient"},"resource":{"resourceType":"Patient","identifier":[{"system":"urn:graph","value":"replace-new"}]}},
            {"request":{"method":"POST","url":"Observation"},"resource":observation("Patient?identifier=urn:graph|replace-new")}
        ]),
    ] {
        let (status, body) = send_bundle(&client, &base, "transaction", entries).await;
        assert!(
            status == StatusCode::UNPROCESSABLE_ENTITY || status == StatusCode::PRECONDITION_FAILED,
            "{status} {body}"
        );
        assert_eq!(
            read_resource(&client, &base, "Patient", id).await.unwrap(),
            before
        );
        let history: Value = client
            .get(format!("{base}/Patient/{id}/_history"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(history["entry"].as_array().unwrap().len(), 2, "{history}");
        let search: Value = client
            .get(format!("{base}/Patient?identifier=urn:graph|replace-new"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(search["entry"].as_array().unwrap().len(), 1, "{search}");
    }
    // A matched conditional POST ignores its incoming references, too.
    let (status, body) = send_bundle(&client,&base,"transaction",json!([{
        "request":{"method":"POST","url":"Patient","ifNoneExist":"identifier=urn:graph|replace-new"},
        "resource":{"resourceType":"Patient","managingOrganization":{"reference":"Organization?identifier=missing"},
        "generalPractitioner":[{"reference":"urn:uuid:ignored-unresolved"}]}
    }])).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["entry"][0]["resource"], before);
    let _ = shutdown.send(());
}

#[tokio::test]
async fn transaction_conditional_writes_match_completed_phases_and_patch() {
    use base64::{Engine, engine::general_purpose::STANDARD};
    use reqwest::StatusCode;
    let (_container, postgres_url) = start_postgres().await;
    let config = create_config(&postgres_url);
    let (base, shutdown, _server) = start_server(&config).await;
    let client = reqwest::Client::new();
    for reverse in [false, true] {
        let marker = format!("phase-{reverse}");
        let patient: Value = client.post(format!("{base}/Patient"))
            .json(&json!({"resourceType":"Patient","active":true,"identifier":[{"system":"urn:phase","value":marker}]}))
            .send().await.unwrap().json().await.unwrap();
        let old_id = patient["id"].as_str().unwrap();
        let mut entries = vec![
            json!({"request":{"method":"POST","url":"Patient","ifNoneExist":format!("identifier=urn:phase|{marker}")},
                "resource":{"resourceType":"Patient","active":false,"identifier":[{"system":"urn:phase","value":marker}]}}),
            json!({"request":{"method":"DELETE","url":format!("Patient/{old_id}")}}),
        ];
        if reverse {
            entries.reverse();
        }
        let (status, body) = send_bundle(&client, &base, "transaction", json!(entries)).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let created = &body["entry"][usize::from(reverse)];
        assert_eq!(created["response"]["status"], "201 Created", "{body}");
        let id = created["resource"]["id"].as_str().unwrap();
        assert_ne!(id, old_id);
        assert_eq!(created["resource"]["active"], false);
        assert!(
            read_resource(&client, &base, "Patient", old_id)
                .await
                .is_none()
        );
        // PUT and PATCH conditions see POST resources, including reverse chains.
        for method in ["PUT", "PATCH"] {
            let code = format!("{marker}-{method}");
            let candidate = if method == "PUT" {
                json!({"resourceType":"Patient","active":true,"identifier":[{"system":"urn:phase","value":marker}]})
            } else {
                json!({"resourceType":"Binary","contentType":"application/json-patch+json",
                    "id":"patch-instructions",
                    "data":STANDARD.encode(serde_json::to_vec(&json!([{"op":"add","path":"/birthDate","value":"2000-01-01"}])).unwrap())})
            };
            let mut entries = vec![
                json!({"request":{"method":method,"url":format!("Patient?_id={id}&_has:Observation:subject:code={code}")},"resource":candidate}),
                json!({"request":{"method":"POST","url":"Observation"},"resource":{"resourceType":"Observation","status":"final",
                    "code":{"coding":[{"code":code}]},"subject":{"reference":format!("Patient/{id}")}}}),
            ];
            if reverse {
                entries.reverse();
            }
            let (status, body) = send_bundle(&client, &base, "transaction", json!(entries)).await;
            assert_eq!(status, StatusCode::OK, "{method}: {body}");
            let updated = &body["entry"][usize::from(reverse)];
            assert_eq!(updated["response"]["status"], "200 OK", "{body}");
            assert_eq!(updated["resource"]["id"], id);
            if method == "PUT" {
                assert_eq!(updated["resource"]["active"], true);
            } else {
                assert_eq!(updated["resource"]["birthDate"], "2000-01-01");
            }
        }
    }
    let _ = shutdown.send(());
}

#[tokio::test]
async fn transaction_reference_dependencies_exceed_eight_rounds_and_bind_cycles() {
    use reqwest::StatusCode;
    let (_container, postgres_url) = start_postgres().await;
    let mut config = create_config(&postgres_url);
    // Exact budgets: the unique 2x2 graph requires checking all four assignments.
    config.server.max_conditional_reference_candidates = 2;
    config.server.max_conditional_reference_assignments = 4;
    let (base, shutdown_tx, _handle) = start_server(&config).await;
    let client = reqwest::Client::new();
    let mut entries = Vec::new();
    for i in 0..12 {
        let mut resource = json!({"resourceType":"Observation","status":"final","code":{"text":"deep conditional graph"},
            "identifier":[{"system":"urn:deep-reference","value":i.to_string()}]});
        if i > 0 {
            let condition = if i == 1 {
                "Observation?identifier=urn:deep-reference|0".into()
            } else {
                format!(
                    "Observation?identifier=urn:deep-reference|{}&derived-from:Observation.identifier=urn:deep-reference|{}",
                    i - 1,
                    i - 2
                )
            };
            resource["derivedFrom"] = json!([{"reference":condition}]);
        }
        entries.push(json!({"request":{"method":"POST","url":"Observation"},"resource":resource}));
    }
    entries.reverse();
    let (status, body) = send_bundle(&client, &base, "transaction", json!(entries)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    for i in 0..11 {
        assert_eq!(
            body["entry"][i]["resource"]["derivedFrom"][0]["reference"],
            format!(
                "Observation/{}",
                body["entry"][i + 1]["resource"]["id"].as_str().unwrap()
            ),
            "{body}"
        );
    }
    let (status, cycle) = send_bundle(&client, &base, "transaction", json!([
        {"request":{"method":"POST","url":"Observation"},"resource":{"resourceType":"Observation","status":"final","code":{"text":"cycle"},
            "identifier":[{"system":"urn:reference-cycle","value":"left"}],
            "derivedFrom":[{"reference":"Observation?identifier=urn:reference-cycle|right&derived-from:Observation.identifier=urn:reference-cycle|left"}]}},
        {"request":{"method":"POST","url":"Observation"},"resource":{"resourceType":"Observation","status":"final","code":{"text":"cycle"},
            "identifier":[{"system":"urn:reference-cycle","value":"right"}],
            "derivedFrom":[{"reference":"Observation?identifier=urn:reference-cycle|left&derived-from:Observation.identifier=urn:reference-cycle|right"}]}}
    ])).await;
    assert_eq!(status, StatusCode::OK, "{cycle}");
    for (left, right) in [(0, 1), (1, 0)] {
        assert_eq!(
            cycle["entry"][left]["resource"]["derivedFrom"][0]["reference"],
            format!(
                "Observation/{}",
                cycle["entry"][right]["resource"]["id"].as_str().unwrap()
            )
        );
        let id = cycle["entry"][left]["resource"]["id"].as_str().unwrap();
        assert_eq!(
            read_resource(&client, &base, "Observation", id)
                .await
                .unwrap(),
            cycle["entry"][left]["resource"]
        );
    }
    // Both identity domains contain two candidates. Only the A/A binding
    // satisfies both complete chained predicates with singleton cardinality.
    let left_condition = "Observation?identifier=urn:unique-cycle|left&derived-from:Observation.code=urn:cycle-code|A";
    let right_condition = "Observation?identifier=urn:unique-cycle|right&derived-from:Observation.code=urn:cycle-code|A";
    let bad_left = "urn:uuid:550e8400-e29b-41d4-a716-446655440001";
    let bad_right = "urn:uuid:550e8400-e29b-41d4-a716-446655440002";
    let (status, unique) = send_bundle(&client, &base, "transaction", json!([
        {"request":{"method":"POST","url":"Observation"},"resource":{"resourceType":"Observation","status":"final",
            "code":{"coding":[{"system":"urn:cycle-code","code":"A"}]},"identifier":[{"system":"urn:unique-cycle","value":"left"}],
            "derivedFrom":[{"reference":right_condition}]}},
        {"fullUrl":bad_left,"request":{"method":"POST","url":"Observation"},"resource":{"resourceType":"Observation","status":"final",
            "code":{"coding":[{"system":"urn:cycle-code","code":"B"}]},"identifier":[{"system":"urn:unique-cycle","value":"left"}],
            "derivedFrom":[{"reference":bad_right}]}},
        {"request":{"method":"POST","url":"Observation"},"resource":{"resourceType":"Observation","status":"final",
            "code":{"coding":[{"system":"urn:cycle-code","code":"A"}]},"identifier":[{"system":"urn:unique-cycle","value":"right"}],
            "derivedFrom":[{"reference":left_condition}]}},
        {"fullUrl":bad_right,"request":{"method":"POST","url":"Observation"},"resource":{"resourceType":"Observation","status":"final",
            "code":{"coding":[{"system":"urn:cycle-code","code":"B"}]},"identifier":[{"system":"urn:unique-cycle","value":"right"}],
            "derivedFrom":[{"reference":bad_left}]}}
    ])).await;
    assert_eq!(status, StatusCode::OK, "{unique}");
    for (source, target) in [(0, 2), (2, 0), (1, 3), (3, 1)] {
        assert_eq!(
            unique["entry"][source]["resource"]["derivedFrom"][0]["reference"],
            format!(
                "Observation/{}",
                unique["entry"][target]["resource"]["id"].as_str().unwrap()
            ),
            "{unique}"
        );
        let resource = &unique["entry"][source]["resource"];
        assert_eq!(
            read_resource(
                &client,
                &base,
                "Observation",
                resource["id"].as_str().unwrap()
            )
            .await
            .unwrap(),
            *resource
        );
    }
    // Reverse chains permit several self-consistent graphs. Never select the
    // first solution just because each predicate is unique in that graph.
    let left_condition = "Observation?identifier=urn:ambiguous-cycle|left&_has:Observation:derived-from:identifier=urn:ambiguous-cycle|right";
    let right_condition = "Observation?identifier=urn:ambiguous-cycle|right&_has:Observation:derived-from:identifier=urn:ambiguous-cycle|left";
    let mut ambiguous = Vec::new();
    for side in ["left", "right"] {
        for _ in 0..2 {
            ambiguous.push(json!({"request":{"method":"POST","url":"Observation"},"resource":{
                "resourceType":"Observation","status":"final","code":{"text":"ambiguous cycle"},
                "identifier":[{"system":"urn:ambiguous-cycle","value":side}],
                "derivedFrom":[{"reference":if side == "left" {right_condition} else {left_condition}}]}}));
        }
    }
    let (status, rejected) = send_bundle(&client, &base, "transaction", json!(ambiguous)).await;
    assert_eq!(status, StatusCode::PRECONDITION_FAILED, "{rejected}");
    let count: Value = client
        .get(format!("{base}/Observation"))
        .query(&[
            (
                "identifier",
                "urn:ambiguous-cycle|left,urn:ambiguous-cycle|right",
            ),
            ("_summary", "count"),
        ])
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(count["total"], 0, "{count}");
    let condition = "Patient?identifier=urn%3Aconditional-typed%7CP";
    let (status, typed) = send_bundle(&client, &base, "transaction", json!([
        {"request":{"method":"POST","url":"Patient"},"resource":{"resourceType":"Patient",
            "identifier":[{"system":"urn:conditional-typed","value":"P"}]}},
        {"request":{"method":"POST","url":"Observation"},"resource":{"resourceType":"Observation","status":"final","code":{"text":"typed conditional links"},
            "subject":{"reference":condition},"identifier":[{"system":condition,"value":condition}],
            "extension":[{"url":"urn:typed:uri","valueUri":condition},{"url":"urn:typed:canonical","valueCanonical":condition}],
            "text":{"status":"generated","div":format!("<div xmlns='http://www.w3.org/1999/xhtml'><a href='{condition}'>link</a></div>")}}}
    ])).await;
    assert_eq!(status, StatusCode::OK, "{typed}");
    let resource = &typed["entry"][1]["resource"];
    assert_eq!(
        resource["subject"]["reference"],
        format!(
            "Patient/{}",
            typed["entry"][0]["resource"]["id"].as_str().unwrap()
        )
    );
    assert_eq!(resource["identifier"][0]["system"], condition);
    assert_eq!(resource["identifier"][0]["value"], condition);
    assert_eq!(resource["extension"][0]["valueUri"], condition);
    assert_eq!(resource["extension"][1]["valueCanonical"], condition);
    assert!(
        resource["text"]["div"]
            .as_str()
            .unwrap()
            .contains(condition)
    );
    assert_eq!(
        read_resource(
            &client,
            &base,
            "Observation",
            resource["id"].as_str().unwrap()
        )
        .await
        .unwrap(),
        *resource
    );
    let alias = "urn:uuid:550e8400-e29b-41d4-a716-446655440000";
    let (status, mapped) = send_bundle(&client, &base, "transaction", json!([
        {"fullUrl":alias,"request":{"method":"POST","url":"Patient"},"resource":{"resourceType":"Patient","id":"client-id","active":true}},
        {"request":{"method":"POST","url":"Patient"},"resource":{"resourceType":"Patient",
            "identifier":[{"system":alias,"value":alias}],"photo":[{"url":alias,"title":alias}],
            "meta":{"profile":[alias]},
            "text":{"status":"generated","div":format!("<div xmlns='http://www.w3.org/1999/xhtml'><a href='{alias}/_history/old#part'>Patient</a><img src='{alias}' /></div>")}}},
        {"request":{"method":"POST","url":"Observation"},"resource":{"resourceType":"Observation","status":"final","code":{"text":"versioned bundle link"},
            "subject":{"reference":format!("{alias}/_history/arbitrary-client-version")}}}
    ])).await;
    assert_eq!(status, StatusCode::OK, "{mapped}");
    let patient = &mapped["entry"][0]["resource"];
    let patient_id = patient["id"].as_str().unwrap();
    assert_ne!(patient_id, "client-id");
    let version = patient["meta"]["versionId"].as_str().unwrap();
    let relative = format!("Patient/{patient_id}");
    let companion = &mapped["entry"][1]["resource"];
    assert_eq!(companion["identifier"][0]["system"], relative);
    assert_eq!(companion["identifier"][0]["value"], alias);
    assert_eq!(companion["photo"][0]["url"], relative);
    assert_eq!(companion["photo"][0]["title"], alias);
    assert_eq!(companion["meta"]["profile"][0], alias);
    assert!(
        companion["text"]["div"]
            .as_str()
            .unwrap()
            .contains(&format!("{relative}/_history/{version}#part")),
        "{mapped}"
    );
    assert_eq!(
        mapped["entry"][2]["resource"]["subject"]["reference"],
        format!("{relative}/_history/{version}")
    );
    for entry in mapped["entry"].as_array().unwrap() {
        let resource = &entry["resource"];
        assert_eq!(
            read_resource(
                &client,
                &base,
                resource["resourceType"].as_str().unwrap(),
                resource["id"].as_str().unwrap()
            )
            .await
            .unwrap(),
            *resource
        );
        assert_eq!(
            entry["response"]["lastModified"],
            resource["meta"]["lastUpdated"]
        );
    }
    let _ = shutdown_tx.send(());
}

#[tokio::test]
async fn transaction_reference_graph_limits_reject_without_rows_or_history() {
    use reqwest::StatusCode;
    let (_container, postgres_url) = start_postgres().await;
    let client = reqwest::Client::new();
    let left = "Observation?identifier=urn:budget-cycle|left&derived-from:Observation.code=urn:cycle-code|A";
    let right = "Observation?identifier=urn:budget-cycle|right&derived-from:Observation.code=urn:cycle-code|A";
    let entries = json!([
        {"request":{"method":"PUT","url":"Observation/csp-budget-0-good-left"},"resource":{"resourceType":"Observation","id":"csp-budget-0-good-left","status":"final",
            "code":{"coding":[{"system":"urn:cycle-code","code":"A"}]},"identifier":[{"system":"urn:budget-cycle","value":"left"}],"derivedFrom":[{"reference":right}]}},
        {"request":{"method":"PUT","url":"Observation/csp-budget-1-bad-left"},"resource":{"resourceType":"Observation","id":"csp-budget-1-bad-left","status":"final",
            "code":{"coding":[{"system":"urn:cycle-code","code":"B"}]},"identifier":[{"system":"urn:budget-cycle","value":"left"}],"derivedFrom":[{"reference":"Observation/csp-budget-1-bad-right"}]}},
        {"request":{"method":"PUT","url":"Observation/csp-budget-0-good-right"},"resource":{"resourceType":"Observation","id":"csp-budget-0-good-right","status":"final",
            "code":{"coding":[{"system":"urn:cycle-code","code":"A"}]},"identifier":[{"system":"urn:budget-cycle","value":"right"}],"derivedFrom":[{"reference":left}]}},
        {"request":{"method":"PUT","url":"Observation/csp-budget-1-bad-right"},"resource":{"resourceType":"Observation","id":"csp-budget-1-bad-right","status":"final",
            "code":{"coding":[{"system":"urn:cycle-code","code":"B"}]},"identifier":[{"system":"urn:budget-cycle","value":"right"}],"derivedFrom":[{"reference":"Observation/csp-budget-1-bad-left"}]}}
    ]);
    for limit in ["domain", "total", "assignments"] {
        let mut config = create_config(&postgres_url);
        match limit {
            "domain" => config.server.max_conditional_reference_candidates = 1,
            "total" => config.server.max_conditional_reference_total_candidates = 3,
            _ => config.server.max_conditional_reference_assignments = 1,
        }
        let (base, shutdown, handle) = start_server(&config).await;
        let (status, outcome) = send_bundle(&client, &base, "transaction", entries.clone()).await;
        assert_eq!(
            status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{limit}: {outcome}"
        );
        assert_eq!(
            outcome["issue"][0]["code"], "too-costly",
            "{limit}: {outcome}"
        );
        // The first assignment is a valid A/A solution, but the assignments
        // budget cannot prove uniqueness. It must still reject atomically.
        for entry in entries.as_array().unwrap() {
            assert!(
                read_resource(
                    &client,
                    &base,
                    "Observation",
                    entry["resource"]["id"].as_str().unwrap()
                )
                .await
                .is_none()
            );
        }
        let history: Value = client
            .get(format!("{base}/Observation/_history"))
            .query(&[("_total", "accurate")])
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(history["total"], 0, "{limit}: {history}");
        let _ = shutdown.send(());
        tokio::time::timeout(std::time::Duration::from_secs(10), handle)
            .await
            .unwrap()
            .unwrap();
    }
}
