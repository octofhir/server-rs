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
        .with_tag("17-alpine")
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
async fn linked_patient_compartment_is_directed_transitive_and_intersects_grants() {
    use octofhir_storage::{SearchConstraint, SearchParams};
    use std::collections::HashMap;
    let (_container, pool, registry) = setup().await;
    registry.register(
        SearchParameter::new(
            "link",
            "urn:test:Patient:link",
            SearchParameterType::Reference,
            vec!["Patient".into()],
        )
        .with_expression("Patient.link.other")
        .with_targets(vec!["Patient".into(), "RelatedPerson".into()]),
    );
    let storage = PostgresStorage::from_pool(pool.clone());
    for patient in [
        json!({"resourceType":"Patient","id":"target"}),
        json!({"resourceType":"Patient","id":"source","link":[{"other":{"reference":"Patient/target/_history/1"},"type":"replaced-by"}]}),
        json!({"resourceType":"Patient","id":"ancestor","link":[{"other":{"reference":"https://local.test/fhir/Patient/source"},"type":"refer"}]}),
        json!({"resourceType":"Patient","id":"cycle","link":[{"other":{"reference":"Patient/ancestor"},"type":"seealso"},{"other":{"reference":"Patient/cycle"},"type":"seealso"}]}),
        json!({"resourceType":"Patient","id":"foreign","link":[{"other":{"reference":"https://foreign.test/fhir/Patient/target"},"type":"replaced-by"}]}),
    ] {
        storage.create(&patient).await.unwrap();
        let id = patient["id"].as_str().unwrap();
        storage.create(&json!({"resourceType":"Observation","id":id,"subject":{"reference":format!("Patient/{id}")}})).await.unwrap();
    }
    let membership = |id: &str, rt: &str| SearchConstraint::LinkedPatient {
        patient_id: id.into(),
        fhir_base_url: "https://local.test/fhir".into(),
        parameters: if rt == "Patient" {
            vec!["link".into()]
        } else {
            vec!["subject".into()]
        },
    };
    for rt in ["Patient", "Observation"] {
        for (id, expected) in [
            ("target", vec!["ancestor", "cycle", "source", "target"]),
            ("source", vec!["ancestor", "cycle", "source"]),
        ] {
            let mut params = SearchParams::new()
                .with_count(2)
                .with_total(octofhir_storage::TotalMode::Accurate);
            params.constraint = Some(membership(id, rt));
            let page = octofhir_db_postgres::queries::search::execute_search_raw_with_config(
                &pool,
                rt,
                &params,
                Some(&registry),
                None,
                None,
            )
            .await
            .unwrap();
            assert_eq!(page.total, Some(expected.len() as u32));
            assert_eq!(page.entries.len(), 2);
            assert!(page.has_more);
            params.count = Some(100);
            let all = octofhir_db_postgres::queries::search::execute_search_raw_with_config(
                &pool,
                rt,
                &params,
                Some(&registry),
                None,
                None,
            )
            .await
            .unwrap();
            assert_eq!(
                all.entries
                    .iter()
                    .map(|r| r.id.as_str())
                    .collect::<BTreeSet<_>>(),
                expected.into_iter().collect()
            );
        }
    }
    // Multiple roots share one recursive search and deduplicate overlapping
    // closures before counts and pagination.
    for resource_type in ["Patient", "Observation"] {
        let mut grouped = SearchParams::new()
            .with_count(100)
            .with_total(octofhir_storage::TotalMode::Accurate);
        grouped.constraint = Some(SearchConstraint::LinkedPatients {
            patient_ids: vec![
                "target".into(),
                "source".into(),
                "foreign".into(),
                "target".into(),
            ],
            fhir_base_url: "https://local.test/fhir".into(),
            parameters: vec![
                if resource_type == "Patient" {
                    "link"
                } else {
                    "subject"
                }
                .into(),
            ],
        });
        let result = octofhir_db_postgres::queries::search::execute_search_raw_with_config(
            &pool,
            resource_type,
            &grouped,
            Some(&registry),
            None,
            None,
        )
        .await
        .unwrap();
        assert_eq!(result.total, Some(5));
        assert_eq!(
            result
                .entries
                .iter()
                .map(|resource| resource.id.as_str())
                .collect::<BTreeSet<_>>(),
            ["ancestor", "cycle", "foreign", "source", "target"]
                .into_iter()
                .collect()
        );
    }
    // Patient-level export uses internal membership without enumerating Patient
    // snapshots or conferring a Patient read/search grant. Orphans and foreign
    // references are excluded; current resource grants still intersect the set.
    for (id, reference) in [
        ("orphan-record", "Patient/missing"),
        ("foreign-record", "https://foreign.test/fhir/Patient/target"),
    ] {
        storage
            .create(&json!({"resourceType":"Observation","id":id,
            "subject":{"reference":reference}}))
            .await
            .unwrap();
    }
    for rt in ["Patient", "Observation"] {
        let mut params = SearchParams::new()
            .with_count(100)
            .with_total(octofhir_storage::TotalMode::Accurate);
        params.constraint = Some(SearchConstraint::AllPatients {
            fhir_base_url: "https://local.test/fhir".into(),
            parameters: vec!["subject".into()],
        });
        let result = octofhir_db_postgres::queries::search::execute_search_raw_with_config(
            &pool,
            rt,
            &params,
            Some(&registry),
            None,
            None,
        )
        .await
        .unwrap();
        assert_eq!(result.total, Some(5));
        assert_eq!(
            result
                .entries
                .iter()
                .map(|resource| resource.id.as_str())
                .collect::<BTreeSet<_>>(),
            ["ancestor", "cycle", "foreign", "source", "target"]
                .into_iter()
                .collect()
        );
        params.constraint = Some(SearchConstraint::And(vec![
            params.constraint.take().unwrap(),
            SearchConstraint::Query(HashMap::from([("_id".into(), vec!["target".into()])])),
        ]));
        let result = octofhir_db_postgres::queries::search::execute_search_raw_with_config(
            &pool,
            rt,
            &params,
            Some(&registry),
            None,
            None,
        )
        .await
        .unwrap();
        assert_eq!(result.total, Some(1));
        assert_eq!(result.entries[0].id, "target");
    }
    // A compartment expansion cannot expand the launch patient's clinical grant.
    let mut params = SearchParams::new()
        .with_count(100)
        .with_total(octofhir_storage::TotalMode::Accurate);
    params.constraint = Some(SearchConstraint::And(vec![
        membership("target", "Observation"),
        SearchConstraint::Query(HashMap::from([(
            "subject".into(),
            vec!["Patient/target".into()],
        )])),
    ]));
    let own = octofhir_db_postgres::queries::search::execute_search_raw_with_config(
        &pool,
        "Observation",
        &params,
        Some(&registry),
        None,
        None,
    )
    .await
    .unwrap();
    assert_eq!(own.total, Some(1));
    assert_eq!(own.entries[0].id, "target");
}

