//! Keyset paging, long POST searches, `Patient/$summary`, `$validate-code` with `_explain`,
//! and the MCP endpoint, over one server.
//!
//! Requirements: Docker running (testcontainers).

use base64::Engine as _;
use std::collections::HashSet;
use std::sync::Arc;

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

async fn start_server(postgres_url: &str) -> (String, ServerHandle) {
    let mut config = AppConfig::default();
    config.storage.postgres = Some(PostgresStorageConfig {
        url: Some(postgres_url.to_owned()),
        pool_size: 10,
        connect_timeout_ms: 10_000,
        idle_timeout_ms: Some(60_000),
        ..Default::default()
    });
    config.auth.policy.anonymous_access = true;
    config.mcp.allow_writes = true;
    config.cql.enabled = true;
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
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("bind");
    let addr = listener.local_addr().unwrap();
    config.server.base_url = Some(format!("http://{addr}"));
    let app = build_app(&config, config_manager).await.expect("build app");
    let server = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (format!("http://{addr}"), ServerHandle(server))
}

async fn put(client: &Client, base: &str, resource: Value) {
    let path = format!(
        "{base}/fhir/{}/{}",
        resource["resourceType"].as_str().unwrap(),
        resource["id"].as_str().unwrap()
    );
    let response = client.put(path).json(&resource).send().await.unwrap();
    let status = response.status();
    let body = response.text().await.unwrap();
    assert!(status.is_success(), "{status}: {body}");
}

async fn get(client: &Client, url: &str) -> Value {
    let response = client.get(url).send().await.unwrap();
    let status = response.status();
    let body = response.text().await.unwrap();
    assert!(status.is_success(), "{url}: {status}: {body}");
    serde_json::from_str(&body).unwrap()
}

fn link<'a>(bundle: &'a Value, relation: &str) -> Option<&'a str> {
    bundle["link"]
        .as_array()?
        .iter()
        .find(|link| link["relation"] == relation)?["url"]
        .as_str()
}

fn ids(bundle: &Value) -> Vec<String> {
    bundle["entry"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|entry| entry["resource"]["id"].as_str().map(str::to_owned))
        .collect()
}

/// Follows `next` to the end; the ids in order, and how many pages it took.
async fn walk(client: &Client, first: &str) -> (Vec<String>, usize) {
    let mut seen = Vec::new();
    let mut pages = 0;
    let mut url = first.to_owned();
    loop {
        let bundle = get(client, &url).await;
        seen.extend(ids(&bundle));
        pages += 1;
        assert!(link(&bundle, "last").is_none() || !url.contains("_cursor"));
        match link(&bundle, "next") {
            Some(next) => url = next.to_owned(),
            None => return (seen, pages),
        }
        assert!(pages < 50, "paging does not end");
    }
}

