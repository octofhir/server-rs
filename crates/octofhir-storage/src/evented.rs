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
use octofhir_core::events::{EventBroadcaster, ResourceEvent};
use serde_json::Value;
use tracing::debug;

use crate::error::StorageError;
use crate::traits::{FhirStorage, Transaction};
use crate::types::{HistoryParams, HistoryResult, SearchParams, SearchResult, StoredResource};

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

    fn emit_created(&self, resource_type: &str, resource_id: &str, resource: &Value) {
        if self.broadcaster.subscriber_count() == 0 {
            return;
        }
        let count = self
            .broadcaster
            .send_created(resource_type, resource_id, resource.clone());
        debug!(
            resource_type = %resource_type,
            resource_id = %resource_id,
            subscribers = count,
            "Emitted ResourceCreated event"
        );
    }

    fn emit_updated(&self, resource_type: &str, resource_id: &str, resource: &Value) {
        if self.broadcaster.subscriber_count() == 0 {
            return;
        }
        let count = self
            .broadcaster
            .send_updated(resource_type, resource_id, resource.clone());
        debug!(
            resource_type = %resource_type,
            resource_id = %resource_id,
            subscribers = count,
            "Emitted ResourceUpdated event"
        );
    }

    fn emit_deleted(&self, resource_type: &str, resource_id: &str) {
        if self.broadcaster.subscriber_count() == 0 {
            return;
        }
        let count = self.broadcaster.send_deleted(resource_type, resource_id);
        debug!(
            resource_type = %resource_type,
            resource_id = %resource_id,
            subscribers = count,
            "Emitted ResourceDeleted event"
        );
    }
}

#[async_trait]
impl<S: FhirStorage> FhirStorage for EventedStorage<S> {
    async fn create(&self, resource: &Value) -> Result<StoredResource, StorageError> {
        let result = self.inner.create(resource).await?;

        // Emit event after successful create
        let resource_type = result
            .resource
            .get("resourceType")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown");
        self.emit_created(resource_type, &result.id, &result.resource);

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

    async fn update(
        &self,
        resource: &Value,
        if_match: Option<&str>,
    ) -> Result<StoredResource, StorageError> {
        let result = self.inner.update(resource, if_match).await?;

        // Emit event after successful update
        let resource_type = result
            .resource
            .get("resourceType")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown");
        self.emit_updated(resource_type, &result.id, &result.resource);

        Ok(result)
    }

    async fn delete(&self, resource_type: &str, id: &str) -> Result<(), StorageError> {
        self.inner.delete(resource_type, id).await?;

        // Emit event after successful delete
        self.emit_deleted(resource_type, id);

        Ok(())
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
        // Read operations don't emit events
        self.inner.system_history(params).await
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
        debug!(count = event_count, "Emitted pending transaction events");

        Ok(())
    }

    async fn rollback(self: Box<Self>) -> Result<(), StorageError> {
        // On rollback, don't emit any events
        self.inner.rollback().await
    }

    async fn create(&mut self, resource: &Value) -> Result<StoredResource, StorageError> {
        let result = self.inner.create(resource).await?;

        // Queue event for emission on commit
        let resource_type = result
            .resource
            .get("resourceType")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown")
            .to_string();
        self.queue_event(ResourceEvent::created(
            resource_type,
            &result.id,
            result.resource.clone(),
        ));

        Ok(result)
    }

    async fn update(&mut self, resource: &Value) -> Result<StoredResource, StorageError> {
        let result = self.inner.update(resource).await?;

        // Queue event for emission on commit
        let resource_type = result
            .resource
            .get("resourceType")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown")
            .to_string();
        self.queue_event(ResourceEvent::updated(
            resource_type,
            &result.id,
            result.resource.clone(),
        ));

        Ok(result)
    }

    async fn delete(&mut self, resource_type: &str, id: &str) -> Result<(), StorageError> {
        self.inner.delete(resource_type, id).await?;

        // Queue event for emission on commit
        self.queue_event(ResourceEvent::deleted(resource_type, id));

        Ok(())
    }

    async fn read(
        &self,
        resource_type: &str,
        id: &str,
    ) -> Result<Option<StoredResource>, StorageError> {
        self.inner.read(resource_type, id).await
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
