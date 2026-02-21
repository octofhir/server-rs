//! Schema management for the PostgreSQL storage backend.
//!
//! This module handles database schema operations such as table creation,
//! index management, and schema introspection. It uses a table-per-resource
//! pattern where each FHIR resource type gets its own table.

use sqlx_postgres::PgPool;
use tracing::{debug, info, instrument};

use crate::error::{PostgresError, Result};

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
    /// Creates: resource table, history table, triggers, indexes, search index partitions.
    /// All DDL uses `IF NOT EXISTS` / `CREATE OR REPLACE` for idempotency — no caching
    /// or existence checks needed.
    ///
    /// # Errors
    ///
    /// Returns an error if any DDL statement fails.
    #[instrument(skip(self), fields(resource_type = %resource_type))]
    pub async fn create_resource_schema(&self, resource_type: &str) -> Result<()> {
        let table = Self::table_name(resource_type);

        self.create_resource_table(resource_type).await?;
        self.create_update_trigger(resource_type).await?;

        if !Self::is_internal_resource(&table) {
            self.create_history_table(resource_type).await?;
            self.create_history_trigger(resource_type).await?;
        }

        // Indexes after history table so history table indexes can be created
        self.create_indexes(resource_type).await?;

        if !Self::is_internal_resource(&table) {
            self.ensure_search_index_partitions(resource_type).await?;
        }

        if Self::is_gateway_resource(&table) {
            self.create_gateway_trigger(resource_type).await?;
        }

        if Self::is_policy_resource(&table) {
            self.create_policy_trigger(resource_type).await?;
        }

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

    /// Creates the main resource table.
    #[instrument(skip(self))]
    async fn create_resource_table(&self, resource_type: &str) -> Result<()> {
        let table = Self::table_name(resource_type);

        let sql = format!(
            r#"
            CREATE TABLE IF NOT EXISTS "{table}" (
                id TEXT PRIMARY KEY,
                txid BIGINT NOT NULL REFERENCES _transaction(txid),
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                resource JSONB NOT NULL,
                status resource_status NOT NULL DEFAULT 'created'
            )
            "#
        );

        sqlx_core::query::query(&sql)
            .execute(&self.pool)
            .await
            .map_err(PostgresError::from)?;

        debug!("Ensured table: {}", table);
        Ok(())
    }

    /// Creates the history table for storing previous versions.
    ///
    /// Like the main table, resource_type is not stored since:
    /// 1. The history table name indicates the resource type
    /// 2. The JSONB resource field contains resourceType
    #[instrument(skip(self))]
    async fn create_history_table(&self, resource_type: &str) -> Result<()> {
        let table = Self::table_name(resource_type);
        let history_table = format!("{}_history", table);

        let sql = format!(
            r#"
            CREATE TABLE IF NOT EXISTS "{history_table}" (
                id TEXT NOT NULL,
                txid BIGINT NOT NULL,
                created_at TIMESTAMPTZ NOT NULL,
                updated_at TIMESTAMPTZ NOT NULL,
                resource JSONB NOT NULL,
                status resource_status NOT NULL,
                PRIMARY KEY (id, txid)
            )
            "#
        );

        sqlx_core::query::query(&sql)
            .execute(&self.pool)
            .await
            .map_err(PostgresError::from)?;

        info!("Created history table: {}", history_table);
        Ok(())
    }

    /// Creates indexes for efficient querying.
    #[instrument(skip(self))]
    async fn create_indexes(&self, resource_type: &str) -> Result<()> {
        let table = Self::table_name(resource_type);

        // GIN index for JSONB search (enables efficient @>, ?, ?& operators)
        let gin_sql = format!(
            r#"CREATE INDEX IF NOT EXISTS "idx_{table}_gin" ON "{table}" USING GIN (resource jsonb_path_ops)"#
        );
        sqlx_core::query::query(&gin_sql)
            .execute(&self.pool)
            .await
            .map_err(PostgresError::from)?;

        // Index on txid for transaction-based queries
        let txid_sql =
            format!(r#"CREATE INDEX IF NOT EXISTS "idx_{table}_txid" ON "{table}"(txid)"#);
        sqlx_core::query::query(&txid_sql)
            .execute(&self.pool)
            .await
            .map_err(PostgresError::from)?;

        // Index on created_at for time-based queries
        let created_at_sql = format!(
            r#"CREATE INDEX IF NOT EXISTS "idx_{table}_created_at" ON "{table}"(created_at)"#
        );
        sqlx_core::query::query(&created_at_sql)
            .execute(&self.pool)
            .await
            .map_err(PostgresError::from)?;

        // Index on updated_at for time-based sorting and filtering
        let updated_at_sql = format!(
            r#"CREATE INDEX IF NOT EXISTS "idx_{table}_updated_at" ON "{table}"(updated_at)"#
        );
        sqlx_core::query::query(&updated_at_sql)
            .execute(&self.pool)
            .await
            .map_err(PostgresError::from)?;

        // Index on status for filtering deleted resources
        let status_sql =
            format!(r#"CREATE INDEX IF NOT EXISTS "idx_{table}_status" ON "{table}"(status)"#);
        sqlx_core::query::query(&status_sql)
            .execute(&self.pool)
            .await
            .map_err(PostgresError::from)?;

        // Skip history table indexes for internal resources
        if !Self::is_internal_resource(&table) {
            let history_table = format!("{}_history", table);

            // History table index on updated_at for history queries
            let history_updated_at_sql = format!(
                r#"CREATE INDEX IF NOT EXISTS "idx_{history_table}_updated_at" ON "{history_table}"(updated_at)"#
            );
            sqlx_core::query::query(&history_updated_at_sql)
                .execute(&self.pool)
                .await
                .map_err(PostgresError::from)?;

            // History table index on id for resource-specific history
            let history_id_sql = format!(
                r#"CREATE INDEX IF NOT EXISTS "idx_{history_table}_id" ON "{history_table}"(id)"#
            );
            sqlx_core::query::query(&history_id_sql)
                .execute(&self.pool)
                .await
                .map_err(PostgresError::from)?;
        }

        info!("Created indexes for: {}", table);
        Ok(())
    }

    /// Creates the update timestamp trigger that automatically updates updated_at.
    ///
    /// Uses the shared `update_updated_at_column()` function created by migration.
    #[instrument(skip(self))]
    async fn create_update_trigger(&self, resource_type: &str) -> Result<()> {
        let table = Self::table_name(resource_type);
        let trigger_name = format!("{}_update_timestamp", table);

        // Drop existing trigger first
        let drop_sql = format!(r#"DROP TRIGGER IF EXISTS "{trigger_name}" ON "{table}""#);
        sqlx_core::query::query(&drop_sql)
            .execute(&self.pool)
            .await
            .map_err(PostgresError::from)?;

        // Create the trigger using the shared function from migration
        let create_sql = format!(
            r#"CREATE TRIGGER "{trigger_name}"
                BEFORE UPDATE ON "{table}"
                FOR EACH ROW EXECUTE FUNCTION update_updated_at_column()"#
        );
        sqlx_core::query::query(&create_sql)
            .execute(&self.pool)
            .await
            .map_err(PostgresError::from)?;

        info!("Created update timestamp trigger for: {}", table);
        Ok(())
    }

    /// Creates the history trigger that archives rows before UPDATE/DELETE.
    #[instrument(skip(self))]
    async fn create_history_trigger(&self, resource_type: &str) -> Result<()> {
        let table = Self::table_name(resource_type);
        let trigger_name = format!("{}_history_trigger", table);

        // Create the archive function if it doesn't exist
        // This function copies the OLD row to the history table
        // Uses ON CONFLICT to overwrite existing history records (handles idempotent upserts)
        let fn_sql = r#"
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
                RETURN NEW;
            END;
            $$ LANGUAGE plpgsql;
        "#;
        sqlx_core::query::query(fn_sql)
            .execute(&self.pool)
            .await
            .map_err(PostgresError::from)?;

        // Drop existing trigger first (separate query - PostgreSQL doesn't allow multiple commands)
        let drop_sql = format!(r#"DROP TRIGGER IF EXISTS "{trigger_name}" ON "{table}""#);
        sqlx_core::query::query(&drop_sql)
            .execute(&self.pool)
            .await
            .map_err(PostgresError::from)?;

        // Create the trigger
        let create_sql = format!(
            r#"CREATE TRIGGER "{trigger_name}"
                BEFORE UPDATE OR DELETE ON "{table}"
                FOR EACH ROW EXECUTE FUNCTION archive_to_history()"#
        );
        sqlx_core::query::query(&create_sql)
            .execute(&self.pool)
            .await
            .map_err(PostgresError::from)?;

        info!("Created history trigger for: {}", table);
        Ok(())
    }

    /// Creates gateway notification trigger for App and CustomOperation resources.
    ///
    /// This trigger calls `notify_gateway_resource_change()` (created by migration 003)
    /// on INSERT/UPDATE/DELETE to enable hot-reload of gateway routes.
    #[instrument(skip(self))]
    async fn create_gateway_trigger(&self, resource_type: &str) -> Result<()> {
        let table = Self::table_name(resource_type);
        let trigger_name = format!("{}_gateway_notify", table);

        // Drop existing trigger first (separate query - PostgreSQL doesn't allow multiple commands)
        let drop_sql = format!(r#"DROP TRIGGER IF EXISTS "{trigger_name}" ON "{table}""#);
        sqlx_core::query::query(&drop_sql)
            .execute(&self.pool)
            .await
            .map_err(PostgresError::from)?;

        // Create the trigger
        // Uses notify_gateway_resource_change() function created by migration 003
        let create_sql = format!(
            r#"CREATE TRIGGER "{trigger_name}"
                AFTER INSERT OR UPDATE OR DELETE ON "{table}"
                FOR EACH ROW EXECUTE FUNCTION notify_gateway_resource_change()"#
        );
        sqlx_core::query::query(&create_sql)
            .execute(&self.pool)
            .await
            .map_err(PostgresError::from)?;

        info!("Created gateway notification trigger for: {}", table);
        Ok(())
    }

    /// Creates policy notification trigger for AccessPolicy resources.
    ///
    /// This trigger calls `notify_policy_change()` (created by migration 009)
    /// on INSERT/UPDATE/DELETE to enable hot-reload of access policies.
    #[instrument(skip(self))]
    async fn create_policy_trigger(&self, resource_type: &str) -> Result<()> {
        let table = Self::table_name(resource_type);
        let trigger_name = format!("{}_policy_notify", table);

        // Drop existing trigger first (separate query - PostgreSQL doesn't allow multiple commands)
        let drop_sql = format!(r#"DROP TRIGGER IF EXISTS "{trigger_name}" ON "{table}""#);
        sqlx_core::query::query(&drop_sql)
            .execute(&self.pool)
            .await
            .map_err(PostgresError::from)?;

        // Create the trigger
        // Uses notify_policy_change() function created by migration 009
        let create_sql = format!(
            r#"CREATE TRIGGER "{trigger_name}"
                AFTER INSERT OR UPDATE OR DELETE ON "{table}"
                FOR EACH ROW EXECUTE FUNCTION notify_policy_change()"#
        );
        sqlx_core::query::query(&create_sql)
            .execute(&self.pool)
            .await
            .map_err(PostgresError::from)?;

        info!("Created policy notification trigger for: {}", table);
        Ok(())
    }

    /// Creates search index partitions for a resource type.
    ///
    /// Creates partitions of `search_idx_reference` and `search_idx_date`
    /// for the given resource type. Uses `IF NOT EXISTS`
    /// for idempotency.
    #[instrument(skip(self))]
    async fn ensure_search_index_partitions(&self, resource_type: &str) -> Result<()> {
        let table = Self::table_name(resource_type);

        let index_tables = ["search_idx_reference", "search_idx_date"];

        for idx_table in &index_tables {
            let partition_name = format!("{idx_table}_{table}");
            let sql = format!(
                r#"CREATE TABLE IF NOT EXISTS "{partition_name}"
                   PARTITION OF {idx_table} FOR VALUES IN ('{resource_type}')"#
            );
            sqlx_core::query::query(&sql)
                .execute(&self.pool)
                .await
                .map_err(PostgresError::from)?;
        }

        debug!("Created search index partitions for: {}", resource_type);
        Ok(())
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
