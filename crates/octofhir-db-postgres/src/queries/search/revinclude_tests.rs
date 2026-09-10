use super::*;
use testcontainers::{ImageExt, runners::AsyncRunner};
use testcontainers_modules::postgres::Postgres;

const REF_ARRAY: &str = "CASE WHEN jsonb_typeof(s.resource->'subject') = 'array' THEN s.resource->'subject' WHEN s.resource->'subject' IS NULL THEN '[]'::jsonb ELSE jsonb_build_array(s.resource->'subject') END";

fn legacy_sql() -> String {
    format!(
        r#"SELECT DISTINCT s.resource::text, s.id, s.txid, s.created_at, s.updated_at
        FROM "observation" s WHERE s.status != 'deleted'
        AND EXISTS (SELECT 1 FROM jsonb_array_elements({REF_ARRAY}) AS ref
          CROSS JOIN LATERAL (SELECT regexp_match(ref->>'reference', '{REFERENCE_TYPE_ID_RE}') AS m) x
          WHERE x.m[1] = $1 AND x.m[2] = ANY($2::text[]))"#
    )
}

#[test]
fn revinclude_guards_regexp_and_does_not_multiply_source_rows() {
    for raw in [false, true] {
        let sql = build_revinclude_sql("observation", REF_ARRAY, raw);
        assert!(sql.contains("CASE WHEN split_part"));
        assert!(sql.contains("THEN (regexp_match"));
        assert!(sql.contains(REFERENCE_TYPE_ID_RE));
        assert!(sql.contains("s.status != 'deleted'"));
        assert!(sql.contains("AND EXISTS"));
        assert!(!sql.contains("DISTINCT"));
        assert_eq!(sql.contains("s.resource::text"), raw);
    }
}

#[tokio::test]
#[ignore = "manual isolated PostgreSQL revinclude SQL A/B benchmark"]
async fn revinclude_sql_ab() {
    let container = Postgres::default()
        .with_tag("16-alpine")
        .start()
        .await
        .unwrap();
    let port = container.get_host_port_ipv4(5432).await.unwrap();
    let url = format!("postgres://postgres:postgres@127.0.0.1:{port}/postgres");
    let pool = sqlx_postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await
        .unwrap();
    crate::migrations::run(&pool, &url).await.unwrap();
    SchemaManager::ensure_archive_function(&pool).await.unwrap();
    SchemaManager::new(pool.clone())
        .create_resource_schema("Observation")
        .await
        .unwrap();
    struct UnusedResolver;
    #[async_trait::async_trait]
    impl octofhir_search::loader::ElementTypeResolver for UnusedResolver {
        async fn resolve(&self, _: &str, _: &str) -> Option<(String, bool)> {
            panic!("reference indexes do not require element type resolution");
        }
    }
    let registry = SearchParameterRegistry::new();
    registry.register(
        octofhir_search::SearchParameter::new(
            "subject",
            "urn:test:subject",
            octofhir_search::SearchParameterType::Reference,
            vec!["Observation".into()],
        )
        .with_expression("Observation.subject")
        .with_targets(vec!["Patient".into()]),
    );
    assert_eq!(
        crate::functional_indexes::create_default_search_indexes(
            &pool,
            &registry,
            &["Observation.subject".into()],
            &UnusedResolver,
        )
        .await,
        1
    );
    sqlx_core::raw_sql::raw_sql(AssertSqlSafe(r#"
        INSERT INTO observation(id,txid,resource) SELECT 'o-'||i,i,jsonb_build_object(
            'resourceType','Observation','id','o-'||i,'subject',jsonb_build_object(
            'reference',CASE WHEN i%3=0 THEN 'https://remote.example/fhir/' WHEN i%3=1 THEN '' ELSE 'https://local.example/fhir/' END || 'Patient/p-'||i)) FROM generate_series(1,20000) i;
        ANALYZE observation;
        SET jit=off; SET max_parallel_workers_per_gather=0; SET statement_timeout='10s';
    "#.to_owned())).execute(&pool).await.unwrap();
    let legacy = legacy_sql();
    let fresh = build_revinclude_sql("observation", REF_ARRAY, true);
    for count in [1, 10, 100] {
        let ids: Vec<_> = (1..=count).map(|i| format!("p-{i}")).collect();
        let expected: HashSet<_> = (1..=count).map(|i| format!("o-{i}")).collect();
        for candidate in [&legacy, &fresh] {
            let result: Vec<String> = query_scalar(AssertSqlSafe(format!(
                "SELECT id FROM ({candidate}) matches"
            )))
            .bind("Patient")
            .bind(&ids)
            .fetch_all(&pool)
            .await
            .unwrap();
            assert_eq!(result.len(), expected.len());
            assert_eq!(result.into_iter().collect::<HashSet<_>>(), expected);
        }
        for sample in 0..6 {
            let variants = if sample % 2 == 0 {
                [("legacy", &legacy), ("new", &fresh)]
            } else {
                [("new", &fresh), ("legacy", &legacy)]
            };
            for (variant, candidate) in variants {
                let plan: Value = query_scalar(AssertSqlSafe(format!(
                    "EXPLAIN (ANALYZE, BUFFERS, FORMAT JSON) {candidate}"
                )))
                .bind("Patient")
                .bind(&ids)
                .fetch_one(&pool)
                .await
                .unwrap();
                assert_eq!(plan[0]["Plan"]["Actual Rows"], count);
                if sample > 0 {
                    println!(
                        "revinclude targets={count} {variant} sample={sample} execution_ms={} plan={}",
                        plan[0]["Execution Time"], plan[0]["Plan"]
                    );
                }
            }
        }
    }
    pool.close().await;
}
