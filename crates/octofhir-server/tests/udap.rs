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
    BasicConstraints, Certificate, CertificateParams, CertificateRevocationListParams,
    CrlDistributionPoint, CustomExtension, DnType, IsCa, Issuer, KeyIdMethod, KeyPair,
    PKCS_ECDSA_P256_SHA256, PKCS_RSA_SHA256, RevocationReason, RevokedCertParams, SanType,
    SerialNumber, SigningKey,
};
use reqwest::{Client, StatusCode};
use serde_json::{Value, json};
use testcontainers::{ImageExt, runners::AsyncRunner};
use testcontainers_modules::postgres::Postgres;
use time::OffsetDateTime;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Respond, ResponseTemplate};
use x509_cert::der::asn1::{BitString, GeneralizedTime, OctetString};
use x509_cert::der::{DateTime, Decode, Encode};
use x509_cert::spki::{AlgorithmIdentifierOwned, ObjectIdentifier};
use x509_ocsp::{
    BasicOcspResponse, CertStatus, OcspGeneralizedTime, OcspRequest, OcspResponse,
    OcspResponseStatus, ResponderId, ResponseBytes, ResponseData, RevokedInfo, SingleResponse,
    Version,
};

const COMMUNITY: &str = "urn:example:community";
const APP: &str = "https://app.example.org/registered";

struct Authority {
    issuer: Issuer<'static, KeyPair>,
    pem: String,
    /// The key, to sign OCSP answers with.
    key_pem: String,
}

fn authority(name: &str) -> Authority {
    let key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).unwrap();
    let mut params = CertificateParams::default();
    params.distinguished_name.push(DnType::CommonName, name);
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    let certificate = params.self_signed(&key).unwrap();
    Authority {
        pem: certificate.pem(),
        key_pem: key.serialize_pem(),
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

/// A member whose certificate, with this serial number, names a revocation list at `crl`.
fn member_with_crl(authority: &Authority, uri: &str, serial: u64, crl: &str) -> Member {
    let key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).unwrap();
    let mut params = CertificateParams::default();
    params.distinguished_name.push(DnType::CommonName, "member");
    params.subject_alt_names = vec![SanType::URI(uri.try_into().unwrap())];
    params.serial_number = Some(SerialNumber::from(serial));
    params.crl_distribution_points = vec![CrlDistributionPoint {
        uris: vec![crl.to_owned()],
    }];
    let certificate = params.signed_by(&key, &authority.issuer).unwrap();
    Member {
        key: EncodingKey::from_ec_pem(key.serialize_pem().as_bytes()).unwrap(),
        x5c: vec![STANDARD.encode(certificate.der())],
    }
}

/// The community publishes a revocation list at the distribution point its certificates name:
/// a revoked certificate cannot register, the one next to it can.
async fn revoked_certificates(client: &Client, base: &str, ca: &Authority) {
    let lists = MockServer::start().await;
    let crl_url = format!("{}/community.crl", lists.uri());
    let today = OffsetDateTime::now_utc();
    let crl = CertificateRevocationListParams {
        this_update: today - time::Duration::hours(1),
        next_update: today + time::Duration::days(1),
        crl_number: SerialNumber::from(1u64),
        issuing_distribution_point: None,
        revoked_certs: vec![RevokedCertParams {
            serial_number: SerialNumber::from(7001u64),
            revocation_time: today - time::Duration::hours(1),
            reason_code: Some(RevocationReason::KeyCompromise),
            invalidity_date: None,
        }],
        key_identifier_method: KeyIdMethod::Sha256,
    }
    .signed_by(&ca.issuer)
    .unwrap();
    Mock::given(method("GET"))
        .and(path("/community.crl"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(crl.der().to_vec()))
        .mount(&lists)
        .await;

    let app = "https://revocation.example.org/app";
    let registration = |member: &Member| {
        let mut claims = statement(base, json!(["client_credentials"]));
        claims["iss"] = json!(app);
        claims["sub"] = json!(app);
        member.sign(&claims)
    };
    let revoked = member_with_crl(ca, app, 7001, &crl_url);
    let (status, body) = register(client, base, registration(&revoked)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], "unapproved_software_statement");
    assert!(
        body["error_description"]
            .as_str()
            .is_some_and(|text| text.to_lowercase().contains("revoked")),
        "{body}"
    );

    let current = member_with_crl(ca, app, 7002, &crl_url);
    let (status, body) = register(client, base, registration(&current)).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(
        lists.received_requests().await.unwrap().len(),
        1,
        "the list is fetched once and kept until its nextUpdate"
    );
}

