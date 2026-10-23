//! EventedStorage - A storage wrapper that emits events after CRUD operations.
//!
//! This wrapper delegates all operations to an inner storage implementation
//! while emitting events to a broadcaster after successful operations.
//!
//! # Example
//!
//! ```ignore
//! use octofhir_storage::EventedStorage;
//! use octofhir_core::events::EventBroadcaster;
//!
//! let broadcaster = EventBroadcaster::new_shared();
//! let storage = EventedStorage::new(postgres_storage, broadcaster);
//!
//! // After this, an event will be emitted to the broadcaster
//! storage.create(&patient_json).await?;
//! ```

use std::sync::Arc;

use async_trait::async_trait;
use octofhir_core::events::{EventBroadcaster, ResourceEvent, ResourceEventType};
use serde_json::Value;
use tracing::trace;

use crate::error::StorageError;
use crate::traits::{FhirStorage, Transaction};
use crate::types::{
    HistoryParams, HistoryResult, IncludeLimits, ProposedResource, RawHistoryResult,
    RawSearchResult, RawStoredResource, SearchParams, SearchResult, StoredResource,
};

fn snapshot_event(
    event_type: ResourceEventType,
    snapshot: &StoredResource,
    previous: Option<&StoredResource>,
) -> ResourceEvent {
    let mut event = ResourceEvent::new(
        event_type,
        &snapshot.resource_type,
        &snapshot.id,
        Some(Arc::new(snapshot.resource.clone())),
    );
    event.version_id = snapshot.version_id.parse().ok();
    event.timestamp = snapshot.last_updated;
    event.previous_resource = previous.map(|row| Arc::new(row.resource.clone()));
    event
}

fn raw_snapshot_event(
    event_type: ResourceEventType,
    snapshot: &RawStoredResource,
    previous: Option<&StoredResource>,
) -> Result<ResourceEvent, StorageError> {
    let resource = serde_json::from_str(&snapshot.resource_json)
        .map_err(|error| StorageError::internal(format!("Invalid stored event JSON: {error}")))?;
    let mut event = ResourceEvent::new(
        event_type,
        &snapshot.resource_type,
        &snapshot.id,
        Some(resource),
    );
    event.version_id = snapshot.version_id.parse().ok();
    event.timestamp = snapshot.last_updated;
    event.previous_resource = previous.map(|row| Arc::new(row.resource.clone()));
    Ok(event)
}

/// A storage wrapper that emits events after successful CRUD operations.
///
/// This wrapper implements `FhirStorage` by delegating to an inner implementation
/// while emitting `ResourceEvent`s to a broadcaster after each successful operation.
///
/// Events are emitted **after** the operation succeeds, ensuring that events
/// only correspond to actual changes in the database.
pub struct EventedStorage<S: FhirStorage> {
    /// The inner storage implementation.
    inner: S,
    /// The event broadcaster.
    broadcaster: Arc<EventBroadcaster>,
}

impl<S: FhirStorage> EventedStorage<S> {
    /// Create a new evented storage wrapper.
    pub fn new(inner: S, broadcaster: Arc<EventBroadcaster>) -> Self {
        Self { inner, broadcaster }
    }

    /// Get a reference to the inner storage.
    pub fn inner(&self) -> &S {
        &self.inner
    }

    /// Get a reference to the broadcaster.
    pub fn broadcaster(&self) -> &Arc<EventBroadcaster> {
        &self.broadcaster
    }
}

#[async_trait]
impl<S: FhirStorage> FhirStorage for EventedStorage<S> {
    async fn create(&self, resource: &Value) -> Result<StoredResource, StorageError> {
        let result = self.inner.create(resource).await?;
        self.broadcaster
            .send_resource(snapshot_event(ResourceEventType::Created, &result, None));
        Ok(result)
    }

