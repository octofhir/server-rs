use octofhir_db_postgres::{PostgresStorage, SchemaManager, migrations};
use octofhir_search::{
    BuiltQuery, ElementTypeHint, SearchParameter, SearchParameterRegistry, SearchParameterType,
    SqlValue, build_native_ir_query_from_params, parameters::SearchParameterComponent,
    parse_query_string, register_common_parameters,
};
use octofhir_storage::FhirStorage;
use serde_json::{Value, json};
use sqlx_core::query_as::query_as;
use sqlx_postgres::{PgPool, PgPoolOptions};
use std::collections::hash_map::DefaultHasher;
use std::env;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::Instant;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::postgres::Postgres;

#[tokio::test]
#[ignore = "manual live-DB EXPLAIN smoke test"]
async fn representative_search_explain_json_runs_with_bound_params() {
    let container = Postgres::default()
        .start()
        .await
        .expect("Failed to start PostgreSQL container");

    let port = container
        .get_host_port_ipv4(5432)
        .await
        .expect("Failed to get port");
    let db_url = format!("postgres://postgres:postgres@localhost:{port}/postgres");

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&db_url)
        .await
        .expect("Failed to connect to database");

    migrations::run(&pool, &db_url)
        .await
        .expect("Migrations should succeed");
    SchemaManager::ensure_archive_function(&pool)
        .await
        .expect("archive function should be created");
    let schema = SchemaManager::new(pool.clone());
    schema
        .create_resource_schema("Patient")
        .await
        .expect("Patient schema should be created");
    schema
        .create_resource_schema("Observation")
        .await
        .expect("Observation schema should be created");

    let registry = Arc::new(representative_registry());
    let storage = PostgresStorage::from_pool(pool.clone());
    assert!(
        storage.search_registry_slot().set(registry.clone()).is_ok(),
        "registry should only be set once"
    );
    seed_representative_data(&storage).await;
    let synthetic_rows = synthetic_row_count();
    let explain_analyze = explain_analyze_enabled();
    if synthetic_rows > 0 {
        let started = Instant::now();
        seed_synthetic_data(&storage, synthetic_rows).await;
        println!(
            "seed_synthetic_data | patients={} | observations={} | elapsed_ms={:.3}",
            synthetic_rows,
            synthetic_rows,
            started.elapsed().as_secs_f64() * 1000.0
        );
    }
    println!(
        "search_explain_config | synthetic_rows={} | explain_analyze={}",
        synthetic_rows, explain_analyze
    );

    for case in representative_queries() {
        let build_started = Instant::now();
        let params = parse_query_string(case.query, 10, 100);
        let query =
            build_native_ir_query_from_params(case.resource_type, &params, &registry, "public")
                .unwrap_or_else(|error| panic!("{} query should build: {error}", case.label))
                .builder
                .with_raw_resource(true)
                .build()
                .unwrap_or_else(|error| panic!("{} SQL should render: {error}", case.label));
        let build_elapsed = build_started.elapsed();
        let sql_shape = redact_sql_shape(&query.sql);
        assert_redacted_shape(case.label, &sql_shape);
        assert_expected_shape(case.label, &sql_shape, case.expected_sql_fragments);

        let started = Instant::now();
        let explain = octofhir_db_postgres::queries::search::explain_built_search_query_json(
            &pool,
            &query,
            explain_analyze,
        )
        .await
        .unwrap_or_else(|error| panic!("{} EXPLAIN should run: {error}", case.label));
        let elapsed = started.elapsed();

        let execute_started = Instant::now();
        let row_count = execute_built_query_row_count(&pool, &query)
            .await
            .unwrap_or_else(|error| panic!("{} query should execute: {error}", case.label));
        let execute_elapsed = execute_started.elapsed();

        assert!(
            explain.is_array(),
            "{} EXPLAIN JSON should be an array: {explain}",
            case.label
        );
        assert!(
            explain[0].get("Plan").is_some(),
            "{} EXPLAIN JSON should contain a plan: {explain}",
            case.label
        );

        let mut node_types = Vec::new();
        collect_node_types(&explain[0]["Plan"], &mut node_types);
        let planning_ms = explain[0].get("Planning Time").and_then(Value::as_f64);
        let analyze_execution_ms = explain[0].get("Execution Time").and_then(Value::as_f64);
        let analyze_actual_rows = explain[0]["Plan"]
            .get("Actual Rows")
            .and_then(Value::as_f64);
        println!(
            "{} | resource={} | build_ms={:.3} | explain_ms={:.3} | execute_ms={:.3} | rows={} | analyze_planning_ms={} | analyze_execution_ms={} | analyze_actual_rows={} | params={} | sql_shape_hash={:016x} | nodes={:?}\nsql_shape={}",
            case.label,
            case.resource_type,
            build_elapsed.as_secs_f64() * 1000.0,
            elapsed.as_secs_f64() * 1000.0,
            execute_elapsed.as_secs_f64() * 1000.0,
            row_count,
            format_optional_f64(planning_ms),
            format_optional_f64(analyze_execution_ms),
            format_optional_f64(analyze_actual_rows),
            query.params.len(),
            stable_hash(&sql_shape),
            node_types,
            sql_shape
        );
    }
}

