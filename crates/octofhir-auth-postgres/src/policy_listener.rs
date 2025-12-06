//! PostgreSQL LISTEN/NOTIFY listener for access policy changes.
//!
//! This module listens to PostgreSQL NOTIFY events for policy changes
//! and forwards them to subscribers for cache invalidation.
//!
//! # Example
//!
//! ```ignore
//! use octofhir_auth_postgres::PolicyListener;
//! use std::sync::Arc;
//!
//! let listener = Arc::new(PolicyListener::new(pool));
//! let mut rx = listener.subscribe();
//!
//! // Start listening in background
//! listener.clone().start();
//!
//! // Handle events
//! while let Ok(event) = rx.recv().await {
//!     println!("Policy changed: {:?}", event);
//! }
//! ```

use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use sqlx_postgres::PgListener;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tokio::time::sleep;
use tracing::{debug, error, info, instrument, warn};

use crate::PgPool;

// =============================================================================
// Constants
// =============================================================================

/// PostgreSQL channel for policy change notifications.
const POLICY_CHANNEL: &str = "octofhir_policy_changes";

/// Reconnection delay on error.
const RECONNECT_DELAY: Duration = Duration::from_secs(5);

/// Broadcast channel capacity.
const CHANNEL_CAPACITY: usize = 100;

// =============================================================================
// Types
// =============================================================================

/// Payload from PostgreSQL NOTIFY trigger.
#[derive(Debug, Deserialize)]
struct NotifyPayload {
    /// Database operation (INSERT, UPDATE, DELETE).
    operation: String,
    /// Policy resource ID.
    id: String,
}

impl NotifyPayload {
    /// Converts PostgreSQL operation name to `PolicyChangeOp`.
    fn to_change_op(&self) -> PolicyChangeOp {
        match self.operation.to_uppercase().as_str() {
            "INSERT" => PolicyChangeOp::Insert,
            "UPDATE" => PolicyChangeOp::Update,
            "DELETE" => PolicyChangeOp::Delete,
            other => {
                warn!(operation = %other, "Unknown operation type, treating as Update");
                PolicyChangeOp::Update
            }
        }
    }

    /// Converts to `PolicyChangeEvent`.
    fn to_event(&self) -> PolicyChangeEvent {
        PolicyChangeEvent {
            operation: self.to_change_op(),
            policy_id: self.id.clone(),
        }
    }
}

/// Type of policy change operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyChangeOp {
    /// A new policy was created.
    Insert,
    /// An existing policy was updated.
    Update,
    /// A policy was deleted.
    Delete,
}

/// Event representing a policy change.
#[derive(Debug, Clone)]
pub struct PolicyChangeEvent {
    /// Type of change operation.
    pub operation: PolicyChangeOp,
    /// ID of the affected policy.
    pub policy_id: String,
}

// =============================================================================
// Policy Listener
// =============================================================================

/// PostgreSQL LISTEN/NOTIFY listener for access policy changes.
///
/// This struct manages a PostgreSQL LISTEN connection and forwards
/// change notifications to subscribers via a broadcast channel.
///
/// # Thread Safety
///
/// The listener is designed to be shared across threads using `Arc<PolicyListener>`.
/// Multiple subscribers can receive events independently.
pub struct PolicyListener {
    pool: PgPool,
    sender: broadcast::Sender<PolicyChangeEvent>,
}

