use crate::factory::StorageOptions;
use crate::query::{QueryFilter, QueryResult, SearchQuery};
use crate::transaction::{
    Transaction, TransactionManager, TransactionOperation, TransactionOperationResult,
    TransactionStats,
};
use octofhir_core::{CoreError, ResourceEnvelope, ResourceType, Result};
use octofhir_storage::{HistoryEntry, HistoryMethod, StoredResource};
use papaya::HashMap as PapayaHashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use time::OffsetDateTime;
use tokio::sync::RwLock;

pub type StorageKey = String; // Format: "ResourceType/id"

pub(crate) fn make_storage_key(resource_type: &ResourceType, id: &str) -> StorageKey {
    format!("{resource_type}/{id}")
}

pub(crate) fn make_storage_key_str(resource_type: &str, id: &str) -> StorageKey {
    format!("{resource_type}/{id}")
}

/// In-memory FHIR storage backend using papaya lock-free HashMap.
///
/// This storage implementation provides:
/// - Lock-free concurrent access via papaya::HashMap
/// - Full CRUD operations
/// - History tracking for resource versions
/// - Transaction support with rollback
/// - Search functionality with filtering and pagination
#[derive(Debug)]
pub struct InMemoryStorage {
    /// Main storage using papaya for lock-free concurrent access
    pub(crate) data: Arc<PapayaHashMap<StorageKey, ResourceEnvelope>>,
    /// History storage: key -> list of historical versions
    pub(crate) history: Arc<RwLock<std::collections::HashMap<StorageKey, Vec<HistoryEntry>>>>,
    /// Atomic counter for generating version IDs
    pub(crate) version_counter: AtomicU64,
    /// Transaction statistics
    transaction_stats: Arc<RwLock<TransactionStats>>,
    /// Storage configuration options (soft hints for in-memory backend)
    _options: StorageOptions,
}

impl InMemoryStorage {
    /// Creates a new in-memory storage with default options.
    pub fn new() -> Self {
        Self {
            data: Arc::new(PapayaHashMap::new()),
            history: Arc::new(RwLock::new(std::collections::HashMap::new())),
            version_counter: AtomicU64::new(1),
            transaction_stats: Arc::new(RwLock::new(TransactionStats::new())),
            _options: StorageOptions::default(),
        }
    }

    /// Creates a new in-memory storage with the given options.
    pub fn with_options(options: StorageOptions) -> Self {
        Self {
            data: Arc::new(PapayaHashMap::new()),
            history: Arc::new(RwLock::new(std::collections::HashMap::new())),
            version_counter: AtomicU64::new(1),
            transaction_stats: Arc::new(RwLock::new(TransactionStats::new())),
            _options: options,
        }
    }

    /// Generates the next version ID.
    pub(crate) fn next_version(&self) -> String {
        self.version_counter
            .fetch_add(1, Ordering::SeqCst)
            .to_string()
    }

    /// Adds a history entry for a resource.
    pub(crate) async fn add_history(&self, stored: &StoredResource, method: HistoryMethod) {
        let key = make_storage_key_str(&stored.resource_type, &stored.id);
        let entry = HistoryEntry::new(stored.clone(), method);
        let mut history_guard = self.history.write().await;
        history_guard.entry(key).or_default().push(entry);
    }

    /// Converts a ResourceEnvelope to a StoredResource.
    #[allow(dead_code)]
    pub(crate) fn envelope_to_stored(
        &self,
        env: &ResourceEnvelope,
        version_id: &str,
    ) -> StoredResource {
        let now = OffsetDateTime::now_utc();
        StoredResource {
            id: env.id.clone(),
            version_id: version_id.to_string(),
            resource_type: env.resource_type.to_string(),
            resource: serde_json::to_value(env).unwrap_or_default(),
            last_updated: now,
            created_at: now,
        }
    }

    /// Gets history entries for a resource.
    pub(crate) async fn get_history(&self, resource_type: &str, id: &str) -> Vec<HistoryEntry> {
        let key = make_storage_key_str(resource_type, id);
        let history_guard = self.history.read().await;
        history_guard.get(&key).cloned().unwrap_or_default()
    }

    /// Gets all history entries for a resource type.
    pub(crate) async fn get_type_history(&self, resource_type: &str) -> Vec<HistoryEntry> {
        let prefix = format!("{resource_type}/");
        let history_guard = self.history.read().await;
        history_guard
            .iter()
            .filter(|(k, _)| k.starts_with(&prefix))
            .flat_map(|(_, entries)| entries.clone())
            .collect()
    }

    pub async fn get(
        &self,
        resource_type: &ResourceType,
        id: &str,
    ) -> Result<Option<ResourceEnvelope>> {
        let key = make_storage_key(resource_type, id);
        let guard = self.data.pin();
        match guard.get(&key) {
            Some(env) => {
                if env.is_deleted() {
                    Err(CoreError::resource_deleted(
                        resource_type.to_string(),
                        id.to_string(),
                    ))
                } else {
                    Ok(Some(env.clone()))
                }
            }
            None => Ok(None),
        }
    }

    /// Internal method to get a resource regardless of deleted status.
    /// Used for transaction rollback purposes.
    async fn get_raw(&self, resource_type: &ResourceType, id: &str) -> Option<ResourceEnvelope> {
        let key = make_storage_key(resource_type, id);
        let guard = self.data.pin();
        guard.get(&key).cloned()
    }

    /// Internal method to restore a deleted resource or force insert.
    /// Used for transaction rollback purposes.
    async fn force_insert(&self, resource_type: &ResourceType, resource: ResourceEnvelope) {
        let key = make_storage_key(resource_type, &resource.id);
        let guard = self.data.pin();
        guard.insert(key, resource);
    }

    /// Internal method to hard delete a resource (actually remove it).
    /// Used for transaction rollback of create operations.
    async fn hard_delete(&self, resource_type: &ResourceType, id: &str) {
        let key = make_storage_key(resource_type, id);
        let guard = self.data.pin();
        guard.remove(&key);
    }

    pub async fn insert(
        &self,
        resource_type: &ResourceType,
        resource: ResourceEnvelope,
    ) -> Result<()> {
        let key = make_storage_key(resource_type, &resource.id);
        let guard = self.data.pin();

        // Check for conflicts
        if guard.get(&key).is_some() {
            return Err(CoreError::resource_conflict(
                resource_type.to_string(),
                resource.id,
            ));
        }

        guard.insert(key, resource);
        Ok(())
    }

    pub async fn update(
        &self,
        resource_type: &ResourceType,
        id: &str,
        resource: ResourceEnvelope,
    ) -> Result<ResourceEnvelope> {
        let key = make_storage_key(resource_type, id);
        let guard = self.data.pin();

        // Check if resource exists
        let old_resource = guard
            .get(&key)
            .ok_or_else(|| {
                CoreError::resource_not_found(resource_type.to_string(), id.to_string())
            })?
            .clone();

        guard.insert(key, resource);
        Ok(old_resource)
    }

    pub async fn delete(&self, resource_type: &ResourceType, id: &str) -> Result<ResourceEnvelope> {
        let key = make_storage_key(resource_type, id);
        let guard = self.data.pin();

        // Check if resource exists
        let existing = match guard.get(&key) {
            Some(env) => env.clone(),
            None => {
                // Per FHIR spec: delete of non-existent resource is idempotent
                // Return a placeholder envelope to indicate success
                return Ok(ResourceEnvelope::new(id.to_string(), resource_type.clone()));
            }
        };

        // Check if already deleted - idempotent success
        if existing.is_deleted() {
            return Ok(existing);
        }

        // Soft delete: mark as deleted instead of removing
        let mut deleted_env = existing.clone();
        deleted_env.mark_deleted();

        guard.insert(key, deleted_env);

        Ok(existing)
    }

    pub async fn exists(&self, resource_type: &ResourceType, id: &str) -> bool {
        let key = make_storage_key(resource_type, id);
        let guard = self.data.pin();
        match guard.get(&key) {
            Some(env) => !env.is_deleted(),
            None => false,
        }
    }

    pub async fn count(&self) -> usize {
        let guard = self.data.pin();
        guard.iter().filter(|(_, env)| !env.is_deleted()).count()
    }

    pub async fn count_by_type(&self, resource_type: &ResourceType) -> usize {
        let prefix = format!("{resource_type}/");
        let guard = self.data.pin();
        guard
            .iter()
            .filter(|(key, env)| key.starts_with(&prefix) && !env.is_deleted())
            .count()
    }

    /// Search for resources matching the given query
    pub async fn search(&self, query: &SearchQuery) -> Result<QueryResult> {
        let prefix = format!("{}/", query.resource_type);
        let guard = self.data.pin();

        // Collect all matching resources (excluding soft-deleted ones)
        let mut matching_resources: Vec<ResourceEnvelope> = guard
            .iter()
            .filter(|(key, resource)| {
                key.starts_with(&prefix) && !resource.is_deleted() && query.matches(resource)
            })
            .map(|(_, resource)| resource.clone())
            .collect();

        // Sort results if requested
        if let Some(sort_field) = &query.sort_field {
            self.sort_resources(&mut matching_resources, sort_field, query.sort_ascending);
        }

        let total = matching_resources.len();

        // Apply pagination
        let _end_idx = std::cmp::min(query.offset + query.count, total);
        let page_resources = if query.offset < total {
            matching_resources
                .into_iter()
                .skip(query.offset)
                .take(query.count)
                .collect()
        } else {
            Vec::new()
        };

        Ok(QueryResult::new(
            total,
            page_resources,
            query.offset,
            query.count,
        ))
    }

