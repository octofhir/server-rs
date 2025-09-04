use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;
use crate::error::CoreError;

/// FHIR version enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FhirVersion {
    #[serde(rename = "4.1.0")]
    R4,
    #[serde(rename = "4.3.0")]
    R4B,
    #[serde(rename = "5.0.0")]
    R5,
}

impl fmt::Display for FhirVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FhirVersion::R4B => write!(f, "4.3.0"),
            FhirVersion::R5 => write!(f, "5.0.0"),
            &FhirVersion::R4 => write!(f, "4.1.0"),
        }
    }
}

impl FromStr for FhirVersion {
    type Err = CoreError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "4.1.0" | "R4" => Ok(FhirVersion::R4),
            "4.3.0" | "R4B" => Ok(FhirVersion::R4B),
            "5.0.0" | "R5" => Ok(FhirVersion::R5),
            _ => Err(CoreError::invalid_resource_type(format!("Unknown FHIR version: {}", s))),
        }
    }
}

impl Default for FhirVersion {
    fn default() -> Self {
        FhirVersion::R4B
    }
}

/// Common FHIR resource types for MVP
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ResourceType {
    Patient,
    Practitioner,
    Organization,
    Encounter,
    Observation,
    Condition,
    DiagnosticReport,
    Medication,
    MedicationRequest,
    Procedure,
    Specimen,
    DocumentReference,
    Bundle,
    CapabilityStatement,
    StructureDefinition,
    ValueSet,
    CodeSystem,
    SearchParameter,
    OperationOutcome,
    #[serde(untagged)]
    Custom(String),
}

impl fmt::Display for ResourceType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ResourceType::Patient => write!(f, "Patient"),
            ResourceType::Practitioner => write!(f, "Practitioner"),
            ResourceType::Organization => write!(f, "Organization"),
            ResourceType::Encounter => write!(f, "Encounter"),
            ResourceType::Observation => write!(f, "Observation"),
            ResourceType::Condition => write!(f, "Condition"),
            ResourceType::DiagnosticReport => write!(f, "DiagnosticReport"),
            ResourceType::Medication => write!(f, "Medication"),
            ResourceType::MedicationRequest => write!(f, "MedicationRequest"),
            ResourceType::Procedure => write!(f, "Procedure"),
            ResourceType::Specimen => write!(f, "Specimen"),
            ResourceType::DocumentReference => write!(f, "DocumentReference"),
            ResourceType::Bundle => write!(f, "Bundle"),
            ResourceType::CapabilityStatement => write!(f, "CapabilityStatement"),
            ResourceType::StructureDefinition => write!(f, "StructureDefinition"),
            ResourceType::ValueSet => write!(f, "ValueSet"),
            ResourceType::CodeSystem => write!(f, "CodeSystem"),
            ResourceType::SearchParameter => write!(f, "SearchParameter"),
            ResourceType::OperationOutcome => write!(f, "OperationOutcome"),
            ResourceType::Custom(name) => write!(f, "{}", name),
        }
    }
}

impl FromStr for ResourceType {
    type Err = CoreError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Patient" => Ok(ResourceType::Patient),
            "Practitioner" => Ok(ResourceType::Practitioner),
            "Organization" => Ok(ResourceType::Organization),
            "Encounter" => Ok(ResourceType::Encounter),
            "Observation" => Ok(ResourceType::Observation),
            "Condition" => Ok(ResourceType::Condition),
            "DiagnosticReport" => Ok(ResourceType::DiagnosticReport),
            "Medication" => Ok(ResourceType::Medication),
            "MedicationRequest" => Ok(ResourceType::MedicationRequest),
            "Procedure" => Ok(ResourceType::Procedure),
            "Specimen" => Ok(ResourceType::Specimen),
            "DocumentReference" => Ok(ResourceType::DocumentReference),
            "Bundle" => Ok(ResourceType::Bundle),
            "CapabilityStatement" => Ok(ResourceType::CapabilityStatement),
            "StructureDefinition" => Ok(ResourceType::StructureDefinition),
            "ValueSet" => Ok(ResourceType::ValueSet),
            "CodeSystem" => Ok(ResourceType::CodeSystem),
            "SearchParameter" => Ok(ResourceType::SearchParameter),
            "OperationOutcome" => Ok(ResourceType::OperationOutcome),
            name => {
                // Validate custom resource type name matches FHIR requirements
                if is_valid_resource_type_name(name) {
                    Ok(ResourceType::Custom(name.to_string()))
                } else {
                    Err(CoreError::invalid_resource_type(name.to_string()))
                }
            }
        }
    }
}

