//! Sign-in through an external OpenID Connect provider, against a provider made for the test.
//!
//! Requirements: Docker running (testcontainers).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::extract::{Form, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use octofhir_config::ConfigurationManager;
use octofhir_server::config::{AdminUserConfig, PackageSpec};
use octofhir_server::{AppConfig, PostgresStorageConfig, build_app};
use rcgen::{KeyPair, PKCS_ECDSA_P256_SHA256, PublicKeyData};
use reqwest::header::{COOKIE, LOCATION, SET_COOKIE};
use reqwest::redirect::Policy;
use reqwest::{Client, Response, StatusCode};
use serde_json::{Value, json};
use testcontainers::{ImageExt, runners::AsyncRunner};
use testcontainers_modules::postgres::Postgres;
use time::OffsetDateTime;

const CLIENT_ID: &str = "server-at-provider";
const KID: &str = "test-key";

/// What the provider says about the next person who signs in.
#[derive(Clone, Default)]
struct Person {
    claims: Value,
    /// What only userinfo tells.
    userinfo: Value,
    /// Answer as an OAuth 2.0 provider: no ID token.
    plain: bool,
    nonce: String,
}

#[derive(Clone)]
struct Provider {
    issuer: String,
    key: Arc<EncodingKey>,
    jwk: Value,
    person: Arc<Mutex<Person>>,
}

async fn discovery(State(provider): State<Provider>) -> Json<Value> {
    let issuer = &provider.issuer;
    Json(json!({
        "issuer": issuer,
        "authorization_endpoint": format!("{issuer}/authorize"),
        "token_endpoint": format!("{issuer}/token"),
        "jwks_uri": format!("{issuer}/jwks"),
        "userinfo_endpoint": format!("{issuer}/userinfo"),
        "response_types_supported": ["code"],
        "subject_types_supported": ["public"],
        "id_token_signing_alg_values_supported": ["ES256"],
    }))
}

async fn keys(State(provider): State<Provider>) -> Json<Value> {
    Json(json!({"keys": [provider.jwk]}))
}

async fn userinfo(State(provider): State<Provider>) -> Json<Value> {
    let person = provider.person.lock().unwrap().clone();
    let mut info = person.userinfo;
    if info["sub"].is_null() {
        info["sub"] = person.claims["sub"].clone();
    }
    Json(info)
}

async fn token(
    State(provider): State<Provider>,
    Form(form): Form<HashMap<String, String>>,
) -> Json<Value> {
    assert_eq!(form["grant_type"], "authorization_code");
    assert!(form.contains_key("code_verifier"), "PKCE is used");
    let person = provider.person.lock().unwrap().clone();
    if person.plain {
        return Json(json!({"access_token": "provider-token", "token_type": "bearer"}));
    }
    let now = OffsetDateTime::now_utc().unix_timestamp();
    let mut claims = person.claims;
    claims["iss"] = json!(provider.issuer);
    claims["aud"] = json!(CLIENT_ID);
    claims["iat"] = json!(now);
    claims["exp"] = json!(now + 300);
    claims["nonce"] = json!(person.nonce);
    let mut header = Header::new(Algorithm::ES256);
    header.kid = Some(KID.to_owned());
    let id_token = encode(&header, &claims, &provider.key).unwrap();
    Json(json!({
        "access_token": "provider-token",
        "token_type": "Bearer",
        "expires_in": 300,
        "id_token": id_token,
    }))
}

async fn start_provider() -> Provider {
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let issuer = format!("http://{}", listener.local_addr().unwrap());
    let key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).unwrap();
    // The uncompressed point closes the public key: 0x04, x, y.
    let public = key.subject_public_key_info();
    let (x, y) = public[public.len() - 64..].split_at(32);
    let provider = Provider {
        issuer,
        key: Arc::new(EncodingKey::from_ec_pem(key.serialize_pem().as_bytes()).unwrap()),
        jwk: json!({
            "kty": "EC", "crv": "P-256", "use": "sig", "alg": "ES256", "kid": KID,
            "x": URL_SAFE_NO_PAD.encode(x),
            "y": URL_SAFE_NO_PAD.encode(y),
        }),
        person: Arc::default(),
    };
    let app = Router::new()
        .route("/.well-known/openid-configuration", get(discovery))
        .route("/jwks", get(keys))
        .route("/token", post(token))
        .route("/userinfo", get(userinfo))
        .with_state(provider.clone());
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    provider
}