    async fn create_raw(&self, resource: &Value) -> Result<RawStoredResource, StorageError> {
        let result = self.inner.create_raw(resource).await?;
        if self.broadcaster.subscriber_count() > 0 {
            self.broadcaster.send_resource(raw_snapshot_event(
                ResourceEventType::Created,
                &result,
                None,
            )?);
        }
        Ok(result)
    }

    async fn read(
        &self,
        resource_type: &str,
        id: &str,
    ) -> Result<Option<StoredResource>, StorageError> {
        // Read operations don't emit events
        self.inner.read(resource_type, id).await
    }

    async fn exists(&self, resource_type: &str, id: &str) -> Result<bool, StorageError> {
        self.inner.exists(resource_type, id).await
    }

    async fn exists_many(
        &self,
        resource_type: &str,
        ids: &[String],
    ) -> Result<std::collections::HashSet<String>, StorageError> {
        self.inner.exists_many(resource_type, ids).await
    }

    async fn exists_many_grouped(
        &self,
        groups: &[(String, Vec<String>)],
    ) -> Result<std::collections::HashSet<String>, StorageError> {
        self.inner.exists_many_grouped(groups).await
    }

    async fn profiles_many_grouped(
        &self,
        groups: &[(String, Vec<String>)],
    ) -> Result<std::collections::HashMap<String, Vec<String>>, StorageError> {
        self.inner.profiles_many_grouped(groups).await
    }

    async fn read_many(
        &self,
        resource_type: &str,
        ids: &[String],
    ) -> Result<Vec<StoredResource>, StorageError> {
        self.inner.read_many(resource_type, ids).await
    }

    async fn read_raw(
        &self,
        resource_type: &str,
        id: &str,
    ) -> Result<Option<crate::RawStoredResource>, StorageError> {
        self.inner.read_raw(resource_type, id).await
    }

    async fn update(
        &self,
        resource: &Value,
        if_match: Option<&str>,
    ) -> Result<StoredResource, StorageError> {
        let mut tx = self.begin_transaction().await?;
        let result = tx.update(resource, if_match).await?;
        tx.commit().await?;
        Ok(result)
    }

    async fn update_raw(
        &self,
        resource: &Value,
        if_match: Option<&str>,
    ) -> Result<RawStoredResource, StorageError> {
        let mut tx = self.begin_transaction().await?;
        let result = tx.update_raw(resource, if_match).await?;
        tx.commit().await?;
        Ok(result)
    }

    async fn delete(&self, resource_type: &str, id: &str) -> Result<(), StorageError> {
        let mut tx = self.begin_transaction().await?;
        tx.delete(resource_type, id).await?;
        tx.commit().await
    }

    async fn current_snapshot(
        &self,
        resource_type: &str,
        id: &str,
    ) -> Result<Option<crate::RawHistoryEntry>, StorageError> {
        self.inner.current_snapshot(resource_type, id).await
    }

    async fn version_snapshot(
        &self,
        resource_type: &str,
        id: &str,
        version: &str,
    ) -> Result<Option<crate::RawHistoryEntry>, StorageError> {
        self.inner
            .version_snapshot(resource_type, id, version)
            .await
    }

    async fn vread(
        &self,
        resource_type: &str,
        id: &str,
        version: &str,
    ) -> Result<Option<StoredResource>, StorageError> {
        // Read operations don't emit events
        self.inner.vread(resource_type, id, version).await
    }

    async fn history(
        &self,
        resource_type: &str,
        id: Option<&str>,
        params: &HistoryParams,
    ) -> Result<HistoryResult, StorageError> {
        // Read operations don't emit events
        self.inner.history(resource_type, id, params).await
    }

    async fn system_history(&self, params: &HistoryParams) -> Result<HistoryResult, StorageError> {
        self.inner.system_history(params).await
    }

    async fn history_raw(
        &self,
        resource_type: &str,
        id: Option<&str>,
        params: &HistoryParams,
    ) -> Result<RawHistoryResult, StorageError> {
        self.inner.history_raw(resource_type, id, params).await
    }