impl PolicyListener {
    /// Creates a new `PolicyListener`.
    ///
    /// This sets up the notification channel but does not start listening yet.
    /// Call `start()` to begin listening for changes.
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        let (sender, _) = broadcast::channel(CHANNEL_CAPACITY);
        Self { pool, sender }
    }

    /// Returns a clone of the change event sender.
    ///
    /// This can be used to manually inject change events for testing.
    #[must_use]
    pub fn sender(&self) -> broadcast::Sender<PolicyChangeEvent> {
        self.sender.clone()
    }

    /// Subscribes to policy change events.
    ///
    /// Returns a receiver that yields `PolicyChangeEvent` as they occur.
    /// Multiple subscribers can call this method to receive independent copies
    /// of all events.
    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<PolicyChangeEvent> {
        self.sender.subscribe()
    }

    /// Returns the number of active subscribers.
    #[must_use]
    pub fn subscriber_count(&self) -> usize {
        self.sender.receiver_count()
    }

    /// Starts the policy listener in the background.
    ///
    /// This spawns a tokio task that:
    /// 1. Connects to PostgreSQL with LISTEN
    /// 2. Receives NOTIFY events
    /// 3. Forwards them to the change event channel
    ///
    /// The task runs until the sender is dropped or an unrecoverable error occurs.
    /// On connection errors, it automatically reconnects after a delay.
    ///
    /// # Returns
    ///
    /// A handle to the spawned task.
    #[instrument(skip(self), name = "policy_listener")]
    pub fn start(self: Arc<Self>) -> JoinHandle<()> {
        info!("Starting policy change listener");

        tokio::spawn(async move {
            loop {
                match self.listen_loop().await {
                    Ok(()) => {
                        info!("Policy listener stopped gracefully");
                        break;
                    }
                    Err(e) => {
                        error!(
                            error = %e,
                            delay_secs = RECONNECT_DELAY.as_secs(),
                            "Policy listener error, reconnecting"
                        );
                        sleep(RECONNECT_DELAY).await;
                    }
                }
            }
        })
    }

    /// Main listen loop that connects and receives notifications.
    async fn listen_loop(&self) -> Result<(), ListenerError> {
        let mut listener = PgListener::connect_with(&self.pool).await?;
        listener.listen(POLICY_CHANNEL).await?;

        info!(channel = POLICY_CHANNEL, "Listening for policy changes");

        loop {
            let notification = listener.recv().await?;
            let payload = notification.payload();

            debug!(payload = %payload, "Received policy NOTIFY");

            match serde_json::from_str::<NotifyPayload>(payload) {
                Ok(parsed) => {
                    let event = parsed.to_event();
                    info!(
                        operation = ?event.operation,
                        policy_id = %event.policy_id,
                        "Policy changed"
                    );

                    if self.sender.send(event).is_err() {
                        warn!("Failed to send policy change event - no subscribers");
                        // Continue listening even without subscribers
                    }
                }
                Err(e) => {
                    warn!(
                        error = %e,
                        payload = %payload,
                        "Failed to parse policy NOTIFY payload"
                    );
                }
            }
        }
    }
}

// =============================================================================
// Error Types
// =============================================================================

/// Errors that can occur in the policy listener.
#[derive(Debug, thiserror::Error)]
pub enum ListenerError {
    /// Database connection or query error.
    #[error("Database error: {0}")]
    Database(#[from] sqlx_core::Error),
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_notify_payload_to_change_op_insert() {
        let payload = NotifyPayload {
            operation: "INSERT".to_string(),
            id: "policy-123".to_string(),
        };

        assert_eq!(payload.to_change_op(), PolicyChangeOp::Insert);
    }

    #[test]
    fn test_notify_payload_to_change_op_update() {
        let payload = NotifyPayload {
            operation: "UPDATE".to_string(),
            id: "policy-123".to_string(),
        };

        assert_eq!(payload.to_change_op(), PolicyChangeOp::Update);
    }

    #[test]
    fn test_notify_payload_to_change_op_delete() {
        let payload = NotifyPayload {
            operation: "DELETE".to_string(),
            id: "policy-123".to_string(),
        };

        assert_eq!(payload.to_change_op(), PolicyChangeOp::Delete);
    }

    #[test]
    fn test_notify_payload_to_change_op_lowercase() {
        let payload = NotifyPayload {
            operation: "insert".to_string(),
            id: "policy-123".to_string(),
        };

        assert_eq!(payload.to_change_op(), PolicyChangeOp::Insert);
    }

    #[test]
    fn test_notify_payload_to_change_op_unknown() {
        let payload = NotifyPayload {
            operation: "UNKNOWN".to_string(),
            id: "policy-123".to_string(),
        };

        // Unknown operations default to Update
        assert_eq!(payload.to_change_op(), PolicyChangeOp::Update);
    }

    #[test]
    fn test_notify_payload_to_event() {
        let payload = NotifyPayload {
            operation: "INSERT".to_string(),
            id: "policy-456".to_string(),
        };

        let event = payload.to_event();

        assert_eq!(event.operation, PolicyChangeOp::Insert);
        assert_eq!(event.policy_id, "policy-456");
    }

    #[test]
    fn test_parse_notify_payload() {
        let json = r#"{"operation":"UPDATE","id":"abc-123"}"#;
        let payload: NotifyPayload = serde_json::from_str(json).unwrap();

        assert_eq!(payload.operation, "UPDATE");
        assert_eq!(payload.id, "abc-123");
    }
}
