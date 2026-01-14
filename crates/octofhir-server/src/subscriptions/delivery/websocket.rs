//! WebSocket delivery channel for FHIR Subscriptions.
//!
//! Implements the `$events` WebSocket endpoint for real-time subscription notifications.
//! Endpoint: `/fhir/Subscription/{id}/$events`

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::ws::{Message, WebSocket};
use futures_util::{SinkExt, StreamExt};
use parking_lot::RwLock;
use serde_json::json;
use tokio::sync::mpsc;
use tokio::time::interval;

use crate::subscriptions::types::{ActiveSubscription, SubscriptionEvent};

/// Message sent to a WebSocket client.
#[derive(Debug, Clone)]
pub enum WebSocketMessage {
    /// Subscription notification event
    Event(SubscriptionEvent),
    /// Heartbeat ping
    Heartbeat,
    /// Close connection
    Close,
}

/// Handle for sending messages to a connected WebSocket client.
#[derive(Clone)]
pub struct WebSocketHandle {
    sender: mpsc::Sender<WebSocketMessage>,
}

impl WebSocketHandle {
    /// Send a subscription event to the client.
    pub async fn send_event(
        &self,
        event: SubscriptionEvent,
    ) -> Result<(), mpsc::error::SendError<WebSocketMessage>> {
        self.sender.send(WebSocketMessage::Event(event)).await
    }

    /// Send a heartbeat to the client.
    pub async fn send_heartbeat(&self) -> Result<(), mpsc::error::SendError<WebSocketMessage>> {
        self.sender.send(WebSocketMessage::Heartbeat).await
    }

    /// Close the connection.
    pub async fn close(&self) -> Result<(), mpsc::error::SendError<WebSocketMessage>> {
        self.sender.send(WebSocketMessage::Close).await
    }
}

/// Registry of active WebSocket connections for subscriptions.
#[derive(Default)]
pub struct WebSocketRegistry {
    /// Map from subscription ID to list of connected handles
    connections: RwLock<HashMap<String, Vec<WebSocketHandle>>>,
}

impl WebSocketRegistry {
    /// Create a new registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a new WebSocket connection for a subscription.
    pub fn register(&self, subscription_id: &str, handle: WebSocketHandle) {
        let mut connections = self.connections.write();
        connections
            .entry(subscription_id.to_string())
            .or_default()
            .push(handle);

        tracing::debug!(
            subscription_id = subscription_id,
            "WebSocket connection registered"
        );
    }

    /// Unregister a WebSocket connection.
    pub fn unregister(&self, subscription_id: &str, _handle: &WebSocketHandle) {
        let mut connections = self.connections.write();
        if let Some(handles) = connections.get_mut(subscription_id) {
            // Remove handles with closed channels
            handles.retain(|h| !h.sender.is_closed());

            if handles.is_empty() {
                connections.remove(subscription_id);
            }
        }

        tracing::debug!(
            subscription_id = subscription_id,
            "WebSocket connection unregistered"
        );
    }

    /// Get all active handles for a subscription.
    pub fn get_handles(&self, subscription_id: &str) -> Vec<WebSocketHandle> {
        let connections = self.connections.read();
        connections
            .get(subscription_id)
            .cloned()
            .unwrap_or_default()
    }

    /// Broadcast an event to all connections for a subscription.
    pub async fn broadcast(&self, subscription_id: &str, event: SubscriptionEvent) {
        let handles = self.get_handles(subscription_id);

        for handle in handles {
            if let Err(e) = handle.send_event(event.clone()).await {
                tracing::debug!(
                    subscription_id = subscription_id,
                    error = %e,
                    "Failed to send event to WebSocket client"
                );
            }
        }
    }

    /// Get the number of active connections for a subscription.
    pub fn connection_count(&self, subscription_id: &str) -> usize {
        let connections = self.connections.read();
        connections
            .get(subscription_id)
            .map(|h| h.len())
            .unwrap_or(0)
    }

    /// Get the total number of active connections.
    pub fn total_connections(&self) -> usize {
        let connections = self.connections.read();
        connections.values().map(|h| h.len()).sum()
    }
}

