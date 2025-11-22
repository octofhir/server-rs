//! Search parameter registry for indexing and lookup.
//!
//! This module provides a registry that stores search parameters indexed by:
//! - Resource type and code (for efficient lookup)
//! - Canonical URL (for resolving references)
//! - Common parameters (applicable to all resources)

use std::collections::HashMap;
use std::sync::Arc;

use crate::parameters::SearchParameter;

/// Registry for search parameters loaded from FHIR packages.
///
/// Provides efficient lookup of search parameters by resource type, code,
/// and canonical URL. Also tracks common parameters that apply to all resources.
#[derive(Debug, Default)]
pub struct SearchParameterRegistry {
    /// Parameters indexed by (resource_type, code)
    by_resource: HashMap<String, HashMap<String, Arc<SearchParameter>>>,
    /// All parameters by canonical URL
    by_url: HashMap<String, Arc<SearchParameter>>,
    /// Common parameters (apply to all resources: base includes "Resource" or "DomainResource")
    common: HashMap<String, Arc<SearchParameter>>,
}

impl SearchParameterRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self {
            by_resource: HashMap::new(),
            by_url: HashMap::new(),
            common: HashMap::new(),
        }
    }

    /// Register a search parameter in the registry.
    ///
    /// The parameter will be indexed by:
    /// - Canonical URL
    /// - Each base resource type + code
    /// - Common parameters if base includes "Resource" or "DomainResource"
    pub fn register(&mut self, param: SearchParameter) {
        let param = Arc::new(param);

        // Store by URL
        self.by_url.insert(param.url.clone(), param.clone());

        // Check if common parameter (base includes "Resource" or "DomainResource")
        if param.is_common() {
            self.common.insert(param.code.clone(), param.clone());
        }

        // Store by resource type
        for base in &param.base {
            self.by_resource
                .entry(base.clone())
                .or_default()
                .insert(param.code.clone(), param.clone());
        }
    }

    /// Get a search parameter for a specific resource type and code.
    ///
    /// First checks resource-specific parameters, then falls back to common parameters.
    pub fn get(&self, resource_type: &str, code: &str) -> Option<Arc<SearchParameter>> {
        // Check resource-specific first
        if let Some(param) = self
            .by_resource
            .get(resource_type)
            .and_then(|params| params.get(code))
        {
            return Some(param.clone());
        }

        // Check common parameters
        self.common.get(code).cloned()
    }

    /// Get all search parameters applicable to a resource type.
    ///
    /// Returns both resource-specific and common parameters.
    pub fn get_all_for_type(&self, resource_type: &str) -> Vec<Arc<SearchParameter>> {
        let mut params: Vec<_> = self.common.values().cloned().collect();

        if let Some(type_params) = self.by_resource.get(resource_type) {
            params.extend(type_params.values().cloned());
        }

        params
    }

    /// Get a search parameter by its canonical URL.
    pub fn get_by_url(&self, url: &str) -> Option<Arc<SearchParameter>> {
        self.by_url.get(url).cloned()
    }

    /// Get all common parameters (applicable to all resources).
    pub fn get_common_parameters(&self) -> Vec<Arc<SearchParameter>> {
        self.common.values().cloned().collect()
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
            .get(resource_type)
            .map(|m| m.len())
            .unwrap_or(0);
        type_count + self.common.len()
    }

    /// List all resource types that have specific parameters.
    pub fn list_resource_types(&self) -> Vec<&str> {
        self.by_resource.keys().map(|s| s.as_str()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parameters::SearchParameterType;

    #[test]
    fn test_register_and_get() {
        let mut registry = SearchParameterRegistry::new();

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
        let mut registry = SearchParameterRegistry::new();

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
        let mut registry = SearchParameterRegistry::new();

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
        let mut registry = SearchParameterRegistry::new();

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
}
