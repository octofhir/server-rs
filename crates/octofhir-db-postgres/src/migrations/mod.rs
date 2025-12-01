//! Database migration management for the PostgreSQL storage backend.
//!
//! This module handles running and managing SQL migrations.
//! Migrations are embedded at compile time and executed in order.

use sqlx_core::executor::Executor;
use sqlx_core::query::query;
use sqlx_core::query_scalar::query_scalar;
use sqlx_postgres::PgPool;
use tracing::{debug, info, instrument, warn};

use crate::error::Result;

/// Embedded migration scripts
///
/// Migration 003 creates the gateway notification function.
/// Triggers are added dynamically by SchemaManager for `app` and `customoperation` tables.
const MIGRATIONS: &[(&str, &str)] = &[
    ("001_base_schema", include_str!("../../migrations/001_base_schema.sql")),
    ("002_octofhir_schema", include_str!("../../migrations/002_octofhir_schema.sql")),
    ("003_gateway_resource_notify", include_str!("../../migrations/003_gateway_resource_notify.sql")),
];

/// Runs all pending database migrations.
///
/// This function:
/// 1. Creates a `_migrations` tracking table if it doesn't exist
/// 2. Checks which migrations have already been applied
/// 3. Runs any pending migrations in order
///
/// # Errors
///
/// Returns an error if a migration fails to execute.
#[instrument(skip(pool))]
pub async fn run(pool: &PgPool) -> Result<()> {
    // Create migrations tracking table if it doesn't exist
    pool.execute(
        r#"
        CREATE TABLE IF NOT EXISTS _migrations (
            name VARCHAR(255) PRIMARY KEY,
            applied_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        )
        "#,
    )
    .await?;

    // Get list of already applied migrations
    let applied: Vec<String> = query_scalar("SELECT name FROM _migrations")
        .fetch_all(pool)
        .await?;

    let applied_count = applied.len();
    let mut newly_applied = 0;

    for (name, sql) in MIGRATIONS {
        if applied.contains(&name.to_string()) {
            debug!(migration = name, "Migration already applied, skipping");
            continue;
        }

        info!(migration = name, "Applying migration");

        // Execute the migration SQL
        // Split by semicolons to handle multiple statements, but be careful with
        // function bodies that contain semicolons
        match pool.execute(*sql).await {
            Ok(_) => {
                // Record that this migration was applied
                query("INSERT INTO _migrations (name) VALUES ($1)")
                    .bind(*name)
                    .execute(pool)
                    .await?;

                info!(migration = name, "Migration applied successfully");
                newly_applied += 1;
            }
            Err(e) => {
                warn!(migration = name, error = %e, "Migration failed");
                return Err(e.into());
            }
        }
    }

    if newly_applied > 0 {
        info!(
            total = MIGRATIONS.len(),
            previously_applied = applied_count,
            newly_applied = newly_applied,
            "Database migrations completed"
        );
    } else {
        debug!(
            total = MIGRATIONS.len(),
            applied = applied_count,
            "All migrations already applied"
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_migrations_embedded() {
        // Verify that all migration files are properly embedded
        assert_eq!(MIGRATIONS.len(), 3);

        for (name, sql) in MIGRATIONS {
            assert!(!name.is_empty(), "Migration name should not be empty");
            assert!(!sql.is_empty(), "Migration SQL should not be empty");
            assert!(
                sql.contains("CREATE") || sql.contains("create"),
                "Migration {} should contain CREATE statements",
                name
            );
        }
    }

    #[test]
    fn test_migration_order() {
        // Verify migrations are in correct order
        let names: Vec<&str> = MIGRATIONS.iter().map(|(n, _)| *n).collect();
        assert_eq!(names[0], "001_base_schema");
        assert_eq!(names[1], "002_octofhir_schema");
        assert_eq!(names[2], "003_gateway_resource_notify");
    }
}
