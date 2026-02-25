//! Background delivery processor for subscription events.
//!
//! Event-driven processor that wakes up when new events are enqueued
//! instead of polling the database on a fixed interval.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{watch, Notify};

use super::websocket::WebSocketRegistry;
use super::{DeliveryChannel, EmailChannel, RestHookChannel};
use crate::subscriptions::error::SubscriptionResult;
use crate::subscriptions::storage::SubscriptionEventStorage;
use crate::subscriptions::subscription_manager::SubscriptionManager;
use crate::subscriptions::types::{DeliveryResult, SubscriptionChannel};

/// Maximum idle timeout before a periodic check (catches retries with exponential backoff).
const MAX_IDLE_TIMEOUT: Duration = Duration::from_secs(60);

/// Background delivery processor.
pub struct DeliveryProcessor {
    /// Event storage for queue operations
    event_storage: Arc<SubscriptionEventStorage>,

    /// Subscription manager for looking up subscriptions
    subscription_manager: Arc<SubscriptionManager>,

    /// REST-hook delivery channel
    rest_hook_channel: RestHookChannel,

    /// WebSocket connection registry
    websocket_registry: Arc<WebSocketRegistry>,

    /// Email delivery channel (optional)
    email_channel: Option<EmailChannel>,

    /// Batch size for claiming events
    batch_size: i32,

    /// Notify handle — wakes up the processor when new events are enqueued
    event_notify: Arc<Notify>,
}

impl DeliveryProcessor {
    /// Create a new delivery processor.
    pub fn new(
        event_storage: Arc<SubscriptionEventStorage>,
        subscription_manager: Arc<SubscriptionManager>,
        websocket_registry: Arc<WebSocketRegistry>,
    ) -> Self {
        let event_notify = event_storage.event_notify();
        Self {
            event_storage,
            subscription_manager,
            rest_hook_channel: RestHookChannel::new(),
            websocket_registry,
            email_channel: None,
            batch_size: 10,
            event_notify,
        }
    }

    /// Set the batch size for claiming events.
    pub fn with_batch_size(mut self, batch_size: i32) -> Self {
        self.batch_size = batch_size;
        self
    }

    /// Configure email delivery via SMTP.
    pub fn with_smtp_email(
        mut self,
        host: String,
        port: u16,
        username: Option<String>,
        password: Option<String>,
        from_email: String,
    ) -> Self {
        self.email_channel = Some(EmailChannel::with_smtp(
            host, port, username, password, from_email,
        ));
        self
    }

    /// Configure email delivery via SendGrid.
    pub fn with_sendgrid_email(mut self, api_key: String, from_email: String) -> Self {
        self.email_channel = Some(EmailChannel::with_sendgrid(api_key, from_email));
        self
    }

    /// Start the delivery processor with a shutdown signal.
    ///
    /// The processor sleeps until woken by either:
    /// - `event_notify` — a new event was enqueued
    /// - `MAX_IDLE_TIMEOUT` — periodic check for events with retry backoff
    /// - shutdown signal
    pub async fn run(self, mut shutdown: watch::Receiver<bool>) -> SubscriptionResult<()> {
        tracing::info!("Starting subscription delivery processor (event-driven)");

        loop {
            // Process all available batches until the queue is drained
            loop {
                let processed = self.process_batch().await;
                if !processed {
                    break;
                }
            }

            // Wait for new events, periodic timeout, or shutdown
            tokio::select! {
                biased;

                result = shutdown.changed() => {
                    match result {
                        Ok(()) if *shutdown.borrow() => {
                            tracing::info!("Subscription delivery processor shutting down");
                            break;
                        }
                        Ok(()) => {
                            // Value changed but not to shutdown, continue
                        }
                        Err(_) => {
                            tracing::info!("Subscription delivery processor shutdown channel closed");
                            break;
                        }
                    }
                }
                _ = self.event_notify.notified() => {
                    // New event enqueued, process it
                }
                _ = tokio::time::sleep(MAX_IDLE_TIMEOUT) => {
                    // Periodic check for events with retry backoff (next_retry_at may have arrived)
                }
            }
        }

        Ok(())
    }

