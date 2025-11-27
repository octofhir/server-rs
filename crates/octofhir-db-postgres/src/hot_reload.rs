//! Hot-reload functionality for conformance resources.
//!
//! This module listens to PostgreSQL NOTIFY events and triggers
//! re-synchronization of conformance resources to the canonical manager.

use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use sqlx_postgres::{PgListener, PgPool};
use tokio::sync::broadcast;
use tokio::time::sleep;
use tracing::{debug, error, info, instrument, warn};

use octofhir_storage::{ConformanceChangeEvent, ConformanceChangeOp};

/// Channel for conformance change notifications.
const CONFORMANCE_CHANNEL: &str = "octofhir_conformance_changes";

/// Payload from PostgreSQL NOTIFY trigger.
#[derive(Debug, Deserialize)]
struct NotifyPayload {
    table: String,
    operation: String,
    id: String,
    url: String,
}

impl NotifyPayload {
    /// Converts PostgreSQL operation name to ConformanceChangeOp.
    fn to_change_op(&self) -> ConformanceChangeOp {
        match self.operation.to_uppercase().as_str() {
            "INSERT" => ConformanceChangeOp::Insert,
            "UPDATE" => ConformanceChangeOp::Update,
            "DELETE" => ConformanceChangeOp::Delete,
            _ => {
                warn!(operation = %self.operation, "Unknown operation type, treating as Update");
                ConformanceChangeOp::Update
            }
        }
    }

    /// Converts table name to resource type.
    fn to_resource_type(&self) -> String {
        match self.table.as_str() {
            "structuredefinition" => "StructureDefinition".to_string(),
            "valueset" => "ValueSet".to_string(),
            "codesystem" => "CodeSystem".to_string(),
            "searchparameter" => "SearchParameter".to_string(),
            other => {
                warn!(table = %other, "Unknown table name");
                other.to_string()
            }
        }
    }

    /// Converts to ConformanceChangeEvent.
    fn to_event(&self) -> ConformanceChangeEvent {
        ConformanceChangeEvent {
            resource_type: self.to_resource_type(),
            operation: self.to_change_op(),
            id: self.id.clone(),
            url: Some(self.url.clone()),
        }
    }
}

/// Hot-reload listener that monitors conformance resource changes.
///
/// This struct manages a PostgreSQL LISTEN connection and forwards
/// change notifications to subscribers.
pub struct HotReloadListener {
    pool: PgPool,
    sender: broadcast::Sender<ConformanceChangeEvent>,
}

impl HotReloadListener {
    /// Creates a new HotReloadListener.
    ///
    /// This sets up the notification channel but does not start listening yet.
    /// Call `start()` to begin listening for changes.
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        // Create broadcast channel with capacity of 100 events
        let (sender, _) = broadcast::channel(100);

        Self { pool, sender }
    }

    /// Returns a clone of the change event sender.
    ///
    /// This can be used to manually inject change events for testing.
    #[must_use]
    pub fn sender(&self) -> broadcast::Sender<ConformanceChangeEvent> {
        self.sender.clone()
    }

    /// Starts the hot-reload listener in the background.
    ///
    /// This spawns a tokio task that:
    /// 1. Connects to PostgreSQL with LISTEN
    /// 2. Receives NOTIFY events
    /// 3. Forwards them to the change event channel
    ///
    /// The task runs until the sender is dropped or an unrecoverable error occurs.
    ///
    /// # Returns
    ///
    /// A handle to the spawned task.
    #[instrument(skip(self))]
    pub fn start(self: Arc<Self>) -> tokio::task::JoinHandle<()> {
        info!("Starting hot-reload listener for conformance resources");

        tokio::spawn(async move {
            loop {
                match self.listen_loop().await {
                    Ok(()) => {
                        info!("Hot-reload listener stopped gracefully");
                        break;
                    }
                    Err(e) => {
                        error!(error = %e, "Hot-reload listener error, reconnecting in 5s");
                        sleep(Duration::from_secs(5)).await;
                    }
                }
            }
        })
    }

    /// Main listen loop that connects and receives notifications.
    async fn listen_loop(&self) -> Result<(), Box<dyn std::error::Error + Send>> {
        let mut listener = PgListener::connect_with(&self.pool)
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send>)?;
        listener
            .listen(CONFORMANCE_CHANNEL)
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send>)?;

        info!(
            channel = CONFORMANCE_CHANNEL,
            "Listening for conformance changes"
        );

        loop {
            let notification = listener
                .recv()
                .await
                .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send>)?;
            let payload = notification.payload();

            debug!(payload = %payload, "Received NOTIFY");

            match serde_json::from_str::<NotifyPayload>(payload) {
                Ok(parsed) => {
                    let event = parsed.to_event();
                    info!(
                        resource_type = %event.resource_type,
                        operation = ?event.operation,
                        id = %event.id,
                        url = ?event.url,
                        "Conformance resource changed"
                    );

                    if self.sender.send(event).is_err() {
                        warn!("Failed to send change event - channel closed");
                        return Ok(());
                    }
                }
                Err(e) => {
                    warn!(
                        error = %e,
                        payload = %payload,
                        "Failed to parse NOTIFY payload"
                    );
                }
            }
        }
    }

    /// Subscribes to conformance change events.
    ///
    /// Returns a receiver that yields `ConformanceChangeEvent` as they occur.
    /// Multiple subscribers can call this method to receive independent copies
    /// of all events.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let mut rx = listener.subscribe();
    /// while let Ok(event) = rx.recv().await {
    ///     println!("Resource changed: {:?}", event);
    /// }
    /// ```
    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<ConformanceChangeEvent> {
        self.sender.subscribe()
    }
}

