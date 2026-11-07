//! App subscriptions: a change that matches a trigger goes to the subscription's webhook and
//! becomes notifications.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use octofhir_core::events::{HookError, ResourceEvent, ResourceEventType, ResourceHook};
use octofhir_fhirpath::{Collection, EvaluationContext, FhirPathEngine};
use octofhir_notifications::{
    Notification, NotificationChannel as Channel, NotificationQueueStorage, NotificationRecipient,
    NotificationStatus, parse_iso_duration,
};
use octofhir_storage::{DynStorage, SearchParams};
use serde_json::{Value, json};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use tokio::sync::RwLock;

use crate::app_platform::types::SubscriptionEvent;
use crate::gateway::types::AppSubscription;

/// Dispatches resource changes to the App subscriptions they match.
pub struct AppSubscriptionHook {
    storage: DynStorage,
    queue: Arc<dyn NotificationQueueStorage>,
    engine: Arc<FhirPathEngine>,
    client: reqwest::Client,
    /// Active subscriptions; read again after an `AppSubscription` changes.
    subscriptions: RwLock<Option<Arc<Vec<AppSubscription>>>>,
}

impl AppSubscriptionHook {
    pub fn new(
        storage: DynStorage,
        queue: Arc<dyn NotificationQueueStorage>,
        engine: Arc<FhirPathEngine>,
        client: reqwest::Client,
    ) -> Self {
        Self {
            storage,
            queue,
            engine,
            client,
            subscriptions: RwLock::new(None),
        }
    }

    async fn active(&self) -> Result<Arc<Vec<AppSubscription>>, HookError> {
        if let Some(loaded) = self.subscriptions.read().await.as_ref() {
            return Ok(loaded.clone());
        }
        let found = self
            .storage
            .search("AppSubscription", &SearchParams::new().with_count(1000))
            .await
            .map_err(|error| HookError::database(error.to_string()))?;
        let loaded: Arc<Vec<AppSubscription>> = Arc::new(
            found
                .entries
                .into_iter()
                .filter_map(|stored| {
                    serde_json::from_value::<AppSubscription>(stored.resource).ok()
                })
                .filter(|subscription| subscription.active)
                .collect(),
        );
        *self.subscriptions.write().await = Some(loaded.clone());
        Ok(loaded)
    }

    /// The values of `expression` on `resource`, as JSON.
    async fn evaluate(&self, expression: &str, resource: &Value) -> Result<Vec<Value>, String> {
        let provider = self.engine.get_model_provider();
        let collection = Collection::from_json_resource(resource.clone(), Some(provider.clone()))
            .await
            .map_err(|error| error.to_string())?;
        let context = EvaluationContext::new(collection, provider, None, None, None);
        let result = self
            .engine
            .evaluate(expression, &context)
            .await
            .map_err(|error| error.to_string())?;
        Ok(result
            .value
            .into_vec()
            .into_iter()
            .map(|value| value.to_json_value())
            .collect())
    }

    async fn deliver(&self, subscription: &AppSubscription, event: &str, resource: &Value) {
        let id = subscription.id.as_deref().unwrap_or_default();
        if let Some(channel) = &subscription.channel
            && channel.channel_type == "webhook"
        {
            let body = json!({"subscription": id, "event": event, "resource": resource});
            let sent = self.client.post(&channel.endpoint).json(&body).send().await;
            match sent.map(|response| response.status()) {
                Ok(status) if status.is_success() => {}
                Ok(status) => tracing::warn!(subscription = %id, %status, "App webhook refused"),
                Err(error) => tracing::warn!(subscription = %id, %error, "App webhook failed"),
            }
        }
        if let Some(notification) = &subscription.notification
            && let Err(error) = self.notify(id, notification, resource).await
        {
            tracing::warn!(subscription = %id, %error, "App notification not queued");
        }
    }

