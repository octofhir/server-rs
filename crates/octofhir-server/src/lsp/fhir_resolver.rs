//! FHIR element path resolver for LSP completions.
//!
//! This module provides element path completion from embedded FHIR schemas
//! using the FhirSchemaModelProvider for blazing fast lookups.
//!
//! Elements are loaded lazily on-demand with caching to optimize memory usage
//! and response times. Common resource types can be preloaded at startup.

use dashmap::DashMap;
use std::sync::Arc;

use crate::model_provider::OctoFhirModelProvider;
use crate::server::SharedModelProvider;

/// Common FHIR resource types that are frequently accessed.
/// These can be optionally preloaded at startup for faster first-access.
const COMMON_RESOURCES: &[&str] = &[
    "Patient",
    "Observation",
    "Encounter",
    "Condition",
    "Procedure",
    "MedicationRequest",
    "DiagnosticReport",
    "Immunization",
    "AllergyIntolerance",
    "CarePlan",
];

/// Loading state for tracking concurrent loads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoadingState {
    /// Not yet loaded
    NotLoaded,
    /// Currently being loaded
    Loading,
    /// Successfully loaded
    Loaded,
    /// Loading failed
    Failed(String),
}

/// Information about a FHIR element from a StructureDefinition.
#[derive(Debug, Clone)]
pub struct ElementInfo {
    /// Element path (e.g., "Patient.name")
    pub path: String,
    /// Short name for the element
    pub name: String,
    /// Element type code (e.g., "string", "HumanName")
    pub type_code: String,
    /// Cardinality min
    pub min: u32,
    /// Cardinality max (0 = unlimited)
    pub max: u32,
    /// Short description
    pub short: Option<String>,
    /// Full definition
    pub definition: Option<String>,
    /// Whether this is an array element
    pub is_array: bool,
    /// Whether this is a backbone element (has children)
    pub is_backbone: bool,
}

/// Cache of element trees by resource type with lazy loading support.
pub struct FhirResolver {
    /// Model provider for accessing FHIR schemas (shared with validation/FHIRPath)
    model_provider: SharedModelProvider,
    /// OctoFhir model provider for accessing choice type information
    octofhir_provider: Arc<OctoFhirModelProvider>,
    /// Cache of children by (resource_type, parent_path) for fast lookups
    children_cache: DashMap<(String, String), Vec<ElementInfo>>,
    /// Loading state tracker to prevent duplicate parallel loads
    loading_state: DashMap<(String, String), LoadingState>,
}

impl FhirResolver {
    /// Creates a new FHIR resolver with a shared model provider.
    pub fn with_model_provider(octofhir_provider: Arc<OctoFhirModelProvider>) -> Self {
        let model_provider: SharedModelProvider = octofhir_provider.clone();

        tracing::info!("FhirResolver: Created with shared model provider");
        Self {
            model_provider,
            octofhir_provider,
            children_cache: DashMap::new(),
            loading_state: DashMap::new(),
        }
    }

    /// Preload top-level elements for common FHIR resource types.
    ///
    /// This is an optional optimization that can be called at startup to
    /// improve first-access latency for common resources. The preloading
    /// runs in the background and doesn't block.
    pub async fn preload_common_resources(&self) {
        tracing::info!(
            "Preloading {} common FHIR resource types",
            COMMON_RESOURCES.len()
        );

        for resource_type in COMMON_RESOURCES {
            // Only preload top-level elements (empty parent path)
            // Nested elements are loaded lazily when accessed
            let _ = self.get_children(resource_type, "").await;
        }

        tracing::info!("Preloading of common FHIR resources complete");
    }

    /// Preload common resources in the background (non-blocking).
    pub fn preload_common_resources_background(self: &Arc<Self>) {
        let resolver = Arc::clone(self);
        tokio::spawn(async move {
            resolver.preload_common_resources().await;
        });
    }

    /// Get the loading state for a cache key.
    pub fn get_loading_state(&self, resource_type: &str, parent_path: &str) -> LoadingState {
        let key = (resource_type.to_string(), parent_path.to_string());
        self.loading_state
            .get(&key)
            .map(|r| r.value().clone())
            .unwrap_or(LoadingState::NotLoaded)
    }

