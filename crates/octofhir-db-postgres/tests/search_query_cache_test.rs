use octofhir_db_postgres::{PostgresStorage, SchemaManager, migrations};
use octofhir_search::{
    QueryCache, SearchParameter, SearchParameterRegistry, SearchParameterType,
    build_native_ir_query_from_params, parse_query_string,
};
use octofhir_storage::FhirStorage;
use serde_json::json;
use sqlx_postgres::PgPoolOptions;
use std::{collections::BTreeSet, hint::black_box, sync::Arc, time::Instant};
use testcontainers::{ImageExt, runners::AsyncRunner};
use testcontainers_modules::postgres::Postgres;

struct UnusedResolver;

#[async_trait::async_trait]
impl octofhir_search::loader::ElementTypeResolver for UnusedResolver {
    async fn resolve(&self, _: &str, _: &str) -> Option<(String, bool)> {
        panic!("no indexed parameters require type resolution");
    }
}

fn registry() -> Arc<SearchParameterRegistry> {
    let registry = Arc::new(SearchParameterRegistry::new());
    octofhir_search::register_common_parameters(&registry);
    registry.register(
        SearchParameter::new(
            "value-quantity",
            "http://hl7.org/fhir/SearchParameter/Observation-value-quantity",
            SearchParameterType::Quantity,
            vec!["Observation".into()],
        )
        .with_expression("Observation.valueQuantity"),
    );
    registry
}

async fn observation_fixture() -> (
    testcontainers::ContainerAsync<Postgres>,
    sqlx_postgres::PgPool,
    PostgresStorage,
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
    SchemaManager::new(pool.clone())
        .create_resource_schema("Observation")
        .await
        .unwrap();
    let storage = PostgresStorage::from_pool(pool.clone());
    (container, pool, storage)
}