async fn start_server(postgres_url: &str, auto_provision: bool) -> String {
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let mut config = AppConfig::default();
    config.storage.postgres = Some(PostgresStorageConfig {
        url: Some(postgres_url.to_owned()),
        pool_size: 10,
        connect_timeout_ms: 10_000,
        idle_timeout_ms: Some(60_000),
        ..Default::default()
    });
    config.packages.load = vec![PackageSpec::Table {
        id: Some("hl7.fhir.r4.core".into()),
        version: Some("4.0.1".into()),
        path: None,
        url: Some("https://hl7.org/fhir/R4/hl7.fhir.r4.core.tgz".into()),
    }];
    config.server.base_url = Some(base.clone());
    config.auth.issuer = base.clone();
    config.auth.federation.allow_http = true;
    config.auth.federation.auto_provision_users = auto_provision;
    config.bootstrap.admin_user = Some(AdminUserConfig {
        username: "admin".to_owned(),
        password: "admin-password".to_owned(),
        email: Some("admin@example.org".to_owned()),
        fhir_user: None,
    });
    let config_manager = Arc::new(ConfigurationManager::builder().build().await.unwrap());
    let app = build_app(&config, config_manager).await.unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    base
}

/// A browser, as far as cookies go.
#[derive(Default)]
struct Browser {
    cookies: HashMap<String, String>,
}

impl Browser {
    async fn get(&mut self, client: &Client, url: &str) -> Response {
        let cookies: Vec<String> = self
            .cookies
            .iter()
            .map(|(name, value)| format!("{name}={value}"))
            .collect();
        let response = client
            .get(url)
            .header(COOKIE, cookies.join("; "))
            .send()
            .await
            .unwrap();
        for cookie in response.headers().get_all(SET_COOKIE) {
            let pair = cookie.to_str().unwrap().split(';').next().unwrap();
            let (name, value) = pair.split_once('=').unwrap();
            self.cookies.insert(name.to_owned(), value.to_owned());
        }
        response
    }

    /// Up to the provider: where it would send the browser back to, with `state`.
    async fn at_provider(&mut self, client: &Client, url: &str, provider: &Provider) -> String {
        let response = self.get(client, url).await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER, "{url}");
        let location = response.headers()[LOCATION].to_str().unwrap();
        let location = url::Url::parse(location).unwrap();
        assert!(location.as_str().starts_with(&provider.issuer));
        let query: HashMap<_, _> = location.query_pairs().into_owned().collect();
        assert_eq!(query["code_challenge_method"], "S256");
        provider.person.lock().unwrap().nonce = query["nonce"].clone();
        query["state"].clone()
    }
}

fn says(provider: &Provider, claims: Value) {
    let mut person = provider.person.lock().unwrap();
    person.claims = claims;
    person.userinfo = json!({});
}

async fn admin_token(client: &Client, base: &str) -> String {
    let token: Value = client
        .post(format!("{base}/auth/token"))
        .form(&[
            ("grant_type", "password"),
            ("client_id", "octofhir-ui"),
            ("username", "admin"),
            ("password", "admin-password"),
            ("scope", "openid user/*.cruds system/*.cruds"),
        ])
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    token["access_token"].as_str().unwrap().to_owned()
}

/// The roles of the user behind `subject`.
async fn roles(client: &Client, base: &str, bearer: &str, subject: &str) -> Value {
    let user: Value = client
        .get(format!("{base}/User/{subject}"))
        .bearer_auth(bearer)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    user["roles"].clone()
}

async fn sign_in(
    client: &Client,
    base: &str,
    id: &str,
    provider: &Provider,
    return_to: &str,
) -> (Browser, Response) {
    let mut browser = Browser::default();
    let url = format!("{base}/auth/idp/{id}/login?return_to={return_to}");
    let state = browser.at_provider(client, &url, provider).await;
    let callback = format!("{base}/auth/idp/callback?code=abc&state={state}");
    let response = browser.get(client, &callback).await;
    (browser, response)
}

async fn subject(client: &Client, base: &str, browser: &mut Browser) -> String {
    let response = browser.get(client, &format!("{base}/auth/userinfo")).await;
    assert_eq!(response.status(), StatusCode::OK);
    let info: Value = response.json().await.unwrap();
    info["sub"].as_str().unwrap().to_owned()
}

