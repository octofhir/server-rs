//! Simple test to verify sqlx migrations work

use octofhir_db_postgres::migrations;
use sqlx_core::query_as::query_as;
use testcontainers::{ImageExt, runners::AsyncRunner};
use testcontainers_modules::postgres::Postgres;

#[tokio::test]
async fn test_migrations_run_successfully() {
    // Start PostgreSQL testcontainer
    let container = Postgres::default()
        .with_tag("17-alpine")
        .start()
        .await
        .expect("Failed to start PostgreSQL container");

    let port = container
        .get_host_port_ipv4(5432)
        .await
        .expect("Failed to get port");
    let db_url = format!("postgres://postgres:postgres@localhost:{}/postgres", port);

    println!("PostgreSQL running on port {} with URL: {}", port, db_url);

    // Create connection pool
    let pool = sqlx_postgres::PgPoolOptions::new()
        .max_connections(5)
        .connect(&db_url)
        .await
        .expect("Failed to connect to database");

    // Run migrations
    let result = migrations::run(&pool, &db_url).await;

    match &result {
        Ok(_) => println!("✓ Migrations ran successfully"),
        Err(e) => {
            eprintln!("✗ Migration failed: {}", e);
            eprintln!("Full error: {:?}", e);

            // Try to get more details from the database
            let applied: Result<Vec<(i64, String)>, _> =
                query_as("SELECT version, description FROM _sqlx_migrations ORDER BY version")
                    .fetch_all(&pool)
                    .await;

            if let Ok(migrations) = applied {
                eprintln!("\nApplied migrations:");
                for (version, description) in migrations {
                    eprintln!("  V{}: {}", version, description);
                }
            }

            panic!("Migration failed: {}", e);
        }
    }

    result.expect("Migrations should succeed");

    // Verify migrations created the expected tables
    let tables: Vec<(String,)> =
        query_as("SELECT tablename FROM pg_tables WHERE schemaname = 'public' ORDER BY tablename")
            .fetch_all(&pool)
            .await
            .expect("Failed to query tables");

    println!("Tables created:");
    for (table_name,) in &tables {
        println!("  - {}", table_name);
    }

    // Check for key tables from our migrations
    let table_names: Vec<String> = tables.iter().map(|(name,)| name.clone()).collect();

    let sequences: Vec<(String,)> =
        query_as("SELECT sequencename FROM pg_sequences WHERE schemaname = 'public'")
            .fetch_all(&pool)
            .await
            .expect("Failed to query sequences");
    let sequence_names: Vec<String> = sequences.iter().map(|(name,)| name.clone()).collect();
    assert!(
        sequence_names.contains(&"_transaction_txid_seq".to_string()),
        "Missing _transaction_txid_seq sequence"
    );
    assert!(
        table_names.contains(&"temp_valueset_codes".to_string()),
        "Missing temp_valueset_codes table"
    );
    assert!(
        table_names.contains(&"_sqlx_migrations".to_string()),
        "Missing _sqlx_migrations table"
    );

    // Freshly created resource tables must preserve exact snapshots and align
    // JSON metadata with columns. Repeated startup must not revise resources.
    use octofhir_db_postgres::{PostgresStorage, SchemaManager};
    use octofhir_storage::{FhirStorage, HistoryParams};
    use serde_json::{Value, json};
    SchemaManager::ensure_archive_function(&pool).await.unwrap();
    SchemaManager::new(pool.clone())
        .create_resource_schema("Patient")
        .await
        .unwrap();
    let storage = PostgresStorage::from_pool(pool.clone());
    let original = storage
        .create(&json!({"resourceType":"Patient","id":"fresh-metadata","active":true}))
        .await
        .unwrap();
    storage
        .update(
            &json!({"resourceType":"Patient","id":original.id,"active":false}),
            None,
        )
        .await
        .unwrap();
    let before = storage
        .history("Patient", Some(&original.id), &HistoryParams::default())
        .await
        .unwrap();
    migrations::run(&pool, &db_url).await.unwrap();
    let snapshots: Vec<(Value, String)> = query_as("SELECT resource, to_char(updated_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') FROM patient UNION ALL SELECT resource, to_char(updated_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') FROM patient_history")
        .fetch_all(&pool).await.unwrap();
    for (body, timestamp) in snapshots {
        assert_eq!(body["meta"]["lastUpdated"], timestamp);
    }
    let after = storage
        .history("Patient", Some(&original.id), &HistoryParams::default())
        .await
        .unwrap();
    assert_eq!(
        before
            .entries
            .iter()
            .map(|r| &r.resource.resource)
            .collect::<Vec<_>>(),
        after
            .entries
            .iter()
            .map(|r| &r.resource.resource)
            .collect::<Vec<_>>()
    );

    let checksums: Vec<(i64, Vec<u8>)> =
        query_as("SELECT version, checksum FROM _sqlx_migrations ORDER BY version")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert!(checksums.iter().all(|(_, checksum)| checksum.len() == 48));
    assert_eq!(checksums.len(), 1);
    let job_columns: Vec<(String,)> = query_as("SELECT column_name FROM information_schema.columns WHERE table_schema='public' AND table_name='async_jobs'")
        .fetch_all(&pool).await.unwrap();
    for expected in ["execution_id", "lease_expires_at", "attempts", "checkpoint"] {
        assert!(job_columns.iter().any(|(column,)| column == expected));
    }
    // Missing or changed digests must be rejected; startup never adopts an
    // unknown schema. A failed run must also release its migration lock.
    sqlx_core::query::query(
        "UPDATE _sqlx_migrations SET checksum = ''::bytea WHERE version = 20241213000001",
    )
    .execute(&pool)
    .await
    .unwrap();
    assert!(migrations::run(&pool, &db_url).await.is_err());
    sqlx_core::query::query(
        "UPDATE _sqlx_migrations SET checksum = 'changed'::bytea WHERE version = 20241213000001",
    )
    .execute(&pool)
    .await
    .unwrap();
    assert!(migrations::run(&pool, &db_url).await.is_err());
    sqlx_core::query::query(
        "UPDATE _sqlx_migrations SET checksum = $1 WHERE version = 20241213000001",
    )
    .bind(&checksums[0].1)
    .execute(&pool)
    .await
    .unwrap();
    tokio::time::timeout(
        std::time::Duration::from_secs(10),
        migrations::run(&pool, &db_url),
    )
    .await
    .unwrap()
    .unwrap();

    println!("✓ All expected tables exist");
}
