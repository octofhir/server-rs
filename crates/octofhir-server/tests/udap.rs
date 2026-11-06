//! UDAP Security against a running server: discovery, registration, and client credentials.
//!
//! Requirements: Docker running (testcontainers).

use std::path::Path;
use std::sync::Arc;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use octofhir_auth::config::UdapCommunityConfig;
use octofhir_config::ConfigurationManager;
use octofhir_server::{AppConfig, PostgresStorageConfig, build_app};
use rcgen::{
    BasicConstraints, Certificate, CertificateParams, DnType, IsCa, Issuer, KeyPair,
    PKCS_ECDSA_P256_SHA256, PKCS_RSA_SHA256, SanType,
};
use reqwest::{Client, StatusCode};
use serde_json::{Value, json};
use testcontainers::{ImageExt, runners::AsyncRunner};
use testcontainers_modules::postgres::Postgres;
use time::OffsetDateTime;

const COMMUNITY: &str = "urn:example:community";
const APP: &str = "https://app.example.org/registered";

struct Authority {
    issuer: Issuer<'static, KeyPair>,
    pem: String,
}

fn authority(name: &str) -> Authority {
    let key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).unwrap();
    let mut params = CertificateParams::default();
    params.distinguished_name.push(DnType::CommonName, name);
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    let certificate = params.self_signed(&key).unwrap();
    Authority {
        pem: certificate.pem(),
        issuer: Issuer::new(params, key),
    }
}

fn issue(authority: &Authority, key: &KeyPair, uri: &str) -> Certificate {
    let mut params = CertificateParams::default();
    params.distinguished_name.push(DnType::CommonName, "member");
    params.subject_alt_names = vec![SanType::URI(uri.try_into().unwrap())];
    params.signed_by(key, &authority.issuer).unwrap()
}

/// An application: its key, and its certificate as `x5c`.
struct Member {
    key: EncodingKey,
    x5c: Vec<String>,
}

fn member(authority: &Authority, uri: &str) -> Member {
    let key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).unwrap();
    let certificate = issue(authority, &key, uri);
    Member {
        key: EncodingKey::from_ec_pem(key.serialize_pem().as_bytes()).unwrap(),
        x5c: vec![STANDARD.encode(certificate.der())],
    }
}

impl Member {
    fn sign(&self, claims: &Value) -> String {
        let mut header = Header::new(Algorithm::ES256);
        header.x5c = Some(self.x5c.clone());
        encode(&header, claims, &self.key).unwrap()
    }
}

fn now() -> i64 {
    OffsetDateTime::now_utc().unix_timestamp()
}

fn jti() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn statement(base: &str, grant_types: Value) -> Value {
    let now = now();
    json!({
        "iss": APP,
        "sub": APP,
        "aud": format!("{base}/auth/udap/register"),
        "exp": now + 240,
        "iat": now,
        "jti": jti(),
        "client_name": "Registered app",
        "grant_types": grant_types,
        "token_endpoint_auth_method": "private_key_jwt",
        "scope": "system/Patient.rs system/*.cruds",
        "contacts": ["mailto:ops@example.org"],
    })
}

fn authentication(base: &str, client_id: &str, b2b: Option<Value>) -> Value {
    let now = now();
    let mut claims = json!({
        "iss": client_id,
        "sub": client_id,
        "aud": format!("{base}/auth/token"),
        "exp": now + 240,
        "iat": now,
        "jti": jti(),
    });
    if let Some(b2b) = b2b {
        claims["extensions"] = json!({"hl7-b2b": b2b});
    }
    claims
}

fn b2b() -> Value {
    json!({
        "version": "1",
        "organization_id": "https://org.example.org",
        "purpose_of_use": ["urn:oid:2.16.840.1.113883.5.8#TREAT"],
    })
}

async fn register(client: &Client, base: &str, token: String) -> (StatusCode, Value) {
    let response = client
        .post(format!("{base}/auth/udap/register"))
        .json(&json!({"software_statement": token, "udap": "1"}))
        .send()
        .await
        .unwrap();
    let status = response.status();
    let text = response.text().await.unwrap();
    (status, serde_json::from_str(&text).unwrap_or(Value::Null))
}

