//! Automation dispatcher hook.
//!
//! This hook listens for resource events and dispatches matching automations for execution.

use std::sync::Arc;

use super::types::AutomationEvent;
use async_trait::async_trait;
use octofhir_core::events::{HookError, ResourceEvent, ResourceEventType, ResourceHook};
use octofhir_fhirpath::{Collection, EvaluationContext, FhirPathEngine, FhirPathValue};
use tracing::{debug, info, warn};

use super::executor::AutomationExecutor;
use super::storage::AutomationStorage;

/// Hook that dispatches automations when resources are created/updated/deleted.
///
/// When a resource event occurs, this hook:
/// 1. Queries for active automations with matching triggers
/// 2. Optionally evaluates FHIRPath filters
/// 3. Executes matching automations with the event context
///
/// Automation execution is asynchronous - the hook returns immediately while
/// automations run in the background.
pub struct AutomationDispatcherHook {
    automation_storage: Arc<dyn AutomationStorage>,
    executor: Arc<AutomationExecutor>,
    fhirpath_engine: Arc<FhirPathEngine>,
}

impl AutomationDispatcherHook {
    /// Create a new automation dispatcher hook
    pub fn new(
        automation_storage: Arc<dyn AutomationStorage>,
        executor: Arc<AutomationExecutor>,
        fhirpath_engine: Arc<FhirPathEngine>,
    ) -> Self {
        Self {
            automation_storage,
            executor,
            fhirpath_engine,
        }
    }

    /// Evaluate a FHIRPath filter expression against a resource.
    ///
    /// Returns true if the expression evaluates to a truthy value (non-empty result).
    async fn evaluate_fhirpath_filter(
        &self,
        filter: &str,
        resource: &serde_json::Value,
    ) -> Result<bool, String> {
        // Create evaluation context
        let provider = self.fhirpath_engine.get_model_provider();
        let collection = Collection::from_json_resource(resource.clone(), Some(provider.clone()))
            .await
            .map_err(|e| format!("Failed to create FHIRPath context: {e}"))?;

        let context = EvaluationContext::new(collection, provider, None, None, None);

        // Evaluate expression
        let result = self
            .fhirpath_engine
            .evaluate(filter, &context)
            .await
            .map_err(|e| format!("FHIRPath evaluation failed: {e}"))?;

        // Convert result to boolean - non-empty and truthy = true
        let values = result.value.into_vec();
        Ok(Self::result_to_bool(&values))
    }

    /// Convert FHIRPath result to boolean.
    fn result_to_bool(result: &[FhirPathValue]) -> bool {
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

#[async_trait]
impl ResourceHook for AutomationDispatcherHook {
    fn name(&self) -> &str {
        "automation_dispatcher"
    }

    fn resource_types(&self) -> &[&str] {
        // Match all resource types - we'll filter by automation triggers
        &[]
    }

    fn matches(&self, _event: &ResourceEvent) -> bool {
        // Always match - we'll check for matching automations in handle()
        true
    }

    async fn handle(&self, event: &ResourceEvent) -> Result<(), HookError> {
        let event_type_str = match event.event_type {
            ResourceEventType::Created => "created",
            ResourceEventType::Updated => "updated",
            ResourceEventType::Deleted => "deleted",
        };

        debug!(
            resource_type = %event.resource_type,
            resource_id = %event.resource_id,
            event_type = %event_type_str,
            "AutomationDispatcherHook: checking for matching automations"
        );

        // Find matching automations
        let matching_automations = match self
            .automation_storage
            .get_matching_automations(&event.resource_type, event_type_str)
            .await
        {
            Ok(automations) => automations,
            Err(e) => {
                warn!(error = %e, "Failed to query matching automations");
                return Ok(()); // Don't fail the hook, just log
            }
        };

        if matching_automations.is_empty() {
            debug!(
                resource_type = %event.resource_type,
                event_type = %event_type_str,
                "No matching automations found"
            );
            return Ok(());
        }

        info!(
            resource_type = %event.resource_type,
            resource_id = %event.resource_id,
            event_type = %event_type_str,
            automation_count = matching_automations.len(),
            "Found matching automations, dispatching execution"
        );

        // Create automation event from resource event
        let automation_event = AutomationEvent {
            event_type: event_type_str.to_string(),
            resource: event.resource.clone().unwrap_or_default(),
            previous: None, // Previous version is not available in ResourceEvent
            timestamp: time::OffsetDateTime::now_utc().to_string(),
        };

        // Execute each matching automation
        for (automation, trigger) in matching_automations {
            // Evaluate FHIRPath filter if present
            if let Some(ref filter) = trigger.fhirpath_filter {
                if let Some(ref resource) = event.resource {
                    match self.evaluate_fhirpath_filter(filter, resource).await {
                        Ok(true) => {
                            debug!(
                                automation_id = %automation.id,
                                filter = %filter,
                                "FHIRPath filter matched"
                            );
                        }
                        Ok(false) => {
                            debug!(
                                automation_id = %automation.id,
                                filter = %filter,
                                "FHIRPath filter did not match, skipping automation"
                            );
                            continue; // Skip this automation
                        }
                        Err(e) => {
                            warn!(
                                automation_id = %automation.id,
                                filter = %filter,
                                error = %e,
                                "FHIRPath filter evaluation failed, skipping automation"
                            );
                            continue; // Skip on error
                        }
                    }
                }
            }

            let executor = self.executor.clone();
            let automation_event = automation_event.clone();

            // Spawn automation execution as a background task
            tokio::spawn(async move {
                let result = executor
                    .execute(&automation, Some(&trigger), automation_event)
                    .await;

                if !result.success {
                    warn!(
                        automation_id = %automation.id,
                        automation_name = %automation.name,
                        error = ?result.error,
                        "Automation execution failed"
                    );
                }
            });
        }

        Ok(())
    }
}

impl std::fmt::Debug for AutomationDispatcherHook {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AutomationDispatcherHook").finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Integration tests would go here
}