#[tokio::test]
async fn trusted_visibility_precedes_pagination_and_counts() {
    use octofhir_storage::SearchConstraint;
    use std::collections::HashMap;
    let (_container, pool, registry) = setup().await;
    let storage = PostgresStorage::from_pool(pool.clone());
    // An IG canonical must retain the logical ID semantics in both the
    // client's query and trusted grant predicates, even without a type hint.
    for resource_type in ["Patient", "Observation"] {
        registry.register(
            SearchParameter::new(
                "_id",
                format!("https://ig.example/SearchParameter/{resource_type}-id"),
                SearchParameterType::Token,
                vec![resource_type.into()],
            )
            .with_expression(if resource_type == "Patient" {
                "id".into()
            } else {
                format!("{resource_type}.id")
            }),
        );
    }
    registry.register(
        SearchParameter::new(
            "active",
            "urn:test:Patient:active",
            SearchParameterType::Token,
            vec!["Patient".into()],
        )
        .with_expression("Patient.active")
        .with_element_type_hint(ElementTypeHint::from_fhir_type("boolean", false)),
    );
    for (id, active) in [
        ("active", Some(true)),
        ("inactive", Some(false)),
        ("unset", None),
    ] {
        let mut patient = json!({"resourceType":"Patient", "id":id});
        if let Some(active) = active {
            patient["active"] = json!(active);
        }
        storage.create(&patient).await.unwrap();
    }
    assert_search(&pool, &registry, "Patient", "active=true", &["active"]).await;
    assert_search(&pool, &registry, "Patient", "_id=active", &["active"]).await;
    assert_search(&pool, &registry, "Patient", "active=false", &["inactive"]).await;
    assert_search(
        &pool,
        &registry,
        "Patient",
        "active:missing=true",
        &["unset"],
    )
    .await;
    for (id, reference, code) in [
        ("a", "Patient/p1", "lab"),
        ("b", "Patient/p1", "lab"),
        ("c", "Patient/p1", "vital"),
        ("d", "Patient/p2", "lab"),
        ("e", "https://foreign.test/fhir/Patient/p1", "lab"),
        ("f", "Practitioner/p1", "lab"),
        ("g", "https://local.test/fhir/Patient/p1/_history/7", "lab"),
        ("h", "Patient/p1/_history/3", "lab"),
    ] {
        storage
            .create(&json!({"resourceType":"Observation", "id":id,
                "subject":{"reference":reference}, "code":{"coding":[{"code":code}]}
            }))
            .await
            .unwrap();
    }
    let predicate = |name: &str, value: &str| {
        SearchConstraint::Query(HashMap::from([(name.into(), vec![value.into()])]))
    };
    let constraint = SearchConstraint::Or(vec![
        SearchConstraint::And(vec![
            predicate("subject", "Patient/p1,https://local.test/fhir/Patient/p1"),
            predicate("code", "lab"),
        ]),
        predicate("_id", "d"),
    ]);
    let mut params = parse_query_string("_count=2&_total=accurate&_sort=_id", 10, 100);
    params.constraint = Some(constraint);
    let mut observed = BTreeSet::new();
    for offset in [0, 2, 4] {
        params.offset = Some(offset);
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
        assert_eq!(result.total, Some(5));
        assert_eq!(result.has_more, offset < 4);
        assert_eq!(result.entries.len(), if offset < 4 { 2 } else { 1 });
        observed.extend(result.entries.into_iter().map(|entry| entry.id));
    }
    assert_eq!(
        observed,
        ["a", "b", "d", "g", "h"]
            .into_iter()
            .map(String::from)
            .collect()
    );

    params.count = Some(0);
    let mut transaction = pool.begin().await.unwrap();
    let result = octofhir_db_postgres::queries::search::execute_search_with_tx(
        &mut transaction,
        "Observation",
        &params,
        Some(&registry),
    )
    .await
    .unwrap();
    assert_eq!(result.total, Some(5));
    assert!(result.entries.is_empty());
    transaction.rollback().await.unwrap();

    params.constraint = Some(SearchConstraint::Or(Vec::new()));
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
    assert_eq!(result.total, Some(0));
}

