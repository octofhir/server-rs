//! PostgreSQL transaction implementation with ACID guarantees.
//!
//! This module provides native PostgreSQL transaction support for FHIR operations,
//! ensuring atomicity, consistency, isolation, and durability for multi-resource
//! operations like Bundle transactions.

use async_trait::async_trait;
use serde_json::Value;
use sqlx_core::query_scalar::query_scalar;
use sqlx_postgres::PgTransaction;
use std::sync::Arc;
use tokio::sync::Mutex;

use octofhir_search::SearchParameterRegistry;
use octofhir_storage::{
    BatchUpdateOutcome, IncludeLimits, ProposedResource, RawSearchResult, SearchParams,
    SearchResult, StorageError, StoredResource, Transaction,
};

use crate::queries;

/// PostgreSQL transaction wrapper providing ACID guarantees.
///
/// This struct wraps an sqlx PostgreSQL transaction and provides
/// transaction-aware FHIR CRUD operations. The transaction automatically
/// rolls back on drop if not explicitly committed.
///
/// Uses Mutex for interior mutability since sqlx requires mutable access
/// to transactions for all operations, including reads.
pub struct PostgresTransaction {
    /// The underlying sqlx transaction.
    /// Wrapped in Mutex for thread-safe interior mutability.
    /// Wrapped in Option so we can take ownership during commit/rollback.
    /// Box allows us to work without lifetime parameters.
    tx: Mutex<Option<Box<PgTransaction<'static>>>>,
    /// Registry used to keep search indexes in sync inside the same transaction.
    search_registry: Option<Arc<SearchParameterRegistry>>,
    /// Lazily allocated `_transaction` row id reused across all writes inside
    /// this transaction. Allocated on the first create/update/delete call —
    /// subsequent writes reuse it instead of inserting a new `_transaction`
    /// row per operation.
    txid: Mutex<Option<i64>>,
}

impl PostgresTransaction {
    /// Creates a new PostgreSQL transaction.
    pub fn new(
        tx: PgTransaction<'static>,
        search_registry: Option<Arc<SearchParameterRegistry>>,
    ) -> Self {
        Self {
            tx: Mutex::new(Some(Box::new(tx))),
            search_registry,
            txid: Mutex::new(None),
        }
    }
}

/// Allocates a `_transaction` row inside the given sqlx transaction if no
/// `txid` has been cached yet, and returns the cached/allocated value.
async fn ensure_txid(
    txid_slot: &mut Option<i64>,
    tx: &mut PgTransaction<'_>,
) -> Result<i64, StorageError> {
    if let Some(t) = *txid_slot {
        return Ok(t);
    }
    let t: i64 = query_scalar("WITH allocated AS MATERIALIZED (SELECT nextval('_transaction_txid_seq') AS txid) SELECT set_config('octofhir.write_version', txid::text, true)::bigint FROM allocated")
        .fetch_one(&mut **tx)
        .await
        .map_err(|e| StorageError::internal(format!("Failed to allocate txid: {e}")))?;
    *txid_slot = Some(t);
    Ok(t)
}

#[async_trait]
impl Transaction for PostgresTransaction {
    async fn vread(
        &self,
        resource_type: &str,
        id: &str,
        version: &str,
    ) -> Result<Option<StoredResource>, StorageError> {
        let mut guard = self.tx.lock().await;
        let tx = guard
            .as_deref_mut()
            .ok_or_else(|| StorageError::transaction_error("Transaction already completed"))?;
        queries::crud::vread_with_tx(tx, resource_type, id, version).await
    }

    async fn current_snapshot(
        &self,
        resource_type: &str,
        id: &str,
    ) -> Result<Option<octofhir_storage::RawHistoryEntry>, StorageError> {
        let mut guard = self.tx.lock().await;
        let tx = guard
            .as_deref_mut()
            .ok_or_else(|| StorageError::transaction_error("Transaction already completed"))?;
        queries::crud::resource_snapshot(&mut **tx, resource_type, id, None).await
    }

    async fn version_snapshot(
        &self,
        resource_type: &str,
        id: &str,
        version: &str,
    ) -> Result<Option<octofhir_storage::RawHistoryEntry>, StorageError> {
        let mut guard = self.tx.lock().await;
        let tx = guard
            .as_deref_mut()
            .ok_or_else(|| StorageError::transaction_error("Transaction already completed"))?;
        queries::crud::version_snapshot(&mut **tx, resource_type, id, version).await
    }