    /// Search for resources by type with simple filters
    pub async fn search_by_type(
        &self,
        resource_type: &ResourceType,
        filters: Vec<QueryFilter>,
        offset: usize,
        count: usize,
    ) -> Result<QueryResult> {
        let query = SearchQuery::new(resource_type.clone()).with_pagination(offset, count);

        let query = filters
            .into_iter()
            .fold(query, |q, filter| q.with_filter(filter));

        self.search(&query).await
    }

    fn sort_resources(
        &self,
        resources: &mut [ResourceEnvelope],
        sort_field: &str,
        ascending: bool,
    ) {
        resources.sort_by(|a, b| {
            let comparison = match sort_field {
                "_id" => a.id.cmp(&b.id),
                "_lastUpdated" => a.meta.last_updated.cmp(&b.meta.last_updated),
                "resourceType" => a
                    .resource_type
                    .to_string()
                    .cmp(&b.resource_type.to_string()),
                _ => {
                    // Compare field values from resource data
                    let a_val = a
                        .get_field(sort_field)
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let b_val = b
                        .get_field(sort_field)
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    a_val.cmp(b_val)
                }
            };

            if ascending {
                comparison
            } else {
                comparison.reverse()
            }
        });
    }

    async fn execute_operation(
        &self,
        operation: &TransactionOperation,
    ) -> Result<Option<ResourceEnvelope>> {
        match operation {
            TransactionOperation::Create {
                resource_type,
                resource,
            } => {
                self.insert(resource_type, resource.clone()).await?;
                Ok(Some(resource.clone()))
            }
            TransactionOperation::Update {
                resource_type,
                id,
                resource,
            } => {
                let old_resource = self.update(resource_type, id, resource.clone()).await?;
                Ok(Some(old_resource))
            }
            TransactionOperation::Delete { resource_type, id } => {
                let deleted_resource = self.delete(resource_type, id).await?;
                Ok(Some(deleted_resource))
            }
            TransactionOperation::Read { resource_type, id } => self.get(resource_type, id).await,
        }
    }

    async fn capture_rollback_snapshot(
        &self,
        operation: &TransactionOperation,
    ) -> Result<Option<ResourceEnvelope>> {
        match operation {
            TransactionOperation::Create { .. } => {
                // For create operations, rollback means delete, so no snapshot needed
                Ok(None)
            }
            TransactionOperation::Update {
                resource_type, id, ..
            } => {
                // For update, we need the current state to rollback to
                self.get(resource_type, id).await
            }
            TransactionOperation::Delete { resource_type, id } => {
                // For delete, use get_raw to capture even if already deleted
                Ok(self.get_raw(resource_type, id).await)
            }
            TransactionOperation::Read { .. } => {
                // Read operations don't need rollback snapshots
                Ok(None)
            }
        }
    }

    async fn rollback_operation(
        &self,
        operation: &TransactionOperation,
        snapshot: &Option<ResourceEnvelope>,
    ) -> Result<()> {
        match (operation, snapshot) {
            (
                TransactionOperation::Create {
                    resource_type,
                    resource,
                },
                None,
            ) => {
                // Rollback create by hard deleting the created resource
                self.hard_delete(resource_type, &resource.id).await;
            }
            (
                TransactionOperation::Update {
                    resource_type,
                    id: _,
                    ..
                },
                Some(original_resource),
            ) => {
                // Rollback update by force inserting the original resource
                self.force_insert(resource_type, original_resource.clone())
                    .await;
            }
            (
                TransactionOperation::Update {
                    resource_type, id, ..
                },
                None,
            ) => {
                // If there was no original resource, hard delete the updated one
                self.hard_delete(resource_type, id).await;
            }
            (TransactionOperation::Delete { resource_type, .. }, Some(deleted_resource)) => {
                // Rollback delete by force inserting the original resource
                self.force_insert(resource_type, deleted_resource.clone())
                    .await;
            }
            (TransactionOperation::Read { .. }, _) => {
                // Read operations don't need rollback
            }
            _ => {
                // Unexpected combinations - should not happen with proper snapshots
                return Err(CoreError::invalid_resource(
                    "Invalid rollback state".to_string(),
                ));
            }
        }
        Ok(())
    }
}

impl Default for InMemoryStorage {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl TransactionManager for InMemoryStorage {
    async fn begin_transaction(&mut self) -> Result<Transaction> {
        Ok(Transaction::new())
    }

    async fn execute_transaction(&mut self, transaction: &mut Transaction) -> Result<()> {
        if !transaction.can_execute() {
            return Err(CoreError::invalid_resource(
                "Transaction cannot be executed".to_string(),
            ));
        }

        transaction.mark_executing();

        // Capture rollback snapshots for all write operations
        // Clone operations to avoid borrow checker issues
        let operations = transaction.operations.clone();
        for (_operation_id, operation) in &operations {
            if operation.is_write_operation() {
                let snapshot = self.capture_rollback_snapshot(operation).await?;
                let key = match operation {
                    TransactionOperation::Create {
                        resource_type,
                        resource,
                    } => make_storage_key(resource_type, &resource.id),
                    TransactionOperation::Update {
                        resource_type, id, ..
                    }
                    | TransactionOperation::Delete { resource_type, id } => {
                        make_storage_key(resource_type, id)
                    }
                    _ => continue,
                };
                transaction.add_rollback_snapshot(key, snapshot);
            }
        }

        // Execute all operations
        let mut has_failures = false;
        for (operation_id, operation) in transaction.operations.clone() {
            let result = match self.execute_operation(&operation).await {
                Ok(resource) => {
                    TransactionOperationResult::success(operation_id, operation, resource)
                }
                Err(error) => {
                    has_failures = true;
                    TransactionOperationResult::failure(operation_id, operation, error.to_string())
                }
            };
            transaction.add_result(result);
        }

        if has_failures {
            transaction.mark_failed();
            return Err(CoreError::invalid_resource(
                "Transaction execution failed".to_string(),
            ));
        }

        Ok(())
    }

    async fn commit_transaction(&mut self, transaction: &mut Transaction) -> Result<()> {
        if !transaction.can_commit() {
            return Err(CoreError::invalid_resource(
                "Transaction cannot be committed".to_string(),
            ));
        }

        // In our in-memory implementation, operations are already applied during execution
        // So commit just marks the transaction as completed
        transaction.mark_committed();

        // Update statistics
        let mut stats = self.transaction_stats.write().await;
        stats.record_transaction(transaction);

        Ok(())
    }

    async fn rollback_transaction(&mut self, transaction: &mut Transaction) -> Result<()> {
        if !transaction.can_rollback() {
            return Err(CoreError::invalid_resource(
                "Transaction cannot be rolled back".to_string(),
            ));
        }

        // Rollback operations in reverse order
        for (operation_id, operation) in transaction.operations.iter().rev() {
            if operation.is_write_operation() {
                let key = match operation {
                    TransactionOperation::Create {
                        resource_type,
                        resource,
                    } => make_storage_key(resource_type, &resource.id),
                    TransactionOperation::Update {
                        resource_type, id, ..
                    }
                    | TransactionOperation::Delete { resource_type, id } => {
                        make_storage_key(resource_type, id)
                    }
                    _ => continue,
                };

                if let Some(snapshot) = transaction.get_rollback_snapshot(&key)
                    && let Err(error) = self.rollback_operation(operation, snapshot).await
                {
                    // Log rollback errors but continue with other operations
                    eprintln!("Rollback error for operation {operation_id}: {error}");
                }
            }
        }

        transaction.mark_rolled_back();

        // Update statistics
        let mut stats = self.transaction_stats.write().await;
        stats.record_transaction(transaction);

        Ok(())
    }

    async fn abort_transaction(&mut self, transaction: &mut Transaction) -> Result<()> {
        // Abort is like rollback but can be called even if not failed
        if transaction.state == crate::transaction::TransactionState::Executing
            || transaction.state == crate::transaction::TransactionState::Failed
        {
            self.rollback_transaction(transaction).await
        } else {
            // If not executing, just mark as failed
            transaction.mark_failed();
            Ok(())
        }
    }

