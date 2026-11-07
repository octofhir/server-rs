//! Event Matcher for evaluating subscription filters.
//!
//! Matches resource events against subscription topic triggers and filter criteria
//! using FHIRPath evaluation.

use std::sync::Arc;

use octofhir_fhirpath::{Collection, EvaluationContext, FhirPathEngine, FhirPathValue};

use octofhir_search::ReloadableSearchConfig;

use super::error::{SubscriptionError, SubscriptionResult};
use super::search_match::SearchMatcher;
use super::types::{
    ActiveSubscription, AppliedFilter, ParsedSubscriptionTopic, TriggerInteraction,
};

/// Event matcher for evaluating FHIRPath filters.
pub struct EventMatcher {
    /// FHIRPath engine for expression evaluation
    engine: Arc<FhirPathEngine>,
    /// Search criteria in memory: subscription filters and topic query criteria.
    search: SearchMatcher,
}

impl EventMatcher {
    /// Create an event matcher: FHIRPath for topic criteria, the server's search parameters for
    /// filters and query criteria.
    pub fn new(engine: Arc<FhirPathEngine>, search: ReloadableSearchConfig) -> Self {
        Self {
            search: SearchMatcher::new(engine.clone(), search),
            engine,
        }
    }

    /// The search criteria matcher, also used to check filters when a subscription is created.
    pub fn search(&self) -> &SearchMatcher {
        &self.search
    }

