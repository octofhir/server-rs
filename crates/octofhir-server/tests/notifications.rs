//! Notifications end to end: provider and template as resources, `$send`, delivery to a
//! webhook, and the queue operations.
//!
//! Requirements: Docker running (testcontainers).

use std::sync::Arc;
use std::time::Duration;

use aws_lc_rs::hmac;
use octofhir_config::ConfigurationManager;
use octofhir_server::config::{AdminUserConfig, PackageSpec};
use octofhir_server::{AppConfig, PostgresStorageConfig, build_app};
use reqwest::{Client, StatusCode};
use serde_json::{Value, json};
use testcontainers::{ImageExt, runners::AsyncRunner};
use testcontainers_modules::postgres::Postgres;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn start_server(postgres_url: &str) -> String {
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
    config.notifications.poll_interval_ms = 200;
    config.bootstrap.admin_user = Some(AdminUserConfig {
        username: "admin".to_owned(),
        password: "admin-password".to_owned(),
        email: None,
        fhir_user: None,
    });
    let config_manager = Arc::new(ConfigurationManager::builder().build().await.unwrap());
    let app = build_app(&config, config_manager).await.unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    base
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

struct Api {
    client: Client,
    base: String,
    bearer: String,
}

impl Api {
    async fn post(&self, path: &str, body: Value) -> (StatusCode, Value) {
        self.post_at(&format!("fhir/{path}"), body).await
    }

    async fn post_at(&self, path: &str, body: Value) -> (StatusCode, Value) {
        let response = self
            .client
            .post(format!("{}/{path}", self.base))
            .bearer_auth(&self.bearer)
            .header("content-type", "application/fhir+json")
            .json(&body)
            .send()
            .await
            .unwrap();
        let status = response.status();
        (status, response.json().await.unwrap_or(Value::Null))
    }

    async fn stats(&self) -> Value {
        let (status, body) = self
            .post("Notification/$stats", json!({"resourceType": "Parameters"}))
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body["parameter"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| {
                (
                    p["name"].as_str().unwrap().to_owned(),
                    p["valueInteger"].clone(),
                )
            })
            .collect::<serde_json::Map<_, _>>()
            .into()
    }
}

fn send(provider: &str, template: &str, extra: Vec<Value>) -> Value {
    let mut parameter = vec![
        json!({"name": "provider", "valueString": provider}),
        json!({"name": "template", "valueString": template}),
        json!({"name": "data", "part": [
            {"name": "name", "valueString": "Ada"},
            {"name": "count", "valueInteger": 3},
        ]}),
    ];
    parameter.extend(extra);
    json!({"resourceType": "Parameters", "parameter": parameter})
}

#[tokio::test(flavor = "multi_thread")]
async fn a_notification_goes_out_through_a_webhook() {
    let postgres = Postgres::default()
        .with_tag("18-alpine")
        .start()
        .await
        .unwrap();
    let port = postgres.get_host_port_ipv4(5432).await.unwrap();
    let url = format!("postgres://postgres:postgres@127.0.0.1:{port}/postgres");
    let base = start_server(&url).await;
    let client = Client::new();
    let api = Api {
        bearer: admin_token(&client, &base).await,
        client,
        base,
    };
    let hooks = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/hook"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&hooks)
        .await;
    Mock::given(method("POST"))
        .and(path("/broken"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&hooks)
        .await;

    let (status, provider) = api
        .post(
            "NotificationProvider",
            json!({
                "resourceType": "NotificationProvider",
                "name": "Partner",
                "type": "webhook",
                "provider": "generic",
                "active": true,
                "config": {
                    "webhookUrl": format!("{}/hook", hooks.uri()),
                    "webhookSecret": "s3cret",
                    "webhookHeader": [{"name": "X-Partner", "value": "a"}],
                },
            }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{provider}");
    assert!(
        provider["config"].get("webhookSecret").is_none(),
        "{provider}"
    );
    let provider = provider["id"].as_str().unwrap().to_owned();
    let (status, template) = api
        .post(
            "NotificationTemplate",
            json!({
                "resourceType": "NotificationTemplate",
                "name": "greeting",
                "channel": "webhook",
                "subject": "For {{name}}",
                "body": "Hello {{name}}, {{count}} new results",
                "active": true,
            }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{template}");
    let template = template["id"].as_str().unwrap().to_owned();

    // Queued, then delivered, signed, with the provider's headers.
    let (status, queued) = api
        .post("Notification/$send", send(&provider, &template, vec![]))
        .await;
    assert_eq!(status, StatusCode::OK, "{queued}");
    let mut received = Vec::new();
    for _ in 0..50 {
        received = hooks.received_requests().await.unwrap();
        if !received.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(received.len(), 1);
    let request = &received[0];
    assert_eq!(request.headers["x-partner"], "a");
    let key = hmac::Key::new(hmac::HMAC_SHA256, b"s3cret");
    let signature = hex(hmac::sign(&key, &request.body).as_ref());
    assert_eq!(
        request.headers["x-signature-256"].to_str().unwrap(),
        format!("sha256={signature}")
    );
    let body: Value = serde_json::from_slice(&request.body).unwrap();
    assert_eq!(body["content"]["body"], "Hello Ada, 3 new results");
    assert_eq!(body["content"]["subject"], "For Ada");
    assert_eq!(
        body["notification_id"],
        queued["parameter"][0]["valueString"]
    );
    for _ in 0..50 {
        if api.stats().await["sent"] == 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(api.stats().await["sent"], 1);

    // A failed delivery waits for its retry, and can be cancelled.
    let (status, failing) = api
        .post(
            "Notification/$send",
            send(
                &provider,
                &template,
                vec![
                    json!({"name": "webhookUrl", "valueString": format!("{}/broken", hooks.uri())}),
                ],
            ),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{failing}");
    let failing = failing["parameter"][0]["valueString"]
        .as_str()
        .unwrap()
        .to_owned();
    for _ in 0..50 {
        let broken = hooks
            .received_requests()
            .await
            .unwrap()
            .iter()
            .filter(|r| r.url.path() == "/broken")
            .count();
        if broken > 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    tokio::time::sleep(Duration::from_millis(300)).await;
    let stats = api.stats().await;
    assert_eq!(stats["pending"], 1, "{stats:?}");
    let (status, _) = api
        .post(
            &format!("Notification/{failing}/$cancel"),
            json!({"resourceType": "Parameters"}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(api.stats().await["cancelled"], 1);

    // An App subscription: a matching change goes to its webhook and becomes a notification.
    Mock::given(method("POST"))
        .and(path("/app"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&hooks)
        .await;
    let (status, subscription) = api
        .post_at(
            "App",
            json!({
                "resourceType": "App",
                "name": "Portal",
                "status": "active",
                "secret": "portal-secret",
                "endpoint": {"url": format!("{}/backend", hooks.uri())},
                "subscriptions": [{
                    "id": "female-patients",
                    "trigger": {"resourceType": "Patient", "event": "create", "fhirpath": "gender = 'female'"},
                    "channel": {"type": "webhook", "endpoint": format!("{}/app", hooks.uri())},
                    "notification": {
                        "provider": format!("NotificationProvider/{provider}"),
                        "channel": ["webhook"],
                        "template": template,
                    },
                }],
            }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{subscription}");
    tokio::time::sleep(Duration::from_millis(300)).await;
    for gender in ["male", "female"] {
        let (status, body) = api
            .post(
                "Patient",
                json!({"resourceType": "Patient", "gender": gender, "name": [{"family": "Lovelace"}]}),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
    }
    let mut app = Vec::new();
    let mut hook = Vec::new();
    for _ in 0..50 {
        let received = hooks.received_requests().await.unwrap();
        app = received
            .iter()
            .filter(|r| r.url.path() == "/app")
            .cloned()
            .collect();
        hook = received
            .iter()
            .filter(|r| r.url.path() == "/hook")
            .cloned()
            .collect();
        if !app.is_empty() && hook.len() == 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(app.len(), 1, "only the female patient matches");
    let body: Value = serde_json::from_slice(&app[0].body).unwrap();
    assert_eq!(body["event"], "create");
    assert_eq!(body["resource"]["gender"], "female");
    assert_eq!(hook.len(), 2, "the subscription's notification went out");
    let body: Value = serde_json::from_slice(&hook[1].body).unwrap();
    assert_eq!(body["data"]["gender"], "female");

    // What cannot be delivered is not queued.
    for (extra, provider, template) in [
        (vec![], "missing", template.as_str()),
        (vec![], provider.as_str(), "missing"),
        (
            vec![json!({"name": "scheduledAt", "valueDateTime": "tomorrow"})],
            provider.as_str(),
            template.as_str(),
        ),
    ] {
        let (status, body) = api
            .post("Notification/$send", send(provider, template, extra))
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
