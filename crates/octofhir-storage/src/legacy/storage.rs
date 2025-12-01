//! Legacy Storage trait for backward compatibility with ResourceEnvelope-based handlers.

use std::sync::Arc;

use async_trait::async_trait;

use super::query::{QueryFilter, QueryResult, SearchQuery};
use super::transaction::{TransactionManager, TransactionStats};
use crate::types::{HistoryParams, HistoryResult};
use octofhir_core::{ResourceEnvelope, ResourceType, Result};

/// Storage trait representing common CRUD, query, and transaction capabilities.
///
/// This is a legacy trait maintained for backward compatibility with handlers that
/// use `ResourceEnvelope`-based operations. New code should prefer `FhirStorage`.
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