/// A member whose certificate, with this serial number, names an OCSP responder at `ocsp`
/// (Authority Information Access).
fn member_with_ocsp(authority: &Authority, uri: &str, serial: u64, ocsp: &str) -> Member {
    let key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).unwrap();
    let mut params = CertificateParams::default();
    params.distinguished_name.push(DnType::CommonName, "member");
    params.subject_alt_names = vec![SanType::URI(uri.try_into().unwrap())];
    params.serial_number = Some(SerialNumber::from(serial));
    // AuthorityInfoAccessSyntax: one AccessDescription, id-ad-ocsp and a URI.
    let mut description = vec![
        0x06,
        0x08,
        0x2b,
        0x06,
        0x01,
        0x05,
        0x05,
        0x07,
        0x30,
        0x01,
        0x86,
        ocsp.len() as u8,
    ];
    description.extend_from_slice(ocsp.as_bytes());
    let mut access = vec![0x30, description.len() as u8];
    access.extend(description);
    let mut content = vec![0x30, access.len() as u8];
    content.extend(access);
    params.custom_extensions = vec![CustomExtension::from_oid_content(
        &[1, 3, 6, 1, 5, 5, 7, 1, 1],
        content,
    )];
    let certificate = params.signed_by(&key, &authority.issuer).unwrap();
    Member {
        key: EncodingKey::from_ec_pem(key.serialize_pem().as_bytes()).unwrap(),
        x5c: vec![STANDARD.encode(certificate.der())],
    }
}

/// An OCSP responder of the community: answers about the certificate it is asked about,
/// signed with the community's key.
struct Responder {
    key: KeyPair,
    revoked: Vec<u64>,
}

fn ocsp_time(seconds: i64) -> OcspGeneralizedTime {
    let time =
        DateTime::from_unix_duration(std::time::Duration::from_secs(seconds as u64)).unwrap();
    OcspGeneralizedTime(GeneralizedTime::from_date_time(time))
}

impl Respond for Responder {
    fn respond(&self, request: &wiremock::Request) -> ResponseTemplate {
        let asked = OcspRequest::from_der(&request.body).unwrap();
        let cert_id = asked.tbs_request.request_list[0].req_cert.clone();
        let serial = cert_id
            .serial_number
            .as_bytes()
            .iter()
            .fold(0u64, |value, byte| (value << 8) | u64::from(*byte));
        let cert_status = if self.revoked.contains(&serial) {
            CertStatus::Revoked(RevokedInfo {
                revocation_time: ocsp_time(now() - 3600),
                revocation_reason: None,
            })
        } else {
            CertStatus::Good(x509_cert::der::asn1::Null)
        };
        let tbs = ResponseData {
            version: Version::V1,
            responder_id: ResponderId::ByKey(OctetString::new(vec![0u8; 20]).unwrap()),
            produced_at: ocsp_time(now()),
            responses: vec![SingleResponse {
                cert_id,
                cert_status,
                this_update: ocsp_time(now() - 60),
                next_update: Some(ocsp_time(now() + 3600)),
                single_extensions: None,
            }],
            response_extensions: None,
        };
        let signature = self.key.sign(&tbs.to_der().unwrap()).unwrap();
        let basic = BasicOcspResponse {
            tbs_response_data: tbs,
            // ecdsa-with-SHA256
            signature_algorithm: AlgorithmIdentifierOwned {
                oid: ObjectIdentifier::new_unwrap("1.2.840.10045.4.3.2"),
                parameters: None,
            },
            signature: BitString::from_bytes(&signature).unwrap(),
            certs: None,
        };
        let response = OcspResponse {
            response_status: OcspResponseStatus::Successful,
            response_bytes: Some(ResponseBytes {
                response_type: ObjectIdentifier::new_unwrap("1.3.6.1.5.5.7.48.1.1"),
                response: OctetString::new(basic.to_der().unwrap()).unwrap(),
            }),
        };
        ResponseTemplate::new(200)
            .insert_header("content-type", "application/ocsp-response")
            .set_body_bytes(response.to_der().unwrap())
    }
}

/// The certificate names an OCSP responder: the one it says is revoked cannot register, the
/// good one can.
async fn ocsp_revocation(client: &Client, base: &str, ca: &Authority) {
    let responder = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/ocsp"))
        .respond_with(Responder {
            key: KeyPair::from_pem(&ca.key_pem).unwrap(),
            revoked: vec![8001],
        })
        .mount(&responder)
        .await;
    let ocsp = format!("{}/ocsp", responder.uri());
    let app = "https://ocsp.example.org/app";
    let registration = |member: &Member| {
        let mut claims = statement(base, json!(["client_credentials"]));
        claims["iss"] = json!(app);
        claims["sub"] = json!(app);
        member.sign(&claims)
    };
    let revoked = member_with_ocsp(ca, app, 8001, &ocsp);
    let (status, body) = register(client, base, registration(&revoked)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], "unapproved_software_statement");
    assert!(
        body["error_description"]
            .as_str()
            .is_some_and(|text| text.contains("OCSP")),
        "{body}"
    );
    let good = member_with_ocsp(ca, app, 8002, &ocsp);
    let (status, body) = register(client, base, registration(&good)).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
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

    // A changed signature does not verify. The first character of it holds six bits of the
    // signature, while the last one holds only two.
    let signed = app.sign(&statement(base, json!(["client_credentials"])));
    let (content, signature) = signed.rsplit_once('.').unwrap();
    let first = if signature.starts_with('A') { 'B' } else { 'A' };
    let forged = format!("{content}.{first}{}", &signature[1..]);
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
    revoked_certificates(&client, &base, &ca).await;
    ocsp_revocation(&client, &base, &ca).await;

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
