use std::sync::Arc;

use octofhir_db_postgres::{PostgresStorage, migrations};
use octofhir_search::{SearchParameter, SearchParameterRegistry, SearchParameterType};
use octofhir_storage::FhirStorage;
use serde_json::json;
use sqlx_core::query::query;
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
    storage
        .schema_manager()
        .create_resource_schema("Patient")
        .await
        .expect("Patient schema should be created");

    let registry = Arc::new(SearchParameterRegistry::new());
    registry.register(
        SearchParameter::new(
            "organization",
            "http://example.org/SearchParameter/Patient-organization",
            SearchParameterType::Reference,
            vec!["Patient".to_string()],
        )
        .with_expression("Patient.managingOrganization"),
    );
    storage
        .search_registry_slot()
        .set(registry)
        .expect("registry should only be set once");

    // Drop the Patient partition so index insert fails inside the CRUD transaction.
    query(r#"DROP TABLE "search_idx_reference_patient""#)
        .execute(storage.pool())
        .await
        .expect("reference index partition should be dropped");

    (container, storage)
}

#[tokio::test]
async fn create_rolls_back_when_search_index_write_fails() {
    let (_container, storage) = setup_storage().await;

    let patient = json!({
        "resourceType": "Patient",
        "id": "rollback-patient",
        "managingOrganization": {
            "reference": "Organization/org-1"
        }
    });

    let result = storage.create(&patient).await;
    assert!(result.is_err(), "create should fail when index write fails");

    let stored = storage
        .read("Patient", "rollback-patient")
        .await
        .expect("read should succeed");
    assert!(stored.is_none(), "resource insert must be rolled back");
}
