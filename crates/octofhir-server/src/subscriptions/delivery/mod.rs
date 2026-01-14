//! Notification delivery system for subscriptions.
//!
//! This module handles the delivery of subscription notifications through various channels:
//! - REST-hook (HTTP POST)
//! - WebSocket (real-time)
//! - Email (via SMTP)

pub mod email;
pub mod processor;
pub mod rest_hook;
pub mod websocket;

pub use email::EmailChannel;
pub use processor::DeliveryProcessor;
pub use rest_hook::RestHookChannel;
pub use websocket::{WebSocketRegistry, handle_subscription_websocket};

use async_trait::async_trait;

use super::error::SubscriptionResult;
use super::types::{ActiveSubscription, DeliveryResult, SubscriptionEvent};

/// Trait for notification delivery channels.
#[async_trait]
pub trait DeliveryChannel: Send + Sync {
    /// Channel name for logging and metrics.
    fn name(&self) -> &str;

    /// Deliver a notification event to the subscription endpoint.
    async fn deliver(
        &self,
        subscription: &ActiveSubscription,
        event: &SubscriptionEvent,
    ) -> SubscriptionResult<DeliveryResult>;
}