#[tokio::test]
async fn inline_visibility_filters_each_hop_before_limits() {
    use octofhir_storage::{IncludeVisibility, SearchConstraint};
    use std::collections::HashMap;
    let (_container, pool, registry) = setup().await;
    for (name, path, targets) in [
        ("derived", "Observation.derivedFrom", vec!["Observation"]),
        ("performer", "Observation.performer", vec!["Organization"]),
    ] {
        registry.register(
            SearchParameter::new(
                name,
                format!("urn:test:{name}"),
                SearchParameterType::Reference,
                vec!["Observation".into()],
            )
            .with_expression(path)
            .with_targets(targets.into_iter().map(String::from).collect()),
        );
    }
    let storage = PostgresStorage::from_pool(pool.clone());
    for resource in [
        json!({"resourceType":"Organization", "id":"org1"}),
        json!({"resourceType":"Organization", "id":"org2"}),
        json!({"resourceType":"Patient", "id":"p1", "managingOrganization":{"reference":"Organization/org1"}}),
        json!({"resourceType":"Patient", "id":"p2", "managingOrganization":{"reference":"Organization/org2"}}),
        json!({"resourceType":"Observation", "id":"root", "subject":{"reference":"Patient/p1"}, "code":{"coding":[{"code":"lab"}]},
            "derivedFrom":[{"reference":"Observation/peer"},{"reference":"Observation/denied"},{"reference":"https://foreign.test/fhir/Observation/alias"}]}),
        json!({"resourceType":"Observation", "id":"peer", "subject":{"reference":"Patient/p1"}, "code":{"coding":[{"code":"lab"}]},
            "performer":[{"reference":"Organization/org1"}]}),
        json!({"resourceType":"Observation", "id":"denied", "subject":{"reference":"Patient/p2"}, "code":{"coding":[{"code":"lab"}]},
            "performer":[{"reference":"Organization/org2"}]}),
        json!({"resourceType":"Observation", "id":"alias", "subject":{"reference":"Patient/p1"}, "code":{"coding":[{"code":"lab"}]}}),
        json!({"resourceType":"Observation", "id":"vital", "subject":{"reference":"Patient/p1"}, "code":{"coding":[{"code":"vital"}]}}),
        json!({"resourceType":"Observation", "id":"foreign", "subject":{"reference":"https://foreign.test/fhir/Patient/p1"}, "code":{"coding":[{"code":"lab"}]}}),
        json!({"resourceType":"Observation", "id":"deleted", "subject":{"reference":"Patient/p1"}, "code":{"coding":[{"code":"lab"}]}}),
    ] {
        storage.create(&resource).await.unwrap();
    }
    storage.delete("Observation", "deleted").await.unwrap();
    let predicate = |key: &str, value: &str| {
        SearchConstraint::Query(HashMap::from([(key.into(), vec![value.into()])]))
    };
    let visibility = IncludeVisibility {
        constraints: HashMap::from([
            (
                "Observation".into(),
                SearchConstraint::And(vec![
                    predicate("subject", "Patient/p1"),
                    predicate("code", "lab"),
                ]),
            ),
            ("Patient".into(), predicate("_id", "p1")),
            ("Organization".into(), SearchConstraint::And(Vec::new())),
        ]),
        fhir_base_url: Some("https://local.test/fhir".into()),
    };
    for (rt, query, expected) in [
        (
            "Observation",
            "_id=root&_include:iterate=Observation:derived&_include:iterate=Observation:subject:Patient&_include:iterate=Observation:performer&_include:iterate=Patient:organization",
            vec![
                ("Observation", "peer"),
                ("Patient", "p1"),
                ("Organization", "org1"),
            ],
        ),
        (
            "Patient",
            "_id=p1&_revinclude=Observation:subject&_include:iterate=Observation:performer",
            vec![
                ("Observation", "root"),
                ("Observation", "peer"),
                ("Observation", "alias"),
                ("Organization", "org1"),
            ],
        ),
    ] {
        let mut params = parse_query_string(&format!("{query}&_count=1&_total=accurate"), 10, 100);
        params.included_visibility = Some(visibility.clone());
        let raw = octofhir_db_postgres::queries::search::execute_search_raw_with_options(
            &pool,
            rt,
            &params,
            Some(&registry),
            None,
            octofhir_db_postgres::queries::search::RawSearchOptions {
                include_limits: octofhir_db_postgres::queries::search::IncludeLimits {
                    max_resources: expected.len(),
                    ..Default::default()
                },
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let parsed = octofhir_db_postgres::queries::search::execute_search(
            &pool,
            rt,
            &params,
            Some(&registry),
        )
        .await
        .unwrap();
        let expected: BTreeSet<_> = expected.into_iter().collect();
        assert_eq!(
            raw.included
                .iter()
                .map(|e| (e.resource_type.as_str(), e.id.as_str()))
                .collect::<BTreeSet<_>>(),
            expected,
            "{query}"
        );
        assert_eq!(
            parsed
                .entries
                .iter()
                .skip(1)
                .map(|e| (e.resource_type.as_str(), e.id.as_str()))
                .collect::<BTreeSet<_>>(),
            expected,
            "{query}"
        );
        assert_eq!(raw.total, Some(1));
        assert_eq!(parsed.total, Some(1));
        assert_eq!(raw.entries.len(), 1);
        assert!(!raw.has_more);
        params.included_visibility = Some(IncludeVisibility::default());
        let empty = octofhir_db_postgres::queries::search::execute_search_raw_with_config(
            &pool,
            rt,
            &params,
            Some(&registry),
            None,
            None,
        )
        .await
        .unwrap();
        assert!(empty.included.is_empty());
        assert_eq!(empty.total, Some(1));
    }
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
    sql(&pool, "UPDATE patient SET txid=nextval('_transaction_txid_seq'), status='deleted' WHERE id='deleted'; UPDATE observation SET txid=nextval('_transaction_txid_seq'), status='deleted' WHERE id='deleted-source'").await;
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
        "UPDATE organization SET txid=nextval('_transaction_txid_seq'), status='deleted' WHERE id='org'",
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

#[tokio::test]
async fn aggregate_visibility_precedes_global_pagination_and_inline_expansion() {
    use octofhir_db_postgres::queries::search::{RawSearchOptions, execute_system_search_raw};
    use octofhir_storage::{IncludeVisibility, SearchConstraint};
    use std::collections::HashMap;
    let (_container, pool, registry) = setup().await;
    let storage = PostgresStorage::from_pool(pool.clone());
    for id in ["own", "other"] {
        storage
            .create(&json!({"resourceType":"Patient", "id":id}))
            .await
            .unwrap();
    }
    for (id, subject, code) in [
        ("a", "Patient/own", "lab"),
        ("b", "https://local.test/fhir/Patient/own", "lab"),
        ("foreign", "Patient/other", "lab"),
        ("vital", "Patient/own", "vital"),
    ] {
        storage
            .create(&json!({"resourceType":"Observation", "id":id,
            "code":{"coding":[{"code":code}]}, "subject":{"reference":subject}}))
            .await
            .unwrap();
    }
    storage
        .create(&json!({"resourceType":"Organization", "id":"own"}))
        .await
        .unwrap();
    let query = |key: &str, value: &str| {
        SearchConstraint::Query(HashMap::from([(key.into(), vec![value.into()])]))
    };
    let mut params = parse_query_string(
        "_count=1&_total=accurate&_include=Observation:subject:Patient",
        10,
        100,
    );
    params.type_constraints = Some(HashMap::from([
        ("Patient".into(), query("_id", "own")),
        (
            "Observation".into(),
            SearchConstraint::And(vec![
                query("subject", "Patient/own,https://local.test/fhir/Patient/own"),
                query("code", "lab"),
            ]),
        ),
    ]));
    params.included_visibility = Some(IncludeVisibility {
        constraints: HashMap::from([("Patient".into(), query("_id", "own"))]),
        fhir_base_url: Some("https://local.test/fhir".into()),
    });
    // Duplicate types must neither duplicate rows nor inflate the global total.
    let types = ["Patient", "Observation", "Organization", "Patient"];
    for (offset, expected_type, expected_id) in [
        (0, "Observation", "a"),
        (1, "Observation", "b"),
        (2, "Patient", "own"),
    ] {
        params.offset = Some(offset);
        let result = execute_system_search_raw(
            &pool,
            &types,
            &params,
            &registry,
            None,
            RawSearchOptions::default(),
        )
        .await
        .unwrap();
        assert_eq!(result.total, Some(3));
        assert_eq!(result.entries.len(), 1);
        assert_eq!(result.entries[0].resource_type, expected_type);
        assert_eq!(result.entries[0].id, expected_id);
        assert_eq!(result.has_more, offset < 2);
        assert_eq!(
            result.included.len(),
            usize::from(expected_type == "Observation")
        );
        if let Some(included) = result.included.first() {
            assert_eq!(
                (included.resource_type.as_str(), included.id.as_str()),
                ("Patient", "own")
            );
        }
    }
    params.offset = Some(0);
    params
        .included_visibility
        .as_mut()
        .unwrap()
        .constraints
        .clear();
    let result = execute_system_search_raw(
        &pool,
        &types,
        &params,
        &registry,
        None,
        RawSearchOptions::default(),
    )
    .await
    .unwrap();
    assert_eq!(result.total, Some(3));
    assert!(result.included.is_empty());
    params.constraint = Some(query("_id", "a,own"));
    params.count = Some(0);
    let result = execute_system_search_raw(
        &pool,
        &types,
        &params,
        &registry,
        None,
        RawSearchOptions::default(),
    )
    .await
    .unwrap();
    assert_eq!(result.total, Some(2));
    assert!(result.entries.is_empty());
    params.type_constraints = Some(HashMap::new());
    let result = execute_system_search_raw(
        &pool,
        &types,
        &params,
        &registry,
        None,
        RawSearchOptions::default(),
    )
    .await
    .unwrap();
    assert_eq!(result.total, Some(0));
    // Lenient conversion cannot silently apply a known Patient-only criterion
    // to the Patient branch while ignoring it for Observation.
    params
        .parameters
        .insert("family".into(), vec!["Allowed".into()]);
    assert!(
        execute_system_search_raw(
            &pool,
            &types,
            &params,
            &registry,
            None,
            RawSearchOptions::default()
        )
        .await
        .is_err()
    );
}

#[tokio::test]
async fn transactional_aggregate_and_inline_search_use_one_connection() {
    use octofhir_storage::{IncludeLimits, IncludeVisibility, SearchConstraint};
    use std::collections::HashMap;
    use std::time::Duration;
    let (_container, pool, registry) = setup().await;
    let storage = PostgresStorage::from_pool(pool.clone());
    storage.search_registry_slot().set(registry).unwrap();
    storage
        .create(&json!({"resourceType":"Patient", "id":"other"}))
        .await
        .unwrap();
    // The fixture deliberately has just one pool connection: any fallback to
    // pool search/include deadlocks, and cannot see these uncommitted writes.
    let mut tx = storage.begin_transaction().await.unwrap();
    tx.create(&json!({"resourceType":"Patient", "id":"own"}))
        .await
        .unwrap();
    for (id, subject, code) in [
        ("lab", "own", "lab"),
        ("vital", "own", "vital"),
        ("foreign", "other", "lab"),
    ] {
        tx.create(&json!({"resourceType":"Observation", "id":id,
            "subject":{"reference":format!("Patient/{subject}")},
            "code":{"coding":[{"code":code}]}}))
            .await
            .unwrap();
    }
    let query = |key: &str, value: &str| {
        SearchConstraint::Query(HashMap::from([(key.into(), vec![value.into()])]))
    };
    let observation =
        SearchConstraint::And(vec![query("subject", "Patient/own"), query("code", "lab")]);
    let constraints = HashMap::from([
        ("Patient".into(), query("_id", "own")),
        ("Observation".into(), observation),
    ]);
    let mut params = parse_query_string(
        "_count=1&_total=accurate&_include:iterate=Observation:subject:Patient&_revinclude:iterate=Observation:subject:Patient",
        10,
        100,
    );
    params.type_constraints = Some(constraints.clone());
    params.included_visibility = Some(IncludeVisibility {
        constraints,
        fhir_base_url: Some("https://local.test/fhir".into()),
    });
    let search = tx.search_system_raw(
        &["Patient", "Observation"],
        &params,
        IncludeLimits::default(),
        false,
    );
    let result = tokio::time::timeout(Duration::from_secs(5), search)
        .await
        .expect("search checked out another connection")
        .unwrap();
    assert_eq!(result.total, Some(2));
    assert!(result.has_more);
    assert_eq!(result.entries[0].id, "lab");
    assert_eq!(result.included.len(), 1);
    assert_eq!(
        (
            result.included[0].resource_type.as_str(),
            result.included[0].id.as_str()
        ),
        ("Patient", "own")
    );
    params.offset = Some(1);
    let result = tx
        .search_system_raw(
            &["Patient", "Observation"],
            &params,
            IncludeLimits::default(),
            false,
        )
        .await
        .unwrap();
    assert_eq!(result.total, Some(2));
    assert!(!result.has_more);
    assert_eq!(result.entries[0].id, "own");
    assert_eq!(result.included.len(), 1);
    assert_eq!(result.included[0].id, "lab");
    params.count = Some(0);
    let result = tx
        .search_system_raw(
            &["Patient", "Observation"],
            &params,
            IncludeLimits::default(),
            false,
        )
        .await
        .unwrap();
    assert_eq!(result.total, Some(2));
    assert!(result.entries.is_empty());
    assert!(result.included.is_empty());
    // Independent native predicates retain their own cardinality and grants,
    // ignore result controls, and observe own writes with this one connection.
    let mut scoped = parse_query_string("_count=0&_offset=999&_summary=count", 10, 100);
    scoped.constraint = Some(SearchConstraint::And(vec![
        query("subject", "Patient/own"),
        query("code", "lab"),
    ]));
    let mut hidden = scoped.clone();
    hidden.constraint = Some(query("_id", "missing"));
    let mut other = scoped.clone();
    other.constraint = Some(query("_id", "foreign"));
    let mut predicates = vec![
        ("Observation".into(), scoped),
        ("Observation".into(), hidden),
        ("Observation".into(), other),
        ("Observation".into(), parse_query_string("", 1, 1)),
    ];
    predicates.extend((0..130).map(|_| ("Patient".into(), parse_query_string("_id=own", 1, 1))));
    let result = tokio::time::timeout(
        Duration::from_secs(5),
        tx.search_conditional_batch(&predicates),
    )
    .await
    .expect("conditional batch checked out a second connection")
    .unwrap();
    assert_eq!(result.len(), predicates.len());
    assert_eq!(result[0].len(), 1);
    assert_eq!(result[0][0].id, "lab");
    assert!(result[1].is_empty());
    assert_eq!(result[2].len(), 1);
    assert_eq!(result[2][0].id, "foreign");
    assert_eq!(
        result[3].len(),
        2,
        "each branch must count at least two regardless of client limit"
    );
    for result in &result[4..] {
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, "own");
    }
    let saved = tx.read("Observation", "lab").await.unwrap().unwrap();
    assert_eq!(
        result[0][0].resource, saved.resource,
        "conditional snapshots preserve saved JSON"
    );
    let mut invalid = parse_query_string("", 2, 2);
    invalid.constraint = Some(query("not-a-search-parameter", "value"));
    assert!(
        tx.search_conditional_batch(&[("Observation".into(), invalid)])
            .await
            .is_err(),
        "unknown trusted predicates cannot fall back to unrestricted matching"
    );
    tx.rollback().await.unwrap();
    assert!(storage.read("Patient", "own").await.unwrap().is_none());
    assert!(storage.read("Observation", "lab").await.unwrap().is_none());
    assert!(storage.read("Patient", "other").await.unwrap().is_some());
}

#[tokio::test]
async fn reference_locks_hold_through_write_and_rollback_discards_events() {
    use octofhir_core::events::{EventBroadcaster, ResourceEventType};
    use octofhir_storage::EventedStorage;
    use std::time::Duration;

    let (_container, pool, _) = setup().await;
    let inner = PostgresStorage::from_pool(pool.clone());
    inner
        .create(&json!({"resourceType":"Patient","id":"locked-parent"}))
        .await
        .unwrap();
    inner
        .create(&json!({"resourceType":"Patient","id":"committed-parent"}))
        .await
        .unwrap();
    // A distinct pool rules out connection-pool starvation as the reason the
    // concurrent delete waits; its second connection observes PostgreSQL locks.
    let other_pool = PgPoolOptions::new()
        .max_connections(3)
        .connect_with((*pool.connect_options()).clone())
        .await
        .unwrap();
    let other = PostgresStorage::from_pool(other_pool.clone());
    let broadcaster = EventBroadcaster::new_shared();
    let mut receiver = broadcaster.subscribe();
    let storage = EventedStorage::new(inner, broadcaster);
    let mut tx = storage.begin_transaction().await.unwrap();
    let found = tx
        .lock_references(&[(
            "Patient".into(),
            vec!["missing".into(), "locked-parent".into()],
        )])
        .await
        .unwrap();
    assert_eq!(
        found,
        std::collections::HashSet::from(["Patient/locked-parent".to_owned()])
    );
    let delete_storage = other.clone();
    let delete =
        tokio::spawn(async move { delete_storage.delete("Patient", "locked-parent").await });
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let blocked: bool = query_scalar(
                "SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE wait_event_type = 'Lock' AND query LIKE '%DELETE FROM%patient%')",
            ).fetch_one(&other_pool).await.unwrap();
            if blocked { break; }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }).await.expect("delete must reach a row-lock wait, not merely await a pool slot");
    let speculative = tx.create(&json!({"resourceType":"Observation","id":"rolled-back-child", "subject":{"reference":"Patient/locked-parent"}})).await.unwrap();
    assert_eq!(speculative.id, "rolled-back-child");
    assert!(matches!(
        receiver.try_recv(),
        Err(tokio::sync::broadcast::error::TryRecvError::Empty)
    ));
    assert!(
        other
            .read("Observation", "rolled-back-child")
            .await
            .unwrap()
            .is_none()
    );
    assert!(!delete.is_finished());
    tx.rollback().await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), delete)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(
        storage
            .read("Observation", "rolled-back-child")
            .await
            .unwrap()
            .is_none()
    );
    assert!(matches!(
        receiver.try_recv(),
        Err(tokio::sync::broadcast::error::TryRecvError::Empty)
    ));

    let mut tx = storage.begin_transaction().await.unwrap();
    tx.lock_references(&[("Patient".into(), vec!["committed-parent".into()])])
        .await
        .unwrap();
    let committed = tx.create(&json!({"resourceType":"Observation","id":"committed-child", "subject":{"reference":"Patient/committed-parent"}})).await.unwrap();
    assert!(matches!(
        receiver.try_recv(),
        Err(tokio::sync::broadcast::error::TryRecvError::Empty)
    ));
    tx.commit().await.unwrap();
    let event = receiver.try_recv().unwrap();
    let resource = event.as_resource().unwrap();
    assert_eq!(resource.event_type, ResourceEventType::Created);
    assert_eq!(resource.resource_id, committed.id);
    assert_eq!(resource.resource.as_deref(), Some(&committed.resource));
    assert!(matches!(
        receiver.try_recv(),
        Err(tokio::sync::broadcast::error::TryRecvError::Empty)
    ));
    assert_eq!(
        storage
            .read("Observation", "committed-child")
            .await
            .unwrap()
            .unwrap()
            .resource,
        committed.resource
    );
    let batch = storage
        .read_many("Observation", &["committed-child".to_owned()])
        .await
        .unwrap();
    assert_eq!(batch[0].resource, committed.resource);
    let raw = storage
        .read_raw("Observation", "committed-child")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&raw.resource_json).unwrap(),
        committed.resource
    );
    let tx = storage.begin_transaction().await.unwrap();
    let locked = tx
        .read_for_update("Observation", "committed-child")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(locked.resource, committed.resource);
    tx.rollback().await.unwrap();

    let delete_ids = vec![
        "unknown-batch-delete".to_owned(),
        committed.id.clone(),
        committed.id.clone(),
    ];
    let mut tx = storage.begin_transaction().await.unwrap();
    assert_eq!(
        tx.delete_batch("Observation", &delete_ids).await.unwrap(),
        vec![committed.id.clone()]
    );
    assert!(matches!(
        receiver.try_recv(),
        Err(tokio::sync::broadcast::error::TryRecvError::Empty)
    ));
    tx.rollback().await.unwrap();
    assert_eq!(
        storage
            .read("Observation", &committed.id)
            .await
            .unwrap()
            .unwrap()
            .resource,
        committed.resource
    );
    let history_count = || async {
        query_scalar::<_, i64>("SELECT count(*) FROM observation_history WHERE id=$1")
            .bind(&committed.id)
            .fetch_one(&pool)
            .await
            .unwrap()
    };
    assert_eq!(history_count().await, 0);
    let mut tx = storage.begin_transaction().await.unwrap();
    assert_eq!(
        tx.delete_batch("Observation", &delete_ids).await.unwrap(),
        vec![committed.id.clone()]
    );
    assert!(matches!(
        receiver.try_recv(),
        Err(tokio::sync::broadcast::error::TryRecvError::Empty)
    ));
    tx.commit().await.unwrap();
    let event = receiver.try_recv().unwrap();
    assert_eq!(
        event.as_resource().unwrap().event_type,
        ResourceEventType::Deleted
    );
    assert_eq!(event.as_resource().unwrap().resource_id, committed.id);
    let deletion = event.as_resource().unwrap();
    assert_eq!(
        deletion.previous_resource.as_deref(),
        Some(&committed.resource)
    );
    assert!(deletion.resource.is_none());
    let deletion_version = deletion.version_id.unwrap().to_string();
    assert_ne!(deletion_version, committed.version_id);
    assert!(
        storage
            .vread("Observation", &committed.id, &deletion_version)
            .await
            .unwrap_err()
            .is_deleted()
    );
    assert!(
        storage
            .vread_raw("Observation", &committed.id, &deletion_version)
            .await
            .unwrap_err()
            .is_deleted()
    );
    assert_eq!(
        storage
            .vread("Observation", &committed.id, &committed.version_id)
            .await
            .unwrap()
            .unwrap()
            .resource,
        committed.resource
    );
    let tx_read = storage.begin_transaction().await.unwrap();
    assert!(
        tx_read
            .vread("Observation", &committed.id, &deletion_version)
            .await
            .unwrap_err()
            .is_deleted()
    );
    tx_read.rollback().await.unwrap();
    let tombstone_time: chrono::DateTime<chrono::Utc> =
        query_scalar("SELECT updated_at FROM observation_history WHERE id=$1 AND txid=$2")
            .bind(&committed.id)
            .bind(deletion.version_id.unwrap())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        tombstone_time.timestamp_millis(),
        deletion.timestamp.unix_timestamp_nanos() as i64 / 1_000_000
    );

    assert!(matches!(
        receiver.try_recv(),
        Err(tokio::sync::broadcast::error::TryRecvError::Empty)
    ));
    assert_eq!(history_count().await, 2);
    let mut tx = storage.begin_transaction().await.unwrap();
    assert!(
        tx.delete_batch("Observation", &delete_ids)
            .await
            .unwrap()
            .is_empty()
    );
    tx.commit().await.unwrap();
    assert_eq!(history_count().await, 2);
    assert!(matches!(
        receiver.try_recv(),
        Err(tokio::sync::broadcast::error::TryRecvError::Empty)
    ));
    let tx = storage.begin_transaction().await.unwrap();
    let locked = tx
        .read_many_for_update(
            "Patient",
            &[
                "unknown-batch-lock".into(),
                "committed-parent".into(),
                "committed-parent".into(),
            ],
        )
        .await
        .unwrap();
    assert_eq!(locked.len(), 1);
    assert_eq!(locked[0].id, "committed-parent");
    let update = tokio::spawn(async move {
        other
            .update(
                &json!({"resourceType":"Patient","id":"committed-parent","active":true}),
                None,
            )
            .await
    });
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let blocked: bool = query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE wait_event_type='Lock' AND query LIKE '%UPDATE%patient%')")
                .fetch_one(&other_pool).await.unwrap();
            if blocked { break; }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }).await.expect("batch write snapshots must hold actual PostgreSQL row locks");
    tx.rollback().await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), update)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn conditional_gate_coordinates_cross_pool_writes_on_fresh_tables() {
    use octofhir_core::events::EventBroadcaster;
    use octofhir_storage::EventedStorage;
    use std::time::Duration;

    let (_container, pool, _) = setup().await;
    let live_gates: i64 = query_scalar(
        "SELECT count(*) FROM pg_trigger WHERE tgname='octofhir_write_gate' AND NOT tgisinternal",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        live_gates, 3,
        "all fresh live tables protected, history tables excluded"
    );
    let inner = PostgresStorage::from_pool(pool.clone());
    inner
        .create(&json!({"resourceType":"Patient","id":"gate-patient","active":true}))
        .await
        .unwrap();
    inner
        .create(&json!({"resourceType":"Organization","id":"gate-organization"}))
        .await
        .unwrap();
    let other_pool = PgPoolOptions::new()
        .max_connections(4)
        .connect_with((*pool.connect_options()).clone())
        .await
        .unwrap();
    let other = PostgresStorage::from_pool(other_pool.clone());
    let broadcaster = EventBroadcaster::new_shared();
    let mut receiver = broadcaster.subscribe();
    let storage = EventedStorage::new(inner, broadcaster);
    let tx = storage.begin_transaction().await.unwrap();
    tx.read_for_update("Patient", "gate-patient").await.unwrap();
    // Two ordinary mutations share the gate and keep their parallelism.
    tokio::time::timeout(
        Duration::from_secs(5),
        other.create(&json!({"resourceType":"Observation","id":"parallel-ordinary"})),
    )
    .await
    .unwrap()
    .unwrap();
    tx.rollback().await.unwrap();

    let mut tx = storage.begin_transaction().await.unwrap();
    tx.lock_conditional_writes().await.unwrap();
    // The exclusive owner can use ordinary writes and reference locks itself.
    tx.lock_references(&[("Patient".into(), vec!["gate-patient".into()])])
        .await
        .unwrap();
    tx.create(&json!({"resourceType":"Observation","id":"gate-rollback"}))
        .await
        .unwrap();
    let raw_pool = other_pool.clone();
    let insert = tokio::spawn(async move {
        // Direct SQL proves the database trigger protects autocommit callers.
        sql(&raw_pool, "INSERT INTO observation (id,txid,resource) VALUES ('gate-raw',nextval('_transaction_txid_seq'),'{\"resourceType\":\"Observation\",\"id\":\"gate-raw\"}')").await;
    });
    let update_storage = other.clone();
    let update = tokio::spawn(async move {
        update_storage
            .update(
                &json!({"resourceType":"Patient","id":"gate-patient","active":false}),
                None,
            )
            .await
    });
    let delete_storage = other.clone();
    let delete = tokio::spawn(async move {
        delete_storage
            .delete("Organization", "gate-organization")
            .await
    });
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let blocked: i64 = query_scalar("SELECT count(*) FROM pg_stat_activity WHERE wait_event_type='Lock' AND wait_event='advisory'")
                .fetch_one(&other_pool).await.unwrap();
            if blocked >= 3 { break; }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }).await.expect("independent connections must reach the shared advisory gate");
    assert!(!insert.is_finished() && !update.is_finished() && !delete.is_finished());
    assert!(matches!(
        receiver.try_recv(),
        Err(tokio::sync::broadcast::error::TryRecvError::Empty)
    ));
    tx.rollback().await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), insert)
        .await
        .unwrap()
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), update)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), delete)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(
        storage
            .read("Observation", "gate-rollback")
            .await
            .unwrap()
            .is_none()
    );
    let archived: i64 =
        query_scalar("SELECT count(*) FROM observation_history WHERE id='gate-rollback'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(archived, 0);
    assert!(matches!(
        receiver.try_recv(),
        Err(tokio::sync::broadcast::error::TryRecvError::Empty)
    ));
    assert!(
        storage
            .read("Observation", "gate-raw")
            .await
            .unwrap()
            .is_some()
    );
    assert_eq!(
        storage
            .read("Patient", "gate-patient")
            .await
            .unwrap()
            .unwrap()
            .resource["active"],
        false
    );
}