/// Builder for hot-reload with automatic sync.
///
/// This provides a convenient way to set up hot-reload with automatic
/// synchronization to canonical manager.
pub struct HotReloadBuilder {
    pool: PgPool,
    conformance_storage: Option<Arc<crate::conformance::PostgresConformanceStorage>>,
    canonical_manager: Option<Arc<octofhir_canonical_manager::CanonicalManager>>,
    base_dir: Option<std::path::PathBuf>,
}

impl HotReloadBuilder {
    /// Creates a new builder with the given PostgreSQL pool.
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            conformance_storage: None,
            canonical_manager: None,
            base_dir: None,
        }
    }

    /// Sets the conformance storage for loading resources.
    #[must_use]
    pub fn with_conformance_storage(
        mut self,
        storage: Arc<crate::conformance::PostgresConformanceStorage>,
    ) -> Self {
        self.conformance_storage = Some(storage);
        self
    }

    /// Sets the canonical manager for syncing resources.
    #[must_use]
    pub fn with_canonical_manager(
        mut self,
        manager: Arc<octofhir_canonical_manager::CanonicalManager>,
    ) -> Self {
        self.canonical_manager = Some(manager);
        self
    }

    /// Sets the base directory for package sync.
    #[must_use]
    pub fn with_base_dir(mut self, base_dir: std::path::PathBuf) -> Self {
        self.base_dir = Some(base_dir);
        self
    }

    /// Starts the hot-reload listener with automatic synchronization.
    ///
    /// When conformance resources change, this will automatically:
    /// 1. Sync the resources to the filesystem
    /// 2. Reload them into the canonical manager
    ///
    /// Returns a handle to the spawned task.
    pub fn start(self) -> Result<tokio::task::JoinHandle<()>, Box<dyn std::error::Error>> {
        let listener = Arc::new(HotReloadListener::new(self.pool.clone()));

        // If auto-sync is configured, subscribe before starting
        let receiver = if self.conformance_storage.is_some() && self.base_dir.is_some() {
            Some(listener.subscribe())
        } else {
            None
        };

        // Start the listener task
        let listener_handle = listener.start();

        // If auto-sync is configured, start the sync task
        if let (Some(storage), Some(base_dir), Some(mut rx)) =
            (self.conformance_storage, self.base_dir, receiver)
        {
            let manager = self.canonical_manager;

            tokio::spawn(async move {
                while let Ok(event) = rx.recv().await {
                    info!(
                        resource_type = %event.resource_type,
                        operation = ?event.operation,
                        "Triggering conformance sync"
                    );

                    // Perform sync
                    match crate::db_sync::sync_and_load(&storage, &base_dir, manager.as_ref()).await
                    {
                        Ok(path) => {
                            info!(
                                path = %path.display(),
                                "Conformance resources reloaded successfully"
                            );
                        }
                        Err(e) => {
                            error!(error = %e, "Failed to reload conformance resources");
                        }
                    }
                }
            });
        }

        Ok(listener_handle)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_notify_payload_to_change_op() {
        let payload = NotifyPayload {
            table: "structuredefinition".to_string(),
            operation: "INSERT".to_string(),
            id: "test-id".to_string(),
            url: "http://example.org/SD/Test".to_string(),
        };

        assert_eq!(payload.to_change_op(), ConformanceChangeOp::Insert);
    }

    #[test]
    fn test_notify_payload_to_resource_type() {
        let payload = NotifyPayload {
            table: "valueset".to_string(),
            operation: "UPDATE".to_string(),
            id: "test-id".to_string(),
            url: "http://example.org/VS/Test".to_string(),
        };

        assert_eq!(payload.to_resource_type(), "ValueSet");
    }
}
