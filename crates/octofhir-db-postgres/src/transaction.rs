//! PostgreSQL transaction implementation with ACID guarantees.
//!
//! This module provides native PostgreSQL transaction support for FHIR operations,
//! ensuring atomicity, consistency, isolation, and durability for multi-resource
//! operations like Bundle transactions.

use async_trait::async_trait;
use serde_json::Value;
use sqlx_postgres::PgTransaction;
use tokio::sync::Mutex;

use octofhir_storage::{StorageError, StoredResource, Transaction};

use crate::queries;
use crate::schema::SchemaManager;

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
    /// Schema manager for table management and SQL generation.
    schema_manager: SchemaManager,
}

impl PostgresTransaction {
    /// Creates a new PostgreSQL transaction.
    ///
    /// # Arguments
    ///
    /// * `tx` - The sqlx transaction to wrap
    /// * `schema_manager` - Schema manager for table operations
    pub fn new(tx: PgTransaction<'static>, schema_manager: SchemaManager) -> Self {
        Self {
            tx: Mutex::new(Some(Box::new(tx))),
            schema_manager,
        }
    }
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
        let schema = self.schema_manager.clone();
        let mut tx_guard = self.tx.lock().await;
        let tx = tx_guard.as_deref_mut().ok_or_else(|| {
            StorageError::transaction_error("Transaction already completed (committed or rolled back)")
        })?;
        queries::crud::create_with_tx(tx, &schema, resource).await
    }

    async fn update(&mut self, resource: &Value) -> Result<StoredResource, StorageError> {
        let schema = self.schema_manager.clone();
        let mut tx_guard = self.tx.lock().await;
        let tx = tx_guard.as_deref_mut().ok_or_else(|| {
            StorageError::transaction_error("Transaction already completed (committed or rolled back)")
        })?;
        queries::crud::update_with_tx(tx, &schema, resource).await
    }

    async fn delete(&mut self, resource_type: &str, id: &str) -> Result<(), StorageError> {
        let schema = self.schema_manager.clone();
        let mut tx_guard = self.tx.lock().await;
        let tx = tx_guard.as_deref_mut().ok_or_else(|| {
            StorageError::transaction_error("Transaction already completed (committed or rolled back)")
        })?;
        queries::crud::delete_with_tx(tx, &schema, resource_type, id).await
    }

    async fn read(
        &self,
        resource_type: &str,
        id: &str,
    ) -> Result<Option<StoredResource>, StorageError> {
        // Read operations see uncommitted changes within this transaction
        let mut tx_guard = self.tx.lock().await;
        let tx = tx_guard.as_deref_mut().ok_or_else(|| {
            StorageError::transaction_error("Transaction already completed (committed or rolled back)")
        })?;
        queries::crud::read_with_tx(tx, &self.schema_manager, resource_type, id).await
    }
}

impl Drop for PostgresTransaction {
    /// Automatically rolls back the transaction if it hasn't been explicitly
    /// committed or rolled back.
    ///
    /// This provides safety against accidentally leaving transactions open.
    /// sqlx's Transaction Drop implementation automatically issues a ROLLBACK.
    fn drop(&mut self) {
        if self.tx.blocking_lock().is_some() {
            tracing::warn!(
                "PostgresTransaction dropped without explicit commit/rollback - will auto-rollback"
            );
            // The inner SqlxTransaction's Drop impl will automatically rollback
        }
    }
}

// Tests removed - will be added as integration tests with testcontainers
