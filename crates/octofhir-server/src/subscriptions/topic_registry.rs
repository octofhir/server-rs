//! Topic Registry for efficient subscription topic lookup.
//!
//! The registry maintains an in-memory cache of parsed `SubscriptionTopic` resources
//! for fast event matching. It syncs with FHIR storage and reloads on changes.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use octofhir_storage::{FhirStorage, SearchParams};
use parking_lot::RwLock;

use super::error::{SubscriptionError, SubscriptionResult};
use super::types::{
    FilterDefinition, NotificationShape, ParsedSubscriptionTopic, QueryCriteria,
    QueryResultBehavior, ResourceTrigger, TopicStatus, TriggerInteraction,
};

/// Registry of subscription topics with in-memory caching.
///
/// Provides fast lookup of topics by resource type and interaction for event matching.
pub struct TopicRegistry {
    /// Storage for loading topics
    storage: Arc<dyn FhirStorage>,

    /// Cached topics by URL
    topics: RwLock<HashMap<String, ParsedSubscriptionTopic>>,

    /// Index: resource_type -> Set of topic URLs
    type_index: RwLock<HashMap<String, HashSet<String>>>,
}

impl TopicRegistry {
    /// Create a new topic registry.
    pub fn new(storage: Arc<dyn FhirStorage>) -> Self {
        Self {
            storage,
            topics: RwLock::new(HashMap::new()),
            type_index: RwLock::new(HashMap::new()),
        }
    }

    /// Reload all topics from storage.
    pub async fn reload(&self) -> SubscriptionResult<()> {
        tracing::debug!("Reloading subscription topics from storage");

        // Search for all active SubscriptionTopic resources
        let mut parameters = std::collections::HashMap::new();
        parameters.insert("status".to_string(), vec!["active".to_string()]);

        let params = SearchParams {
            parameters,
            count: Some(1000), // Reasonable limit
            ..Default::default()
        };

        let result = self
            .storage
            .search("SubscriptionTopic", &params)
            .await
            .map_err(|e| SubscriptionError::Storage(e.to_string()))?;

        let mut topics = HashMap::new();
        let mut type_index: HashMap<String, HashSet<String>> = HashMap::new();

        for stored in result.entries {
            match self.parse_topic(&stored.resource) {
                Ok(topic) => {
                    // Update type index
                    for trigger in &topic.resource_triggers {
                        type_index
                            .entry(trigger.resource_type.clone())
                            .or_default()
                            .insert(topic.url.clone());
                    }

                    topics.insert(topic.url.clone(), topic);
                }
                Err(e) => {
                    tracing::warn!(
                        id = stored.id,
                        error = %e,
                        "Failed to parse SubscriptionTopic, skipping"
                    );
                }
            }
        }

        tracing::info!(count = topics.len(), "Loaded subscription topics");

        // Update caches
        *self.topics.write() = topics;
        *self.type_index.write() = type_index;

        Ok(())
    }

    /// Get the number of cached topics.
    pub fn topic_count(&self) -> usize {
        self.topics.read().len()
    }

    /// Find topics that match a resource type and interaction.
    pub fn find_matching_topics(
        &self,
        resource_type: &str,
        interaction: TriggerInteraction,
    ) -> Vec<ParsedSubscriptionTopic> {
        let type_index = self.type_index.read();
        let topics = self.topics.read();

        // Get topic URLs for this resource type
        let Some(topic_urls) = type_index.get(resource_type) else {
            return Vec::new();
        };

        // Filter topics that support this interaction
        topic_urls
            .iter()
            .filter_map(|url| topics.get(url))
            .filter(|topic| {
                topic.resource_triggers.iter().any(|trigger| {
                    trigger.resource_type == resource_type
                        && trigger.supported_interactions.contains(&interaction)
                })
            })
            .cloned()
            .collect()
    }

    /// Get a topic by URL.
    pub fn get_topic(&self, url: &str) -> Option<ParsedSubscriptionTopic> {
        self.topics.read().get(url).cloned()
    }

    /// Get a topic by ID.
    pub fn get_topic_by_id(&self, id: &str) -> Option<ParsedSubscriptionTopic> {
        self.topics.read().values().find(|t| t.id == id).cloned()
    }

    /// Parse a SubscriptionTopic FHIR resource into our internal representation.
    fn parse_topic(
        &self,
        resource: &serde_json::Value,
    ) -> SubscriptionResult<ParsedSubscriptionTopic> {
        let id = resource
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| SubscriptionError::InvalidTopic("Missing id".to_string()))?
            .to_string();

