use octofhir_db_postgres::{PostgresStorage, SchemaManager, migrations};
use octofhir_search::{
    BuiltQuery, ElementTypeHint, SearchParameter, SearchParameterRegistry, SearchParameterType,
    SqlValue, build_native_ir_query_from_params, parse_query_string,
};
use octofhir_storage::FhirStorage;
use serde_json::{Value, json};
use sqlx_core::{query_scalar::query_scalar, raw_sql::raw_sql, sql_str::AssertSqlSafe};
use sqlx_postgres::{PgPool, PgPoolOptions};
use std::{collections::BTreeSet, sync::Arc};
use testcontainers::{ImageExt, runners::AsyncRunner};
use testcontainers_modules::postgres::Postgres;

struct Resolver;
#[async_trait::async_trait]
impl octofhir_search::loader::ElementTypeResolver for Resolver {
    async fn resolve(&self, _: &str, path: &str) -> Option<(String, bool)> {
        match path {
            "name" => Some(("HumanName".into(), true)),
            "name.family" => Some(("string".into(), false)),
            _ => None,
        }
    }
}

fn registry() -> Arc<SearchParameterRegistry> {
    let registry = Arc::new(SearchParameterRegistry::new());
    octofhir_search::register_common_parameters(&registry);
    for (rt, name, expression, targets) in [
        (
            "Observation",
            "subject",
            "Observation.subject",
            vec!["Patient", "Group"],
        ),
        (
            "Patient",
            "organization",
            "Patient.managingOrganization",
            vec!["Organization"],
        ),
    ] {
        registry.register(
            SearchParameter::new(
                name,
                format!("urn:test:{rt}:{name}"),
                SearchParameterType::Reference,
                vec![rt.into()],
            )
            .with_expression(expression)
            .with_targets(targets.into_iter().map(String::from).collect()),
        );
    }
    for (rt, name, expression, kind, hint) in [
        (
            "Patient",
            "family",
            "Patient.name.family",
            SearchParameterType::String,
            ElementTypeHint::Array("string".into()),
        ),
        (
            "Observation",
            "code",
            "Observation.code",
            SearchParameterType::Token,
            ElementTypeHint::Token,
        ),
        (
            "Organization",
            "name",
            "Organization.name",
            SearchParameterType::String,
            ElementTypeHint::Unknown,
        ),
    ] {
        registry.register(
            SearchParameter::new(name, format!("urn:test:{rt}:{name}"), kind, vec![rt.into()])
                .with_expression(expression)
                .with_element_type_hint(hint),
        );
    }
    registry
}

async fn setup() -> (
    testcontainers::ContainerAsync<Postgres>,
    PgPool,
    Arc<SearchParameterRegistry>,
) {
    let container = Postgres::default()
        .with_tag("16-alpine")
        .start()
        .await
        .unwrap();
    let port = container.get_host_port_ipv4(5432).await.unwrap();
    let url = format!("postgres://postgres:postgres@127.0.0.1:{port}/postgres");
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await
        .unwrap();
    migrations::run(&pool, &url).await.unwrap();
    SchemaManager::ensure_archive_function(&pool).await.unwrap();
    for rt in ["Patient", "Observation", "Organization"] {
        SchemaManager::new(pool.clone())
            .create_resource_schema(rt)
            .await
            .unwrap();
    }
    let registry = registry();
    assert_eq!(
        octofhir_db_postgres::functional_indexes::create_default_search_indexes(
            &pool,
            &registry,
            &["Patient.family".into(), "Observation.subject".into()],
            &Resolver,
        )
        .await,
        2
    );
    (container, pool, registry)
}

async fn sql(pool: &PgPool, sql: &str) {
    raw_sql(AssertSqlSafe(sql.to_owned()))
        .execute(pool)
        .await
        .unwrap();
}

async fn assert_search(
    pool: &PgPool,
    registry: &Arc<SearchParameterRegistry>,
    rt: &str,
    query: &str,
    expected: &[&str],
) {
    let params = parse_query_string(&format!("{query}&_count=100&_total=accurate"), 10, 100);
    let result = octofhir_db_postgres::queries::search::execute_search_raw_with_config(
        pool,
        rt,
        &params,
        Some(registry),
        None,
        None,
    )
    .await
    .unwrap();
    let ids: BTreeSet<_> = result.entries.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids, expected.iter().copied().collect(), "{rt}?{query}");
    assert_eq!(
        result.entries.len(),
        ids.len(),
        "duplicate matches: {query}"
    );
    assert_eq!(result.total, Some(expected.len() as u32), "{query}");
    assert!(!result.has_more, "{query}");
}

