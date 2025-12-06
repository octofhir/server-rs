//! Schema management for the PostgreSQL storage backend.
//!
//! This module handles database schema operations such as table creation,
//! index management, and schema introspection. It uses a table-per-resource
//! pattern where each FHIR resource type gets its own table.

use std::sync::Arc;

use dashmap::DashSet;
use sqlx_postgres::PgPool;
use tracing::{debug, info, instrument};

use crate::error::{PostgresError, Result};

/// Manages the database schema for FHIR resources.
///
/// The `SchemaManager` is responsible for:
/// - Creating and managing resource tables dynamically
/// - Creating history tables with triggers for versioning
/// - Managing indexes for efficient JSONB search
/// - Caching table existence to avoid repeated database checks
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
    /// Cache of tables that have been verified to exist.
    /// Uses DashSet for thread-safe concurrent access.
    created_tables: Arc<DashSet<String>>,
}

impl SchemaManager {
    /// Creates a new `SchemaManager` with the given connection pool.
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            created_tables: Arc::new(DashSet::new()),
        }
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

    /// Ensures the table exists for the given resource type.
    ///
    /// This method is idempotent - calling it multiple times for the same
    /// resource type is safe and efficient due to caching.
    ///
    /// # Process
    ///
    /// 1. Check the in-memory cache
    /// 2. If not cached, check if the table exists in the database
    /// 3. If the table doesn't exist, create it along with history table,
    ///    indexes, and triggers
    /// 4. Add to cache
    ///
    /// # Errors
    ///
    /// Returns an error if database queries fail or table creation fails.
    #[instrument(skip(self), fields(resource_type = %resource_type))]
    pub async fn ensure_table(&self, resource_type: &str) -> Result<()> {
        let table = Self::table_name(resource_type);

        // Check cache first (fast path)
        if self.created_tables.contains(&table) {
            debug!("Table {} found in cache", table);
            return Ok(());
        }

        // Check database
        if self.table_exists(&table).await? {
            debug!("Table {} exists in database, adding to cache", table);
            self.created_tables.insert(table);
            return Ok(());
        }

        // Create table and related objects
        info!("Creating schema for resource type: {}", resource_type);
        self.create_resource_table(resource_type).await?;
        self.create_history_table(resource_type).await?;
        self.create_indexes(resource_type).await?;
        self.create_history_trigger(resource_type).await?;

        // Add gateway notification trigger for App and CustomOperation resources
        if Self::is_gateway_resource(&table) {
            self.create_gateway_trigger(resource_type).await?;
        }

        // Add policy notification trigger for AccessPolicy resources
        if Self::is_policy_resource(&table) {
            self.create_policy_trigger(resource_type).await?;
        }

        self.created_tables.insert(table);
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

    /// Checks if a table exists in the database.
    #[instrument(skip(self))]
    async fn table_exists(&self, table: &str) -> Result<bool> {
        let row: Option<(bool,)> = sqlx_core::query_as::query_as(
            "SELECT EXISTS (
                SELECT FROM information_schema.tables
                WHERE table_schema = 'public' AND table_name = $1
            )",
        )
        .bind(table)
        .fetch_optional(&self.pool)
        .await
        .map_err(PostgresError::from)?;

        Ok(row.map(|(exists,)| exists).unwrap_or(false))
    }

    /// Creates the main resource table.
    #[instrument(skip(self))]
    async fn create_resource_table(&self, resource_type: &str) -> Result<()> {
        let table = Self::table_name(resource_type);

        // Using format! for table name since it can't be parameterized
        // The table name is derived from resource_type which should be validated
        // Note: resource_type is not stored as a column since:
        // 1. The table name already indicates the resource type
        // 2. The JSONB resource field contains resourceType
        let sql = format!(
            r#"
            CREATE TABLE "{table}" (
                id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
                txid BIGINT NOT NULL REFERENCES _transaction(txid),
                ts TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                resource JSONB NOT NULL,
                status resource_status NOT NULL DEFAULT 'created'
            )
            "#
        );

        sqlx_core::query::query(&sql)
            .execute(&self.pool)
            .await
            .map_err(PostgresError::from)?;

        info!("Created table: {}", table);
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
            CREATE TABLE "{history_table}" (
                id UUID NOT NULL,
                txid BIGINT NOT NULL,
                ts TIMESTAMPTZ NOT NULL,
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
        let history_table = format!("{}_history", table);

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

        // Index on ts for time-based sorting and filtering
        let ts_sql = format!(r#"CREATE INDEX IF NOT EXISTS "idx_{table}_ts" ON "{table}"(ts)"#);
        sqlx_core::query::query(&ts_sql)
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

        // History table index on timestamp for history queries
        let history_ts_sql = format!(
            r#"CREATE INDEX IF NOT EXISTS "idx_{history_table}_ts" ON "{history_table}"(ts)"#
        );
        sqlx_core::query::query(&history_ts_sql)
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

        info!("Created indexes for: {}", table);
        Ok(())
    }

    /// Creates the history trigger that archives rows before UPDATE/DELETE.
    #[instrument(skip(self))]
    async fn create_history_trigger(&self, resource_type: &str) -> Result<()> {
        let table = Self::table_name(resource_type);
        let trigger_name = format!("{}_history_trigger", table);

        // Create the archive function if it doesn't exist
        // This function copies the OLD row to the history table
        let fn_sql = r#"
            CREATE OR REPLACE FUNCTION archive_to_history()
            RETURNS TRIGGER AS $$
            BEGIN
                EXECUTE format(
                    'INSERT INTO %I_history (id, txid, ts, resource, status)
                     VALUES ($1, $2, $3, $4, $5)',
                    TG_TABLE_NAME
                ) USING OLD.id, OLD.txid, OLD.ts, OLD.resource, OLD.status;
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

    /// Clears the internal cache of created tables.
    ///
    /// This can be useful after schema changes or for testing.
    pub fn clear_cache(&self) {
        self.created_tables.clear();
    }

    /// Returns the number of tables in the cache.
    #[must_use]
    pub fn cache_size(&self) -> usize {
        self.created_tables.len()
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
