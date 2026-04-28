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
use octofhir_storage::{SearchParams, SearchResult, StorageError, StoredResource, Transaction};

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
    let t: i64 =
        query_scalar("INSERT INTO _transaction (status) VALUES ('committed') RETURNING txid")
            .fetch_one(&mut **tx)
            .await
            .map_err(|e| StorageError::internal(format!("Failed to allocate txid: {e}")))?;
    *txid_slot = Some(t);
    Ok(t)
}

#[async_trait]
impl Transaction for PostgresTransaction {
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
        if let Some(registry) = &self.search_registry {
            let (refs, dates) = crate::search_index::extract_search_index_rows(
                registry,
                &result.resource_type,
                &result.resource,
            );
            crate::search_index::write_reference_index_with_tx(
                tx,
                &result.resource_type,
                &result.id,
                &refs,
            )
            .await?;
            crate::search_index::write_date_index_with_tx(
                tx,
                &result.resource_type,
                &result.id,
                &dates,
            )
            .await?;
        }
        Ok(result)
    }

    async fn update(
        &mut self,
        resource: &Value,
        if_match: Option<&str>,
    ) -> Result<StoredResource, StorageError> {
        let mut tx_guard = self.tx.lock().await;
        let tx = tx_guard.as_deref_mut().ok_or_else(|| {
            StorageError::transaction_error(
                "Transaction already completed (committed or rolled back)",
            )
        })?;
        let mut txid_guard = self.txid.lock().await;
        let txid = ensure_txid(&mut txid_guard, tx).await?;
        drop(txid_guard);
        let result =
            queries::crud::update_with_tx_if_match(tx, resource, if_match, Some(txid)).await?;
        if let Some(registry) = &self.search_registry {
            let (refs, dates) = crate::search_index::extract_search_index_rows(
                registry,
                &result.resource_type,
                &result.resource,
            );
            crate::search_index::write_reference_index_with_tx(
                tx,
                &result.resource_type,
                &result.id,
                &refs,
            )
            .await?;
            crate::search_index::write_date_index_with_tx(
                tx,
                &result.resource_type,
                &result.id,
                &dates,
            )
            .await?;
        }
        Ok(result)
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
        crate::search_index::delete_search_indexes_with_tx(tx, resource_type, id).await
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

        if let Some(registry) = &self.search_registry {
            let mut buffer = crate::search_index::BatchIndexBuffer::new();
            for s in &stored {
                let (refs, dates) = crate::search_index::extract_search_index_rows(
                    registry,
                    &s.resource_type,
                    &s.resource,
                );
                buffer.extend_with(&s.resource_type, &s.id, &refs, &dates);
            }
            if !buffer.is_empty() {
                buffer.flush_with_tx(tx).await?;
            }
        }

        Ok(stored)
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
