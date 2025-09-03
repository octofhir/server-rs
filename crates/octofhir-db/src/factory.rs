use std::sync::Arc;

use async_trait::async_trait;

use crate::{
    query::{QueryFilter, QueryResult, SearchQuery},
    transaction::{Transaction, TransactionManager, TransactionStats},
    InMemoryStorage,
};
use octofhir_core::{ResourceEnvelope, ResourceType, Result};

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
    async fn get(&self, resource_type: &ResourceType, id: &str) -> Result<Option<ResourceEnvelope>>;
    async fn insert(&self, resource_type: &ResourceType, resource: ResourceEnvelope) -> Result<()>;
    async fn update(&self, resource_type: &ResourceType, id: &str, resource: ResourceEnvelope) -> Result<ResourceEnvelope>;
    async fn delete(&self, resource_type: &ResourceType, id: &str) -> Result<ResourceEnvelope>;
    async fn exists(&self, resource_type: &ResourceType, id: &str) -> bool;
    async fn count(&self) -> usize;
    async fn count_by_type(&self, resource_type: &ResourceType) -> usize;
    async fn search(&self, query: &SearchQuery) -> Result<QueryResult>;
    async fn search_by_type(&self, resource_type: &ResourceType, filters: Vec<QueryFilter>, offset: usize, count: usize) -> Result<QueryResult>;

    fn transaction_stats(&self) -> &TransactionStats {
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
    async fn get(&self, resource_type: &ResourceType, id: &str) -> Result<Option<ResourceEnvelope>> {
        InMemoryStorage::get(self, resource_type, id).await
    }

    async fn insert(&self, resource_type: &ResourceType, resource: ResourceEnvelope) -> Result<()> {
        InMemoryStorage::insert(self, resource_type, resource).await
    }

    async fn update(&self, resource_type: &ResourceType, id: &str, resource: ResourceEnvelope) -> Result<ResourceEnvelope> {
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

    async fn search_by_type(&self, resource_type: &ResourceType, filters: Vec<QueryFilter>, offset: usize, count: usize) -> Result<QueryResult> {
        InMemoryStorage::search_by_type(self, resource_type, filters, offset, count).await
    }
}
