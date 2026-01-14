//! Subscription Manager for subscription lifecycle management.
//!
//! Handles subscription CRUD operations, status transitions, and querying
//! active subscriptions for event matching.

use std::sync::Arc;

use octofhir_storage::{FhirStorage, SearchParams};
use sqlx_core::query::query;
use sqlx_postgres::PgPool;
use time::OffsetDateTime;

use super::error::{SubscriptionError, SubscriptionResult};
use super::types::{
    ActiveSubscription, AppliedFilter, PayloadContent, SubscriptionChannel, SubscriptionStatus,
};

/// Manager for subscription lifecycle and querying.
pub struct SubscriptionManager {
    /// FHIR storage for Subscription resources
    storage: Arc<dyn FhirStorage>,

    /// Database pool for subscription status updates
    db_pool: PgPool,
}

impl SubscriptionManager {
    /// Create a new subscription manager.
    pub fn new(storage: Arc<dyn FhirStorage>, db_pool: PgPool) -> Self {
        Self { storage, db_pool }
    }

    /// Get all active subscriptions for a given topic URL.
    pub async fn get_subscriptions_for_topic(
        &self,
        topic_url: &str,
    ) -> SubscriptionResult<Vec<ActiveSubscription>> {
        // Search for subscriptions with matching topic and active status
        // Note: In R4 with Backport IG, the criteria field contains the topic URL
        // In R5, there's a dedicated topic element
        let mut parameters = std::collections::HashMap::new();
        parameters.insert("status".to_string(), vec!["active".to_string()]);

        let params = SearchParams {
            parameters,
            count: Some(1000), // Reasonable limit
            ..Default::default()
        };

        let result = self
            .storage
            .search("Subscription", &params)
            .await
            .map_err(|e| SubscriptionError::Storage(e.to_string()))?;

        let mut subscriptions = Vec::new();

        for stored in result.entries {
            match self.parse_subscription(&stored.resource) {
                Ok(sub) => {
                    // Filter by topic URL
                    if sub.topic_url == topic_url && sub.status == SubscriptionStatus::Active {
                        subscriptions.push(sub);
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        id = stored.id,
                        error = %e,
                        "Failed to parse Subscription, skipping"
                    );
                }
            }
        }

        Ok(subscriptions)
    }

    /// Get a subscription by ID.
    pub async fn get_subscription(
        &self,
        id: &str,
    ) -> SubscriptionResult<Option<ActiveSubscription>> {
        let stored = self
            .storage
            .read("Subscription", id)
            .await
            .map_err(|e| SubscriptionError::Storage(e.to_string()))?;

        match stored {
            Some(s) => Ok(Some(self.parse_subscription(&s.resource)?)),
            None => Ok(None),
        }
    }

    /// Activate a subscription (transition from requested to active).
    pub async fn activate(&self, subscription_id: &str) -> SubscriptionResult<()> {
        // Read current subscription
        let stored = self
            .storage
            .read("Subscription", subscription_id)
            .await
            .map_err(|e| SubscriptionError::Storage(e.to_string()))?
            .ok_or_else(|| SubscriptionError::SubscriptionNotFound(subscription_id.to_string()))?;

        let mut resource = stored.resource;

        // Update status
        resource["status"] = serde_json::json!("active");

        // Update in storage
        self.storage
            .update(&resource, None)
            .await
            .map_err(|e| SubscriptionError::Storage(e.to_string()))?;

        // Initialize subscription status in our tracking table
        query(
            r#"
            INSERT INTO subscription_status (subscription_id, topic_url, created_at, updated_at)
            VALUES ($1, $2, NOW(), NOW())
            ON CONFLICT (subscription_id) DO UPDATE
            SET updated_at = NOW()
            "#,
        )
        .bind(subscription_id)
        .bind(
            resource
                .get("criteria")
                .and_then(|v| v.as_str())
                .unwrap_or(""),
        )
        .execute(&self.db_pool)
        .await?;

        tracing::info!(id = subscription_id, "Subscription activated");

        Ok(())
    }

    /// Set subscription to error state.
    pub async fn set_error(&self, subscription_id: &str, error: &str) -> SubscriptionResult<()> {
        // Read current subscription
        let stored = self
            .storage
            .read("Subscription", subscription_id)
            .await
            .map_err(|e| SubscriptionError::Storage(e.to_string()))?
            .ok_or_else(|| SubscriptionError::SubscriptionNotFound(subscription_id.to_string()))?;

        let mut resource = stored.resource;

        // Update status and error
        resource["status"] = serde_json::json!("error");
        resource["error"] = serde_json::json!(error);

        // Update in storage
        self.storage
            .update(&resource, None)
            .await
            .map_err(|e| SubscriptionError::Storage(e.to_string()))?;

        tracing::warn!(
            id = subscription_id,
            error = error,
            "Subscription set to error state"
        );

        Ok(())
    }

    /// Parse a Subscription FHIR resource into our internal representation.
    fn parse_subscription(
        &self,
        resource: &serde_json::Value,
    ) -> SubscriptionResult<ActiveSubscription> {
        let id = resource
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| SubscriptionError::InvalidSubscription("Missing id".to_string()))?
            .to_string();

        // Topic URL can be in different places depending on FHIR version:
        // - R5: subscription.topic
        // - R4 Backport: subscription.criteria (URL format)
        let topic_url = resource
            .get("topic")
            .and_then(|v| v.as_str())
            .or_else(|| resource.get("criteria").and_then(|v| v.as_str()))
            .ok_or_else(|| {
                SubscriptionError::InvalidSubscription("Missing topic or criteria".to_string())
            })?
            .to_string();

