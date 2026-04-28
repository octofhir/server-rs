//! Async, batched search-index writer.
//!
//! The resource transaction stays synchronous (so the response implies the
//! row is durable). Search-index I/O — DELETE + UNNEST INSERT against the
//! partitioned `search_idx_*` tables — is handed to a worker task that
//! coalesces many resources into one transaction per batch. Reuses
//! [`crate::search_index::BatchIndexBuffer`] from the bulk-import path.
//!
//! Search-by-property reads see the new rows after at most one batch flush
//! (`flush_interval`, default 5 ms). Read-by-id stays strongly consistent
//! because it reads the resource table directly.

use std::sync::Arc;
use std::time::Duration;

use sqlx_postgres::PgPool;
use tokio::sync::mpsc;
use tokio::time::Instant;

use octofhir_core::search_index::{ExtractedDate, ExtractedReference};
use octofhir_storage::StorageError;

use crate::search_index;

/// Op kind controls whether stale rows are deleted before insert:
/// `Create` → insert only (no rows can exist yet); `Update` → DELETE then
/// INSERT; `Delete` → DELETE only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexOp {
    Create,
    Update,
    Delete,
}

/// One unit of index work. `refs` / `dates` are pre-extracted on the caller
/// side so the worker doesn't need the search-parameter registry or the
/// original JSONB.
#[derive(Debug)]
pub struct IndexJob {
    pub op: IndexOp,
    pub resource_type: String,
    pub resource_id: String,
    pub refs: Vec<ExtractedReference>,
    pub dates: Vec<ExtractedDate>,
}

#[derive(Debug, Clone, Copy)]
pub struct IndexWriterConfig {
    /// Bounded channel capacity. Full queue → enqueue falls back to inline
    /// sync flush.
    pub queue_capacity: usize,
    pub batch_max: usize,
    /// Wall-clock the worker waits for additional jobs after the first one
    /// arrives before forcing a flush.
    pub flush_interval: Duration,
}

impl Default for IndexWriterConfig {
    fn default() -> Self {
        Self {
            queue_capacity: 4_096,
            batch_max: 256,
            flush_interval: Duration::from_millis(5),
        }
    }
}

/// Clonable handle. All clones share the same channel + fallback pool, so
/// `PostgresStorage::Clone` keeps working unchanged.
#[derive(Clone, Debug)]
pub struct AsyncIndexWriter {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    sender: mpsc::Sender<IndexJob>,
    fallback_pool: PgPool,
}

impl AsyncIndexWriter {
    /// Spawn the worker on the current Tokio runtime.
    pub fn start(pool: PgPool, config: IndexWriterConfig) -> Self {
        let (tx, rx) = mpsc::channel(config.queue_capacity);
        let worker_pool = pool.clone();
        tokio::spawn(async move {
            run_worker(rx, worker_pool, config.batch_max, config.flush_interval).await;
        });

        Self {
            inner: Arc::new(Inner {
                sender: tx,
                fallback_pool: pool,
            }),
        }
    }

    /// Enqueue a job. If the queue is full the call degrades to an inline
    /// synchronous flush so an index update is never lost.
    pub async fn submit(&self, job: IndexJob) -> Result<(), StorageError> {
        match self.inner.sender.try_send(job) {
            Ok(()) => Ok(()),
            Err(mpsc::error::TrySendError::Full(job)) => {
                flush_one(&self.inner.fallback_pool, &job).await
            }
            Err(mpsc::error::TrySendError::Closed(_)) => Err(StorageError::internal(
                "async index worker channel is closed",
            )),
        }
    }
}

async fn run_worker(
    mut rx: mpsc::Receiver<IndexJob>,
    pool: PgPool,
    batch_max: usize,
    flush_interval: Duration,
) {
    let mut batch: Vec<IndexJob> = Vec::with_capacity(batch_max);
    loop {
        let first = match rx.recv().await {
            Some(job) => job,
            None => break,
        };
        batch.push(first);

        let deadline = Instant::now() + flush_interval;
        while batch.len() < batch_max {
            tokio::select! {
                biased;
                next = rx.recv() => match next {
                    Some(job) => batch.push(job),
                    None => break,
                },
                _ = tokio::time::sleep_until(deadline) => break,
            }
        }

        if let Err(e) = flush_batch(&pool, &batch).await {
            // Errors don't propagate back to the producing request; affected
            // rows can be rebuilt with `$reindex`.
            tracing::warn!(
                error = %e,
                batch_size = batch.len(),
                "async indexer batch flush failed"
            );
        }
        batch.clear();
    }

    if !batch.is_empty()
        && let Err(e) = flush_batch(&pool, &batch).await
    {
        tracing::warn!(
            error = %e,
            batch_size = batch.len(),
            "async indexer final flush failed during shutdown"
        );
    }
}

/// One transaction per batch: group stale-row DELETEs by `resource_type`
/// (one statement per group via `resource_id = ANY(...)`, partition pruning
/// stays effective), then one UNNEST INSERT per index table for all Create
/// + Update jobs.
async fn flush_batch(pool: &PgPool, batch: &[IndexJob]) -> Result<(), StorageError> {
    if batch.is_empty() {
        return Ok(());
    }

    let mut tx = pool.begin().await.map_err(|e| {
        StorageError::transaction_error(format!("Failed to begin async-index tx: {e}"))
    })?;

    let mut delete_groups: std::collections::HashMap<&str, Vec<String>> =
        std::collections::HashMap::new();
    for job in batch {
        if matches!(job.op, IndexOp::Update | IndexOp::Delete) {
            delete_groups
                .entry(job.resource_type.as_str())
                .or_default()
                .push(job.resource_id.clone());
        }
    }
    for (resource_type, ids) in &delete_groups {
        search_index::delete_search_indexes_batch_with_tx(&mut tx, resource_type, ids).await?;
    }

    let mut buffer = search_index::BatchIndexBuffer::new();
    for job in batch {
        if matches!(job.op, IndexOp::Delete) {
            continue;
        }
        buffer.extend_with(&job.resource_type, &job.resource_id, &job.refs, &job.dates);
    }
    if !buffer.is_empty() {
        buffer.flush_with_tx(&mut tx).await?;
    }

    tx.commit().await.map_err(|e| {
        StorageError::transaction_error(format!("Failed to commit async-index tx: {e}"))
    })?;

    Ok(())
}

/// Sync fallback for a single job (used on full queue).
async fn flush_one(pool: &PgPool, job: &IndexJob) -> Result<(), StorageError> {
    let mut tx = pool.begin().await.map_err(|e| {
        StorageError::transaction_error(format!("Failed to begin sync-fallback index tx: {e}"))
    })?;

    if matches!(job.op, IndexOp::Update | IndexOp::Delete) {
        search_index::delete_search_indexes_with_tx(&mut tx, &job.resource_type, &job.resource_id)
            .await?;
    }

    if matches!(job.op, IndexOp::Create | IndexOp::Update) {
        let mut buffer = search_index::BatchIndexBuffer::new();
        buffer.extend_with(&job.resource_type, &job.resource_id, &job.refs, &job.dates);
        if !buffer.is_empty() {
            buffer.flush_with_tx(&mut tx).await?;
        }
    }

    tx.commit().await.map_err(|e| {
        StorageError::transaction_error(format!("Failed to commit sync-fallback index tx: {e}"))
    })?;

    Ok(())
}
