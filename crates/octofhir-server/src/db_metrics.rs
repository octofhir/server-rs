//! PostgreSQL statistics on `/metrics`: connections by state, lock waits, the longest open
//! transaction, transaction and block counters, database and table sizes, and resource counts.
//!
//! Read from `pg_stat_*` views when Prometheus scrapes, at most once per [`MIN_INTERVAL`] however
//! many scrapers there are, and abandoned after [`TIMEOUT`] so a slow database never stalls a
//! scrape. These describe the database, not the instance: every instance reports the same values,
//! so dashboards aggregate them with `max`, not `sum`.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use metrics::{Unit, counter, describe_counter, describe_gauge, gauge};
use sqlx_core::query_as::query_as;
use sqlx_postgres::PgPool;

pub mod names {
    pub const PG_CONNECTIONS: &str = "octofhir_pg_connections";
    pub const PG_CONNECTIONS_WAITING: &str = "octofhir_pg_connections_waiting_on_locks";
    pub const PG_LONGEST_TRANSACTION_SECONDS: &str = "octofhir_pg_longest_transaction_seconds";
    pub const PG_TRANSACTIONS_TOTAL: &str = "octofhir_pg_transactions_total";
    pub const PG_BLOCKS_TOTAL: &str = "octofhir_pg_blocks_total";
    pub const PG_DEADLOCKS_TOTAL: &str = "octofhir_pg_deadlocks_total";
    pub const PG_TEMP_BYTES_TOTAL: &str = "octofhir_pg_temp_bytes_total";
    pub const PG_ROWS_TOTAL: &str = "octofhir_pg_rows_total";
    pub const PG_DATABASE_SIZE_BYTES: &str = "octofhir_pg_database_size_bytes";
    pub const PG_TABLE_ROWS: &str = "octofhir_pg_table_rows";
    pub const PG_TABLE_DEAD_ROWS: &str = "octofhir_pg_table_dead_rows";
    pub const PG_TABLE_SIZE_BYTES: &str = "octofhir_pg_table_size_bytes";
}

const MIN_INTERVAL: Duration = Duration::from_secs(10);
const TIMEOUT: Duration = Duration::from_secs(2);

/// Backend states reported by `pg_stat_activity`; absent ones are set to 0 so gauges don't stick.
const STATES: &[&str] = &[
    "active",
    "idle",
    "idle in transaction",
    "idle in transaction (aborted)",
    "fastpath function call",
];

static LAST_RUN_SECS: AtomicU64 = AtomicU64::new(0);

/// Refreshes the PostgreSQL gauges unless they were refreshed less than [`MIN_INTERVAL`] ago.
/// `resource_types` maps table names back to resource types for `octofhir_fhir_resources`.
pub async fn collect(pool: &PgPool, resource_types: &HashSet<String>) {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let last = LAST_RUN_SECS.load(Ordering::Relaxed);
    if now.saturating_sub(last) < MIN_INTERVAL.as_secs()
        || LAST_RUN_SECS
            .compare_exchange(last, now, Ordering::AcqRel, Ordering::Relaxed)
            .is_err()
    {
        return;
    }
    match tokio::time::timeout(TIMEOUT, refresh(pool, resource_types)).await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => tracing::debug!(%error, "PostgreSQL statistics unavailable"),
        Err(_) => tracing::debug!("PostgreSQL statistics timed out"),
    }
}

