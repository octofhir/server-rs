//! Simple test to verify sqlx migrations work

use octofhir_db_postgres::migrations;
use sqlx_core::query_as::query_as;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::postgres::Postgres;

#[tokio::test]
async fn test_migrations_run_successfully() {
    // Start PostgreSQL testcontainer
    let container = Postgres::default()
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

    assert!(
        table_names.contains(&"_transaction".to_string()),
        "Missing _transaction table"
    );
    assert!(
        table_names.contains(&"temp_valueset_codes".to_string()),
        "Missing temp_valueset_codes table"
    );
    assert!(
        table_names.contains(&"_sqlx_migrations".to_string()),
        "Missing _sqlx_migrations table"
    );

    println!("✓ All expected tables exist");
}