#[tokio::test]
async fn search_escaping_preserves_literal_delimiters() {
    let (_container, pool, registry) = setup().await;
    let storage = PostgresStorage::from_pool(pool.clone());
    storage
        .create(
            &json!({"resourceType":"Patient", "id":"comma", "name":[{"family":"Smith, Jones"}]}),
        )
        .await
        .unwrap();
    for (id, code) in [
        ("pipe", "a|b"),
        ("comma", "a,b"),
        ("slash", "a\\b"),
        ("dollar", "a$b"),
    ] {
        storage
            .create(
                &json!({"resourceType":"Observation", "id":id, "code":{"coding":[{"code":code}]}}),
            )
            .await
            .unwrap();
    }
    assert_search(
        &pool,
        &registry,
        "Patient",
        "family:exact=Smith%5C%2C%20Jones",
        &["comma"],
    )
    .await;
    for (query, expected) in [
        ("code=a%5C%7Cb", "pipe"),
        ("code=a%5C%2Cb", "comma"),
        ("code=a%5C%5Cb", "slash"),
        ("code=a%5C%24b", "dollar"),
    ] {
        assert_search(&pool, &registry, "Observation", query, &[expected]).await;
    }
    assert_search(
        &pool,
        &registry,
        "Observation",
        "code=a%5C%7Cb,a%5C%2Cb",
        &["pipe", "comma"],
    )
    .await;
}

#[tokio::test]
async fn string_search_matches_individual_values() {
    let (_container, pool, registry) = setup().await;
    let storage = PostgresStorage::from_pool(pool.clone());
    for (id, families) in [
        ("whole", vec!["Alice Smith"]),
        ("split", vec!["Alice", "Smith"]),
        ("prefix", vec!["Smithson"]),
        ("accent", vec!["Smíth"]),
    ] {
        let names: Vec<_> = families
            .into_iter()
            .map(|family| json!({"family":family}))
            .collect();
        storage
            .create(&json!({"resourceType":"Patient", "id":id, "name":names}))
            .await
            .unwrap();
    }
    assert_search(
        &pool,
        &registry,
        "Patient",
        "family=Smith",
        &["split", "prefix", "accent"],
    )
    .await;
    assert_search(
        &pool,
        &registry,
        "Patient",
        "family:contains=Alice%20Smith",
        &["whole"],
    )
    .await;
    assert_search(
        &pool,
        &registry,
        "Patient",
        "family:exact=Smith",
        &["split"],
    )
    .await;
}