    async fn system_history_raw(
        &self,
        params: &HistoryParams,
    ) -> Result<RawHistoryResult, StorageError> {
        self.inner.system_history_raw(params).await
    }

    async fn search(
        &self,
        resource_type: &str,
        params: &SearchParams,
    ) -> Result<SearchResult, StorageError> {
        // Read operations don't emit events
        self.inner.search(resource_type, params).await
    }

    async fn begin_transaction(&self) -> Result<Box<dyn Transaction>, StorageError> {
        // Transactions are delegated to inner storage
        // Events will be emitted by EventedTransaction wrapper
        let inner_tx = self.inner.begin_transaction().await?;
        Ok(Box::new(EventedTransaction::new(
            inner_tx,
            self.broadcaster.clone(),
        )))
    }

    fn supports_transactions(&self) -> bool {
        self.inner.supports_transactions()
    }

    fn backend_name(&self) -> &'static str {
        self.inner.backend_name()
    }
}

impl<S: FhirStorage> std::fmt::Debug for EventedStorage<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EventedStorage")
            .field("backend", &self.inner.backend_name())
            .field("subscriber_count", &self.broadcaster.subscriber_count())
            .finish()
    }
}

// ============================================================================
// EventedTransaction
// ============================================================================

/// A transaction wrapper that collects events and emits them on commit.
///
/// Events are collected during the transaction and only emitted when the
/// transaction successfully commits. This ensures consistency between
/// the database state and emitted events.
pub struct EventedTransaction {
    inner: Box<dyn Transaction>,
    broadcaster: Arc<EventBroadcaster>,
    /// Pending events to emit on commit.
    pending_events: Vec<ResourceEvent>,
}

impl EventedTransaction {
    /// Create a new evented transaction.
    pub fn new(inner: Box<dyn Transaction>, broadcaster: Arc<EventBroadcaster>) -> Self {
        Self {
            inner,
            broadcaster,
            pending_events: Vec::new(),
        }
    }

    /// Queue an event to be emitted on commit.
    fn queue_event(&mut self, event: ResourceEvent) {
        self.pending_events.push(event);
    }
}

#[async_trait]
impl Transaction for EventedTransaction {
    async fn current_snapshot(
        &self,
        resource_type: &str,
        id: &str,
    ) -> Result<Option<crate::RawHistoryEntry>, StorageError> {
        self.inner.current_snapshot(resource_type, id).await
    }

    async fn version_snapshot(
        &self,
        resource_type: &str,
        id: &str,
        version: &str,
    ) -> Result<Option<crate::RawHistoryEntry>, StorageError> {
        self.inner
            .version_snapshot(resource_type, id, version)
            .await
    }

    async fn vread(
        &self,
        resource_type: &str,
        id: &str,
        version: &str,
    ) -> Result<Option<StoredResource>, StorageError> {
        self.inner.vread(resource_type, id, version).await
    }

    async fn save_async_job_checkpoint(
        &mut self,
        job_id: uuid::Uuid,
        execution_id: uuid::Uuid,
        key: Option<&str>,
        result: &Value,
    ) -> Result<(), StorageError> {
        self.inner
            .save_async_job_checkpoint(job_id, execution_id, key, result)
            .await
    }

    async fn commit(self: Box<Self>) -> Result<(), StorageError> {
        // Destructure self to get ownership of all fields
        let EventedTransaction {
            inner,
            broadcaster,
            pending_events,
        } = *self;

        // First commit the transaction
        inner.commit().await?;

        // Then emit all pending events (only on successful commit)
        let event_count = pending_events.len();
        for event in pending_events {
            broadcaster.send_resource(event);
        }
        trace!(count = event_count, "Emitted pending transaction events");

        Ok(())
    }

    async fn rollback(self: Box<Self>) -> Result<(), StorageError> {
        // On rollback, don't emit any events
        self.inner.rollback().await
    }

