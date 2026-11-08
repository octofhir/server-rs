//! Database migration management for the PostgreSQL storage backend.
//!
//! This module uses embedded migrations for single-binary deployment.

use sqlx_core::migrate::{Migrate, MigrateError, Migration, MigrationType};
use sqlx_core::sql_str::{AssertSqlSafe, SqlSafeStr};
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
                concat!(
                    include_str!("../../migrations/20241213000001_consolidated_schema.sql"),
                    include_str!("../../sql/resource_write_gate.sql"),
                    include_str!("../../sql/resource_write_metadata.sql"),
                    include_str!("../../sql/resource_history.sql"),
                ),
            ),
            (
                20261105000001i64,
                "bulk_submissions",
                include_str!("../../migrations/20261105000001_bulk_submissions.sql"),
            ),
            (
                20261106000001i64,
                "client_assertion_jti",
                include_str!("../../migrations/20261106000001_client_assertion_jti.sql"),
            ),
            (
                20261108000001i64,
                "dicom_sync",
                include_str!("../../migrations/20261108000001_dicom_sync.sql"),
            ),
            (
                20261108000002i64,
                "dicom_manifests",
                include_str!("../../migrations/20261108000002_dicom_manifests.sql"),
            ),
            (
                20261108000003i64,
                "dicom_ups",
                concat!(
                    include_str!("../../sql/dicom_task_guard.sql"),
                    include_str!("../../migrations/20261108000003_dicom_ups.sql")
                ),
            ),
            (
                20261108000004i64,
                "storage_providers",
                include_str!("../../migrations/20261108000004_storage_providers.sql"),
            ),
        ]
    };
}

/// Builds a vector of Migration structs from embedded migration data.
fn build_migrations() -> Vec<Migration> {
    embedded_migrations!()
        .iter()
        .map(|(version, description, sql)| {
            Migration::new(
                *version,
                Cow::Borrowed(description),
                MigrationType::Simple,
                AssertSqlSafe(sql.to_string()).into_sql_str(),
                false,
            )
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

    // Hold SQLx's database-specific migration lock through validation and setup. Closing this dedicated connection on drop also
    // releases a session lock when startup is cancelled or migration fails.
    let mut connection = pool.acquire().await?;
    connection.close_on_drop();
    let migration_error = |error: MigrateError| {
        crate::error::PostgresError::Migration(format!("Migration failed: {error}"))
    };
    connection.lock().await.map_err(migration_error)?;
    let result: std::result::Result<(), MigrateError> = async {
        connection
            .ensure_migrations_table("_sqlx_migrations")
            .await?;
        let migrator = sqlx_core::migrate::Migrator {
            migrations: Cow::Owned(migrations),
            ignore_missing: false,
            locking: false, // The same SQLx migration lock is already held.
            no_tx: false,
            ..sqlx_core::migrate::Migrator::DEFAULT
        };
        migrator.run_direct(None, &mut *connection, false).await
    }
    .await;
    let unlocked = connection.unlock().await;
    result.map_err(migration_error)?;
    unlocked.map_err(migration_error)?;

    info!("Database migrations completed successfully");

    Ok(())
}
