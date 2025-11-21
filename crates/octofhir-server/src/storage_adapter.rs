//! Storage adapter for PostgreSQL backend.
//!
//! This module provides an adapter that wraps `PostgresStorage` and implements
//! the internal `Storage` trait used by handlers, bridging between the
//! `FhirStorage` trait (raw JSON) and the internal trait (ResourceEnvelope).

use async_trait::async_trait;
use octofhir_core::{CoreError, ResourceEnvelope, ResourceType, Result};
use octofhir_db_memory::{
    Storage,
    query::{QueryFilter, QueryResult, SearchQuery},
    transaction::{Transaction, TransactionManager, TransactionStats},
};
use octofhir_db_postgres::PostgresStorage;
use octofhir_storage::{FhirStorage, StorageError};
use serde_json::Value;
use std::sync::Arc;

/// Adapter that wraps `PostgresStorage` and implements the internal `Storage` trait.
pub struct PostgresStorageAdapter {
    inner: Arc<PostgresStorage>,
    stats: TransactionStats,
}

impl PostgresStorageAdapter {
    /// Creates a new adapter wrapping the given PostgreSQL storage.
    pub fn new(storage: PostgresStorage) -> Self {
        Self {
            inner: Arc::new(storage),
            stats: TransactionStats::default(),
        }
    }

    /// Creates a new adapter from an Arc-wrapped PostgreSQL storage.
    pub fn from_arc(storage: Arc<PostgresStorage>) -> Self {
        Self {
            inner: storage,
            stats: TransactionStats::default(),
        }
    }

    /// Convert StorageError to CoreError
    fn map_error(e: StorageError, resource_type: &str, id: &str) -> CoreError {
        match e {
            StorageError::NotFound { .. } => CoreError::resource_not_found(resource_type, id),
            StorageError::AlreadyExists { .. } => CoreError::resource_conflict(resource_type, id),
            StorageError::Deleted { .. } => CoreError::resource_deleted(resource_type, id),
            StorageError::VersionConflict { expected, actual } => CoreError::invalid_resource(
                format!("Version conflict: expected {expected}, got {actual}"),
            ),
            StorageError::InvalidResource { message } => CoreError::invalid_resource(message),
            _ => CoreError::invalid_resource(e.to_string()),
        }
    }

    /// Convert JSON Value to ResourceEnvelope
    fn value_to_envelope(value: &Value) -> Result<ResourceEnvelope> {
        serde_json::from_value(value.clone()).map_err(CoreError::from)
    }

    /// Convert ResourceEnvelope to JSON Value
    fn envelope_to_value(envelope: &ResourceEnvelope) -> Result<Value> {
        serde_json::to_value(envelope).map_err(CoreError::from)
    }
}

impl std::fmt::Debug for PostgresStorageAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PostgresStorageAdapter")
            .field("stats", &self.stats)
            .finish()
    }
}

#[async_trait]
impl Storage for PostgresStorageAdapter {
    async fn get(
        &self,
        resource_type: &ResourceType,
        id: &str,
    ) -> Result<Option<ResourceEnvelope>> {
        let rt_str = resource_type.to_string();
        match self.inner.read(&rt_str, id).await {
            Ok(Some(stored)) => {
                let envelope = Self::value_to_envelope(&stored.resource)?;
                Ok(Some(envelope))
            }
            Ok(None) => Ok(None),
            Err(StorageError::Deleted { .. }) => Err(CoreError::resource_deleted(&rt_str, id)),
            Err(e) => Err(Self::map_error(e, &rt_str, id)),
        }
    }

    async fn insert(&self, resource_type: &ResourceType, resource: ResourceEnvelope) -> Result<()> {
        let rt_str = resource_type.to_string();
        let value = Self::envelope_to_value(&resource)?;
        match self.inner.create(&value).await {
            Ok(_) => Ok(()),
            Err(e) => Err(Self::map_error(e, &rt_str, &resource.id)),
        }
    }

    async fn update(
        &self,
        resource_type: &ResourceType,
        id: &str,
        resource: ResourceEnvelope,
    ) -> Result<ResourceEnvelope> {
        let rt_str = resource_type.to_string();
        let value = Self::envelope_to_value(&resource)?;
        match self.inner.update(&value, None).await {
            Ok(stored) => Self::value_to_envelope(&stored.resource),
            Err(e) => Err(Self::map_error(e, &rt_str, id)),
        }
    }

    async fn delete(&self, resource_type: &ResourceType, id: &str) -> Result<ResourceEnvelope> {
        let rt_str = resource_type.to_string();

        // First get the current resource to return it
        let current = match self.inner.read(&rt_str, id).await {
            Ok(Some(stored)) => Self::value_to_envelope(&stored.resource)?,
            Ok(None) => {
                // Per FHIR spec: delete of non-existent is idempotent
                return Ok(ResourceEnvelope::new(id.to_string(), resource_type.clone()));
            }
            Err(StorageError::Deleted { .. }) => {
                // Already deleted - return empty envelope
                return Ok(ResourceEnvelope::new(id.to_string(), resource_type.clone()));
            }
            Err(e) => return Err(Self::map_error(e, &rt_str, id)),
        };

        // Now delete it
        match self.inner.delete(&rt_str, id).await {
            Ok(()) => Ok(current),
            Err(StorageError::NotFound { .. }) => {
                // Idempotent delete
                Ok(ResourceEnvelope::new(id.to_string(), resource_type.clone()))
            }
            Err(e) => Err(Self::map_error(e, &rt_str, id)),
        }
    }

    async fn exists(&self, resource_type: &ResourceType, id: &str) -> bool {
        let rt_str = resource_type.to_string();
        matches!(self.inner.read(&rt_str, id).await, Ok(Some(_)))
    }

    async fn count(&self) -> usize {
        // PostgreSQL search is not yet implemented, return 0
        0
    }

    async fn count_by_type(&self, _resource_type: &ResourceType) -> usize {
        // PostgreSQL search is not yet implemented, return 0
        0
    }

    async fn search(&self, _query: &SearchQuery) -> Result<QueryResult> {
        // PostgreSQL search is not yet implemented
        Ok(QueryResult::empty())
    }

    async fn search_by_type(
        &self,
        _resource_type: &ResourceType,
        _filters: Vec<QueryFilter>,
        _offset: usize,
        _count: usize,
    ) -> Result<QueryResult> {
        // PostgreSQL search is not yet implemented
        Ok(QueryResult::empty())
    }
}

#[async_trait]
impl TransactionManager for PostgresStorageAdapter {
    async fn begin_transaction(&mut self) -> Result<Transaction> {
        Ok(Transaction::new())
    }

    async fn execute_transaction(&mut self, _transaction: &mut Transaction) -> Result<()> {
        // PostgreSQL transactions not yet fully implemented
        Ok(())
    }

    async fn commit_transaction(&mut self, _transaction: &mut Transaction) -> Result<()> {
        // PostgreSQL transactions not yet fully implemented
        Ok(())
    }

    async fn rollback_transaction(&mut self, _transaction: &mut Transaction) -> Result<()> {
        // PostgreSQL transactions not yet fully implemented
        Ok(())
    }

    async fn abort_transaction(&mut self, _transaction: &mut Transaction) -> Result<()> {
        // PostgreSQL transactions not yet fully implemented
        Ok(())
    }

    fn get_transaction_stats(&self) -> TransactionStats {
        self.stats.clone()
    }
}