    async fn lock_conditional_writes(&mut self) -> Result<(), StorageError> {
        self.inner.lock_conditional_writes().await
    }

    async fn create(&mut self, resource: &Value) -> Result<StoredResource, StorageError> {
        let result = self.inner.create(resource).await?;
        self.queue_event(snapshot_event(ResourceEventType::Created, &result, None));
        Ok(result)
    }

    async fn create_raw(&mut self, resource: &Value) -> Result<RawStoredResource, StorageError> {
        let result = self.inner.create_raw(resource).await?;
        self.queue_event(raw_snapshot_event(
            ResourceEventType::Created,
            &result,
            None,
        )?);
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
    ) -> Result<crate::UpdatedResource, StorageError> {
        let result = self.inner.update_with_snapshot(resource, if_match).await?;
        self.queue_event(snapshot_event(
            ResourceEventType::Updated,
            &result.stored,
            Some(&result.previous),
        ));
        Ok(result)
    }

    async fn update_raw(
        &mut self,
        resource: &Value,
        if_match: Option<&str>,
    ) -> Result<RawStoredResource, StorageError> {
        let result = self.update_with_snapshot(resource, if_match).await?.stored;
        Ok(RawStoredResource {
            resource_json: result.resource.to_string(),
            id: result.id,
            version_id: result.version_id,
            resource_type: result.resource_type,
            last_updated: result.last_updated,
            created_at: result.created_at,
        })
    }

    async fn delete(&mut self, resource_type: &str, id: &str) -> Result<(), StorageError> {
        self.delete_batch(resource_type, &[id.to_owned()]).await?;
        Ok(())
    }

    async fn read(
        &self,
        resource_type: &str,
        id: &str,
    ) -> Result<Option<StoredResource>, StorageError> {
        self.inner.read(resource_type, id).await
    }

    async fn exists_many_grouped(
        &mut self,
        groups: &[(String, Vec<String>)],
    ) -> Result<std::collections::HashSet<String>, StorageError> {
        self.inner.exists_many_grouped(groups).await
    }

    async fn read_for_update(
        &self,
        resource_type: &str,
        id: &str,
    ) -> Result<Option<StoredResource>, StorageError> {
        self.inner.read_for_update(resource_type, id).await
    }

    async fn read_many_for_update(
        &self,
        resource_type: &str,
        ids: &[String],
    ) -> Result<Vec<StoredResource>, StorageError> {
        self.inner.read_many_for_update(resource_type, ids).await
    }

    async fn lock_references(
        &mut self,
        groups: &[(String, Vec<String>)],
    ) -> Result<std::collections::HashSet<String>, StorageError> {
        self.inner.lock_references(groups).await
    }

    async fn lock_reference_snapshot(
        &mut self,
        reference: &octofhir_core::FhirReference,
    ) -> Result<Option<Value>, StorageError> {
        self.inner.lock_reference_snapshot(reference).await
    }

    async fn lock_reference_versions(
        &mut self,
        references: &[octofhir_core::FhirReference],
    ) -> Result<std::collections::HashSet<String>, StorageError> {
        self.inner.lock_reference_versions(references).await
    }

    async fn lock_reference_snapshots(
        &mut self,
        references: &[octofhir_core::FhirReference],
    ) -> Result<std::collections::HashMap<String, Value>, StorageError> {
        self.inner.lock_reference_snapshots(references).await
    }

    async fn search(
        &self,
        resource_type: &str,
        params: &SearchParams,
    ) -> Result<SearchResult, StorageError> {
        self.inner.search(resource_type, params).await
    }

    async fn search_system_raw(
        &self,
        resource_types: &[&str],
        params: &SearchParams,
        include_limits: IncludeLimits,
        strict: bool,
    ) -> Result<RawSearchResult, StorageError> {
        self.inner
            .search_system_raw(resource_types, params, include_limits, strict)
            .await
    }

