//! Schema management for the PostgreSQL storage backend.
//!
//! This module handles database schema operations such as table creation,
//! index management, and schema introspection. It uses a table-per-resource
//! pattern where each FHIR resource type gets its own table.

use sqlx_core::sql_str::AssertSqlSafe;
use sqlx_postgres::PgPool;
use tracing::{debug, instrument};

use crate::error::{PostgresError, Result};

/// Shared `archive_to_history()` plpgsql function used by all history triggers.
///
/// Created once via `ensure_archive_function()` so per-resource schema creation
/// doesn't redefine the same server-wide function from many concurrent
/// connections (which would serialize).
const ARCHIVE_FN_SQL: &str = r#"
    CREATE OR REPLACE FUNCTION archive_to_history()
    RETURNS TRIGGER AS $$
    BEGIN
        EXECUTE format(
            'INSERT INTO %I_history (id, txid, created_at, updated_at, resource, status)
             VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT (id, txid) DO UPDATE SET
                 created_at = EXCLUDED.created_at,
                 updated_at = EXCLUDED.updated_at,
                 resource = EXCLUDED.resource,
                 status = EXCLUDED.status',
            TG_TABLE_NAME
        ) USING OLD.id, OLD.txid, OLD.created_at, OLD.updated_at, OLD.resource, OLD.status;
        -- NEW on UPDATE (apply it), OLD on DELETE (proceed with removal).
        -- Returning NEW unconditionally would be NULL on DELETE and silently
        -- cancel the row removal.
        RETURN COALESCE(NEW, OLD);
    END;
    $$ LANGUAGE plpgsql;
"#;

/// Manages the database schema for FHIR resources.
///
/// The `SchemaManager` is responsible for:
/// - Creating and managing resource tables dynamically
/// - Creating history tables with triggers for versioning
/// - Managing indexes for efficient JSONB search
///
/// # Table Structure
///
/// For each resource type (e.g., "Patient"), the manager creates:
/// - A main table (`patient`) with the current resource state
/// - A history table (`patient_history`) for previous versions
/// - GIN indexes for efficient JSONB queries
/// - A trigger that archives old versions on UPDATE/DELETE
#[derive(Debug, Clone)]
pub struct SchemaManager {
    pool: PgPool,
}

impl SchemaManager {
    /// Creates a new `SchemaManager` with the given connection pool.
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Returns a reference to the connection pool.
    #[must_use]
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Converts a FHIR resource type to a table name.
    ///
    /// Table names are always lowercase to avoid case-sensitivity issues
    /// in PostgreSQL.
    #[must_use]
    pub fn table_name(resource_type: &str) -> String {
        resource_type.to_lowercase()
    }

    /// Creates the full schema for a resource type (idempotent).
    ///
    /// Builds a single multi-statement DDL block and runs it via `raw_sql` so
    /// the whole resource_type costs one server round-trip instead of ~10.
    ///
    /// Pre-condition: `ensure_archive_function()` must have been called on
    /// `pool` before this method runs for any resource that needs history.
    ///
    /// All DDL uses `IF NOT EXISTS` / `CREATE OR REPLACE` for idempotency.
    ///
    /// # Errors
    ///
    /// Returns an error if any DDL statement fails.
    #[instrument(skip(self), fields(resource_type = %resource_type))]
    pub async fn create_resource_schema(&self, resource_type: &str) -> Result<()> {
        let sql = Self::build_resource_schema_sql(resource_type);
        sqlx_core::raw_sql::raw_sql(AssertSqlSafe(sql.to_string()))
            .execute(&self.pool)
            .await
            .map_err(PostgresError::from)?;
        debug!("Ensured schema for resource type: {}", resource_type);
        Ok(())
    }