#[tokio::test]
async fn exact_reference_versions_lock_current_and_history_and_survive_target_deletion() {
    use octofhir_core::{FhirReference, events::EventBroadcaster};
    use octofhir_storage::{EventedStorage, StorageError};
    use std::time::Duration;

    let (_container, pool, _) = setup().await;
    let inner = PostgresStorage::from_pool(pool.clone());
    let first = inner
        .create(&json!({"resourceType":"Patient","id":"version-parent","active":true}))
        .await
        .unwrap();
    let second = inner
        .update(
            &json!({"resourceType":"Patient","id":"version-parent","active":false}),
            None,
        )
        .await
        .unwrap();
    assert_eq!(
        inner
            .vread("Patient", &first.id, &first.version_id)
            .await
            .unwrap()
            .unwrap()
            .resource,
        first.resource
    );
    let prefixed_version = format!("0{}", first.version_id);
    let prefixed_current_version = format!("0{}", second.version_id);
    let unknown_versions = [
        prefixed_version.as_str(),
        prefixed_current_version.as_str(),
        "missing.version",
        ".",
        "-0",
        "9223372036854775808",
        "-9223372036854775809",
    ];
    for version in unknown_versions {
        assert!(
            inner
                .vread("Patient", &first.id, version)
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            inner
                .vread_raw("Patient", &first.id, version)
                .await
                .unwrap()
                .is_none()
        );
    }
    let other_pool = PgPoolOptions::new()
        .max_connections(3)
        .connect_with((*pool.connect_options()).clone())
        .await
        .unwrap();
    let other = PostgresStorage::from_pool(other_pool.clone());
    let broadcaster = EventBroadcaster::new_shared();
    let mut receiver = broadcaster.subscribe();
    let storage = EventedStorage::new(inner, broadcaster);
    let mut tx = storage.begin_transaction().await.unwrap();
    assert!(matches!(
        tx.lock_reference_versions(&[FhirReference::with_version(
            "Patient\"unsafe",
            &first.id,
            &first.version_id
        )])
        .await,
        Err(StorageError::InvalidResource { .. })
    ));
    assert!(matches!(
        tx.lock_reference_versions(&[FhirReference::with_version(
            "Patient\"unsafe",
            &first.id,
            "missing.version"
        )])
        .await,
        Err(StorageError::InvalidResource { .. })
    ));
    let mut targets = vec![
        FhirReference::with_version("Patient", &first.id, &first.version_id),
        FhirReference::with_version("Patient", &first.id, &second.version_id),
        FhirReference::with_version("Patient", &first.id, &prefixed_version),
        FhirReference::with_version("Patient", &first.id, "missing.version"),
    ];
    targets.extend(
        unknown_versions
            .into_iter()
            .map(|version| FhirReference::with_version("Patient", &first.id, version)),
    );
    let found = tx.lock_reference_versions(&targets).await.unwrap();
    let mut batched_targets = targets.clone();
    batched_targets.extend(targets.clone());
    batched_targets.push(FhirReference::new("Patient", &first.id));
    batched_targets.push(FhirReference::new("Patient", "unknown-batch-target"));
    let snapshots = tx.lock_reference_snapshots(&batched_targets).await.unwrap();
    assert_eq!(
        snapshots.len(),
        3,
        "missing and duplicate targets must not appear"
    );
    assert_eq!(snapshots[&targets[0].to_versioned()], first.resource);
    assert_eq!(snapshots[&targets[1].to_versioned()], second.resource);
    assert_eq!(snapshots[&format!("Patient/{}", first.id)], second.resource);
    assert_eq!(
        tx.lock_reference_snapshot(&targets[0]).await.unwrap(),
        Some(first.resource.clone())
    );
    assert_eq!(
        tx.lock_reference_snapshot(&targets[1]).await.unwrap(),
        Some(second.resource.clone())
    );
    assert!(
        tx.lock_reference_snapshot(&targets[2])
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        tx.lock_reference_snapshot(&FhirReference::new("Patient", &first.id))
            .await
            .unwrap(),
        Some(second.resource.clone())
    );
    assert_eq!(
        found,
        std::collections::HashSet::from([targets[0].to_versioned(), targets[1].to_versioned()])
    );
    let update = tokio::spawn(async move {
        other
            .update(
                &json!({"resourceType":"Patient","id":"version-parent","active":true}),
                None,
            )
            .await
    });
    let purge_pool = other_pool.clone();
    let purge_id = first.id.clone();
    let purge_version = first.version_id.clone();
    let purge = tokio::spawn(async move {
        sqlx_core::query::query("DELETE FROM patient_history WHERE id=$1 AND txid::text=$2")
            .bind(purge_id)
            .bind(purge_version)
            .execute(&purge_pool)
            .await
            .unwrap();
    });
    tokio::time::timeout(Duration::from_secs(5),async {
        loop {
            let waiting: i64 = query_scalar("SELECT count(*) FROM pg_stat_activity WHERE wait_event_type='Lock' AND (query LIKE '%UPDATE%patient%' OR query LIKE '%DELETE FROM patient_history%')")
                .fetch_one(&other_pool).await.unwrap();
            if waiting >= 2 { break; }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }).await.expect("current mutation and historical purge must wait on actual PostgreSQL row locks");
    tx.create(&json!({"resourceType":"Observation","id":"version-rollback","subject":{"reference":targets[0].to_versioned()}})).await.unwrap();
    assert!(matches!(
        receiver.try_recv(),
        Err(tokio::sync::broadcast::error::TryRecvError::Empty)
    ));
    tx.rollback().await.unwrap();
    let third = tokio::time::timeout(Duration::from_secs(5), update)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), purge)
        .await
        .unwrap()
        .unwrap();
    assert!(
        storage
            .read("Observation", "version-rollback")
            .await
            .unwrap()
            .is_none()
    );
    assert!(matches!(
        receiver.try_recv(),
        Err(tokio::sync::broadcast::error::TryRecvError::Empty)
    ));
    storage.delete("Patient", &first.id).await.unwrap();
    // Discard the separately committed deletion event.
    receiver.try_recv().unwrap();
    let mut tx = storage.begin_transaction().await.unwrap();
    let retained = vec![
        targets[1].clone(),
        FhirReference::with_version("Patient", &first.id, &third.version_id),
    ];
    assert_eq!(
        tx.lock_reference_versions(&retained).await.unwrap(),
        retained.iter().map(FhirReference::to_versioned).collect()
    );
    assert!(
        tx.lock_references(&[("Patient".into(), vec![first.id.clone()])])
            .await
            .unwrap()
            .is_empty()
    );
    let child = tx.create(&json!({"resourceType":"Observation","id":"retained-version-child","subject":{"reference":retained[0].to_versioned()}})).await.unwrap();
    assert!(matches!(
        receiver.try_recv(),
        Err(tokio::sync::broadcast::error::TryRecvError::Empty)
    ));
    tx.commit().await.unwrap();
    let event = receiver.try_recv().unwrap();
    assert_eq!(
        event.as_resource().unwrap().resource.as_deref(),
        Some(&child.resource)
    );
    assert!(matches!(
        receiver.try_recv(),
        Err(tokio::sync::broadcast::error::TryRecvError::Empty)
    ));
    assert_eq!(
        storage
            .vread("Patient", &first.id, &second.version_id)
            .await
            .unwrap()
            .unwrap()
            .resource,
        second.resource
    );
}