#[tokio::test]
async fn include_iterate_follows_resources_from_other_specs() {
    let (_container, pool, registry) = setup().await;
    let storage = PostgresStorage::from_pool(pool.clone());
    for resource in [
        json!({"resourceType":"Organization", "id":"org"}),
        json!({"resourceType":"Patient", "id":"patient", "managingOrganization":{"reference":"Organization/org"}}),
        json!({"resourceType":"Observation", "id":"obs", "subject":{"reference":"Patient/patient"}}),
    ] {
        storage.create(&resource).await.unwrap();
    }
    let iterative_query =
        "_id=obs&_include=Observation:subject:Patient&_include:iterate=Patient:organization";
    for limits in [
        octofhir_db_postgres::queries::search::IncludeLimits {
            max_resources: 1,
            ..Default::default()
        },
        octofhir_db_postgres::queries::search::IncludeLimits {
            max_bytes: 1,
            ..Default::default()
        },
        octofhir_db_postgres::queries::search::IncludeLimits {
            max_depth: 1,
            ..Default::default()
        },
    ] {
        let params = parse_query_string(iterative_query, 10, 100);
        let error = octofhir_db_postgres::queries::search::execute_search_raw_with_options(
            &pool,
            "Observation",
            &params,
            Some(&registry),
            None,
            octofhir_db_postgres::queries::search::RawSearchOptions {
                include_limits: limits,
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(
            error,
            octofhir_storage::StorageError::InvalidResource { .. }
        ));
        assert!(error.to_string().contains("budget"), "{error}");
        let params = parse_query_string(&format!("{iterative_query}&_count=0"), 10, 100);
        let count = octofhir_db_postgres::queries::search::execute_search_raw_with_options(
            &pool,
            "Observation",
            &params,
            Some(&registry),
            None,
            octofhir_db_postgres::queries::search::RawSearchOptions {
                include_limits: limits,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(count.total, Some(1));
        assert!(count.included.is_empty());
    }
    for (specs, expected) in [
        (
            "_include=Observation:subject:Patient&_include:iterate=Patient:organization",
            vec![("Patient", "patient"), ("Organization", "org")],
        ),
        (
            "_include:iterate=Patient:organization&_include=Observation:subject:Patient",
            vec![("Patient", "patient"), ("Organization", "org")],
        ),
        (
            "_include=Observation:subject:Patient&_include=Patient:organization",
            vec![("Patient", "patient")],
        ),
    ] {
        let params = parse_query_string(&format!("_id=obs&_total=accurate&{specs}"), 10, 100);
        let raw = octofhir_db_postgres::queries::search::execute_search_raw_with_config(
            &pool,
            "Observation",
            &params,
            Some(&registry),
            None,
            None,
        )
        .await
        .unwrap();
        let parsed = octofhir_db_postgres::queries::search::execute_search(
            &pool,
            "Observation",
            &params,
            Some(&registry),
        )
        .await
        .unwrap();
        let expected: BTreeSet<_> = expected.into_iter().collect();
        assert_eq!(
            raw.included
                .iter()
                .map(|r| (r.resource_type.as_str(), r.id.as_str()))
                .collect::<BTreeSet<_>>(),
            expected,
            "{specs}"
        );
        assert_eq!(
            parsed
                .entries
                .iter()
                .skip(1)
                .map(|r| (r.resource_type.as_str(), r.id.as_str()))
                .collect::<BTreeSet<_>>(),
            expected,
            "{specs}"
        );
        assert_eq!(raw.included.len(), expected.len());
        assert_eq!(parsed.entries.len(), expected.len() + 1);
        assert_eq!(raw.total, Some(1));
        assert_eq!(parsed.total, Some(1));
    }
}

#[tokio::test]
async fn includes_deduplicate_across_specs_and_exclude_matches() {
    let (_container, pool, registry) = setup().await;
    registry.register(
        SearchParameter::new(
            "derived",
            "urn:test:derived",
            SearchParameterType::Reference,
            vec!["Observation".into()],
        )
        .with_expression("Observation.derivedFrom")
        .with_targets(vec!["Observation".into()]),
    );
    let storage = PostgresStorage::from_pool(pool.clone());
    storage
        .create(&json!({"resourceType":"Patient", "id":"main"}))
        .await
        .unwrap();
    storage.create(&json!({"resourceType":"Observation", "id":"main", "subject":{"reference":"Patient/main"}, "derivedFrom":[{"reference":"Observation/main"},{"reference":"Observation/peer"}]})).await.unwrap();
    storage.create(&json!({"resourceType":"Observation", "id":"peer", "derivedFrom":[{"reference":"Observation/main"}]})).await.unwrap();
    for specs in [
        "_include=Observation:derived&_include=Observation:derived:Observation&_revinclude=Observation:derived&_include=Observation:subject:Patient",
        "_include=Observation:subject:Patient&_revinclude=Observation:derived&_include=Observation:derived:Observation&_include=Observation:derived",
    ] {
        let params = parse_query_string(&format!("_id=main&_total=accurate&{specs}"), 10, 100);
        let raw = octofhir_db_postgres::queries::search::execute_search_raw_with_config(
            &pool,
            "Observation",
            &params,
            Some(&registry),
            None,
            None,
        )
        .await
        .unwrap();
        let parsed = octofhir_db_postgres::queries::search::execute_search(
            &pool,
            "Observation",
            &params,
            Some(&registry),
        )
        .await
        .unwrap();
        for (entries, included, total, has_more) in [
            (
                raw.entries.len(),
                raw.included
                    .iter()
                    .map(|r| (r.resource_type.as_str(), r.id.as_str()))
                    .collect::<Vec<_>>(),
                raw.total,
                raw.has_more,
            ),
            (
                1,
                parsed
                    .entries
                    .iter()
                    .skip(1)
                    .map(|r| (r.resource_type.as_str(), r.id.as_str()))
                    .collect::<Vec<_>>(),
                parsed.total,
                parsed.has_more,
            ),
        ] {
            assert_eq!(entries, 1);
            assert_eq!(total, Some(1));
            assert!(!has_more);
            assert_eq!(
                included.iter().copied().collect::<BTreeSet<_>>(),
                BTreeSet::from([("Observation", "peer"), ("Patient", "main")])
            );
            assert_eq!(included.len(), 2, "duplicate includes: {included:?}");
        }
    }
}

#[tokio::test]
async fn chained_search_preserves_unicode_and_prefix_looking_names() {
    let (_container, pool, registry) = setup().await;
    let storage = PostgresStorage::from_pool(pool.clone());
    for (id, family) in [
        ("chinese", "李"),
        ("french", "Émile"),
        ("russian", "Иванов"),
        ("prefix", "ge李"),
    ] {
        storage
            .create(&json!({"resourceType":"Patient", "id":id, "name":[{"family":family}]}))
            .await
            .unwrap();
        storage.create(&json!({"resourceType":"Observation", "id":id, "subject":{"reference":format!("Patient/{id}")}})).await.unwrap();
    }
    for (family, expected) in [
        ("李", vec!["chinese"]),
        ("Émile", vec!["french"]),
        ("Иванов", vec!["russian"]),
        ("ge李", vec!["prefix"]),
        ("王", vec![]),
    ] {
        let query = format!("subject:Patient.family={family}");
        assert_search(&pool, &registry, "Observation", &query, &expected).await;
    }
}

#[tokio::test]
async fn traversal_preserves_reference_forms_types_deletion_and_duplicates() {
    let (_container, pool, registry) = setup().await;
    let storage = PostgresStorage::from_pool(pool.clone());
    storage
        .create(&json!({"resourceType":"Organization", "id":"org", "name":"Acme"}))
        .await
        .unwrap();
    for (id, family) in [("good", "Smith"), ("other", "Jones"), ("deleted", "Smith")] {
        storage.create(&json!({"resourceType":"Patient", "id":id, "name":[{"family":family}], "managingOrganization":{"reference":"Organization/org"}})).await.unwrap();
    }
    for (id, subject) in [
        ("local", json!({"reference":"Patient/good"})),
        (
            "absolute",
            json!({"reference":"https://local.example/fhir/Patient/good"}),
        ),
        (
            "remote",
            json!({"reference":"https://remote.example/fhir/Patient/good"}),
        ),
        ("versioned", json!({"reference":"Patient/good/_history/1"})),
        (
            "absolute-versioned",
            json!({"reference":"https://remote.example/fhir/Patient/good/_history/1"}),
        ),
        ("wrong-type", json!({"reference":"Group/good"})),
        ("wrong-type-suffix", json!({"reference":"NotPatient/good"})),
        (
            "array",
            json!([{"reference":"Group/good"}, {"reference":"Patient/good"}, {"reference":"Patient/other"}]),
        ),
        (
            "duplicate",
            json!([{"reference":"Patient/good"}, {"reference":"Patient/good"}]),
        ),
        ("deleted-target", json!({"reference":"Patient/deleted"})),
        ("deleted-source", json!({"reference":"Patient/good"})),
        ("dangling", json!({"reference":"Patient/absent"})),
        ("contained", json!({"reference":"#good"})),
        ("identifier", json!({"identifier":{"value":"good"}})),
        ("null", Value::Null),
        ("empty", json!([])),
    ] {
        storage.create(&json!({"resourceType":"Observation", "id":id, "status":"final", "subject":subject, "code":{"coding":[{"code":"hit"}]}})).await.unwrap();
    }
    sql(&pool, "UPDATE patient SET status='deleted' WHERE id='deleted'; UPDATE observation SET status='deleted' WHERE id='deleted-source'").await;
    let expected = ["absolute", "array", "duplicate", "local", "remote"];
    for (ids, specification, expected_includes, expected_total) in [
        ("good", "Observation:subject", expected.as_slice(), 1),
        (
            "good,other",
            "Observation:subject:Patient",
            expected.as_slice(),
            2,
        ),
        ("good", "Observation:subject:Group", &[][..], 1),
        ("deleted", "Observation:subject", &[][..], 0),
        ("absent", "Observation:subject", &[][..], 0),
    ] {
        let params = parse_query_string(
            &format!("_id={ids}&_revinclude={specification}&_total=accurate&_count=100"),
            10,
            100,
        );
        let raw = octofhir_db_postgres::queries::search::execute_search_raw(
            &pool,
            "Patient",
            &params,
            Some(&registry),
        )
        .await
        .unwrap();
        let parsed = octofhir_db_postgres::queries::search::execute_search(
            &pool,
            "Patient",
            &params,
            Some(&registry),
        )
        .await
        .unwrap();
        assert_eq!(raw.entries.len(), expected_total as usize);
        assert_eq!(
            parsed.entries.len(),
            expected_total as usize + expected_includes.len()
        );
        for (included, total, has_more) in [
            (
                raw.included
                    .iter()
                    .map(|r| (r.resource_type.as_str(), r.id.as_str()))
                    .collect::<Vec<_>>(),
                raw.total,
                raw.has_more,
            ),
            (
                parsed
                    .entries
                    .iter()
                    .filter(|r| r.resource_type == "Observation")
                    .map(|r| (r.resource_type.as_str(), r.id.as_str()))
                    .collect::<Vec<_>>(),
                parsed.total,
                parsed.has_more,
            ),
        ] {
            let expected_set: BTreeSet<_> = expected_includes
                .iter()
                .map(|id| ("Observation", *id))
                .collect();
            assert_eq!(
                included.iter().copied().collect::<BTreeSet<_>>(),
                expected_set,
                "{ids} {specification}"
            );
            assert_eq!(
                included.len(),
                expected_set.len(),
                "duplicate revinclude {ids}"
            );
            assert_eq!(total, Some(expected_total));
            assert!(!has_more);
        }
    }
    // Preserve the current suffix-matching contract, including remote bases and
    // nonmatching versioned references. This optimization does not redefine resolution.
    assert_search(
        &pool,
        &registry,
        "Observation",
        "subject:Patient.family=Smith",
        &expected,
    )
    .await;
    assert_search(
        &pool,
        &registry,
        "Observation",
        "subject:Patient.organization:Organization.name=Acme",
        &expected,
    )
    .await;
    assert_search(
        &pool,
        &registry,
        "Patient",
        "_has:Observation:subject:code=hit",
        &["good", "other"],
    )
    .await;
    assert_search(
        &pool,
        &registry,
        "Organization",
        "_has:Patient:organization:_has:Observation:subject:code=hit",
        &["org"],
    )
    .await;
    assert_search(
        &pool,
        &registry,
        "Observation",
        "subject:Patient.family=Smith&subject:Patient.family=Jones",
        &["array"],
    )
    .await;
    assert_search(
        &pool,
        &registry,
        "Observation",
        "code=hit&subject:Patient.family=Smith",
        &expected,
    )
    .await;
    assert_search(
        &pool,
        &registry,
        "Patient",
        "_has:Observation:subject:code=hit&_has:Observation:subject:_id=local",
        &["good"],
    )
    .await;
    assert_search(
        &pool,
        &registry,
        "Observation",
        "subject:Patient._id=good",
        &expected,
    )
    .await;
    assert_search(
        &pool,
        &registry,
        "Patient",
        "_has:Observation:subject:_id=local,array",
        &["good", "other"],
    )
    .await;
    // Separate negative cases prevent a valid sibling reference from masking a bad match.
    for id in [
        "versioned",
        "absolute-versioned",
        "wrong-type",
        "wrong-type-suffix",
        "dangling",
        "contained",
        "identifier",
        "null",
        "empty",
        "deleted-source",
        "deleted-target",
    ] {
        assert_search(
            &pool,
            &registry,
            "Patient",
            &format!("_has:Observation:subject:_id={id}"),
            &[],
        )
        .await;
    }
    sql(
        &pool,
        "UPDATE organization SET status='deleted' WHERE id='org'",
    )
    .await;
    assert_search(
        &pool,
        &registry,
        "Observation",
        "subject:Patient.organization:Organization.name=Acme",
        &[],
    )
    .await;
    assert_search(
        &pool,
        &registry,
        "Organization",
        "_has:Patient:organization:_has:Observation:subject:code=hit",
        &[],
    )
    .await;
    pool.close().await;
}

#[tokio::test]
#[ignore = "manual isolated PostgreSQL SQL A/B benchmark; no HTTP latency"]
async fn reference_sql_ab_benchmark() {
    let (_container, pool, registry) = setup().await;
    sql(&pool, r#"
        INSERT INTO patient(id,txid,resource) SELECT 'p-'||i,i,jsonb_build_object('resourceType','Patient','id','p-'||i,'name',jsonb_build_array(jsonb_build_object('family',CASE WHEN i%10=0 THEN 'Smith' ELSE 'Jones' END))) FROM generate_series(1,20000) i;
        INSERT INTO observation(id,txid,resource) SELECT 'o-'||i,i,jsonb_build_object('resourceType','Observation','id','o-'||i,'subject',jsonb_build_object('reference','Patient/p-'||i),'code',jsonb_build_object('coding',jsonb_build_array(jsonb_build_object('code',CASE WHEN i%10=0 THEN 'hit' ELSE 'miss' END)))) FROM generate_series(1,20000) i;
        ANALYZE patient; ANALYZE observation;
        SET jit=off; SET max_parallel_workers_per_gather=0; SET statement_timeout='10s';
    "#).await;
    for (label, rt, query) in [
        (
            "chain",
            "Observation",
            "subject:Patient.family=Smith&_count=10",
        ),
        (
            "has",
            "Patient",
            "_has:Observation:subject:code=hit&_count=10",
        ),
        (
            "has_id",
            "Patient",
            "_has:Observation:subject:code=hit&_id=p-10&_count=10",
        ),
    ] {
        let built = build_native_ir_query_from_params(
            rt,
            &parse_query_string(query, 10, 100),
            &registry,
            "public",
        )
        .unwrap()
        .builder
        .with_raw_resource(true)
        .build()
        .unwrap();
        let mut legacy = BuiltQuery {
            sql: match label {
                "chain" => include_str!("fixtures/reference_chain_legacy.sql"),
                _ => include_str!("fixtures/reference_has_legacy.sql"),
            }
            .trim()
            .to_owned(),
            params: built.params.clone(),
        };
        if label == "has_id" {
            assert_eq!(built.params.len(), 3);
            legacy.sql = legacy
                .sql
                .replace("AND r.status", "AND r.id = $3 AND r.status");
        }
        let expected_rows = if label == "has_id" { 1 } else { 11 };
        // No explicit sort: different valid pages are permitted. Verify each
        // returned ID against the deterministic fixture, and reject duplicates.
        for candidate in [&legacy, &built] {
            let ids = query_ids(&pool, candidate).await;
            assert_eq!(ids.len(), expected_rows);
            assert_eq!(ids.iter().collect::<BTreeSet<_>>().len(), expected_rows);
            assert!(
                ids.iter()
                    .all(|id| id.split_once('-').unwrap().1.parse::<u32>().unwrap() % 10 == 0)
            );
        }
        println!("{label} SQL: {}", built.sql);
        for iteration in 0..6 {
            let variants = if iteration % 2 == 0 {
                [("legacy", &legacy), ("new", &built)]
            } else {
                [("new", &built), ("legacy", &legacy)]
            };
            for (variant, candidate) in variants {
                let plan = octofhir_db_postgres::queries::search::explain_built_search_query_json(
                    &pool, candidate, true,
                )
                .await
                .unwrap();
                assert_eq!(plan[0]["Plan"]["Actual Rows"], expected_rows);
                if iteration > 0 {
                    println!(
                        "{label} {variant} sample={iteration}: execution_ms={} plan={}",
                        plan[0]["Execution Time"], plan[0]["Plan"]
                    );
                }
            }
        }
    }
    pool.close().await;
}

async fn query_ids(pool: &PgPool, built: &BuiltQuery) -> Vec<String> {
    let mut query = query_scalar::<_, String>(AssertSqlSafe(format!(
        "SELECT id FROM ({}) AS matches",
        built.sql
    )));
    for param in &built.params {
        query = match param {
            SqlValue::Text(s) | SqlValue::Json(s) | SqlValue::Timestamp(s) => query.bind(s),
            SqlValue::Integer(i) => query.bind(*i),
            SqlValue::Float(f) => query.bind(*f),
            SqlValue::Boolean(b) => query.bind(*b),
            SqlValue::Null => query.bind(None::<String>),
        };
    }
    query.fetch_all(pool).await.unwrap()
}
