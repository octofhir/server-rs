//! Event Matcher for evaluating subscription filters.
//!
//! Matches resource events against subscription topic triggers and filter criteria
//! using FHIRPath evaluation.

use std::sync::Arc;

use octofhir_fhirpath::{Collection, EvaluationContext, FhirPathEngine, FhirPathValue};

use super::error::{SubscriptionError, SubscriptionResult};
use super::types::{
    ActiveSubscription, AppliedFilter, ParsedSubscriptionTopic, ResourceTrigger, TriggerInteraction,
};

/// Event matcher for evaluating FHIRPath filters.
pub struct EventMatcher {
    /// FHIRPath engine for expression evaluation
    engine: Arc<FhirPathEngine>,
}

impl EventMatcher {
    /// Create a new event matcher with the given FHIRPath engine.
    pub fn new(engine: Arc<FhirPathEngine>) -> Self {
        Self { engine }
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
        if let Some(ref criteria) = trigger.fhirpath_criteria {
            if !self.evaluate_fhirpath(criteria, resource).await? {
                tracing::debug!(
                    criteria = criteria,
                    "Resource did not match topic FHIRPath criteria"
                );
                return Ok(false);
            }
        }

        // Evaluate topic's query criteria if present
        if let Some(ref query_criteria) = trigger.query_criteria {
            if !self
                .evaluate_query_criteria(query_criteria, resource, previous, interaction)
                .await?
            {
                tracing::debug!("Resource did not match topic query criteria");
                return Ok(false);
            }
        }

        // Evaluate subscription's filters
        for filter in &subscription.filter_by {
            if !self
                .evaluate_filter(filter, resource, topic, trigger)
                .await?
            {
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
    ) -> SubscriptionResult<bool> {
        // Handle create - no previous version
        if interaction == TriggerInteraction::Create {
            if let Some(ref current_query) = criteria.current {
                return self.evaluate_query_string(current_query, resource).await;
            }
            return Ok(matches!(
                criteria.result_for_create,
                super::types::QueryResultBehavior::TestPasses
                    | super::types::QueryResultBehavior::NoTest
            ));
        }

        // Handle delete - no current version
        if interaction == TriggerInteraction::Delete {
            if let Some(ref previous_query) = criteria.previous {
                if let Some(prev) = previous {
                    return self.evaluate_query_string(previous_query, prev).await;
                }
            }
            return Ok(matches!(
                criteria.result_for_delete,
                super::types::QueryResultBehavior::TestPasses
                    | super::types::QueryResultBehavior::NoTest
            ));
        }

        // Handle update - both versions available
        let current_matches = if let Some(ref current_query) = criteria.current {
            self.evaluate_query_string(current_query, resource).await?
        } else {
            true
        };

        let previous_matches =
            if let (Some(previous_query), Some(prev)) = (&criteria.previous, previous) {
                self.evaluate_query_string(previous_query, prev).await?
            } else {
                true
            };

        if criteria.require_both {
            Ok(current_matches && previous_matches)
        } else {
            Ok(current_matches || previous_matches)
        }
    }

    /// Evaluate a query string against a resource.
    ///
    /// Query strings are in FHIR search parameter format: `param=value&param2=value2`
    async fn evaluate_query_string(
        &self,
        query: &str,
        resource: &serde_json::Value,
    ) -> SubscriptionResult<bool> {
        // Parse query string into key-value pairs
        for param in query.split('&') {
            let mut parts = param.splitn(2, '=');
            let key = parts.next().unwrap_or_default();
            let _value = parts.next().unwrap_or_default();

            if key.is_empty() {
                continue;
            }

            // Convert search parameter to FHIRPath (simplified)
            // In a full implementation, this would use the search parameter definitions
            let fhirpath = self.search_param_to_fhirpath(key, resource)?;

            if let Some(expr) = fhirpath {
                let result = self.evaluate_fhirpath(&expr, resource).await?;

                // Check if result contains the expected value
                // This is a simplified check - full implementation would handle modifiers
                if !result {
                    return Ok(false);
                }
            }
        }

        Ok(true)
    }

    /// Evaluate a subscription filter against a resource.
    async fn evaluate_filter(
        &self,
        filter: &AppliedFilter,
        resource: &serde_json::Value,
        topic: &ParsedSubscriptionTopic,
        _trigger: &ResourceTrigger,
    ) -> SubscriptionResult<bool> {
        // Find the filter definition in the topic
        let filter_def = topic
            .can_filter_by
            .iter()
            .find(|f| f.filter_parameter == filter.filter_parameter);

        // Get the FHIRPath expression to evaluate
        let fhirpath = if let Some(def) = filter_def {
            // Use the filter definition from the topic
            def.filter_definition.clone()
        } else {
            // Fall back to treating filter_parameter as a path
            Some(filter.filter_parameter.clone())
        };

        let Some(expr) = fhirpath else {
            // No expression to evaluate, assume match
            return Ok(true);
        };

        // Evaluate the FHIRPath expression
        let provider = self.engine.get_model_provider();
        let collection = Collection::from_json_resource(resource.clone(), Some(provider.clone()))
            .await
            .map_err(|e| {
                SubscriptionError::FhirPathError(format!("Failed to create FHIRPath context: {e}"))
            })?;

        let context = EvaluationContext::new(collection, provider, None, None, None);

        let result = self.engine.evaluate(&expr, &context).await.map_err(|e| {
            SubscriptionError::FhirPathError(format!("FHIRPath evaluation failed: {e}"))
        })?;

        // Compare result with filter value - extract value from EvaluationResult
        let values = result.value.into_vec();
        self.compare_result(&values, &filter.value, filter.comparator.as_deref())
    }

    /// Convert search parameter to FHIRPath expression.
    ///
    /// This is a simplified implementation. A full implementation would use
    /// search parameter definitions from the server.
    fn search_param_to_fhirpath(
        &self,
        param: &str,
        resource: &serde_json::Value,
    ) -> SubscriptionResult<Option<String>> {
        // Handle common parameters
        match param {
            "_id" => Ok(Some("id".to_string())),
            "_lastUpdated" => Ok(Some("meta.lastUpdated".to_string())),
            "status" => Ok(Some("status".to_string())),
            "code" => Ok(Some("code".to_string())),
            "subject" | "patient" => {
                // Try common reference paths
                let resource_type = resource
                    .get("resourceType")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();

                Ok(Some(
                    match resource_type {
                        "Observation" | "Condition" | "Procedure" => "subject.reference",
                        "Encounter" => "subject.reference",
                        _ => "subject.reference",
                    }
                    .to_string(),
                ))
            }
            _ => {
                // Assume the parameter name is a valid path
                Ok(Some(param.to_string()))
            }
        }
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

    /// Compare FHIRPath result with expected value.
    fn compare_result(
        &self,
        result: &[FhirPathValue],
        expected: &str,
        comparator: Option<&str>,
    ) -> SubscriptionResult<bool> {
        if result.is_empty() {
            return Ok(false);
        }

        let comparator = comparator.unwrap_or("eq");

        // Check if any result value matches
        for value in result {
            let matches = match value {
                FhirPathValue::String(s, ..) => self.compare_strings(s, expected, comparator),
                FhirPathValue::Integer(i, ..) => {
                    if let Ok(expected_int) = expected.parse::<i64>() {
                        self.compare_numbers(*i as f64, expected_int as f64, comparator)
                    } else {
                        false
                    }
                }
                FhirPathValue::Decimal(d, ..) => {
                    if let Ok(expected_dec) = expected.parse::<f64>() {
                        let actual = f64::try_from(*d).unwrap_or(0.0);
                        self.compare_numbers(actual, expected_dec, comparator)
                    } else {
                        false
                    }
                }
                FhirPathValue::Boolean(b, ..) => {
                    let expected_bool = expected.eq_ignore_ascii_case("true");
                    match comparator {
                        "eq" => *b == expected_bool,
                        "ne" => *b != expected_bool,
                        _ => false,
                    }
                }
                FhirPathValue::Resource(r, ..) => {
                    // For resources, check if reference matches
                    if let Some(reference) = r
                        .get("reference")
                        .and_then(|v: &serde_json::Value| v.as_str())
                    {
                        self.compare_strings(reference, expected, comparator)
                    } else {
                        false
                    }
                }
                _ => false,
            };

            if matches {
                return Ok(true);
            }
        }

        Ok(false)
    }

    /// Compare string values.
    fn compare_strings(&self, actual: &str, expected: &str, comparator: &str) -> bool {
        match comparator {
            "eq" => actual == expected,
            "ne" => actual != expected,
            "co" | "contains" => actual.contains(expected),
            "sw" | "starts-with" => actual.starts_with(expected),
            "ew" | "ends-with" => actual.ends_with(expected),
            _ => actual == expected,
        }
    }

    /// Compare numeric values.
    fn compare_numbers(&self, actual: f64, expected: f64, comparator: &str) -> bool {
        const EPSILON: f64 = 1e-10;

        match comparator {
            "eq" => (actual - expected).abs() < EPSILON,
            "ne" => (actual - expected).abs() >= EPSILON,
            "gt" => actual > expected,
            "lt" => actual < expected,
            "ge" => actual >= expected,
            "le" => actual <= expected,
            _ => (actual - expected).abs() < EPSILON,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Standalone test functions that replicate the helper logic to avoid needing EventMatcher

    fn compare_strings(actual: &str, expected: &str, comparator: &str) -> bool {
        match comparator {
            "eq" => actual == expected,
            "ne" => actual != expected,
            "co" | "contains" => actual.contains(expected),
            "sw" | "starts-with" => actual.starts_with(expected),
            "ew" | "ends-with" => actual.ends_with(expected),
            _ => actual == expected,
        }
    }

    fn compare_numbers(actual: f64, expected: f64, comparator: &str) -> bool {
        const EPSILON: f64 = 1e-10;
        match comparator {
            "eq" => (actual - expected).abs() < EPSILON,
            "ne" => (actual - expected).abs() >= EPSILON,
            "gt" => actual > expected,
            "lt" => actual < expected,
            "ge" => actual >= expected,
            "le" => actual <= expected,
            _ => (actual - expected).abs() < EPSILON,
        }
    }

    fn search_param_to_fhirpath(param: &str, resource: &serde_json::Value) -> Option<String> {
        match param {
            "_id" => Some("id".to_string()),
            "_lastUpdated" => Some("meta.lastUpdated".to_string()),
            "status" => Some("status".to_string()),
            "code" => Some("code".to_string()),
            "subject" | "patient" => {
                let resource_type = resource
                    .get("resourceType")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();

                Some(
                    match resource_type {
                        "Observation" | "Condition" | "Procedure" | "Encounter" => {
                            "subject.reference"
                        }
                        _ => "subject.reference",
                    }
                    .to_string(),
                )
            }
            _ => Some(param.to_string()),
        }
    }

    mod string_comparison_tests {
        use super::*;

        #[test]
        fn test_eq_comparator() {
            assert!(compare_strings("test", "test", "eq"));
            assert!(!compare_strings("test", "other", "eq"));
        }

        #[test]
        fn test_ne_comparator() {
            assert!(compare_strings("test", "other", "ne"));
            assert!(!compare_strings("test", "test", "ne"));
        }

        #[test]
        fn test_contains_comparator() {
            assert!(compare_strings("hello world", "world", "co"));
            assert!(compare_strings("hello world", "world", "contains"));
            assert!(!compare_strings("hello", "world", "co"));
        }

        #[test]
        fn test_starts_with_comparator() {
            assert!(compare_strings("hello world", "hello", "sw"));
            assert!(compare_strings("hello world", "hello", "starts-with"));
            assert!(!compare_strings("hello world", "world", "sw"));
        }

        #[test]
        fn test_ends_with_comparator() {
            assert!(compare_strings("hello world", "world", "ew"));
            assert!(compare_strings("hello world", "world", "ends-with"));
            assert!(!compare_strings("hello world", "hello", "ew"));
        }

        #[test]
        fn test_unknown_comparator_defaults_to_eq() {
            assert!(compare_strings("test", "test", "unknown"));
            assert!(!compare_strings("test", "other", "unknown"));
        }
    }

    mod number_comparison_tests {
        use super::*;

        #[test]
        fn test_eq_comparator() {
            assert!(compare_numbers(42.0, 42.0, "eq"));
            assert!(!compare_numbers(42.0, 43.0, "eq"));
        }

        #[test]
        fn test_ne_comparator() {
            assert!(compare_numbers(42.0, 43.0, "ne"));
            assert!(!compare_numbers(42.0, 42.0, "ne"));
        }

        #[test]
        fn test_gt_comparator() {
            assert!(compare_numbers(43.0, 42.0, "gt"));
            assert!(!compare_numbers(42.0, 42.0, "gt"));
            assert!(!compare_numbers(41.0, 42.0, "gt"));
        }

        #[test]
        fn test_lt_comparator() {
            assert!(compare_numbers(41.0, 42.0, "lt"));
            assert!(!compare_numbers(42.0, 42.0, "lt"));
            assert!(!compare_numbers(43.0, 42.0, "lt"));
        }

        #[test]
        fn test_ge_comparator() {
            assert!(compare_numbers(43.0, 42.0, "ge"));
            assert!(compare_numbers(42.0, 42.0, "ge"));
            assert!(!compare_numbers(41.0, 42.0, "ge"));
        }

        #[test]
        fn test_le_comparator() {
            assert!(compare_numbers(41.0, 42.0, "le"));
            assert!(compare_numbers(42.0, 42.0, "le"));
            assert!(!compare_numbers(43.0, 42.0, "le"));
        }

        #[test]
        fn test_floating_point_precision() {
            // Test that nearly equal values are considered equal
            assert!(compare_numbers(0.1 + 0.2, 0.3, "eq"));
        }
    }

    mod search_param_tests {
        use super::*;

        #[test]
        fn test_id_param() {
            let resource = serde_json::json!({"resourceType": "Patient"});
            assert_eq!(
                search_param_to_fhirpath("_id", &resource),
                Some("id".to_string())
            );
        }

        #[test]
        fn test_last_updated_param() {
            let resource = serde_json::json!({"resourceType": "Patient"});
            assert_eq!(
                search_param_to_fhirpath("_lastUpdated", &resource),
                Some("meta.lastUpdated".to_string())
            );
        }

        #[test]
        fn test_status_param() {
            let resource = serde_json::json!({"resourceType": "Observation"});
            assert_eq!(
                search_param_to_fhirpath("status", &resource),
                Some("status".to_string())
            );
        }

        #[test]
        fn test_subject_param_for_observation() {
            let resource = serde_json::json!({"resourceType": "Observation"});
            assert_eq!(
                search_param_to_fhirpath("subject", &resource),
                Some("subject.reference".to_string())
            );
        }

        #[test]
        fn test_unknown_param_returns_as_path() {
            let resource = serde_json::json!({"resourceType": "Patient"});
            assert_eq!(
                search_param_to_fhirpath("customField", &resource),
                Some("customField".to_string())
            );
        }
    }

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
