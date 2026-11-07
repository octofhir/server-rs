//! UDAP Tiered OAuth against a running server and an identity provider stand-in: the client
//! app names the provider, the server trusts it by its signed metadata, registers with it,
//! signs the user in there and comes back to the authorization request; an untrusted provider
//! is refused with `invalid_idp`.
//!
//! Requirements: Docker running (testcontainers).

use std::path::Path;
use std::sync::Arc;

use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use octofhir_auth::config::UdapCommunityConfig;
use octofhir_config::ConfigurationManager;
use octofhir_server::{AppConfig, PostgresStorageConfig, build_app};
use rcgen::{
    BasicConstraints, Certificate, CertificateParams, DnType, IsCa, Issuer, KeyPair,
    PKCS_ECDSA_P256_SHA256, PKCS_RSA_SHA256, SanType,
};
use reqwest::header::{COOKIE, LOCATION, SET_COOKIE};
use reqwest::{Client, StatusCode, redirect};
use serde_json::{Value, json};
use testcontainers::{ImageExt, runners::AsyncRunner};
use testcontainers_modules::postgres::Postgres;
use time::OffsetDateTime;
use url::Url;
use wiremock::matchers::{body_string_contains, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const COMMUNITY: &str = "urn:example:community";
const APP: &str = "https://app.example.org/registered";
const APP_CALLBACK: &str = "https://app.example.org/callback";
const IDP_CLIENT_ID: &str = "octofhir-at-idp";

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

/// A member of a community: its key, its certificate as `x5c`, and its public key as a JWK.
struct Member {
    key: EncodingKey,
    x5c: Vec<String>,
    jwk: Value,
}

fn member(authority: &Authority, uri: &str) -> Member {
    let key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).unwrap();
    let certificate = issue(authority, &key, uri);
    // An uncompressed P-256 point: 0x04, x, y.
    let point = key.public_key_raw();
    Member {
        key: EncodingKey::from_ec_pem(key.serialize_pem().as_bytes()).unwrap(),
        x5c: vec![STANDARD.encode(certificate.der())],
        jwk: json!({
            "kty": "EC", "crv": "P-256", "kid": "idp-1", "alg": "ES256", "use": "sig",
            "x": URL_SAFE_NO_PAD.encode(&point[1..33]),
            "y": URL_SAFE_NO_PAD.encode(&point[33..65]),
        }),
    }
}

impl Member {
    fn sign(&self, claims: &Value) -> String {
        let mut header = Header::new(Algorithm::ES256);
        header.x5c = Some(self.x5c.clone());
        encode(&header, claims, &self.key).unwrap()
    }

    /// An ID token: signed with the key the JWKS publishes, by its key id.
    fn id_token(&self, claims: &Value) -> String {
        let mut header = Header::new(Algorithm::ES256);
        header.kid = Some("idp-1".into());
        encode(&header, claims, &self.key).unwrap()
    }
}

fn now() -> i64 {
    OffsetDateTime::now_utc().unix_timestamp()
}

fn jti() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// The claims of a JWT, unverified (the test reads what the server sent).
fn claims(token: &str) -> Value {
    let payload = token.split('.').nth(1).unwrap();
    serde_json::from_slice(&URL_SAFE_NO_PAD.decode(payload).unwrap()).unwrap()
}

fn query(url: &str, name: &str) -> Option<String> {
    let url = Url::parse(url)
        .or_else(|_| Url::parse(&format!("http://local{url}")))
        .unwrap();
    url.query_pairs()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.into_owned())
}

/// The `name=value` pairs of a response's cookies, for the next request.
fn cookies(response: &reqwest::Response) -> Vec<String> {
    response
        .headers()
        .get_all(SET_COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .filter_map(|value| value.split(';').next())
        .map(str::to_owned)
        .collect()
}

/// An identity provider of a community, with its UDAP metadata signed by `signer`.
async fn identity_provider(signer: &Authority) -> (MockServer, Member) {
    let idp = MockServer::start().await;
    let base = idp.uri();
    let member = member(signer, &base);
    let signed_metadata = member.sign(&json!({
        "iss": base, "sub": base, "iat": now(), "exp": now() + 3600, "jti": jti(),
        "authorization_endpoint": format!("{base}/authorize"),
        "token_endpoint": format!("{base}/token"),
        "registration_endpoint": format!("{base}/register"),
    }));
    Mock::given(method("GET"))
        .and(path("/.well-known/udap"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "udap_versions_supported": ["1"],
            "scopes_supported": ["openid", "udap", "email", "profile"],
            "authorization_endpoint": format!("{base}/authorize"),
            "token_endpoint": format!("{base}/token"),
            "registration_endpoint": format!("{base}/register"),
            "signed_metadata": signed_metadata,
        })))
        .mount(&idp)
        .await;
    Mock::given(method("POST"))
        .and(path("/register"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "client_id": IDP_CLIENT_ID,
            "client_name": "data holder",
        })))
        .mount(&idp)
        .await;
    Mock::given(method("GET"))
        .and(path("/.well-known/openid-configuration"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "issuer": base,
            "authorization_endpoint": format!("{base}/authorize"),
            "token_endpoint": format!("{base}/token"),
            "jwks_uri": format!("{base}/jwks"),
            "response_types_supported": ["code"],
            "subject_types_supported": ["public"],
            "id_token_signing_alg_values_supported": ["ES256"],
        })))
        .mount(&idp)
        .await;
    Mock::given(method("GET"))
        .and(path("/jwks"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"keys": [member.jwk]})))
        .mount(&idp)
        .await;
    (idp, member)
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
    config.auth.udap.tiered_oauth.enabled = true;
    config.auth.udap.tiered_oauth.contacts = vec!["mailto:ops@example.org".into()];
    config.auth.udap.tiered_oauth.logo_uri = Some("https://example.org/logo.png".into());
    // The identity provider stand-in speaks plain HTTP.
    config.auth.federation.allow_http = true;
    config.auth.federation.auto_provision_users = true;
    let config_manager = Arc::new(ConfigurationManager::builder().build().await.unwrap());
    let app = build_app(&config, config_manager).await.unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    base
}

