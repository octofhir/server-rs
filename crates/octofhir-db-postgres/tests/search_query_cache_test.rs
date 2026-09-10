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

#[tokio::test]
async fn quantity_cache_preserves_ids_total_and_lookahead() {
    // Always a new isolated database, never DATABASE_URL or a working server's pool.
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