    async fn notify(
        &self,
        subscription: &str,
        definition: &crate::app_platform::types::NotificationDef,
        resource: &Value,
    ) -> Result<(), String> {
        let provider = definition
            .provider
            .as_deref()
            .map(|reference| reference.trim_start_matches("NotificationProvider/"))
            .ok_or("no provider")?;
        let addresses = match &definition.recipient {
            Some(recipient) => self.recipients(&recipient.fhirpath, resource).await?,
            None => Vec::new(),
        };
        let scheduled_at = match &definition.delay {
            Some(delay) => {
                let at = self
                    .evaluate(&delay.relative_to, resource)
                    .await?
                    .first()
                    .and_then(Value::as_str)
                    .and_then(|text| OffsetDateTime::parse(text, &Rfc3339).ok())
                    .ok_or("the delay has no date to count from")?;
                Some(at + parse_iso_duration(&delay.offset)?)
            }
            None => None,
        };
        // Simple elements of the resource fill `{{name}}`.
        let data: HashMap<String, Value> = resource
            .as_object()
            .into_iter()
            .flatten()
            .filter(|(_, value)| !value.is_object() && !value.is_array())
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect();
        for channel in definition.channel.channels() {
            let (channel, recipient) = match channel {
                "email" => (Channel::Email, pick(&addresses, "email")),
                "telegram" => (Channel::Telegram, pick(&addresses, "telegram")),
                "webhook" => (Channel::Webhook, pick(&addresses, "url")),
                other => {
                    tracing::warn!(subscription, channel = other, "Channel cannot be notified");
                    continue;
                }
            };
            let notification = Notification {
                id: octofhir_core::generate_id(),
                channel,
                provider_id: provider.to_owned(),
                status: NotificationStatus::Pending,
                recipient: NotificationRecipient {
                    reference: resource_reference(resource),
                    email: (channel == Channel::Email)
                        .then(|| recipient.clone())
                        .flatten(),
                    telegram_chat_id: (channel == Channel::Telegram)
                        .then(|| recipient.clone())
                        .flatten(),
                    webhook_url: (channel == Channel::Webhook)
                        .then(|| recipient.clone())
                        .flatten(),
                },
                template_id: definition.template.clone(),
                template_data: data.clone(),
                scheduled_at,
                created_at: OffsetDateTime::now_utc(),
                sent_at: None,
                error: None,
                retry_count: 0,
            };
            self.queue
                .enqueue(&notification)
                .await
                .map_err(|error| error.to_string())?;
        }
        Ok(())
    }

    /// Addresses by kind (`email`, `telegram`, `url`) from what `expression` gives: text, a
    /// ContactPoint, or a Reference to a resource whose `telecom` holds them.
    async fn recipients(
        &self,
        expression: &str,
        resource: &Value,
    ) -> Result<Vec<(String, String)>, String> {
        let mut found = Vec::new();
        for value in self.evaluate(expression, resource).await? {
            if let Some(text) = value.as_str() {
                let kind = if text.contains('@') { "email" } else { "url" };
                found.push((kind.to_owned(), text.to_owned()));
            } else if value.get("system").is_some() {
                found.extend(contact(&value));
            } else if let Some((kind, id)) =
                value["reference"].as_str().and_then(|r| r.split_once('/'))
            {
                let target = self
                    .storage
                    .read(kind, id)
                    .await
                    .map_err(|error| error.to_string())?;
                for point in target.iter().flat_map(|stored| {
                    stored.resource["telecom"]
                        .as_array()
                        .cloned()
                        .unwrap_or_default()
                }) {
                    found.extend(contact(&point));
                }
            }
        }
        Ok(found)
    }
}

fn contact(point: &Value) -> Option<(String, String)> {
    let value = point["value"].as_str()?;
    let kind = match point["system"].as_str()? {
        "email" => "email",
        "url" => "url",
        "other" => "telegram",
        _ => return None,
    };
    Some((kind.to_owned(), value.to_owned()))
}

fn pick(addresses: &[(String, String)], kind: &str) -> Option<String> {
    addresses
        .iter()
        .find(|(found, _)| found == kind)
        .map(|(_, value)| value.clone())
}

fn resource_reference(resource: &Value) -> Option<String> {
    Some(format!(
        "{}/{}",
        resource["resourceType"].as_str()?,
        resource["id"].as_str()?
    ))
}

#[async_trait]
impl ResourceHook for AppSubscriptionHook {
    fn name(&self) -> &str {
        "app_subscriptions"
    }

    fn resource_types(&self) -> &[&str] {
        &[]
    }

    async fn handle(&self, event: &ResourceEvent) -> Result<(), HookError> {
        if event.resource_type == "AppSubscription" {
            *self.subscriptions.write().await = None;
            return Ok(());
        }
        let (kind, resource) = match event.event_type {
            ResourceEventType::Created => (SubscriptionEvent::Create, &event.resource),
            ResourceEventType::Updated => (SubscriptionEvent::Update, &event.resource),
            ResourceEventType::Deleted => (SubscriptionEvent::Delete, &event.previous_resource),
        };
        let Some(resource) = resource else {
            return Ok(());
        };
        for subscription in self.active().await?.iter() {
            let trigger = &subscription.trigger;
            if trigger.resource_type != event.resource_type || trigger.event != kind {
                continue;
            }
            if let Some(filter) = &trigger.fhirpath {
                match self.evaluate(filter, resource).await {
                    Ok(values) if values == [Value::Bool(true)] => {}
                    Ok(_) => continue,
                    Err(error) => {
                        tracing::warn!(subscription = ?subscription.id, %error, "Trigger filter failed");
                        continue;
                    }
                }
            }
            self.deliver(subscription, &kind.to_string(), resource)
                .await;
        }
        Ok(())
    }
}

impl std::fmt::Debug for AppSubscriptionHook {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppSubscriptionHook")
            .finish_non_exhaustive()
    }
}
