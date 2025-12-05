//! Database migration management for the PostgreSQL storage backend.
//!
//! This module uses sqlx-core's Migrator API for PostgreSQL-only migrations.

use sqlx_core::migrate::Migrator;
use sqlx_postgres::PgPool;
use std::path::Path;
use tracing::{info, instrument};

use crate::error::Result;

/// Runs all pending database migrations using sqlx-core Migrator.
///
/// Migration system:
/// - Uses sqlx-core Migrator to load migrations from filesystem
/// - Tracks applied migrations in _sqlx_migrations table
/// - Executes migrations in order based on timestamp in filename
/// - Runs programmatically on application startup
/// - No CLI required
///
/// # Errors
///
/// Returns an error if a migration fails to execute.
#[instrument(skip(pool))]
pub async fn run(pool: &PgPool, _db_url: &str) -> Result<()> {
    info!("Running database migrations");

    // Get the migrations directory path relative to the crate root
    let migrations_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");

    info!("Loading migrations from: {:?}", migrations_path);

    let migrator = Migrator::new(migrations_path).await.map_err(|e| {
        crate::error::PostgresError::Migration(format!("Failed to load migrations: {}", e))
    })?;

    info!("Found {} migrations to apply", migrator.migrations.len());

    migrator
        .run(pool)
        .await
        .map_err(|e| crate::error::PostgresError::Migration(format!("Migration failed: {}", e)))?;

    info!("Database migrations completed successfully");

    Ok(())
}