async fn refresh(pool: &PgPool, resource_types: &HashSet<String>) -> Result<(), sqlx_core::Error> {
    let activity: Vec<(String, i64, i64)> = query_as(
        "SELECT coalesce(state, 'unknown'), count(*)::bigint, \
                count(*) FILTER (WHERE wait_event_type = 'Lock')::bigint \
         FROM pg_stat_activity \
         WHERE datname = current_database() AND backend_type = 'client backend' \
         GROUP BY 1",
    )
    .fetch_all(pool)
    .await?;
    let by_state: HashMap<&str, i64> = activity.iter().map(|(s, n, _)| (s.as_str(), *n)).collect();
    for state in STATES {
        gauge!(names::PG_CONNECTIONS, "state" => *state)
            .set(by_state.get(state).copied().unwrap_or(0) as f64);
    }
    let waiting: i64 = activity.iter().map(|(_, _, w)| w).sum();
    gauge!(names::PG_CONNECTIONS_WAITING).set(waiting as f64);

    let (longest,): (f64,) = query_as(
        "SELECT coalesce(extract(epoch FROM max(now() - xact_start)), 0)::float8 \
         FROM pg_stat_activity \
         WHERE datname = current_database() AND backend_type = 'client backend' \
           AND state <> 'idle' AND pid <> pg_backend_pid()",
    )
    .fetch_one(pool)
    .await?;
    gauge!(names::PG_LONGEST_TRANSACTION_SECONDS).set(longest);

    type DatabaseRow = (i64, i64, i64, i64, i64, i64, i64, i64, i64, i64, i64, i64);
    let row: DatabaseRow = query_as(
        "SELECT xact_commit, xact_rollback, blks_read, blks_hit, deadlocks, temp_bytes, \
                tup_returned, tup_fetched, tup_inserted, tup_updated, tup_deleted, \
                pg_database_size(datname) \
         FROM pg_stat_database WHERE datname = current_database()",
    )
    .fetch_one(pool)
    .await?;
    let (
        commit,
        rollback,
        read,
        hit,
        deadlocks,
        temp,
        returned,
        fetched,
        inserted,
        updated,
        deleted,
        size,
    ) = row;
    let absolute = |value: i64| value.max(0) as u64;
    counter!(names::PG_TRANSACTIONS_TOTAL, "outcome" => "commit").absolute(absolute(commit));
    counter!(names::PG_TRANSACTIONS_TOTAL, "outcome" => "rollback").absolute(absolute(rollback));
    counter!(names::PG_BLOCKS_TOTAL, "source" => "cache").absolute(absolute(hit));
    counter!(names::PG_BLOCKS_TOTAL, "source" => "disk").absolute(absolute(read));
    counter!(names::PG_DEADLOCKS_TOTAL).absolute(absolute(deadlocks));
    counter!(names::PG_TEMP_BYTES_TOTAL).absolute(absolute(temp));
    for (operation, value) in [
        ("returned", returned),
        ("fetched", fetched),
        ("inserted", inserted),
        ("updated", updated),
        ("deleted", deleted),
    ] {
        counter!(names::PG_ROWS_TOTAL, "operation" => operation).absolute(absolute(value));
    }
    gauge!(names::PG_DATABASE_SIZE_BYTES).set(size as f64);

    let tables: Vec<(String, i64, i64, i64)> = query_as(
        "SELECT relname::text, n_live_tup, n_dead_tup, pg_total_relation_size(relid) \
         FROM pg_stat_user_tables WHERE schemaname = 'public'",
    )
    .fetch_all(pool)
    .await?;
    // Resource tables are named after the lower-cased resource type.
    let by_table: HashMap<String, &str> = resource_types
        .iter()
        .map(|rt| (rt.to_lowercase(), rt.as_str()))
        .collect();
    // Every resource type has a table (and a history table), most of them empty: export only
    // tables that hold rows, so a deployment's series count follows the data it actually has.
    for (table, live, dead, bytes) in tables
        .into_iter()
        .filter(|(_, live, dead, _)| live + dead > 0)
    {
        gauge!(names::PG_TABLE_ROWS, "table" => table.clone()).set(live as f64);
        gauge!(names::PG_TABLE_DEAD_ROWS, "table" => table.clone()).set(dead as f64);
        gauge!(names::PG_TABLE_SIZE_BYTES, "table" => table.clone()).set(bytes as f64);
        if let Some(resource_type) = by_table.get(&table) {
            crate::metrics::set_fhir_resource_count(resource_type, live.max(0) as u64);
        }
    }
    Ok(())
}

/// `# HELP` texts, registered with the other metric descriptions.
pub fn describe() {
    describe_gauge!(
        names::PG_CONNECTIONS,
        "PostgreSQL client connections to this database, by state"
    );
    describe_gauge!(
        names::PG_CONNECTIONS_WAITING,
        "PostgreSQL connections waiting on a lock"
    );
    describe_gauge!(
        names::PG_LONGEST_TRANSACTION_SECONDS,
        Unit::Seconds,
        "Age of the oldest open (non-idle) transaction"
    );
    describe_counter!(
        names::PG_TRANSACTIONS_TOTAL,
        "PostgreSQL transactions, by outcome (commit, rollback)"
    );
    describe_counter!(
        names::PG_BLOCKS_TOTAL,
        "Data blocks read, by source: shared buffer cache or disk"
    );
    describe_counter!(
        names::PG_DEADLOCKS_TOTAL,
        "Deadlocks detected in this database"
    );
    describe_counter!(
        names::PG_TEMP_BYTES_TOTAL,
        Unit::Bytes,
        "Bytes written to temporary files by queries"
    );
    describe_counter!(
        names::PG_ROWS_TOTAL,
        "Rows returned, fetched, inserted, updated and deleted"
    );
    describe_gauge!(
        names::PG_DATABASE_SIZE_BYTES,
        Unit::Bytes,
        "Size of the database"
    );
    describe_gauge!(names::PG_TABLE_ROWS, "Estimated live rows, by table");
    describe_gauge!(
        names::PG_TABLE_DEAD_ROWS,
        "Dead rows awaiting vacuum, by table"
    );
    describe_gauge!(
        names::PG_TABLE_SIZE_BYTES,
        Unit::Bytes,
        "Table size including indexes and TOAST, by table"
    );
}