async fn providers(client: &Client, base: &str) -> Value {
    client
        .get(format!("{base}/auth/idp/providers"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn sign_in_through_an_external_provider() {
    let postgres = Postgres::default()
        .with_tag("18-alpine")
        .start()
        .await
        .unwrap();
    let port = postgres.get_host_port_ipv4(5432).await.unwrap();
    let url = format!("postgres://postgres:postgres@127.0.0.1:{port}/postgres");
    let client = Client::builder().redirect(Policy::none()).build().unwrap();
    let provider = start_provider().await;
    let strict = start_server(&url, false).await;
    assert_eq!(providers(&client, &strict).await, json!([]));

    // A provider made the way the console does it takes effect without a restart.
    let bearer = admin_token(&client, &strict).await;
    let created = client
        .post(format!("{strict}/IdentityProvider"))
        .bearer_auth(&bearer)
        .json(&json!({
            "resourceType": "IdentityProvider",
            "name": "Example",
            "type": "oidc",
            "issuer": provider.issuer,
            "clientId": CLIENT_ID,
            "clientSecret": "secret-at-the-provider",
            "active": true,
            "scopes": ["openid", "email"],
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(created.status(), StatusCode::CREATED);
    let created: Value = created.json().await.unwrap();
    assert!(created.get("clientSecret").is_none(), "{created}");
    let id = created["id"].as_str().unwrap().to_owned();
    let mut listed = json!([]);
    for _ in 0..50 {
        listed = providers(&client, &strict).await;
        if listed != json!([]) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert_eq!(listed, json!([{"id": id, "name": "Example"}]));

    // Nobody signs in as someone the server does not know.
    says(
        &provider,
        json!({"sub": "stranger", "email": "s@example.org"}),
    );
    let (_, response) = sign_in(&client, &strict, &id, &provider, "/ui/").await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    // The way back stays on this server.
    let foreign = format!("{strict}/auth/idp/{id}/login?return_to=https://evil.example/");
    let response = client.get(foreign).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    // A code planted in a browser that did not start the sign-in goes nowhere.
    let mut victim = Browser::default();
    let mut attacker = Browser::default();
    let login = format!("{strict}/auth/idp/{id}/login");
    let state = attacker.at_provider(&client, &login, &provider).await;
    let callback = format!("{strict}/auth/idp/callback?code=abc&state={state}");
    let response = victim.get(&client, &callback).await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    // An answer is taken once, and a token for another sign-in is not one.
    let state = attacker.at_provider(&client, &login, &provider).await;
    provider.person.lock().unwrap().nonce = "from-another-sign-in".to_owned();
    let callback = format!("{strict}/auth/idp/callback?code=abc&state={state}");
    let response = attacker.get(&client, &callback).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let response = attacker.get(&client, &callback).await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    // A server started later reads the providers, and this one creates users.
    let open = start_server(&url, true).await;
    assert_eq!(
        providers(&client, &open).await,
        json!([{"id": id, "name": "Example"}])
    );
    says(
        &provider,
        json!({"sub": "new-person", "email": "new@example.org", "email_verified": true}),
    );
    let (mut browser, response) = sign_in(&client, &open, &id, &provider, "/ui/").await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(response.headers()[LOCATION], "/ui/");
    let first = subject(&client, &open, &mut browser).await;

    // The same person is the same user.
    let (mut browser, _) = sign_in(&client, &open, &id, &provider, "/ui/").await;
    assert_eq!(subject(&client, &open, &mut browser).await, first);

    // Claiming a name somebody has gives a user of one's own.
    says(
        &provider,
        json!({"sub": "impostor", "preferred_username": "admin"}),
    );
    let (mut browser, response) = sign_in(&client, &open, &id, &provider, "/ui/").await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    let impostor = subject(&client, &open, &mut browser).await;
    let admin = client
        .get(format!("{strict}/auth/userinfo"))
        .bearer_auth(&bearer)
        .send()
        .await
        .unwrap()
        .json::<Value>()
        .await
        .unwrap();
    assert_ne!(impostor, admin["sub"].as_str().unwrap());
    assert_ne!(impostor, first);

    // With rules the provider decides roles. A broken rule takes the provider off, not the rules.
    let open_bearer = admin_token(&client, &open).await;
    let replace = |rules: Value| {
        client
            .put(format!("{open}/IdentityProvider/{id}"))
            .bearer_auth(&open_bearer)
            .json(&json!({
                "resourceType": "IdentityProvider",
                "id": id,
                "name": "Example",
                "displayName": "Example SSO",
                "type": "oidc",
                "issuer": provider.issuer,
                "clientId": CLIENT_ID,
                "active": true,
                "scopes": ["openid", "email"],
                "userMapping": {"claimsToRoles": rules},
            }))
            .send()
    };
    let response = replace(json!([{"claim": "groups", "value": "^(broken$", "roles": ["admin"]}]))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let mut listed = providers(&client, &open).await;
    for _ in 0..50 {
        if listed == json!([]) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        listed = providers(&client, &open).await;
    }
    assert_eq!(listed, json!([]));

    let response = replace(json!([
        {"claim": "groups", "value": "fhir-*", "roles": ["$1"]},
        {"claim": "groups", "value": "ops", "roles": ["operator", "auditor"]},
    ]))
    .await
    .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let mut listed = providers(&client, &open).await;
    for _ in 0..50 {
        if listed != json!([]) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        listed = providers(&client, &open).await;
    }
    assert_eq!(listed, json!([{"id": id, "name": "Example SSO"}]));

    // Groups can come from the ID token.
    says(
        &provider,
        json!({"sub": "new-person", "email": "new@example.org", "groups": ["fhir-editor", "ops"]}),
    );
    let (_, response) = sign_in(&client, &open, &id, &provider, "/ui/").await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        roles(&client, &open, &open_bearer, &first).await,
        json!(["editor", "operator", "auditor"])
    );

    // Or from userinfo only, and leaving a group takes its role away.
    says(&provider, json!({"sub": "new-person"}));
    provider.person.lock().unwrap().userinfo = json!({"groups": ["fhir-viewer"]});
    let (_, response) = sign_in(&client, &open, &id, &provider, "/ui/").await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        roles(&client, &open, &open_bearer, &first).await,
        json!(["viewer"])
    );

    // Userinfo about somebody else is not believed.
    provider.person.lock().unwrap().userinfo = json!({"sub": "other", "groups": ["fhir-admin"]});
    let (_, response) = sign_in(&client, &open, &id, &provider, "/ui/").await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    // No group, no way in, for known and new people alike.
    provider.person.lock().unwrap().userinfo = json!({"groups": ["unrelated"]});
    let (_, response) = sign_in(&client, &open, &id, &provider, "/ui/").await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    says(&provider, json!({"sub": "another-stranger"}));
    let (_, response) = sign_in(&client, &open, &id, &provider, "/ui/").await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    // A provider without OpenID Connect has its endpoints named, and no ID token to give.
    let issuer = &provider.issuer;
    let created = client
        .post(format!("{open}/IdentityProvider"))
        .bearer_auth(&open_bearer)
        .json(&json!({
            "resourceType": "IdentityProvider",
            "name": "Plain",
            "type": "oauth2",
            "issuer": issuer,
            "clientId": CLIENT_ID,
            "clientSecret": "secret-at-the-provider",
            "active": true,
            "scopes": ["read:user"],
            "authorizationEndpoint": format!("{issuer}/authorize"),
            "tokenEndpoint": format!("{issuer}/token"),
            "userinfoEndpoint": format!("{issuer}/userinfo"),
            "userMapping": {"subjectClaim": "id", "usernameClaim": "login"},
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(created.status(), StatusCode::CREATED);
    let created: Value = created.json().await.unwrap();
    let plain = created["id"].as_str().unwrap().to_owned();
    for _ in 0..50 {
        if providers(&client, &open).await.as_array().unwrap().len() == 2 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    {
        let mut person = provider.person.lock().unwrap();
        person.plain = true;
        person.claims = json!({});
        person.userinfo = json!({"id": 583231, "login": "octo", "email": "o@example.org"});
    }
    let mut browser = Browser::default();
    let url = format!("{open}/auth/idp/{plain}/login");
    let response = browser.get(&client, &url).await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    let location = url::Url::parse(response.headers()[LOCATION].to_str().unwrap()).unwrap();
    let query: HashMap<_, _> = location.query_pairs().into_owned().collect();
    assert!(!query.contains_key("nonce"));
    assert_eq!(query["code_challenge_method"], "S256");
    let callback = format!("{open}/auth/idp/callback?code=abc&state={}", query["state"]);
    let response = browser.get(&client, &callback).await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    let subject = subject(&client, &open, &mut browser).await;
    let user: Value = client
        .get(format!("{open}/User/{subject}"))
        .bearer_auth(&open_bearer)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(user["username"], "octo");
    // Nothing vouches for this address, so it links to nobody.
    assert_eq!(user["email"], "o@example.org");
}
