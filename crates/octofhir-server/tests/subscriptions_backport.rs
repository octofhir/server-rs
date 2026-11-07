//! The Subscriptions Backport on an R4 server, end to end against a listener.
//!
//! Requirements: Docker running (testcontainers).

use std::sync::Arc;
use std::time::Duration;

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
}

fn parameter<'a>(parameters: &'a Value, name: &str) -> Option<&'a Value> {
    parameters["parameter"]
        .as_array()?
        .iter()
        .find(|parameter| parameter["name"] == name)
}

async fn received(hooks: &MockServer, count: usize) -> Vec<Value> {
    for _ in 0..100 {
        let requests = hooks.received_requests().await.unwrap();
        if requests.len() >= count {
            return requests
                .iter()
                .map(|request| serde_json::from_slice(&request.body).unwrap())
                .collect();
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("fewer than {count} notifications arrived");
}

/// The R4 server speaks the Subscriptions Backport: status as Parameters in history bundles.
#[tokio::test(flavor = "multi_thread")]
async fn subscriptions_in_the_r4_backport_form() {
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
        .and(path("/listener"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&hooks)
        .await;
    let topic = "https://octofhir.io/topics/encounter-start";
    let (status, body) = api
        .post(
            "SubscriptionTopic",
            json!({"resourceType": "SubscriptionTopic", "url": topic, "status": "active",
                "resourceTrigger": [{"resource": "Encounter", "supportedInteraction": ["create"]}]}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let backport = "http://hl7.org/fhir/uv/subscriptions-backport/StructureDefinition";
    let subscription = json!({
        "resourceType": "Subscription",
        "status": "requested",
        "reason": "test",
        "criteria": topic,
        "channel": {
            "extension": [{"url": format!("{backport}/backport-heartbeat-period"), "valueUnsignedInt": 1}],
            "type": "rest-hook",
            "endpoint": format!("{}/listener", hooks.uri()),
            "payload": "application/fhir+json",
            "_payload": {"extension": [{"url": format!("{backport}/backport-payload-content"), "valueCode": "id-only"}]},
        },
    });

    // An extension of another version is refused rather than ignored.
    let mut crossed = subscription.clone();
    crossed["_criteria"] = json!({"extension": [{
        "url": "http://hl7.org/fhir/5.0/StructureDefinition/extension-Subscription.filterBy",
        "valueString": "Encounter.patient=Patient/1",
    }]});
    let (status, _) = api.post("Subscription", crossed).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    let (status, created) = api.post("Subscription", subscription).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let id = created["id"].as_str().unwrap().to_owned();

    // The handshake: a history bundle, Parameters first, still `requested`.
    let handshake = &received(&hooks, 1).await[0];
    assert_eq!(handshake["type"], "history");
    let status = &handshake["entry"][0]["resource"];
    assert_eq!(status["resourceType"], "Parameters");
    assert_eq!(parameter(status, "type").unwrap()["valueCode"], "handshake");
    assert_eq!(
        parameter(status, "status").unwrap()["valueCode"],
        "requested"
    );

    // An event: the focus is referenced, and with id-only not included.
    let (status, encounter) = api
        .post(
            "Encounter",
            json!({"resourceType": "Encounter", "status": "in-progress",
                "class": {"system": "http://terminology.hl7.org/CodeSystem/v3-ActCode", "code": "IMP"}}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{encounter}");
    let mut event = Value::Null;
    for _ in 0..100 {
        let all = received(&hooks, 1).await;
        if let Some(found) = all.into_iter().find(|bundle| {
            parameter(&bundle["entry"][0]["resource"], "type").unwrap()["valueCode"]
                == "event-notification"
        }) {
            event = found;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let status = &event["entry"][0]["resource"];
    let notification = parameter(status, "notification-event").unwrap();
    let focus = notification["part"]
        .as_array()
        .unwrap()
        .iter()
        .find(|part| part["name"] == "focus")
        .unwrap();
    let reference = format!("Encounter/{}", encounter["id"].as_str().unwrap());
    assert_eq!(focus["valueReference"]["reference"], reference);
    assert!(event["entry"][1].get("resource").is_none(), "{event}");

    // Heartbeats when nothing else is sent.
    let mut heartbeat = false;
    for _ in 0..150 {
        let all = received(&hooks, 1).await;
        heartbeat = all.iter().any(|bundle| {
            parameter(&bundle["entry"][0]["resource"], "type").unwrap()["valueCode"] == "heartbeat"
        });
        if heartbeat {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(heartbeat, "no heartbeat");

    // $status: a Bundle of Parameters.
    let (status, bundle) = api
        .post(
            &format!("Subscription/{id}/$status"),
            json!({"resourceType": "Parameters"}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{bundle}");
    assert_eq!(bundle["resourceType"], "Bundle");
    assert_eq!(bundle["entry"][0]["resource"]["resourceType"], "Parameters");

    // The topic is listed on the Subscription resource of the CapabilityStatement.
    let metadata: Value = api
        .client
        .get(format!("{}/fhir/metadata", api.base))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let subscription = metadata["rest"][0]["resource"]
        .as_array()
        .unwrap()
        .iter()
        .find(|resource| resource["type"] == "Subscription")
        .unwrap();
    assert!(
        subscription["extension"].to_string().contains(topic),
        "{subscription}"
    );
}