    async fn search_system_raw_authorized(
        &self,
        resource_types: &[&str],
        params: &SearchParams,
        include_limits: IncludeLimits,
        strict: bool,
        authorization: crate::RawSearchAuthorization<'_>,
    ) -> Result<RawSearchResult, StorageError> {
        self.inner
            .search_system_raw_authorized(
                resource_types,
                params,
                include_limits,
                strict,
                authorization,
            )
            .await
    }

    async fn search_conditional_batch(
        &self,
        predicates: &[(String, SearchParams)],
    ) -> Result<Vec<Vec<StoredResource>>, StorageError> {
        self.inner.search_conditional_batch(predicates).await
    }

    async fn reserve_write_metadata(
        &mut self,
        planned: Option<&crate::TransactionWriteMetadata>,
    ) -> Result<crate::TransactionWriteMetadata, StorageError> {
        self.inner.reserve_write_metadata(planned).await
    }

    async fn search_proposed_conditional_batch(
        &self,
        predicates: &[(String, SearchParams)],
        proposed: &[ProposedResource],
    ) -> Result<Vec<Vec<StoredResource>>, StorageError> {
        self.inner
            .search_proposed_conditional_batch(predicates, proposed)
            .await
    }

    async fn search_proposed_conditional_candidates(
        &self,
        predicates: &[(String, SearchParams)],
        proposed: &[ProposedResource],
        max_candidates: usize,
    ) -> Result<Vec<Vec<String>>, StorageError> {
        self.inner
            .search_proposed_conditional_candidates(predicates, proposed, max_candidates)
            .await
    }

    async fn create_batch(
        &mut self,
        resource_type: &str,
        resources: &[Value],
    ) -> Result<Vec<StoredResource>, StorageError> {
        let stored = self.inner.create_batch(resource_type, resources).await?;
        for snapshot in &stored {
            self.queue_event(snapshot_event(ResourceEventType::Created, snapshot, None));
        }
        Ok(stored)
    }

    async fn update_batch(
        &mut self,
        resource_type: &str,
        resources: &[Value],
    ) -> Result<crate::traits::BatchUpdateOutcome, StorageError> {
        let outcome = self.inner.update_batch(resource_type, resources).await?;
        let previous: std::collections::HashMap<_, _> = outcome
            .previous
            .iter()
            .map(|snapshot| (snapshot.id.as_str(), snapshot))
            .collect();
        for snapshot in &outcome.updated {
            let previous = previous.get(snapshot.id.as_str()).copied().ok_or_else(|| {
                StorageError::internal("Batch update lacks its preceding snapshot")
            })?;
            self.queue_event(snapshot_event(
                ResourceEventType::Updated,
                snapshot,
                Some(previous),
            ));
        }
        Ok(outcome)
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
    ) -> Result<Vec<crate::DeletedResource>, StorageError> {
        let deleted = self
            .inner
            .delete_batch_with_snapshots(resource_type, ids)
            .await?;
        for snapshot in &deleted {
            let mut event = ResourceEvent::deleted(resource_type, &snapshot.previous.id)
                .with_previous(snapshot.previous.resource.clone())
                .with_version(
                    snapshot
                        .version_id
                        .parse()
                        .map_err(|_| StorageError::internal("Invalid deletion version"))?,
                );
            event.timestamp = snapshot.last_updated;
            self.queue_event(event);
        }
        Ok(deleted)
    }
}

impl std::fmt::Debug for EventedTransaction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EventedTransaction")
            .field("pending_events", &self.pending_events.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    // Note: Full tests require a mock storage implementation
    // which would be added in a separate test module

    use super::*;

    #[test]
    fn test_resource_event_created() {
        let event = ResourceEvent::created("Patient", "123", serde_json::json!({"id": "123"}));
        assert_eq!(event.resource_type, "Patient");
        assert_eq!(event.resource_id, "123");
    }
}