async fn keyset_paging(client: &Client, base: &str) {
    for n in 0..7 {
        put(
            client,
            base,
            json!({"resourceType":"Practitioner","id":format!("pr-{n}")}),
        )
        .await;
    }
    let all: Vec<String> = (0..7).map(|n| format!("pr-{n}")).collect();

    // A plain listing seeks by id.
    let first = get(client, &format!("{base}/fhir/Practitioner?_count=3")).await;
    assert!(link(&first, "next").unwrap().contains("_cursor="));
    assert!(!link(&first, "next").unwrap().contains("_offset"));
    let (seen, pages) = walk(client, &format!("{base}/fhir/Practitioner?_count=3")).await;
    assert_eq!(seen, all);
    assert_eq!(pages, 3);

    let (seen, _) = walk(
        client,
        &format!("{base}/fhir/Practitioner?_sort=-_id&_count=3"),
    )
    .await;
    let mut reversed = all.clone();
    reversed.reverse();
    assert_eq!(seen, reversed);

    // Order by last update, with every row once.
    let (seen, _) = walk(
        client,
        &format!("{base}/fhir/Practitioner?_sort=-_lastUpdated&_count=2"),
    )
    .await;
    assert_eq!(seen.iter().collect::<HashSet<_>>().len(), 7);
    let mut newest_last = seen.clone();
    newest_last.reverse();
    assert_eq!(newest_last, all);

    // A resource written between pages neither repeats nor hides another row.
    let first = get(client, &format!("{base}/fhir/Practitioner?_count=3")).await;
    put(
        client,
        base,
        json!({"resourceType":"Practitioner","id":"pr-0","active":true}),
    )
    .await;
    let next = get(client, link(&first, "next").unwrap()).await;
    assert_eq!(ids(&next), ["pr-3", "pr-4", "pr-5"]);

    // Criteria without a keyset order stay on offsets, and a bad cursor is refused.
    let filtered = get(
        client,
        &format!("{base}/fhir/Practitioner?active=true&_count=1"),
    )
    .await;
    assert!(link(&filtered, "next").is_none());
    let invalid = client
        .get(format!(
            "{base}/fhir/Practitioner?_sort=-_lastUpdated&_cursor=nope"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);
}

async fn long_post_search(client: &Client, base: &str) {
    let mut wanted: Vec<String> = (0..7).map(|n| format!("pr-{n}")).collect();
    wanted.extend((0..600).map(|n| format!("absent-{n}")));
    let form = format!("_id={}&_count=3&_sort=_id", wanted.join(","));
    let response = client
        .post(format!("{base}/fhir/Practitioner/_search"))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(form)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let first: Value = response.json().await.unwrap();
    assert_eq!(ids(&first).len(), 3);
    let next = link(&first, "next").expect("a next link");
    assert!(
        next.len() < 300,
        "the link must not repeat the query: {next}"
    );
    assert!(next.contains("_page="));
    let (seen, _) = walk(client, next).await;
    assert_eq!(seen.len(), 4, "the rest of the seven, by the kept query");
    let gone = client
        .get(format!("{base}/fhir/Practitioner?_page=unknown"))
        .send()
        .await
        .unwrap();
    assert_eq!(gone.status(), StatusCode::GONE);
}

async fn summary(client: &Client, base: &str) {
    put(
        client,
        base,
        json!({"resourceType":"Patient","id":"s1","identifier":[
        {"system":"urn:mrn","value":"123"}]}),
    )
    .await;
    put(client, base, json!({"resourceType":"Patient","id":"s2"})).await;
    put(
        client,
        base,
        json!({"resourceType":"Organization","id":"org","name":"Lab"}),
    )
    .await;
    put(client, base, json!({"resourceType":"Condition","id":"c-active",
        "clinicalStatus":{"coding":[{"system":"http://terminology.hl7.org/CodeSystem/condition-clinical","code":"active"}]},
        "code":{"text":"Asthma"},"subject":{"reference":"Patient/s1"}}))
    .await;
    put(client, base, json!({"resourceType":"Condition","id":"c-old",
        "clinicalStatus":{"coding":[{"system":"http://terminology.hl7.org/CodeSystem/condition-clinical","code":"resolved"}]},
        "code":{"text":"Measles"},"subject":{"reference":"Patient/s1"}}))
    .await;
    put(client, base, json!({"resourceType":"Observation","id":"lab","status":"final",
        "category":[{"coding":[{"system":"http://terminology.hl7.org/CodeSystem/observation-category","code":"laboratory"}]}],
        "code":{"text":"Glucose"},"subject":{"reference":"Patient/s1"},
        "performer":[{"reference":"Organization/org"}]}))
    .await;
    put(client, base, json!({"resourceType":"Condition","id":"c-other",
        "clinicalStatus":{"coding":[{"system":"http://terminology.hl7.org/CodeSystem/condition-clinical","code":"active"}]},
        "code":{"text":"Other"},"subject":{"reference":"Patient/s2"}}))
    .await;

    let document = get(client, &format!("{base}/fhir/Patient/s1/$summary")).await;
    assert_eq!(document["type"], "document");
    assert!(document["identifier"]["value"].is_string());
    assert!(document["timestamp"].is_string());
    let entries = document["entry"].as_array().unwrap();
    let composition = &entries[0]["resource"];
    assert_eq!(composition["resourceType"], "Composition");
    assert_eq!(composition["type"]["coding"][0]["code"], "60591-5");
    assert_eq!(composition["subject"]["reference"], "Patient/s1");
    assert_eq!(entries[1]["resource"]["resourceType"], "Patient");
    let section = |code: &str| {
        composition["section"]
            .as_array()
            .unwrap()
            .iter()
            .find(|section| section["code"]["coding"][0]["code"] == code)
            .cloned()
    };
    let problems = section("11450-4").expect("problems are required");
    assert_eq!(problems["entry"][0]["reference"], "Condition/c-active");
    assert_eq!(problems["entry"].as_array().unwrap().len(), 1);
    assert!(problems["text"]["div"].as_str().unwrap().contains("Asthma"));
    let allergies = section("48765-2").expect("allergies are required");
    assert_eq!(allergies["emptyReason"]["coding"][0]["code"], "unavailable");
    assert!(section("10160-0").is_some(), "medications are required");
    assert_eq!(
        section("11348-0").unwrap()["entry"][0]["reference"],
        "Condition/c-old"
    );
    assert_eq!(
        section("30954-2").unwrap()["entry"][0]["reference"],
        "Observation/lab"
    );
    assert!(
        section("8716-3").is_none(),
        "an empty optional section stays out"
    );

    let held: Vec<_> = entries
        .iter()
        .map(|entry| {
            format!(
                "{}/{}",
                entry["resource"]["resourceType"].as_str().unwrap(),
                entry["resource"]["id"].as_str().unwrap()
            )
        })
        .collect();
    assert!(
        held.iter().any(|entry| entry == "Organization/org"),
        "{held:?}"
    );
    assert!(
        !held.iter().any(|entry| entry == "Condition/c-other"),
        "another patient's data"
    );

    let by_identifier = get(
        client,
        &format!("{base}/fhir/Patient/$summary?identifier=urn:mrn%7C123"),
    )
    .await;
    assert_eq!(by_identifier["entry"][1]["resource"]["id"], "s1");
    let europe = get(
        client,
        &format!(
            "{base}/fhir/Patient/s1/$summary?profile=\
http://hl7.eu/fhir/eps/StructureDefinition/composition-eu-eps"
        ),
    )
    .await;
    let composition = &europe["entry"][0]["resource"];
    assert_eq!(composition["title"], "Patient Summary");
    assert_eq!(
        composition["meta"]["profile"][0],
        "http://hl7.eu/fhir/eps/StructureDefinition/composition-eu-eps"
    );
    let codes: Vec<_> = composition["section"]
        .as_array()
        .unwrap()
        .iter()
        .map(|section| section["code"]["coding"][0]["code"].as_str().unwrap())
        .collect();
    for required in ["11450-4", "48765-2", "10160-0", "47519-4", "46264-8"] {
        assert!(codes.contains(&required), "{required} missing in {codes:?}");
    }
    let other = client
        .get(format!(
            "{base}/fhir/Patient/s1/$summary?profile=http://example.org/other"
        ))
        .send()
        .await
        .unwrap();
    assert!(other.status().is_client_error(), "{}", other.status());

    let missing = client
        .get(format!("{base}/fhir/Patient/$summary"))
        .send()
        .await
        .unwrap();
    assert_eq!(missing.status(), StatusCode::BAD_REQUEST);
}

async fn explain(client: &Client, base: &str) {
    let response: Value = client
        .post(format!("{base}/fhir/ValueSet/$validate-code"))
        .json(&json!({"resourceType":"Parameters","parameter":[
            {"name":"url","valueUri":"http://hl7.org/fhir/ValueSet/administrative-gender"},
            {"name":"system","valueUri":"http://hl7.org/fhir/administrative-gender"},
            {"name":"code","valueCode":"male"},
            {"name":"_explain","valueBoolean":true}
        ]}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let parameters = response["parameter"].as_array().unwrap();
    assert_eq!(parameters[0]["valueBoolean"], true, "{response}");
    let explain = parameters
        .iter()
        .find(|parameter| parameter["name"] == "explain")
        .expect("explain output");
    let parts = explain["part"].as_array().unwrap();
    let source = parts.iter().find(|part| part["name"] == "source").unwrap();
    assert!(
        source["valueString"]
            .as_str()
            .unwrap()
            .contains("hl7.fhir.r4.core"),
        "{source}"
    );
    assert!(parts.iter().any(|part| part["name"] == "codeSystem"));
}

async fn mcp_call(client: &Client, base: &str, method: &str, params: Value) -> (StatusCode, Value) {
    let response = client
        .post(format!("{base}/mcp"))
        .header("Accept", "application/json, text/event-stream")
        .header("MCP-Protocol-Version", "2026-07-28")
        .header("Mcp-Method", method)
        .json(&json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}))
        .send()
        .await
        .unwrap();
    let status = response.status();
    (status, response.json().await.unwrap_or(Value::Null))
}

async fn tool(client: &Client, base: &str, name: &str, arguments: Value) -> Value {
    let (status, body) = mcp_call(
        client,
        base,
        "tools/call",
        json!({"name":name,"arguments":arguments}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body["result"].clone()
}

async fn mcp(client: &Client, base: &str) {
    let (status, discover) = mcp_call(client, base, "server/discover", json!({})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(discover["result"]["supportedVersions"][0], "2026-07-28");
    assert_eq!(discover["result"]["resultType"], "complete");

    let (_, legacy) = mcp_call(
        client,
        base,
        "initialize",
        json!({"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"1"}}),
    )
    .await;
    assert_eq!(legacy["result"]["protocolVersion"], "2025-06-18");

    let (_, listed) = mcp_call(client, base, "tools/list", json!({})).await;
    let names: Vec<&str> = listed["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"patient_summary"));
    assert!(names.contains(&"write_resource"));
    let mut sorted = names.clone();
    sorted.sort_unstable();
    assert_eq!(names, sorted, "a deterministic order lets clients cache");
    assert!(listed["result"]["ttlMs"].as_u64().is_some());

    let (status, unknown) = mcp_call(client, base, "no/such", json!({})).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(unknown["error"]["code"], -32601);

    let summary = tool(client, base, "patient_summary", json!({"patientId":"s1"})).await;
    assert_eq!(summary["isError"], false);
    assert_eq!(summary["structuredContent"]["type"], "document");

    let found = tool(
        client,
        base,
        "search_resources",
        json!({"resourceType":"Condition","query":"patient=s1"}),
    )
    .await;
    assert_eq!(found["structuredContent"]["resourceType"], "Bundle");

    let missing = tool(
        client,
        base,
        "read_resource",
        json!({"resourceType":"Patient","id":"nobody"}),
    )
    .await;
    assert_eq!(missing["isError"], true);

    let (_, escape) = mcp_call(
        client,
        base,
        "tools/call",
        json!({"name":"read_resource","arguments":{"resourceType":"..","id":"admin"}}),
    )
    .await;
    assert!(escape.get("error").is_some(), "{escape}");

    let written = tool(
        client,
        base,
        "write_resource",
        json!({"resource":{"resourceType":"Observation","id":"ai-note","status":"preliminary",
            "code":{"text":"Suggested"},"subject":{"reference":"Patient/s1"}}}),
    )
    .await;
    assert_eq!(written["isError"], false, "{written}");
    let stored = get(client, &format!("{base}/fhir/Observation/ai-note")).await;
    assert_eq!(stored["meta"]["security"][0]["code"], "AIAST");
    let provenance = get(
        client,
        &format!("{base}/fhir/Provenance?target=Observation/ai-note"),
    )
    .await;
    let record = &provenance["entry"][0]["resource"];
    assert_eq!(record["reason"][0]["coding"][0]["code"], "AIAST");
    let device = record["agent"][0]["who"]["reference"].as_str().unwrap();
    let agent = get(client, &format!("{base}/fhir/{device}")).await;
    assert_eq!(
        agent["type"]["coding"][0]["code"],
        "Artificial-Intelligence"
    );

    // A second write reuses the agent's Device.
    tool(
        client,
        base,
        "write_resource",
        json!({"resource":{"resourceType":"Observation","id":"ai-note-2","status":"preliminary",
            "code":{"text":"Suggested"},"subject":{"reference":"Patient/s1"}}}),
    )
    .await;
    let devices = get(client, &format!("{base}/fhir/Device?_count=10")).await;
    assert_eq!(ids(&devices).len(), 1);
}

async fn data_requirements(client: &Client, base: &str) {
    let measure = get(
        client,
        &format!(
            "{base}/fhir/Measure/screening/$data-requirements?periodStart=2024-01-01&periodEnd=2024-12-31"
        ),
    )
    .await;
    assert_eq!(measure["resourceType"], "Library");
    assert_eq!(measure["type"]["coding"][0]["code"], "module-definition");
    assert_eq!(
        measure["relatedArtifact"][0]["resource"],
        "http://example.org/Library/screening"
    );
    let types: Vec<_> = measure["dataRequirement"]
        .as_array()
        .unwrap()
        .iter()
        .map(|requirement| requirement["type"].as_str().unwrap())
        .collect();
    assert!(types.contains(&"Observation"), "{types:?}");
    assert_eq!(measure["parameter"][0]["name"], "Measurement Period");
    assert_eq!(measure["parameter"][0]["type"], "Period");

    let library = get(
        client,
        &format!("{base}/fhir/Library/screening/$data-requirements"),
    )
    .await;
    assert_eq!(library["dataRequirement"], measure["dataRequirement"]);

    let no_period = client
        .get(format!("{base}/fhir/Measure/screening/$data-requirements"))
        .send()
        .await
        .unwrap();
    assert_eq!(no_period.status(), StatusCode::BAD_REQUEST);
}

/// An Encounter-based measure: length of stay, excluding cancelled ones by a define.
/// Needs the fixtures of the screening measure: Group/mg holds m1 (screened), m2 and m3 (male).
async fn composites_and_supplemental_data(client: &Client, base: &str) {
    let population = |code: &str, name: &str| {
        json!({"code":{"coding":[{"system":"http://terminology.hl7.org/CodeSystem/measure-population","code":code}]},
            "criteria":{"language":"text/cql-identifier","expression":name}})
    };
    let cql = r#"library Contact version '1.0'
using FHIR version '4.0.1'
parameter "Measurement Period" Interval<DateTime>
context Patient
define "Initial Population": true
define "Denominator": true
define "Numerator": Patient.gender = 'female'
"#;
    put(
        client,
        base,
        json!({"resourceType":"Library","id":"contact","status":"active",
        "url":"http://example.org/Library/contact","version":"1.0",
        "type":{"coding":[{"code":"logic-library"}]},
        "content":[{"contentType":"text/cql",
            "data":base64::engine::general_purpose::STANDARD.encode(cql)}]}),
    )
    .await;
    put(
        client,
        base,
        json!({"resourceType":"Measure","id":"contact","status":"active",
        "url":"http://example.org/Measure/contact","version":"1.0",
        "library":["http://example.org/Library/contact"],
        "scoring":{"coding":[{"code":"proportion"}]},
        "group":[{"population":[
            population("initial-population","Initial Population"),
            population("denominator","Denominator"),
            population("numerator","Numerator")]}]}),
    )
    .await;
    let evaluate = |id: String, subject: &'static str| {
        format!(
            "{base}/fhir/Measure/{id}/$evaluate-measure?periodStart=2024-01-01&periodEnd=2024-12-31&subject={subject}"
        )
    };

    // Screening: m1 of m1 and m2. Contact: m1 and m2 of all three.
    for (method, expected) in [
        ("all-or-nothing", 1.0 / 3.0),
        ("opportunity", 0.6),
        ("linear", 0.5),
        ("weighted", (0.5 * 3.0 + 2.0 / 3.0) / 4.0),
    ] {
        put(client, base, json!({"resourceType":"Measure","id":format!("care-{method}"),
            "status":"active","url":format!("http://example.org/Measure/care-{method}"),
            "scoring":{"coding":[{"code":"composite"}]},
            "compositeScoring":{"coding":[{"code":method}]},
            "relatedArtifact":[
                {"type":"composed-of","resource":"http://example.org/Measure/screening",
                 "extension":[{"url":"http://hl7.org/fhir/us/cqfmeasures/StructureDefinition/cqfm-weight","valueDecimal":3}]},
                {"type":"composed-of","resource":"http://example.org/Measure/contact|1.0"}]}))
        .await;
        let report = get(client, &evaluate(format!("care-{method}"), "Group/mg")).await;
        assert_eq!(report["resourceType"], "MeasureReport", "{report}");
        let score = report["group"][0]["measureScore"]["value"].as_f64();
        assert!(
            score.is_some_and(|score| (score - expected).abs() < 1e-9),
            "{method}: {report}"
        );
    }

    put(
        client,
        base,
        json!({"resourceType":"Measure","id":"contact-sde","status":"active",
        "url":"http://example.org/Measure/contact-sde",
        "library":["http://example.org/Library/screening"],
        "scoring":{"coding":[{"code":"proportion"}]},
        "supplementalData":[{"code":{"text":"Sex"},
            "criteria":{"language":"text/cql-identifier","expression":"Gender"}}],
        "group":[{"population":[
            population("initial-population","Initial Population"),
            population("denominator","Denominator"),
            population("numerator","Numerator")]}]}),
    )
    .await;
    let summary = get(client, &evaluate("contact-sde".to_owned(), "Group/mg")).await;
    assert_eq!(
        summary["extension"][0]["url"],
        "http://hl7.org/fhir/5.0/StructureDefinition/extension-MeasureReport.supplementalData"
    );
    let reference = summary["extension"][0]["valueReference"]["reference"]
        .as_str()
        .unwrap();
    let observation = summary["contained"]
        .as_array()
        .unwrap()
        .iter()
        .find(|resource| format!("#{}", resource["id"].as_str().unwrap()) == reference)
        .unwrap();
    assert_eq!(observation["code"]["text"], "Sex");
    // m3 is not in the initial population.
    assert_eq!(
        observation["component"],
        json!([{"code":{"text":"female"},"valueInteger":2}])
    );

    let individual = get(client, &evaluate("contact-sde".to_owned(), "Patient/m1")).await;
    assert_eq!(individual["contained"][0]["valueString"], "female");
}

async fn fhirpath_debug_trace(client: &Client, base: &str) {
    let run = |expression: &str| {
        let body = json!({"resourceType":"Parameters","parameter":[
            {"name":"expression","valueString":expression},
            {"name":"debug","valueBoolean":true},
            {"name":"resource","resource":{"resourceType":"Patient","gender":"female"}}]});
        let request = client.post(format!("{base}/fhir/$fhirpath")).json(&body);
        async move { request.send().await.unwrap().json::<Value>().await.unwrap() }
    };
    let named = |response: &Value, name: &str| -> Vec<Value> {
        response["parameter"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|parameter| parameter["name"] == name)
            .cloned()
            .collect()
    };

    let done = run("gender.trace('g')").await;
    assert_eq!(named(&done, "traceStatus")[0]["valueCode"], "complete");
    let events = named(&done, "trace");
    assert!(events.len() >= 3, "{done}");
    assert!(done.to_string().contains("\"phase\""));
    assert_eq!(named(&done, "metadata").len(), 1);

    // A failure answers with what happened up to it.
    let failed = run("gender.nosuchfunction()").await;
    assert_eq!(failed["resourceType"], "Parameters", "{failed}");
    assert_eq!(named(&failed, "traceStatus")[0]["valueCode"], "stopped");
    assert_eq!(named(&failed, "outcome").len(), 1);
}

async fn continuous_variable(client: &Client, base: &str) {
    let cql = r#"library Stays version '1.0'
using FHIR version '4.0.1'
parameter "Measurement Period" Interval<DateTime>
context Patient
define "Initial Population": [Encounter] E where E.period.start during "Measurement Period"
define "Measure Population": "Initial Population" E where E.status = 'finished'
define "Measure Population Exclusion": "Measure Population" E where E.class.code = 'EMER'
define function "Stay"(E Encounter): difference in minutes between E.period.start and E.period.end
"#;
    put(
        client,
        base,
        json!({"resourceType":"Library","id":"stays","status":"active",
        "url":"http://example.org/Library/stays","version":"1.0",
        "type":{"coding":[{"code":"logic-library"}]},
        "content":[{"contentType":"text/cql",
            "data":base64::engine::general_purpose::STANDARD.encode(cql)}]}),
    )
    .await;
    let population = |code: &str, name: &str| {
        json!({"code":{"coding":[{"system":"http://terminology.hl7.org/CodeSystem/measure-population","code":code}]},
            "criteria":{"language":"text/cql-identifier","expression":name}})
    };
    let mut observation = population("measure-observation", "Stay");
    observation["extension"] = json!([{
        "url":"http://hl7.org/fhir/us/cqfmeasures/StructureDefinition/cqfm-aggregateMethod",
        "valueCode":"median"}]);
    put(client, base, json!({"resourceType":"Measure","id":"stays","status":"active",
        "url":"http://example.org/Measure/stays",
        "extension":[{"url":"http://hl7.org/fhir/us/cqfmeasures/StructureDefinition/cqfm-populationBasis",
            "valueCode":"Encounter"}],
        "library":["http://example.org/Library/stays"],
        "scoring":{"coding":[{"system":"http://terminology.hl7.org/CodeSystem/measure-scoring",
            "code":"continuous-variable"}]},
        "group":[{"population":[
            population("initial-population","Initial Population"),
            population("measure-population","Measure Population"),
            population("measure-population-exclusion","Measure Population Exclusion"),
            observation]}]}))
    .await;
    for (id, status, class, hours) in [
        ("st1", "finished", "AMB", 1),
        ("st2", "finished", "AMB", 3),
        ("st3", "finished", "EMER", 9),
        ("st4", "cancelled", "AMB", 5),
    ] {
        put(
            client,
            base,
            json!({"resourceType":"Encounter","id":id,"status":status,
            "class":{"system":"http://terminology.hl7.org/CodeSystem/v3-ActCode","code":class},
            "subject":{"reference":"Patient/m1"},
            "period":{"start":"2024-05-01T08:00:00Z",
                "end":format!("2024-05-01T{:02}:00:00Z", 8 + hours)}}),
        )
        .await;
    }
    let report = get(
        client,
        &format!(
            "{base}/fhir/Measure/stays/$evaluate-measure?periodStart=2024-01-01&periodEnd=2024-12-31&subject=Patient/m1"
        ),
    )
    .await;
    let group = &report["group"][0];
    let count = |code: &str| {
        group["population"]
            .as_array()
            .unwrap()
            .iter()
            .find(|population| population["code"]["coding"][0]["code"] == code)
            .unwrap_or_else(|| panic!("{code} missing in {group}"))["count"]
            .as_u64()
            .unwrap()
    };
    assert_eq!(count("initial-population"), 4, "{group}");
    assert_eq!(count("measure-population"), 3, "{group}");
    assert_eq!(count("measure-population-exclusion"), 1, "{group}");
    assert_eq!(count("measure-observation"), 2, "{group}");
    assert_eq!(group["measureScore"]["value"], 120.0, "{group}");

    // A ratio of two observations: minutes of finished stays over minutes of all stays.
    let observed = |id: &str, target: &str| {
        let mut observation = population("measure-observation", "Stay");
        observation["id"] = json!(id);
        observation["extension"] = json!([
            {"url":"http://hl7.org/fhir/us/cqfmeasures/StructureDefinition/cqfm-aggregateMethod",
             "valueCode":"sum"},
            {"url":"http://hl7.org/fhir/us/cqfmeasures/StructureDefinition/cqfm-criteriaReference",
             "valueString":target}]);
        observation
    };
    let mut denominator = population("denominator", "Initial Population");
    denominator["id"] = json!("all");
    let mut numerator = population("numerator", "Measure Population");
    numerator["id"] = json!("finished");
    put(client, base, json!({"resourceType":"Measure","id":"stay-ratio","status":"active",
        "url":"http://example.org/Measure/stay-ratio",
        "extension":[{"url":"http://hl7.org/fhir/us/cqfmeasures/StructureDefinition/cqfm-populationBasis",
            "valueCode":"Encounter"}],
        "library":["http://example.org/Library/stays"],
        "scoring":{"coding":[{"system":"http://terminology.hl7.org/CodeSystem/measure-scoring",
            "code":"ratio"}]},
        "group":[{"population":[
            population("initial-population","Initial Population"),
            denominator,
            numerator,
            population("numerator-exclusion","Measure Population Exclusion"),
            observed("over-finished", "finished"),
            observed("over-all", "all")]}]}))
    .await;
    let report = get(
        client,
        &format!(
            "{base}/fhir/Measure/stay-ratio/$evaluate-measure?periodStart=2024-01-01&periodEnd=2024-12-31&subject=Patient/m1"
        ),
    )
    .await;
    let group = &report["group"][0];
    let score = group["measureScore"]["value"].as_f64().unwrap();
    assert!((score - 240.0 / 1080.0).abs() < 1e-9, "{group}");
}

async fn measure(client: &Client, base: &str) {
    let cql = r#"library Screening version '1.0'
using FHIR version '4.0.1'
parameter "Measurement Period" Interval<DateTime>
context Patient
define "Initial Population": Patient.gender = 'female'
define "Denominator": "Initial Population"
define "Screenings": [Observation] O where O.status = 'final' and O.effective during "Measurement Period"
define "Numerator": exists "Screenings"
define "Gender": Patient.gender
"#;
    put(
        client,
        base,
        json!({"resourceType":"Library","id":"screening","status":"active",
        "url":"http://example.org/Library/screening","version":"1.0",
        "type":{"coding":[{"code":"logic-library"}]},
        "content":[{"contentType":"text/cql",
            "data":base64::engine::general_purpose::STANDARD.encode(cql)}]}),
    )
    .await;
    let population = |code: &str, name: &str| {
        json!({"code":{"coding":[{"system":"http://terminology.hl7.org/CodeSystem/measure-population","code":code}]},
            "criteria":{"language":"text/cql-identifier","expression":name}})
    };
    put(client, base, json!({"resourceType":"Measure","id":"screening","status":"active",
        "url":"http://example.org/Measure/screening",
        "library":["http://example.org/Library/screening"],
        "scoring":{"coding":[{"system":"http://terminology.hl7.org/CodeSystem/measure-scoring","code":"proportion"}]},
        "group":[{"population":[
            population("initial-population","Initial Population"),
            population("denominator","Denominator"),
            population("numerator","Numerator")],
            "stratifier":[{"criteria":{"language":"text/cql-identifier","expression":"Gender"}}]}]}))
    .await;
    for (id, gender) in [("m1", "female"), ("m2", "female"), ("m3", "male")] {
        put(
            client,
            base,
            json!({"resourceType":"Patient","id":id,"gender":gender}),
        )
        .await;
    }
    for (id, patient, date) in [("mo1", "m1", "2024-03-01"), ("mo2", "m2", "2020-03-01")] {
        put(
            client,
            base,
            json!({"resourceType":"Observation","id":id,"status":"final",
            "code":{"text":"Screening"},"subject":{"reference":format!("Patient/{patient}")},
            "effectiveDateTime":date}),
        )
        .await;
    }
    put(
        client,
        base,
        json!({"resourceType":"Group","id":"mg","type":"person","actual":true,
        "member":[{"entity":{"reference":"Patient/m1"}},{"entity":{"reference":"Patient/m2"}},
                  {"entity":{"reference":"Patient/m3"}}]}),
    )
    .await;
    continuous_variable(client, base).await;
    data_requirements(client, base).await;
    composites_and_supplemental_data(client, base).await;
    fhirpath_debug_trace(client, base).await;
    let url = |extra: &str| {
        format!(
            "{base}/fhir/Measure/screening/$evaluate-measure?periodStart=2024-01-01&periodEnd=2024-12-31{extra}"
        )
    };
    let counts = |group: &Value| -> Vec<(String, u64)> {
        group["population"]
            .as_array()
            .unwrap()
            .iter()
            .map(|population| {
                (
                    population["code"]["coding"][0]["code"]
                        .as_str()
                        .unwrap()
                        .to_owned(),
                    population["count"].as_u64().unwrap(),
                )
            })
            .collect()
    };

    let individual = get(client, &url("&subject=Patient/m1")).await;
    assert_eq!(individual["resourceType"], "MeasureReport");
    assert_eq!(individual["type"], "individual");
    assert_eq!(individual["subject"]["reference"], "Patient/m1");
    assert_eq!(
        individual["measure"],
        "http://example.org/Measure/screening"
    );
    assert_eq!(individual["group"][0]["measureScore"]["value"], 1.0);

    let summary = get(client, &url("&subject=Group/mg")).await;
    assert_eq!(summary["type"], "summary");
    let group = &summary["group"][0];
    let found = counts(group);
    assert!(
        found.contains(&("initial-population".to_owned(), 2)),
        "{found:?}"
    );
    assert!(found.contains(&("denominator".to_owned(), 2)), "{found:?}");
    assert!(found.contains(&("numerator".to_owned(), 1)), "{found:?}");
    assert_eq!(group["measureScore"]["value"], 0.5);
    let strata = group["stratifier"][0]["stratum"].as_array().unwrap();
    assert_eq!(strata.len(), 2, "{strata:?}");

    let listed = get(client, &url("&subject=Group/mg&reportType=subject-list")).await;
    assert_eq!(listed["type"], "subject-list");
    let list = listed["contained"]
        .as_array()
        .unwrap()
        .iter()
        .find(|list| list["id"] == "group-0-numerator")
        .unwrap();
    assert_eq!(list["entry"][0]["item"]["reference"], "Patient/m1");

    let by_url = get(
        client,
        &format!("{base}/fhir/Measure/$evaluate-measure?measure=http://example.org/Measure/screening&periodStart=2024&periodEnd=2024&subject=Patient/m2"),
    )
    .await;
    assert!(
        by_url["group"][0]["measureScore"]["value"].as_f64() == Some(0.0),
        "{by_url}"
    );

    let scored = tool(
        client,
        base,
        "evaluate_measure",
        json!({"measureId":"screening","periodStart":"2024-01-01","periodEnd":"2024-12-31",
            "subject":"Patient/m1"}),
    )
    .await;
    assert_eq!(
        scored["structuredContent"]["type"], "individual",
        "{scored}"
    );

    let response = client
        .get(format!("{base}/fhir/Measure/screening/$evaluate-measure"))
        .send()
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        StatusCode::BAD_REQUEST,
        "the period is required"
    );
}

/// `compose.lockedDate` picks the version of a code system stored in two.
async fn locked_value_set(client: &Client, base: &str) {
    let url = "http://example.org/CodeSystem/locked";
    for (id, version, date, code) in [
        ("locked-1", "1", "2020-01-01", "old"),
        ("locked-2", "2", "2024-06-01", "new"),
    ] {
        put(
            client,
            base,
            json!({"resourceType":"CodeSystem","id":id,"url":url,"version":version,"date":date,
                "status":"active","content":"complete","concept":[{"code":code}]}),
        )
        .await;
    }
    let codes = |expansion: &Value| -> Vec<String> {
        expansion["expansion"]["contains"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["code"].as_str().unwrap().to_owned())
            .collect()
    };
    for (id, locked, expected) in [
        ("locked-then", json!("2021-01-01"), "old"),
        ("locked-now", json!("2025-01-01"), "new"),
    ] {
        put(
            client,
            base,
            json!({"resourceType":"ValueSet","id":id,"status":"active",
                "url":format!("http://example.org/ValueSet/{id}"),
                "compose":{"lockedDate":locked,"include":[{"system":url}]}}),
        )
        .await;
        let expansion = get(client, &format!("{base}/fhir/ValueSet/{id}/$expand")).await;
        assert_eq!(codes(&expansion), [expected], "{expansion}");
    }
}

/// Every JSON answer of the API is the FHIR media type, unless plain JSON was asked for.
async fn media_types(client: &Client, base: &str) {
    let versions = get(client, &format!("{base}/fhir/$versions")).await;
    assert_eq!(versions["parameter"][0]["valueCode"], "4.0", "{versions}");
    assert_eq!(versions["parameter"][1]["name"], "default", "{versions}");

    for path in ["metadata", "Patient?_count=1", "Patient/no-such-patient"] {
        let answer = client
            .get(format!("{base}/fhir/{path}"))
            .header("Accept", "application/fhir+json")
            .send()
            .await
            .unwrap();
        let content_type = answer.headers()["content-type"].to_str().unwrap();
        assert!(
            content_type.starts_with("application/fhir+json"),
            "{path}: {content_type}"
        );
    }
    let plain = client
        .get(format!("{base}/fhir/Patient?_count=1"))
        .header("Accept", "application/json")
        .send()
        .await
        .unwrap();
    assert_eq!(plain.headers()["content-type"], "application/json");
}

/// A document Bundle is found by what is inside it. Before R6 these parameters are
/// `reference` typed, and they chain into the entry all the same.
async fn embedded_chains(client: &Client, base: &str) {
    put(
        client,
        base,
        json!({"resourceType": "Patient", "id": "bundled", "active": true}),
    )
    .await;
    put(
        client,
        base,
        json!({"resourceType": "Bundle", "id": "document", "type": "document",
            "identifier": {"system": "urn:example", "value": "document"},
            "timestamp": "2026-01-01T00:00:00Z",
            "entry": [{"fullUrl": "urn:uuid:7a4b9f3e-2a31-4f1c-9e68-0a3c2c1d0001", "resource": {
                "resourceType": "Composition", "id": "inside", "status": "final",
                "type": {"text": "Discharge"}, "subject": {"reference": "Patient/bundled"},
                "date": "2026-01-01", "author": [{"display": "A"}], "title": "Discharge"}}]}),
    )
    .await;
    for (query, total) in [
        ("composition._id=inside", 1),
        ("composition._id=elsewhere", 0),
        ("composition.subject=Patient/bundled", 1),
        ("composition.subject=Patient/other", 0),
        ("composition.subject:Patient.active=true", 1),
    ] {
        let bundle = get(client, &format!("{base}/fhir/Bundle?{query}")).await;
        assert_eq!(bundle["total"], total, "{query}");
    }
}

async fn delete(client: &Client, url: &str) -> StatusCode {
    client.delete(url).send().await.unwrap().status()
}

async fn versions(client: &Client, url: &str) -> Vec<String> {
    get(client, url).await["entry"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| {
            entry["resource"]["meta"]["versionId"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect()
}

/// `delete-history` and `delete-history-version` keep the current version, both for a
/// type with a compartment and without one.
async fn delete_history(client: &Client, base: &str) {
    for (resource_type, body) in [
        ("Patient", json!({"resourceType": "Patient", "id": "aged"})),
        (
            "Basic",
            json!({"resourceType": "Basic", "id": "aged", "code": {"text": "a"}}),
        ),
    ] {
        let url = format!("{base}/fhir/{resource_type}/aged");
        for language in ["en", "de", "fr", "es"] {
            let mut body = body.clone();
            body["language"] = json!(language);
            put(client, base, body).await;
        }
        let history = format!("{url}/_history");
        let all = versions(client, &history).await;
        assert_eq!(all.len(), 4, "{resource_type}");

        assert_eq!(
            delete(client, &format!("{history}/{}", all[0])).await,
            StatusCode::CONFLICT
        );
        assert_eq!(
            delete(client, &format!("{history}/{}", all[1])).await,
            StatusCode::NO_CONTENT
        );
        assert_eq!(
            delete(client, &format!("{history}/{}", all[1])).await,
            StatusCode::NOT_FOUND
        );
        assert_eq!(versions(client, &history).await.len(), 3);
        assert_eq!(delete(client, &history).await, StatusCode::NO_CONTENT);
        assert_eq!(versions(client, &history).await, all[..1]);
        assert_eq!(get(client, &url).await["language"], "es");
    }
    // A deleted resource keeps the record of its deletion.
    let url = format!("{base}/fhir/Basic/aged");
    assert!(delete(client, &url).await.is_success());
    let before = client
        .get(format!("{url}/_history"))
        .send()
        .await
        .unwrap()
        .json::<Value>()
        .await
        .unwrap();
    assert_eq!(before["entry"].as_array().unwrap().len(), 2);
    assert_eq!(
        delete(client, &format!("{url}/_history")).await,
        StatusCode::NO_CONTENT
    );
    let after = client
        .get(format!("{url}/_history"))
        .send()
        .await
        .unwrap()
        .json::<Value>()
        .await
        .unwrap();
    assert_eq!(after["entry"].as_array().unwrap().len(), 1);
    assert_eq!(after["entry"][0]["request"]["method"], "DELETE");
    assert_eq!(
        delete(client, &format!("{base}/fhir/Patient/aged/Observation")).await,
        StatusCode::METHOD_NOT_ALLOWED
    );
}

#[tokio::test]
async fn paging_summary_and_agents() {
    let postgres = Postgres::default()
        .with_tag("18-alpine")
        .start()
        .await
        .expect("postgres");
    let port = postgres.get_host_port_ipv4(5432).await.unwrap();
    let (base, _server) = start_server(&format!(
        "postgres://postgres:postgres@127.0.0.1:{port}/postgres"
    ))
    .await;
    let client = Client::new();
    keyset_paging(&client, &base).await;
    long_post_search(&client, &base).await;
    summary(&client, &base).await;
    explain(&client, &base).await;
    mcp(&client, &base).await;
    measure(&client, &base).await;
    media_types(&client, &base).await;
    locked_value_set(&client, &base).await;
    embedded_chains(&client, &base).await;
    delete_history(&client, &base).await;
}
