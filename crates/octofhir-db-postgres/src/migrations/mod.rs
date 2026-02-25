//! Database migration management for the PostgreSQL storage backend.
//!
//! This module uses embedded migrations for single-binary deployment.

use sqlx_core::migrate::{Migration, MigrationType};
use sqlx_postgres::PgPool;
use std::borrow::Cow;
use tracing::{info, instrument};

use crate::error::Result;

/// Macro to define embedded migrations at compile time.
///
/// Usage: Add new migrations here in chronological order.
/// Each migration is a tuple of (version, description, sql_path)
macro_rules! embedded_migrations {
    () => {
        &[
            (
                20241213000001i64,
                "consolidated_schema",
                include_str!("../../migrations/20241213000001_consolidated_schema.sql"),
            ),
            (
                20260225000001i64,
                "db_console_history",
                include_str!("../../migrations/20260225000001_db_console_history.sql"),
            ),
        ]
    };
}

/// Builds a vector of Migration structs from embedded migration data.
fn build_migrations() -> Vec<Migration> {
    embedded_migrations!()
        .iter()
        .map(|(version, description, sql)| Migration {
            version: *version,
            description: Cow::Borrowed(description),
            migration_type: MigrationType::Simple,
            sql: Cow::Borrowed(sql),
            checksum: Cow::Borrowed(&[]), // Empty checksum for embedded migrations
            no_tx: false,                 // Run in transaction
        })
        .collect()
}

/// Runs all pending database migrations using embedded migrations.
///
/// Migration system:
/// - Migrations are embedded in the binary at compile time using include_str!()
/// - Tracks applied migrations in _sqlx_migrations table
/// - Executes migrations in order based on timestamp in filename
/// - Runs programmatically on application startup
/// - No CLI or filesystem access required
/// - Works in single-binary deployment
///
/// To add a new migration:
/// 1. Create the SQL file in migrations/ directory
/// 2. Add an entry to the embedded_migrations!() macro above
///
/// # Errors
///
/// Returns an error if a migration fails to execute.
#[instrument(skip(pool))]
pub async fn run(pool: &PgPool, _db_url: &str) -> Result<()> {
    info!("Running database migrations (embedded)");

    let migrations = build_migrations();
    info!("Found {} migration(s) to apply", migrations.len());

    // Create migrator with all embedded migrations
    let migrator = sqlx_core::migrate::Migrator {
        migrations: Cow::Owned(migrations),
        ignore_missing: false,
        locking: true,
        no_tx: false, // Run in transaction
    };

    migrator
        .run(pool)
        .await
        .map_err(|e| crate::error::PostgresError::Migration(format!("Migration failed: {}", e)))?;

    info!("Database migrations completed successfully");

    Ok(())
}
