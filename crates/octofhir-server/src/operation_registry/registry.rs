//! Operation Registry Service
//!
//! Coordinates operation providers and storage, syncs operations on startup.
//! Maintains an in-memory store of all operations as the source of truth,
//! with derived indexes for fast lookups.

use std::sync::Arc;

use dashmap::{DashMap, DashSet};
use octofhir_core::{OperationDefinition, OperationProvider};
use tracing::{debug, info, warn};

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
/// Maintains an in-memory map of operation public status for O(1) middleware lookups.
pub struct OperationRegistryService {
    storage: Arc<dyn OperationStorage>,
    providers: Vec<Arc<dyn OperationProvider>>,
    /// In-memory map: path_pattern -> is_public (source of truth for fast lookups)
    public_status: Arc<DashMap<String, bool>>,
    /// Derived index: exact public paths for O(1) lookup
    public_exact_paths: Arc<DashSet<String>>,
    /// Derived index: public path prefixes for prefix matching
    public_path_prefixes: Arc<DashSet<String>>,
}

impl OperationRegistryService {
    /// Create a new registry service
    pub fn new(storage: Arc<dyn OperationStorage>) -> Self {
        Self {
            storage,
            providers: Vec::new(),
            public_status: Arc::new(DashMap::new()),
            public_exact_paths: Arc::new(DashSet::new()),
            public_path_prefixes: Arc::new(DashSet::new()),
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
        Self {
            storage,
            providers,
            public_status: Arc::new(DashMap::new()),
            public_exact_paths: Arc::new(DashSet::new()),
            public_path_prefixes: Arc::new(DashSet::new()),
        }
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

    /// Sync all operations to the database and rebuild in-memory indexes.
    ///
    /// This should be called on startup. It will:
    /// 1. Collect operations from all providers
    /// 2. Upsert them to the database
    /// 3. Optionally clean up stale operations
    /// 4. Rebuild in-memory indexes for fast lookups
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

        // Rebuild in-memory indexes
        self.rebuild_indexes(&operations);

        info!(count, "Operations synced successfully");
        Ok(count)
    }

    /// Rebuild in-memory indexes from operations list.
    fn rebuild_indexes(&self, operations: &[OperationDefinition]) {
        // Clear existing indexes
        self.public_status.clear();
        self.public_exact_paths.clear();
        self.public_path_prefixes.clear();

        for op in operations {
            let path = &op.path_pattern;
            self.public_status.insert(path.clone(), op.public);

            if op.public {
                // Determine if this is a prefix pattern or exact path
                if path.ends_with('*') || path.contains('{') {
                    // Convert pattern to prefix
                    let prefix = path
                        .trim_end_matches('*')
                        .split('{')
                        .next()
                        .unwrap_or(path)
                        .to_string();
                    if !prefix.is_empty() {
                        self.public_path_prefixes.insert(prefix);
                    }
                } else {
                    self.public_exact_paths.insert(path.clone());
                }
            }
        }

        debug!(
            exact_paths = self.public_exact_paths.len(),
            path_prefixes = self.public_path_prefixes.len(),
            "Rebuilt public paths indexes"
        );
    }

    /// Check if a request path is public (O(1) lookup, no DB access).
    ///
    /// This is the primary method for middleware authentication bypass.
    #[inline]
    pub fn is_path_public(&self, path: &str) -> bool {
        // Check exact match first (O(1))
        if self.public_exact_paths.contains(path) {
            return true;
        }

        // Check prefix matches
        self.public_path_prefixes
            .iter()
            .any(|prefix| path.starts_with(prefix.key()))
    }

    /// Update an operation's public status (called from UI/API).
    ///
    /// Updates both the database and in-memory indexes.
    pub async fn set_operation_public(
        &self,
        id: &str,
        public: bool,
    ) -> Result<Option<OperationDefinition>, RegistryError> {
        use super::storage::OperationUpdate;

        // Update in database
        let updated = self
            .storage
            .update(id, OperationUpdate {
                public: Some(public),
                description: None,
            })
            .await?;

        // Update in-memory indexes
        if let Some(ref op) = updated {
            let path = &op.path_pattern;

            // Update public_status map
            self.public_status.insert(path.clone(), public);

            // Update derived indexes
            if public {
                if path.ends_with('*') || path.contains('{') {
                    let prefix = path
                        .trim_end_matches('*')
                        .split('{')
                        .next()
                        .unwrap_or(path)
                        .to_string();
                    if !prefix.is_empty() {
                        self.public_path_prefixes.insert(prefix);
                    }
                } else {
                    self.public_exact_paths.insert(path.clone());
                }
            } else {
                // Remove from public indexes
                if path.ends_with('*') || path.contains('{') {
                    let prefix = path
                        .trim_end_matches('*')
                        .split('{')
                        .next()
                        .unwrap_or(path)
                        .to_string();
                    self.public_path_prefixes.remove(&prefix);
                } else {
                    self.public_exact_paths.remove(path);
                }
            }
        }

        Ok(updated)
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
