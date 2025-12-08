//! FHIR element path resolver for LSP completions.
//!
//! This module provides element path completion from FHIR StructureDefinitions
//! using the canonical manager.

use dashmap::DashMap;
use serde_json::Value;

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
    /// Cache of elements by resource type
    element_cache: DashMap<String, Vec<ElementInfo>>,
}

impl FhirResolver {
    /// Creates a new FHIR resolver.
    pub fn new() -> Self {
        Self {
            element_cache: DashMap::new(),
        }
    }

    /// Get elements for a resource type from the canonical manager.
    pub async fn get_elements(&self, resource_type: &str) -> Vec<ElementInfo> {
        // Check cache first
        if let Some(cached) = self.element_cache.get(resource_type) {
            return cached.value().clone();
        }

        // Try to load from canonical manager
        if let Some(elements) = self.load_elements_from_manager(resource_type).await {
            self.element_cache.insert(resource_type.to_string(), elements.clone());
            return elements;
        }

        // Fall back to common FHIR elements
        self.get_common_elements(resource_type)
    }

    /// Load elements from the canonical manager.
    async fn load_elements_from_manager(&self, resource_type: &str) -> Option<Vec<ElementInfo>> {
        let manager = crate::canonical::get_manager()?;

        // Query for the StructureDefinition
        // Note: The canonical manager search API uses resource_type and limit,
        // we then filter the results locally for the specific resource type
        let search_result = manager
            .search()
            .await
            .resource_type("StructureDefinition")
            .limit(1000)
            .execute()
            .await
            .ok()?;

        // Find the StructureDefinition for this resource type
        for resource_match in &search_result.resources {
            let resource = &resource_match.resource;
            let content = &resource.content;

            // Check if this is the resource type we're looking for
            let type_value = content.get("type").and_then(|t| t.as_str());
            let kind_value = content.get("kind").and_then(|k| k.as_str());

            match (type_value, kind_value) {
                (Some(t), Some(k)) if t.eq_ignore_ascii_case(resource_type) && k == "resource" => {
                    return self.parse_structure_definition(content);
                }
                _ => continue,
            }
        }

        None
    }

    /// Parse a StructureDefinition to extract element info.
    fn parse_structure_definition(&self, sd: &Value) -> Option<Vec<ElementInfo>> {
        let snapshot = sd.get("snapshot")?;
        let elements_arr = snapshot.get("element")?.as_array()?;

        let mut elements = Vec::new();

        for elem in elements_arr {
            if let Some(info) = self.parse_element(elem) {
                elements.push(info);
            }
        }

        Some(elements)
    }

    /// Parse a single element from the snapshot.
    fn parse_element(&self, elem: &Value) -> Option<ElementInfo> {
        let path = elem.get("path")?.as_str()?.to_string();

        // Extract element name from path (last segment)
        let name = path.rsplit('.').next()?.to_string();

        // Get type code
        let type_code = elem
            .get("type")
            .and_then(|t| t.as_array())
            .and_then(|arr| arr.first())
            .and_then(|t| t.get("code"))
            .and_then(|c| c.as_str())
            .unwrap_or("BackboneElement")
            .to_string();

        // Get cardinality
        let min = elem.get("min").and_then(|m| m.as_u64()).unwrap_or(0) as u32;
        let max_str = elem.get("max").and_then(|m| m.as_str()).unwrap_or("1");
        let max = if max_str == "*" { 0 } else { max_str.parse().unwrap_or(1) };

        // Get descriptions
        let short = elem.get("short").and_then(|s| s.as_str()).map(|s| s.to_string());
        let definition = elem.get("definition").and_then(|d| d.as_str()).map(|s| s.to_string());

        let is_array = max_str == "*" || max > 1;
        let is_backbone = type_code == "BackboneElement" || type_code == "Element";

        Some(ElementInfo {
            path,
            name,
            type_code,
            min,
            max,
            short,
            definition,
            is_array,
            is_backbone,
        })
    }

