//! Search parameter registry hook.
//!
//! This hook updates the search parameter registry when SearchParameter resources
//! are created, updated, or deleted. It provides incremental updates without
//! requiring a full registry reload.

use async_trait::async_trait;
use octofhir_core::events::{HookError, ResourceEvent, ResourceEventType, ResourceHook};
use octofhir_search::ReloadableSearchConfig;
use tracing::{debug, info};

/// Hook that updates search parameter registry on SearchParameter changes.
///
/// When a SearchParameter resource is created or updated, this hook:
/// 1. Parses the SearchParameter from JSON
/// 2. Upserts it into the registry
/// 3. Clears the query cache
///
/// When deleted, it removes the parameter from the registry.
///
/// # Example
///
/// ```ignore
/// let hook = SearchParamHook::new(search_config.clone());
/// registry.register(Arc::new(hook));
/// ```
pub struct SearchParamHook {
    search_config: ReloadableSearchConfig,
}

impl SearchParamHook {
    /// Create a new search parameter hook.
    ///
    /// # Arguments
    ///
    /// * `search_config` - The reloadable search configuration to update
    pub fn new(search_config: ReloadableSearchConfig) -> Self {
        Self { search_config }
    }
}

#[async_trait]
impl ResourceHook for SearchParamHook {
    fn name(&self) -> &str {
        "search_param"
    }

    fn resource_types(&self) -> &[&str] {
        &["SearchParameter"]
    }

    async fn handle(&self, event: &ResourceEvent) -> Result<(), HookError> {
        debug!(
            resource_id = %event.resource_id,
            event_type = %event.event_type,
            "SearchParamHook: processing search parameter change"
        );

        let config = self.search_config.config();

        let version = event.version_id.ok_or_else(|| {
            HookError::execution("SearchParameter event has no committed revision")
        })?;
        let parameter = match event.event_type {
            ResourceEventType::Created | ResourceEventType::Updated => {
                let resource = event
                    .resource
                    .as_ref()
                    .ok_or_else(|| HookError::execution("SearchParameter event has no snapshot"))?;
                Some(
                    octofhir_search::parse_search_parameter(resource)
                        .map_err(|error| HookError::execution(error.to_string()))?,
                )
            }
            ResourceEventType::Deleted => None,
        };
        if config
            .registry
            .apply_resource(&event.resource_id, version, parameter)
        {
            if let Some(cache) = &config.cache {
                cache.clear();
            }
            info!(resource_id = %event.resource_id, version, "Committed search parameter revision applied");
        }

        Ok(())
    }
}

impl std::fmt::Debug for SearchParamHook {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SearchParamHook").finish()
    }
}

#[cfg(test)]
mod tests {

    use octofhir_core::events::ResourceEvent;
    use serde_json::json;

    // Note: Full tests require mock implementations of SearchParameterRegistry.
    // These are basic tests for matching logic.

    #[test]
    fn test_resource_type_matching() {
        let search_event = ResourceEvent::created("SearchParameter", "test-id", json!({}));
        let patient_event = ResourceEvent::created("Patient", "test-id", json!({}));

        assert_eq!(search_event.resource_type, "SearchParameter");
        assert_ne!(patient_event.resource_type, "SearchParameter");
    }
}
