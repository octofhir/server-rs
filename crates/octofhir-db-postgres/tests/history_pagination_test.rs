use octofhir_db_postgres::{PostgresStorage, migrations};
use octofhir_storage::{FhirStorage, HistoryParams};
use serde_json::json;
use sqlx_postgres::PgPoolOptions;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::postgres::Postgres;

async fn setup_storage() -> (testcontainers::ContainerAsync<Postgres>, PostgresStorage) {
    let container = Postgres::default()
        .start()
        .await
        .expect("Failed to start PostgreSQL container");

    let port = container
        .get_host_port_ipv4(5432)
        .await
        .expect("Failed to get port");
    let db_url = format!("postgres://postgres:postgres@localhost:{}/postgres", port);

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&db_url)
        .await
        .expect("Failed to connect to database");

    migrations::run(&pool, &db_url)
        .await
        .expect("Migrations should succeed");

    let storage = PostgresStorage::from_pool(pool);
    octofhir_db_postgres::SchemaManager::ensure_archive_function(storage.pool())
        .await
        .expect("archive function should be created");
    storage
        .schema_manager()
        .create_resource_schema("Patient")
        .await
        .expect("Patient schema should be created");

    (container, storage)
}

#[tokio::test]
async fn paged_history_reports_total_matching_versions() {
    let (_container, storage) = setup_storage().await;

    storage
        .create(&json!({"resourceType": "Patient", "id": "history-patient", "active": true}))
        .await
        .expect("create should succeed");
    storage
        .update(
            &json!({"resourceType": "Patient", "id": "history-patient", "active": false}),
            None,
        )
        .await
        .expect("first update should succeed");
    storage
        .update(
            &json!({"resourceType": "Patient", "id": "history-patient", "active": true}),
            None,
        )
        .await
        .expect("second update should succeed");

    let params = HistoryParams::new().count(1).offset(1);
    let history = storage
        .history_raw("Patient", Some("history-patient"), &params)
        .await
        .expect("history should succeed");

    assert_eq!(history.entries.len(), 1, "page size should be honored");
    assert_eq!(
        history.total,
        Some(3),
        "total must count all matching versions, not just the current page"
    );
}

#[tokio::test]
async fn paged_type_history_reports_total_matching_versions() {
    let (_container, storage) = setup_storage().await;

    storage
        .create(&json!({"resourceType": "Patient", "id": "type-history-a"}))
        .await
        .expect("first create should succeed");
    storage
        .create(&json!({"resourceType": "Patient", "id": "type-history-b"}))
        .await
        .expect("second create should succeed");
    storage
        .update(
            &json!({"resourceType": "Patient", "id": "type-history-a", "active": true}),
            None,
        )
        .await
        .expect("update should succeed");

    let params = HistoryParams::new().count(1).offset(1);
    let history = storage
        .history_raw("Patient", None, &params)
        .await
        .expect("type history should succeed");

    assert_eq!(history.entries.len(), 1, "page size should be honored");
    assert_eq!(
        history.total,
        Some(3),
        "type history total must count all matching versions"
    );
}
