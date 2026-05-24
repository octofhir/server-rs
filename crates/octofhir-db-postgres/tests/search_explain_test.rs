use octofhir_db_postgres::{SchemaManager, migrations};
use octofhir_search::{
    SearchParameterRegistry, build_native_ir_query_from_params, parse_query_string,
    register_common_parameters,
};
use sqlx_postgres::PgPoolOptions;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::postgres::Postgres;

#[tokio::test]
#[ignore = "manual live-DB EXPLAIN smoke test"]
async fn search_explain_json_runs_with_bound_params() {
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
    SchemaManager::new(pool.clone())
        .create_resource_schema("Patient")
        .await
        .expect("Patient schema should be created");

    let registry = SearchParameterRegistry::new();
    register_common_parameters(&registry);
    let params = parse_query_string("_id=explain-patient&_count=1", 10, 100);
    let query = build_native_ir_query_from_params("Patient", &params, &registry, "public")
        .expect("query should build")
        .builder
        .with_raw_resource(true)
        .build()
        .expect("SQL should render");

    let explain = octofhir_db_postgres::queries::search::explain_built_search_query_json(
        &pool, &query, false,
    )
    .await
    .expect("EXPLAIN should run");

    assert!(
        explain.is_array(),
        "EXPLAIN JSON should be an array: {explain}"
    );
    assert!(
        explain[0].get("Plan").is_some(),
        "EXPLAIN JSON should contain a plan: {explain}"
    );
}
