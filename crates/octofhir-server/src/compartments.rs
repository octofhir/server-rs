//! FHIR Compartment Search Implementation
//!
//! This module provides compartment-based search functionality according to the FHIR R4B specification.
//! Compartments provide logical groupings of resources related to a specific context (Patient, Encounter, etc.).
//!
//! ## FHIR Compartments
//!
//! The FHIR specification defines 5 standard compartments:
//! - Patient: All resources related to a patient
//! - Encounter: All resources related to an encounter
//! - Practitioner: All resources related to a practitioner
//! - RelatedPerson: All resources related to a related person
//! - Device: All resources related to a device
//!
//! ## Usage
//!
//! ```rust
//! // Create registry from canonical manager
//! let registry = CompartmentRegistry::new(canonical_manager).await?;
//!
//! // Get compartment definition
//! let patient_compartment = registry.get("Patient")?;
//!
//! // Get inclusion parameters for a resource type
//! let params = patient_compartment.get_inclusion_params("Observation")?;
//! // Returns: ["subject", "performer"] - search parameters that link Observation to Patient
//! ```

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use octofhir_canonical_manager::CanonicalManager;

/// Errors that can occur during compartment operations
#[derive(Debug, Error)]
pub enum CompartmentError {
    #[error("Compartment '{0}' not found")]
    NotFound(String),

    #[error("Resource type '{0}' not in compartment '{1}'")]
    ResourceNotInCompartment(String, String),

    #[error("Failed to load compartment definition: {0}")]
    LoadError(String),

    #[error("Invalid compartment definition: {0}")]
    InvalidDefinition(String),
}

/// A single resource inclusion rule within a compartment
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompartmentResourceRule {
    /// The resource type this rule applies to
    pub resource_type: String,

    /// Search parameters that link this resource to the compartment
    /// For example, Observation has ["subject", "performer"] for Patient compartment
    pub params: Vec<String>,

    /// Optional documentation about this inclusion
    pub documentation: Option<String>,
}

/// Parsed CompartmentDefinition resource
#[derive(Debug, Clone)]
pub struct CompartmentDefinition {
    /// Name of the compartment (e.g., "Patient", "Encounter")
    pub name: String,

    /// Canonical URL of the compartment definition
    pub url: String,

    /// Map of resource type -> inclusion parameters
    /// Example: "Observation" -> ["subject", "performer"]
    resource_rules: HashMap<String, Vec<String>>,
}

impl CompartmentDefinition {
    /// Create a new compartment definition
    pub fn new(name: String, url: String) -> Self {
        Self {
            name,
            url,
            resource_rules: HashMap::new(),
        }
    }

    /// Add a resource inclusion rule
    pub fn add_resource_rule(&mut self, resource_type: String, params: Vec<String>) {
        self.resource_rules.insert(resource_type, params);
    }

    /// Get the inclusion parameters for a specific resource type
    ///
    /// Returns the list of search parameter names that can link resources of this type
    /// to the compartment. Returns None if the resource type is not in this compartment.
    pub fn get_inclusion_params(&self, resource_type: &str) -> Option<&[String]> {
        self.resource_rules.get(resource_type).map(|v| v.as_slice())
    }

    /// Check if a resource type is included in this compartment
    pub fn contains_resource_type(&self, resource_type: &str) -> bool {
        self.resource_rules.contains_key(resource_type)
    }

    /// Get all resource types in this compartment
    pub fn resource_types(&self) -> Vec<&str> {
        self.resource_rules.keys().map(|s| s.as_str()).collect()
    }

    /// Parse a CompartmentDefinition from a FHIR resource
    pub fn from_fhir_resource(resource: &serde_json::Value) -> Result<Self, CompartmentError> {
        let name = resource
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CompartmentError::InvalidDefinition("Missing 'name' field".to_string()))?
            .to_string();

        let url = resource
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CompartmentError::InvalidDefinition("Missing 'url' field".to_string()))?
            .to_string();

        let mut def = CompartmentDefinition::new(name, url);

        // Parse resource rules from the 'resource' array
        if let Some(resources) = resource.get("resource").and_then(|v| v.as_array()) {
            for resource_entry in resources {
                let resource_type = resource_entry
                    .get("code")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        CompartmentError::InvalidDefinition(
                            "Resource entry missing 'code' field".to_string(),
                        )
                    })?
                    .to_string();

                let params: Vec<String> = resource_entry
                    .get("param")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str())
                            .map(String::from)
                            .collect()
                    })
                    .unwrap_or_default();

                if !params.is_empty() {
                    def.add_resource_rule(resource_type, params);
                }
            }
        }

        Ok(def)
    }
}