/// Handle a WebSocket connection for subscription events.
///
/// This function:
/// 1. Validates the subscription exists and is active
/// 2. Creates a message channel for this connection
/// 3. Registers the connection in the registry
/// 4. Handles incoming messages and sends outgoing events
/// 5. Sends periodic heartbeats
pub async fn handle_subscription_websocket(
    socket: WebSocket,
    subscription: ActiveSubscription,
    registry: Arc<WebSocketRegistry>,
) {
    let subscription_id = subscription.id.clone();
    let heartbeat_period = subscription.heartbeat_period.unwrap_or(60);

    tracing::info!(
        subscription_id = %subscription_id,
        heartbeat_period = heartbeat_period,
        "WebSocket connection established for subscription"
    );

    // Create channel for sending messages to this client
    let (tx, mut rx) = mpsc::channel::<WebSocketMessage>(32);
    let handle = WebSocketHandle { sender: tx };

    // Register the connection
    registry.register(&subscription_id, handle.clone());

    // Split the socket
    let (mut sender, mut receiver) = socket.split();

    // Send initial handshake event
    let handshake = create_handshake_bundle(&subscription);
    if let Err(e) = sender
        .send(Message::Text(handshake.to_string().into()))
        .await
    {
        tracing::error!(error = %e, "Failed to send handshake");
        registry.unregister(&subscription_id, &handle);
        return;
    }

    // Heartbeat interval
    let mut heartbeat_interval = interval(Duration::from_secs(heartbeat_period as u64));

    loop {
        tokio::select! {
            // Handle incoming messages from client
            msg = receiver.next() => {
                match msg {
                    Some(Ok(Message::Close(_))) | None => {
                        tracing::debug!(subscription_id = %subscription_id, "Client closed WebSocket");
                        break;
                    }
                    Some(Ok(Message::Ping(data))) => {
                        if let Err(e) = sender.send(Message::Pong(data)).await {
                            tracing::debug!(error = %e, "Failed to send pong");
                            break;
                        }
                    }
                    Some(Ok(Message::Pong(_))) => {
                        // Client responded to our ping
                    }
                    Some(Ok(Message::Text(_))) => {
                        // Client sent text - typically ignored for subscription WebSockets
                    }
                    Some(Ok(Message::Binary(_))) => {
                        // Client sent binary - typically ignored
                    }
                    Some(Err(e)) => {
                        tracing::debug!(error = %e, "WebSocket error");
                        break;
                    }
                }
            }

            // Handle outgoing messages from channel
            msg = rx.recv() => {
                match msg {
                    Some(WebSocketMessage::Event(event)) => {
                        let bundle = create_notification_bundle(&subscription, &event);
                        if let Err(e) = sender.send(Message::Text(bundle.to_string().into())).await {
                            tracing::debug!(error = %e, "Failed to send event");
                            break;
                        }
                    }
                    Some(WebSocketMessage::Heartbeat) => {
                        let heartbeat = create_heartbeat_bundle(&subscription);
                        if let Err(e) = sender.send(Message::Text(heartbeat.to_string().into())).await {
                            tracing::debug!(error = %e, "Failed to send heartbeat");
                            break;
                        }
                    }
                    Some(WebSocketMessage::Close) | None => {
                        tracing::debug!(subscription_id = %subscription_id, "Channel closed");
                        break;
                    }
                }
            }

            // Send heartbeat
            _ = heartbeat_interval.tick() => {
                let heartbeat = create_heartbeat_bundle(&subscription);
                if let Err(e) = sender.send(Message::Text(heartbeat.to_string().into())).await {
                    tracing::debug!(error = %e, "Failed to send heartbeat");
                    break;
                }
            }
        }
    }

    // Unregister the connection
    registry.unregister(&subscription_id, &handle);

    tracing::info!(
        subscription_id = %subscription_id,
        "WebSocket connection closed for subscription"
    );
}

/// Create a handshake notification bundle.
fn create_handshake_bundle(subscription: &ActiveSubscription) -> serde_json::Value {
    json!({
        "resourceType": "Bundle",
        "type": "subscription-notification",
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "entry": [{
            "resource": {
                "resourceType": "SubscriptionStatus",
                "status": "active",
                "type": "handshake",
                "subscription": {
                    "reference": format!("Subscription/{}", subscription.id)
                },
                "topic": subscription.topic_url
            }
        }]
    })
}

/// Create a heartbeat notification bundle.
fn create_heartbeat_bundle(subscription: &ActiveSubscription) -> serde_json::Value {
    json!({
        "resourceType": "Bundle",
        "type": "subscription-notification",
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "entry": [{
            "resource": {
                "resourceType": "SubscriptionStatus",
                "status": "active",
                "type": "heartbeat",
                "subscription": {
                    "reference": format!("Subscription/{}", subscription.id)
                },
                "topic": subscription.topic_url
            }
        }]
    })
}

/// Create a notification bundle for an event.
fn create_notification_bundle(
    _subscription: &ActiveSubscription,
    event: &SubscriptionEvent,
) -> serde_json::Value {
    // The event already contains the notification bundle
    event.notification_bundle.clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_websocket_registry() {
        let registry = WebSocketRegistry::new();

        assert_eq!(registry.total_connections(), 0);
        assert_eq!(registry.connection_count("sub-1"), 0);
    }

    #[test]
    fn test_create_handshake_bundle() {
        use crate::subscriptions::types::{SubscriptionChannel, SubscriptionStatus};

        let subscription = ActiveSubscription {
            id: "test-sub".to_string(),
            topic_url: "http://example.org/topic".to_string(),
            status: SubscriptionStatus::Active,
            channel: SubscriptionChannel::WebSocket {
                heartbeat_period: 60,
            },
            filter_by: vec![],
            end_time: None,
            heartbeat_period: Some(60),
            max_count: None,
            contact: None,
        };

        let bundle = create_handshake_bundle(&subscription);

        assert_eq!(bundle["resourceType"], "Bundle");
        assert_eq!(bundle["type"], "subscription-notification");
        assert_eq!(bundle["entry"][0]["resource"]["type"], "handshake");
    }
}
