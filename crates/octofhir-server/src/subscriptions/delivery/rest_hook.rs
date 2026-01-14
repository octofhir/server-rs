//! REST-hook channel for HTTP POST notifications.
//!
//! Delivers subscription notifications via HTTP POST to the configured endpoint.

use std::time::Instant;

use async_trait::async_trait;
use reqwest::{Client, header};

use super::DeliveryChannel;
use crate::subscriptions::SubscriptionEvent;
use crate::subscriptions::error::{SubscriptionError, SubscriptionResult};
use crate::subscriptions::types::{ActiveSubscription, DeliveryResult, SubscriptionChannel};

/// REST-hook delivery channel.
pub struct RestHookChannel {
    /// HTTP client for making requests
    client: Client,
}

impl RestHookChannel {
    /// Create a new REST-hook channel.
    pub fn new() -> Self {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .connect_timeout(std::time::Duration::from_secs(10))
            .build()
            .expect("Failed to create HTTP client");

        Self { client }
    }

    /// Create with a custom client.
    pub fn with_client(client: Client) -> Self {
        Self { client }
    }
}

impl Default for RestHookChannel {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl DeliveryChannel for RestHookChannel {
    fn name(&self) -> &str {
        "rest-hook"
    }

    async fn deliver(
        &self,
        subscription: &ActiveSubscription,
        event: &SubscriptionEvent,
    ) -> SubscriptionResult<DeliveryResult> {
        let SubscriptionChannel::RestHook {
            ref endpoint,
            ref headers,
            ref content_type,
            ..
        } = subscription.channel
        else {
            return Err(SubscriptionError::DeliveryError(
                "Subscription channel is not REST-hook".to_string(),
            ));
        };

        let start = Instant::now();

        // Build request
        let mut request = self
            .client
            .post(endpoint)
            .header(header::CONTENT_TYPE, content_type.as_str());

        // Add custom headers
        for (key, value) in headers {
            request = request.header(key.as_str(), value.as_str());
        }

        // Send notification bundle
        let body = serde_json::to_string(&event.notification_bundle).map_err(|e| {
            SubscriptionError::DeliveryError(format!("Failed to serialize bundle: {e}"))
        })?;

        let response = request.body(body).send().await;

        let elapsed = start.elapsed().as_millis() as u32;

        match response {
            Ok(resp) => {
                let status = resp.status();

                if status.is_success() {
                    tracing::debug!(
                        subscription_id = subscription.id,
                        endpoint = endpoint,
                        status = status.as_u16(),
                        elapsed_ms = elapsed,
                        "REST-hook delivery succeeded"
                    );
                    Ok(DeliveryResult::success(status.as_u16(), elapsed))
                } else {
                    let error_body = resp.text().await.unwrap_or_default();
                    tracing::warn!(
                        subscription_id = subscription.id,
                        endpoint = endpoint,
                        status = status.as_u16(),
                        error = error_body,
                        elapsed_ms = elapsed,
                        "REST-hook delivery failed with HTTP error"
                    );
                    Ok(DeliveryResult::http_failure(
                        status.as_u16(),
                        format!("HTTP {}: {}", status.as_u16(), error_body),
                        elapsed,
                    ))
                }
            }
            Err(e) => {
                tracing::warn!(
                    subscription_id = subscription.id,
                    endpoint = endpoint,
                    error = %e,
                    elapsed_ms = elapsed,
                    "REST-hook delivery failed with network error"
                );
                Ok(DeliveryResult::failure(e.to_string(), elapsed))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rest_hook_channel_name() {
        let channel = RestHookChannel::new();
        assert_eq!(channel.name(), "rest-hook");
    }
}
