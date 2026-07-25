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
///
/// Archiving is statement-level: one set-based insert per statement, reading the
/// old rows from a transition table. A Bundle updating 200 rows issues a single
/// `UPDATE ... FROM UNNEST(...)`, so the whole batch archives in one insert.
const ARCHIVE_FN_SQL: &str = r#"
    CREATE OR REPLACE FUNCTION archive_to_history()
    RETURNS TRIGGER AS $$
    BEGIN
        EXECUTE format(
            'INSERT INTO %I_history (id, txid, created_at, updated_at, resource, status)
             SELECT id, txid, created_at, updated_at, resource, status FROM old_rows
             ON CONFLICT (id, txid) DO NOTHING',
            TG_TABLE_NAME
        );
        RETURN NULL;
    END;
    $$ LANGUAGE plpgsql;
"#;

/// Extracts `meta.profile` as a `text[]`, or NULL when the resource declares no
/// profiles. Backs the generated `profile` column on every resource table.
///
/// Declared `IMMUTABLE` because a generated column's expression may only call
/// immutable functions. A plain `resource->'meta'->'profile'` expression would
/// avoid the function entirely, but yields `jsonb`; `text[]` decodes straight
/// into `Vec<String>` and supports array-overlap (`&&`) membership tests.
///
/// Never dropped: PostgreSQL records a dependency from the generated column to
/// this function, so `DROP FUNCTION` fails while any resource table exists.
/// `CREATE OR REPLACE` with an unchanged signature stays legal, which is what
/// makes re-running this on every boot safe.
const META_PROFILES_FN_SQL: &str = r#"
    CREATE OR REPLACE FUNCTION octofhir_meta_profiles(res jsonb)
    RETURNS text[] AS $$
        SELECT CASE
            WHEN jsonb_typeof(res->'meta'->'profile') = 'array'
            THEN ARRAY(SELECT jsonb_array_elements_text(res->'meta'->'profile'))
            ELSE NULL
        END
    $$ LANGUAGE sql IMMUTABLE PARALLEL SAFE;
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
    /// Table names (lowercase) that get a whole-document GIN index on
    /// `resource`. `None` means every table gets one — see
    /// [`SchemaManager::with_document_gin_tables`].
    document_gin_tables: Option<std::collections::HashSet<String>>,
    /// Whether resource tables carry the generated `profile` column and its
    /// covering index — see [`SchemaManager::with_profile_column`].
    profile_column: bool,
}