    /// Process a batch of events. Returns `true` if events were processed.
    async fn process_batch(&self) -> bool {
        // Claim events from queue
        let events = match self.event_storage.claim_events(self.batch_size).await {
            Ok(events) => events,
            Err(e) => {
                tracing::error!(error = %e, "Failed to claim subscription events");
                return false;
            }
        };

        if events.is_empty() {
            return false;
        }

        tracing::debug!(count = events.len(), "Processing subscription events");

        for event in events {
            // Get the subscription
            let subscription = match self
                .subscription_manager
                .get_subscription(&event.subscription_id)
                .await
            {
                Ok(Some(sub)) => sub,
                Ok(None) => {
                    tracing::warn!(
                        subscription_id = event.subscription_id,
                        "Subscription not found, marking event as failed"
                    );
                    if let Err(e) = self
                        .event_storage
                        .mark_retry(&event.id, Some("Subscription not found"))
                        .await
                    {
                        tracing::error!(error = %e, "Failed to mark event as retry");
                    }
                    continue;
                }
                Err(e) => {
                    tracing::error!(
                        subscription_id = event.subscription_id,
                        error = %e,
                        "Failed to get subscription"
                    );
                    continue;
                }
            };

            // Deliver based on channel type
            let result = match &subscription.channel {
                SubscriptionChannel::RestHook { .. } => {
                    self.rest_hook_channel.deliver(&subscription, &event).await
                }
                SubscriptionChannel::WebSocket { .. } => {
                    // Broadcast to connected WebSocket clients
                    self.websocket_registry
                        .broadcast(&subscription.id, event.clone())
                        .await;

                    // WebSocket delivery is always considered successful if there are connected clients
                    let connection_count =
                        self.websocket_registry.connection_count(&subscription.id);
                    Ok(DeliveryResult {
                        success: connection_count > 0,
                        http_status: None,
                        response_time_ms: 0,
                        error: if connection_count == 0 {
                            Some("No connected WebSocket clients".to_string())
                        } else {
                            None
                        },
                        error_code: None,
                    })
                }
                SubscriptionChannel::Email { .. } => {
                    if let Some(ref email_channel) = self.email_channel {
                        email_channel.deliver(&subscription, &event).await
                    } else {
                        tracing::warn!("Email delivery not configured");
                        Ok(DeliveryResult {
                            success: false,
                            http_status: None,
                            response_time_ms: 0,
                            error: Some("Email delivery not configured".to_string()),
                            error_code: Some("EMAIL_NOT_CONFIGURED".to_string()),
                        })
                    }
                }
                SubscriptionChannel::Message { .. } => {
                    // TODO: Implement Message delivery
                    tracing::warn!("Message delivery not yet implemented");
                    continue;
                }
            };

            match result {
                Ok(delivery_result) => {
                    // Record delivery attempt
                    if let Err(e) = self
                        .event_storage
                        .record_delivery_attempt(
                            &event.id,
                            &event.subscription_id,
                            event.attempts + 1,
                            subscription.channel.channel_type(),
                            delivery_result.success,
                            delivery_result.http_status.map(|s| s as i32),
                            delivery_result.response_time_ms as i32,
                            delivery_result.error.as_deref(),
                        )
                        .await
                    {
                        tracing::error!(error = %e, "Failed to record delivery attempt");
                    }

                    if delivery_result.success {
                        if let Err(e) = self.event_storage.mark_delivered(&event.id).await {
                            tracing::error!(error = %e, "Failed to mark event as delivered");
                        }
                    } else {
                        if let Err(e) = self
                            .event_storage
                            .mark_retry(&event.id, delivery_result.error.as_deref())
                            .await
                        {
                            tracing::error!(error = %e, "Failed to mark event for retry");
                        }
                    }
                }
                Err(e) => {
                    tracing::error!(
                        event_id = event.id,
                        error = %e,
                        "Delivery failed with error"
                    );
                    if let Err(e) = self
                        .event_storage
                        .mark_retry(&event.id, Some(&e.to_string()))
                        .await
                    {
                        tracing::error!(error = %e, "Failed to mark event for retry");
                    }
                }
            }
        }

        true
    }
}
