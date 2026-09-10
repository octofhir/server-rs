use octofhir_db_postgres::{PostgresStorage, SchemaManager, migrations};
use octofhir_search::{
    SearchParameter, SearchParameterRegistry, SearchParameterType, parse_query_string,
};
use octofhir_storage::FhirStorage;
use serde_json::json;
use sqlx_postgres::{PgPool, PgPoolOptions};
use std::{collections::BTreeSet, sync::Arc};
use testcontainers::{ImageExt, runners::AsyncRunner};
use testcontainers_modules::postgres::Postgres;

async fn fixture() -> (
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
    let registry = Arc::new(SearchParameterRegistry::new());
    octofhir_search::register_common_parameters(&registry);
    for (rt, name, expression) in [
        ("Patient", "birthdate", "Patient.birthDate"),
        ("Encounter", "date", "Encounter.period"),
        (
            "Observation",
            "component-date",
            "Observation.component.valueDateTime",
        ),
    ] {
        SchemaManager::new(pool.clone())
            .create_resource_schema(rt)
            .await
            .unwrap();
        registry.register(
            SearchParameter::new(
                name,
                format!("urn:test:{rt}:{name}"),
                SearchParameterType::Date,
                vec![rt.into()],
            )
            .with_expression(expression),
        );
    }
    (container, pool, registry)
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
    assert_eq!(result.entries.len(), ids.len(), "duplicates: {query}");
    assert_eq!(result.total, Some(expected.len() as u32), "{query}");
    assert!(!result.has_more, "{query}");
    let count_params = parse_query_string(&format!("{query}&_count=0"), 10, 100);
    let count = octofhir_db_postgres::queries::search::execute_search_raw_with_config(
        pool,
        rt,
        &count_params,
        Some(registry),
        None,
        None,
    )
    .await
    .unwrap();
    assert_eq!(count.total, result.total, "count-only: {query}");
    assert!(count.entries.is_empty());
    assert!(!count.has_more);
    let parsed =
        octofhir_db_postgres::queries::search::execute_search(pool, rt, &params, Some(registry))
            .await
            .unwrap();
    assert_eq!(
        parsed
            .entries
            .iter()
            .map(|r| r.id.as_str())
            .collect::<BTreeSet<_>>(),
        ids,
        "parsed: {query}"
    );
    assert_eq!(parsed.total, result.total, "parsed total: {query}");
    assert!(!parsed.has_more);
    let mut tx = pool.begin().await.unwrap();
    let transactional = octofhir_db_postgres::queries::search::execute_search_with_tx(
        &mut tx,
        rt,
        &params,
        Some(registry),
    )
    .await
    .unwrap();
    assert_eq!(
        transactional
            .entries
            .iter()
            .map(|r| r.id.as_str())
            .collect::<BTreeSet<_>>(),
        ids,
        "transactional: {query}"
    );
    assert_eq!(
        transactional.total, result.total,
        "transactional total: {query}"
    );
    assert!(!transactional.has_more);
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn system_search_uses_global_order_offset_total_and_lookahead() {
    let (_container, pool, registry) = fixture().await;
    let storage = PostgresStorage::from_pool(pool.clone());
    for (rt, id) in [
        ("Patient", "a"),
        ("Patient", "c"),
        ("Observation", "b"),
        ("Observation", "d"),
    ] {
        storage
            .create(&json!({"resourceType":rt, "id":id}))
            .await
            .unwrap();
    }
    for types in [
        ["Patient", "Observation", "Patient"],
        ["Observation", "Patient", "Observation"],
    ] {
        for (sort, all) in [
            ("_id", vec!["a", "b", "c", "d"]),
            ("-_id", vec!["d", "c", "b", "a"]),
        ] {
            for offset in [0, 1, 2, 3, 4, 99] {
                for total_mode in ["none", "accurate"] {
                    let params = parse_query_string(
                        &format!(
                            "_id=a,b,c,d&_sort={sort}&_count=2&_offset={offset}&_total={total_mode}"
                        ),
                        10,
                        100,
                    );
                    let result = octofhir_db_postgres::queries::search::execute_system_search_raw(
                        &pool,
                        &types,
                        &params,
                        &registry,
                        None,
                        Default::default(),
                    )
                    .await
                    .unwrap();
                    assert_eq!(
                        result
                            .entries
                            .iter()
                            .map(|r| r.id.as_str())
                            .collect::<Vec<_>>(),
                        all.iter().copied().skip(offset).take(2).collect::<Vec<_>>(),
                        "{types:?}: {sort}, {offset}"
                    );
                    assert_eq!(result.total, (total_mode == "accurate").then_some(4));
                    assert_eq!(result.has_more, offset + 2 < 4);
                }
            }
        }
        for control in ["_count=0", "_summary=count&_count=2"] {
            let params = parse_query_string(&format!("{control}&_offset=99&_total=none"), 10, 100);
            let result = octofhir_db_postgres::queries::search::execute_system_search_raw(
                &pool,
                &types,
                &params,
                &registry,
                None,
                Default::default(),
            )
            .await
            .unwrap();
            assert_eq!(result.total, Some(4));
            assert!(result.entries.is_empty());
            assert!(result.included.is_empty());
            assert!(!result.has_more);
        }
    }
}

#[tokio::test]
async fn typed_sorts_order_numbers_and_repeating_values() {
    let (_container, pool, registry) = fixture().await;
    registry.register(
        SearchParameter::new(
            "score",
            "urn:test:score",
            SearchParameterType::Number,
            vec!["Observation".into()],
        )
        .with_expression("Observation.component.valueInteger"),
    );
    let storage = PostgresStorage::from_pool(pool.clone());
    for (id, values) in [
        ("two", vec![2]),
        ("ten", vec![10]),
        ("array", vec![3, 1]),
        ("missing", vec![]),
    ] {
        let component: Vec<_> = values
            .into_iter()
            .map(|v| json!({"code":{"text":"score"}, "valueInteger":v}))
            .collect();
        storage
            .create(&json!({"resourceType":"Observation", "id":id, "component":component}))
            .await
            .unwrap();
    }
    for (sort, expected) in [
        ("score", vec!["array", "two", "ten", "missing"]),
        ("-score", vec!["ten", "array", "two", "missing"]),
    ] {
        for offset in 0..=4 {
            let params = parse_query_string(
                &format!("_sort={sort}&_offset={offset}&_count=2&_total=accurate"),
                10,
                100,
            );
            let result = octofhir_db_postgres::queries::search::execute_search_raw_with_config(
                &pool,
                "Observation",
                &params,
                Some(&registry),
                None,
                None,
            )
            .await
            .unwrap();
            assert_eq!(
                result
                    .entries
                    .iter()
                    .map(|r| r.id.as_str())
                    .collect::<Vec<_>>(),
                expected
                    .iter()
                    .copied()
                    .skip(offset)
                    .take(2)
                    .collect::<Vec<_>>(),
                "{sort}: {offset}"
            );
            assert_eq!(result.total, Some(4));
            assert_eq!(result.has_more, offset + 2 < 4);
        }
    }
    registry.register(
        SearchParameter::new(
            "family",
            "urn:test:family",
            SearchParameterType::String,
            vec!["Patient".into()],
        )
        .with_expression("Patient.name.family"),
    );
    for (id, birthdate, families) in [
        ("a", "2024", vec!["Zulu", "Alpha"]),
        ("b", "2024-01-10", vec!["Beta"]),
        ("c", "2023-01", vec!["Éclair"]),
    ] {
        let name: Vec<_> = families
            .into_iter()
            .map(|family| json!({"family":family}))
            .collect();
        storage
            .create(&json!({"resourceType":"Patient", "id":id, "birthDate":birthdate, "name":name}))
            .await
            .unwrap();
    }
    for (sort, expected) in [
        ("family", vec!["a", "b", "c"]),
        ("-family", vec!["a", "c", "b"]),
        ("birthdate", vec!["c", "a", "b"]),
        ("-birthdate", vec!["a", "b", "c"]),
    ] {
        let params = parse_query_string(&format!("_sort={sort}"), 10, 100);
        let result = octofhir_db_postgres::queries::search::execute_search_raw_with_config(
            &pool,
            "Patient",
            &params,
            Some(&registry),
            None,
            None,
        )
        .await
        .unwrap();
        assert_eq!(
            result
                .entries
                .iter()
                .map(|r| r.id.as_str())
                .collect::<Vec<_>>(),
            expected,
            "{sort}"
        );
    }
}

#[tokio::test]
async fn date_eq_requires_containment_at_each_precision() {
    let (_container, pool, registry) = fixture().await;
    let storage = PostgresStorage::from_pool(pool.clone());
    for (id, date) in [
        ("year", "2024"),
        ("month", "2024-06"),
        ("day", "2024-06-15"),
        ("outside", "2023-12-31"),
        ("deleted", "2024-06-15"),
    ] {
        storage
            .create(&json!({"resourceType":"Patient", "id":id, "birthDate":date}))
            .await
            .unwrap();
    }
    storage
        .create(&json!({"resourceType":"Patient", "id":"missing"}))
        .await
        .unwrap();
    storage.delete("Patient", "deleted").await.unwrap();
    for (query, expected) in [
        ("birthdate=eq2024-06-15", vec!["day"]),
        ("birthdate=2024-06", vec!["day", "month"]),
        ("birthdate=eq2024", vec!["day", "month", "year"]),
        ("birthdate=eq2024-06-16", vec![]),
        ("birthdate=eq2024-06-15,2023-12-31", vec!["day", "outside"]),
    ] {
        assert_search(&pool, &registry, "Patient", query, &expected).await;
    }
    for (id, period) in [
        ("inside", json!({"start":"2024-06-15", "end":"2024-06-15"})),
        ("wide", json!({"start":"2024-01", "end":"2024-12"})),
        ("open-start", json!({"end":"2024-06-15"})),
        ("open-end", json!({"start":"2024-06-15"})),
    ] {
        storage
            .create(&json!({"resourceType":"Encounter", "id":id, "period":period}))
            .await
            .unwrap();
    }
    assert_search(
        &pool,
        &registry,
        "Encounter",
        "date=eq2024-06-15",
        &["inside"],
    )
    .await;
    assert_search(
        &pool,
        &registry,
        "Encounter",
        "date=eq2024",
        &["inside", "wide"],
    )
    .await;
}

#[tokio::test]
async fn repeated_date_bounds_match_independent_occurrences() {
    let (_container, pool, registry) = fixture().await;
    let storage = PostgresStorage::from_pool(pool.clone());
    for (id, dates) in [
        ("split", vec!["2020-01-01", "2030-01-01"]),
        ("inside", vec!["2024-06-15"]),
        ("early", vec!["2020-01-01"]),
        ("late", vec!["2030-01-01"]),
    ] {
        let component: Vec<_> = dates
            .iter()
            .map(|date| json!({"code":{"text":"date"}, "valueDateTime":date}))
            .collect();
        storage
            .create(&json!({"resourceType":"Observation", "id":id, "component":component}))
            .await
            .unwrap();
    }
    for query in [
        "component-date=gt2024-01-01&component-date=lt2025-01-01",
        "component-date=lt2025-01-01&component-date=gt2024-01-01",
        "component-date=ge2024-01-01&component-date=le2025-01-01",
        "component-date=gt2023-01-01&component-date=gt2024-01-01&component-date=lt2025-01-01",
    ] {
        assert_search(&pool, &registry, "Observation", query, &["inside", "split"]).await;
    }
    for query in [
        "component-date=gt2025-01-01&component-date=lt2024-01-01",
        "component-date=lt2024-01-01&component-date=gt2025-01-01",
    ] {
        assert_search(&pool, &registry, "Observation", query, &["split"]).await;
    }
    assert_search(
        &pool,
        &registry,
        "Observation",
        "component-date=eq2024-06-15",
        &["inside"],
    )
    .await;
    storage.create(&json!({"resourceType":"Encounter", "id":"wide", "period":{"start":"2020-01-01", "end":"2030-01-01"}})).await.unwrap();
    assert_search(
        &pool,
        &registry,
        "Encounter",
        "date=gt2025-01-01&date=lt2024-01-01",
        &["wide"],
    )
    .await;
}
