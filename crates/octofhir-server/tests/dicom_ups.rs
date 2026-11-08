//! UPS Requestor/Watcher across real PostgreSQL and an independent protocol peer.
//! Covers stable identities, ambiguous creation, source reconnect, clinical output
//! references, cancellation semantics, immutable inputs and deletion-lock ordering.
use axum::{
    Json, Router,
    extract::{
        Path, Query, State,
        ws::{Message, WebSocketUpgrade},
    },
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use futures_util::StreamExt;
use octofhir_config::ConfigurationManager;
use octofhir_server::{AppConfig, PostgresStorageConfig, build_app};
use serde_json::{Value, json};
use sqlx_core::{query::query, row::Row};
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};
use testcontainers::{ImageExt, runners::AsyncRunner};
use testcontainers_modules::postgres::Postgres;
use tokio::sync::{Mutex, Notify, broadcast};
const CT: &str = "1.2.840.10008.5.1.4.1.1.2";
const SR: &str = "1.2.840.10008.5.1.4.1.1.88.22";
const SEG: &str = "1.2.840.10008.5.1.4.1.1.66.4";
#[derive(Clone)]
struct Pacs {
    work: Arc<Mutex<BTreeMap<String, Value>>>,
    events: broadcast::Sender<Value>,
    metadata: Arc<Mutex<Vec<Value>>>,
    connected: Arc<AtomicUsize>,
    creates: Arc<AtomicUsize>,
    cancels: Arc<AtomicUsize>,
    released: Arc<AtomicUsize>,
    subscriptions: Arc<AtomicUsize>,
    lose_create_response: Arc<AtomicBool>,
    block_input: Arc<AtomicBool>,
    entered: Arc<Notify>,
    release: Arc<Notify>,
    pool: sqlx_postgres::PgPool,
}
fn attribute(vr: &str, value: &str) -> Value {
    json!({"vr":vr,"Value":[value]})
}
fn dataset(study: &str, series: &str, instance: &str, class: &str, modality: &str) -> Value {
    json!({"0020000D":attribute("UI",study),"0020000E":attribute("UI",series),"00080018":attribute("UI",instance),"00080016":attribute("UI",class),"00080060":attribute("CS",modality),"00100020":attribute("LO","MRN-1"),"00100021":attribute("LO","HOSPITAL")})
}
fn dicom(value: Value) -> Response {
    (
        [(axum::http::header::CONTENT_TYPE, "application/dicom+json")],
        Json(value),
    )
        .into_response()
}
async fn metadata(
    State(state): State<Pacs>,
    Path(study): Path<String>,
    headers: HeaderMap,
) -> Response {
    assert!(
        !headers.contains_key("authorization"),
        "FHIR credential reached PACS"
    );
    if state.block_input.load(Ordering::SeqCst) {
        state.entered.notify_one();
        state.release.notified().await;
    }
    dicom(json!(
        state
            .metadata
            .lock()
            .await
            .iter()
            .filter(|row| row["0020000D"]["Value"][0] == study)
            .cloned()
            .collect::<Vec<_>>()
    ))
}
async fn create(
    State(state): State<Pacs>,
    Query(params): Query<BTreeMap<String, String>>,
    Json(rows): Json<Value>,
) -> Response {
    let uid = params.get("workitem").unwrap();
    let mut rows = rows.as_array().unwrap().clone();
    assert_eq!(rows.len(), 1);
    let mut dataset = rows.pop().unwrap();
    assert!(dataset.get("00080018").is_none());
    assert!(dataset["00081195"].get("Value").is_none());
    assert_eq!(dataset["00741000"]["Value"][0], "SCHEDULED");
    assert!(
        state.subscriptions.load(Ordering::SeqCst) > 0,
        "UPS created before its filtered deletion lock"
    );
    dataset["00080018"] = attribute("UI", uid);
    dataset["00080016"] = attribute("UI", "1.2.840.10008.5.1.4.34.6.1");
    let mut work = state.work.lock().await;
    if work.contains_key(uid) {
        return StatusCode::CONFLICT.into_response();
    }
    work.insert(uid.clone(), dataset);
    drop(work);
    state.creates.fetch_add(1, Ordering::SeqCst);
    let _ = state
        .events
        .send(json!({"00741000":attribute("CS","SCHEDULED")}));
    if state.lose_create_response.swap(false, Ordering::SeqCst) {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    StatusCode::CREATED.into_response()
}
async fn retrieve(State(state): State<Pacs>, Path(uid): Path<String>) -> Response {
    match state.work.lock().await.get(&uid) {
        Some(dataset) => dicom(json!([dataset])),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}
async fn cancel(
    State(state): State<Pacs>,
    Path(uid): Path<String>,
    Query(params): Query<BTreeMap<String, String>>,
) -> Response {
    assert_eq!(
        params.get("requester").map(String::as_str),
        Some("OCTOFHIR")
    );
    assert!(state.work.lock().await.contains_key(&uid));
    state.cancels.fetch_add(1, Ordering::SeqCst);
    // Acknowledge the request without changing the performer's state.
    let _ = state
        .events
        .send(json!({"00741238":attribute("LT","Cancellation requested")}));
    StatusCode::ACCEPTED.into_response()
}
async fn subscribe(
    State(state): State<Pacs>,
    Path((uid, ae)): Path<(String, String)>,
    Query(params): Query<BTreeMap<String, String>>,
) -> Response {
    assert_eq!(uid, "1.2.840.10008.5.1.4.34.5.1");
    assert_eq!(ae, "OCTOFHIR");
    assert!(
        state.connected.load(Ordering::SeqCst) > 0,
        "Subscribe ran before Open Notification Connection"
    );
    assert_eq!(params.get("deletionlock").map(String::as_str), Some("true"));
    let label = &params["ProcedureStepLabel"];
    assert!(label.starts_with("OFHIR ") && label.ends_with('*'));
    state.subscriptions.fetch_add(1, Ordering::SeqCst);
    StatusCode::CREATED.into_response()
}
async fn unsubscribe(
    State(state): State<Pacs>,
    Path((uid, ae)): Path<(String, String)>,
) -> Response {
    assert_eq!(ae, "OCTOFHIR");
    let row=query("SELECT t.resource->>'status' AS status,s.remote_state,s.applied_revision,s.desired_revision,s.pending_job FROM dicom_ups_tasks s JOIN task t ON t.id=s.task_id WHERE s.workitem_uid=$1").bind(uid).fetch_one(&state.pool).await.unwrap();
    let remote: String = row.get("remote_state");
    let task: String = row.get("status");
    assert_eq!(
        task,
        if remote == "COMPLETED" {
            "completed"
        } else {
            "cancelled"
        }
    );
    assert!(row.get::<i64, _>("applied_revision") >= row.get::<i64, _>("desired_revision"));
    assert!(row.get::<Option<uuid::Uuid>, _>("pending_job").is_none());
    state.released.fetch_add(1, Ordering::SeqCst);
    StatusCode::NO_CONTENT.into_response()
}
struct Connection(Arc<AtomicUsize>);
impl Drop for Connection {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}
async fn notifications(
    State(state): State<Pacs>,
    Path(ae): Path<String>,
    upgrade: WebSocketUpgrade,
) -> Response {
    assert_eq!(ae, "OCTOFHIR");
    upgrade.on_upgrade(move|mut socket|async move{
  state.connected.fetch_add(1,Ordering::SeqCst);let _guard=Connection(state.connected.clone());let mut events=state.events.subscribe();
  loop{tokio::select!{event=events.recv()=>{let Ok(event)=event else{break;};if socket.send(Message::Text(event.to_string().into())).await.is_err(){break;}},message=socket.next()=>{match message{Some(Ok(Message::Ping(value)))=>{if socket.send(Message::Pong(value)).await.is_err(){break;}},Some(Ok(Message::Close(_)))|None|Some(Err(_))=>break,_=>{}}}}}
 }).into_response()
}
async fn serve(config: &AppConfig) -> (String, tokio::task::JoinHandle<()>) {
    let app = build_app(
        config,
        Arc::new(ConfigurationManager::builder().build().await.unwrap()),
    )
    .await
    .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    (
        base,
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() }),
    )
}
async fn create_task(client: &reqwest::Client, base: &str, patient: &str, study: &str) -> String {
    let response=client.post(format!("{base}/fhir/Task")).json(&json!({"resourceType":"Task","status":"draft","intent":"order","for":{"reference":format!("Patient/{patient}")},"focus":{"reference":study},"code":{"coding":[{"system":"urn:test:procedure","code":"TEST","display":"Synthetic processing"}]},"input":[{"type":{"coding":[{"system":"urn:test:procedure","code":"THRESHOLD","display":"Threshold"}]},"valueQuantity":{"value":1.0,"system":"http://unitsofmeasure.org","code":"mm","unit":"millimetre"}}]})).send().await.unwrap();
    let status = response.status();
    let body: Value = response.json().await.unwrap();
    assert_eq!(status, 201, "{body}");
    body["id"].as_str().unwrap().into()
}
async fn command(client: &reqwest::Client, base: &str, id: &str, action: &str) -> Value {
    let response=client.post(format!("{base}/fhir/Task/{id}/$dicom-ups")).json(&json!({"resourceType":"Parameters","parameter":[{"name":"connection","valueString":"pacs"},{"name":"action","valueString":action}]})).send().await.unwrap();
    let status = response.status();
    let body: Value = response.json().await.unwrap();
    assert_eq!(status, 200, "{body}");
    body
}
fn parameter<'a>(value: &'a Value, name: &str) -> &'a Value {
    value["parameter"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == name)
        .unwrap()
}
async fn wait_task(client: &reqwest::Client, base: &str, id: &str, status: &str) -> Value {
    let mut last = json!(null);
    for _ in 0..200 {
        last = client
            .get(format!("{base}/fhir/Task/{id}"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        if last["status"] == status {
            return last;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    panic!("Task did not reach {status}: {last}")
}
async fn set_state(state: &Pacs, uid: &str, status: &str, outputs: bool) {
    let mut work = state.work.lock().await;
    let item = work.get_mut(uid).unwrap();
    item["00741000"] = attribute("CS", status);
    if outputs {
        let references = vec![("1.2.3", "1.2.3.2", "1.2.3.2.1", SR), ("1.2.4", "1.2.4.3", "1.2.4.3.1", SEG)].into_iter().map(|(study,series,instance,class)|json!({"0040E020":attribute("CS","DICOM"),"0020000D":attribute("UI",study),"0020000E":attribute("UI",series),"00081199":{"vr":"SQ","Value":[{"00081150":attribute("UI",class),"00081155":attribute("UI",instance)}]}})).collect::<Vec<_>>();
        item["0020000D"] = attribute("UI", "1.2.4");
        item["00741216"] = json!({"vr":"SQ","Value":[{"00404033":{"vr":"SQ","Value":references}}]});
        let mut metadata = state.metadata.lock().await;
        metadata.push(dataset("1.2.3", "1.2.3.2", "1.2.3.2.1", SR, "SR"));
        metadata.push(dataset("1.2.4", "1.2.4.3", "1.2.4.3.1", SEG, "SEG"));
    }
    drop(work);
    let _ = state
        .events
        .send(json!({"00741000":attribute("CS",status)}));
}
#[test]
fn ups_delivery_recovery_publication_and_cancellation_preserve_boundaries() {
    std::thread::Builder::new()
        .name("dicom-ups".into())
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
async fn authenticated(http: &reqwest::Client, base: &str) -> reqwest::Client {
    let token: Value = http
        .post(format!("{base}/auth/token"))
        .basic_auth("ups-service", Some("isolated-ups-secret"))
        .form(&[
            ("grant_type", "client_credentials"),
            ("scope", "system/*.cruds"),
        ])
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let mut headers = HeaderMap::new();
    headers.insert(
        "authorization",
        format!("Bearer {}", token["access_token"].as_str().unwrap())
            .parse()
            .unwrap(),
    );
    reqwest::Client::builder()
        .default_headers(headers)
        .build()
        .unwrap()
}
async fn scenario() {
    let pg = Postgres::default()
        .with_tag("18-alpine")
        .start()
        .await
        .unwrap();
    let database = format!(
        "postgres://postgres:postgres@127.0.0.1:{}/postgres",
        pg.get_host_port_ipv4(5432).await.unwrap()
    );
    let pool = sqlx_postgres::PgPoolOptions::new()
        .max_connections(3)
        .connect(&database)
        .await
        .unwrap();
    let (events, _) = broadcast::channel(32);
    let state = Pacs {
        work: Arc::default(),
        events,
        metadata: Arc::new(Mutex::new(vec![dataset(
            "1.2.3",
            "1.2.3.1",
            "1.2.3.1.1",
            CT,
            "CT",
        )])),
        connected: Arc::default(),
        creates: Arc::default(),
        cancels: Arc::default(),
        released: Arc::default(),
        subscriptions: Arc::default(),
        lose_create_response: Arc::new(AtomicBool::new(true)),
        block_input: Arc::default(),
        entered: Arc::default(),
        release: Arc::default(),
        pool: pool.clone(),
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let pacs = format!("http://{}", listener.local_addr().unwrap());
    let router = Router::new()
        .route("/wado/studies/{uid}/metadata", get(metadata))
        .route("/ups/subscribers/{ae}", get(notifications))
        .route("/ups/workitems", post(create))
        .route("/ups/workitems/{uid}", get(retrieve))
        .route("/ups/workitems/{uid}/cancelrequest", post(cancel))
        .route(
            "/ups/workitems/{uid}/subscribers/{ae}",
            post(subscribe).delete(unsubscribe),
        )
        .with_state(state.clone());
    let pacs_task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let mut config = AppConfig::default();
    config.fhir.version = "R4".into();
    config.storage.postgres = Some(PostgresStorageConfig {
        url: Some(database),
        pool_size: 10,
        ..Default::default()
    });
    config.packages.load = vec![octofhir_server::config::PackageSpec::Table {
        id: Some("hl7.fhir.r4.core".into()),
        version: Some("4.0.1".into()),
        path: None,
        url: Some("https://hl7.org/fhir/R4/hl7.fhir.r4.core.tgz".into()),
    }];
    config.auth.policy.anonymous_access = true;
    config.server.outbound_allowlist = vec!["127.0.0.1/32".into()];
    config.bootstrap.backend_client = Some(octofhir_server::config::BackendClientConfig {
        client_id: "ups-service".into(),
        client_secret: Some("isolated-ups-secret".into()),
        scopes: vec!["system/*.cruds".into()],
        name: None,
        description: None,
    });
    config.dicom=serde_json::from_value(json!({"connections":{"pacs":{"wado_url":format!("{pacs}/wado"),"allow_http":true,"patient_identifier_systems":{"HOSPITAL":"urn:test:mrn"},"ups":{"url":format!("{pacs}/ups"),"client_id":"ups-service","scopes":["system/*.cruds"],"ae_title":"OCTOFHIR","poll_seconds":1,"retry_seconds":1,"coding_schemes":{"urn:test:procedure":"99TEST","http://unitsofmeasure.org":"UCUM"},"result_mappings":{SR:{"kind":"diagnostic_report","system":"urn:test:result","code":"IMAGING","display":"Imaging output","report_status":"unknown"},SEG:{"kind":"document_reference","system":"urn:test:result","code":"SEG","display":"Segmentation"}}}}}})).unwrap();
    let (base, server) = serve(&config).await;
    let http = reqwest::Client::new();
    let client = authenticated(&http, &base).await;
    let patient:Value=client.post(format!("{base}/fhir/Patient")).json(&json!({"resourceType":"Patient","identifier":[{"system":"urn:test:mrn","value":"MRN-1"}],"meta":{"security":[{"system":"http://terminology.hl7.org/CodeSystem/v3-Confidentiality","code":"R"}]}})).send().await.unwrap().json().await.unwrap();
    let patient_id = patient["id"].as_str().unwrap();
    let imported:Value=client.post(format!("{base}/fhir/Patient/{patient_id}/$dicom-import")).json(&json!({"resourceType":"Parameters","parameter":[{"name":"connection","valueString":"pacs"},{"name":"study","valueString":"1.2.3"}]})).send().await.unwrap().json().await.unwrap();
    let study = imported["parameter"][0]["valueReference"]["reference"]
        .as_str()
        .unwrap();
    let id = create_task(&client, &base, patient_id, study).await;
    let params = json!({"resourceType":"Parameters","parameter":[{"name":"connection","valueString":"pacs"},{"name":"action","valueString":"submit"}]});
    let denied = http
        .post(format!("{base}/fhir/Task/{id}/$dicom-ups"))
        .json(&params)
        .send()
        .await
        .unwrap();
    assert_eq!(denied.status(), 403);
    let narrow: Value = http
        .post(format!("{base}/auth/token"))
        .basic_auth("ups-service", Some("isolated-ups-secret"))
        .form(&[
            ("grant_type", "client_credentials"),
            ("scope", "system/Task.r"),
        ])
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let denied = http
        .post(format!("{base}/fhir/Task/{id}/$dicom-ups"))
        .bearer_auth(narrow["access_token"].as_str().unwrap())
        .json(&params)
        .send()
        .await
        .unwrap();
    assert_eq!(denied.status(), 403);
    assert_eq!(state.creates.load(Ordering::SeqCst), 0);
    let submitted = command(&client, &base, &id, "submit").await;
    let workitem = parameter(&submitted, "workitem")["valueUri"]
        .as_str()
        .unwrap()
        .to_owned();
    let uid = workitem.rsplit('/').next().unwrap().to_owned();
    assert_eq!(
        parameter(&command(&client, &base, &id, "submit").await, "workitem")["valueUri"],
        workitem
    );
    // A 503 after successful creation must recover by GET, not mint another UID.
    let mut confirmed = false;
    for _ in 0..150 {
        let row = query("SELECT created_confirmed FROM dicom_ups_tasks WHERE task_id=$1")
            .bind(&id)
            .fetch_one(&pool)
            .await
            .unwrap();
        if row.get::<bool, _>("created_confirmed") {
            confirmed = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    assert!(confirmed, "Ambiguous creation did not recover");
    assert_eq!(state.creates.load(Ordering::SeqCst), 1);
    set_state(&state, &uid, "IN PROGRESS", false).await;
    let progress = wait_task(&client, &base, &id, "in-progress").await;
    assert_eq!(progress["meta"]["security"][0]["code"], "R");
    let mut annotated = progress.clone();
    annotated["note"] = json!([{"text":"Keep clinical annotation"}]);
    let response = client
        .put(format!("{base}/fhir/Task/{id}"))
        .json(&annotated)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200, "{}", response.text().await.unwrap());
    let other:Value=client.post(format!("{base}/fhir/Patient")).json(&json!({"resourceType":"Patient","identifier":[{"system":"urn:test:mrn","value":"MRN-2"}]})).send().await.unwrap().json().await.unwrap();
    let mut rebound = annotated.clone();
    rebound["for"]["reference"] = json!(format!("Patient/{}", other["id"].as_str().unwrap()));
    let denied = client
        .put(format!("{base}/fhir/Task/{id}"))
        .json(&rebound)
        .send()
        .await
        .unwrap();
    assert_eq!(denied.status(), 409, "{}", denied.text().await.unwrap());
    let denied_delete = client
        .delete(format!("{base}/fhir/Task/{id}"))
        .send()
        .await
        .unwrap();
    assert_eq!(
        denied_delete.status(),
        409,
        "{}",
        denied_delete.text().await.unwrap()
    );
    // Drop the embedding router and lose the notification; DB leases and GET recover.
    server.abort();
    tokio::time::sleep(Duration::from_millis(300)).await;
    set_state(&state, &uid, "COMPLETED", true).await;
    query("UPDATE async_jobs SET lease_expires_at=clock_timestamp()-interval '1 second' WHERE request_type='dicom-ups' AND status='in_progress'").execute(&pool).await.unwrap();
    let (base, server) = serve(&config).await;
    // Ephemeral development signing keys rotate when rebuilding the embedded app.
    let client = authenticated(&http, &base).await;
    let completed = wait_task(&client, &base, &id, "completed").await;
    assert_eq!(completed["note"][0]["text"], "Keep clinical annotation");
    assert_eq!(completed["output"].as_array().unwrap().len(), 2);
    for output in completed["output"].as_array().unwrap() {
        let reference = output["valueReference"]["reference"].as_str().unwrap();
        let resource: Value = client
            .get(format!("{base}/fhir/{reference}"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(
            resource["subject"]["reference"],
            format!("Patient/{patient_id}")
        );
        assert_eq!(resource["meta"]["security"][0]["code"], "R");
        assert!(!resource.to_string().contains("InlineBinary"));
        if resource["resourceType"] == "DiagnosticReport" {
            assert_eq!(resource["status"], "unknown");
        }
    }
    for _ in 0..100 {
        if state.released.load(Ordering::SeqCst) > 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(state.released.load(Ordering::SeqCst) > 0);
    let version = completed["meta"]["versionId"].clone();
    command(&client, &base, &id, "refresh").await;
    tokio::time::sleep(Duration::from_secs(3)).await;
    assert_eq!(
        wait_task(&client, &base, &id, "completed").await["meta"]["versionId"],
        version
    );
    // Cancellation wins before begin_creation's fenced compare-and-set.
    let before = state.creates.load(Ordering::SeqCst);
    state.block_input.store(true, Ordering::SeqCst);
    let cancelled = create_task(&client, &base, patient_id, study).await;
    command(&client, &base, &cancelled, "submit").await;
    tokio::time::timeout(Duration::from_secs(10), state.entered.notified())
        .await
        .unwrap();
    command(&client, &base, &cancelled, "cancel").await;
    state.block_input.store(false, Ordering::SeqCst);
    state.release.notify_one();
    wait_task(&client, &base, &cancelled, "cancelled").await;
    assert_eq!(state.creates.load(Ordering::SeqCst), before);
    assert_eq!(
        parameter(
            &command(&client, &base, &cancelled, "status").await,
            "cancelledBeforeSubmission"
        )["valueBoolean"],
        true
    );
    // An accepted cancel request must not fabricate a CANCELED source state.
    let cancel_later = create_task(&client, &base, patient_id, study).await;
    let submitted = command(&client, &base, &cancel_later, "submit").await;
    let cancel_uid = parameter(&submitted, "workitem")["valueUri"]
        .as_str()
        .unwrap()
        .rsplit('/')
        .next()
        .unwrap()
        .to_owned();
    for _ in 0..100 {
        if state.work.lock().await.contains_key(&cancel_uid) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    set_state(&state, &cancel_uid, "IN PROGRESS", false).await;
    wait_task(&client, &base, &cancel_later, "in-progress").await;
    command(&client, &base, &cancel_later, "cancel").await;
    let mut delivered = false;
    for _ in 0..100 {
        let status = command(&client, &base, &cancel_later, "status").await;
        if parameter(&status, "cancelDelivered")["valueBoolean"] == true {
            delivered = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(delivered);
    assert_eq!(
        wait_task(&client, &base, &cancel_later, "in-progress").await["status"],
        "in-progress"
    );
    set_state(&state, &cancel_uid, "CANCELED", false).await;
    wait_task(&client, &base, &cancel_later, "cancelled").await;
    assert_eq!(state.cancels.load(Ordering::SeqCst), 1);
    server.abort();
    pacs_task.abort();
}
