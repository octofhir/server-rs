//! Terminology in SQL (SQL on FHIR, "Terminology in SQL") through `$sql-run`,
//! `memberOf()` in a ViewDefinition, and a record-returning function read
//! through a column definition list in the sandbox.
//!
//! The specification's worked example end to end (a value set supplied with
//! its expansion, a concept map with a `noMap` element), value sets and
//! concept maps the server resolves itself (stored, from the R4 core
//! package, expanded by a terminology server), the provenance header, and
//! the 404 / 422 answers for a dependency that cannot be resolved or whose
//! content cannot be determined.
//!
//! Requirements: Docker running (testcontainers).

use std::sync::{Arc, Mutex};

use axum::extract::{Query, State};
use octofhir_config::ConfigurationManager;
use octofhir_server::{AppConfig, PostgresStorageConfig, build_app};
use reqwest::{Client, StatusCode};
use serde_json::{Value, json};
use testcontainers::{ImageExt, runners::AsyncRunner};
use testcontainers_modules::postgres::Postgres;

struct ServerHandle(tokio::task::JoinHandle<()>);

impl Drop for ServerHandle {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// `$expand` requests the stand-in terminology server received.
type Requests = Arc<Mutex<Vec<Vec<(String, String)>>>>;

/// A terminology server that expands the R4 `clinical-findings` value set
/// completely and `condition-code` only partly.
async fn start_terminology_server() -> (String, Requests, ServerHandle) {
    async fn expand(
        State(requests): State<Requests>,
        Query(query): Query<Vec<(String, String)>>,
    ) -> axum::response::Response {
        use axum::response::IntoResponse;
        requests.lock().unwrap().push(query.clone());
        let url = query
            .iter()
            .find(|(name, _)| name == "url")
            .map(|(_, value)| value.as_str())
            .unwrap_or_default();
        let expansion = match url {
            "http://hl7.org/fhir/ValueSet/clinical-findings" => json!({
                "identifier": "urn:uuid:7c1f0b5e-remote",
                "timestamp": "2026-11-01T00:00:00Z",
                "total": 2,
                "parameter": [{"name": "version",
                    "valueUri": "http://snomed.info/sct|http://snomed.info/sct/900000000000207008/version/20260901"}],
                "contains": [
                    {"system": "http://snomed.info/sct", "code": "22298006",
                     "display": "Myocardial infarction"},
                    {"system": "http://snomed.info/sct", "code": "73211009",
                     "display": "Diabetes mellitus"}
                ]
            }),
            "http://example.org/ValueSet/remote-only" => json!({
                "identifier": "urn:uuid:remote-only", "total": 1,
                "contains": [{"system": "http://hl7.org/fhir/sid/icd-10", "code": "I21"}]
            }),
            "http://hl7.org/fhir/ValueSet/condition-code" => json!({
                "identifier": "urn:uuid:paged", "total": 100000, "offset": 0,
                "contains": [{"system": "http://snomed.info/sct", "code": "22298006"}]
            }),
            _ => return (axum::http::StatusCode::NOT_FOUND, "").into_response(),
        };
        (
            [(axum::http::header::CONTENT_TYPE, "application/fhir+json")],
            json!({"resourceType": "ValueSet", "url": url, "expansion": expansion}).to_string(),
        )
            .into_response()
    }
    let requests: Requests = Arc::default();
    let app = axum::Router::new()
        .route("/ValueSet/$expand", axum::routing::get(expand))
        .with_state(requests.clone());
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (format!("http://{addr}"), requests, ServerHandle(server))
}

async fn start_server(postgres_url: &str, terminology_server: &str) -> (String, ServerHandle) {
    let mut config = AppConfig::default();
    config.storage.postgres = Some(PostgresStorageConfig {
        url: Some(postgres_url.to_owned()),
        pool_size: 5,
        connect_timeout_ms: 10_000,
        idle_timeout_ms: Some(60_000),
        ..Default::default()
    });
    config.auth.policy.anonymous_access = true;
    config.sql_on_fhir.enabled = true;
    config.terminology.server_url = terminology_server.to_owned();
    config.packages.load = vec![octofhir_server::config::PackageSpec::Table {
        id: Some("hl7.fhir.r4.core".into()),
        version: Some("4.0.1".into()),
        path: None,
        url: Some("https://hl7.org/fhir/R4/hl7.fhir.r4.core.tgz".into()),
    }];
    let config_manager = Arc::new(
        ConfigurationManager::builder()
            .build()
            .await
            .expect("config manager"),
    );
    let app = build_app(&config, config_manager).await.expect("build app");
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("bind");
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (format!("http://{addr}/fhir"), ServerHandle(server))
}

async fn put(client: &Client, fhir: &str, resource: Value) {
    let path = format!(
        "{fhir}/{}/{}",
        resource["resourceType"].as_str().unwrap(),
        resource["id"].as_str().unwrap()
    );
    let response = client.put(path).json(&resource).send().await.unwrap();
    let status = response.status();
    let body = response.text().await.unwrap();
    assert!(status.is_success(), "{status}: {body}");
}

struct Answer {
    status: StatusCode,
    provenance: Option<Value>,
    body: String,
}

impl Answer {
    fn rows(&self) -> Vec<Value> {
        assert_eq!(self.status, StatusCode::OK, "{}", self.body);
        serde_json::from_str::<Value>(&self.body)
            .unwrap_or_else(|error| panic!("{error}: {}", self.body))
            .as_array()
            .unwrap()
            .clone()
    }
}

async fn run(client: &Client, url: &str, parameters: Vec<Value>) -> Answer {
    let response = client
        .post(url)
        .json(&json!({"resourceType": "Parameters", "parameter": parameters}))
        .send()
        .await
        .unwrap();
    let status = response.status();
    let provenance = response
        .headers()
        .get("x-sof-terminology")
        .map(|value| serde_json::from_str(value.to_str().unwrap()).unwrap());
    Answer {
        status,
        provenance,
        body: response.text().await.unwrap(),
    }
}

const LIBRARY_TYPES: &str = "http://hl7.org/fhir/uv/sql-on-fhir/CodeSystem/LibraryTypesCodes";

fn sql_query(sql: &str, depends_on: &[(&str, &str)]) -> Value {
    use base64::Engine;
    json!({
        "resourceType": "Library",
        "url": "http://example.org/Library/TerminologyQuery",
        "status": "active",
        "type": {"coding": [{"system": LIBRARY_TYPES, "code": "sql-query"}]},
        "relatedArtifact": depends_on.iter().map(|(label, canonical)| json!({
            "type": "depends-on", "label": label, "resource": canonical
        })).collect::<Vec<_>>(),
        "content": [{
            "contentType": "application/sql",
            "data": base64::engine::general_purpose::STANDARD.encode(sql)
        }]
    })
}

fn subject(library: Value) -> Value {
    json!({"name": "subjectResource", "resource": library})
}

fn context(resource: &Value) -> Value {
    json!({"name": "context", "resource": resource})
}

fn json_format() -> Value {
    json!({"name": "_format", "valueCode": "json"})
}

/// The value set of the specification's worked example, with its expansion.
fn cvd_codes() -> Value {
    json!({
        "resourceType": "ValueSet",
        "url": "http://example.org/ValueSet/cardiovascular-disease",
        "version": "2026",
        "name": "CardiovascularDisease",
        "status": "active",
        "expansion": {
            "identifier": "urn:uuid:635ad7e4-4f98-48ef-a3c2-1e3af6cc03ec",
            "timestamp": "2026-09-16T00:00:00Z",
            "total": 3,
            "parameter": [
                {"name": "version", "valueUri": "http://snomed.info/sct|http://snomed.info/sct/900000000000207008/version/20260201"},
                {"name": "version", "valueUri": "http://hl7.org/fhir/sid/icd-10|2019"}
            ],
            "contains": [{
                "abstract": true,
                "display": "Ischaemic heart disease",
                "contains": [
                    {"system": "http://snomed.info/sct",
                     "version": "http://snomed.info/sct/900000000000207008/version/20260201",
                     "code": "22298006", "display": "Myocardial infarction"},
                    {"system": "http://hl7.org/fhir/sid/icd-10", "version": "2019",
                     "code": "I21", "display": "Acute myocardial infarction"}
                ]
            }]
        }
    })
}

/// The concept map of the specification's worked example.
fn sct_to_icd10() -> Value {
    json!({
        "resourceType": "ConceptMap",
        "url": "http://example.org/ConceptMap/sct-to-icd10",
        "version": "2026",
        "name": "SnomedCtToIcd10",
        "status": "active",
        "group": [{
            "source": "http://snomed.info/sct",
            "target": "http://hl7.org/fhir/sid/icd-10|2019",
            "element": [
                {"code": "22298006", "display": "Myocardial infarction", "target": [
                    {"code": "I21", "display": "Acute myocardial infarction",
                     "relationship": "equivalent"}]},
                {"code": "73211009", "display": "Diabetes mellitus", "target": [
                    {"code": "E14", "display": "Unspecified diabetes mellitus",
                     "relationship": "source-is-broader-than-target"}]},
                {"code": "102499006", "display": "Fit and well", "noMap": true}
            ]
        }]
    })
}

const CONDITIONS: &str = "http://example.org/ViewDefinition/conditions";
const CVD: &str = "http://example.org/ValueSet/cardiovascular-disease|2026";
const SCT_TO_ICD10: &str = "http://example.org/ConceptMap/sct-to-icd10|2026";

#[tokio::test]
async fn terminology_dependencies_are_relations() {
    let postgres = Postgres::default()
        .with_tag("17-alpine")
        .start()
        .await
        .expect("postgres");
    let port = postgres.get_host_port_ipv4(5432).await.unwrap();
    let (terminology_server, requests, _tx) = start_terminology_server().await;
    let (fhir, _server) = start_server(
        &format!("postgres://postgres:postgres@127.0.0.1:{port}/postgres"),
        &terminology_server,
    )
    .await;
    let client = Client::new();
    let sql_run = format!("{fhir}/$sql-run");

    // The worked example's data: one condition per patient.
    for (patient, gender, system, code) in [
        ("p1", "male", "http://snomed.info/sct", "22298006"),
        ("p2", "female", "http://hl7.org/fhir/sid/icd-10", "I21"),
        ("p3", "other", "http://snomed.info/sct", "73211009"),
        ("p4", "unknown", "http://snomed.info/sct", "102499006"),
    ] {
        let mut resource = json!({"resourceType": "Patient", "id": patient, "gender": gender});
        if patient == "p1" {
            resource["maritalStatus"] = json!({"coding": [{
                "system": "http://terminology.hl7.org/CodeSystem/v3-MaritalStatus", "code": "M"}]});
        }
        put(&client, &fhir, resource).await;
        put(
            &client,
            &fhir,
            json!({"resourceType": "Condition", "id": format!("c-{patient}"),
                "subject": {"reference": format!("Patient/{patient}")},
                "code": {"coding": [{"system": system, "code": code}]}}),
        )
        .await;
    }
    let conditions = json!({"resourceType": "ViewDefinition", "status": "active",
    "url": CONDITIONS, "name": "conditions", "resource": "Condition",
    "select": [
        {"column": [{"name": "patient_id", "path": "subject.getReferenceKey(Patient)", "type": "id"}]},
        {"forEach": "code.coding", "column": [
            {"name": "system", "path": "system", "type": "uri"},
            {"name": "version", "path": "version", "type": "string"},
            {"name": "code", "path": "code", "type": "code"}]}
    ]});
    let patients = json!({"resourceType": "ViewDefinition", "status": "active",
        "url": "http://example.org/ViewDefinition/patients", "name": "patients",
        "resource": "Patient",
        "select": [{"column": [
            {"name": "id", "path": "getResourceKey()", "type": "id"},
            {"name": "gender", "path": "gender", "type": "code"}]}]});

    // Value set membership as a semi-join (the specification's query).
    let membership = sql_query(
        "SELECT conditions.patient_id, conditions.code FROM conditions \
         WHERE EXISTS (SELECT 1 FROM cvd_codes \
           WHERE cvd_codes.system = conditions.system AND cvd_codes.code = conditions.code) \
         ORDER BY conditions.patient_id",
        &[("conditions", CONDITIONS), ("cvd_codes", CVD)],
    );
    let answer = run(
        &client,
        &sql_run,
        vec![
            subject(membership.clone()),
            context(&conditions),
            context(&cvd_codes()),
            json_format(),
        ],
    )
    .await;
    assert_eq!(
        answer.rows(),
        [
            json!({"patient_id": "p1", "code": "22298006"}),
            json!({"patient_id": "p2", "code": "I21"})
        ]
    );
    let provenance = answer.provenance.expect("provenance header");
    assert_eq!(provenance[0]["label"], "cvd_codes");
    assert_eq!(provenance[0]["source"], "context");
    assert_eq!(provenance[0]["version"], "2026");
    assert_eq!(
        provenance[0]["expansion"]["identifier"],
        "urn:uuid:635ad7e4-4f98-48ef-a3c2-1e3af6cc03ec"
    );
    assert_eq!(
        provenance[0]["expansion"]["parameter"]
            .as_array()
            .unwrap()
            .len(),
        2
    );

    // The relation itself: the abstract grouping entry is not a member.
    let relation = run(
        &client,
        &sql_run,
        vec![
            subject(sql_query(
                "SELECT system, version, code, display, inactive FROM cvd_codes ORDER BY code",
                &[("cvd_codes", CVD)],
            )),
            context(&cvd_codes()),
            json_format(),
        ],
    )
    .await;
    assert_eq!(
        relation.rows(),
        [
            json!({"system": "http://snomed.info/sct",
                   "version": "http://snomed.info/sct/900000000000207008/version/20260201",
                   "code": "22298006", "display": "Myocardial infarction", "inactive": null}),
            json!({"system": "http://hl7.org/fhir/sid/icd-10", "version": "2019",
                   "code": "I21", "display": "Acute myocardial infarction", "inactive": null})
        ]
    );

    // Two labels naming one value set, pinned and not: one content, one record.
    let twice = run(
        &client,
        &sql_run,
        vec![
            subject(sql_query(
                "SELECT count(*) AS n FROM pinned JOIN bare USING (system, version, code)",
                &[
                    ("pinned", CVD),
                    ("bare", "http://example.org/ValueSet/cardiovascular-disease"),
                ],
            )),
            context(&cvd_codes()),
            json_format(),
        ],
    )
    .await;
    assert_eq!(twice.rows(), [json!({"n": 2})]);
    assert_eq!(twice.provenance.unwrap().as_array().unwrap().len(), 1);

    // Translation with a LEFT JOIN, keeping noMap rows apart from silence.
    let translation = sql_query(
        "SELECT conditions.patient_id, conditions.code, sct_to_icd10.target_code, \
                sct_to_icd10.relationship, sct_to_icd10.source_code IS NULL AS silent \
         FROM conditions LEFT JOIN sct_to_icd10 \
           ON sct_to_icd10.source_system = conditions.system \
          AND sct_to_icd10.source_code = conditions.code \
          AND (sct_to_icd10.relationship IS NULL OR sct_to_icd10.relationship <> 'not-related-to') \
         ORDER BY conditions.patient_id",
        &[("conditions", CONDITIONS), ("sct_to_icd10", SCT_TO_ICD10)],
    );
    let answer = run(
        &client,
        &sql_run,
        vec![
            subject(translation.clone()),
            context(&conditions),
            context(&sct_to_icd10()),
            json_format(),
        ],
    )
    .await;
    assert_eq!(
        answer.rows(),
        [
            json!({"patient_id": "p1", "code": "22298006", "target_code": "I21",
                   "relationship": "equivalent", "silent": false}),
            json!({"patient_id": "p2", "code": "I21", "target_code": null,
                   "relationship": null, "silent": true}),
            json!({"patient_id": "p3", "code": "73211009", "target_code": "E14",
                   "relationship": "source-is-broader-than-target", "silent": false}),
            json!({"patient_id": "p4", "code": "102499006", "target_code": null,
                   "relationship": null, "silent": false})
        ]
    );
    let provenance = answer.provenance.expect("provenance header");
    assert_eq!(provenance[0]["resourceType"], "ConceptMap");
    assert_eq!(provenance[0]["mappings"], 3);

    // A concept map with dependsOn cannot be a relation: 422 before any SQL.
    let mut conditional = sct_to_icd10();
    conditional["group"][0]["element"][0]["target"][0]["dependsOn"] =
        json!([{"attribute": "laterality", "valueCode": "left"}]);
    let answer = run(
        &client,
        &sql_run,
        vec![
            subject(translation.clone()),
            context(&conditions),
            context(&conditional),
        ],
    )
    .await;
    assert_eq!(
        answer.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        answer.body
    );
    assert!(answer.body.contains("dependsOn"), "{}", answer.body);

    // A supplied expansion that is a page does not determine membership: 422.
    let mut paged = cvd_codes();
    paged["expansion"]["total"] = json!(40);
    let answer = run(
        &client,
        &sql_run,
        vec![
            subject(membership.clone()),
            context(&conditions),
            context(&paged),
        ],
    )
    .await;
    assert_eq!(
        answer.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        answer.body
    );
    assert!(answer.body.contains("of 40"), "{}", answer.body);

    // A value set nobody can resolve, the terminology server included: 404.
    let answer = run(
        &client,
        &sql_run,
        vec![subject(membership.clone()), context(&conditions)],
    )
    .await;
    assert_eq!(answer.status, StatusCode::NOT_FOUND, "{}", answer.body);

    // Stored on the server: a code system and a value set over it, computed
    // here; pinned to its version.
    put(
        &client,
        &fhir,
        json!({"resourceType": "CodeSystem", "id": "cardio", "url": "http://example.org/cs/cardio",
            "version": "5", "status": "active", "content": "complete",
            "concept": [{"code": "heart", "display": "Heart disease", "concept": [
                {"code": "22298006", "display": "Myocardial infarction"}]},
                {"code": "lung", "display": "Lung disease"}]}),
    )
    .await;
    for (id, version, filter_value) in [("cardio-1", "1", "heart"), ("cardio-2", "2", "lung")] {
        put(
            &client,
            &fhir,
            json!({"resourceType": "ValueSet", "id": id, "status": "active",
                "url": "http://example.org/ValueSet/cardio", "version": version,
                "compose": {"include": [{"system": "http://example.org/cs/cardio",
                    "filter": [{"property": "concept", "op": "is-a", "value": filter_value}]}]}}),
        )
        .await;
    }
    let stored = |canonical: &str| {
        sql_query(
            "SELECT code, version FROM cardio ORDER BY code",
            &[("cardio", canonical)],
        )
    };
    let answer = run(
        &client,
        &sql_run,
        vec![
            subject(stored("http://example.org/ValueSet/cardio|1")),
            json_format(),
        ],
    )
    .await;
    assert_eq!(
        answer.rows(),
        [
            json!({"code": "22298006", "version": "5"}),
            json!({"code": "heart", "version": "5"})
        ]
    );
    let provenance = answer.provenance.expect("provenance header");
    assert_eq!(provenance[0]["source"], "ValueSet/cardio-1");
    assert_eq!(provenance[0]["expandedBy"], "this server");
    assert_eq!(
        provenance[0]["expansion"]["parameter"],
        json!([{"name": "version", "valueUri": "http://example.org/cs/cardio|5"}])
    );
    // Two stored versions and no version pinned: 404.
    let answer = run(
        &client,
        &sql_run,
        vec![subject(stored("http://example.org/ValueSet/cardio"))],
    )
    .await;
    assert_eq!(answer.status, StatusCode::NOT_FOUND, "{}", answer.body);
    assert!(answer.body.contains("pin"), "{}", answer.body);

    // A stored value set over SNOMED CT is not sent anywhere: 422.
    put(
        &client,
        &fhir,
        json!({"resourceType": "ValueSet", "id": "local-sct", "status": "active",
            "url": "http://example.org/ValueSet/local-sct",
            "compose": {"include": [{"system": "http://snomed.info/sct",
                "filter": [{"property": "concept", "op": "is-a", "value": "404684003"}]}]}}),
    )
    .await;
    let answer = run(
        &client,
        &sql_run,
        vec![subject(stored("http://example.org/ValueSet/local-sct"))],
    )
    .await;
    assert_eq!(
        answer.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        answer.body
    );
    assert!(answer.body.contains("snomed"), "{}", answer.body);
    assert!(
        !requests
            .lock()
            .unwrap()
            .iter()
            .flatten()
            .any(|(_, value)| value.contains("local-sct"))
    );

    // Found nowhere here: the terminology server expands it by canonical.
    let remote_only = sql_query(
        "SELECT conditions.patient_id FROM conditions JOIN codes \
           ON codes.system = conditions.system AND codes.code = conditions.code",
        &[
            ("conditions", CONDITIONS),
            ("codes", "http://example.org/ValueSet/remote-only"),
        ],
    );
    let answer = run(
        &client,
        &sql_run,
        vec![subject(remote_only), context(&conditions), json_format()],
    )
    .await;
    assert_eq!(answer.rows(), [json!({"patient_id": "p2"})]);
    let provenance = answer.provenance.expect("provenance header");
    assert_eq!(
        provenance[0]["source"],
        format!("terminology server {terminology_server}")
    );
    assert_eq!(provenance[0]["expandedBy"], provenance[0]["source"]);
    assert_eq!(
        provenance[0]["expansion"]["identifier"],
        "urn:uuid:remote-only"
    );

    // From the R4 core package: a value set expanded here and a concept map
    // (R4 `equivalence`), joined to the patients.
    let genders = sql_query(
        "SELECT patients.id, genders.display, gender_v2.target_code, gender_v2.relationship \
         FROM patients \
         JOIN genders ON genders.system = 'http://hl7.org/fhir/administrative-gender' \
          AND genders.code = patients.gender \
         LEFT JOIN gender_v2 ON gender_v2.source_system = genders.system \
          AND gender_v2.source_code = genders.code \
         ORDER BY patients.id, gender_v2.target_code",
        &[
            ("patients", "http://example.org/ViewDefinition/patients"),
            (
                "genders",
                "http://hl7.org/fhir/ValueSet/administrative-gender|4.0.1",
            ),
            (
                "gender_v2",
                "http://hl7.org/fhir/ConceptMap/cm-administrative-gender-v2",
            ),
        ],
    );
    let answer = run(
        &client,
        &sql_run,
        vec![subject(genders), context(&patients), json_format()],
    )
    .await;
    assert_eq!(
        answer.rows(),
        [
            json!({"id": "p1", "display": "Male", "target_code": "M", "relationship": "equivalent"}),
            json!({"id": "p2", "display": "Female", "target_code": "F", "relationship": "equivalent"}),
            json!({"id": "p3", "display": "Other", "target_code": "A",
                   "relationship": "source-is-narrower-than-target"}),
            json!({"id": "p3", "display": "Other", "target_code": "O",
                   "relationship": "source-is-narrower-than-target"}),
            json!({"id": "p4", "display": "Unknown", "target_code": "U", "relationship": "equivalent"})
        ]
    );
    let provenance = answer.provenance.expect("provenance header");
    assert_eq!(provenance[0]["source"], "package hl7.fhir.r4.core#4.0.1");
    assert_eq!(
        provenance[0]["expansion"]["parameter"],
        json!([{"name": "version", "valueUri": "http://hl7.org/fhir/administrative-gender|4.0.1"}])
    );
    assert_eq!(provenance[1]["resourceType"], "ConceptMap");
    assert_eq!(provenance[1]["source"], "package hl7.fhir.r4.core#4.0.1");

    // A published value set over SNOMED CT: the terminology server expands
    // it, by canonical; a partial expansion from it is refused.
    let findings = sql_query(
        "SELECT conditions.patient_id FROM conditions WHERE EXISTS (SELECT 1 FROM findings \
           WHERE findings.system = conditions.system AND findings.code = conditions.code) \
         ORDER BY 1",
        &[
            ("conditions", CONDITIONS),
            ("findings", "http://hl7.org/fhir/ValueSet/clinical-findings"),
        ],
    );
    let answer = run(
        &client,
        &sql_run,
        vec![subject(findings), context(&conditions), json_format()],
    )
    .await;
    assert_eq!(
        answer.rows(),
        [json!({"patient_id": "p1"}), json!({"patient_id": "p3"})]
    );
    let provenance = answer.provenance.expect("provenance header");
    assert_eq!(
        provenance[0]["expandedBy"],
        format!("terminology server {terminology_server}")
    );
    assert_eq!(
        provenance[0]["expansion"]["identifier"],
        "urn:uuid:7c1f0b5e-remote"
    );
    {
        let requests = requests.lock().unwrap();
        let query = requests
            .iter()
            .find(|query| {
                query.contains(&(
                    "url".into(),
                    "http://hl7.org/fhir/ValueSet/clinical-findings".into(),
                ))
            })
            .expect("clinical-findings was asked for");
        assert!(query.contains(&("valueSetVersion".into(), "4.0.1".into())));
    }
    let partial = sql_query(
        "SELECT count(*) AS n FROM codes",
        &[("codes", "http://hl7.org/fhir/ValueSet/condition-code")],
    );
    let answer = run(&client, &sql_run, vec![subject(partial)]).await;
    assert_eq!(
        answer.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        answer.body
    );
    assert!(answer.body.contains("1 of 100000"), "{}", answer.body);

    // $expand computes membership with the same strict expander.
    let expand = |query: Vec<(&'static str, &'static str)>| {
        let client = client.clone();
        let url = format!("{fhir}/ValueSet/$expand");
        async move {
            let response = client.get(url).query(&query).send().await.unwrap();
            let status = response.status();
            (status, response.json::<Value>().await.unwrap())
        }
    };
    let (status, body) = expand(vec![(
        "url",
        "http://hl7.org/fhir/ValueSet/administrative-gender",
    )])
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["expansion"]["total"], 4);
    let (status, body) = expand(vec![
        ("url", "http://example.org/ValueSet/cardio"),
        ("valueSetVersion", "1"),
        ("filter", "infarct"),
    ])
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["expansion"]["total"], 1);
    assert_eq!(body["expansion"]["contains"][0]["code"], "22298006");
    // A stored value set over SNOMED CT: reported, not half-expanded.
    let (status, body) = expand(vec![("url", "http://example.org/ValueSet/local-sct")]).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");

    // A record-returning function read through a column definition list
    // runs in the sandbox.
    let records = sql_query(
        "SELECT t.a, t.b FROM jsonb_to_recordset('[{\"a\":1,\"b\":\"x\"},{\"a\":2}]'::jsonb) \
         AS t(a int, b text) ORDER BY t.a",
        &[],
    );
    let answer = run(&client, &sql_run, vec![subject(records), json_format()]).await;
    assert_eq!(
        answer.rows(),
        [json!({"a": 1, "b": "x"}), json!({"a": 2, "b": null})]
    );
    assert!(answer.provenance.is_none());

    // memberOf() in a ViewDefinition path runs in memory with the server's
    // terminology layer.
    let married = json!({"resourceType": "ViewDefinition", "status": "active",
        "name": "married", "resource": "Patient",
        "select": [{"column": [
            {"name": "id", "path": "getResourceKey()", "type": "id"},
            {"name": "married", "type": "boolean",
             "path": "maritalStatus.memberOf('http://hl7.org/fhir/ValueSet/marital-status')"}]}]});
    let answer = run(
        &client,
        &format!("{fhir}/$viewdefinition-run"),
        vec![
            json!({"name": "viewResource", "resource": married}),
            json!({"name": "patient", "valueReference": {"reference": "Patient/p1"}}),
            json_format(),
        ],
    )
    .await;
    assert_eq!(answer.rows(), [json!({"id": "p1", "married": true})]);
}