#[tokio::test]
async fn proposed_graph_search_preserves_native_chains_grants_and_saved_rows() {
    use octofhir_storage::{ProposedResource, SearchConstraint};
    use std::collections::HashMap;
    let (_container, pool, registry) = setup().await;
    let storage = PostgresStorage::from_pool(pool.clone());
    storage.search_registry_slot().set(registry).unwrap();
    let saved = storage
        .create(&json!({"resourceType":"Patient","id":"replace",
        "name":[{"family":"Old"}]}))
        .await
        .unwrap();
    storage
        .create(&json!({"resourceType":"Patient","id":"remove",
        "name":[{"family":"Gone"}]}))
        .await
        .unwrap();
    storage
        .create(&json!({"resourceType":"Observation","id":"existing",
        "subject":{"reference":"Patient/replace"}}))
        .await
        .unwrap();
    let count_before: i64 = query_scalar(AssertSqlSafe(String::from(
        "SELECT last_value FROM _transaction_txid_seq",
    )))
    .fetch_one(&pool)
    .await
    .unwrap();
    let proposed = vec![
        ProposedResource {
            resource_type: "Patient".into(),
            id: "replace".into(),
            resource: Some(
                json!({"resourceType":"Patient","id":"replace","name":[{"family":"Changed"}]}),
            ),
        },
        ProposedResource {
            resource_type: "Patient".into(),
            id: "remove".into(),
            resource: None,
        },
        ProposedResource {
            resource_type: "Patient".into(),
            id: "new".into(),
            resource: Some(json!({"resourceType":"Patient","id":"new","name":[{"family":"New"}]})),
        },
        ProposedResource {
            resource_type: "Observation".into(),
            id: "new-obs".into(),
            resource: Some(json!({"resourceType":"Observation","id":"new-obs",
                "code":{"coding":[{"code":"proposed"}]},"subject":{"reference":"Patient/new"}})),
        },
    ];
    let mut predicates: Vec<_> = [
        ("Patient", "family=New"),
        ("Patient", "family=Old"),
        ("Patient", "family=Gone"),
        ("Observation", "subject:Patient.family=Changed"),
        ("Patient", "_has:Observation:subject:code=proposed"),
    ]
    .into_iter()
    .map(|(rt, q)| (rt.into(), parse_query_string(q, 2, 2)))
    .collect();
    let mut scoped = parse_query_string("_id=existing", 2, 2);
    scoped.constraint = Some(SearchConstraint::Query(HashMap::from([(
        "subject:Patient.family".into(),
        vec!["Changed".into()],
    )])));
    predicates.push(("Observation".into(), scoped));
    for _ in 0..130 {
        predicates.push(("Patient".into(), parse_query_string("family=New", 2, 2)));
    }
    let broadcaster = octofhir_core::events::EventBroadcaster::new_shared();
    let mut receiver = broadcaster.subscribe();
    let storage = octofhir_storage::EventedStorage::new(storage, broadcaster);
    let tx = storage.begin_transaction().await.unwrap();
    let rows = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        tx.search_proposed_conditional_batch(&predicates, &proposed),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(rows.len(), 136);
    assert_eq!(rows[0][0].id, "new");
    assert!(rows[1].is_empty());
    assert!(rows[2].is_empty());
    assert_eq!(rows[3][0].id, "existing");
    assert_eq!(rows[4][0].id, "new");
    assert_eq!(rows[5][0].id, "existing");
    assert!(
        rows[6..]
            .iter()
            .all(|rows| rows.len() == 1 && rows[0].id == "new")
    );
    let mut duplicate = proposed.clone();
    duplicate.push(proposed[0].clone());
    assert!(
        tx.search_proposed_conditional_batch(&predicates, &duplicate)
            .await
            .is_err()
    );
    assert_eq!(
        tx.read("Patient", "replace")
            .await
            .unwrap()
            .unwrap()
            .resource,
        saved.resource
    );
    assert!(tx.read("Patient", "remove").await.unwrap().is_some());
    assert!(tx.read("Patient", "new").await.unwrap().is_none());
    tx.rollback().await.unwrap();
    assert!(matches!(
        receiver.try_recv(),
        Err(tokio::sync::broadcast::error::TryRecvError::Empty)
    ));

    let count_after: i64 = query_scalar(AssertSqlSafe(String::from(
        "SELECT last_value FROM _transaction_txid_seq",
    )))
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        count_after, count_before,
        "graph preview must not advance the FHIR version counter"
    );
    assert_eq!(
        storage
            .read("Patient", "replace")
            .await
            .unwrap()
            .unwrap()
            .resource,
        saved.resource
    );
    assert_eq!(
        storage
            .history("Patient", Some("replace"), &Default::default())
            .await
            .unwrap()
            .entries
            .len(),
        1
    );
}