impl SchemaManager {
    /// Creates a new `SchemaManager` with the given connection pool, giving
    /// every resource table a whole-document GIN index.
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            document_gin_tables: None,
            profile_column: false,
        }
    }

    /// Adds a generated `profile` column (from `meta.profile`) and a partial
    /// covering index on it to every resource table.
    ///
    /// Only `targetProfile` conformance reads it, and nothing else does, so this
    /// is off by default: the column is recomputed on every write regardless,
    /// and on profile-heavy data the column plus index roughly double the cost
    /// of a bulk insert. Enable it together with
    /// `validation.check_target_profile`.
    ///
    /// The column is created with the table and never added afterwards:
    /// switching this on for an existing database has no effect until the
    /// database is recreated.
    #[must_use]
    pub fn with_profile_column(mut self, on: bool) -> Self {
        self.profile_column = on;
        self
    }

    /// Restricts the whole-document GIN index on `resource` to the given
    /// resource types (matched case-insensitively).
    ///
    /// The document GIN answers a search on any element, which is what makes a
    /// search on a parameter outside `search.indexed_params` viable at all. It
    /// is also the largest index on a resource table and is maintained by every
    /// write, so a deployment that knows exactly which types get searched
    /// broadly can narrow it — or pass an empty set to drop it entirely.
    ///
    /// `None` keeps the default: one on every table.
    #[must_use]
    pub fn with_document_gin_tables<I, S>(mut self, resource_types: Option<I>) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.document_gin_tables = resource_types.map(|types| {
            types
                .into_iter()
                .map(|rt| Self::table_name(rt.as_ref()))
                .collect()
        });
        self
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
        let sql = self.build_resource_schema_sql(resource_type);
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
    fn build_resource_schema_sql(&self, resource_type: &str) -> String {
        let table = Self::table_name(resource_type);
        let history_table = format!("{}_history", table);
        let is_internal = Self::is_internal_resource(&table);
        let is_gateway = Self::is_gateway_resource(&table);
        let is_policy = Self::is_policy_resource(&table);

        let mut sql = String::with_capacity(2048);

        // Main table.
        //
        // Every UPDATE writes a new row version and none of them can be HOT
        // (`resource` carries index entries), so `fillfactor` leaves room to
        // keep the new version on the same page, and autovacuum runs far more
        // eagerly than the stock 0.2 scale factor — at that setting a
        // million-row table waits for 200k dead tuples.
        // `profile` is generated from `meta.profile` and exists only to serve
        // `targetProfile` conformance, so it is created with the table or not at
        // all — see `with_profile_column`. Flipping the setting on an existing
        // database is not supported: the database is recreated instead, which
        // also avoids the table rewrite `ADD COLUMN ... STORED` would impose.
        let profile_col = if self.profile_column {
            ",\n                profile TEXT[] GENERATED ALWAYS AS (octofhir_meta_profiles(resource)) STORED"
        } else {
            ""
        };
        sql.push_str(&format!(
            "CREATE TABLE IF NOT EXISTS \"{table}\" (\n\
                id TEXT PRIMARY KEY,\n\
                txid BIGINT NOT NULL,\n\
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),\n\
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),\n\
                resource JSONB NOT NULL,\n\
                status resource_status NOT NULL DEFAULT 'created'{profile_col}\n\
            );\n\
            ALTER TABLE \"{table}\" SET (\n\
                toast_tuple_target = 8160,\n\
                fillfactor = 90,\n\
                autovacuum_vacuum_scale_factor = 0.02,\n\
                autovacuum_analyze_scale_factor = 0.01,\n\
                autovacuum_vacuum_cost_limit = 2000\n\
            );\n"
        ));

        // Update trigger (drop + create)
        let update_trigger = format!("{}_update_timestamp", table);
        sql.push_str(&format!(
            "DROP TRIGGER IF EXISTS \"{update_trigger}\" ON \"{table}\";\n\
             CREATE TRIGGER \"{update_trigger}\" BEFORE UPDATE ON \"{table}\" \
             FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();\n"
        ));

        // History table + triggers (skipped for internal resources).
        //
        // One statement-level trigger per event, each with a transition table,
        // so a batched write archives in a single set-based insert. UPDATE and
        // DELETE need separate triggers: a trigger declaring `REFERENCING` may
        // only name one event.
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
                );\n\
                ALTER TABLE \"{history_table}\" SET (toast_tuple_target = 8160);\n"
            ));
            let update_hist = format!("{}_history_update", table);
            let delete_hist = format!("{}_history_delete", table);
            sql.push_str(&format!(
                "DROP TRIGGER IF EXISTS \"{update_hist}\" ON \"{table}\";\n\
                 CREATE TRIGGER \"{update_hist}\" AFTER UPDATE ON \"{table}\" \
                 REFERENCING OLD TABLE AS old_rows \
                 FOR EACH STATEMENT EXECUTE FUNCTION archive_to_history();\n\
                 DROP TRIGGER IF EXISTS \"{delete_hist}\" ON \"{table}\";\n\
                 CREATE TRIGGER \"{delete_hist}\" AFTER DELETE ON \"{table}\" \
                 REFERENCING OLD TABLE AS old_rows \
                 FOR EACH STATEMENT EXECUTE FUNCTION archive_to_history();\n"
            ));
        }

        // `updated_at`, `created_at`, `txid` and `status` stay unindexed: an
        // index on `updated_at` alone costs every table its HOT updates, since
        // the timestamp trigger touches the column on every write.
        //
        // History carries only its `(id, txid)` primary key, which also serves
        // lookups by `id` as the leading column.

        // Whole-document GIN — on every table unless the deployment narrowed the
        // set. It is what keeps a search on a parameter outside
        // `search.indexed_params` from becoming a sequential scan.
        if self
            .document_gin_tables
            .as_ref()
            .is_none_or(|tables| tables.contains(&table))
        {
            sql.push_str(&format!(
                "CREATE INDEX IF NOT EXISTS \"idx_{table}_gin\" ON \"{table}\" \
                 USING GIN (resource jsonb_path_ops) \
                 WITH (fastupdate=on);\n"
            ));
        }

        // Generated `profile` column plus its covering index, for the
        // `targetProfile` fast path. Opt-in (see `with_profile_column`) because
        // every write recomputes the column whether or not anything reads it.
        //
        // Reading the profiles a referenced resource declares must not touch the
        // heap: the heap tuple drags in the whole `resource` JSONB (and its
        // TOAST chunks) for what is a set-membership test against a handful of
        // canonicals.
        //
        // The index is partial on `profile IS NOT NULL` so it stays small —
        // resources declaring no profile (all of plain R4) contribute no index
        // tuple, and the primary key keeps its current width. Callers must spell
        // `profile IS NOT NULL` in the query for the planner to consider it; the
        // fast path only cares about profiled rows anyway, since existence was
        // already settled by the batched existence check.
        //
        // `status` rides along in INCLUDE for the same reason: without it the
        // `status != 'deleted'` filter forces a heap fetch and the index-only
        // scan degrades into exactly the read we were avoiding.
        // Both indexes are partial on `profile IS NOT NULL`, so a deployment on
        // plain R4 — where nothing declares a profile — carries two empty
        // indexes rather than two full ones.
        //
        // The GIN index serves `_profile` search: an exact match renders as
        // `profile @> ARRAY[$1]`, and `@>` being strict is what lets the planner
        // prove the partial-index predicate holds.
        if self.profile_column {
            sql.push_str(&format!(
                "CREATE INDEX IF NOT EXISTS \"idx_{table}_profile_cov\" ON \"{table}\" \
                 (id) INCLUDE (profile, status) WHERE profile IS NOT NULL;\n\
                 CREATE INDEX IF NOT EXISTS \"idx_{table}_profile_gin\" ON \"{table}\" \
                 USING GIN (profile) WHERE profile IS NOT NULL;\n"
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

    /// Ensure the shared `archive_to_history()` and `octofhir_meta_profiles()`
    /// functions exist. Call once before parallel resource-schema creation —
    /// `octofhir_meta_profiles` backs the generated `profile` column, so it must
    /// be in place before any table referencing it is created.
    pub async fn ensure_archive_function(pool: &PgPool) -> Result<()> {
        sqlx_core::query::query(AssertSqlSafe((ARCHIVE_FN_SQL).to_string()))
            .execute(pool)
            .await
            .map_err(PostgresError::from)?;
        sqlx_core::query::query(AssertSqlSafe((META_PROFILES_FN_SQL).to_string()))
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
            // Both underscores are escaped — unescaped they are single-character
            // wildcards, and `%_history` would drop `familymemberhistory` from
            // the list of resource tables.
            "SELECT table_name FROM information_schema.tables
             WHERE table_schema = 'public'
             AND table_name NOT LIKE '%\\_history' ESCAPE '\\'
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