    async fn lock_reference_snapshot(
        &mut self,
        reference: &octofhir_core::FhirReference,
    ) -> Result<Option<Value>, StorageError> {
        let mut guard = self.tx.lock().await;
        let tx = guard
            .as_deref_mut()
            .ok_or_else(|| StorageError::transaction_error("Transaction already completed"))?;
        queries::crud::lock_reference_snapshot_with_tx(tx, reference).await
    }

    async fn lock_reference_versions(
        &mut self,
        references: &[octofhir_core::FhirReference],
    ) -> Result<std::collections::HashSet<String>, StorageError> {
        let mut guard = self.tx.lock().await;
        let tx = guard
            .as_deref_mut()
            .ok_or_else(|| StorageError::transaction_error("Transaction already completed"))?;
        queries::crud::lock_reference_versions_with_tx(tx, references).await
    }

    async fn lock_reference_snapshots(
        &mut self,
        references: &[octofhir_core::FhirReference],
    ) -> Result<std::collections::HashMap<String, Value>, StorageError> {
        let mut guard = self.tx.lock().await;
        let tx = guard
            .as_deref_mut()
            .ok_or_else(|| StorageError::transaction_error("Transaction already completed"))?;
        queries::crud::lock_reference_snapshots_with_tx(tx, references).await
    }

    async fn lock_conditional_writes(&mut self) -> Result<(), StorageError> {
        let mut guard = self.tx.lock().await;
        let tx = guard
            .as_deref_mut()
            .ok_or_else(|| StorageError::transaction_error("Transaction already completed"))?;
        queries::crud::lock_resource_writes(tx, true).await
    }

    async fn save_async_job_checkpoint(
        &mut self,
        job_id: uuid::Uuid,
        execution_id: uuid::Uuid,
        key: Option<&str>,
        result: &Value,
    ) -> Result<(), StorageError> {
        let mut guard = self.tx.lock().await;
        let tx = guard
            .as_deref_mut()
            .ok_or_else(|| StorageError::transaction_error("Transaction already completed"))?;
        let updated = sqlx_core::query::query(
            "UPDATE async_jobs SET \
             checkpoint = CASE WHEN $3::text IS NULL THEN checkpoint \
                 ELSE jsonb_set(checkpoint, ARRAY[$3::text], $4::jsonb, true) END, \
             result = CASE WHEN $3::text IS NULL THEN $4::jsonb ELSE result END, \
             status = CASE WHEN $3::text IS NULL THEN 'completed' ELSE status END, \
             progress = CASE WHEN $3::text IS NULL THEN 1.0 ELSE progress END, \
             completed_at = CASE WHEN $3::text IS NULL THEN clock_timestamp() ELSE completed_at END \
             WHERE id = $1 AND execution_id = $2 AND status = 'in_progress' \
             AND expires_at > clock_timestamp() AND lease_expires_at > clock_timestamp() \
             AND ($3::text IS NULL OR NOT checkpoint ? $3::text OR checkpoint -> $3::text = $4::jsonb)",
        ).bind(job_id).bind(execution_id).bind(key).bind(result).execute(&mut **tx).await
            .map_err(|error| StorageError::transaction_error(error.to_string()))?;
        if updated.rows_affected() != 1 {
            return Err(StorageError::transaction_error(
                "Async execution claim is unavailable",
            ));
        }
        Ok(())
    }

    async fn commit(mut self: Box<Self>) -> Result<(), StorageError> {
        if let Some(tx) = self.tx.lock().await.take() {
            tx.commit().await.map_err(|e| {
                StorageError::transaction_error(format!("Failed to commit transaction: {}", e))
            })?;
            tracing::debug!("Transaction committed successfully");
        }
        Ok(())
    }

    async fn rollback(mut self: Box<Self>) -> Result<(), StorageError> {
        if let Some(tx) = self.tx.lock().await.take() {
            tx.rollback().await.map_err(|e| {
                StorageError::transaction_error(format!("Failed to rollback transaction: {}", e))
            })?;
            tracing::debug!("Transaction rolled back successfully");
        }
        Ok(())
    }

    async fn create(&mut self, resource: &Value) -> Result<StoredResource, StorageError> {
        let mut tx_guard = self.tx.lock().await;
        let tx = tx_guard.as_deref_mut().ok_or_else(|| {
            StorageError::transaction_error(
                "Transaction already completed (committed or rolled back)",
            )
        })?;
        let mut txid_guard = self.txid.lock().await;
        let txid = ensure_txid(&mut txid_guard, tx).await?;
        drop(txid_guard);
        let result = queries::crud::create_with_tx(tx, resource, Some(txid)).await?;
        Ok(result)
    }