    /// Check if elements for a path are already cached.
    pub fn is_cached(&self, resource_type: &str, parent_path: &str) -> bool {
        let key = (resource_type.to_string(), parent_path.to_string());
        self.children_cache.contains_key(&key)
    }

    /// Get cache statistics for debugging.
    pub fn cache_stats(&self) -> (usize, usize) {
        (self.children_cache.len(), self.loading_state.len())
    }

    /// Get elements for a resource type from embedded schemas.
    pub async fn get_elements(&self, resource_type: &str) -> Vec<ElementInfo> {
        // Use model provider to get all elements for this resource type
        tracing::debug!("get_elements called for resource_type={}", resource_type);
        match self.model_provider.get_elements(resource_type).await {
            Ok(model_elements) => {
                tracing::debug!(
                    "get_elements: got {} elements for {}",
                    model_elements.len(),
                    resource_type
                );
                // Convert ModelElementInfo to our ElementInfo format
                model_elements
                    .into_iter()
                    .map(|elem| {
                        // Extract the element name from the path (e.g., "Patient.name" -> "name")
                        let name = elem.name.clone();

                        // Build the full path
                        let path = if elem.name.contains('.') {
                            elem.name.clone()
                        } else {
                            format!("{}.{}", resource_type, elem.name)
                        };

                        ElementInfo {
                            path,
                            name,
                            type_code: elem.element_type.clone(),
                            min: 0, // ModelElementInfo doesn't expose cardinality
                            max: 1, // ModelElementInfo doesn't expose cardinality
                            short: elem.documentation.clone(),
                            definition: elem.documentation,
                            is_array: false, // Could infer from type or path
                            is_backbone: elem.element_type == "BackboneElement",
                        }
                    })
                    .collect()
            }
            Err(e) => {
                tracing::warn!("Failed to get elements for {}: {}", resource_type, e);
                Vec::new()
            }
        }
    }

    /// Determine if an element is a choice type variant and return the base name.
    /// E.g., for "deceasedBoolean" returns Some("deceased") if "deceased[x]" exists in schema.
    /// Delegates to OctoFhirModelProvider for choice type detection.
    async fn get_choice_base_name(
        &self,
        resource_type: &str,
        element_name: &str,
    ) -> Option<String> {
        self.octofhir_provider
            .get_choice_base_name(resource_type, element_name)
            .await
    }

