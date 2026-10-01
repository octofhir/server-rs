use octofhir_db_postgres::{PostgresStorage, SchemaManager, migrations};
use octofhir_search::{
    BuiltQuery, ElementTypeHint, SearchParameter, SearchParameterRegistry, SearchParameterType,
    SqlValue, build_native_ir_query_from_params, parameters::SearchParameterComponent,
    parse_query_string, register_common_parameters,
};
use octofhir_storage::FhirStorage;
use serde_json::{Value, json};
use sqlx_core::query_as::query_as;
use sqlx_core::sql_str::AssertSqlSafe;
use sqlx_postgres::{PgPool, PgPoolOptions};

use std::env;

use std::sync::Arc;
use std::time::Instant;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::postgres::Postgres;

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
    >(AssertSqlSafe(query.sql.to_string()));
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

use octofhir_search::{QueryCache, loader::ElementTypeResolver};
use sqlx_core::raw_sql::raw_sql;
use std::path::Path;
use testcontainers::ImageExt;
struct Resolver;
#[async_trait::async_trait]
impl ElementTypeResolver for Resolver {
    async fn resolve(&self, _rt: &str, path: &str) -> Option<(String, bool)> {
        match path {
            "name" => Some(("HumanName".into(), true)),
            "name.family" => Some(("string".into(), false)),
            _ => None,
        }
    }
}
async fn sql(pool: &PgPool, s: &str) {
    raw_sql(AssertSqlSafe(s.to_owned()))
        .execute(pool)
        .await
        .unwrap();
}
fn emit(v: Value) {
    println!("{}", v);
}
async fn record(pool: &PgPool, label: &str, q: &BuiltQuery, out: &Path) {
    let start = Instant::now();
    match octofhir_db_postgres::queries::search::explain_built_search_query_json(pool, q, true)
        .await
    {
        Ok(plan) => {
            std::fs::write(
                out.join(format!("{label}.json")),
                serde_json::to_string_pretty(
                    &json!({"sql":q.sql,"params":format!("{:?}",q.params),"explain":plan}),
                )
                .unwrap(),
            )
            .unwrap();
            emit(
                json!({"case":label,"sql":q.sql,"params":format!("{:?}",q.params),"planning_ms":plan[0]["Planning Time"],"execution_ms":plan[0]["Execution Time"],"rows":plan[0]["Plan"]["Actual Rows"],"buffers_hit":plan[0]["Plan"]["Shared Hit Blocks"],"buffers_read":plan[0]["Plan"]["Shared Read Blocks"]}),
            );
        }
        Err(e) => emit(
            json!({"case":label,"error":e.to_string(),"elapsed_ms":start.elapsed().as_secs_f64()*1000.,"sql":q.sql}),
        ),
    }
}
#[tokio::main]
async fn main() {
    let out_string = env::var("AUDIT_OUTPUT").unwrap();
    let out = Path::new(&out_string);
    let container = Postgres::default().with_tag("18").start().await.unwrap();
    let port = container.get_host_port_ipv4(5432).await.unwrap();
    let url = format!("postgres://postgres:postgres@127.0.0.1:{port}/postgres");
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await
        .unwrap();
    migrations::run(&pool, &url).await.unwrap();
    SchemaManager::ensure_archive_function(&pool).await.unwrap();
    let schema = SchemaManager::new(pool.clone());
    for rt in ["Patient", "Observation"] {
        schema.create_resource_schema(rt).await.unwrap();
    }
    let registry = Arc::new(representative_registry());
    registry.register(
        SearchParameter::new(
            "date",
            "http://hl7.org/fhir/SearchParameter/clinical-date",
            SearchParameterType::Date,
            vec!["Observation".into()],
        )
        .with_expression("Observation.effective"),
    );
    let storage = PostgresStorage::from_pool(pool.clone());
    storage
        .search_registry_slot()
        .set(registry.clone())
        .unwrap();
    seed_representative_data(&storage).await;
    // Bulk synthetic fixtures ONLY inside this newly created disposable container.
    let rows: usize = env::var("AUDIT_ROWS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(20000);
    sql(&pool,&format!(r#"INSERT INTO patient(id,txid,resource,updated_at)
 SELECT 'p-'||i,i,jsonb_build_object('resourceType','Patient','id','p-'||i,'gender',CASE WHEN i%2=0 THEN 'female' ELSE 'male' END,'name',jsonb_build_array(jsonb_build_object('family',CASE WHEN i%10=0 THEN 'Smith' ELSE 'Jones' END)),'birthDate',(1970+i%40)::text||'-03-12','identifier',jsonb_build_array(jsonb_build_object('system','http://hospital.example/mrn','value','mrn-'||i))), '2024-01-01'::timestamptz+i*interval '1 second' FROM generate_series(1,{rows}) i;
 INSERT INTO observation(id,txid,resource,updated_at)
 SELECT 'o-'||i,i,jsonb_build_object('resourceType','Observation','id','o-'||i,'status','final','code',jsonb_build_object('coding',jsonb_build_array(jsonb_build_object('system','http://loinc.org','code',CASE WHEN i%10=0 THEN '8480-6' ELSE '8310-5' END))),'subject',jsonb_build_object('reference','Patient/p-'||i),'valueQuantity',jsonb_build_object('value',CASE WHEN i%10=0 THEN 120 ELSE 80 END,'system','http://unitsofmeasure.org','code','mm[Hg]'),'component',jsonb_build_array(jsonb_build_object('code',jsonb_build_object('coding',jsonb_build_array(jsonb_build_object('system','http://loinc.org','code',CASE WHEN i%10=0 THEN '8480-6' ELSE '8310-5' END))),'valueQuantity',jsonb_build_object('value',CASE WHEN i%10=0 THEN 120 ELSE 80 END,'system','http://unitsofmeasure.org','code','mm[Hg]')))),'2024-01-01'::timestamptz+i*interval '1 second' FROM generate_series(1,{rows}) i;"#)).await;
    let indexed = vec![
        "Patient.family".into(),
        "Patient.birthdate".into(),
        "Observation.subject".into(),
    ];
    let n = octofhir_db_postgres::functional_indexes::create_default_search_indexes(
        &pool, &registry, &indexed, &Resolver,
    )
    .await;
    sql(&pool,"ANALYZE patient; ANALYZE observation; SET statement_timeout='5s'; SET jit=off; SET max_parallel_workers_per_gather=0;").await;
    let (version,): (String,) = query_as(AssertSqlSafe("SELECT version()".to_string()))
        .fetch_one(&pool)
        .await
        .unwrap();
    emit(
        json!({"environment":{"version":version,"rows_per_type":rows,"functional_indexes":n,"concurrency":1,"pool_size":1,"jit":false,"parallel_workers":0,"statement_timeout":"5s","fixture":"synthetic, narrow hand-registered SearchParameter set; actual schema/migrations/index builder"}}),
    );
    for (label, rt, query_text) in [
        (
            "identifier_rare",
            "Patient",
            "identifier=http://hospital.example/mrn|mrn-19990&_count=10",
        ),
        (
            "identifier_empty",
            "Patient",
            "identifier=http://hospital.example/mrn|absent&_count=10",
        ),
        ("family_prefix", "Patient", "family=Smith&_count=10"),
        ("family_exact", "Patient", "family:exact=Smith&_count=10"),
        (
            "birthdate_window",
            "Patient",
            "birthdate=ge1980-01-01&birthdate=le2000-12-31&_count=10",
        ),
        (
            "combined",
            "Patient",
            "family=Smith&gender=female&birthdate=ge1980-01-01&birthdate=le2000-12-31&_count=10",
        ),
        ("gender_common", "Patient", "gender=female&_count=10"),
        (
            "reference",
            "Observation",
            "subject=Patient/p-19990&_count=10",
        ),
        (
            "chain",
            "Observation",
            "subject:Patient.family=Smith&_count=10",
        ),
        (
            "has",
            "Patient",
            "_has:Observation:subject:code=http://loinc.org|8480-6&_count=10",
        ),
        (
            "sort",
            "Patient",
            "gender=female&_sort=-_lastUpdated&_count=10",
        ),
        (
            "deep_offset",
            "Patient",
            "gender=female&_sort=-_lastUpdated&_count=10&_offset=9000",
        ),
        (
            "code",
            "Observation",
            "code=http://loinc.org|8480-6&_count=10",
        ),
        (
            "quantity",
            "Observation",
            "value-quantity=ge100|http://unitsofmeasure.org|mm[Hg]&_count=10",
        ),
        (
            "composite",
            "Observation",
            "code-value-quantity=http://loinc.org|8480-6$ge100|http://unitsofmeasure.org|mm[Hg]&_count=10",
        ),
        ("sort_family", "Patient", "_sort=family&_count=10"),
    ] {
        let mut p = parse_query_string(query_text, 10, 100);
        p.count = Some(p.count.unwrap() + 1);
        let start = Instant::now();
        let converted = build_native_ir_query_from_params(rt, &p, &registry, "public").unwrap();
        let q = converted
            .builder
            .clone()
            .with_raw_resource(true)
            .build()
            .unwrap();
        emit(json!({"case":label,"build_us":start.elapsed().as_micros(),"query":query_text}));
        record(&pool, label, &q, out).await;
        if [
            "identifier_rare",
            "gender_common",
            "family_prefix",
            "reference",
            "sort",
            "deep_offset",
        ]
        .contains(&label)
        {
            let mut times = Vec::new();
            for _ in 0..20 {
                let t = Instant::now();
                match execute_built_query_row_count(&pool, &q).await {
                    Ok(_) => times.push(t.elapsed().as_secs_f64() * 1000.),
                    Err(_) => break,
                }
            }
            times.sort_by(f64::total_cmp);
            emit(
                json!({"case":label,"sql_client_samples_ms":times,"note":"20 sequential executions; includes pool/transport/raw row decode, no HTTP"}),
            );
        }
        if label == "gender_common" {
            record(
                &pool,
                "gender_count",
                &converted.builder.build_count().unwrap(),
                out,
            )
            .await;
        }
    }
    // End-to-end storage paths, including query cache, count and includes (not HTTP).
    let cache = QueryCache::new(100);
    for (label, rt, qs, cached) in [
        (
            "cache_quantity_warm",
            "Observation",
            "value-quantity=ge100&_count=10&_total=accurate",
            true,
        ),
        (
            "cache_quantity_second",
            "Observation",
            "value-quantity=ge200&_count=10&_total=accurate",
            true,
        ),
        (
            "cache_quantity_control",
            "Observation",
            "value-quantity=ge200&_count=10&_total=accurate",
            false,
        ),
        (
            "include",
            "Observation",
            "_id=o-10&_include=Observation:subject",
            false,
        ),
        (
            "include_duplicate",
            "Observation",
            "_id=o-10&_include=Observation:subject&_include=Observation:subject",
            false,
        ),
        (
            "revinclude",
            "Patient",
            "_id=p-10&_revinclude=Observation:subject",
            false,
        ),
        (
            "summary_count",
            "Patient",
            "gender=female&_summary=count",
            false,
        ),
        ("count_zero", "Patient", "gender=female&_count=0", false),
    ] {
        let p = parse_query_string(qs, 10, 100);
        let t = Instant::now();
        match octofhir_db_postgres::queries::search::execute_search_raw_with_config(
            &pool,
            rt,
            &p,
            Some(&registry),
            None,
            if cached { Some(&cache) } else { None },
        )
        .await
        {
            Ok(r) => emit(
                json!({"case":label,"query":qs,"elapsed_ms":t.elapsed().as_secs_f64()*1000.,"entries":r.entries.len(),"included":r.included.len(),"total":r.total,"has_more":r.has_more,"ids":r.entries.iter().map(|r|r.id.clone()).collect::<Vec<_>>()}),
            ),
            Err(e) => emit(json!({"case":label,"error":e.to_string()})),
        }
    }
    // Run actual regexp revinclude predicate with and without redundant DISTINCT.
    let ref_array = "CASE WHEN jsonb_typeof(s.resource->'subject') = 'array' THEN s.resource->'subject' WHEN s.resource->'subject' IS NOT NULL AND s.resource->'subject' != 'null'::jsonb THEN jsonb_build_array(s.resource->'subject') ELSE '[]'::jsonb END";
    for (label, distinct) in [
        ("revinclude_distinct", "DISTINCT "),
        ("revinclude_no_distinct", ""),
    ] {
        let q = BuiltQuery {
            sql: format!(
                "SELECT {distinct}s.resource::text,s.id,s.txid,s.created_at,s.updated_at FROM observation s WHERE s.status != 'deleted' AND EXISTS(SELECT 1 FROM jsonb_array_elements({ref_array}) ref CROSS JOIN LATERAL (SELECT regexp_match(ref->>'reference', '([A-Za-z]+)/([A-Za-z0-9.-]{{1,64}})$') AS m) x WHERE x.m[1]=$1 AND x.m[2]=ANY($2::text[]))"
            ),
            params: vec![
                SqlValue::Text("Patient".into()),
                SqlValue::Text("{p-10}".into()),
            ],
        };
        record(&pool, label, &q, out).await;
    }
    let q=BuiltQuery{sql:"SELECT s.resource::text,s.id,s.txid,s.created_at,s.updated_at FROM observation s WHERE s.status != 'deleted' AND fhir_extract_ref(s.resource, ARRAY['$.\"subject\".\"reference\"[*]'::jsonpath]) && ARRAY[$1]".into(),params:vec![SqlValue::Text("Patient/p-10".into())]};
    record(&pool, "revinclude_local_candidate", &q, out).await;
    // Semantic probes.
    for (label, query_text) in [
        ("unicode", "family=李"),
        ("escaped_comma", r"family=Smith\,Jones"),
        ("count_zero_parse", "_count=0"),
    ] {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let p = parse_query_string(query_text, 10, 100);
            let c = build_native_ir_query_from_params("Patient", &p, &registry, "public").unwrap();
            c.builder.with_raw_resource(true).build().unwrap()
        }));
        emit(json!({"case":label,"build":format!("{:?}",result)}));
    }
    // Compare equivalent shapes for these local, scalar-reference fixtures only.
    let chain_candidate=BuiltQuery{sql:r#"SELECT r.resource::text,r.id,r.txid,r.created_at,r.updated_at FROM observation r WHERE r.status != 'deleted' AND EXISTS (SELECT 1 FROM patient t WHERE t.id=split_part(r.resource->'subject'->>'reference','/',2) AND split_part(r.resource->'subject'->>'reference','/',1)='Patient' AND t.status != 'deleted' AND fhir_text_blob(fhir_s_patient_family(t.resource)) LIKE $1) LIMIT 12"#.into(),params:vec![SqlValue::Text("% smith%".into())]};
    record(&pool, "chain_local_scalar_candidate", &chain_candidate, out).await;
    // Flush bulk-load GIN pending entries via normal maintenance in this fixture.
    sql(&pool, "VACUUM ANALYZE patient").await;
    sql(&pool, "VACUUM ANALYZE observation").await;
    sql(&pool,r#"PREPARE audit_token(jsonb) AS SELECT r.resource::text,r.id,r.txid,r.created_at,r.updated_at FROM patient r WHERE r.resource @> $1 AND r.status != 'deleted' LIMIT 12"#).await;
    for mode in ["force_custom_plan", "force_generic_plan"] {
        sql(&pool, &format!("SET plan_cache_mode={mode}")).await;
        for (label, arg) in [
            (
                "identifier_clean",
                r#"{"identifier":[{"system":"http://hospital.example/mrn","value":"mrn-19990"}]}"#,
            ),
            ("gender_clean", r#"{"gender":"female"}"#),
        ] {
            let q = BuiltQuery {
                sql: format!("EXECUTE audit_token('{}'::jsonb)", arg.replace('\'', "''")),
                params: vec![],
            };
            record(&pool, &format!("{label}_{mode}"), &q, out).await;
        }
    }
    sql(&pool, "SET plan_cache_mode=auto").await;
    let extra_indexes = vec![
        "Observation.code".into(),
        "Observation.value-quantity".into(),
    ];
    let ix_started = Instant::now();
    let extra = octofhir_db_postgres::functional_indexes::create_default_search_indexes(
        &pool,
        &registry,
        &extra_indexes,
        &Resolver,
    )
    .await;
    emit(
        json!({"additional_fixture_indexes":extra,"build_elapsed_ms":ix_started.elapsed().as_secs_f64()*1000.}),
    );
    sql(&pool, "ANALYZE observation;").await;
    for (label, qs) in [
        ("code_indexed", "code=http://loinc.org|8480-6&_count=10"),
        (
            "quantity_indexed",
            "value-quantity=ge100|http://unitsofmeasure.org|mm[Hg]&_count=10",
        ),
    ] {
        let p = parse_query_string(qs, 10, 100);
        let c = build_native_ir_query_from_params("Observation", &p, &registry, "public").unwrap();
        record(
            &pool,
            label,
            &c.builder.clone().with_raw_resource(true).build().unwrap(),
            out,
        )
        .await;
        record(
            &pool,
            &format!("{label}_count"),
            &c.builder.build_count().unwrap(),
            out,
        )
        .await;
    }
    let sizes:Vec<(String,i64)>=query_as(AssertSqlSafe("SELECT indexrelname::text,pg_relation_size(indexrelid)::bigint FROM pg_stat_user_indexes WHERE relname IN ('patient','observation') ORDER BY indexrelname".to_string())).fetch_all(&pool).await.unwrap();
    emit(json!({"fixture_index_bytes":sizes}));
    // Targeted semantic fixtures, seeded only AFTER the performance baseline.
    for res in [
        json!({"resourceType":"Patient","id":"sem-year","birthDate":"1985","name":[{"family":"Jon Smith"}]}),
        json!({"resourceType":"Patient","id":"sem-name","name":[{"family":"Smith"},{"family":"Jones"}]}),
        json!({"resourceType":"Observation","id":"sem-timing","status":"final","code":{"text":"fixture"},"effectiveTiming":{"event":["2020-01-01T00:00:00Z","2030-01-01T00:00:00Z"]}}),
        json!({"resourceType":"Observation","id":"sem-period","status":"final","code":{"text":"fixture"},"effectivePeriod":{"end":"2020-12-31"}}),
    ] {
        storage.create(&res).await.unwrap();
    }
    for (label, rt, qs) in [
        (
            "date_scalar_eq",
            "Patient",
            "_id=sem-year&birthdate=1985-03-12",
        ),
        (
            "date_repeated_ge_le",
            "Observation",
            "_id=sem-timing&date=ge2025&date=le2026",
        ),
        (
            "date_repeated_gt_lt",
            "Observation",
            "_id=sem-timing&date=gt2025&date=lt2026",
        ),
        (
            "date_open_period",
            "Observation",
            "_id=sem-period&date=lt2010",
        ),
        ("name_word_prefix", "Patient", "_id=sem-year&family=Smith"),
        (
            "name_cross_values",
            "Patient",
            "_id=sem-name&family:contains=Smith%20Jones",
        ),
    ] {
        let p = parse_query_string(qs, 10, 100);
        let r = octofhir_db_postgres::queries::search::execute_search_raw_with_config(
            &pool,
            rt,
            &p,
            Some(&registry),
            None,
            None,
        )
        .await;
        emit(
            json!({"case":label,"query":qs,"result":r.as_ref().map(|r|r.entries.iter().map(|x|x.id.clone()).collect::<Vec<_>>()).map_err(|e|e.to_string())}),
        );
    }
    // The chain parser is distinct from the ordinary UTF-8-safe conversion path.
    let chained_unicode = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let p = parse_query_string("subject:Patient.family=李", 10, 100);
        build_native_ir_query_from_params("Observation", &p, &registry, "public")
    }));
    emit(json!({"case":"chained_unicode","panicked":chained_unicode.is_err()}));

    pool.close().await;
}