    fn get_transaction_stats(&self) -> TransactionStats {
        // Clone stats under read lock to avoid lifetimes/unsafe
        self.transaction_stats.blocking_read().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use octofhir_core::{ResourceStatus, ResourceType};

    fn create_test_resource(id: &str, resource_type: ResourceType) -> ResourceEnvelope {
        ResourceEnvelope::new(id.to_string(), resource_type).with_status(ResourceStatus::Active)
    }

    #[tokio::test]
    async fn test_storage_basic_operations() {
        let storage = InMemoryStorage::new();
        let resource = create_test_resource("patient-123", ResourceType::Patient);

        // Test insert
        storage
            .insert(&ResourceType::Patient, resource.clone())
            .await
            .unwrap();
        assert_eq!(storage.count().await, 1);

        // Test get
        let retrieved = storage
            .get(&ResourceType::Patient, "patient-123")
            .await
            .unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().id, "patient-123");

        // Test exists
        assert!(storage.exists(&ResourceType::Patient, "patient-123").await);
        assert!(!storage.exists(&ResourceType::Patient, "nonexistent").await);

        // Test update
        let mut updated_resource = resource.clone();
        updated_resource.status = ResourceStatus::Inactive;
        let old_resource = storage
            .update(&ResourceType::Patient, "patient-123", updated_resource)
            .await
            .unwrap();
        assert_eq!(old_resource.status, ResourceStatus::Active);

        let current = storage
            .get(&ResourceType::Patient, "patient-123")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(current.status, ResourceStatus::Inactive);

        // Test delete
        let deleted = storage
            .delete(&ResourceType::Patient, "patient-123")
            .await
            .unwrap();
        assert_eq!(deleted.id, "patient-123");
        assert_eq!(storage.count().await, 0);
    }

    #[tokio::test]
    async fn test_storage_conflicts_and_not_found() {
        let storage = InMemoryStorage::new();
        let resource = create_test_resource("patient-123", ResourceType::Patient);

        // Insert resource
        storage
            .insert(&ResourceType::Patient, resource.clone())
            .await
            .unwrap();

        // Test conflict on duplicate insert
        let conflict_result = storage.insert(&ResourceType::Patient, resource).await;
        assert!(conflict_result.is_err());
        assert!(matches!(
            conflict_result.unwrap_err(),
            CoreError::ResourceConflict { .. }
        ));

        // Test not found on update
        let update_result = storage
            .update(
                &ResourceType::Patient,
                "nonexistent",
                create_test_resource("nonexistent", ResourceType::Patient),
            )
            .await;
        assert!(update_result.is_err());
        assert!(matches!(
            update_result.unwrap_err(),
            CoreError::ResourceNotFound { .. }
        ));

        // Test idempotent delete on non-existent resource (per FHIR spec)
        let delete_result = storage.delete(&ResourceType::Patient, "nonexistent").await;
        assert!(
            delete_result.is_ok(),
            "Delete of non-existent resource should succeed (idempotent)"
        );
    }

    #[tokio::test]
    async fn test_storage_count_by_type() {
        let storage = InMemoryStorage::new();

        storage
            .insert(
                &ResourceType::Patient,
                create_test_resource("patient-1", ResourceType::Patient),
            )
            .await
            .unwrap();
        storage
            .insert(
                &ResourceType::Patient,
                create_test_resource("patient-2", ResourceType::Patient),
            )
            .await
            .unwrap();
        storage
            .insert(
                &ResourceType::Organization,
                create_test_resource("org-1", ResourceType::Organization),
            )
            .await
            .unwrap();

        assert_eq!(storage.count().await, 3);
        assert_eq!(storage.count_by_type(&ResourceType::Patient).await, 2);
        assert_eq!(storage.count_by_type(&ResourceType::Organization).await, 1);
        assert_eq!(storage.count_by_type(&ResourceType::Practitioner).await, 0);
    }

    #[tokio::test]
    async fn test_transaction_create_commit() {
        let mut storage = InMemoryStorage::new();
        let mut tx = storage.begin_transaction().await.unwrap();

        let resource = create_test_resource("patient-123", ResourceType::Patient);
        tx.create_resource(ResourceType::Patient, resource).unwrap();

        // Execute the transaction
        storage.execute_transaction(&mut tx).await.unwrap();
        assert_eq!(tx.state, crate::transaction::TransactionState::Executing);

        // Commit the transaction
        storage.commit_transaction(&mut tx).await.unwrap();
        assert_eq!(tx.state, crate::transaction::TransactionState::Committed);

        // Verify resource exists
        assert!(storage.exists(&ResourceType::Patient, "patient-123").await);
        assert_eq!(storage.count().await, 1);
    }

    #[tokio::test]
    async fn test_transaction_rollback() {
        let mut storage = InMemoryStorage::new();

        // Pre-populate with a resource
        let original_resource = create_test_resource("patient-123", ResourceType::Patient);
        storage
            .insert(&ResourceType::Patient, original_resource.clone())
            .await
            .unwrap();

        let mut tx = storage.begin_transaction().await.unwrap();

        // Update the resource in the transaction
        let mut updated_resource = original_resource.clone();
        updated_resource.status = ResourceStatus::Inactive;
        tx.update_resource(
            ResourceType::Patient,
            "patient-123".to_string(),
            updated_resource,
        )
        .unwrap();

        // Execute the transaction
        storage.execute_transaction(&mut tx).await.unwrap();

        // Verify the update was applied
        let current = storage
            .get(&ResourceType::Patient, "patient-123")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(current.status, ResourceStatus::Inactive);

        // Rollback the transaction
        storage.rollback_transaction(&mut tx).await.unwrap();
        assert_eq!(tx.state, crate::transaction::TransactionState::RolledBack);

        // Verify the resource was rolled back to original state
        let rolled_back = storage
            .get(&ResourceType::Patient, "patient-123")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(rolled_back.status, ResourceStatus::Active);
    }

    #[tokio::test]
    async fn test_transaction_rollback_create() {
        let mut storage = InMemoryStorage::new();
        let mut tx = storage.begin_transaction().await.unwrap();

        let resource = create_test_resource("patient-123", ResourceType::Patient);
        tx.create_resource(ResourceType::Patient, resource).unwrap();

        // Execute the transaction
        storage.execute_transaction(&mut tx).await.unwrap();
        assert!(storage.exists(&ResourceType::Patient, "patient-123").await);

        // Rollback the transaction
        storage.rollback_transaction(&mut tx).await.unwrap();

        // Verify the created resource was removed
        assert!(!storage.exists(&ResourceType::Patient, "patient-123").await);
        assert_eq!(storage.count().await, 0);
    }

    #[tokio::test]
    async fn test_transaction_rollback_delete() {
        let mut storage = InMemoryStorage::new();

        // Pre-populate with a resource
        let resource = create_test_resource("patient-123", ResourceType::Patient);
        storage
            .insert(&ResourceType::Patient, resource.clone())
            .await
            .unwrap();

        let mut tx = storage.begin_transaction().await.unwrap();
        tx.delete_resource(ResourceType::Patient, "patient-123".to_string())
            .unwrap();

        // Execute the transaction
        storage.execute_transaction(&mut tx).await.unwrap();
        assert!(!storage.exists(&ResourceType::Patient, "patient-123").await);

        // Rollback the transaction
        storage.rollback_transaction(&mut tx).await.unwrap();

        // Verify the resource was restored
        assert!(storage.exists(&ResourceType::Patient, "patient-123").await);
        let restored = storage
            .get(&ResourceType::Patient, "patient-123")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(restored.id, "patient-123");
    }

    #[tokio::test]
    async fn test_transaction_mixed_operations() {
        let mut storage = InMemoryStorage::new();

        // Pre-populate
        let existing_resource = create_test_resource("patient-existing", ResourceType::Patient);
        storage
            .insert(&ResourceType::Patient, existing_resource.clone())
            .await
            .unwrap();

        let mut tx = storage.begin_transaction().await.unwrap();

        // Create a new resource
        let new_resource = create_test_resource("patient-new", ResourceType::Patient);
        tx.create_resource(ResourceType::Patient, new_resource)
            .unwrap();

        // Update existing resource
        let mut updated_resource = existing_resource.clone();
        updated_resource.status = ResourceStatus::Inactive;
        tx.update_resource(
            ResourceType::Patient,
            "patient-existing".to_string(),
            updated_resource,
        )
        .unwrap();

        // Delete operation (we'll create another resource to delete)
        let to_delete = create_test_resource("patient-delete", ResourceType::Patient);
        storage
            .insert(&ResourceType::Patient, to_delete)
            .await
            .unwrap();
        tx.delete_resource(ResourceType::Patient, "patient-delete".to_string())
            .unwrap();

        assert_eq!(tx.operation_count(), 3);
        assert_eq!(tx.write_operation_count(), 3);

        // Execute transaction
        storage.execute_transaction(&mut tx).await.unwrap();

        // Verify all operations were applied
        assert!(storage.exists(&ResourceType::Patient, "patient-new").await);
        assert!(
            storage
                .exists(&ResourceType::Patient, "patient-existing")
                .await
        );
        assert!(
            !storage
                .exists(&ResourceType::Patient, "patient-delete")
                .await
        );

        let updated = storage
            .get(&ResourceType::Patient, "patient-existing")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.status, ResourceStatus::Inactive);

        // Now rollback
        storage.rollback_transaction(&mut tx).await.unwrap();

        // Verify rollback
        assert!(!storage.exists(&ResourceType::Patient, "patient-new").await); // Created resource removed
        assert!(
            storage
                .exists(&ResourceType::Patient, "patient-delete")
                .await
        ); // Deleted resource restored

        let restored = storage
            .get(&ResourceType::Patient, "patient-existing")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(restored.status, ResourceStatus::Active); // Update rolled back
    }