    async fn update(
        &mut self,
        resource: &Value,
        if_match: Option<&str>,
    ) -> Result<StoredResource, StorageError> {
        Ok(self.update_with_snapshot(resource, if_match).await?.stored)
    }

    async fn update_with_snapshot(
        &mut self,
        resource: &Value,
        if_match: Option<&str>,
    ) -> Result<octofhir_storage::UpdatedResource, StorageError> {
        let mut guard = self.tx.lock().await;
        let tx = guard
            .as_deref_mut()
            .ok_or_else(|| StorageError::transaction_error("Transaction already completed"))?;
        let mut txid_guard = self.txid.lock().await;
        let txid = ensure_txid(&mut txid_guard, tx).await?;
        queries::crud::update_with_tx_snapshot(tx, resource, if_match, Some(txid)).await
    }

    async fn delete(&mut self, resource_type: &str, id: &str) -> Result<(), StorageError> {
        let mut tx_guard = self.tx.lock().await;
        let tx = tx_guard.as_deref_mut().ok_or_else(|| {
            StorageError::transaction_error(
                "Transaction already completed (committed or rolled back)",
            )
        })?;
        let mut txid_guard = self.txid.lock().await;
        let txid = ensure_txid(&mut txid_guard, tx).await?;
        drop(txid_guard);
        queries::crud::delete_with_tx(tx, resource_type, id, Some(txid)).await?;
        Ok(())
    }

    async fn read(
        &self,
        resource_type: &str,
        id: &str,
    ) -> Result<Option<StoredResource>, StorageError> {
        // Read operations see uncommitted changes within this transaction
        let mut tx_guard = self.tx.lock().await;
        let tx = tx_guard.as_deref_mut().ok_or_else(|| {
            StorageError::transaction_error(
                "Transaction already completed (committed or rolled back)",
            )
        })?;
        queries::crud::read_with_tx(tx, resource_type, id).await
    }

    async fn exists_many_grouped(
        &mut self,
        groups: &[(String, Vec<String>)],
    ) -> Result<std::collections::HashSet<String>, StorageError> {
        let mut tx_guard = self.tx.lock().await;
        let tx = tx_guard.as_deref_mut().ok_or_else(|| {
            StorageError::transaction_error(
                "Transaction already completed (committed or rolled back)",
            )
        })?;
        queries::crud::exists_many_grouped_with_tx(tx, groups).await
    }

    async fn read_for_update(
        &self,
        resource_type: &str,
        id: &str,
    ) -> Result<Option<StoredResource>, StorageError> {
        let mut guard = self.tx.lock().await;
        let tx = guard
            .as_deref_mut()
            .ok_or_else(|| StorageError::transaction_error("Transaction already completed"))?;
        queries::crud::read_with_tx_for_update(tx, resource_type, id).await
    }

    async fn read_many_for_update(
        &self,
        resource_type: &str,
        ids: &[String],
    ) -> Result<Vec<StoredResource>, StorageError> {
        let mut guard = self.tx.lock().await;
        let tx = guard
            .as_deref_mut()
            .ok_or_else(|| StorageError::transaction_error("Transaction already completed"))?;
        queries::crud::read_many_with_tx_for_update(tx, resource_type, ids).await
    }

    async fn lock_references(
        &mut self,
        groups: &[(String, Vec<String>)],
    ) -> Result<std::collections::HashSet<String>, StorageError> {
        let mut guard = self.tx.lock().await;
        let tx = guard
            .as_deref_mut()
            .ok_or_else(|| StorageError::transaction_error("Transaction already completed"))?;
        queries::crud::lock_references_with_tx(tx, groups).await
    }

    async fn search(
        &self,
        resource_type: &str,
        params: &SearchParams,
    ) -> Result<SearchResult, StorageError> {
        let mut tx_guard = self.tx.lock().await;
        let tx = tx_guard.as_deref_mut().ok_or_else(|| {
            StorageError::transaction_error(
                "Transaction already completed (committed or rolled back)",
            )
        })?;
        queries::search::execute_search_with_tx(
            tx,
            resource_type,
            params,
            self.search_registry.as_ref(),
        )
        .await
    }

