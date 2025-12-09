//! FHIR element path resolver for LSP completions.
//!
//! This module provides element path completion from embedded FHIR schemas
//! using the FhirSchemaModelProvider for blazing fast lookups.

use dashmap::DashMap;
use std::sync::Arc;

use crate::server::SharedModelProvider;

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

/// Cache of element trees by resource type.
pub struct FhirResolver {
    /// Model provider for accessing FHIR schemas (shared with validation/FHIRPath)
    model_provider: SharedModelProvider,
    /// Cache of children by (resource_type, parent_path) for fast lookups
    children_cache: DashMap<(String, String), Vec<ElementInfo>>,
}

impl FhirResolver {
    /// Creates a new FHIR resolver with a shared model provider.
    pub fn with_model_provider(model_provider: SharedModelProvider) -> Self {
        tracing::info!("FhirResolver: Created with shared model provider");
        Self {
            model_provider,
            children_cache: DashMap::new(),
        }
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


    /// Get children of a path (direct descendants only) with caching.
    pub async fn get_children(&self, resource_type: &str, parent_path: &str) -> Vec<ElementInfo> {
        // Check children cache first for instant lookup
        let cache_key = (resource_type.to_string(), parent_path.to_string());
        if let Some(cached) = self.children_cache.get(&cache_key) {
            tracing::trace!("Cache hit for {}.{}", resource_type, parent_path);
            return cached.value().clone();
        }

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
                    self.get_direct_children(&parent_type, &full_parent_path).await
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

        // Cache the result for next time
        self.children_cache
            .insert(cache_key, children.clone());

        children
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
                    model_elements.iter().take(3).map(|e| &e.name).collect::<Vec<_>>()
                );
                model_elements.into_iter().filter_map(|elem| {
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
                        tracing::debug!(
                            "Element '{}' not found in type '{}'",
                            part,
                            current_type
                        );
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

    /// Clear the children cache.
    pub fn clear_cache(&self) {
        self.children_cache.clear();
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

#[cfg(test)]
mod tests {
    use super::*;
    use octofhir_fhirschema::{embedded::get_schemas, model_provider::DynamicSchemaProvider};
    use octofhir_fhir_model::provider::FhirVersion;

    fn create_test_provider() -> SharedModelProvider {
        let schemas = get_schemas(octofhir_fhirschema::FhirVersion::R4B);
        Arc::new(DynamicSchemaProvider::new(schemas.clone(), FhirVersion::R4B))
    }

    #[tokio::test]
    async fn test_get_patient_elements() {
        let model_provider = create_test_provider();
        let resolver = FhirResolver::with_model_provider(model_provider);
        let elements = resolver.get_elements("Patient").await;

        // Should have elements from embedded schema
        assert!(!elements.is_empty(), "Patient should have elements");

        // Check for specific Patient elements
        let has_name = elements.iter().any(|e| e.name == "name");
        let has_gender = elements.iter().any(|e| e.name == "gender");
        let has_birthdate = elements.iter().any(|e| e.name == "birthDate");

        assert!(has_name, "Patient should have name element");
        assert!(has_gender, "Patient should have gender element");
        assert!(has_birthdate, "Patient should have birthDate element");
    }

    #[tokio::test]
    async fn test_get_nested_elements() {
        let model_provider = create_test_provider();
        let resolver = FhirResolver::with_model_provider(model_provider);

        // Get children of "name" element (should be HumanName elements)
        let name_children = resolver.get_children("Patient", "name").await;

        assert!(!name_children.is_empty(), "Patient.name should have children");

        // HumanName should have "given", "family", etc.
        let has_given = name_children.iter().any(|e| e.name == "given");
        let has_family = name_children.iter().any(|e| e.name == "family");

        assert!(has_given, "HumanName should have given element");
        assert!(has_family, "HumanName should have family element");
    }

    #[tokio::test]
    async fn test_get_element_type() {
        let model_provider = create_test_provider();
        let resolver = FhirResolver::with_model_provider(model_provider);

        // "name" in Patient should resolve to "HumanName"
        let name_type = resolver.get_element_type("Patient", "name").await;
        assert_eq!(name_type, Some("HumanName".to_string()));

        // Non-existent element should return None
        let invalid_type = resolver.get_element_type("Patient", "nonexistent").await;
        assert_eq!(invalid_type, None);
    }

    #[tokio::test]
    async fn test_cache_works() {
        let model_provider = create_test_provider();
        let resolver = FhirResolver::with_model_provider(model_provider);

        // First call - populates cache
        let children1 = resolver.get_children("Patient", "name").await;

        // Second call - should use cache
        let children2 = resolver.get_children("Patient", "name").await;

        assert_eq!(children1.len(), children2.len());

        // Clear cache
        resolver.clear_cache();

        // Third call - should repopulate
        let children3 = resolver.get_children("Patient", "name").await;
        assert_eq!(children1.len(), children3.len());
    }
}