/// Registry of all available compartment definitions
#[derive(Debug, Clone)]
pub struct CompartmentRegistry {
    definitions: HashMap<String, CompartmentDefinition>,
}

impl CompartmentRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        Self {
            definitions: HashMap::new(),
        }
    }

    /// Load compartment definitions from the canonical manager
    ///
    /// This loads all 5 standard FHIR compartments from the loaded packages:
    /// - Patient
    /// - Encounter
    /// - Practitioner
    /// - RelatedPerson
    /// - Device
    pub async fn from_canonical_manager(
        manager: &CanonicalManager,
    ) -> Result<Self, CompartmentError> {
        let mut registry = Self::new();

        // Query for all CompartmentDefinition resources
        let search_results = manager
            .search()
            .await
            .resource_type("CompartmentDefinition")
            .limit(100)
            .execute()
            .await
            .map_err(|e| {
                CompartmentError::LoadError(format!(
                    "Failed to search for CompartmentDefinition resources: {}",
                    e
                ))
            })?;

        tracing::debug!(
            count = search_results.resources.len(),
            "Found CompartmentDefinition resources"
        );

        for resource_match in &search_results.resources {
            match CompartmentDefinition::from_fhir_resource(&resource_match.resource.content) {
                Ok(def) => {
                    tracing::info!(
                        compartment = %def.name,
                        resource_types = def.resource_rules.len(),
                        "Loaded compartment definition"
                    );
                    registry.definitions.insert(def.name.clone(), def);
                }
                Err(e) => {
                    tracing::warn!(error = %e, "Failed to parse CompartmentDefinition");
                }
            }
        }

        if registry.definitions.is_empty() {
            return Err(CompartmentError::LoadError(
                "No compartment definitions found in packages".to_string(),
            ));
        }

        tracing::info!(
            compartments = %registry.definitions.keys().map(|k| k.as_str()).collect::<Vec<_>>().join(", "),
            "Compartment registry initialized"
        );

        Ok(registry)
    }

    /// Get a compartment definition by name
    pub fn get(&self, name: &str) -> Result<&CompartmentDefinition, CompartmentError> {
        self.definitions
            .get(name)
            .ok_or_else(|| CompartmentError::NotFound(name.to_string()))
    }

    /// List all available compartment names
    pub fn list_compartments(&self) -> Vec<&str> {
        self.definitions.keys().map(|s| s.as_str()).collect()
    }

    /// Check if a compartment is available
    pub fn has_compartment(&self, name: &str) -> bool {
        self.definitions.contains_key(name)
    }
}

impl Default for CompartmentRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compartment_definition_parsing() {
        let resource = serde_json::json!({
            "resourceType": "CompartmentDefinition",
            "name": "Patient",
            "url": "http://hl7.org/fhir/CompartmentDefinition/patient",
            "status": "active",
            "resource": [
                {
                    "code": "Observation",
                    "param": ["subject", "performer"]
                },
                {
                    "code": "Condition",
                    "param": ["subject", "asserter"]
                }
            ]
        });

        let def = CompartmentDefinition::from_fhir_resource(&resource).unwrap();

        assert_eq!(def.name, "Patient");
        assert_eq!(def.url, "http://hl7.org/fhir/CompartmentDefinition/patient");
        assert!(def.contains_resource_type("Observation"));
        assert!(def.contains_resource_type("Condition"));

        let obs_params = def.get_inclusion_params("Observation").unwrap();
        assert_eq!(obs_params, &["subject", "performer"]);

        let cond_params = def.get_inclusion_params("Condition").unwrap();
        assert_eq!(cond_params, &["subject", "asserter"]);
    }

    #[test]
    fn test_compartment_definition_resource_types() {
        let mut def =
            CompartmentDefinition::new("Patient".to_string(), "http://example.com".to_string());
        def.add_resource_rule("Observation".to_string(), vec!["subject".to_string()]);
        def.add_resource_rule("Condition".to_string(), vec!["subject".to_string()]);

        let types = def.resource_types();
        assert_eq!(types.len(), 2);
        assert!(types.contains(&"Observation"));
        assert!(types.contains(&"Condition"));
    }
}