#[tokio::test]
async fn reserved_metadata_matches_overlay_native_batches_saved_snapshots_and_events() {
    use octofhir_core::events::EventBroadcaster;
    use octofhir_storage::{EventedStorage, ProposedResource};
    use time::format_description::well_known::Rfc3339;
    let (_container, pool, registry) = setup().await;
    let inner = PostgresStorage::from_pool(pool.clone());
    inner.search_registry_slot().set(registry).unwrap();
    let original = inner
        .create(&json!({"resourceType":"Patient","id":"metadata-existing","active":true}))
        .await
        .unwrap();
    assert_eq!(
        time::OffsetDateTime::parse(
            original.resource["meta"]["lastUpdated"].as_str().unwrap(),
            &Rfc3339
        )
        .unwrap(),
        original.last_updated
    );
    let broadcaster = EventBroadcaster::new_shared();
    let mut receiver = broadcaster.subscribe();
    let storage = EventedStorage::new(inner, broadcaster);
    let mut preview = storage.begin_transaction().await.unwrap();
    let planned = preview.reserve_write_metadata(None).await.unwrap();
    let body = json!({"resourceType":"Patient","id":"metadata-new","meta":{
        "versionId":planned.version_id,"lastUpdated":planned.last_updated}});
    let proposed = vec![ProposedResource {
        resource_type: "Patient".into(),
        id: "metadata-new".into(),
        resource: Some(body.clone()),
    }];
    let predicates = vec![(
        "Patient".into(),
        parse_query_string(
            &format!("_id=metadata-new&_lastUpdated={}", planned.last_updated),
            2,
            2,
        ),
    )];
    let rows = preview
        .search_proposed_conditional_batch(&predicates, &proposed)
        .await
        .unwrap();
    assert_eq!(rows[0].len(), 1);
    assert_eq!(rows[0][0].version_id, planned.version_id);
    assert_eq!(rows[0][0].resource, body);
    assert_eq!(
        rows[0][0].last_updated,
        time::OffsetDateTime::parse(&planned.last_updated, &Rfc3339).unwrap()
    );
    preview.rollback().await.unwrap();
    assert!(receiver.try_recv().is_err());
    tokio::time::sleep(std::time::Duration::from_millis(30)).await;
    let mut tx = storage.begin_transaction().await.unwrap();
    tx.reserve_write_metadata(Some(&planned)).await.unwrap();
    let created = tx.create_batch("Patient", &[body]).await.unwrap();
    let updated = tx
        .update_batch(
            "Patient",
            &[json!({"resourceType":"Patient","id":"metadata-existing","active":false})],
        )
        .await
        .unwrap();
    for row in created.iter().chain(updated.updated.iter()) {
        assert_eq!(row.version_id, planned.version_id);
        assert_eq!(row.resource["meta"]["lastUpdated"], planned.last_updated);
        assert_eq!(
            row.last_updated,
            time::OffsetDateTime::parse(&planned.last_updated, &Rfc3339).unwrap()
        );
        assert_eq!(
            tx.read("Patient", &row.id).await.unwrap().unwrap().resource,
            row.resource
        );
    }
    assert!(receiver.try_recv().is_err());
    tx.commit().await.unwrap();
    let mut actual = std::collections::HashMap::new();
    for _ in 0..2 {
        let event = receiver.try_recv().unwrap();
        let resource = event.as_resource().unwrap();
        actual.insert(
            resource.resource_id.clone(),
            resource.resource.as_ref().unwrap().as_ref().clone(),
        );
    }
    for row in created.iter().chain(updated.updated.iter()) {
        assert_eq!(actual[&row.id], row.resource);
    }
    assert!(receiver.try_recv().is_err());
    assert_eq!(
        storage
            .vread("Patient", &original.id, &original.version_id)
            .await
            .unwrap()
            .unwrap()
            .resource,
        original.resource
    );
    let raw = storage
        .read_raw("Patient", &created[0].id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&raw.resource_json).unwrap(),
        created[0].resource
    );
}

