use std::sync::Arc;

use async_trait::async_trait;

use crate::{
    InMemoryStorage,
    query::{QueryFilter, QueryResult, SearchQuery},
    transaction::{TransactionManager, TransactionStats},
};
use octofhir_core::{ResourceEnvelope, ResourceType, Result};
use octofhir_storage::{HistoryParams, HistoryResult};

/// Supported storage backend types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageBackend {
    /// In-memory storage implemented on top of papaya::HashMap
    InMemoryPapaya,
}

/// Storage-specific configuration options.
///
/// These are best-effort for the in-memory backend. Some options may be
/// no-ops until a backend that supports them is implemented.
#[derive(Debug, Clone, Default)]
pub struct StorageOptions {
    /// Soft memory limit hint in bytes (not enforced by in-memory backend).
    pub memory_limit_bytes: Option<usize>,
    /// Optional preallocation hint (e.g., initial capacity). Not used by papaya.
    pub preallocate_items: Option<usize>,
}

/// Factory configuration to construct a storage instance.
#[derive(Debug, Clone)]
pub struct StorageConfig {
    pub backend: StorageBackend,
    pub options: StorageOptions,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            backend: StorageBackend::InMemoryPapaya,
            options: StorageOptions::default(),
        }
    }
}

/// Storage trait representing common CRUD, query, and transaction capabilities.
#[async_trait]
pub trait Storage: Send + Sync + TransactionManager {
    async fn get(&self, resource_type: &ResourceType, id: &str)
    -> Result<Option<ResourceEnvelope>>;
    async fn insert(&self, resource_type: &ResourceType, resource: ResourceEnvelope) -> Result<()>;
    async fn update(
        &self,
        resource_type: &ResourceType,
        id: &str,
        resource: ResourceEnvelope,
    ) -> Result<ResourceEnvelope>;
    async fn delete(&self, resource_type: &ResourceType, id: &str) -> Result<ResourceEnvelope>;
    async fn exists(&self, resource_type: &ResourceType, id: &str) -> bool;
    async fn count(&self) -> usize;
    async fn count_by_type(&self, resource_type: &ResourceType) -> usize;
    async fn search(&self, query: &SearchQuery) -> Result<QueryResult>;
    async fn search_by_type(
        &self,
        resource_type: &ResourceType,
        filters: Vec<QueryFilter>,
        offset: usize,
        count: usize,
    ) -> Result<QueryResult>;

    /// Get history for a specific resource instance or all resources of a type.
    /// If `id` is Some, returns history for the specific resource.
    /// If `id` is None, returns history for all resources of that type.
    async fn history(
        &self,
        resource_type: &str,
        id: Option<&str>,
        params: &HistoryParams,
    ) -> Result<HistoryResult>;

    fn transaction_stats(&self) -> TransactionStats {
        self.get_transaction_stats()
    }
}

/// Type alias for a shareable storage instance
pub type DynStorage = Arc<dyn Storage>;

/// Create a storage instance based on the provided configuration.
///
/// For now, only the in-memory papaya backend is supported.
pub fn create_storage(config: &StorageConfig) -> DynStorage {
    match config.backend {
        StorageBackend::InMemoryPapaya => {
            let storage = InMemoryStorage::with_options(config.options.clone());
            // Note: options are currently hints and have no effect for papaya backend
            Arc::new(storage)
        }
    }
}

// Implement Storage trait for InMemoryStorage by delegating to its methods.
#[async_trait]
impl Storage for InMemoryStorage {
    async fn get(
        &self,
        resource_type: &ResourceType,
        id: &str,
    ) -> Result<Option<ResourceEnvelope>> {
        InMemoryStorage::get(self, resource_type, id).await
    }

    async fn insert(&self, resource_type: &ResourceType, resource: ResourceEnvelope) -> Result<()> {
        InMemoryStorage::insert(self, resource_type, resource).await
    }

    async fn update(
        &self,
        resource_type: &ResourceType,
        id: &str,
        resource: ResourceEnvelope,
    ) -> Result<ResourceEnvelope> {
        InMemoryStorage::update(self, resource_type, id, resource).await
    }

    async fn delete(&self, resource_type: &ResourceType, id: &str) -> Result<ResourceEnvelope> {
        InMemoryStorage::delete(self, resource_type, id).await
    }

    async fn exists(&self, resource_type: &ResourceType, id: &str) -> bool {
        InMemoryStorage::exists(self, resource_type, id).await
    }

    async fn count(&self) -> usize {
        InMemoryStorage::count(self).await
    }

    async fn count_by_type(&self, resource_type: &ResourceType) -> usize {
        InMemoryStorage::count_by_type(self, resource_type).await
    }

    async fn search(&self, query: &SearchQuery) -> Result<QueryResult> {
        InMemoryStorage::search(self, query).await
    }

    async fn search_by_type(
        &self,
        resource_type: &ResourceType,
        filters: Vec<QueryFilter>,
        offset: usize,
        count: usize,
    ) -> Result<QueryResult> {
        InMemoryStorage::search_by_type(self, resource_type, filters, offset, count).await
    }

    async fn history(
        &self,
        resource_type: &str,
        id: Option<&str>,
        params: &HistoryParams,
    ) -> Result<HistoryResult> {
        let mut entries = match id {
            Some(id) => self.get_history(resource_type, id).await,
            None => self.get_type_history(resource_type).await,
        };

        // Sort by last_updated descending (newest first)
        entries.sort_by(|a, b| b.resource.last_updated.cmp(&a.resource.last_updated));

        // Apply _since filter
        if let Some(since) = params.since {
            entries.retain(|e| e.resource.last_updated > since);
        }

        // Apply _at filter
        if let Some(at) = params.at {
            entries.retain(|e| e.resource.last_updated <= at);
        }

        let total = entries.len() as u32;

        // Apply pagination
        let offset = params.offset.unwrap_or(0) as usize;
        let count = params.count.unwrap_or(100) as usize;

        let paginated: Vec<_> = entries.into_iter().skip(offset).take(count).collect();

        Ok(HistoryResult {
            entries: paginated,
            total: Some(total),
        })
    }
}
