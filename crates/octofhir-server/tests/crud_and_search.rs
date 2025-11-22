use octofhir_server::AppConfig;
use octofhir_server::build_app;
use serde_json::Value;
use serde_json::json;
use tokio::task::JoinHandle;

async fn start_server() -> (String, tokio::sync::oneshot::Sender<()>, JoinHandle<()>) {
    let app = build_app(&AppConfig::default()).await.expect("build app");

    // Bind to an ephemeral port
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

    (format!("http://{addr}"), tx, server)
}

#[tokio::test]
async fn patient_crud_and_search_flow() {
    let (base, shutdown_tx, handle) = start_server().await;
    let client = reqwest::Client::new();

    // Create Patient
    let payload = json!({
        "resourceType": "Patient",
        "active": true,
        "name": [{"family": "Smith", "given": ["John"]}],
        "identifier": [{"system": "http://sys", "value": "MRN-123"}],
    });
    let resp = client
        .post(format!("{base}/Patient"))
        .header("accept", "application/fhir+json")
        .header("content-type", "application/fhir+json")
        .json(&payload)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::CREATED);
    let created: Value = resp.json().await.unwrap();
    let id = created["id"].as_str().expect("created id").to_string();

    // Read Patient
    let resp = client
        .get(format!("{base}/Patient/{id}"))
        .header("accept", "application/fhir+json")
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
    let read_back: Value = resp.json().await.unwrap();
    assert_eq!(read_back["id"], id);
    assert_eq!(read_back["resourceType"], "Patient");

    // Update Patient
    let updated = json!({
        "resourceType": "Patient",
        "id": id,
        "active": true,
        "name": [{"family": "Smith", "given": ["Johnny"]}],
        "identifier": [{"system": "http://sys", "value": "MRN-123"}],
    });
    let resp = client
        .put(format!(
            "{}/Patient/{}",
            base,
            updated["id"].as_str().unwrap()
        ))
        .header("accept", "application/fhir+json")
        .header("content-type", "application/fhir+json")
        .json(&updated)
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
    let after_update: Value = resp.json().await.unwrap();
    assert_eq!(after_update["name"][0]["given"][0], "Johnny");

    // Search by name contains
    let resp = client
        .get(format!("{base}/Patient?name=John"))
        .header("accept", "application/fhir+json")
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
    let bundle: Value = resp.json().await.unwrap();
    assert_eq!(bundle["resourceType"], "Bundle");
    assert_eq!(bundle["type"], "searchset");
    let entries = bundle["entry"].as_array().cloned().unwrap_or_default();
    assert!(entries.iter().any(|e| e["resource"]["id"] == id));

    // Search by identifier
    let resp = client
        .get(format!("{base}/Patient?identifier=http://sys|MRN-123"))
        .header("accept", "application/fhir+json")
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
    let bundle2: Value = resp.json().await.unwrap();
    assert_eq!(bundle2["resourceType"], "Bundle");
    let entries2 = bundle2["entry"].as_array().cloned().unwrap_or_default();
    assert!(entries2.iter().any(|e| e["resource"]["id"] == id));

    // Delete Patient
    let resp = client
        .delete(format!(
            "{}/Patient/{}",
            base,
            read_back["id"].as_str().unwrap()
        ))
        .header("accept", "application/fhir+json")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::NO_CONTENT);

    // Reading after delete should produce 410 Gone (soft delete)
    let resp = client
        .get(format!("{base}/Patient/{id}"))
        .header("accept", "application/fhir+json")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::GONE);

    let _ = shutdown_tx.send(());
    let _ = handle.await;
}

#[tokio::test]
async fn search_pagination_and_links() {
    let (base, shutdown_tx, handle) = start_server().await;
    let client = reqwest::Client::new();

    // Insert a few patients
    for i in 0..5u8 {
        let payload = json!({
            "resourceType": "Patient",
            "name": [{"family": format!("Fam{}", i)}],
            "identifier": [{"system": "http://sys", "value": format!("X{}", i)}],
        });
        let resp = client
            .post(format!("{base}/Patient"))
            .header("accept", "application/fhir+json")
            .header("content-type", "application/fhir+json")
            .json(&payload)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), reqwest::StatusCode::CREATED);
    }

    // Request count=2 offset=2
    let resp = client
        .get(format!("{base}/Patient?_count=2&_offset=2"))
        .header("accept", "application/fhir+json")
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
    let bundle: Value = resp.json().await.unwrap();
    assert_eq!(bundle["resourceType"], "Bundle");
    assert_eq!(bundle["type"], "searchset");
    // Check links present
    let links = bundle["link"].as_array().cloned().unwrap_or_default();
    let rels: std::collections::HashSet<String> = links
        .iter()
        .filter_map(|l| l["relation"].as_str().map(|s| s.to_string()))
        .collect();
    assert!(rels.contains("self") && rels.contains("first") && rels.contains("last"));

    let _ = shutdown_tx.send(());
    let _ = handle.await;
}

#[tokio::test]
async fn error_cases_invalid_resource_and_id_mismatch_and_delete_404() {
    let (base, shutdown_tx, handle) = start_server().await;
    let client = reqwest::Client::new();

    // POST invalid resourceType vs path
    let bad = json!({"resourceType": "Observation"});
    let resp = client
        .post(format!("{base}/Patient"))
        .header("accept", "application/fhir+json")
        .header("content-type", "application/fhir+json")
        .json(&bad)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::BAD_REQUEST);

    // Create a Patient first
    let payload = json!({"resourceType": "Patient", "active": true});
    let resp = client
        .post(format!("{base}/Patient"))
        .header("accept", "application/fhir+json")
        .header("content-type", "application/fhir+json")
        .json(&payload)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::CREATED);
    let created: Value = resp.json().await.unwrap();
    let id = created["id"].as_str().unwrap().to_string();

    // PUT id mismatch (body id != path id)
    let mism = json!({"resourceType": "Patient", "id": "DIFFERENT"});
    let resp = client
        .put(format!("{base}/Patient/{id}"))
        .header("accept", "application/fhir+json")
        .header("content-type", "application/fhir+json")
        .json(&mism)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::BAD_REQUEST);

    // DELETE non-existent resource - per FHIR spec, delete is idempotent (204 No Content)
    let resp = client
        .delete(format!("{}/Patient/{}", base, "non-existent-id"))
        .header("accept", "application/fhir+json")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::NO_CONTENT);

    let _ = shutdown_tx.send(());
    let _ = handle.await;
}
