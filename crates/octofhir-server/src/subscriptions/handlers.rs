//! HTTP handlers for subscription endpoints.
//!
//! Provides handlers for:
//! - `GET /fhir/Subscription/{id}/$events` - WebSocket upgrade for real-time notifications

use std::sync::Arc;

use axum::{
    extract::{Path, State, WebSocketUpgrade},
    http::StatusCode,
    response::{IntoResponse, Response},
};

use super::delivery::websocket::handle_subscription_websocket;
use super::{SubscriptionManager, WebSocketRegistry};
use crate::subscriptions::types::SubscriptionChannel;

/// State required for subscription WebSocket handler.
#[derive(Clone)]
pub struct SubscriptionWebSocketState {
    pub subscription_manager: Arc<SubscriptionManager>,
    pub websocket_registry: Arc<WebSocketRegistry>,
}

/// Handler for WebSocket upgrade at `/fhir/Subscription/{id}/$events`.
///
/// This endpoint upgrades the HTTP connection to a WebSocket for receiving
/// real-time subscription notifications.
pub async fn subscription_events_handler(
    State(state): State<SubscriptionWebSocketState>,
    Path(id): Path<String>,
    ws: WebSocketUpgrade,
) -> Response {
    // Get the subscription
    let subscription = match state.subscription_manager.get_subscription(&id).await {
        Ok(Some(sub)) => sub,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                format!("Subscription/{} not found", id),
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!(error = %e, subscription_id = %id, "Failed to get subscription");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to get subscription",
            )
                .into_response();
        }
    };

    // Verify subscription uses WebSocket channel
    if !matches!(subscription.channel, SubscriptionChannel::WebSocket { .. }) {
        return (
            StatusCode::BAD_REQUEST,
            "Subscription does not use WebSocket channel",
        )
            .into_response();
    }

    // Verify subscription is active
    if subscription.status != super::types::SubscriptionStatus::Active {
        return (
            StatusCode::BAD_REQUEST,
            format!(
                "Subscription is not active (status: {:?})",
                subscription.status
            ),
        )
            .into_response();
    }

    let registry = state.websocket_registry.clone();

    // Upgrade to WebSocket
    ws.on_upgrade(move |socket| async move {
        handle_subscription_websocket(socket, subscription, registry).await;
    })
}