        let status = resource
            .get("status")
            .and_then(|v| v.as_str())
            .map(SubscriptionStatus::from)
            .unwrap_or_default();

        // Parse channel configuration
        let channel = self.parse_channel(resource)?;

        // Parse filters (filterBy in R5, extension in R4 Backport)
        let filter_by = self.parse_filters(resource);

        // Parse end time
        let end_time = resource.get("end").and_then(|v| v.as_str()).and_then(|s| {
            OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339).ok()
        });

        // Parse heartbeat period (from channel.heartbeatPeriod or extension)
        let heartbeat_period = resource
            .get("channel")
            .and_then(|c| c.get("heartbeatPeriod"))
            .and_then(|v| v.as_u64())
            .map(|v| v as u32);

        // Parse max count
        let max_count = resource
            .get("maxCount")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32);

        // Parse contact
        let contact = resource
            .get("contact")
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
            .and_then(|c| c.get("value"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        Ok(ActiveSubscription {
            id,
            topic_url,
            status,
            channel,
            filter_by,
            end_time,
            heartbeat_period,
            max_count,
            contact,
        })
    }

    /// Parse channel configuration from Subscription resource.
    fn parse_channel(
        &self,
        resource: &serde_json::Value,
    ) -> SubscriptionResult<SubscriptionChannel> {
        let channel = resource
            .get("channel")
            .ok_or_else(|| SubscriptionError::InvalidSubscription("Missing channel".to_string()))?;

        // R5: channel.type.coding[0].code
        // R4: channel.type
        let channel_type = channel
            .get("type")
            .and_then(|t| {
                // Try R5 format first
                t.get("coding")
                    .and_then(|c| c.as_array())
                    .and_then(|arr| arr.first())
                    .and_then(|c| c.get("code"))
                    .and_then(|v| v.as_str())
                    // Fall back to R4 format
                    .or_else(|| t.as_str())
            })
            .unwrap_or("rest-hook");

        let endpoint = channel
            .get("endpoint")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();

        match channel_type {
            "rest-hook" => {
                // Parse headers
                let headers: Vec<(String, String)> = channel
                    .get("header")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|h| {
                                let s = h.as_str()?;
                                let mut parts = s.splitn(2, ':');
                                let key = parts.next()?.trim().to_string();
                                let value = parts.next()?.trim().to_string();
                                Some((key, value))
                            })
                            .collect()
                    })
                    .unwrap_or_default();

                // Parse payload content
                let payload_content = channel
                    .get("payload")
                    .and_then(|v| v.as_str())
                    .map(PayloadContent::from)
                    .unwrap_or_default();

                // Parse content type
                let content_type = channel
                    .get("payload")
                    .and_then(|p| {
                        // R5: payload.contentType
                        p.get("contentType").and_then(|v| v.as_str())
                    })
                    .or_else(|| {
                        // R4: header with Content-Type
                        headers
                            .iter()
                            .find(|(k, _)| k.eq_ignore_ascii_case("content-type"))
                            .map(|(_, v)| v.as_str())
                    })
                    .unwrap_or("application/fhir+json")
                    .to_string();

                Ok(SubscriptionChannel::RestHook {
                    endpoint,
                    headers,
                    payload_content,
                    content_type,
                })
            }
            "websocket" => {
                let heartbeat_period = channel
                    .get("heartbeatPeriod")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as u32)
                    .unwrap_or(60); // Default 60 seconds

                Ok(SubscriptionChannel::WebSocket { heartbeat_period })
            }
            "email" => Ok(SubscriptionChannel::Email { address: endpoint }),
            "message" => Ok(SubscriptionChannel::Message { endpoint }),
            _ => Err(SubscriptionError::InvalidSubscription(format!(
                "Unsupported channel type: {channel_type}"
            ))),
        }
    }

    /// Parse filter criteria from Subscription resource.
    fn parse_filters(&self, resource: &serde_json::Value) -> Vec<AppliedFilter> {
        // R5: filterBy array
        if let Some(filter_by) = resource.get("filterBy").and_then(|v| v.as_array()) {
            return filter_by
                .iter()
                .filter_map(|f| {
                    let filter_parameter = f.get("filterParameter").and_then(|v| v.as_str())?;
                    let value = f.get("value").and_then(|v| v.as_str())?;

                    Some(AppliedFilter {
                        filter_parameter: filter_parameter.to_string(),
                        comparator: f
                            .get("comparator")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string()),
                        modifier: f
                            .get("modifier")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string()),
                        value: value.to_string(),
                    })
                })
                .collect();
        }

        // R4 Backport: Parse from criteria URL query string
        // e.g., "http://example.org/topic/patient-admission?patient=Patient/123"
        if let Some(criteria) = resource.get("criteria").and_then(|v| v.as_str()) {
            if let Some(query_start) = criteria.find('?') {
                let query = &criteria[query_start + 1..];
                return query
                    .split('&')
                    .filter_map(|param| {
                        let mut parts = param.splitn(2, '=');
                        let key = parts.next()?;
                        let value = parts.next()?;

                        Some(AppliedFilter {
                            filter_parameter: key.to_string(),
                            comparator: None,
                            modifier: None,
                            value: value.to_string(),
                        })
                    })
                    .collect();
            }
        }

        Vec::new()
    }
}