/// Registers the client app, for the authorization code grant with the `udap` scope.
async fn register_app(client: &Client, base: &str, app: &Member) -> String {
    let statement = app.sign(&json!({
        "iss": APP, "sub": APP, "aud": format!("{base}/auth/udap/register"),
        "iat": now(), "exp": now() + 240, "jti": jti(),
        "client_name": "Tiered app",
        "grant_types": ["authorization_code"],
        "response_types": ["code"],
        "redirect_uris": [APP_CALLBACK],
        "logo_uri": "https://app.example.org/logo.png",
        "token_endpoint_auth_method": "private_key_jwt",
        "scope": "openid udap user/*.rs",
        "contacts": ["mailto:ops@example.org"],
    }));
    let response = client
        .post(format!("{base}/auth/udap/register"))
        .json(&json!({"software_statement": statement, "udap": "1"}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let body: Value = response.json().await.unwrap();
    body["client_id"].as_str().unwrap().to_owned()
}

fn authorize_url(base: &str, client_id: &str, idp: &str) -> String {
    let mut url = Url::parse(&format!("{base}/auth/authorize")).unwrap();
    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", client_id)
        .append_pair("redirect_uri", APP_CALLBACK)
        .append_pair("scope", "openid udap user/Patient.rs")
        .append_pair("state", "app-state-0123456789abcdefghij")
        .append_pair("aud", &format!("{base}/fhir"))
        .append_pair("idp", idp);
    url.to_string()
}

#[tokio::test(flavor = "multi_thread")]
async fn tiered_oauth_signs_the_user_in_at_the_named_identity_provider() {
    let postgres = Postgres::default()
        .with_tag("18-alpine")
        .start()
        .await
        .unwrap();
    let port = postgres.get_host_port_ipv4(5432).await.unwrap();
    let url = format!("postgres://postgres:postgres@127.0.0.1:{port}/postgres");
    let directory = std::env::temp_dir().join(format!("udap-tiered-{}", jti()));
    std::fs::create_dir_all(&directory).unwrap();
    let ca = authority("community");
    let (idp, idp_member) = identity_provider(&ca).await;
    let base = start(&url, &ca, &directory).await;
    let client = Client::builder()
        .redirect(redirect::Policy::none())
        .build()
        .unwrap();

    // The data holder advertises Tiered OAuth.
    let metadata: Value = client
        .get(format!("{base}/fhir/.well-known/udap"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(
        metadata["scopes_supported"]
            .as_array()
            .unwrap()
            .contains(&json!("udap")),
        "{metadata}"
    );

    let app = member(&ca, APP);
    let client_id = register_app(&client, &base, &app).await;

    // The app names the identity provider: the server registers there and sends the browser
    // to sign in.
    let response = client
        .get(authorize_url(&base, &client_id, &idp.uri()))
        .send()
        .await
        .unwrap();
    assert!(response.status().is_redirection(), "{}", response.status());
    let login = response.headers()[LOCATION].to_str().unwrap().to_owned();
    assert!(login.starts_with("/auth/idp/"), "{login}");
    let return_to = query(&login, "return_to").unwrap();
    assert!(return_to.starts_with("/auth/authorize?"), "{return_to}");
    assert!(query(&return_to, "idp").is_none(), "{return_to}");

    let registrations = idp.received_requests().await.unwrap();
    let registration = registrations
        .iter()
        .find(|request| request.url.path() == "/register")
        .expect("the server registered with the identity provider");
    let body: Value = serde_json::from_slice(&registration.body).unwrap();
    assert_eq!(body["udap"], "1");
    let statement = claims(body["software_statement"].as_str().unwrap());
    assert_eq!(statement["iss"], format!("{base}/fhir"));
    assert_eq!(statement["sub"], format!("{base}/fhir"));
    assert_eq!(statement["aud"], format!("{}/register", idp.uri()));
    assert_eq!(statement["scope"], "openid udap");
    assert_eq!(statement["grant_types"], json!(["authorization_code"]));
    assert_eq!(statement["token_endpoint_auth_method"], "private_key_jwt");
    assert_eq!(
        statement["redirect_uris"],
        json!([format!("{base}/auth/idp/callback")])
    );

    // The sign-in request at the identity provider: its own state, a nonce, openid and udap.
    let response = client.get(format!("{base}{login}")).send().await.unwrap();
    assert!(response.status().is_redirection(), "{}", response.status());
    let state_cookie = cookies(&response);
    let at_idp = response.headers()[LOCATION].to_str().unwrap().to_owned();
    assert!(
        at_idp.starts_with(&format!("{}/authorize", idp.uri())),
        "{at_idp}"
    );
    assert_eq!(query(&at_idp, "client_id").unwrap(), IDP_CLIENT_ID);
    assert_eq!(query(&at_idp, "scope").unwrap(), "openid udap");
    let state = query(&at_idp, "state").unwrap();
    assert_ne!(state, "app-state-0123456789abcdefghij");
    let nonce = query(&at_idp, "nonce").unwrap();

    // The identity provider answers: a code, then at its token endpoint (UDAP client
    // authentication, udap=1) an ID token for the user.
    Mock::given(method("POST"))
        .and(path("/token"))
        .and(body_string_contains("udap=1"))
        .and(body_string_contains("client_assertion="))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "idp-access-token",
            "token_type": "Bearer",
            "expires_in": 300,
            "id_token": idp_member.id_token(&json!({
                "iss": idp.uri(),
                "sub": "alice-123",
                "aud": IDP_CLIENT_ID,
                "iat": now(),
                "exp": now() + 300,
                "nonce": nonce,
                "email": "alice@example.org",
                "name": "Alice Example",
            })),
        })))
        .mount(&idp)
        .await;
    let response = client
        .get(format!(
            "{base}/auth/idp/callback?code=idp-code&state={state}"
        ))
        .header(COOKIE, state_cookie.join("; "))
        .send()
        .await
        .unwrap();
    assert!(response.status().is_redirection(), "{}", response.status());
    let session = cookies(&response);
    let back = response.headers()[LOCATION].to_str().unwrap().to_owned();
    assert_eq!(back, return_to);

    let token_request = idp
        .received_requests()
        .await
        .unwrap()
        .into_iter()
        .find(|request| request.url.path() == "/token")
        .unwrap();
    let form: Vec<(String, String)> = url::form_urlencoded::parse(&token_request.body)
        .into_owned()
        .collect();
    let assertion = &form
        .iter()
        .find(|(key, _)| key == "client_assertion")
        .unwrap()
        .1;
    let assertion_claims = claims(assertion);
    assert_eq!(assertion_claims["iss"], IDP_CLIENT_ID);
    assert_eq!(assertion_claims["sub"], IDP_CLIENT_ID);
    assert_eq!(assertion_claims["aud"], format!("{}/token", idp.uri()));
    let header: Value = serde_json::from_slice(
        &URL_SAFE_NO_PAD
            .decode(assertion.split('.').next().unwrap())
            .unwrap(),
    )
    .unwrap();
    assert!(
        header["x5c"]
            .as_array()
            .is_some_and(|chain| !chain.is_empty())
    );

    // Back at the authorization request, signed in: the app's grant proceeds (consent, or the
    // code when consent was given before).
    let response = client
        .get(format!("{base}{back}"))
        .header(COOKIE, session.join("; "))
        .send()
        .await
        .unwrap();
    let status = response.status();
    if status.is_redirection() {
        let location = response.headers()[LOCATION].to_str().unwrap();
        assert!(location.starts_with(APP_CALLBACK), "{location}");
        assert!(query(location, "code").is_some(), "{location}");
    } else {
        assert_eq!(status, StatusCode::OK);
        let page = response.text().await.unwrap();
        assert!(
            page.contains("Tiered app"),
            "the consent page names the app"
        );
    }

    // The registration is kept as an IdentityProvider of type udap, and reused.
    let response = client
        .get(authorize_url(&base, &client_id, &idp.uri()))
        .send()
        .await
        .unwrap();
    assert!(response.status().is_redirection());
    let registered = idp
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|request| request.url.path() == "/register")
        .count();
    assert_eq!(registered, 1, "the second request reuses the registration");

    // An identity provider of another community is not trusted: invalid_idp to the app.
    let stranger = authority("another community");
    let (untrusted, _) = identity_provider(&stranger).await;
    let response = client
        .get(authorize_url(&base, &client_id, &untrusted.uri()))
        .send()
        .await
        .unwrap();
    assert!(response.status().is_redirection());
    let location = response.headers()[LOCATION].to_str().unwrap();
    assert!(location.starts_with(APP_CALLBACK), "{location}");
    assert_eq!(query(location, "error").unwrap(), "invalid_idp");
    assert_eq!(
        query(location, "state").unwrap(),
        "app-state-0123456789abcdefghij"
    );

    let _ = std::fs::remove_dir_all(&directory);
}
