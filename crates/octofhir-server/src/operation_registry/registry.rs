//! Operation Registry Service
//!
//! Coordinates operation providers and storage, syncs operations on startup.

use std::sync::Arc;

use octofhir_core::{OperationDefinition, OperationProvider};
use tracing::{info, warn};

use super::storage::{OperationStorage, OperationStorageError};

/// Error type for registry operations
#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    #[error("Storage error: {0}")]
    Storage(#[from] OperationStorageError),
}

/// Service for managing the operation registry
///
/// Collects operations from all registered providers and syncs them to the database.
pub struct OperationRegistryService {
    storage: Arc<dyn OperationStorage>,
    providers: Vec<Arc<dyn OperationProvider>>,
}

impl OperationRegistryService {
    /// Create a new registry service
    pub fn new(storage: Arc<dyn OperationStorage>) -> Self {
        Self {
            storage,
            providers: Vec::new(),
        }
    }

    /// Add an operation provider
    pub fn add_provider(&mut self, provider: Arc<dyn OperationProvider>) {
        self.providers.push(provider);
    }

    /// Create a registry with providers
    pub fn with_providers(
        storage: Arc<dyn OperationStorage>,
        providers: Vec<Arc<dyn OperationProvider>>,
    ) -> Self {
        Self { storage, providers }
    }

    /// Collect all operations from providers
    fn collect_operations(&self) -> Vec<OperationDefinition> {
        let mut all_ops = Vec::new();
        for provider in &self.providers {
            let ops = provider.get_operations();
            info!(
                module = %provider.module_id(),
                count = ops.len(),
                "Collected operations from provider"
            );
            all_ops.extend(ops);
        }
        all_ops
    }

    /// Sync all operations to the database
    ///
    /// This should be called on startup. It will:
    /// 1. Collect operations from all providers
    /// 2. Upsert them to the database
    /// 3. Optionally clean up stale operations
    pub async fn sync_operations(&self, cleanup_stale: bool) -> Result<usize, RegistryError> {
        let operations = self.collect_operations();
        let count = operations.len();

        info!(count, "Syncing operations to database");

        // Upsert all operations
        self.storage.upsert_all(&operations).await?;

        // Optionally clean up operations that are no longer provided
        if cleanup_stale {
            let ids: Vec<String> = operations.iter().map(|op| op.id.clone()).collect();
            let deleted = self.storage.delete_not_in(&ids).await?;
            if deleted > 0 {
                warn!(deleted, "Removed stale operations from database");
            }
        }

        info!(count, "Operations synced successfully");
        Ok(count)
    }

    /// Get all operations from storage
    pub async fn list_all(&self) -> Result<Vec<OperationDefinition>, RegistryError> {
        Ok(self.storage.list_all().await?)
    }

    /// Get operations by category
    pub async fn list_by_category(
        &self,
        category: &str,
    ) -> Result<Vec<OperationDefinition>, RegistryError> {
        Ok(self.storage.list_by_category(category).await?)
    }

    /// Get operations by module
    pub async fn list_by_module(
        &self,
        module: &str,
    ) -> Result<Vec<OperationDefinition>, RegistryError> {
        Ok(self.storage.list_by_module(module).await?)
    }

    /// Get public operations
    pub async fn list_public(&self) -> Result<Vec<OperationDefinition>, RegistryError> {
        Ok(self.storage.list_public().await?)
    }

    /// Get a single operation by ID
    pub async fn get(&self, id: &str) -> Result<Option<OperationDefinition>, RegistryError> {
        Ok(self.storage.get(id).await?)
    }

    /// Check if an operation is public
    pub async fn is_public(&self, id: &str) -> Result<bool, RegistryError> {
        Ok(self.storage.is_public(id).await?)
    }

    /// Get the number of registered providers
    pub fn provider_count(&self) -> usize {
        self.providers.len()
    }
}