    /// Build the multi-statement DDL string for one resource type.
    ///
    /// Combines table, triggers, indexes, partitions into one semicolon-
    /// separated batch executable via `raw_sql`. Callers MUST invoke
    /// `ensure_archive_function()` once on the pool before running this
    /// concurrently — running `CREATE OR REPLACE FUNCTION
    /// archive_to_history()` from many concurrent connections trips
    /// "tuple concurrently updated" on `pg_proc`, dropping schema
    /// creates on the floor.
    fn build_resource_schema_sql(resource_type: &str) -> String {
        let table = Self::table_name(resource_type);
        let history_table = format!("{}_history", table);
        let is_internal = Self::is_internal_resource(&table);
        let is_gateway = Self::is_gateway_resource(&table);
        let is_policy = Self::is_policy_resource(&table);

        let mut sql = String::with_capacity(2048);

        // Main table
        sql.push_str(&format!(
            "CREATE TABLE IF NOT EXISTS \"{table}\" (\n\
                id TEXT PRIMARY KEY,\n\
                txid BIGINT NOT NULL,\n\
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),\n\
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),\n\
                resource JSONB NOT NULL,\n\
                status resource_status NOT NULL DEFAULT 'created'\n\
            );\n"
        ));

        // Update trigger (drop + create)
        let update_trigger = format!("{}_update_timestamp", table);
        sql.push_str(&format!(
            "DROP TRIGGER IF EXISTS \"{update_trigger}\" ON \"{table}\";\n\
             CREATE TRIGGER \"{update_trigger}\" BEFORE UPDATE ON \"{table}\" \
             FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();\n"
        ));

        // History table + trigger (skipped for internal resources)
        if !is_internal {
            sql.push_str(&format!(
                "CREATE TABLE IF NOT EXISTS \"{history_table}\" (\n\
                    id TEXT NOT NULL,\n\
                    txid BIGINT NOT NULL,\n\
                    created_at TIMESTAMPTZ NOT NULL,\n\
                    updated_at TIMESTAMPTZ NOT NULL,\n\
                    resource JSONB NOT NULL,\n\
                    status resource_status NOT NULL,\n\
                    PRIMARY KEY (id, txid)\n\
                );\n"
            ));
            let history_trigger = format!("{}_history_trigger", table);
            sql.push_str(&format!(
                "DROP TRIGGER IF EXISTS \"{history_trigger}\" ON \"{table}\";\n\
                 CREATE TRIGGER \"{history_trigger}\" BEFORE UPDATE OR DELETE ON \"{table}\" \
                 FOR EACH ROW EXECUTE FUNCTION archive_to_history();\n"
            ));
        }

        // Indexes (main table)
        sql.push_str(&format!(
            "CREATE INDEX IF NOT EXISTS \"idx_{table}_gin\" ON \"{table}\" \
             USING GIN (resource jsonb_path_ops) \
             WITH (fastupdate=on);\n\
             CREATE INDEX IF NOT EXISTS \"idx_{table}_txid\" ON \"{table}\"(txid);\n\
             CREATE INDEX IF NOT EXISTS \"idx_{table}_created_at\" ON \"{table}\"(created_at);\n\
             CREATE INDEX IF NOT EXISTS \"idx_{table}_updated_at\" ON \"{table}\"(updated_at);\n\
             CREATE INDEX IF NOT EXISTS \"idx_{table}_status\" ON \"{table}\"(status);\n"
        ));

        // History indexes
        if !is_internal {
            sql.push_str(&format!(
                "CREATE INDEX IF NOT EXISTS \"idx_{history_table}_updated_at\" \
                    ON \"{history_table}\"(updated_at);\n\
                 CREATE INDEX IF NOT EXISTS \"idx_{history_table}_id\" \
                    ON \"{history_table}\"(id);\n"
            ));
        }

        // Gateway notify trigger
        if is_gateway {
            let trig = format!("{}_gateway_notify", table);
            sql.push_str(&format!(
                "DROP TRIGGER IF EXISTS \"{trig}\" ON \"{table}\";\n\
                 CREATE TRIGGER \"{trig}\" AFTER INSERT OR UPDATE OR DELETE ON \"{table}\" \
                 FOR EACH ROW EXECUTE FUNCTION notify_gateway_resource_change();\n"
            ));
        }

        // Policy notify trigger
        if is_policy {
            let trig = format!("{}_policy_notify", table);
            sql.push_str(&format!(
                "DROP TRIGGER IF EXISTS \"{trig}\" ON \"{table}\";\n\
                 CREATE TRIGGER \"{trig}\" AFTER INSERT OR UPDATE OR DELETE ON \"{table}\" \
                 FOR EACH ROW EXECUTE FUNCTION notify_policy_change();\n"
            ));
        }

        sql
    }

    /// Ensure the shared `archive_to_history()` function exists. Call once
    /// before parallel resource-schema creation.
    pub async fn ensure_archive_function(pool: &PgPool) -> Result<()> {
        sqlx_core::query::query(AssertSqlSafe((ARCHIVE_FN_SQL).to_string()))
            .execute(pool)
            .await
            .map_err(PostgresError::from)?;
        Ok(())
    }

    /// Returns true if this resource type requires gateway notifications.
    fn is_gateway_resource(table: &str) -> bool {
        matches!(table, "app" | "customoperation")
    }

    /// Returns true if this resource type requires policy notifications.
    fn is_policy_resource(table: &str) -> bool {
        table == "accesspolicy"
    }

    /// Returns true if this is an internal resource that should not have history tables.
    /// These resources are managed differently and don't need FHIR-style versioning.
    fn is_internal_resource(table: &str) -> bool {
        matches!(
            table,
            "user"
                | "client"
                | "session"
                | "authsession" // SSO sessions - no history needed
                | "accesspolicy"
                | "refreshtoken"
                | "revokedtoken"
                | "identityprovider"
                | "role"
                | "app"
                | "customoperation"
                | "appsubscription"
                | "notificationlog"
                | "notificationprovider"
                | "notificationtemplate"
        )
    }

    /// Lists all resource tables (excludes history and system tables).
    #[instrument(skip(self))]
    pub async fn list_tables(&self) -> Result<Vec<String>> {
        let rows: Vec<(String,)> = sqlx_core::query_as::query_as(
            "SELECT table_name FROM information_schema.tables
             WHERE table_schema = 'public'
             AND table_name NOT LIKE '%_history'
             AND table_name NOT LIKE '\\_%' ESCAPE '\\'
             ORDER BY table_name",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(PostgresError::from)?;

        Ok(rows.into_iter().map(|(t,)| t).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_table_name_lowercase() {
        assert_eq!(SchemaManager::table_name("Patient"), "patient");
        assert_eq!(SchemaManager::table_name("Observation"), "observation");
        assert_eq!(
            SchemaManager::table_name("MedicationRequest"),
            "medicationrequest"
        );
    }

    #[test]
    fn test_table_name_already_lowercase() {
        assert_eq!(SchemaManager::table_name("patient"), "patient");
    }
}