async fn token(client: &Client, base: &str, assertion: String, udap: bool) -> (StatusCode, Value) {
    let mut form = vec![
        ("grant_type", "client_credentials".to_owned()),
        ("scope", "system/Patient.rs".to_owned()),
        (
            "client_assertion_type",
            "urn:ietf:params:oauth:client-assertion-type:jwt-bearer".to_owned(),
        ),
        ("client_assertion", assertion),
    ];
    if udap {
        form.push(("udap", "1".to_owned()));
    }
    let response = client
        .post(format!("{base}/auth/token"))
        .form(&form)
        .send()
        .await
        .unwrap();
    let status = response.status();
    (status, response.json().await.unwrap())
}

fn write(path: &Path, content: &str) {
    std::fs::write(path, content).unwrap();
}

async fn start(postgres_url: &str, ca: &Authority, directory: &Path) -> String {
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    let base = format!("http://{addr}");

    let key = KeyPair::generate_for(&PKCS_RSA_SHA256).unwrap();
    let certificate = issue(ca, &key, &format!("{base}/fhir"));
    let (certificate_path, key_path, anchor_path) = (
        directory.join("server.pem"),
        directory.join("server.key"),
        directory.join("anchor.pem"),
    );
    write(&certificate_path, &certificate.pem());
    write(&key_path, &key.serialize_pem());
    write(&anchor_path, &ca.pem);

    let mut config = AppConfig::default();
    config.storage.postgres = Some(PostgresStorageConfig {
        url: Some(postgres_url.to_owned()),
        pool_size: 10,
        connect_timeout_ms: 10_000,
        idle_timeout_ms: Some(60_000),
        ..Default::default()
    });
    config.packages.load = vec![octofhir_server::config::PackageSpec::Table {
        id: Some("hl7.fhir.r4.core".into()),
        version: Some("4.0.1".into()),
        path: None,
        url: Some("https://hl7.org/fhir/R4/hl7.fhir.r4.core.tgz".into()),
    }];
    config.server.base_url = Some(base.clone());
    config.auth.issuer = base.clone();
    config.auth.udap.enabled = true;
    config.auth.udap.certificate = Some(certificate_path);
    config.auth.udap.private_key = Some(key_path);
    config.auth.udap.communities = vec![UdapCommunityConfig {
        uri: COMMUNITY.to_owned(),
        anchors: vec![anchor_path],
        revocation_lists: Vec::new(),
    }];
    let config_manager = Arc::new(ConfigurationManager::builder().build().await.unwrap());
    let app = build_app(&config, config_manager).await.unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    base
}