    /// Get children of a path (direct descendants only) with caching and loading state tracking.
    ///
    /// This method implements lazy loading with deduplication:
    /// - Returns cached results immediately if available
    /// - Tracks loading state to prevent duplicate parallel loads
    /// - Updates cache and loading state upon completion
    pub async fn get_children(&self, resource_type: &str, parent_path: &str) -> Vec<ElementInfo> {
        // Check children cache first for instant lookup
        let cache_key = (resource_type.to_string(), parent_path.to_string());
        if let Some(cached) = self.children_cache.get(&cache_key) {
            tracing::trace!("Cache hit for {}.{}", resource_type, parent_path);
            return cached.value().clone();
        }

        // Check if already loading - if so, wait briefly and check cache again
        // This prevents duplicate parallel loads for the same path
        {
            let state = self.loading_state.get(&cache_key);
            if let Some(s) = state {
                if *s.value() == LoadingState::Loading {
                    tracing::trace!(
                        "Already loading {}.{}, waiting for result",
                        resource_type,
                        parent_path
                    );
                    // Drop the reference before sleeping
                    drop(s);
                    // Brief wait then check cache
                    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
                    if let Some(cached) = self.children_cache.get(&cache_key) {
                        return cached.value().clone();
                    }
                }
            }
        }

        // Mark as loading
        self.loading_state
            .insert(cache_key.clone(), LoadingState::Loading);

        tracing::debug!(
            "get_children: resource_type={}, parent_path='{}'",
            resource_type,
            parent_path
        );

        let children = if parent_path.is_empty() {
            // Root level: get direct children of the resource type
            tracing::trace!("Getting root-level children for {}", resource_type);
            self.get_direct_children(resource_type, resource_type).await
        } else {
            // Nested level: need to find the type of the parent element first
            // Example: parent_path = "name" → need to get HumanName elements
            //          parent_path = "name.given" → need to navigate through name first
            tracing::trace!(
                "Getting type for nested path: {}.{}",
                resource_type,
                parent_path
            );
            match self.get_element_type(resource_type, parent_path).await {
                Some(parent_type) => {
                    tracing::trace!("Parent type resolved to: {}", parent_type);
                    // Get children of the parent type
                    let full_parent_path = format!("{}.{}", resource_type, parent_path);
                    self.get_direct_children(&parent_type, &full_parent_path)
                        .await
                }
                None => {
                    tracing::debug!(
                        "Could not determine type for path: {}.{} - this may be a primitive type or invalid path",
                        resource_type,
                        parent_path
                    );
                    Vec::new()
                }
            }
        };

        tracing::debug!(
            "Found {} children for {}.{}",
            children.len(),
            resource_type,
            parent_path
        );

        // Filter out choice type base fields (e.g., hide "deceased" when "deceasedBoolean" exists)
        let filtered_children = self.filter_choice_type_bases(resource_type, children).await;

        tracing::debug!(
            "After choice type filtering: {} children for {}.{}",
            filtered_children.len(),
            resource_type,
            parent_path
        );

        // Cache the result for next time and update loading state
        self.children_cache
            .insert(cache_key.clone(), filtered_children.clone());
        self.loading_state.insert(cache_key, LoadingState::Loaded);

        filtered_children
    }

    /// Get direct children of a type (helper for get_children).
    async fn get_direct_children(
        &self,
        type_name: &str,
        full_parent_path: &str,
    ) -> Vec<ElementInfo> {
        // Get all elements for this type from the model provider
        match self.model_provider.get_elements(type_name).await {
            Ok(model_elements) => {
                tracing::debug!(
                    "get_direct_children: type_name={}, got {} elements, first few: {:?}",
                    type_name,
                    model_elements.len(),
                    model_elements
                        .iter()
                        .take(3)
                        .map(|e| &e.name)
                        .collect::<Vec<_>>()
                );
                model_elements
                    .into_iter()
                    .filter_map(|elem| {
                        // For direct children, the element name should not contain dots
                        // (e.g., "given", "family" but not "name.given")
                        if elem.name.contains('.') {
                            return None;
                        }

                        // Build the full path
                        let path = format!("{}.{}", full_parent_path, elem.name);

                        Some(ElementInfo {
                            path,
                            name: elem.name.clone(),
                            type_code: elem.element_type.clone(),
                            min: 0,
                            max: 1,
                            short: elem.documentation.clone(),
                            definition: elem.documentation,
                            is_array: false,
                            is_backbone: elem.element_type == "BackboneElement",
                        })
                    })
                    .collect()
            }
            Err(e) => {
                tracing::warn!("Failed to get elements for type {}: {}", type_name, e);
                Vec::new()
            }
        }
    }