        let url = resource
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| SubscriptionError::InvalidTopic("Missing url".to_string()))?
            .to_string();

        let title = resource
            .get("title")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let status = resource
            .get("status")
            .and_then(|v| v.as_str())
            .map(TopicStatus::from)
            .unwrap_or_default();

        // Parse resource triggers
        let resource_triggers = self.parse_resource_triggers(resource)?;

        // Parse filter definitions (canFilterBy)
        let can_filter_by = self.parse_filter_definitions(resource);

        // Parse notification shape
        let notification_shape = self.parse_notification_shape(resource);

        Ok(ParsedSubscriptionTopic {
            id,
            url,
            title,
            status,
            resource_triggers,
            event_triggers: vec![], // TODO: Parse event triggers if needed
            can_filter_by,
            notification_shape,
        })
    }

    /// Parse resourceTrigger array from SubscriptionTopic.
    fn parse_resource_triggers(
        &self,
        resource: &serde_json::Value,
    ) -> SubscriptionResult<Vec<ResourceTrigger>> {
        let triggers = resource
            .get("resourceTrigger")
            .and_then(|v| v.as_array())
            .map(|arr| arr.as_slice())
            .unwrap_or(&[]);

        let mut result = Vec::new();

        for trigger in triggers {
            let resource_type = trigger
                .get("resource")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();

            if resource_type.is_empty() {
                continue;
            }

            // Parse supported interactions
            let supported_interactions = trigger
                .get("supportedInteraction")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str())
                        .map(TriggerInteraction::from)
                        .collect()
                })
                .unwrap_or_else(|| {
                    // Default to all interactions if not specified
                    vec![
                        TriggerInteraction::Create,
                        TriggerInteraction::Update,
                        TriggerInteraction::Delete,
                    ]
                });

            // Parse FHIRPath criteria
            let fhirpath_criteria = trigger
                .get("fhirPathCriteria")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            // Parse query criteria
            let query_criteria = trigger.get("queryCriteria").map(|qc| QueryCriteria {
                previous: qc
                    .get("previous")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                current: qc
                    .get("current")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                result_for_create: qc
                    .get("resultForCreate")
                    .and_then(|v| v.as_str())
                    .map(|s| match s {
                        "test-passes" => QueryResultBehavior::TestPasses,
                        "test-fails" => QueryResultBehavior::TestFails,
                        "no-test" => QueryResultBehavior::NoTest,
                        _ => QueryResultBehavior::default(),
                    })
                    .unwrap_or_default(),
                result_for_delete: qc
                    .get("resultForDelete")
                    .and_then(|v| v.as_str())
                    .map(|s| match s {
                        "test-passes" => QueryResultBehavior::TestPasses,
                        "test-fails" => QueryResultBehavior::TestFails,
                        "no-test" => QueryResultBehavior::NoTest,
                        _ => QueryResultBehavior::default(),
                    })
                    .unwrap_or_default(),
                require_both: qc
                    .get("requireBoth")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
            });

            let description = trigger
                .get("description")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            result.push(ResourceTrigger {
                resource_type,
                supported_interactions,
                fhirpath_criteria,
                query_criteria,
                description,
            });
        }

        Ok(result)
    }

    /// Parse canFilterBy array from SubscriptionTopic.
    fn parse_filter_definitions(&self, resource: &serde_json::Value) -> Vec<FilterDefinition> {
        let filters = resource
            .get("canFilterBy")
            .and_then(|v| v.as_array())
            .map(|arr| arr.as_slice())
            .unwrap_or(&[]);

        filters
            .iter()
            .filter_map(|filter| {
                let filter_parameter = filter
                    .get("filterParameter")
                    .and_then(|v| v.as_str())?
                    .to_string();

                Some(FilterDefinition {
                    filter_parameter,
                    description: filter
                        .get("description")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
                    resource: filter
                        .get("resource")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
                    filter_definition: filter
                        .get("filterDefinition")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
                    comparators: filter
                        .get("comparator")
                        .and_then(|v| v.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                                .collect()
                        })
                        .unwrap_or_default(),
                    modifiers: filter
                        .get("modifier")
                        .and_then(|v| v.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                                .collect()
                        })
                        .unwrap_or_default(),
                })
            })
            .collect()
    }

    /// Parse notificationShape array from SubscriptionTopic.
    fn parse_notification_shape(&self, resource: &serde_json::Value) -> Vec<NotificationShape> {
        let shapes = resource
            .get("notificationShape")
            .and_then(|v| v.as_array())
            .map(|arr| arr.as_slice())
            .unwrap_or(&[]);

        shapes
            .iter()
            .filter_map(|shape| {
                let resource_type = shape.get("resource").and_then(|v| v.as_str())?.to_string();

                Some(NotificationShape {
                    resource: resource_type,
                    include: shape
                        .get("include")
                        .and_then(|v| v.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                                .collect()
                        })
                        .unwrap_or_default(),
                    rev_include: shape
                        .get("revInclude")
                        .and_then(|v| v.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                                .collect()
                        })
                        .unwrap_or_default(),
                })
            })
            .collect()
    }
}
