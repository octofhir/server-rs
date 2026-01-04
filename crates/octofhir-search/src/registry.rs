//! Search parameter registry for indexing and lookup.
//!
//! This module provides a registry that stores search parameters indexed by:
//! - Resource type and code (for efficient lookup)
//! - Canonical URL (for resolving references)
//! - Common parameters (applicable to all resources)
//!
//! Uses DashMap for lock-free concurrent access, allowing incremental updates
//! without blocking readers.

use dashmap::DashMap;
use std::sync::Arc;

use crate::parameters::SearchParameter;

/// Registry for search parameters loaded from FHIR packages.
///
/// Provides efficient lookup of search parameters by resource type, code,
/// and canonical URL. Also tracks common parameters that apply to all resources.
///
/// Thread-safe with lock-free reads using DashMap.
#[derive(Debug, Default)]
pub struct SearchParameterRegistry {
    /// Parameters indexed by (resource_type, code) as composite key
    by_resource: DashMap<(String, String), Arc<SearchParameter>>,
    /// All parameters by canonical URL
    by_url: DashMap<String, Arc<SearchParameter>>,
    /// Common parameters (apply to all resources: base includes "Resource" or "DomainResource")
    common: DashMap<String, Arc<SearchParameter>>,
}

impl SearchParameterRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self {
            by_resource: DashMap::new(),
            by_url: DashMap::new(),
            common: DashMap::new(),
        }
    }

    /// Register a search parameter in the registry.
    ///
    /// The parameter will be indexed by:
    /// - Canonical URL
    /// - Each base resource type + code
    /// - Common parameters if base includes "Resource" or "DomainResource"
    ///
    /// Thread-safe - can be called concurrently from multiple threads.
    pub fn register(&self, param: SearchParameter) {
        let param = Arc::new(param);

        // Store by URL
        self.by_url.insert(param.url.clone(), param.clone());

        // Check if common parameter (base includes "Resource" or "DomainResource")
        if param.is_common() {
            self.common.insert(param.code.clone(), param.clone());
        }

        // Store by resource type with composite key (resource_type, code)
        for base in &param.base {
            self.by_resource
                .insert((base.clone(), param.code.clone()), param.clone());
        }
    }

    /// Add or update a single search parameter (thread-safe, incremental).
    ///
    /// This is an alias for `register()` for clarity when doing incremental updates.
    pub fn upsert(&self, param: SearchParameter) {
        self.register(param);
    }

    /// Remove a search parameter by its canonical URL (thread-safe).
    ///
    /// Returns true if the parameter was found and removed.
    pub fn remove_by_url(&self, url: &str) -> bool {
        if let Some((_, param)) = self.by_url.remove(url) {
            // Remove from resource indices
            for base in &param.base {
                if base == "Resource" || base == "DomainResource" {
                    self.common.remove(&param.code);
                } else {
                    self.by_resource.remove(&(base.clone(), param.code.clone()));
                }
            }
            true
        } else {
            false
        }
    }

    /// Get a search parameter for a specific resource type and code.
    ///
    /// First checks resource-specific parameters, then falls back to common parameters.
    pub fn get(&self, resource_type: &str, code: &str) -> Option<Arc<SearchParameter>> {
        // Check resource-specific first with composite key
        let key = (resource_type.to_string(), code.to_string());
        if let Some(param) = self.by_resource.get(&key) {
            return Some(param.clone());
        }

        // Check common parameters
        self.common.get(code).map(|p| p.clone())
    }

    /// Get all search parameters applicable to a resource type.
    ///
    /// Returns both resource-specific and common parameters.
    pub fn get_all_for_type(&self, resource_type: &str) -> Vec<Arc<SearchParameter>> {
        let mut params: Vec<_> = self.common.iter().map(|entry| entry.value().clone()).collect();

        // Iterate over all entries and filter by resource type
        params.extend(
            self.by_resource
                .iter()
                .filter(|entry| entry.key().0 == resource_type)
                .map(|entry| entry.value().clone()),
        );

        params
    }

    /// Get a search parameter by its canonical URL.
    pub fn get_by_url(&self, url: &str) -> Option<Arc<SearchParameter>> {
        self.by_url.get(url).map(|entry| entry.value().clone())
    }

    /// Get all common parameters (applicable to all resources).
    pub fn get_common_parameters(&self) -> Vec<Arc<SearchParameter>> {
        self.common.iter().map(|entry| entry.value().clone()).collect()
    }

    /// Get the total number of registered parameters.
    pub fn len(&self) -> usize {
        self.by_url.len()
    }

    /// Check if the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.by_url.is_empty()
    }

    /// Get the number of parameters for a specific resource type.
    pub fn count_for_type(&self, resource_type: &str) -> usize {
        let type_count = self
            .by_resource
            .iter()
            .filter(|entry| entry.key().0 == resource_type)
            .count();
        type_count + self.common.len()
    }

    /// List all resource types that have specific parameters.
    pub fn list_resource_types(&self) -> Vec<String> {
        let mut types: Vec<String> = self
            .by_resource
            .iter()
            .map(|entry| entry.key().0.clone())
            .collect();
        types.sort();
        types.dedup();
        types
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parameters::SearchParameterType;

    #[test]
    fn test_register_and_get() {
        let registry = SearchParameterRegistry::new();

        let param = SearchParameter::new(
            "name",
            "http://hl7.org/fhir/SearchParameter/Patient-name",
            SearchParameterType::String,
            vec!["Patient".to_string()],
        )
        .with_expression("Patient.name")
        .with_description("A patient's name");

        registry.register(param);

        // Should find by resource type and code
        let found = registry.get("Patient", "name");
        assert!(found.is_some());
        assert_eq!(found.unwrap().code, "name");

        // Should not find for wrong resource type
        assert!(registry.get("Observation", "name").is_none());
    }

    #[test]
    fn test_common_parameters() {
        let registry = SearchParameterRegistry::new();

        // Register a common parameter
        let param = SearchParameter::new(
            "_id",
            "http://hl7.org/fhir/SearchParameter/Resource-id",
            SearchParameterType::Token,
            vec!["Resource".to_string()],
        )
        .with_expression("Resource.id");

        registry.register(param);

        // Should find for any resource type
        assert!(registry.get("Patient", "_id").is_some());
        assert!(registry.get("Observation", "_id").is_some());
        assert!(registry.get("Condition", "_id").is_some());
    }

    #[test]
    fn test_get_all_for_type() {
        let registry = SearchParameterRegistry::new();

        // Register common parameter
        registry.register(SearchParameter::new(
            "_id",
            "http://hl7.org/fhir/SearchParameter/Resource-id",
            SearchParameterType::Token,
            vec!["Resource".to_string()],
        ));

        // Register resource-specific parameter
        registry.register(SearchParameter::new(
            "name",
            "http://hl7.org/fhir/SearchParameter/Patient-name",
            SearchParameterType::String,
            vec!["Patient".to_string()],
        ));

        let patient_params = registry.get_all_for_type("Patient");
        assert_eq!(patient_params.len(), 2);

        let observation_params = registry.get_all_for_type("Observation");
        assert_eq!(observation_params.len(), 1); // Only common params
    }

    #[test]
    fn test_get_by_url() {
        let registry = SearchParameterRegistry::new();

        registry.register(SearchParameter::new(
            "name",
            "http://hl7.org/fhir/SearchParameter/Patient-name",
            SearchParameterType::String,
            vec!["Patient".to_string()],
        ));

        let found = registry.get_by_url("http://hl7.org/fhir/SearchParameter/Patient-name");
        assert!(found.is_some());

        let not_found = registry.get_by_url("http://example.org/unknown");
        assert!(not_found.is_none());
    }

    #[test]
    fn test_upsert_and_remove() {
        let registry = SearchParameterRegistry::new();

        let param = SearchParameter::new(
            "custom",
            "http://example.org/SearchParameter/custom",
            SearchParameterType::String,
            vec!["Patient".to_string()],
        );

        // Upsert (insert)
        registry.upsert(param);
        assert!(registry.get("Patient", "custom").is_some());

        // Remove
        let removed = registry.remove_by_url("http://example.org/SearchParameter/custom");
        assert!(removed);
        assert!(registry.get("Patient", "custom").is_none());

        // Remove non-existent
        let not_removed = registry.remove_by_url("http://example.org/nonexistent");
        assert!(!not_removed);
    }
}