#[tokio::test]
async fn history_period_ends_before_hidden_successor_and_since_is_inclusive() {
    use octofhir_storage::{HistoryParams, SearchConstraint, SearchParams, TotalMode};
    use time::format_description::well_known::Rfc3339;
    let (_container, pool, registry) = setup().await;
    let storage = PostgresStorage::from_pool(pool.clone());
    storage
        .search_registry_slot()
        .set(registry.clone())
        .unwrap();
    let first = storage
        .create(
            &json!({"resourceType":"Patient","id":"history-period", "name":[{"family":"Visible"}]}),
        )
        .await
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    let second = storage
        .update(
            &json!({"resourceType":"Patient","id":"history-period", "name":[{"family":"Hidden"}]}),
            None,
        )
        .await
        .unwrap();
    let mut params = HistoryParams::new().total(TotalMode::Accurate);
    params.at = Some(first.last_updated);
    for raw in [false, true] {
        let versions: Vec<_> = if raw {
            storage
                .history_raw("Patient", Some(&first.id), &params)
                .await
                .unwrap()
                .entries
                .into_iter()
                .map(|row| row.resource.version_id)
                .collect()
        } else {
            storage
                .history("Patient", Some(&first.id), &params)
                .await
                .unwrap()
                .entries
                .into_iter()
                .map(|row| row.resource.version_id)
                .collect()
        };
        assert_eq!(versions, vec![first.version_id.clone()]);
    }
    params.at = Some(second.last_updated);
    let rows = storage
        .history_raw("Patient", Some(&first.id), &params)
        .await
        .unwrap();
    assert_eq!(rows.entries.len(), 1);
    assert_eq!(rows.entries[0].resource.version_id, second.version_id);
    let mut search = SearchParams::new().with_param("_id", &first.id);
    search.constraint = Some(SearchConstraint::Query(std::collections::HashMap::from([
        ("family".into(), vec!["Visible".into()]),
    ])));
    let instant = second.last_updated.format(&Rfc3339).unwrap();
    let mut tx = pool.begin().await.unwrap();
    let hidden = octofhir_db_postgres::queries::search::execute_history_search_raw_with_tx(
        &mut tx,
        &["Patient"],
        &search,
        &registry,
        None,
        10,
        Some((&instant, None)),
    )
    .await
    .unwrap();
    assert!(
        hidden.is_empty(),
        "Hidden successor must end the visible revision's validity"
    );
    tx.rollback().await.unwrap();
    params.at = None;
    params.since = Some(second.last_updated);
    assert_eq!(
        storage
            .history_raw("Patient", Some(&first.id), &params)
            .await
            .unwrap()
            .entries
            .len(),
        1
    );
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    storage.delete("Patient", &first.id).await.unwrap();
    params.since = None;
    params.at = Some(first.last_updated);
    params.at_end = Some(time::OffsetDateTime::now_utc() + time::Duration::seconds(1));
    let raw = storage
        .history_raw("Patient", Some(&first.id), &params)
        .await
        .unwrap();
    assert_eq!(raw.entries.len(), 3);
    assert_eq!(raw.total, Some(3));
    assert_eq!(
        raw.entries[0].method,
        octofhir_storage::HistoryMethod::Delete
    );
    assert_eq!(
        storage
            .system_history_raw(&params)
            .await
            .unwrap()
            .entries
            .len(),
        3
    );
}

#[tokio::test]
async fn stale_reserved_metadata_cannot_reorder_update_or_delete_revisions() {
    use octofhir_core::events::EventBroadcaster;
    use octofhir_storage::{EventedStorage, StorageError};
    let (_container, pool, registry) = setup().await;
    let inner = PostgresStorage::from_pool(pool.clone());
    inner.search_registry_slot().set(registry).unwrap();
    let broadcaster = EventBroadcaster::new_shared();
    let mut receiver = broadcaster.subscribe();
    let storage = EventedStorage::new(inner, broadcaster);
    let original = storage
        .create(&json!({"resourceType":"Patient", "id":"stale-plan", "active":true}))
        .await
        .unwrap();
    receiver.try_recv().unwrap();
    let mut preview = storage.begin_transaction().await.unwrap();
    let planned = preview.reserve_write_metadata(None).await.unwrap();
    preview.rollback().await.unwrap();
    let actual = storage
        .update(
            &json!({"resourceType":"Patient", "id":"stale-plan", "active":false}),
            None,
        )
        .await
        .unwrap();
    let event = receiver.try_recv().unwrap();
    let event = event.as_resource().unwrap();
    assert_eq!(event.previous_resource.as_deref(), Some(&original.resource));
    assert_eq!(event.resource.as_deref(), Some(&actual.resource));
    assert_eq!(event.timestamp, actual.last_updated);
    assert_eq!(event.version_id.unwrap().to_string(), actual.version_id);
    for mutation in ["single", "batch", "delete"] {
        let mut tx = storage.begin_transaction().await.unwrap();
        tx.reserve_write_metadata(Some(&planned)).await.unwrap();
        let outcome = match mutation {
            "single" => tx.update(&original.resource, None).await.map(|_| ()),
            "batch" => tx
                .update_batch("Patient", std::slice::from_ref(&original.resource))
                .await
                .map(|_| ()),
            _ => tx
                .delete_batch_with_snapshots("Patient", std::slice::from_ref(&original.id))
                .await
                .map(|_| ()),
        };
        assert!(
            matches!(outcome, Err(StorageError::WriteConflict { .. })),
            "{mutation}: {outcome:?}"
        );
        tx.rollback().await.unwrap();
        assert_eq!(
            storage
                .read("Patient", &original.id)
                .await
                .unwrap()
                .unwrap()
                .resource,
            actual.resource
        );
        assert_eq!(
            storage
                .history("Patient", Some(&original.id), &Default::default())
                .await
                .unwrap()
                .entries
                .len(),
            2
        );
        assert!(receiver.try_recv().is_err());
    }
}