    /// Check if a resource matches a subscription's filters.
    ///
    /// Returns true if the resource matches all applicable filters.
    pub async fn matches(
        &self,
        resource: &serde_json::Value,
        previous: Option<&serde_json::Value>,
        topic: &ParsedSubscriptionTopic,
        subscription: &ActiveSubscription,
        interaction: TriggerInteraction,
    ) -> SubscriptionResult<bool> {
        // Get the resource type
        let resource_type = resource
            .get("resourceType")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                SubscriptionError::ValidationError("Resource missing resourceType".to_string())
            })?;

        // Find matching trigger for this resource type and interaction
        let trigger = topic.resource_triggers.iter().find(|t| {
            t.resource_type == resource_type && t.supported_interactions.contains(&interaction)
        });

        let Some(trigger) = trigger else {
            // No matching trigger for this resource type/interaction
            return Ok(false);
        };

        // Evaluate topic's FHIRPath criteria if present
        if let Some(ref criteria) = trigger.fhirpath_criteria
            && !self.evaluate_fhirpath(criteria, resource).await?
        {
            tracing::debug!(
                criteria = criteria,
                "Resource did not match topic FHIRPath criteria"
            );
            return Ok(false);
        }

        // Evaluate topic's query criteria if present
        if let Some(ref query_criteria) = trigger.query_criteria
            && !self
                .evaluate_query_criteria(query_criteria, resource, previous, interaction, topic)
                .await?
        {
            tracing::debug!("Resource did not match topic query criteria");
            return Ok(false);
        }

        // Evaluate subscription's filters: those for another resource type do not apply.
        for filter in subscription.filter_by.iter().filter(|f| {
            f.resource_type
                .as_deref()
                .is_none_or(|t| t == resource_type)
        }) {
            if !self.evaluate_filter(filter, resource, topic).await? {
                tracing::debug!(
                    filter = filter.filter_parameter,
                    value = filter.value,
                    "Resource did not match subscription filter"
                );
                return Ok(false);
            }
        }

        Ok(true)
    }

    /// Evaluate a FHIRPath expression against a resource.
    ///
    /// Returns true if the expression evaluates to a truthy value.
    pub async fn evaluate_fhirpath(
        &self,
        expression: &str,
        resource: &serde_json::Value,
    ) -> SubscriptionResult<bool> {
        // Create evaluation context
        let provider = self.engine.get_model_provider();
        let collection = Collection::from_json_resource(resource.clone(), Some(provider.clone()))
            .await
            .map_err(|e| {
                SubscriptionError::FhirPathError(format!("Failed to create FHIRPath context: {e}"))
            })?;

        let context = EvaluationContext::new(collection, provider, None, None, None);

        // Evaluate expression
        let result = self
            .engine
            .evaluate(expression, &context)
            .await
            .map_err(|e| {
                SubscriptionError::FhirPathError(format!("FHIRPath evaluation failed: {e}"))
            })?;

        // Convert result to boolean - extract value from EvaluationResult
        let values = result.value.into_vec();
        Ok(self.result_to_bool(&values))
    }

    /// Evaluate query criteria against current and/or previous resource.
    async fn evaluate_query_criteria(
        &self,
        criteria: &super::types::QueryCriteria,
        resource: &serde_json::Value,
        previous: Option<&serde_json::Value>,
        interaction: TriggerInteraction,
        topic: &ParsedSubscriptionTopic,
    ) -> SubscriptionResult<bool> {
        // Handle create - no previous version
        if interaction == TriggerInteraction::Create {
            if let Some(ref current_query) = criteria.current {
                return self
                    .evaluate_query_string(current_query, resource, topic)
                    .await;
            }
            return Ok(matches!(
                criteria.result_for_create,
                super::types::QueryResultBehavior::TestPasses
                    | super::types::QueryResultBehavior::NoTest
            ));
        }

        // Handle delete - no current version
        if interaction == TriggerInteraction::Delete {
            if let Some(ref previous_query) = criteria.previous
                && let Some(prev) = previous
            {
                return self
                    .evaluate_query_string(previous_query, prev, topic)
                    .await;
            }
            return Ok(matches!(
                criteria.result_for_delete,
                super::types::QueryResultBehavior::TestPasses
                    | super::types::QueryResultBehavior::NoTest
            ));
        }

        // Handle update - both versions available
        let current_matches = if let Some(ref current_query) = criteria.current {
            self.evaluate_query_string(current_query, resource, topic)
                .await?
        } else {
            true
        };

        let previous_matches =
            if let (Some(previous_query), Some(prev)) = (&criteria.previous, previous) {
                self.evaluate_query_string(previous_query, prev, topic)
                    .await?
            } else {
                true
            };

        if criteria.require_both {
            Ok(current_matches && previous_matches)
        } else {
            Ok(current_matches || previous_matches)
        }
    }

    /// Whether a resource matches a query string in FHIR search form: `key=value&key2=value2`,
    /// optionally after `Type?`.
    async fn evaluate_query_string(
        &self,
        query: &str,
        resource: &serde_json::Value,
        topic: &ParsedSubscriptionTopic,
    ) -> SubscriptionResult<bool> {
        let query = query.split_once('?').map_or(query, |(_, query)| query);
        for param in query.split('&').filter(|p| !p.is_empty()) {
            let (key, value) = param.split_once('=').unwrap_or((param, ""));
            let (key, modifier) = match key.split_once(':') {
                Some((key, modifier)) => (key, Some(modifier)),
                None => (key, None),
            };
            if !self
                .search
                .matches(Some(topic), resource, key, modifier, value)
                .await?
            {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Whether a resource passes one subscription filter. An R5 comparator becomes the search
    /// value's prefix (`gt` + `5` is `gt5`).
    async fn evaluate_filter(
        &self,
        filter: &AppliedFilter,
        resource: &serde_json::Value,
        topic: &ParsedSubscriptionTopic,
    ) -> SubscriptionResult<bool> {
        let value = match filter.comparator.as_deref() {
            Some(comparator) if comparator != "eq" => format!("{comparator}{}", filter.value),
            _ => filter.value.clone(),
        };
        self.search
            .matches(
                Some(topic),
                resource,
                &filter.filter_parameter,
                filter.modifier.as_deref(),
                &value,
            )
            .await
    }

    /// Convert FHIRPath result to boolean.
    fn result_to_bool(&self, result: &[FhirPathValue]) -> bool {
        if result.is_empty() {
            return false;
        }

        // Check if any value is truthy
        result.iter().any(|v| match v {
            FhirPathValue::Boolean(b, ..) => *b,
            FhirPathValue::String(s, ..) => !s.is_empty(),
            FhirPathValue::Integer(i, ..) => *i != 0,
            FhirPathValue::Decimal(d, ..) => !d.is_zero(),
            FhirPathValue::Resource(..) => true,
            _ => true,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod trigger_interaction_tests {
        use super::*;

        #[test]
        fn test_as_str() {
            assert_eq!(TriggerInteraction::Create.as_str(), "create");
            assert_eq!(TriggerInteraction::Update.as_str(), "update");
            assert_eq!(TriggerInteraction::Delete.as_str(), "delete");
        }

        #[test]
        fn test_from_str() {
            assert_eq!(
                TriggerInteraction::from("create"),
                TriggerInteraction::Create
            );
            assert_eq!(
                TriggerInteraction::from("CREATE"),
                TriggerInteraction::Create
            );
            assert_eq!(
                TriggerInteraction::from("update"),
                TriggerInteraction::Update
            );
            assert_eq!(
                TriggerInteraction::from("delete"),
                TriggerInteraction::Delete
            );
        }
    }
}