    /// Get the type of an element at a given path.
    async fn get_element_type(&self, resource_type: &str, path: &str) -> Option<String> {
        // For a path like "name", we need to find the element "name" in the resource type
        // and return its type (e.g., "HumanName")

        // Split the path by dots to handle nested paths
        // Example: "name.given" → ["name", "given"]
        let path_parts: Vec<&str> = path.split('.').collect();

        if path_parts.is_empty() {
            return None;
        }

        // Start with the resource type
        let mut current_type = resource_type.to_string();

        // Navigate through each path segment
        for part in path_parts {
            match self.model_provider.get_elements(&current_type).await {
                Ok(elements) => {
                    // Find the element with this name
                    if let Some(elem) = elements.iter().find(|e| e.name == part) {
                        current_type = elem.element_type.clone();
                    } else {
                        tracing::debug!("Element '{}' not found in type '{}'", part, current_type);
                        return None;
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to get elements for type {}: {}", current_type, e);
                    return None;
                }
            }
        }

        Some(current_type)
    }

    /// Clear all cached data and loading states.
    pub fn clear_cache(&self) {
        self.children_cache.clear();
        self.loading_state.clear();
    }

    /// Filter out choice type base fields when typed variants exist.
    ///
    /// In FHIR, choice types (polymorphic elements) have a base field ending in `[x]`
    /// and typed variants. For example:
    /// - Base: `deceased[x]` (hidden in completions)
    /// - Variants: `deceasedBoolean`, `deceasedDateTime` (shown in completions)
    ///
    /// This function uses get_choice_base_name() to identify variants
    /// and removes the base fields when variants exist.
    async fn filter_choice_type_bases(
        &self,
        resource_type: &str,
        elements: Vec<ElementInfo>,
    ) -> Vec<ElementInfo> {
        use std::collections::HashSet;

        // Collect all base names that have typed variants
        // Check each element to see if it's a choice variant, and collect the base names
        let mut bases_with_variants: HashSet<String> = HashSet::new();

        for elem in &elements {
            if let Some(base_name) = self.get_choice_base_name(resource_type, &elem.name).await {
                bases_with_variants.insert(base_name);
            }
        }

        // Filter: hide elements whose name appears as a base with variants
        elements
            .into_iter()
            .filter(|elem| {
                // If this element's name is a choice base with variants → hide it
                if bases_with_variants.contains(&elem.name) {
                    tracing::trace!(
                        "Hiding choice type base field '{}' (has typed variants)",
                        elem.name
                    );
                    false
                } else {
                    true
                }
            })
            .collect()
    }

    /// Get element info for a specific path.
    pub async fn get_element(&self, resource_type: &str, path: &str) -> Option<ElementInfo> {
        let elements = self.get_elements(resource_type).await;

        // Build the full path if needed
        let full_path = if path.starts_with(resource_type) {
            path.to_string()
        } else if path.is_empty() {
            resource_type.to_string()
        } else {
            format!("{}.{}", resource_type, path)
        };

        elements.into_iter().find(|e| e.path == full_path)
    }

    // Note: The old get_common_elements fallback method has been removed.
    // We now use FhirSchemaModelProvider with embedded schemas for ALL resource types.
}

// Default impl removed - FhirResolver must be created with a specific model provider

/// Tests for the FHIR resolver.
///
/// Note: Some tests require a database connection and are marked as `#[ignore]`.
/// Run with `cargo test -- --ignored` to include these tests when a database is available.
#[cfg(test)]
mod tests {
    use super::*;

    // Basic unit tests that don't require a database connection
    #[test]
    fn test_loading_state_enum() {
        assert_eq!(LoadingState::NotLoaded, LoadingState::NotLoaded);
        assert_ne!(LoadingState::Loading, LoadingState::Loaded);
        assert_eq!(
            LoadingState::Failed("error".to_string()),
            LoadingState::Failed("error".to_string())
        );
    }

    #[test]
    fn test_element_info_clone() {
        let elem = ElementInfo {
            path: "Patient.name".to_string(),
            name: "name".to_string(),
            type_code: "HumanName".to_string(),
            min: 0,
            max: 0,
            short: Some("Name of patient".to_string()),
            definition: None,
            is_array: true,
            is_backbone: false,
        };
        let cloned = elem.clone();
        assert_eq!(elem.path, cloned.path);
        assert_eq!(elem.name, cloned.name);
        assert_eq!(elem.type_code, cloned.type_code);
    }

    #[test]
    fn test_common_resources_list() {
        // Verify we have a reasonable set of common resources
        assert!(!COMMON_RESOURCES.is_empty());
        assert!(COMMON_RESOURCES.contains(&"Patient"));
        assert!(COMMON_RESOURCES.contains(&"Observation"));
    }

    // Note: Integration tests requiring OctoFhirModelProvider are not included here
    // as they require a database connection. Run integration tests separately with:
    //   cargo test --test fhir_resolver_integration -- --ignored
}