#[tokio::test]
async fn quantity_cache_preserves_ids_total_and_lookahead() {
    // Always a new isolated database, never DATABASE_URL or a working server's pool.
    let (_container, pool, storage) = observation_fixture().await;
    for value in [80, 120] {
        storage
            .create(&json!({
                "resourceType": "Observation", "id": format!("q-{value}"),
                "status": "final", "code": {"text": "cache regression"},
                "valueQuantity": {"value": value}
            }))
            .await
            .unwrap();
    }
    let registry = registry();
    // Install production query helpers without adding any functional indexes.
    assert_eq!(
        octofhir_db_postgres::functional_indexes::create_default_search_indexes(
            &pool,
            &registry,
            &[],
            &UnusedResolver,
        )
        .await,
        0
    );
    let mut failures = Vec::new();
    for mode in ["absent", "disabled", "enabled"] {
        for thresholds in [[100, 200, 0], [200, 100, 0]] {
            let cache = if mode == "disabled" {
                QueryCache::disabled()
            } else {
                QueryCache::new(16)
            };
            for threshold in thresholds {
                let params = parse_query_string(
                    &format!("value-quantity=ge{threshold}&_count=1&_sort=_id&_total=accurate"),
                    10,
                    100,
                );
                let result = octofhir_db_postgres::queries::search::execute_search_raw_with_config(
                    &pool,
                    "Observation",
                    &params,
                    Some(&registry),
                    None,
                    if mode == "absent" { None } else { Some(&cache) },
                )
                .await
                .unwrap();
                let ids: BTreeSet<_> = result.entries.iter().map(|r| r.id.as_str()).collect();
                let expected: BTreeSet<_> = if threshold == 200 {
                    BTreeSet::new()
                } else {
                    BTreeSet::from(["q-120"])
                };
                let total = match threshold {
                    0 => 2,
                    100 => 1,
                    _ => 0,
                };
                if ids != expected
                    || result.total != Some(total)
                    || result.has_more != (threshold == 0)
                {
                    failures.push(format!("mode={mode}, order={thresholds:?}, ge{threshold}: ids={ids:?} expected={expected:?}, total={:?} expected={total}, has_more={} expected={}", result.total, result.has_more, threshold == 0));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    pool.close().await;
}

#[test]
#[ignore = "manual SQL build CPU measurement; run with --release --ignored --nocapture"]
fn measure_fresh_sql_build_overhead() {
    let registry = registry();
    for query in [
        "value-quantity=ge100",
        "value-quantity=ge200|http://unitsofmeasure.org|mm[Hg]",
    ] {
        let params = parse_query_string(query, 10, 100);
        let builder =
            build_native_ir_query_from_params("Observation", &params, &registry, "public")
                .unwrap()
                .builder
                .with_raw_resource(true);
        let iterations = 100_000;
        for _ in 0..1_000 {
            black_box(builder.build().unwrap());
        }
        let started = Instant::now();
        for _ in 0..iterations {
            black_box(builder.extract_params());
        }
        let extract = started.elapsed().as_secs_f64() * 1e9 / f64::from(iterations);
        let started = Instant::now();
        for _ in 0..iterations {
            black_box(builder.build().unwrap());
        }
        let build = started.elapsed().as_secs_f64() * 1e9 / f64::from(iterations);
        println!(
            "{query}: extract={extract:.0} ns/op, fresh_build={build:.0} ns/op, delta={:.0} ns/op ({iterations} iterations; excludes conversion/DB/cache lookup)",
            build - extract
        );
    }
}

#[tokio::test]
async fn count_only_uses_count_without_reading_resources_or_includes() {
    let (_container, pool, storage) = observation_fixture().await;
    for id in ["a", "b", "deleted"] {
        storage
            .create(&json!({"resourceType":"Observation", "id":id}))
            .await
            .unwrap();
    }
    sqlx_core::raw_sql::raw_sql(sqlx_core::sql_str::AssertSqlSafe(
        "UPDATE observation SET status='deleted' WHERE id='deleted'; CREATE ROLE count_reader; GRANT USAGE ON SCHEMA public TO count_reader; GRANT SELECT(id,status) ON observation TO count_reader; SET ROLE count_reader".to_owned(),
    )).execute(&pool).await.unwrap();
    let registry = registry();
    registry.register(
        SearchParameter::new(
            "subject",
            "urn:test:Observation:subject",
            SearchParameterType::Reference,
            vec!["Observation".into()],
        )
        .with_expression("Observation.subject")
        .with_targets(vec!["Observation".into()]),
    );

    // This role can count matching IDs, but cannot SELECT resource, txid or
    // timestamps. A resource query fails instead of silently doing wasted work.
    for control in [
        "_count=0",
        "_summary=count",
        "_summary=count&_count=10",
        "_count=0&_total=none",
        "_summary=count&_total=estimate",
    ] {
        for (filter, expected) in [("a,b", 2), ("a", 1), ("missing", 0)] {
            let params = parse_query_string(
                &format!(
                    "_id={filter}&{control}&_offset=100&_sort=-_lastUpdated&_revinclude=Observation:subject&_include=Observation:subject&unknown=x"
                ),
                10,
                100,
            );
            let raw = octofhir_db_postgres::queries::search::execute_search_raw_with_options(
                &pool,
                "Observation",
                &params,
                Some(&registry),
                None,
                octofhir_db_postgres::queries::search::RawSearchOptions {
                    collect_debug_plan: true,
                    ..Default::default()
                },
            )
            .await
            .unwrap();
            assert!(
                raw.entries.is_empty() && raw.included.is_empty(),
                "{control}"
            );
            assert_eq!(raw.total, Some(expected), "{control}");
            assert!(!raw.has_more);
            assert_eq!(raw.warnings.len(), 1);
            assert!(
                raw.debug
                    .unwrap()
                    .sql_shape
                    .unwrap()
                    .starts_with("SELECT COUNT(*)")
            );
            let parsed = octofhir_db_postgres::queries::search::execute_search(
                &pool,
                "Observation",
                &params,
                Some(&registry),
            )
            .await
            .unwrap();
            assert!(parsed.entries.is_empty());
            assert_eq!(parsed.total, Some(expected));
            assert!(!parsed.has_more);
            let mut tx = pool.begin().await.unwrap();
            let transactional = octofhir_db_postgres::queries::search::execute_search_with_tx(
                &mut tx,
                "Observation",
                &params,
                Some(&registry),
            )
            .await
            .unwrap();
            assert!(transactional.entries.is_empty());
            assert_eq!(transactional.total, Some(expected));
            assert!(!transactional.has_more);
            tx.rollback().await.unwrap();
        }
    }
    pool.close().await;
}

#[tokio::test]
async fn pagination_fetches_one_lookahead_and_preserves_page_boundaries() {
    let (_container, pool, storage) = observation_fixture().await;
    let all_ids = ["a", "b", "c", "d", "e"];
    for id in all_ids {
        storage
            .create(&json!({"resourceType":"Observation", "id":id}))
            .await
            .unwrap();
    }
    let registry = registry();
    let cache = QueryCache::new(16);
    for count in [1, 2, 3, 5, 10] {
        for offset in [0, 2, 4, 5, 99] {
            for accurate in [false, true] {
                let total_mode = if accurate { "accurate" } else { "none" };
                let params = parse_query_string(
                    &format!("_count={count}&_offset={offset}&_sort=_id&_total={total_mode}"),
                    10,
                    100,
                );
                let expected: Vec<_> = all_ids.iter().copied().skip(offset).take(count).collect();
                let expected_total = accurate.then_some(5);
                let expected_more = offset + count < all_ids.len();
                let result =
                    octofhir_db_postgres::queries::search::execute_search_raw_with_options(
                        &pool,
                        "Observation",
                        &params,
                        Some(&registry),
                        Some(&cache),
                        octofhir_db_postgres::queries::search::RawSearchOptions {
                            collect_debug_plan: true,
                            collect_explain_analyze: true,
                            ..Default::default()
                        },
                    )
                    .await
                    .unwrap();
                let debug = result.debug.unwrap();
                let sql = debug.sql_shape.unwrap();
                assert!(
                    sql.contains(&format!("LIMIT {}", count + 1)),
                    "extra lookahead: {sql}"
                );
                assert_eq!(
                    debug.explain.unwrap()[0]["Plan"]["Actual Rows"],
                    all_ids.len().saturating_sub(offset).min(count + 1)
                );
                assert_eq!(
                    result
                        .entries
                        .iter()
                        .map(|r| r.id.as_str())
                        .collect::<Vec<_>>(),
                    expected
                );
                assert_eq!(result.total, expected_total);
                assert_eq!(result.has_more, expected_more);
                assert!(result.included.is_empty());

                let parsed = octofhir_db_postgres::queries::search::execute_search(
                    &pool,
                    "Observation",
                    &params,
                    Some(&registry),
                )
                .await
                .unwrap();
                assert_eq!(
                    parsed
                        .entries
                        .iter()
                        .map(|r| r.id.as_str())
                        .collect::<Vec<_>>(),
                    expected
                );
                assert_eq!(parsed.total, expected_total);
                assert_eq!(parsed.has_more, expected_more);

                let mut tx = pool.begin().await.unwrap();
                let transactional = octofhir_db_postgres::queries::search::execute_search_with_tx(
                    &mut tx,
                    "Observation",
                    &params,
                    Some(&registry),
                )
                .await
                .unwrap();
                assert_eq!(
                    transactional
                        .entries
                        .iter()
                        .map(|r| r.id.as_str())
                        .collect::<Vec<_>>(),
                    expected
                );
                assert_eq!(transactional.total, expected_total);
                assert_eq!(transactional.has_more, expected_more);
                tx.rollback().await.unwrap();
            }
        }
    }
    pool.close().await;
}