async fn discovery(client: &Client, base: &str, ca: &Authority) {
    let response = client
        .get(format!("{base}/fhir/.well-known/udap"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let document: Value = response.json().await.unwrap();
    assert_eq!(document["udap_versions_supported"], json!(["1"]));
    assert_eq!(
        document["token_endpoint_auth_methods_supported"],
        json!(["private_key_jwt"])
    );
    assert_eq!(
        document["registration_endpoint"],
        format!("{base}/auth/udap/register")
    );

    let signed = document["signed_metadata"].as_str().unwrap();
    let header = jsonwebtoken::decode_header(signed).unwrap();
    assert_eq!(header.alg, Algorithm::RS256);
    assert!(!header.x5c.unwrap().is_empty());
    let claims: Value = serde_json::from_slice(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(signed.split('.').nth(1).unwrap())
            .unwrap(),
    )
    .unwrap();
    assert_eq!(claims["iss"], format!("{base}/fhir"));
    assert_eq!(claims["sub"], claims["iss"]);
    assert_eq!(claims["token_endpoint"], format!("{base}/auth/token"));

    let known = client
        .get(format!(
            "{base}/fhir/.well-known/udap?community={COMMUNITY}"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(known.status(), StatusCode::OK);
    let unknown = client
        .get(format!("{base}/fhir/.well-known/udap?community=urn:other"))
        .send()
        .await
        .unwrap();
    assert_eq!(unknown.status(), StatusCode::NO_CONTENT);
    assert!(!ca.pem.is_empty());
}

async fn refused_registrations(client: &Client, base: &str, app: &Member, ca: &Authority) {
    let grants = json!(["client_credentials"]);
    let outsider = member(&authority("Outsider"), APP);
    let (status, body) = register(
        client,
        base,
        outsider.sign(&statement(base, grants.clone())),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "unapproved_software_statement");

    let (status, body) = register(
        client,
        base,
        member(ca, "https://other.example.org/").sign(&statement(base, grants.clone())),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "unapproved_software_statement");

    let mut long_lived = statement(base, grants.clone());
    long_lived["exp"] = json!(now() + 3600);
    let (_, body) = register(client, base, app.sign(&long_lived)).await;
    assert_eq!(body["error"], "invalid_software_statement");

    let mut wrong_audience = statement(base, grants.clone());
    wrong_audience["aud"] = json!("https://elsewhere.example.org/register");
    let (_, body) = register(client, base, app.sign(&wrong_audience)).await;
    assert_eq!(body["error"], "invalid_software_statement");

    let both = json!(["client_credentials", "authorization_code"]);
    let (_, body) = register(client, base, app.sign(&statement(base, both))).await;
    assert_eq!(body["error"], "invalid_client_metadata");

    let mut no_contact = statement(base, grants);
    no_contact["contacts"] = json!(["not-an-email"]);
    let (_, body) = register(client, base, app.sign(&no_contact)).await;
    assert_eq!(body["error"], "invalid_client_metadata");

    // A certificate signed by someone else cannot pass for the application's own.
    let mut forged = app.sign(&statement(base, json!(["client_credentials"])));
    forged.pop();
    forged.push(if forged.ends_with('A') { 'B' } else { 'A' });
    let (status, _) = register(client, base, forged).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn udap_discovery_registration_and_token() {
    let postgres = Postgres::default()
        .with_tag("18-alpine")
        .start()
        .await
        .unwrap();
    let port = postgres.get_host_port_ipv4(5432).await.unwrap();
    let directory = tempfile::tempdir().unwrap();
    let ca = authority("Community");
    let base = start(
        &format!("postgres://postgres:postgres@127.0.0.1:{port}/postgres"),
        &ca,
        directory.path(),
    )
    .await;
    let client = Client::new();
    let app = member(&ca, APP);

    discovery(&client, &base, &ca).await;
    refused_registrations(&client, &base, &app, &ca).await;

    // Registered, with the scopes this server offers.
    let claims = statement(&base, json!(["client_credentials"]));
    let (status, body) = register(&client, &base, app.sign(&claims)).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let client_id = body["client_id"].as_str().unwrap().to_owned();
    assert_eq!(body["scope"], "system/Patient.rs");
    assert!(body.get("iss").is_none());

    // The same statement again is a replay.
    let (_, body) = register(&client, &base, app.sign(&claims)).await;
    assert_eq!(body["error"], "invalid_software_statement");

    // A new one from the same application changes the registration.
    let (status, body) = register(
        &client,
        &base,
        app.sign(&statement(&base, json!(["client_credentials"]))),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["client_id"], client_id.as_str());

    // Client credentials with the certificate.
    let authenticate = |b2b| app.sign(&authentication(&base, &client_id, b2b));
    let (status, body) = token(&client, &base, authenticate(Some(b2b())), true).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["access_token"].is_string());
    assert!(body["expires_in"].as_i64().unwrap() <= 3600);
    assert_eq!(body["scope"], "system/Patient.rs");

    let (status, body) = token(&client, &base, authenticate(None), true).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], "invalid_request");

    let (status, body) = token(&client, &base, authenticate(Some(b2b())), false).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], "invalid_request");

    let replayed = authenticate(Some(b2b()));
    assert_eq!(
        token(&client, &base, replayed.clone(), true).await.0,
        StatusCode::OK
    );
    let (_, body) = token(&client, &base, replayed, true).await;
    assert_eq!(body["error"], "invalid_client");

    // Another member of the community cannot sign in as this application.
    let other = member(&ca, "https://other.example.org/");
    let (_, body) = token(
        &client,
        &base,
        other.sign(&authentication(&base, &client_id, Some(b2b()))),
        true,
    )
    .await;
    assert_eq!(body["error"], "invalid_client");

    // Cancelled: an empty grant_types, then the client is gone.
    let (status, body) = register(&client, &base, app.sign(&statement(&base, json!([])))).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["grant_types"], json!([]));
    let (_, body) = token(&client, &base, authenticate(Some(b2b())), true).await;
    assert_eq!(body["error"], "invalid_client");
}
