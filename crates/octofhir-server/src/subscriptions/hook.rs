//! Subscription Hook for capturing resource events.
//!
//! Implements the ResourceHook trait to receive FHIR resource events
//! and dispatch them to matching subscriptions.

use std::sync::Arc;

use async_trait::async_trait;
use octofhir_core::events::{
    hooks::{HookError, ResourceHook},
    types::{ResourceEvent, ResourceEventType},
};

use super::error::SubscriptionResult;
use super::event_matcher::EventMatcher;
use super::storage::SubscriptionEventStorage;
use super::subscription_manager::SubscriptionManager;
use super::topic_registry::TopicRegistry;
use super::types::{
    NotificationBundleBuilder, SubscriptionEventType, SubscriptionStatus, TriggerInteraction,
};

/// Hook that captures resource events and dispatches to matching subscriptions.
pub struct SubscriptionHook {
    /// Topic registry for finding matching topics
    topic_registry: Arc<TopicRegistry>,

    /// Subscription manager for getting active subscriptions
    subscription_manager: Arc<SubscriptionManager>,

    /// Event matcher for filter evaluation
    event_matcher: Arc<EventMatcher>,

    /// Event storage for queueing notifications
    event_storage: Arc<SubscriptionEventStorage>,

    /// Whether subscriptions are enabled
    enabled: bool,
}

impl SubscriptionHook {
    /// Create a new subscription hook.
    pub fn new(
        topic_registry: Arc<TopicRegistry>,
        subscription_manager: Arc<SubscriptionManager>,
        event_matcher: Arc<EventMatcher>,
        event_storage: Arc<SubscriptionEventStorage>,
        enabled: bool,
    ) -> Self {
        Self {
            topic_registry,
            subscription_manager,
            event_matcher,
            event_storage,
            enabled,
        }
    }

    /// Process a resource event and dispatch to matching subscriptions.
    async fn process_event(&self, event: &ResourceEvent) -> SubscriptionResult<()> {
        let interaction = match event.event_type {
            ResourceEventType::Created => TriggerInteraction::Create,
            ResourceEventType::Updated => TriggerInteraction::Update,
            ResourceEventType::Deleted => TriggerInteraction::Delete,
        };

        // Find matching topics for this resource type and interaction
        let topics = self
            .topic_registry
            .find_matching_topics(&event.resource_type, interaction);

        if topics.is_empty() {
            tracing::trace!(
                resource_type = event.resource_type,
                interaction = ?interaction,
                "No matching subscription topics"
            );
            return Ok(());
        }

        tracing::debug!(
            resource_type = event.resource_type,
            resource_id = event.resource_id,
            interaction = ?interaction,
            topic_count = topics.len(),
            "Found matching subscription topics"
        );

        // For each matching topic, find active subscriptions
        for topic in topics {
            let subscriptions = self
                .subscription_manager
                .get_subscriptions_for_topic(&topic.url)
                .await?;

            if subscriptions.is_empty() {
                continue;
            }

            tracing::debug!(
                topic_url = topic.url,
                subscription_count = subscriptions.len(),
                "Found active subscriptions for topic"
            );

            // Get the resource data for matching
            let Some(ref resource) = event.resource else {
                // For delete events without resource data, we can only match
                // based on resource type - detailed matching is skipped
                if event.event_type == ResourceEventType::Deleted {
                    self.queue_events_for_delete(
                        &topic.url,
                        &subscriptions,
                        &event.resource_type,
                        &event.resource_id,
                    )
                    .await?;
                }
                continue;
            };

            // Evaluate each subscription's filters
            for subscription in subscriptions {
                if subscription.status != SubscriptionStatus::Active {
                    continue;
                }

                // Check if subscription has expired
                if let Some(end_time) = subscription.end_time {
                    if end_time < time::OffsetDateTime::now_utc() {
                        tracing::debug!(
                            subscription_id = subscription.id,
                            "Subscription has expired, skipping"
                        );
                        continue;
                    }
                }

                // Evaluate filters
                let matches = self
                    .event_matcher
                    .matches(
                        resource,
                        None, // TODO: Support previous resource version for update triggers
                        &topic,
                        &subscription,
                        interaction,
                    )
                    .await;

                match matches {
                    Ok(true) => {
                        tracing::debug!(
                            subscription_id = subscription.id,
                            resource_type = event.resource_type,
                            resource_id = event.resource_id,
                            "Resource matched subscription filters, queueing event"
                        );

                        // Build notification bundle
                        let bundle = NotificationBundleBuilder::new(
                            subscription.id.clone(),
                            topic.url.clone(),
                            SubscriptionEventType::EventNotification,
                            0, // Event number will be assigned by storage
                        )
                        .with_focus(serde_json::Value::clone(resource))
                        .build();

                        // Queue the event
                        self.event_storage
                            .enqueue(
                                &subscription.id,
                                &topic.url,
                                SubscriptionEventType::EventNotification,
                                Some(&event.resource_type),
                                Some(&event.resource_id),
                                Some(interaction),
                                bundle,
                            )
                            .await?;
                    }
                    Ok(false) => {
                        tracing::trace!(
                            subscription_id = subscription.id,
                            "Resource did not match subscription filters"
                        );
                    }
                    Err(e) => {
                        tracing::warn!(
                            subscription_id = subscription.id,
                            error = %e,
                            "Error evaluating subscription filters"
                        );
                    }
                }
            }
        }

        Ok(())
    }