async fn execute_built_query_row_count(
    pool: &PgPool,
    query: &BuiltQuery,
) -> Result<usize, sqlx_core::Error> {
    let mut sqlx_query = query_as::<
        _,
        (
            String,
            String,
            i64,
            chrono::DateTime<chrono::Utc>,
            chrono::DateTime<chrono::Utc>,
        ),
    >(&query.sql);
    for param in &query.params {
        sqlx_query = match param {
            SqlValue::Text(value) => sqlx_query.bind(value.as_str()),
            SqlValue::Integer(value) => sqlx_query.bind(*value),
            SqlValue::Float(value) => sqlx_query.bind(*value),
            SqlValue::Boolean(value) => sqlx_query.bind(*value),
            SqlValue::Json(value) => sqlx_query.bind(value.as_str()),
            SqlValue::Timestamp(value) => sqlx_query.bind(value.as_str()),
            SqlValue::Null => sqlx_query.bind(None::<String>),
        };
    }

    sqlx_query.fetch_all(pool).await.map(|rows| rows.len())
}

fn synthetic_row_count() -> usize {
    env::var("OCTOFHIR_SEARCH_EXPLAIN_SYNTHETIC_ROWS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0)
}

fn explain_analyze_enabled() -> bool {
    env::var("OCTOFHIR_SEARCH_EXPLAIN_ANALYZE")
        .ok()
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

fn format_optional_f64(value: Option<f64>) -> String {
    value
        .map(|value| format!("{value:.3}"))
        .unwrap_or_else(|| "n/a".to_string())
}

async fn seed_representative_data(storage: &PostgresStorage) {
    for resource in [
        json!({
            "resourceType": "Patient",
            "id": "explain-patient",
            "birthDate": "1985-03-12",
            "gender": "female",
            "name": [{"family": "Smith", "given": ["Alex"]}],
            "identifier": [{"system": "http://hospital.example/mrn", "value": "12345"}]
        }),
        json!({
            "resourceType": "Patient",
            "id": "other-patient",
            "birthDate": "1970-06-01",
            "gender": "male",
            "name": [{"family": "Jones"}],
            "identifier": [{"system": "http://hospital.example/mrn", "value": "99999"}]
        }),
        json!({
            "resourceType": "Observation",
            "id": "explain-observation",
            "status": "final",
            "code": {
                "coding": [{
                    "system": "http://loinc.org",
                    "code": "8480-6",
                    "display": "Systolic blood pressure"
                }]
            },
            "subject": {"reference": "Patient/explain-patient"},
            "valueQuantity": {
                "value": 120,
                "system": "http://unitsofmeasure.org",
                "code": "mm[Hg]",
                "unit": "mm[Hg]"
            },
            "component": [{
                "code": {
                    "coding": [{
                        "system": "http://loinc.org",
                        "code": "8480-6"
                    }]
                },
                "valueQuantity": {
                    "value": 120,
                    "system": "http://unitsofmeasure.org",
                    "code": "mm[Hg]",
                    "unit": "mm[Hg]"
                }
            }]
        }),
    ] {
        storage
            .create(&resource)
            .await
            .unwrap_or_else(|error| panic!("seed {} should be created: {error}", resource));
    }
}

async fn seed_synthetic_data(storage: &PostgresStorage, rows: usize) {
    for index in 0..rows {
        let family = if index % 10 == 0 { "Smith" } else { "Jones" };
        let gender = if index % 2 == 0 { "female" } else { "male" };
        let birth_year = 1970 + (index % 40);
        let patient_id = format!("synthetic-patient-{index:06}");
        let identifier_value = format!("synthetic-mrn-{index:06}");
        let patient = json!({
            "resourceType": "Patient",
            "id": patient_id,
            "birthDate": format!("{birth_year:04}-03-12"),
            "gender": gender,
            "name": [{"family": family, "given": ["Synthetic"]}],
            "identifier": [{"system": "http://hospital.example/mrn", "value": identifier_value}]
        });

        storage
            .create(&patient)
            .await
            .unwrap_or_else(|error| panic!("synthetic patient {index} should be created: {error}"));

        let matches_observation = index % 10 == 0;
        let code = if matches_observation {
            "8480-6"
        } else {
            "8310-5"
        };
        let quantity_value = if matches_observation { 120 } else { 80 };
        let observation = json!({
            "resourceType": "Observation",
            "id": format!("synthetic-observation-{index:06}"),
            "status": "final",
            "code": {
                "coding": [{
                    "system": "http://loinc.org",
                    "code": code,
                    "display": "Synthetic observation"
                }]
            },
            "subject": {"reference": format!("Patient/synthetic-patient-{index:06}")},
            "valueQuantity": {
                "value": quantity_value,
                "system": "http://unitsofmeasure.org",
                "code": "mm[Hg]",
                "unit": "mm[Hg]"
            },
            "component": [{
                "code": {
                    "coding": [{
                        "system": "http://loinc.org",
                        "code": code
                    }]
                },
                "valueQuantity": {
                    "value": quantity_value,
                    "system": "http://unitsofmeasure.org",
                    "code": "mm[Hg]",
                    "unit": "mm[Hg]"
                }
            }]
        });

        storage.create(&observation).await.unwrap_or_else(|error| {
            panic!("synthetic observation {index} should be created: {error}")
        });
    }
}

struct SearchExplainCase {
    label: &'static str,
    resource_type: &'static str,
    query: &'static str,
    expected_sql_fragments: &'static [&'static str],
}

fn representative_queries() -> Vec<SearchExplainCase> {
    vec![
        SearchExplainCase {
            label: "patient_id",
            resource_type: "Patient",
            query: "_id=explain-patient&_count=1",
            expected_sql_fragments: &["r.id = $N"],
        },
        SearchExplainCase {
            label: "patient_last_updated_window",
            resource_type: "Patient",
            query: "_lastUpdated=ge2024-01-01&_lastUpdated=le2024-12-31&_count=10",
            expected_sql_fragments: &[
                "r.updated_at >= $N::timestamptz",
                "r.updated_at < $N::timestamptz",
            ],
        },
        SearchExplainCase {
            label: "patient_birthdate_window",
            resource_type: "Patient",
            query: "birthdate=ge1980-01-01&birthdate=le2000-12-31&_count=10",
            expected_sql_fragments: &["fhir_extract_date_min", "tstzrange"],
        },
        SearchExplainCase {
            label: "patient_family_prefix",
            resource_type: "Patient",
            query: "family=Smith&_count=10",
            expected_sql_fragments: &["fhir_text_blob(fhir_extract_text", "LIKE"],
        },
        SearchExplainCase {
            label: "patient_family_exact",
            resource_type: "Patient",
            query: "family:exact=Smith&_count=10",
            expected_sql_fragments: &["ANY(fhir_extract_text"],
        },
        SearchExplainCase {
            label: "patient_identifier",
            resource_type: "Patient",
            query: "identifier=http://hospital.example/mrn|12345&_count=10",
            expected_sql_fragments: &["r.resource @> $N::jsonb"],
        },
        SearchExplainCase {
            label: "patient_gender",
            resource_type: "Patient",
            query: "gender=female&_count=10",
            expected_sql_fragments: &["r.resource @> $N::jsonb"],
        },
        SearchExplainCase {
            label: "patient_gender_system_only",
            resource_type: "Patient",
            query: "gender=http://example.org|&_count=10",
            expected_sql_fragments: &["WHERE FALSE"],
        },
        SearchExplainCase {
            label: "observation_code",
            resource_type: "Observation",
            query: "code=http://loinc.org|8480-6&_count=10",
            expected_sql_fragments: &["r.resource @> $N::jsonb"],
        },
        SearchExplainCase {
            label: "observation_subject",
            resource_type: "Observation",
            query: "subject=Patient/explain-patient&_count=10",
            expected_sql_fragments: &["jsonb_array_elements", "ref->>'reference'"],
        },
        SearchExplainCase {
            label: "observation_subject_patient_family",
            resource_type: "Observation",
            query: "subject:Patient.family=Smith&_count=10",
            expected_sql_fragments: &[
                "\"patient\" chain0",
                "ref->>'reference'",
                "fhir_text_blob(fhir_extract_text",
            ],
        },
        SearchExplainCase {
            label: "patient_has_observation_code",
            resource_type: "Patient",
            query: "_has:Observation:subject:code=http://loinc.org|8480-6&_count=10",
            expected_sql_fragments: &[
                "\"observation\" has0",
                "ref->>'reference'",
                "has0.resource @> $N::jsonb",
            ],
        },
        SearchExplainCase {
            label: "observation_quantity",
            resource_type: "Observation",
            query: "value-quantity=ge100|http://unitsofmeasure.org|mm[Hg]&_count=10",
            expected_sql_fragments: &[
                "(r.resource->'valueQuantity'->>'value')::numeric >= $N::numeric",
                "r.resource @> $N::jsonb",
            ],
        },
        SearchExplainCase {
            label: "observation_composite_code_quantity",
            resource_type: "Observation",
            query: "code-value-quantity=http://loinc.org|8480-6$ge100|http://unitsofmeasure.org|mm[Hg]&_count=10",
            expected_sql_fragments: &[
                "jsonb_array_elements",
                "AS component_elem",
                "component_elem->'valueQuantity'->>'value'",
            ],
        },
    ]
}

fn representative_registry() -> SearchParameterRegistry {
    let registry = SearchParameterRegistry::new();
    register_common_parameters(&registry);

    registry.register(
        SearchParameter::new(
            "birthdate",
            "http://hl7.org/fhir/SearchParameter/Patient-birthdate",
            SearchParameterType::Date,
            vec!["Patient".to_string()],
        )
        .with_expression("Patient.birthDate"),
    );
    registry.register(
        SearchParameter::new(
            "family",
            "http://hl7.org/fhir/SearchParameter/Patient-family",
            SearchParameterType::String,
            vec!["Patient".to_string()],
        )
        .with_expression("Patient.name.family"),
    );
    registry.register(
        SearchParameter::new(
            "identifier",
            "http://hl7.org/fhir/SearchParameter/Patient-identifier",
            SearchParameterType::Token,
            vec!["Patient".to_string()],
        )
        .with_expression("Patient.identifier")
        .with_element_type_hint(ElementTypeHint::Identifier),
    );
    registry.register(
        SearchParameter::new(
            "gender",
            "http://hl7.org/fhir/SearchParameter/Patient-gender",
            SearchParameterType::Token,
            vec!["Patient".to_string()],
        )
        .with_expression("Patient.gender")
        .with_element_type_hint(ElementTypeHint::SimpleCode),
    );
    registry.register(
        SearchParameter::new(
            "code",
            "http://hl7.org/fhir/SearchParameter/Observation-code",
            SearchParameterType::Token,
            vec!["Observation".to_string()],
        )
        .with_expression("Observation.code")
        .with_element_type_hint(ElementTypeHint::Token),
    );
    registry.register(
        SearchParameter::new(
            "subject",
            "http://hl7.org/fhir/SearchParameter/Observation-subject",
            SearchParameterType::Reference,
            vec!["Observation".to_string()],
        )
        .with_expression("Observation.subject")
        .with_targets(vec!["Patient".to_string()]),
    );
    registry.register(
        SearchParameter::new(
            "value-quantity",
            "http://hl7.org/fhir/SearchParameter/Observation-value-quantity",
            SearchParameterType::Quantity,
            vec!["Observation".to_string()],
        )
        .with_expression("Observation.valueQuantity"),
    );
    registry.register(
        SearchParameter::new(
            "code-value-quantity",
            "http://hl7.org/fhir/SearchParameter/Observation-code-value-quantity",
            SearchParameterType::Composite,
            vec!["Observation".to_string()],
        )
        .with_expression("Observation.component")
        .with_components(vec![
            SearchParameterComponent {
                definition: "http://hl7.org/fhir/SearchParameter/Observation-code".to_string(),
                expression: "Observation.component.code".to_string(),
            },
            SearchParameterComponent {
                definition: "http://hl7.org/fhir/SearchParameter/Observation-value-quantity"
                    .to_string(),
                expression: "Observation.component.valueQuantity".to_string(),
            },
        ]),
    );

    registry
}

fn collect_node_types(plan: &Value, out: &mut Vec<String>) {
    if let Some(node_type) = plan.get("Node Type").and_then(Value::as_str) {
        out.push(node_type.to_string());
    }
    if let Some(children) = plan.get("Plans").and_then(Value::as_array) {
        for child in children {
            collect_node_types(child, out);
        }
    }
}

fn redact_sql_shape(sql: &str) -> String {
    let mut redacted = String::with_capacity(sql.len());
    let mut chars = sql.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '$' && matches!(chars.peek(), Some(next) if next.is_ascii_digit()) {
            redacted.push_str("$N");
            while matches!(chars.peek(), Some(next) if next.is_ascii_digit()) {
                chars.next();
            }
        } else {
            redacted.push(ch);
        }
    }

    redacted.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn stable_hash(value: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

fn assert_redacted_shape(label: &str, sql_shape: &str) {
    for leaked_value in [
        "explain-patient",
        "2024-01-01",
        "2024-12-31",
        "1980-01-01",
        "2000-12-31",
        "Smith",
        "female",
        "example.org",
        "12345",
        "8480-6",
        "100",
    ] {
        assert!(
            !sql_shape.contains(leaked_value),
            "{label} SQL shape leaked bind value {leaked_value}: {sql_shape}"
        );
    }
}

fn assert_expected_shape(label: &str, sql_shape: &str, expected_fragments: &[&str]) {
    for expected in expected_fragments {
        assert!(
            sql_shape.contains(expected),
            "{label} SQL shape missing expected fragment {expected:?}: {sql_shape}"
        );
    }
}