/// Validate if a string is a valid FHIR resource type name
pub fn is_valid_resource_type_name(name: &str) -> bool {
    // FHIR resource type names must start with uppercase letter and contain only letters
    !name.is_empty() && 
    name.chars().next().map(|c| c.is_ascii_uppercase()).unwrap_or(false) &&
    name.chars().all(|c| c.is_ascii_alphabetic())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fhir_version_display() {
        assert_eq!(FhirVersion::R4B.to_string(), "4.3.0");
        assert_eq!(FhirVersion::R5.to_string(), "5.0.0");
    }

    #[test]
    fn test_fhir_version_from_str() {
        assert_eq!(FhirVersion::from_str("4.3.0").unwrap(), FhirVersion::R4B);
        assert_eq!(FhirVersion::from_str("R4B").unwrap(), FhirVersion::R4B);
        assert_eq!(FhirVersion::from_str("5.0.0").unwrap(), FhirVersion::R5);
        assert_eq!(FhirVersion::from_str("R5").unwrap(), FhirVersion::R5);
        
        assert!(FhirVersion::from_str("invalid").is_err());
        assert!(FhirVersion::from_str("4.0.0").is_err());
    }

    #[test]
    fn test_fhir_version_default() {
        assert_eq!(FhirVersion::default(), FhirVersion::R4B);
    }

    #[test]
    fn test_fhir_version_serialization() {
        let version = FhirVersion::R4B;
        let json = serde_json::to_string(&version).unwrap();
        assert_eq!(json, "\"4.3.0\"");
        
        let version = FhirVersion::R5;
        let json = serde_json::to_string(&version).unwrap();
        assert_eq!(json, "\"5.0.0\"");
    }

    #[test]
    fn test_fhir_version_deserialization() {
        let version: FhirVersion = serde_json::from_str("\"4.3.0\"").unwrap();
        assert_eq!(version, FhirVersion::R4B);
        
        let version: FhirVersion = serde_json::from_str("\"5.0.0\"").unwrap();
        assert_eq!(version, FhirVersion::R5);
    }

    #[test]
    fn test_resource_type_from_str() {
        assert_eq!(ResourceType::from_str("Patient").unwrap(), ResourceType::Patient);
        assert_eq!(ResourceType::from_str("Organization").unwrap(), ResourceType::Organization);
        assert_eq!(ResourceType::from_str("CapabilityStatement").unwrap(), ResourceType::CapabilityStatement);
        
        // Test custom resource type
        assert_eq!(ResourceType::from_str("CustomResource").unwrap(), ResourceType::Custom("CustomResource".to_string()));
        
        // Test invalid cases
        assert!(ResourceType::from_str("invalidResource").is_err()); // doesn't start with uppercase
        assert!(ResourceType::from_str("Invalid123").is_err()); // contains numbers
        assert!(ResourceType::from_str("").is_err()); // empty string
    }

    #[test]
    fn test_resource_type_display() {
        assert_eq!(ResourceType::Patient.to_string(), "Patient");
        assert_eq!(ResourceType::Organization.to_string(), "Organization");
        assert_eq!(ResourceType::Custom("MyResource".to_string()).to_string(), "MyResource");
    }

    #[test]
    fn test_resource_type_serialization() {
        let resource_type = ResourceType::Patient;
        let json = serde_json::to_string(&resource_type).unwrap();
        assert_eq!(json, "\"Patient\"");
        
        let custom_type = ResourceType::Custom("TestResource".to_string());
        let json = serde_json::to_string(&custom_type).unwrap();
        assert_eq!(json, "\"TestResource\"");
    }

    #[test] 
    fn test_resource_type_deserialization() {
        let resource_type: ResourceType = serde_json::from_str("\"Patient\"").unwrap();
        assert_eq!(resource_type, ResourceType::Patient);
        
        let resource_type: ResourceType = serde_json::from_str("\"Observation\"").unwrap();
        assert_eq!(resource_type, ResourceType::Observation);
    }

    #[test]
    fn test_is_valid_resource_type_name() {
        assert!(is_valid_resource_type_name("Patient"));
        assert!(is_valid_resource_type_name("CustomResource"));
        assert!(is_valid_resource_type_name("A"));
        
        assert!(!is_valid_resource_type_name("patient")); // lowercase start
        assert!(!is_valid_resource_type_name("123Patient")); // starts with number
        assert!(!is_valid_resource_type_name("Patient123")); // contains number
        assert!(!is_valid_resource_type_name("Patient-Type")); // contains dash
        assert!(!is_valid_resource_type_name("")); // empty
    }

    #[test]
    fn test_resource_type_equality() {
        assert_eq!(ResourceType::Patient, ResourceType::Patient);
        assert_ne!(ResourceType::Patient, ResourceType::Organization);
        
        let custom1 = ResourceType::Custom("Test".to_string());
        let custom2 = ResourceType::Custom("Test".to_string());
        let custom3 = ResourceType::Custom("Different".to_string());
        
        assert_eq!(custom1, custom2);
        assert_ne!(custom1, custom3);
    }

    #[test]
    fn test_resource_type_hashing() {
        use std::collections::HashMap;
        
        let mut map = HashMap::new();
        map.insert(ResourceType::Patient, "patient data");
        map.insert(ResourceType::Organization, "org data");
        
        assert_eq!(map.get(&ResourceType::Patient), Some(&"patient data"));
        assert_eq!(map.get(&ResourceType::Organization), Some(&"org data"));
        assert_eq!(map.get(&ResourceType::Observation), None);
    }

    #[test]
    fn test_all_standard_resource_types_parse() {
        let standard_types = [
            "Patient", "Practitioner", "Organization", "Encounter", 
            "Observation", "Condition", "DiagnosticReport", "Medication",
            "MedicationRequest", "Procedure", "Specimen", "DocumentReference",
            "Bundle", "CapabilityStatement", "StructureDefinition", "ValueSet",
            "CodeSystem", "SearchParameter", "OperationOutcome"
        ];
        
        for type_name in &standard_types {
            let parsed = ResourceType::from_str(type_name);
            assert!(parsed.is_ok(), "Failed to parse resource type: {}", type_name);
            assert_eq!(parsed.unwrap().to_string(), *type_name);
        }
    }

    #[test]
    fn test_resource_type_roundtrip() {
        let types = [
            ResourceType::Patient,
            ResourceType::Organization,
            ResourceType::CapabilityStatement,
            ResourceType::Custom("TestResource".to_string())
        ];
        
        for resource_type in &types {
            let as_string = resource_type.to_string();
            let parsed_back = ResourceType::from_str(&as_string).unwrap();
            assert_eq!(*resource_type, parsed_back);
        }
    }

    #[test]
    fn test_fhir_version_copy_semantics() {
        let version1 = FhirVersion::R4B;
        let version2 = version1; // Should copy, not move
        assert_eq!(version1, version2);
        assert_eq!(version1, FhirVersion::R4B); // version1 should still be valid
    }

    #[test]
    fn test_error_messages() {
        match ResourceType::from_str("invalidType") {
            Err(CoreError::InvalidResourceType(msg)) => {
                assert!(msg.contains("invalidType"));
            }
            _ => panic!("Expected InvalidResourceType error"),
        }
        
        match FhirVersion::from_str("unknown") {
            Err(CoreError::InvalidResourceType(msg)) => {
                assert!(msg.contains("unknown"));
            }
            _ => panic!("Expected InvalidResourceType error"),
        }
    }
}