use octofhir_db_postgres::{SchemaManager, migrations};
use octofhir_search::{
    ElementTypeHint, SearchParameter, SearchParameterRegistry, SearchParameterType,
    build_native_ir_query_from_params, parameters::SearchParameterComponent, parse_query_string,
    register_common_parameters,
};
use serde_json::Value;
use sqlx_postgres::PgPoolOptions;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
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

    let registry = representative_registry();

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

        let started = Instant::now();
        let explain = octofhir_db_postgres::queries::search::explain_built_search_query_json(
            &pool, &query, false,
        )
        .await
        .unwrap_or_else(|error| panic!("{} EXPLAIN should run: {error}", case.label));
        let elapsed = started.elapsed();

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
        println!(
            "{} | resource={} | build_ms={:.3} | explain_ms={:.3} | params={} | sql_shape_hash={:016x} | nodes={:?}\nsql_shape={}",
            case.label,
            case.resource_type,
            build_elapsed.as_secs_f64() * 1000.0,
            elapsed.as_secs_f64() * 1000.0,
            query.params.len(),
            stable_hash(&sql_shape),
            node_types,
            sql_shape
        );
    }
}

struct SearchExplainCase {
    label: &'static str,
    resource_type: &'static str,
    query: &'static str,
}

fn representative_queries() -> Vec<SearchExplainCase> {
    vec![
        SearchExplainCase {
            label: "patient_id",
            resource_type: "Patient",
            query: "_id=explain-patient&_count=1",
        },
        SearchExplainCase {
            label: "patient_last_updated_window",
            resource_type: "Patient",
            query: "_lastUpdated=ge2024-01-01&_lastUpdated=le2024-12-31&_count=10",
        },
        SearchExplainCase {
            label: "patient_birthdate_window",
            resource_type: "Patient",
            query: "birthdate=ge1980-01-01&birthdate=le2000-12-31&_count=10",
        },
        SearchExplainCase {
            label: "patient_family_prefix",
            resource_type: "Patient",
            query: "family=Smith&_count=10",
        },
        SearchExplainCase {
            label: "patient_family_exact",
            resource_type: "Patient",
            query: "family:exact=Smith&_count=10",
        },
        SearchExplainCase {
            label: "patient_identifier",
            resource_type: "Patient",
            query: "identifier=http://hospital.example/mrn|12345&_count=10",
        },
        SearchExplainCase {
            label: "observation_code",
            resource_type: "Observation",
            query: "code=http://loinc.org|8480-6&_count=10",
        },
        SearchExplainCase {
            label: "observation_subject",
            resource_type: "Observation",
            query: "subject=Patient/explain-patient&_count=10",
        },
        SearchExplainCase {
            label: "observation_subject_patient_family",
            resource_type: "Observation",
            query: "subject:Patient.family=Smith&_count=10",
        },
        SearchExplainCase {
            label: "patient_has_observation_code",
            resource_type: "Patient",
            query: "_has:Observation:subject:code=http://loinc.org|8480-6&_count=10",
        },
        SearchExplainCase {
            label: "observation_quantity",
            resource_type: "Observation",
            query: "value-quantity=ge100|http://unitsofmeasure.org|mm[Hg]&_count=10",
        },
        SearchExplainCase {
            label: "observation_composite_code_quantity",
            resource_type: "Observation",
            query: "code-value-quantity=http://loinc.org|8480-6$ge100|http://unitsofmeasure.org|mm[Hg]&_count=10",
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