    #[tokio::test]
    async fn test_transaction_abort() {
        let mut storage = InMemoryStorage::new();
        let mut tx = storage.begin_transaction().await.unwrap();

        let resource = create_test_resource("patient-123", ResourceType::Patient);
        tx.create_resource(ResourceType::Patient, resource).unwrap();

        storage.execute_transaction(&mut tx).await.unwrap();
        assert!(storage.exists(&ResourceType::Patient, "patient-123").await);

        // Abort the transaction
        storage.abort_transaction(&mut tx).await.unwrap();
        assert_eq!(tx.state, crate::transaction::TransactionState::RolledBack);

        // Verify resource was removed
        assert!(!storage.exists(&ResourceType::Patient, "patient-123").await);
    }

    #[tokio::test]
    async fn test_transaction_failure_handling() {
        let mut storage = InMemoryStorage::new();
        let mut tx = storage.begin_transaction().await.unwrap();

        // Try to create a resource
        let resource = create_test_resource("patient-123", ResourceType::Patient);
        tx.create_resource(ResourceType::Patient, resource.clone())
            .unwrap();

        // Pre-insert the same resource to cause a conflict
        storage
            .insert(&ResourceType::Patient, resource)
            .await
            .unwrap();

        // Execute should fail due to conflict
        let result = storage.execute_transaction(&mut tx).await;
        assert!(result.is_err());
        assert_eq!(tx.state, crate::transaction::TransactionState::Failed);

        // Should be able to rollback failed transaction
        assert!(tx.can_rollback());
        storage.rollback_transaction(&mut tx).await.unwrap();
    }

    #[tokio::test]
    async fn test_make_storage_key() {
        let key = make_storage_key(&ResourceType::Patient, "123");
        assert_eq!(key, "Patient/123");

        let key2 = make_storage_key(&ResourceType::Organization, "org-456");
        assert_eq!(key2, "Organization/org-456");
    }

    #[test]
    fn test_storage_key_type() {
        let _key: StorageKey = "Patient/123".to_string();
        // Just testing the type alias compiles correctly
    }