    async fn search_system_raw(
        &self,
        resource_types: &[&str],
        params: &SearchParams,
        include_limits: IncludeLimits,
        strict: bool,
    ) -> Result<RawSearchResult, StorageError> {
        let registry = self
            .search_registry
            .as_ref()
            .ok_or_else(|| StorageError::internal("Transaction search registry unavailable"))?;
        let mut guard = self.tx.lock().await;
        let tx = guard
            .as_deref_mut()
            .ok_or_else(|| StorageError::transaction_error("Transaction already completed"))?;
        queries::search::execute_system_search_raw_with_tx(
            tx,
            resource_types,
            params,
            registry,
            include_limits,
            strict,
        )
        .await
    }

    async fn search_system_raw_authorized(
        &self,
        resource_types: &[&str],
        params: &SearchParams,
        include_limits: IncludeLimits,
        strict: bool,
        authorization: octofhir_storage::RawSearchAuthorization<'_>,
    ) -> Result<RawSearchResult, StorageError> {
        let registry = self
            .search_registry
            .as_ref()
            .ok_or_else(|| StorageError::internal("Transaction search registry unavailable"))?;
        let mut guard = self.tx.lock().await;
        let tx = guard
            .as_deref_mut()
            .ok_or_else(|| StorageError::transaction_error("Transaction already completed"))?;
        queries::search::execute_system_search_raw_authorized_with_tx(
            tx,
            resource_types,
            params,
            registry,
            queries::search::RawSearchOptions {
                include_limits,
                unknown_param_handling: Some(if strict {
                    octofhir_search::UnknownParamHandling::Strict
                } else {
                    octofhir_search::UnknownParamHandling::Lenient
                }),
                ..Default::default()
            },
            authorization,
        )
        .await
    }

    async fn search_conditional_batch(
        &self,
        predicates: &[(String, SearchParams)],
    ) -> Result<Vec<Vec<StoredResource>>, StorageError> {
        if predicates.is_empty() {
            return Ok(Vec::new());
        }
        let registry = self
            .search_registry
            .as_ref()
            .ok_or_else(|| StorageError::internal("Transaction search registry unavailable"))?;
        let mut guard = self.tx.lock().await;
        let tx = guard
            .as_deref_mut()
            .ok_or_else(|| StorageError::transaction_error("Transaction already completed"))?;
        queries::search::execute_conditional_search_batch_with_tx(tx, predicates, registry).await
    }

    async fn reserve_write_metadata(
        &mut self,
        planned: Option<&octofhir_storage::TransactionWriteMetadata>,
    ) -> Result<octofhir_storage::TransactionWriteMetadata, StorageError> {
        let mut guard = self.tx.lock().await;
        let tx = guard
            .as_deref_mut()
            .ok_or_else(|| StorageError::transaction_error("Transaction already completed"))?;
        let mut version = self.txid.lock().await;
        let metadata = if let Some(planned) = planned {
            let requested = planned
                .version_id
                .parse::<i64>()
                .ok()
                .filter(|v| *v > 0 && v.to_string() == planned.version_id)
                .ok_or_else(|| StorageError::invalid_resource("Invalid reserved version"))?;
            chrono::DateTime::parse_from_rfc3339(&planned.last_updated)
                .map_err(|_| StorageError::invalid_resource("Invalid reserved timestamp"))?;
            if version.is_some_and(|current| current != requested) {
                return Err(StorageError::transaction_error(
                    "Write metadata already allocated",
                ));
            }
            *version = Some(requested);
            planned.clone()
        } else {
            let id = ensure_txid(&mut version, tx).await?;
            let timestamp: String = query_scalar(
                "SELECT to_char(date_trunc('milliseconds', statement_timestamp()) AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')",
            ).fetch_one(&mut **tx).await
                .map_err(|e| StorageError::internal(format!("Metadata reservation failed: {e}")))?;
            octofhir_storage::TransactionWriteMetadata {
                version_id: id.to_string(),
                last_updated: timestamp,
            }
        };
        let _: String = query_scalar("SELECT set_config('octofhir.write_timestamp', $1, true), set_config('octofhir.write_version', $2, true)")
            .bind(&metadata.last_updated)
            .bind(&metadata.version_id)
            .fetch_one(&mut **tx)
            .await
            .map_err(|e| StorageError::internal(format!("Metadata reservation failed: {e}")))?;
        Ok(metadata)
    }

    async fn search_proposed_conditional_batch(
        &self,
        predicates: &[(String, SearchParams)],
        proposed: &[ProposedResource],
    ) -> Result<Vec<Vec<StoredResource>>, StorageError> {
        if predicates.is_empty() {
            return Ok(Vec::new());
        }
        let registry = self
            .search_registry
            .as_ref()
            .ok_or_else(|| StorageError::internal("Transaction search registry unavailable"))?;
        let mut guard = self.tx.lock().await;
        let tx = guard
            .as_deref_mut()
            .ok_or_else(|| StorageError::transaction_error("Transaction already completed"))?;
        queries::search::execute_proposed_conditional_search_batch_with_tx(
            tx, predicates, proposed, registry,
        )
        .await
    }

