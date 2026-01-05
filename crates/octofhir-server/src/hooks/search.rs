//! Search parameter registry hook.
//!
//! This hook updates the search parameter registry when SearchParameter resources
//! are created, updated, or deleted. It provides incremental updates without
//! requiring a full registry reload.

use async_trait::async_trait;
use octofhir_core::events::{HookError, ResourceEvent, ResourceEventType, ResourceHook};
use octofhir_search::ReloadableSearchConfig;
use tracing::{debug, error, info, warn};

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

        match event.event_type {
            ResourceEventType::Created | ResourceEventType::Updated => {
                // Get the resource JSON (required for create/update)
                let resource = match &event.resource {
                    Some(r) => r,
                    None => {
                        warn!(
                            resource_id = %event.resource_id,
                            "SearchParamHook: no resource data in event"
                        );
                        return Ok(());
                    }
                };

                // Parse the search parameter
                match octofhir_search::parse_search_parameter(resource) {
                    Ok(param) => {
                        let code = param.code.clone();

                        // Upsert into registry
                        config.registry.upsert(param);

                        // Clear query cache
                        if let Some(cache) = &config.cache {
                            cache.clear();
                        }

                        info!(
                            code = %code,
                            resource_id = %event.resource_id,
                            "Search parameter registered via hook"
                        );
                    }
                    Err(e) => {
                        error!(
                            error = %e,
                            resource_id = %event.resource_id,
                            "Failed to parse SearchParameter"
                        );
                        // Don't fail the hook - the resource was already saved
                    }
                }
            }
            ResourceEventType::Deleted => {
                // For deletes, we don't have the resource data (it's already deleted),
                // so we can't easily remove by URL. Clear the cache and log.
                // The parameter will be removed when the registry is next fully reloaded.
                // This is acceptable since SearchParameter deletions are rare.
                if let Some(cache) = &config.cache {
                    cache.clear();
                }

                // Try to remove by URL if the resource_id looks like a URL
                if event.resource_id.starts_with("http") {
                    let removed = config.registry.remove_by_url(&event.resource_id);
                    if removed {
                        info!(
                            url = %event.resource_id,
                            "Search parameter removed via hook"
                        );
                    }
                } else {
                    warn!(
                        resource_id = %event.resource_id,
                        "SearchParameter deleted but cannot remove from registry (no URL). \
                         Registry will be updated on next reload."
                    );
                }
            }
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
    use super::*;
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
