//! Background maintenance for GIN indexes.
//!
//! GIN indexes are created with `fastupdate=on`, which buffers new entries in
//! a pending list. Searches scan that list linearly until autovacuum (or an
//! explicit `gin_clean_pending_list()` call) merges it into the main tree.
//!
//! Autovacuum already handles this, but its cadence is tied to dead-tuple
//! thresholds — it can lag behind a write-heavy workload and let the pending
//! list grow large enough to noticeably hurt read latency. This task runs a
//! cheap periodic flush so reads stay fast without waiting on autovacuum.

use std::time::Duration;

use sqlx_postgres::PgPool;
use tokio::task::JoinHandle;
use tracing::{debug, warn};

/// How often to flush GIN pending lists.
const DEFAULT_INTERVAL: Duration = Duration::from_secs(30);

/// Spawn a background task that periodically flushes GIN pending lists.
///
/// The task runs forever; cancel it by dropping the returned `JoinHandle`.
pub fn spawn_gin_cleaner(pool: PgPool) -> JoinHandle<()> {
    spawn_gin_cleaner_with_interval(pool, DEFAULT_INTERVAL)
}

pub fn spawn_gin_cleaner_with_interval(pool: PgPool, interval: Duration) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        // Skip the immediate first tick — let the server warm up first.
        ticker.tick().await;

        loop {
            ticker.tick().await;
            if let Err(e) = clean_all_gin_indexes(&pool).await {
                warn!("GIN pending-list cleanup failed: {}", e);
            }
        }
    })
}

/// Find every GIN index visible to the current role and flush its pending list.
async fn clean_all_gin_indexes(pool: &PgPool) -> Result<(), sqlx_core::Error> {
    let rows: Vec<(String,)> = sqlx_core::query_as::query_as(
        r#"
        SELECT n.nspname || '.' || c.relname
        FROM pg_index i
        JOIN pg_class c ON c.oid = i.indexrelid
        JOIN pg_am am ON am.oid = c.relam
        JOIN pg_namespace n ON n.oid = c.relnamespace
        WHERE am.amname = 'gin'
          AND n.nspname NOT IN ('pg_catalog', 'information_schema')
        "#,
    )
    .fetch_all(pool)
    .await?;

    for (qualified_name,) in rows {
        // gin_clean_pending_list takes regclass; pass the qualified name as text
        // and let PostgreSQL resolve it. Wrapped in its own statement so a
        // dropped/locked index doesn't abort the whole sweep.
        let sql = format!("SELECT gin_clean_pending_list('{qualified_name}'::regclass)");
        match sqlx_core::query::query(&sql).execute(pool).await {
            Ok(_) => debug!("Flushed GIN pending list: {}", qualified_name),
            Err(e) => debug!("Skipped {}: {}", qualified_name, e),
        }
    }

    Ok(())
}