    async fn search_proposed_conditional_candidates(
        &self,
        predicates: &[(String, SearchParams)],
        proposed: &[ProposedResource],
        max_candidates: usize,
    ) -> Result<Vec<Vec<String>>, StorageError> {
        if predicates.is_empty() {
            return Ok(Vec::new());
        }
        let registry = self
            .search_registry
            .as_ref()
            .ok_or_else(|| StorageError::internal("Transaction search registry unavailable"))?;
        let mut guard = self.tx.lock().await;
        let tx = guard
            .as_deref_mut()
            .ok_or_else(|| StorageError::transaction_error("Transaction already completed"))?;
        queries::search::execute_proposed_conditional_candidates_with_tx(
            tx,
            predicates,
            proposed,
            registry,
            max_candidates,
        )
        .await
    }

    async fn create_batch(
        &mut self,
        resource_type: &str,
        resources: &[Value],
    ) -> Result<Vec<StoredResource>, StorageError> {
        if resources.is_empty() {
            return Ok(Vec::new());
        }
        let mut tx_guard = self.tx.lock().await;
        let tx = tx_guard.as_deref_mut().ok_or_else(|| {
            StorageError::transaction_error(
                "Transaction already completed (committed or rolled back)",
            )
        })?;
        let mut txid_guard = self.txid.lock().await;
        let txid = ensure_txid(&mut txid_guard, tx).await?;
        drop(txid_guard);

        let stored =
            queries::crud::create_batch_with_tx(tx, resource_type, txid, resources).await?;

        Ok(stored)
    }

    async fn update_batch(
        &mut self,
        resource_type: &str,
        resources: &[Value],
    ) -> Result<BatchUpdateOutcome, StorageError> {
        if resources.is_empty() {
            return Ok(BatchUpdateOutcome::default());
        }
        let mut tx_guard = self.tx.lock().await;
        let tx = tx_guard.as_deref_mut().ok_or_else(|| {
            StorageError::transaction_error(
                "Transaction already completed (committed or rolled back)",
            )
        })?;
        let mut txid_guard = self.txid.lock().await;
        let txid = ensure_txid(&mut txid_guard, tx).await?;
        drop(txid_guard);

        let (updated, previous, missing) =
            queries::crud::update_batch_with_tx(tx, resource_type, txid, resources).await?;

        Ok(BatchUpdateOutcome {
            updated,
            previous,
            missing,
        })
    }

    async fn delete_batch(
        &mut self,
        resource_type: &str,
        ids: &[String],
    ) -> Result<Vec<String>, StorageError> {
        Ok(self
            .delete_batch_with_snapshots(resource_type, ids)
            .await?
            .into_iter()
            .map(|deleted| deleted.previous.id)
            .collect())
    }

    async fn delete_batch_with_snapshots(
        &mut self,
        resource_type: &str,
        ids: &[String],
    ) -> Result<Vec<octofhir_storage::DeletedResource>, StorageError> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let mut tx_guard = self.tx.lock().await;
        let tx = tx_guard.as_deref_mut().ok_or_else(|| {
            StorageError::transaction_error(
                "Transaction already completed (committed or rolled back)",
            )
        })?;
        let mut version = self.txid.lock().await;
        ensure_txid(&mut version, tx).await?;
        queries::crud::delete_batch_snapshots_with_tx(tx, resource_type, ids).await
    }
}

impl Drop for PostgresTransaction {
    /// Automatically rolls back the transaction if it hasn't been explicitly
    /// committed or rolled back.
    ///
    /// This provides safety against accidentally leaving transactions open.
    /// sqlx's Transaction Drop implementation automatically issues a ROLLBACK.
    fn drop(&mut self) {
        match self.tx.try_lock() {
            Ok(guard) => {
                if guard.is_some() {
                    tracing::warn!(
                        "PostgresTransaction dropped without explicit commit/rollback - will auto-rollback"
                    );
                }
            }
            Err(_) => {
                tracing::warn!(
                    "PostgresTransaction dropped while lock was held - will auto-rollback"
                );
            }
        }
        // The inner SqlxTransaction's Drop impl will automatically rollback
    }
}

// Tests removed - will be added as integration tests with testcontainers