    /// Queue events for delete operations where we don't have resource data.
    async fn queue_events_for_delete(
        &self,
        topic_url: &str,
        subscriptions: &[super::types::ActiveSubscription],
        resource_type: &str,
        resource_id: &str,
    ) -> SubscriptionResult<()> {
        for subscription in subscriptions {
            if subscription.status != SubscriptionStatus::Active {
                continue;
            }

            // For deletes without resource data, we can't evaluate FHIRPath filters
            // Only subscriptions without filters will receive the notification
            if !subscription.filter_by.is_empty() {
                tracing::debug!(
                    subscription_id = subscription.id,
                    "Skipping delete notification for subscription with filters (no resource data)"
                );
                continue;
            }

            tracing::debug!(
                subscription_id = subscription.id,
                resource_type = resource_type,
                resource_id = resource_id,
                "Queueing delete event for subscription"
            );

            // Build minimal notification bundle for delete
            let bundle = NotificationBundleBuilder::new(
                subscription.id.clone(),
                topic_url.to_string(),
                SubscriptionEventType::EventNotification,
                0,
            )
            .build();

            self.event_storage
                .enqueue(
                    &subscription.id,
                    topic_url,
                    SubscriptionEventType::EventNotification,
                    Some(resource_type),
                    Some(resource_id),
                    Some(TriggerInteraction::Delete),
                    bundle,
                )
                .await?;
        }

        Ok(())
    }
}

#[async_trait]
impl ResourceHook for SubscriptionHook {
    fn name(&self) -> &str {
        "subscription_dispatcher"
    }

    fn resource_types(&self) -> &[&str] {
        // Empty slice means match all resource types
        // Subscriptions can be set up for any FHIR resource type
        &[]
    }

    async fn handle(&self, event: &ResourceEvent) -> Result<(), HookError> {
        if !self.enabled {
            return Ok(());
        }

        // Skip internal/system resources that typically shouldn't trigger subscriptions
        match event.resource_type.as_str() {
            // Skip subscription-related resources to avoid loops
            "Subscription" | "SubscriptionTopic" | "SubscriptionStatus" => {
                return Ok(());
            }
            // Skip AuditEvent to prevent audit loops
            "AuditEvent" => {
                return Ok(());
            }
            _ => {}
        }

        if let Err(e) = self.process_event(event).await {
            tracing::error!(
                resource_type = event.resource_type,
                resource_id = event.resource_id,
                error = %e,
                "Failed to process subscription event"
            );
            // Don't propagate errors to avoid blocking the main request
            // Subscription delivery is best-effort from the hook's perspective
        }

        Ok(())
    }

    async fn on_start(&self) -> Result<(), HookError> {
        if !self.enabled {
            tracing::info!("Subscription hook disabled");
            return Ok(());
        }

        // Reload topics from storage on startup
        if let Err(e) = self.topic_registry.reload().await {
            tracing::error!(error = %e, "Failed to load subscription topics on startup");
            return Err(HookError::execution(format!(
                "Failed to load subscription topics: {e}"
            )));
        }

        tracing::info!(
            topic_count = self.topic_registry.topic_count(),
            "Subscription hook started"
        );

        Ok(())
    }

    async fn on_shutdown(&self) -> Result<(), HookError> {
        tracing::info!("Subscription hook shutting down");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_interaction_mapping() {
        assert_eq!(
            match ResourceEventType::Created {
                ResourceEventType::Created => TriggerInteraction::Create,
                ResourceEventType::Updated => TriggerInteraction::Update,
                ResourceEventType::Deleted => TriggerInteraction::Delete,
            },
            TriggerInteraction::Create
        );

        assert_eq!(
            match ResourceEventType::Updated {
                ResourceEventType::Created => TriggerInteraction::Create,
                ResourceEventType::Updated => TriggerInteraction::Update,
                ResourceEventType::Deleted => TriggerInteraction::Delete,
            },
            TriggerInteraction::Update
        );

        assert_eq!(
            match ResourceEventType::Deleted {
                ResourceEventType::Created => TriggerInteraction::Create,
                ResourceEventType::Updated => TriggerInteraction::Update,
                ResourceEventType::Deleted => TriggerInteraction::Delete,
            },
            TriggerInteraction::Delete
        );
    }
}
