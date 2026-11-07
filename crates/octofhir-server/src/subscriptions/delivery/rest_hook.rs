//! REST-hook channel for HTTP POST notifications.
//!
//! Delivers subscription notifications via HTTP POST to the configured endpoint.

use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use reqwest::{Client, header};

use super::DeliveryChannel;
use crate::egress::EgressPolicy;
use crate::subscriptions::SubscriptionEvent;
use crate::subscriptions::error::{SubscriptionError, SubscriptionResult};
use crate::subscriptions::types::{
    ActiveSubscription, DeliveryResult, PayloadContent, SubscriptionChannel,
};

/// REST-hook delivery channel.
pub struct RestHookChannel {
    /// HTTP client for making requests
    client: Client,
    /// Which endpoint addresses may be called.
    egress: Arc<EgressPolicy>,
}

impl RestHookChannel {
    /// Create a REST-hook channel whose requests only reach addresses `egress` allows.
    pub fn new(egress: Arc<EgressPolicy>) -> Self {
        let client = egress
            .client_builder()
            .timeout(std::time::Duration::from_secs(30))
            .connect_timeout(std::time::Duration::from_secs(10))
            // A redirect to an IP literal would skip the address check; webhooks don't need them.
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("Failed to create HTTP client");

        Self { client, egress }
    }

    /// Create with a custom client.
    pub fn with_client(client: Client) -> Self {
        Self {
            client,
            egress: Arc::default(),
        }
    }
}

impl Default for RestHookChannel {
    fn default() -> Self {
        Self::new(Arc::default())
    }
}

/// A notification bundle with what the subscriber asked for: `full-resource` as it is,
/// `id-only` without the resources, `empty` with the status alone and no focus.
fn with_payload(bundle: &serde_json::Value, payload: &PayloadContent) -> serde_json::Value {
    let mut bundle = bundle.clone();
    let Some(entries) = bundle["entry"].as_array_mut() else {
        return bundle;
    };
    match payload {
        PayloadContent::FullResource => {}
        PayloadContent::IdOnly => {
            for entry in entries.iter_mut().skip(1) {
                if let Some(entry) = entry.as_object_mut() {
                    entry.remove("resource");
                }
            }
        }
        PayloadContent::Empty => {
            entries.truncate(1);
            let status = entries
                .first_mut()
                .and_then(|entry| entry.get_mut("resource"));
            if let Some(status) = status {
                // R5 SubscriptionStatus, or the R4 backport Parameters. `get_mut`, not
                // indexing: an index would add the missing member as null.
                if let Some(events) = status
                    .get_mut("notificationEvent")
                    .and_then(|events| events.as_array_mut())
                {
                    for event in events.iter_mut().filter_map(|event| event.as_object_mut()) {
                        event.remove("focus");
                    }
                }
                if let Some(parameters) = status
                    .get_mut("parameter")
                    .and_then(|parameters| parameters.as_array_mut())
                {
                    for parts in parameters
                        .iter_mut()
                        .filter_map(|parameter| parameter.get_mut("part"))
                        .filter_map(|parts| parts.as_array_mut())
                    {
                        parts.retain(|part| part["name"] != "focus");
                    }
                }
            }
        }
    }
    bundle
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
            ref payload_content,
        } = subscription.channel
        else {
            return Err(SubscriptionError::DeliveryError(
                "Subscription channel is not REST-hook".to_string(),
            ));
        };

        let url = reqwest::Url::parse(endpoint)
            .map_err(|e| SubscriptionError::DeliveryError(format!("Invalid endpoint: {e}")))?;
        self.egress
            .check_url(&url)
            .map_err(SubscriptionError::DeliveryError)?;

        let start = Instant::now();

        // Build request
        let mut request = self
            .client
            .post(url)
            .header(header::CONTENT_TYPE, content_type.as_str());

        // Add custom headers
        for (key, value) in headers {
            request = request.header(key.as_str(), value.as_str());
        }

        // Send notification bundle
        let bundle = with_payload(&event.notification_bundle, payload_content);
        let body = serde_json::to_string(&bundle).map_err(|e| {
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
mod payload_tests {
    use super::*;
    use serde_json::json;

    fn bundle() -> serde_json::Value {
        json!({"resourceType": "Bundle", "type": "history", "entry": [
            {"resource": {"resourceType": "Parameters", "parameter": [
                {"name": "status", "valueCode": "active"},
                {"name": "notification-event", "part": [
                    {"name": "event-number", "valueString": "1"},
                    {"name": "focus", "valueReference": {"reference": "Encounter/e"}},
                ]},
            ]}},
            {"fullUrl": "Encounter/e", "resource": {"resourceType": "Encounter", "id": "e"}},
        ]})
    }

    #[test]
    fn a_payload_keeps_what_was_asked_for() {
        let empty = with_payload(&bundle(), &PayloadContent::Empty);
        assert_eq!(empty["entry"].as_array().unwrap().len(), 1);
        let parameters = &empty["entry"][0]["resource"];
        assert!(parameters.get("notificationEvent").is_none());
        assert!(parameters["parameter"][0].get("part").is_none());
        assert_eq!(
            parameters["parameter"][1]["part"].as_array().unwrap().len(),
            1
        );

        let id_only = with_payload(&bundle(), &PayloadContent::IdOnly);
        assert!(id_only["entry"][1].get("resource").is_none());
        assert_eq!(id_only["entry"][1]["fullUrl"], "Encounter/e");
        assert_eq!(
            with_payload(&bundle(), &PayloadContent::FullResource),
            bundle()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rest_hook_channel_name() {
        let channel = RestHookChannel::default();
        assert_eq!(channel.name(), "rest-hook");
    }
}
