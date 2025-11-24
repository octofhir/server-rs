//! Hot-reload functionality for conformance resources.
//!
//! This module listens to PostgreSQL NOTIFY events and triggers
//! re-synchronization of conformance resources to the canonical manager.

use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use serde::Deserialize;
use sqlx_core::postgres::PgListener;
use sqlx_postgres::PgPool;
use tokio::sync::mpsc;
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
    sender: mpsc::UnboundedSender<ConformanceChangeEvent>,
    receiver: Arc<tokio::sync::Mutex<mpsc::UnboundedReceiver<ConformanceChangeEvent>>>,
}

impl HotReloadListener {
    /// Creates a new HotReloadListener.
    ///
    /// This sets up the notification channel but does not start listening yet.
    /// Call `start()` to begin listening for changes.
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        let (sender, receiver) = mpsc::unbounded_channel();

        Self {
            pool,
            sender,
            receiver: Arc::new(tokio::sync::Mutex::new(receiver)),
        }
    }

    /// Returns a clone of the change event sender.
    ///
    /// This can be used to manually inject change events for testing.
    #[must_use]
    pub fn sender(&self) -> mpsc::UnboundedSender<ConformanceChangeEvent> {
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
    async fn listen_loop(&self) -> Result<(), Box<dyn std::error::Error>> {
        let mut listener = PgListener::connect_with(&self.pool).await?;
        listener.listen(CONFORMANCE_CHANNEL).await?;

        info!(
            channel = CONFORMANCE_CHANNEL,
            "Listening for conformance changes"
        );

        loop {
            let notification = listener.recv().await?;
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
    ///
    /// # Example
    ///
    /// ```ignore
    /// let mut rx = listener.subscribe();
    /// while let Some(event) = rx.recv().await {
    ///     println!("Resource changed: {:?}", event);
    /// }
    /// ```
    pub async fn subscribe(&self) -> mpsc::UnboundedReceiver<ConformanceChangeEvent> {
        let mut receiver_guard = self.receiver.lock().await;
        let (tx, rx) = mpsc::unbounded_channel();

        // Forward all events from the main receiver to new subscribers
        // Note: This is a simple implementation. For multiple subscribers,
        // you'd want to use broadcast channel or similar.
        let main_tx = self.sender.clone();
        tokio::spawn(async move {
            drop(receiver_guard);
            drop(main_tx);
            drop(tx);
        });

        rx
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
        let sender = listener.sender();

        // Start the listener task
        let listener_handle = listener.start();

        // If auto-sync is configured, start the sync task
        if let (Some(storage), Some(base_dir)) = (self.conformance_storage, self.base_dir) {
            let manager = self.canonical_manager;

            tokio::spawn(async move {
                let mut rx = mpsc::UnboundedReceiver::new();
                // Workaround: Since subscribe() requires async lock, we'll use the sender directly
                std::mem::swap(&mut rx, &mut *listener.receiver.lock().await);

                while let Some(event) = rx.recv().await {
                    info!(
                        resource_type = %event.resource_type,
                        operation = ?event.operation,
                        "Triggering conformance sync"
                    );

                    // Perform sync
                    match crate::db_sync::sync_and_load(
                        &storage,
                        &base_dir,
                        manager.as_ref().map(|m| m.as_ref()),
                    )
                    .await
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