#[tokio::test]
async fn ordinary_transaction_cannot_reorder_retained_revisions() {
    use octofhir_storage::StorageError;
    let (_container, pool, registry) = setup().await;
    let storage = PostgresStorage::from_pool(pool.clone());
    storage.search_registry_slot().set(registry).unwrap();
    let writer_pool = PgPoolOptions::new()
        .max_connections(1)
        .connect_with(pool.connect_options().as_ref().clone())
        .await
        .unwrap();
    let writer = PostgresStorage::from_pool(writer_pool);
    for mutation in ["single", "batch", "delete", "revive", "revive-batch"] {
        let id = format!("ordering-{mutation}");
        let body = json!({"resourceType":"Patient","id":id,"active":true});
        storage.create(&body).await.unwrap();
        let mut early = storage.begin_transaction().await.unwrap();
        // A different identity assigns the transaction's version. It takes no
        // lock on the target, allowing a later writer to commit first.
        let seed = format!("seed-{mutation}");
        early
            .create(&json!({"resourceType":"Patient","id":seed}))
            .await
            .unwrap();
        let later = writer
            .update(
                &json!({"resourceType":"Patient","id":id,"active":false}),
                None,
            )
            .await
            .unwrap();
        if mutation.starts_with("revive") {
            writer.delete("Patient", &id).await.unwrap();
        }
        let result = match mutation {
            "single" => early.update(&body, None).await.map(|_| ()),
            "batch" => early
                .update_batch("Patient", std::slice::from_ref(&body))
                .await
                .map(|_| ()),
            "delete" => early
                .delete_batch_with_snapshots("Patient", std::slice::from_ref(&id))
                .await
                .map(|_| ()),
            "revive" => early.create(&body).await.map(|_| ()),
            _ => early
                .create_batch(
                    "Patient",
                    &[
                        json!({"resourceType":"Patient","id":"valid-batch-sibling"}),
                        body.clone(),
                    ],
                )
                .await
                .map(|_| ()),
        };
        assert!(
            matches!(result, Err(StorageError::WriteConflict { .. })),
            "{mutation}: {result:?}"
        );
        early.rollback().await.unwrap();
        assert!(storage.read("Patient", &seed).await.unwrap().is_none());
        assert!(
            storage
                .read("Patient", "valid-batch-sibling")
                .await
                .unwrap()
                .is_none()
        );
        if mutation.starts_with("revive") {
            assert!(matches!(
                storage.read("Patient", &id).await,
                Err(StorageError::Deleted { .. })
            ));
        } else {
            assert_eq!(
                storage
                    .read("Patient", &id)
                    .await
                    .unwrap()
                    .unwrap()
                    .resource,
                later.resource
            );
        }
        assert_eq!(
            storage
                .history("Patient", Some(&id), &Default::default())
                .await
                .unwrap()
                .entries
                .len(),
            if mutation.starts_with("revive") { 3 } else { 2 }
        );
    }
}

#[tokio::test]
async fn snapshot_policies_precede_visible_pages_totals_and_iterative_inline_hops() {
    use octofhir_db_postgres::queries::search::{
        RawSearchOptions, execute_system_search_raw_authorized,
    };
    use octofhir_storage::{RawSearchAuthorization, RawStoredResource, SearchParams, StorageError};
    let (_container, pool, registry) = setup().await;
    registry.register(
        SearchParameter::new(
            "has-member",
            "urn:test:Observation:has-member",
            SearchParameterType::Reference,
            vec!["Observation".into()],
        )
        .with_expression("Observation.hasMember")
        .with_targets(vec!["Observation".into()]),
    );
    let storage = PostgresStorage::from_pool(pool.clone());
    assert!(storage.search_registry_slot().set(registry.clone()).is_ok());
    let patients: Vec<_> = (0..520)
        .map(|index| {
            json!({
                "resourceType":"Patient", "id":format!("p{index:03}"), "active":index % 2 == 0,
                "meta":{"tag":[{"system":"urn:exact-snapshot", "code":index.to_string()}]},
            })
        })
        .collect();
    let mut seed = storage.begin_transaction().await.unwrap();
    let saved = seed.create_batch("Patient", &patients).await.unwrap();
    seed.commit().await.unwrap();
    let active = |row: &RawStoredResource| -> Result<bool, StorageError> {
        let body: Value = serde_json::from_str(&row.resource_json).unwrap();
        Ok(body["active"] == true)
    };
    let params = parse_query_string("_sort=-_id&_count=2&_offset=1&_total=accurate", 10, 1000);
    let result = execute_system_search_raw_authorized(
        &pool,
        &["Patient"],
        &params,
        &registry,
        None,
        RawSearchOptions::default(),
        RawSearchAuthorization {
            main: Some(&active),
            included: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(result.total, Some(260));
    assert!(result.has_more);
    assert_eq!(
        result
            .entries
            .iter()
            .map(|row| row.id.as_str())
            .collect::<Vec<_>>(),
        ["p516", "p514"]
    );
    for raw in result.entries {
        let expected = saved.iter().find(|row| row.id == raw.id).unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&raw.resource_json).unwrap(),
            expected.resource
        );
    }
    let count = execute_system_search_raw_authorized(
        &pool,
        &["Patient"],
        &parse_query_string("_summary=count", 10, 1000),
        &registry,
        None,
        RawSearchOptions::default(),
        RawSearchAuthorization {
            main: Some(&active),
            included: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(count.total, Some(260));
    assert!(count.entries.is_empty() && !count.has_more);
    let mut constrained = SearchParams::new()
        .with_count(100)
        .with_total(octofhir_storage::TotalMode::Accurate);
    constrained.constraint = Some(octofhir_storage::SearchConstraint::Query(
        std::collections::HashMap::from([("_id".into(), vec!["p001,p002,p003".into()])]),
    ));
    let result = execute_system_search_raw_authorized(
        &pool,
        &["Patient"],
        &constrained,
        &registry,
        None,
        RawSearchOptions::default(),
        RawSearchAuthorization {
            main: Some(&active),
            included: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(result.total, Some(1));
    assert_eq!(result.entries[0].id, "p002");
    for body in [
        json!({"resourceType":"Observation","id":"root","status":"final","hasMember":[{"reference":"Observation/bridge"}]}),
        json!({"resourceType":"Observation","id":"bridge","status":"preliminary","hasMember":[{"reference":"Observation/tail"}]}),
        json!({"resourceType":"Observation","id":"tail","status":"final"}),
    ] {
        storage.create(&body).await.unwrap();
    }
    let final_only = |row: &RawStoredResource| -> Result<bool, StorageError> {
        Ok(serde_json::from_str::<Value>(&row.resource_json).unwrap()["status"] == "final")
    };
    let params = parse_query_string("_id=root&_include:iterate=Observation:has-member", 10, 100);
    let result = execute_system_search_raw_authorized(
        &pool,
        &["Observation"],
        &params,
        &registry,
        None,
        RawSearchOptions::default(),
        RawSearchAuthorization {
            main: Some(&final_only),
            included: Some(&final_only),
        },
    )
    .await
    .unwrap();
    assert_eq!(result.entries.len(), 1);
    assert!(
        result.included.is_empty(),
        "A denied bridge cannot expose its permitted descendant"
    );
    let reject = |_: &RawStoredResource| Err(StorageError::internal("Policy runtime failed"));
    assert!(
        execute_system_search_raw_authorized(
            &pool,
            &["Observation"],
            &params,
            &registry,
            None,
            RawSearchOptions::default(),
            RawSearchAuthorization {
                main: None,
                included: Some(&reject)
            },
        )
        .await
        .is_err()
    );
    let mut tx = storage.begin_transaction().await.unwrap();
    let own = tx
        .create(&json!({"resourceType":"Patient","id":"own-snapshot","active":true}))
        .await
        .unwrap();
    let result = tx
        .search_system_raw_authorized(
            &["Patient"],
            &SearchParams::new()
                .with_param("_id", "own-snapshot")
                .with_count(10),
            Default::default(),
            true,
            RawSearchAuthorization {
                main: Some(&active),
                included: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(result.entries.len(), 1);
    assert_eq!(
        serde_json::from_str::<Value>(&result.entries[0].resource_json).unwrap(),
        own.resource
    );
    tx.rollback().await.unwrap();
    assert!(
        storage
            .read("Patient", "own-snapshot")
            .await
            .unwrap()
            .is_none()
    );
}