    /// Get children of a path (direct descendants only).
    pub async fn get_children(&self, resource_type: &str, parent_path: &str) -> Vec<ElementInfo> {
        let elements = self.get_elements(resource_type).await;

        let expected_prefix = if parent_path.is_empty() {
            resource_type.to_string()
        } else {
            format!("{}.", parent_path)
        };

        elements
            .into_iter()
            .filter(|elem| {
                // Must start with parent path
                if !elem.path.starts_with(&expected_prefix) {
                    return false;
                }

                // Must be a direct child (no additional dots after prefix)
                let remaining = &elem.path[expected_prefix.len()..];
                !remaining.contains('.')
            })
            .collect()
    }

    /// Get common FHIR elements as fallback when canonical manager is unavailable.
    fn get_common_elements(&self, resource_type: &str) -> Vec<ElementInfo> {
        // Common Resource elements
        let mut elements = vec![
            ElementInfo {
                path: format!("{}.id", resource_type),
                name: "id".to_string(),
                type_code: "id".to_string(),
                min: 0,
                max: 1,
                short: Some("Logical id of this artifact".to_string()),
                definition: Some("The logical id of the resource".to_string()),
                is_array: false,
                is_backbone: false,
            },
            ElementInfo {
                path: format!("{}.meta", resource_type),
                name: "meta".to_string(),
                type_code: "Meta".to_string(),
                min: 0,
                max: 1,
                short: Some("Metadata about the resource".to_string()),
                definition: Some("The metadata about the resource".to_string()),
                is_array: false,
                is_backbone: false,
            },
            ElementInfo {
                path: format!("{}.implicitRules", resource_type),
                name: "implicitRules".to_string(),
                type_code: "uri".to_string(),
                min: 0,
                max: 1,
                short: Some("A set of rules under which this content was created".to_string()),
                definition: None,
                is_array: false,
                is_backbone: false,
            },
            ElementInfo {
                path: format!("{}.language", resource_type),
                name: "language".to_string(),
                type_code: "code".to_string(),
                min: 0,
                max: 1,
                short: Some("Language of the resource content".to_string()),
                definition: None,
                is_array: false,
                is_backbone: false,
            },
            ElementInfo {
                path: format!("{}.text", resource_type),
                name: "text".to_string(),
                type_code: "Narrative".to_string(),
                min: 0,
                max: 1,
                short: Some("Text summary of the resource".to_string()),
                definition: None,
                is_array: false,
                is_backbone: false,
            },
            ElementInfo {
                path: format!("{}.contained", resource_type),
                name: "contained".to_string(),
                type_code: "Resource".to_string(),
                min: 0,
                max: 0,
                short: Some("Contained resources".to_string()),
                definition: None,
                is_array: true,
                is_backbone: false,
            },
            ElementInfo {
                path: format!("{}.extension", resource_type),
                name: "extension".to_string(),
                type_code: "Extension".to_string(),
                min: 0,
                max: 0,
                short: Some("Additional content defined by implementations".to_string()),
                definition: None,
                is_array: true,
                is_backbone: false,
            },
            ElementInfo {
                path: format!("{}.modifierExtension", resource_type),
                name: "modifierExtension".to_string(),
                type_code: "Extension".to_string(),
                min: 0,
                max: 0,
                short: Some("Extensions that cannot be ignored".to_string()),
                definition: None,
                is_array: true,
                is_backbone: false,
            },
        ];

        // Add common elements for specific resource types
        match resource_type {
            "Patient" => {
                elements.extend(vec![
                    ElementInfo {
                        path: format!("{}.identifier", resource_type),
                        name: "identifier".to_string(),
                        type_code: "Identifier".to_string(),
                        min: 0,
                        max: 0,
                        short: Some("An identifier for this patient".to_string()),
                        definition: None,
                        is_array: true,
                        is_backbone: false,
                    },
                    ElementInfo {
                        path: format!("{}.active", resource_type),
                        name: "active".to_string(),
                        type_code: "boolean".to_string(),
                        min: 0,
                        max: 1,
                        short: Some("Whether this patient record is in active use".to_string()),
                        definition: None,
                        is_array: false,
                        is_backbone: false,
                    },
                    ElementInfo {
                        path: format!("{}.name", resource_type),
                        name: "name".to_string(),
                        type_code: "HumanName".to_string(),
                        min: 0,
                        max: 0,
                        short: Some("A name associated with the patient".to_string()),
                        definition: None,
                        is_array: true,
                        is_backbone: false,
                    },
                    ElementInfo {
                        path: format!("{}.telecom", resource_type),
                        name: "telecom".to_string(),
                        type_code: "ContactPoint".to_string(),
                        min: 0,
                        max: 0,
                        short: Some("A contact detail for the individual".to_string()),
                        definition: None,
                        is_array: true,
                        is_backbone: false,
                    },
                    ElementInfo {
                        path: format!("{}.gender", resource_type),
                        name: "gender".to_string(),
                        type_code: "code".to_string(),
                        min: 0,
                        max: 1,
                        short: Some("male | female | other | unknown".to_string()),
                        definition: None,
                        is_array: false,
                        is_backbone: false,
                    },
                    ElementInfo {
                        path: format!("{}.birthDate", resource_type),
                        name: "birthDate".to_string(),
                        type_code: "date".to_string(),
                        min: 0,
                        max: 1,
                        short: Some("The date of birth for the individual".to_string()),
                        definition: None,
                        is_array: false,
                        is_backbone: false,
                    },
                    ElementInfo {
                        path: format!("{}.deceased[x]", resource_type),
                        name: "deceased[x]".to_string(),
                        type_code: "boolean|dateTime".to_string(),
                        min: 0,
                        max: 1,
                        short: Some("Indicates if the individual is deceased or not".to_string()),
                        definition: None,
                        is_array: false,
                        is_backbone: false,
                    },
                    ElementInfo {
                        path: format!("{}.address", resource_type),
                        name: "address".to_string(),
                        type_code: "Address".to_string(),
                        min: 0,
                        max: 0,
                        short: Some("An address for the individual".to_string()),
                        definition: None,
                        is_array: true,
                        is_backbone: false,
                    },
                    ElementInfo {
                        path: format!("{}.maritalStatus", resource_type),
                        name: "maritalStatus".to_string(),
                        type_code: "CodeableConcept".to_string(),
                        min: 0,
                        max: 1,
                        short: Some("Marital (civil) status of a patient".to_string()),
                        definition: None,
                        is_array: false,
                        is_backbone: false,
                    },
                    ElementInfo {
                        path: format!("{}.multipleBirth[x]", resource_type),
                        name: "multipleBirth[x]".to_string(),
                        type_code: "boolean|integer".to_string(),
                        min: 0,
                        max: 1,
                        short: Some("Whether patient is part of a multiple birth".to_string()),
                        definition: None,
                        is_array: false,
                        is_backbone: false,
                    },
                    ElementInfo {
                        path: format!("{}.photo", resource_type),
                        name: "photo".to_string(),
                        type_code: "Attachment".to_string(),
                        min: 0,
                        max: 0,
                        short: Some("Image of the patient".to_string()),
                        definition: None,
                        is_array: true,
                        is_backbone: false,
                    },
                    ElementInfo {
                        path: format!("{}.contact", resource_type),
                        name: "contact".to_string(),
                        type_code: "BackboneElement".to_string(),
                        min: 0,
                        max: 0,
                        short: Some("A contact party for the patient".to_string()),
                        definition: None,
                        is_array: true,
                        is_backbone: true,
                    },
                    ElementInfo {
                        path: format!("{}.communication", resource_type),
                        name: "communication".to_string(),
                        type_code: "BackboneElement".to_string(),
                        min: 0,
                        max: 0,
                        short: Some("A language the patient can use".to_string()),
                        definition: None,
                        is_array: true,
                        is_backbone: true,
                    },
                    ElementInfo {
                        path: format!("{}.generalPractitioner", resource_type),
                        name: "generalPractitioner".to_string(),
                        type_code: "Reference".to_string(),
                        min: 0,
                        max: 0,
                        short: Some("Patient's nominated primary care provider".to_string()),
                        definition: None,
                        is_array: true,
                        is_backbone: false,
                    },
                    ElementInfo {
                        path: format!("{}.managingOrganization", resource_type),
                        name: "managingOrganization".to_string(),
                        type_code: "Reference".to_string(),
                        min: 0,
                        max: 1,
                        short: Some("Organization that is the custodian of the patient record".to_string()),
                        definition: None,
                        is_array: false,
                        is_backbone: false,
                    },
                    ElementInfo {
                        path: format!("{}.link", resource_type),
                        name: "link".to_string(),
                        type_code: "BackboneElement".to_string(),
                        min: 0,
                        max: 0,
                        short: Some("Link to another patient resource".to_string()),
                        definition: None,
                        is_array: true,
                        is_backbone: true,
                    },
                ]);
            }
            "Observation" => {
                elements.extend(vec![
                    ElementInfo {
                        path: format!("{}.identifier", resource_type),
                        name: "identifier".to_string(),
                        type_code: "Identifier".to_string(),
                        min: 0,
                        max: 0,
                        short: Some("Business Identifier for observation".to_string()),
                        definition: None,
                        is_array: true,
                        is_backbone: false,
                    },
                    ElementInfo {
                        path: format!("{}.status", resource_type),
                        name: "status".to_string(),
                        type_code: "code".to_string(),
                        min: 1,
                        max: 1,
                        short: Some("registered | preliminary | final | amended +".to_string()),
                        definition: None,
                        is_array: false,
                        is_backbone: false,
                    },
                    ElementInfo {
                        path: format!("{}.category", resource_type),
                        name: "category".to_string(),
                        type_code: "CodeableConcept".to_string(),
                        min: 0,
                        max: 0,
                        short: Some("Classification of type of observation".to_string()),
                        definition: None,
                        is_array: true,
                        is_backbone: false,
                    },
                    ElementInfo {
                        path: format!("{}.code", resource_type),
                        name: "code".to_string(),
                        type_code: "CodeableConcept".to_string(),
                        min: 1,
                        max: 1,
                        short: Some("Type of observation (code / type)".to_string()),
                        definition: None,
                        is_array: false,
                        is_backbone: false,
                    },
                    ElementInfo {
                        path: format!("{}.subject", resource_type),
                        name: "subject".to_string(),
                        type_code: "Reference".to_string(),
                        min: 0,
                        max: 1,
                        short: Some("Who/what is the subject of the observation".to_string()),
                        definition: None,
                        is_array: false,
                        is_backbone: false,
                    },
                    ElementInfo {
                        path: format!("{}.effective[x]", resource_type),
                        name: "effective[x]".to_string(),
                        type_code: "dateTime|Period|Timing|instant".to_string(),
                        min: 0,
                        max: 1,
                        short: Some("Clinically relevant time/time-period".to_string()),
                        definition: None,
                        is_array: false,
                        is_backbone: false,
                    },
                    ElementInfo {
                        path: format!("{}.value[x]", resource_type),
                        name: "value[x]".to_string(),
                        type_code: "Quantity|CodeableConcept|string|boolean|integer|Range|Ratio|SampledData|time|dateTime|Period".to_string(),
                        min: 0,
                        max: 1,
                        short: Some("Actual result".to_string()),
                        definition: None,
                        is_array: false,
                        is_backbone: false,
                    },
                    ElementInfo {
                        path: format!("{}.interpretation", resource_type),
                        name: "interpretation".to_string(),
                        type_code: "CodeableConcept".to_string(),
                        min: 0,
                        max: 0,
                        short: Some("High, low, normal, etc.".to_string()),
                        definition: None,
                        is_array: true,
                        is_backbone: false,
                    },
                    ElementInfo {
                        path: format!("{}.note", resource_type),
                        name: "note".to_string(),
                        type_code: "Annotation".to_string(),
                        min: 0,
                        max: 0,
                        short: Some("Comments about the observation".to_string()),
                        definition: None,
                        is_array: true,
                        is_backbone: false,
                    },
                    ElementInfo {
                        path: format!("{}.bodySite", resource_type),
                        name: "bodySite".to_string(),
                        type_code: "CodeableConcept".to_string(),
                        min: 0,
                        max: 1,
                        short: Some("Observed body part".to_string()),
                        definition: None,
                        is_array: false,
                        is_backbone: false,
                    },
                    ElementInfo {
                        path: format!("{}.method", resource_type),
                        name: "method".to_string(),
                        type_code: "CodeableConcept".to_string(),
                        min: 0,
                        max: 1,
                        short: Some("How it was done".to_string()),
                        definition: None,
                        is_array: false,
                        is_backbone: false,
                    },
                    ElementInfo {
                        path: format!("{}.specimen", resource_type),
                        name: "specimen".to_string(),
                        type_code: "Reference".to_string(),
                        min: 0,
                        max: 1,
                        short: Some("Specimen used for this observation".to_string()),
                        definition: None,
                        is_array: false,
                        is_backbone: false,
                    },
                    ElementInfo {
                        path: format!("{}.device", resource_type),
                        name: "device".to_string(),
                        type_code: "Reference".to_string(),
                        min: 0,
                        max: 1,
                        short: Some("(Measurement) Device".to_string()),
                        definition: None,
                        is_array: false,
                        is_backbone: false,
                    },
                    ElementInfo {
                        path: format!("{}.referenceRange", resource_type),
                        name: "referenceRange".to_string(),
                        type_code: "BackboneElement".to_string(),
                        min: 0,
                        max: 0,
                        short: Some("Provides guide for interpretation".to_string()),
                        definition: None,
                        is_array: true,
                        is_backbone: true,
                    },
                    ElementInfo {
                        path: format!("{}.component", resource_type),
                        name: "component".to_string(),
                        type_code: "BackboneElement".to_string(),
                        min: 0,
                        max: 0,
                        short: Some("Component results".to_string()),
                        definition: None,
                        is_array: true,
                        is_backbone: true,
                    },
                ]);
            }
            _ => {
                // Add common DomainResource elements for any resource type
                elements.push(ElementInfo {
                    path: format!("{}.identifier", resource_type),
                    name: "identifier".to_string(),
                    type_code: "Identifier".to_string(),
                    min: 0,
                    max: 0,
                    short: Some("Business identifiers".to_string()),
                    definition: None,
                    is_array: true,
                    is_backbone: false,
                });
            }
        }

        elements
    }

    /// Clear the element cache.
    pub fn clear_cache(&self) {
        self.element_cache.clear();
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
}

impl Default for FhirResolver {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_common_elements() {
        let resolver = FhirResolver::new();
        let elements = resolver.get_common_elements("Patient");

        // Should have common Resource elements plus Patient-specific
        assert!(elements.len() > 8);

        // Check for specific Patient elements
        let has_name = elements.iter().any(|e| e.name == "name");
        let has_gender = elements.iter().any(|e| e.name == "gender");
        let has_birthdate = elements.iter().any(|e| e.name == "birthDate");

        assert!(has_name, "Patient should have name element");
        assert!(has_gender, "Patient should have gender element");
        assert!(has_birthdate, "Patient should have birthDate element");
    }

    #[test]
    fn test_element_info_structure() {
        let resolver = FhirResolver::new();
        let elements = resolver.get_common_elements("Observation");

        // Find the status element which is required
        let status = elements.iter().find(|e| e.name == "status");
        assert!(status.is_some(), "Observation should have status element");

        let status = status.unwrap();
        assert_eq!(status.min, 1, "status should be required");
        assert!(!status.is_array, "status should not be an array");
    }
}