    #[tokio::test]
    async fn test_storage_empty_id() {
        let storage = InMemoryStorage::new();
        let resource = create_test_resource("", ResourceType::Patient);

        // Should handle empty ID
        storage
            .insert(&ResourceType::Patient, resource.clone())
            .await
            .unwrap();
        assert_eq!(storage.count().await, 1);

        let retrieved = storage.get(&ResourceType::Patient, "").await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().id, "");
    }

    #[tokio::test]
    async fn test_storage_special_characters_in_id() {
        let storage = InMemoryStorage::new();
        let special_ids = vec![
            "patient/with/slashes",
            "patient-with-dashes",
            "patient_with_underscores",
            "patient.with.dots",
            "patient@with@symbols",
            "患者-unicode",
            "patient with spaces",
        ];

        for id in special_ids {
            let resource = create_test_resource(id, ResourceType::Patient);
            storage
                .insert(&ResourceType::Patient, resource.clone())
                .await
                .unwrap();

            let retrieved = storage.get(&ResourceType::Patient, id).await.unwrap();
            assert!(retrieved.is_some());
            assert_eq!(retrieved.unwrap().id, id);

            // Test update with special ID
            let mut updated = resource.clone();
            updated.status = ResourceStatus::Inactive;
            storage
                .update(&ResourceType::Patient, id, updated)
                .await
                .unwrap();

            // Test delete with special ID
            storage.delete(&ResourceType::Patient, id).await.unwrap();
        }

        assert_eq!(storage.count().await, 0);
    }

    #[tokio::test]
    async fn test_storage_large_id() {
        let storage = InMemoryStorage::new();
        // Test with very long ID (1000 characters)
        let long_id = "a".repeat(1000);
        let resource = create_test_resource(&long_id, ResourceType::Patient);

        storage
            .insert(&ResourceType::Patient, resource.clone())
            .await
            .unwrap();

        let retrieved = storage.get(&ResourceType::Patient, &long_id).await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().id.len(), 1000);
    }

    #[tokio::test]
    async fn test_storage_multiple_resource_types_same_id() {
        let storage = InMemoryStorage::new();
        let same_id = "same-id-123";

        // Create resources with same ID but different types
        let patient = create_test_resource(same_id, ResourceType::Patient);
        let org = create_test_resource(same_id, ResourceType::Organization);
        let practitioner = create_test_resource(same_id, ResourceType::Practitioner);

        storage
            .insert(&ResourceType::Patient, patient)
            .await
            .unwrap();
        storage
            .insert(&ResourceType::Organization, org)
            .await
            .unwrap();
        storage
            .insert(&ResourceType::Practitioner, practitioner)
            .await
            .unwrap();

        // All should exist independently
        assert!(storage.exists(&ResourceType::Patient, same_id).await);
        assert!(storage.exists(&ResourceType::Organization, same_id).await);
        assert!(storage.exists(&ResourceType::Practitioner, same_id).await);
        assert_eq!(storage.count().await, 3);

        // Count by type should work correctly
        assert_eq!(storage.count_by_type(&ResourceType::Patient).await, 1);
        assert_eq!(storage.count_by_type(&ResourceType::Organization).await, 1);
        assert_eq!(storage.count_by_type(&ResourceType::Practitioner).await, 1);
    }

    #[tokio::test]
    async fn test_storage_update_nonexistent_with_empty_storage() {
        let storage = InMemoryStorage::new();
        assert_eq!(storage.count().await, 0);

        let result = storage
            .update(
                &ResourceType::Patient,
                "nonexistent",
                create_test_resource("nonexistent", ResourceType::Patient),
            )
            .await;

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            CoreError::ResourceNotFound { .. }
        ));
        assert_eq!(storage.count().await, 0); // Storage should remain empty
    }

    #[tokio::test]
    async fn test_storage_delete_from_empty_storage() {
        let storage = InMemoryStorage::new();
        assert_eq!(storage.count().await, 0);

        // Per FHIR spec: delete of non-existent resource is idempotent (success)
        let result = storage.delete(&ResourceType::Patient, "nonexistent").await;
        assert!(
            result.is_ok(),
            "Delete of non-existent resource should succeed (idempotent)"
        );
        assert_eq!(storage.count().await, 0);
    }

    #[tokio::test]
    async fn test_storage_case_sensitive_ids() {
        let storage = InMemoryStorage::new();

        let lower_case = create_test_resource("patient-123", ResourceType::Patient);
        let upper_case = create_test_resource("PATIENT-123", ResourceType::Patient);
        let mixed_case = create_test_resource("Patient-123", ResourceType::Patient);

        storage
            .insert(&ResourceType::Patient, lower_case)
            .await
            .unwrap();
        storage
            .insert(&ResourceType::Patient, upper_case)
            .await
            .unwrap();
        storage
            .insert(&ResourceType::Patient, mixed_case)
            .await
            .unwrap();

        // All should be treated as different resources
        assert_eq!(storage.count().await, 3);
        assert!(storage.exists(&ResourceType::Patient, "patient-123").await);
        assert!(storage.exists(&ResourceType::Patient, "PATIENT-123").await);
        assert!(storage.exists(&ResourceType::Patient, "Patient-123").await);
        assert!(!storage.exists(&ResourceType::Patient, "patient-124").await);
    }

    #[tokio::test]
    async fn test_storage_whitespace_only_id() {
        let storage = InMemoryStorage::new();
        let whitespace_ids = vec![" ", "  ", "\t", "\n", "\r\n", " \t \n "];

        for id in whitespace_ids {
            let resource = create_test_resource(id, ResourceType::Patient);
            storage
                .insert(&ResourceType::Patient, resource.clone())
                .await
                .unwrap();

            let retrieved = storage.get(&ResourceType::Patient, id).await.unwrap();
            assert!(retrieved.is_some());
            assert_eq!(retrieved.unwrap().id, id);

            storage.delete(&ResourceType::Patient, id).await.unwrap();
        }

        assert_eq!(storage.count().await, 0);
    }

    #[tokio::test]
    async fn test_storage_resource_data_integrity() {
        let storage = InMemoryStorage::new();
        let mut resource = create_test_resource("integrity-test", ResourceType::Patient);

        // Add complex data to test deep cloning
        resource.add_field(
            "name".to_string(),
            serde_json::json!([{
                "use": "official",
                "given": ["John", "Middle"],
                "family": "Doe"
            }]),
        );
        resource.add_field(
            "nested".to_string(),
            serde_json::json!({
                "level1": {
                    "level2": {
                        "value": "deep"
                    }
                }
            }),
        );

        storage
            .insert(&ResourceType::Patient, resource.clone())
            .await
            .unwrap();

        // Modify original resource after insertion
        resource.add_field("modified".to_string(), serde_json::json!("after insertion"));

        // Retrieved resource should not have the modification
        let retrieved = storage
            .get(&ResourceType::Patient, "integrity-test")
            .await
            .unwrap()
            .unwrap();
        assert!(retrieved.get_field("modified").is_none());
        assert_eq!(retrieved.get_field("name").unwrap()[0]["family"], "Doe");
        assert_eq!(
            retrieved.get_field("nested").unwrap()["level1"]["level2"]["value"],
            "deep"
        );
    }

    #[tokio::test]
    async fn test_make_storage_key_edge_cases() {
        // Test make_storage_key with edge cases
        let key1 = make_storage_key(&ResourceType::Patient, "");
        assert_eq!(key1, "Patient/");

        let key2 = make_storage_key(&ResourceType::Organization, "id/with/slashes");
        assert_eq!(key2, "Organization/id/with/slashes");

        let key3 = make_storage_key(&ResourceType::Practitioner, "id with spaces");
        assert_eq!(key3, "Practitioner/id with spaces");
    }

    #[tokio::test]
    async fn test_concurrent_read_operations() {
        use std::sync::Arc;
        use tokio::task::JoinSet;

        let storage = Arc::new(InMemoryStorage::new());

        // Pre-populate with some resources
        for i in 0..10 {
            let resource = create_test_resource(&format!("patient-{i}"), ResourceType::Patient);
            storage
                .insert(&ResourceType::Patient, resource)
                .await
                .unwrap();
        }

        // Spawn multiple concurrent read operations
        let mut join_set = JoinSet::new();
        for i in 0..50 {
            let storage_clone = Arc::clone(&storage);
            join_set.spawn(async move {
                let target_id = format!("patient-{}", i % 10);
                let result = storage_clone.get(&ResourceType::Patient, &target_id).await;
                result.unwrap().is_some()
            });
        }

        // All reads should succeed and find resources for IDs 0-9
        let mut success_count = 0;
        while let Some(result) = join_set.join_next().await {
            if result.unwrap() {
                success_count += 1;
            }
        }

        assert_eq!(success_count, 50);
        assert_eq!(storage.count().await, 10);
    }

    #[tokio::test]
    async fn test_concurrent_insert_operations() {
        use std::sync::Arc;
        use tokio::task::JoinSet;

        let storage = Arc::new(InMemoryStorage::new());
        let mut join_set = JoinSet::new();

        // Spawn concurrent insert operations
        for i in 0..20 {
            let storage_clone = Arc::clone(&storage);
            join_set.spawn(async move {
                let resource =
                    create_test_resource(&format!("concurrent-{i}"), ResourceType::Patient);
                storage_clone.insert(&ResourceType::Patient, resource).await
            });
        }

        // All inserts should succeed since they have unique IDs
        let mut success_count = 0;
        while let Some(result) = join_set.join_next().await {
            if result.unwrap().is_ok() {
                success_count += 1;
            }
        }

        assert_eq!(success_count, 20);
        assert_eq!(storage.count().await, 20);
    }

    #[tokio::test]
    async fn test_concurrent_conflicting_inserts() {
        use std::sync::Arc;
        use tokio::task::JoinSet;

        let storage = Arc::new(InMemoryStorage::new());
        let mut join_set = JoinSet::new();

        // Spawn concurrent insert operations with the same ID to test conflict handling
        for _ in 0..10 {
            let storage_clone = Arc::clone(&storage);
            join_set.spawn(async move {
                let resource = create_test_resource("conflict-test", ResourceType::Patient);
                storage_clone.insert(&ResourceType::Patient, resource).await
            });
        }

        let mut success_count = 0;
        let mut conflict_count = 0;

        while let Some(result) = join_set.join_next().await {
            match result.unwrap() {
                Ok(_) => success_count += 1,
                Err(CoreError::ResourceConflict { .. }) => conflict_count += 1,
                Err(_) => panic!("Unexpected error type"),
            }
        }

        // Only one insert should succeed, others should conflict
        assert_eq!(success_count, 1);
        assert_eq!(conflict_count, 9);
        assert_eq!(storage.count().await, 1);
        assert!(
            storage
                .exists(&ResourceType::Patient, "conflict-test")
                .await
        );
    }

    #[tokio::test]
    async fn test_concurrent_mixed_operations() {
        use std::sync::Arc;
        use tokio::task::JoinSet;

        let storage = Arc::new(InMemoryStorage::new());

        // Pre-populate with some resources
        for i in 0..5 {
            let resource = create_test_resource(&format!("mixed-{i}"), ResourceType::Patient);
            storage
                .insert(&ResourceType::Patient, resource)
                .await
                .unwrap();
        }

        let mut join_set = JoinSet::new();

        // Concurrent reads
        for i in 0..10 {
            let storage_clone = Arc::clone(&storage);
            join_set.spawn(async move {
                let id = format!("mixed-{}", i % 5);
                storage_clone
                    .get(&ResourceType::Patient, &id)
                    .await
                    .unwrap()
                    .is_some()
            });
        }

        // Concurrent updates
        for i in 0..5 {
            let storage_clone = Arc::clone(&storage);
            join_set.spawn(async move {
                let id = format!("mixed-{i}");
                let mut resource = create_test_resource(&id, ResourceType::Patient);
                resource.status = ResourceStatus::Inactive;
                storage_clone
                    .update(&ResourceType::Patient, &id, resource)
                    .await
                    .is_ok()
            });
        }

        // Concurrent new inserts
        for i in 5..10 {
            let storage_clone = Arc::clone(&storage);
            join_set.spawn(async move {
                let resource = create_test_resource(&format!("mixed-{i}"), ResourceType::Patient);
                storage_clone
                    .insert(&ResourceType::Patient, resource)
                    .await
                    .is_ok()
            });
        }

        let mut results = Vec::new();
        while let Some(result) = join_set.join_next().await {
            results.push(result.unwrap());
        }

        // All operations should succeed
        assert!(results.iter().all(|&r| r));
        assert_eq!(storage.count().await, 10);
    }

    #[tokio::test]
    async fn test_concurrent_count_operations() {
        use std::sync::Arc;
        use tokio::task::JoinSet;

        let storage = Arc::new(InMemoryStorage::new());

        // Pre-populate storage
        for i in 0..10 {
            let patient = create_test_resource(&format!("patient-{i}"), ResourceType::Patient);
            let org = create_test_resource(&format!("org-{i}"), ResourceType::Organization);
            storage
                .insert(&ResourceType::Patient, patient)
                .await
                .unwrap();
            storage
                .insert(&ResourceType::Organization, org)
                .await
                .unwrap();
        }

        let mut join_set = JoinSet::new();

        // Concurrent count operations
        for _ in 0..20 {
            let storage_clone = Arc::clone(&storage);
            join_set.spawn(async move {
                (
                    storage_clone.count().await,
                    storage_clone.count_by_type(&ResourceType::Patient).await,
                    storage_clone
                        .count_by_type(&ResourceType::Organization)
                        .await,
                )
            });
        }

        // All count operations should return consistent results
        while let Some(result) = join_set.join_next().await {
            let (total, patient_count, org_count) = result.unwrap();
            assert_eq!(total, 20);
            assert_eq!(patient_count, 10);
            assert_eq!(org_count, 10);
        }
    }

    // Advanced concurrency tests for task 2.5.2
    #[tokio::test]
    async fn test_high_volume_concurrent_operations() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};
        use tokio::task::JoinSet;

        let storage = Arc::new(InMemoryStorage::new());
        let success_counter = Arc::new(AtomicUsize::new(0));
        let error_counter = Arc::new(AtomicUsize::new(0));

        let mut join_set = JoinSet::new();

        // Spawn 1000 concurrent operations
        for i in 0..1000 {
            let storage_clone = Arc::clone(&storage);
            let success_counter_clone = Arc::clone(&success_counter);
            let error_counter_clone = Arc::clone(&error_counter);

            join_set.spawn(async move {
                let resource_id = format!("high-volume-{i}");
                let resource = create_test_resource(&resource_id, ResourceType::Patient);

                // Try to insert
                match storage_clone
                    .insert(&ResourceType::Patient, resource.clone())
                    .await
                {
                    Ok(_) => {
                        success_counter_clone.fetch_add(1, Ordering::Relaxed);

                        // Try to read back
                        if storage_clone
                            .get(&ResourceType::Patient, &resource_id)
                            .await
                            .unwrap()
                            .is_some()
                        {
                            // Try to update
                            let mut updated = resource;
                            updated.status = ResourceStatus::Inactive;
                            let _ = storage_clone
                                .update(&ResourceType::Patient, &resource_id, updated)
                                .await;

                            // Try to delete
                            let _ = storage_clone
                                .delete(&ResourceType::Patient, &resource_id)
                                .await;
                        }
                    }
                    Err(_) => {
                        error_counter_clone.fetch_add(1, Ordering::Relaxed);
                    }
                }
            });
        }

        // Wait for all operations to complete
        while (join_set.join_next().await).is_some() {}

        // All operations should have succeeded (unique IDs)
        assert_eq!(success_counter.load(Ordering::Relaxed), 1000);
        assert_eq!(error_counter.load(Ordering::Relaxed), 0);

        // Storage should be empty after all deletes
        assert_eq!(storage.count().await, 0);
    }

    #[tokio::test]
    async fn test_race_condition_update_delete() {
        use std::sync::Arc;
        use tokio::task::JoinSet;

        let storage = Arc::new(InMemoryStorage::new());

        // Insert a resource
        let resource = create_test_resource("race-test", ResourceType::Patient);
        storage
            .insert(&ResourceType::Patient, resource.clone())
            .await
            .unwrap();

        let mut join_set = JoinSet::new();

        // Spawn multiple concurrent updates and deletes on the same resource
        for i in 0..50 {
            let storage_clone = Arc::clone(&storage);
            let resource_clone = resource.clone();

            if i % 2 == 0 {
                // Even: try to update
                join_set.spawn(async move {
                    let mut updated = resource_clone;
                    updated.status = if i % 4 == 0 {
                        ResourceStatus::Active
                    } else {
                        ResourceStatus::Inactive
                    };
                    storage_clone
                        .update(&ResourceType::Patient, "race-test", updated)
                        .await
                });
            } else {
                // Odd: try to delete
                join_set.spawn(async move {
                    storage_clone
                        .delete(&ResourceType::Patient, "race-test")
                        .await
                });
            }
        }

        let mut update_successes = 0;
        let mut update_not_found = 0;
        let mut delete_successes = 0;
        let mut delete_not_found = 0;

        while let Some(result) = join_set.join_next().await {
            match result.unwrap() {
                Ok(_) => {
                    // Could be either successful update or successful delete
                    if storage.exists(&ResourceType::Patient, "race-test").await {
                        update_successes += 1;
                    } else {
                        delete_successes += 1;
                    }
                }
                Err(CoreError::ResourceNotFound { .. }) => {
                    // Resource was already deleted
                    if storage.exists(&ResourceType::Patient, "race-test").await {
                        update_not_found += 1;
                    } else {
                        delete_not_found += 1;
                    }
                }
                Err(_) => panic!("Unexpected error"),
            }
        }

        // Should have some operations succeed and some fail due to race conditions
        // The exact counts depend on timing, but we should have both successes and failures
        println!(
            "Update successes: {update_successes}, Update not found: {update_not_found}, Delete successes: {delete_successes}, Delete not found: {delete_not_found}"
        );

        // At least one delete should have succeeded eventually
        assert!(delete_successes > 0 || !storage.exists(&ResourceType::Patient, "race-test").await);
    }

    #[tokio::test]
    async fn test_concurrent_transaction_operations() {
        use tokio::task::JoinSet;

        // Note: This test demonstrates transaction isolation between different storage instances
        // since papaya doesn't support mutable shared access, each transaction needs its own storage
        let mut join_set = JoinSet::new();

        // Spawn multiple concurrent transactions, each with its own storage
        for tx_id in 0..10 {
            join_set.spawn(async move {
                let mut storage = InMemoryStorage::new();
                let mut tx = storage.begin_transaction().await.unwrap();

                // Each transaction creates 5 resources
                for i in 0..5 {
                    let resource = create_test_resource(
                        &format!("tx-{tx_id}-resource-{i}"),
                        ResourceType::Patient,
                    );
                    tx.create_resource(ResourceType::Patient, resource).unwrap();
                }

                // Execute and commit
                storage.execute_transaction(&mut tx).await.unwrap();
                storage.commit_transaction(&mut tx).await.unwrap();

                // Verify resources were created
                assert_eq!(storage.count().await, 5);

                tx_id
            });
        }

        let mut completed_transactions = Vec::new();
        while let Some(result) = join_set.join_next().await {
            completed_transactions.push(result.unwrap());
        }

        // All transactions should complete successfully
        assert_eq!(completed_transactions.len(), 10);
    }

    #[tokio::test]
    async fn test_stress_concurrent_mixed_workload() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::time::Duration;
        use tokio::task::JoinSet;

        let storage = Arc::new(InMemoryStorage::new());
        let operation_counter = Arc::new(AtomicUsize::new(0));

        // Pre-populate with some data
        for i in 0..100 {
            let resource = create_test_resource(&format!("stress-{i}"), ResourceType::Patient);
            storage
                .insert(&ResourceType::Patient, resource)
                .await
                .unwrap();
        }

        let mut join_set = JoinSet::new();

        // Heavy read workload
        for _ in 0..200 {
            let storage_clone = Arc::clone(&storage);
            let counter_clone = Arc::clone(&operation_counter);

            join_set.spawn(async move {
                for _ in 0..10 {
                    let id = format!("stress-{}", fastrand::usize(0..100));
                    let _ = storage_clone.get(&ResourceType::Patient, &id).await;
                    counter_clone.fetch_add(1, Ordering::Relaxed);
                }
            });
        }

        // Medium write workload
        for i in 0..50 {
            let storage_clone = Arc::clone(&storage);
            let counter_clone = Arc::clone(&operation_counter);

            join_set.spawn(async move {
                let resource =
                    create_test_resource(&format!("stress-new-{i}"), ResourceType::Organization);
                let _ = storage_clone
                    .insert(&ResourceType::Organization, resource)
                    .await;
                counter_clone.fetch_add(1, Ordering::Relaxed);
            });
        }

        // Light update workload
        for _ in 0..25 {
            let storage_clone = Arc::clone(&storage);
            let counter_clone = Arc::clone(&operation_counter);

            join_set.spawn(async move {
                let id = format!("stress-{}", fastrand::usize(0..50));
                if let Ok(Some(mut resource)) = storage_clone.get(&ResourceType::Patient, &id).await
                {
                    resource.status = ResourceStatus::Inactive;
                    let _ = storage_clone
                        .update(&ResourceType::Patient, &id, resource)
                        .await;
                }
                counter_clone.fetch_add(1, Ordering::Relaxed);
            });
        }

        // Wait for all operations with timeout
        let start_time = std::time::Instant::now();
        loop {
            match tokio::time::timeout(Duration::from_secs(30), join_set.join_next()).await {
                Ok(Some(task_result)) => {
                    task_result.unwrap();
                }
                Ok(None) => break,
                Err(_) => {
                    panic!("Operation timed out - possible deadlock or performance issue");
                }
            }
        }

        let elapsed = start_time.elapsed();
        println!("Stress test completed in {elapsed:?}");
        println!(
            "Total operations: {}",
            operation_counter.load(Ordering::Relaxed)
        );

        // Should have completed many operations
        assert!(operation_counter.load(Ordering::Relaxed) > 2000);
        assert!(elapsed.as_secs() < 10); // Should complete within reasonable time
    }

    #[tokio::test]
    async fn test_concurrent_exists_operations() {
        use std::sync::Arc;
        use tokio::task::JoinSet;

        let storage = Arc::new(InMemoryStorage::new());

        // Create some resources
        for i in 0..20 {
            let resource = create_test_resource(&format!("exists-{i}"), ResourceType::Patient);
            storage
                .insert(&ResourceType::Patient, resource)
                .await
                .unwrap();
        }

        let mut join_set = JoinSet::new();

        // Concurrent exists operations while resources are being deleted
        for _ in 0..100 {
            let storage_clone = Arc::clone(&storage);
            join_set.spawn(async move {
                let id = format!("exists-{}", fastrand::usize(0..20));
                storage_clone.exists(&ResourceType::Patient, &id).await
            });
        }

        // Concurrent deletes
        for i in 0..10 {
            let storage_clone = Arc::clone(&storage);
            join_set.spawn(async move {
                let id = format!("exists-{i}");
                let _ = storage_clone.delete(&ResourceType::Patient, &id).await;
                false // Just to match the return type
            });
        }

        // All operations should complete without panic
        while let Some(result) = join_set.join_next().await {
            let _ = result.unwrap(); // exists() returns bool, delete returns different type
        }

        // Some resources should still exist, some should be deleted
        let final_count = storage.count().await;
        assert!(final_count <= 20);
        assert!(final_count >= 10); // At least half should remain
    }

    #[tokio::test]
    async fn test_concurrent_storage_key_generation() {
        use std::collections::HashSet;
        use std::sync::Arc;
        use std::sync::Mutex;
        use tokio::task::JoinSet;

        let keys = Arc::new(Mutex::new(HashSet::new()));
        let mut join_set = JoinSet::new();

        // Generate storage keys concurrently
        for i in 0..1000 {
            let keys_clone = Arc::clone(&keys);

            join_set.spawn(async move {
                let resource_types = [
                    ResourceType::Patient,
                    ResourceType::Organization,
                    ResourceType::Practitioner,
                ];

                let resource_type = &resource_types[i % 3];
                let id = format!("concurrent-key-{i}");
                let key = make_storage_key(resource_type, &id);

                {
                    let mut keys_guard = keys_clone.lock().unwrap();
                    keys_guard.insert(key.clone());
                }

                key
            });
        }

        let mut generated_keys = Vec::new();
        while let Some(result) = join_set.join_next().await {
            generated_keys.push(result.unwrap());
        }

        // All keys should be unique
        assert_eq!(generated_keys.len(), 1000);

        let unique_keys: HashSet<_> = generated_keys.into_iter().collect();
        assert_eq!(unique_keys.len(), 1000);

        // Verify keys have the expected format
        let keys_guard = keys.lock().unwrap();
        for key in keys_guard.iter() {
            assert!(key.contains("/"));
            assert!(
                key.starts_with("Patient/")
                    || key.starts_with("Organization/")
                    || key.starts_with("Practitioner/")
            );
        }
    }

    #[tokio::test]
    async fn test_concurrent_count_by_type_with_mutations() {
        use std::sync::Arc;
        use tokio::task::JoinSet;

        let storage = Arc::new(InMemoryStorage::new());
        let mut join_set = JoinSet::new();

        // Concurrent count operations
        for _ in 0..50 {
            let storage_clone = Arc::clone(&storage);
            join_set
                .spawn(async move { storage_clone.count_by_type(&ResourceType::Patient).await });
        }

        // Concurrent insertions
        for i in 0..25 {
            let storage_clone = Arc::clone(&storage);
            join_set.spawn(async move {
                let resource =
                    create_test_resource(&format!("count-mut-{i}"), ResourceType::Patient);
                storage_clone
                    .insert(&ResourceType::Patient, resource)
                    .await
                    .map(|_| 0)
                    .unwrap_or(0)
            });
        }

        // Concurrent deletions (will fail for non-existent resources)
        for i in 0..10 {
            let storage_clone = Arc::clone(&storage);
            join_set.spawn(async move {
                let _ = storage_clone
                    .delete(&ResourceType::Patient, &format!("count-mut-{i}"))
                    .await;
                0usize
            });
        }

        let mut counts = Vec::new();
        while let Some(result) = join_set.join_next().await {
            counts.push(result.unwrap());
        }

        // Counts should be reasonable (between 0 and 25)
        for count in &counts[0..50] {
            // First 50 are count operations
            assert!(*count <= 25);
        }

        // Final state should be consistent
        let final_count = storage.count().await;
        let final_patient_count = storage.count_by_type(&ResourceType::Patient).await;

        assert_eq!(final_count, final_patient_count);
        assert!(final_patient_count <= 25);
    }

    // Search functionality tests for task 2.5.3

    fn create_test_patient_with_data(
        id: &str,
        name: &str,
        birth_date: &str,
        active: bool,
    ) -> ResourceEnvelope {
        let mut resource = create_test_resource(id, ResourceType::Patient);
        resource.add_field(
            "name".to_string(),
            serde_json::json!([{
                "use": "official",
                "family": name,
                "given": ["Test"]
            }]),
        );
        resource.add_field("birthDate".to_string(), serde_json::json!(birth_date));
        resource.add_field("active".to_string(), serde_json::json!(active));
        resource.add_field(
            "identifier".to_string(),
            serde_json::json!([{
                "system": "http://example.com/mrn",
                "value": format!("MRN-{}", id)
            }]),
        );
        resource
    }

    #[tokio::test]
    async fn test_search_exact_filter() {
        use crate::query::{QueryFilter, SearchQuery};

        let storage = InMemoryStorage::new();

        // Create test data
        let patients = vec![
            create_test_patient_with_data("patient-1", "Smith", "1990-01-01", true),
            create_test_patient_with_data("patient-2", "Johnson", "1985-05-15", false),
            create_test_patient_with_data("patient-3", "Williams", "1992-12-30", true),
        ];

        for patient in &patients {
            storage
                .insert(&ResourceType::Patient, patient.clone())
                .await
                .unwrap();
        }

        // Test exact _id filter
        let query = SearchQuery::new(ResourceType::Patient).with_filter(QueryFilter::Exact {
            field: "_id".to_string(),
            value: "patient-2".to_string(),
        });

        let result = storage.search(&query).await.unwrap();
        assert_eq!(result.total, 1);
        assert_eq!(result.resources[0].id, "patient-2");

        // Test exact field filter
        let query = SearchQuery::new(ResourceType::Patient).with_filter(QueryFilter::Exact {
            field: "birthDate".to_string(),
            value: "1990-01-01".to_string(),
        });

        let result = storage.search(&query).await.unwrap();
        assert_eq!(result.total, 1);
        assert_eq!(result.resources[0].id, "patient-1");
    }

    #[tokio::test]
    async fn test_search_boolean_filter() {
        use crate::query::{QueryFilter, SearchQuery};

        let storage = InMemoryStorage::new();

        let patients = vec![
            create_test_patient_with_data("patient-1", "Smith", "1990-01-01", true),
            create_test_patient_with_data("patient-2", "Johnson", "1985-05-15", false),
            create_test_patient_with_data("patient-3", "Williams", "1992-12-30", true),
        ];

        for patient in &patients {
            storage
                .insert(&ResourceType::Patient, patient.clone())
                .await
                .unwrap();
        }

        // Test active = true
        let query = SearchQuery::new(ResourceType::Patient).with_filter(QueryFilter::Boolean {
            field: "active".to_string(),
            value: true,
        });

        let result = storage.search(&query).await.unwrap();
        assert_eq!(result.total, 2);
        assert!(
            result
                .resources
                .iter()
                .all(|r| r.id == "patient-1" || r.id == "patient-3")
        );

        // Test active = false
        let query = SearchQuery::new(ResourceType::Patient).with_filter(QueryFilter::Boolean {
            field: "active".to_string(),
            value: false,
        });

        let result = storage.search(&query).await.unwrap();
        assert_eq!(result.total, 1);
        assert_eq!(result.resources[0].id, "patient-2");
    }

    #[tokio::test]
    async fn test_search_contains_filter() {
        use crate::query::{QueryFilter, SearchQuery};

        let storage = InMemoryStorage::new();

        let patients = vec![
            create_test_patient_with_data("patient-1", "Smith", "1990-01-01", true),
            create_test_patient_with_data("patient-2", "Johnson", "1985-05-15", true),
            create_test_patient_with_data("patient-3", "Smithson", "1992-12-30", true),
        ];

        for patient in &patients {
            storage
                .insert(&ResourceType::Patient, patient.clone())
                .await
                .unwrap();
        }

        // Test contains "Smith"
        let query = SearchQuery::new(ResourceType::Patient).with_filter(QueryFilter::Contains {
            field: "name".to_string(),
            value: "Smith".to_string(),
        });

        let result = storage.search(&query).await.unwrap();
        assert_eq!(result.total, 2);
        assert!(result.resources.iter().any(|r| r.id == "patient-1"));
        assert!(result.resources.iter().any(|r| r.id == "patient-3"));

        // Test contains "John"
        let query = SearchQuery::new(ResourceType::Patient).with_filter(QueryFilter::Contains {
            field: "name".to_string(),
            value: "John".to_string(),
        });

        let result = storage.search(&query).await.unwrap();
        assert_eq!(result.total, 1);
        assert_eq!(result.resources[0].id, "patient-2");
    }

    #[tokio::test]
    async fn test_search_identifier_filter() {
        use crate::query::{QueryFilter, SearchQuery};

        let storage = InMemoryStorage::new();

        let patients = vec![
            create_test_patient_with_data("patient-1", "Smith", "1990-01-01", true),
            create_test_patient_with_data("patient-2", "Johnson", "1985-05-15", true),
        ];

        for patient in &patients {
            storage
                .insert(&ResourceType::Patient, patient.clone())
                .await
                .unwrap();
        }

        // Test identifier with system
        let query = SearchQuery::new(ResourceType::Patient).with_filter(QueryFilter::Identifier {
            field: "identifier".to_string(),
            system: Some("http://example.com/mrn".to_string()),
            value: "MRN-patient-1".to_string(),
        });

        let result = storage.search(&query).await.unwrap();
        assert_eq!(result.total, 1);
        assert_eq!(result.resources[0].id, "patient-1");

        // Test identifier without system
        let query = SearchQuery::new(ResourceType::Patient).with_filter(QueryFilter::Identifier {
            field: "identifier".to_string(),
            system: None,
            value: "MRN-patient-2".to_string(),
        });

        let result = storage.search(&query).await.unwrap();
        assert_eq!(result.total, 1);
        assert_eq!(result.resources[0].id, "patient-2");
    }

    #[tokio::test]
    async fn test_search_pagination() {
        use crate::query::SearchQuery;

        let storage = InMemoryStorage::new();

        // Create 25 test patients
        for i in 1..=25 {
            let patient = create_test_patient_with_data(
                &format!("patient-{i:02}"),
                &format!("Patient{i:02}"),
                "1990-01-01",
                true,
            );
            storage
                .insert(&ResourceType::Patient, patient)
                .await
                .unwrap();
        }

        // Test pagination - first page
        let query = SearchQuery::new(ResourceType::Patient).with_pagination(0, 10);

        let result = storage.search(&query).await.unwrap();
        assert_eq!(result.total, 25);
        assert_eq!(result.resources.len(), 10);
        assert_eq!(result.offset, 0);
        assert_eq!(result.count, 10);
        assert!(result.has_more);

        // Test pagination - second page
        let query = SearchQuery::new(ResourceType::Patient).with_pagination(10, 10);

        let result = storage.search(&query).await.unwrap();
        assert_eq!(result.total, 25);
        assert_eq!(result.resources.len(), 10);
        assert_eq!(result.offset, 10);
        assert_eq!(result.count, 10);
        assert!(result.has_more);

        // Test pagination - last page
        let query = SearchQuery::new(ResourceType::Patient).with_pagination(20, 10);

        let result = storage.search(&query).await.unwrap();
        assert_eq!(result.total, 25);
        assert_eq!(result.resources.len(), 5);
        assert_eq!(result.offset, 20);
        assert_eq!(result.count, 10);
        assert!(!result.has_more);

        // Test pagination beyond range
        let query = SearchQuery::new(ResourceType::Patient).with_pagination(30, 10);

        let result = storage.search(&query).await.unwrap();
        assert_eq!(result.total, 25);
        assert_eq!(result.resources.len(), 0);
        assert_eq!(result.offset, 30);
        assert!(!result.has_more);
    }

    #[tokio::test]
    async fn test_search_multiple_filters() {
        use crate::query::{QueryFilter, SearchQuery};

        let storage = InMemoryStorage::new();

        let patients = vec![
            create_test_patient_with_data("patient-1", "Smith", "1990-01-01", true),
            create_test_patient_with_data("patient-2", "Smith", "1985-05-15", false),
            create_test_patient_with_data("patient-3", "Johnson", "1990-01-01", true),
            create_test_patient_with_data("patient-4", "Smith", "1990-01-01", false),
        ];

        for patient in &patients {
            storage
                .insert(&ResourceType::Patient, patient.clone())
                .await
                .unwrap();
        }

        // Test multiple filters (name contains "Smith" AND active = true)
        let query = SearchQuery::new(ResourceType::Patient)
            .with_filter(QueryFilter::Contains {
                field: "name".to_string(),
                value: "Smith".to_string(),
            })
            .with_filter(QueryFilter::Boolean {
                field: "active".to_string(),
                value: true,
            });

        let result = storage.search(&query).await.unwrap();
        assert_eq!(result.total, 1);
        assert_eq!(result.resources[0].id, "patient-1");

        // Test multiple filters (birth date AND active)
        let query = SearchQuery::new(ResourceType::Patient)
            .with_filter(QueryFilter::Exact {
                field: "birthDate".to_string(),
                value: "1990-01-01".to_string(),
            })
            .with_filter(QueryFilter::Boolean {
                field: "active".to_string(),
                value: false,
            });

        let result = storage.search(&query).await.unwrap();
        assert_eq!(result.total, 1);
        assert_eq!(result.resources[0].id, "patient-4");
    }

    #[tokio::test]
    async fn test_search_sorting() {
        use crate::query::SearchQuery;

        let storage = InMemoryStorage::new();

        let patients = vec![
            create_test_patient_with_data("patient-3", "Charlie", "1992-12-30", true),
            create_test_patient_with_data("patient-1", "Alice", "1990-01-01", true),
            create_test_patient_with_data("patient-2", "Bob", "1985-05-15", true),
        ];

        for patient in &patients {
            storage
                .insert(&ResourceType::Patient, patient.clone())
                .await
                .unwrap();
        }

        // Test sort by _id ascending
        let query = SearchQuery::new(ResourceType::Patient).with_sort("_id".to_string(), true);

        let result = storage.search(&query).await.unwrap();
        assert_eq!(result.total, 3);
        assert_eq!(result.resources[0].id, "patient-1");
        assert_eq!(result.resources[1].id, "patient-2");
        assert_eq!(result.resources[2].id, "patient-3");

        // Test sort by _id descending
        let query = SearchQuery::new(ResourceType::Patient).with_sort("_id".to_string(), false);

        let result = storage.search(&query).await.unwrap();
        assert_eq!(result.total, 3);
        assert_eq!(result.resources[0].id, "patient-3");
        assert_eq!(result.resources[1].id, "patient-2");
        assert_eq!(result.resources[2].id, "patient-1");

        // Test sort by birth date
        let query =
            SearchQuery::new(ResourceType::Patient).with_sort("birthDate".to_string(), true);

        let result = storage.search(&query).await.unwrap();
        assert_eq!(result.total, 3);
        assert_eq!(result.resources[0].id, "patient-2"); // 1985
        assert_eq!(result.resources[1].id, "patient-1"); // 1990
        assert_eq!(result.resources[2].id, "patient-3"); // 1992
    }

    #[tokio::test]
    async fn test_search_by_type_helper() {
        use crate::query::QueryFilter;

        let storage = InMemoryStorage::new();

        let patients = vec![
            create_test_patient_with_data("patient-1", "Smith", "1990-01-01", true),
            create_test_patient_with_data("patient-2", "Johnson", "1985-05-15", false),
        ];

        // Add some organizations too
        let org = create_test_resource("org-1", ResourceType::Organization);

        for patient in &patients {
            storage
                .insert(&ResourceType::Patient, patient.clone())
                .await
                .unwrap();
        }
        storage
            .insert(&ResourceType::Organization, org)
            .await
            .unwrap();

        // Use the search_by_type helper
        let filters = vec![QueryFilter::Boolean {
            field: "active".to_string(),
            value: true,
        }];

        let result = storage
            .search_by_type(&ResourceType::Patient, filters, 0, 10)
            .await
            .unwrap();
        assert_eq!(result.total, 1);
        assert_eq!(result.resources[0].id, "patient-1");

        // Test empty result
        let filters = vec![QueryFilter::Exact {
            field: "_id".to_string(),
            value: "nonexistent".to_string(),
        }];

        let result = storage
            .search_by_type(&ResourceType::Patient, filters, 0, 10)
            .await
            .unwrap();
        assert_eq!(result.total, 0);
        assert_eq!(result.resources.len(), 0);
    }

    #[tokio::test]
    async fn test_search_performance_large_dataset() {
        use crate::query::{QueryFilter, SearchQuery};
        use std::time::Instant;

        let storage = InMemoryStorage::new();

        // Create 1000 test patients
        for i in 1..=1000 {
            let patient = create_test_patient_with_data(
                &format!("patient-{i:04}"),
                &format!("Patient{i:04}"),
                "1990-01-01",
                i % 2 == 0, // Every other patient is active
            );
            storage
                .insert(&ResourceType::Patient, patient)
                .await
                .unwrap();
        }

        // Test search performance
        let start = Instant::now();

        let query = SearchQuery::new(ResourceType::Patient)
            .with_filter(QueryFilter::Boolean {
                field: "active".to_string(),
                value: true,
            })
            .with_pagination(0, 50);

        let result = storage.search(&query).await.unwrap();

        let duration = start.elapsed();

        assert_eq!(result.total, 500); // Half are active
        assert_eq!(result.resources.len(), 50);
        assert!(result.has_more);

        // Search should complete quickly (under 100ms for 1000 records)
        assert!(
            duration.as_millis() < 100,
            "Search took too long: {duration:?}"
        );

        println!("Search of 1000 records took: {duration:?}");
    }

    #[tokio::test]
    async fn test_search_resource_type_isolation() {
        use crate::query::SearchQuery;

        let storage = InMemoryStorage::new();

        // Create resources of different types with same ID
        let patient = create_test_resource("resource-1", ResourceType::Patient);
        let org = create_test_resource("resource-1", ResourceType::Organization);
        let practitioner = create_test_resource("resource-1", ResourceType::Practitioner);

        storage
            .insert(&ResourceType::Patient, patient)
            .await
            .unwrap();
        storage
            .insert(&ResourceType::Organization, org)
            .await
            .unwrap();
        storage
            .insert(&ResourceType::Practitioner, practitioner)
            .await
            .unwrap();

        // Search for patients only
        let query = SearchQuery::new(ResourceType::Patient);
        let result = storage.search(&query).await.unwrap();
        assert_eq!(result.total, 1);
        assert_eq!(result.resources[0].resource_type, ResourceType::Patient);

        // Search for organizations only
        let query = SearchQuery::new(ResourceType::Organization);
        let result = storage.search(&query).await.unwrap();
        assert_eq!(result.total, 1);
        assert_eq!(
            result.resources[0].resource_type,
            ResourceType::Organization
        );

        // Search for a type that doesn't exist
        let query = SearchQuery::new(ResourceType::Observation);
        let result = storage.search(&query).await.unwrap();
        assert_eq!(result.total, 0);
    }

    #[tokio::test]
    async fn test_search_edge_cases() {
        use crate::query::{QueryFilter, SearchQuery};

        let storage = InMemoryStorage::new();

        // Empty storage search
        let query = SearchQuery::new(ResourceType::Patient);
        let result = storage.search(&query).await.unwrap();
        assert_eq!(result.total, 0);
        assert_eq!(result.resources.len(), 0);
        assert!(!result.has_more);

        // Add one resource
        let patient = create_test_patient_with_data("patient-1", "Smith", "1990-01-01", true);
        storage
            .insert(&ResourceType::Patient, patient)
            .await
            .unwrap();

        // Search with filter that doesn't match
        let query = SearchQuery::new(ResourceType::Patient).with_filter(QueryFilter::Exact {
            field: "_id".to_string(),
            value: "nonexistent".to_string(),
        });

        let result = storage.search(&query).await.unwrap();
        assert_eq!(result.total, 0);

        // Search with filter for non-existent field
        let query = SearchQuery::new(ResourceType::Patient).with_filter(QueryFilter::Exact {
            field: "nonExistentField".to_string(),
            value: "value".to_string(),
        });

        let result = storage.search(&query).await.unwrap();
        assert_eq!(result.total, 0);

        // Search with very large pagination offset
        let query = SearchQuery::new(ResourceType::Patient).with_pagination(1000, 10);

        let result = storage.search(&query).await.unwrap();
        assert_eq!(result.total, 1);
        assert_eq!(result.resources.len(), 0);
        assert_eq!(result.offset, 1000);
        assert!(!result.has_more);
    }
}
