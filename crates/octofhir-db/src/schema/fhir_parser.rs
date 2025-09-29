// Parser for FHIR StructureDefinition resources
// Extracts element definitions and converts them to ElementDescriptor

use super::element::{ElementDescriptor, ElementType};
use serde_json::Value;

/// Parse a FHIR StructureDefinition JSON and extract element descriptors
pub fn parse_structure_definition(sd_json: &str) -> Result<Vec<ElementDescriptor>, ParseError> {
    let json: Value = serde_json::from_str(sd_json)
        .map_err(|e| ParseError::InvalidJson(e.to_string()))?;

    // Verify this is a StructureDefinition
    if json.get("resourceType").and_then(|v| v.as_str()) != Some("StructureDefinition") {
        return Err(ParseError::NotStructureDefinition);
    }

    let resource_type = json
        .get("type")
        .and_then(|v| v.as_str())
        .ok_or(ParseError::MissingField("type".to_string()))?;

    // Extract elements from snapshot (preferred) or differential
    let elements = if let Some(snapshot) = json.get("snapshot") {
        extract_elements_from_snapshot(snapshot, resource_type)?
    } else if let Some(differential) = json.get("differential") {
        extract_elements_from_differential(differential, resource_type)?
    } else {
        return Err(ParseError::MissingField("snapshot or differential".to_string()));
    };

    Ok(elements)
}

fn extract_elements_from_snapshot(
    snapshot: &Value,
    _resource_type: &str,
) -> Result<Vec<ElementDescriptor>, ParseError> {
    let element_array = snapshot
        .get("element")
        .and_then(|e| e.as_array())
        .ok_or(ParseError::MissingField("snapshot.element".to_string()))?;

    let mut descriptors = Vec::new();

    for element in element_array {
        if let Some(descriptor) = parse_element(element)? {
            descriptors.push(descriptor);
        }
    }

    Ok(descriptors)
}

fn extract_elements_from_differential(
    differential: &Value,
    _resource_type: &str,
) -> Result<Vec<ElementDescriptor>, ParseError> {
    let element_array = differential
        .get("element")
        .and_then(|e| e.as_array())
        .ok_or(ParseError::MissingField("differential.element".to_string()))?;

    let mut descriptors = Vec::new();

    for element in element_array {
        if let Some(descriptor) = parse_element(element)? {
            descriptors.push(descriptor);
        }
    }

    Ok(descriptors)
}

fn parse_element(element: &Value) -> Result<Option<ElementDescriptor>, ParseError> {
    // Get the element path
    let path = element
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or(ParseError::MissingField("element.path".to_string()))?;

    // Skip extension elements for now
    if path.contains(".extension") || path.contains(".modifierExtension") {
        return Ok(None);
    }

    let mut descriptor = ElementDescriptor::new(path.to_string());

    // Parse cardinality (min, max)
    if let Some(min) = element.get("min").and_then(|v| v.as_u64()) {
        descriptor.min = min as usize;
    }

    if let Some(max_val) = element.get("max") {
        if let Some(max_str) = max_val.as_str() {
            descriptor.max = if max_str == "*" {
                None
            } else {
                max_str.parse::<usize>().ok()
            };
        }
    }

    // Parse types
    if let Some(type_array) = element.get("type").and_then(|t| t.as_array()) {
        for type_obj in type_array {
            if let Some(code) = type_obj.get("code").and_then(|c| c.as_str()) {
                if let Some(element_type) = map_fhir_type_code(code) {
                    descriptor.types.push(element_type);
                }
            }
        }
    }

    // Parse modifiers and mustSupport flags
    if let Some(is_modifier) = element.get("isModifier").and_then(|v| v.as_bool()) {
        descriptor.is_modifier = is_modifier;
    }

    if let Some(must_support) = element.get("mustSupport").and_then(|v| v.as_bool()) {
        descriptor.must_support = must_support;
    }

    // Parse short description
    if let Some(short) = element.get("short").and_then(|v| v.as_str()) {
        descriptor.short = Some(short.to_string());
    }

    // Parse definition
    if let Some(definition) = element.get("definition").and_then(|v| v.as_str()) {
        descriptor.definition = Some(definition.to_string());
    }

    Ok(Some(descriptor))
}

/// Map FHIR type codes to our ElementType enum
fn map_fhir_type_code(code: &str) -> Option<ElementType> {
    match code {
        "boolean" => Some(ElementType::Boolean),
        "integer" => Some(ElementType::Integer),
        "string" => Some(ElementType::String),
        "decimal" => Some(ElementType::Decimal),
        "uri" => Some(ElementType::Uri),
        "url" => Some(ElementType::Url),
        "canonical" => Some(ElementType::Canonical),
        "base64Binary" => Some(ElementType::Base64Binary),
        "instant" => Some(ElementType::Instant),
        "date" => Some(ElementType::Date),
        "dateTime" => Some(ElementType::DateTime),
        "time" => Some(ElementType::Time),
        "code" => Some(ElementType::Code),
        "oid" => Some(ElementType::Oid),
        "id" => Some(ElementType::Id),
        "markdown" => Some(ElementType::Markdown),
        "unsignedInt" => Some(ElementType::UnsignedInt),
        "positiveInt" => Some(ElementType::PositiveInt),
        "uuid" => Some(ElementType::Uuid),
        "HumanName" => Some(ElementType::HumanName),
        "Address" => Some(ElementType::Address),
        "ContactPoint" => Some(ElementType::ContactPoint),
        "Identifier" => Some(ElementType::Identifier),
        "CodeableConcept" => Some(ElementType::CodeableConcept),
        "Coding" => Some(ElementType::Coding),
        "Quantity" => Some(ElementType::Quantity),
        "Range" => Some(ElementType::Range),
        "Period" => Some(ElementType::Period),
        "Ratio" => Some(ElementType::Ratio),
        "Reference" => Some(ElementType::Reference),
        "Attachment" => Some(ElementType::Attachment),
        "Meta" => Some(ElementType::Meta),
        "Narrative" => Some(ElementType::Narrative),
        "Extension" => Some(ElementType::Extension),
        "BackboneElement" => Some(ElementType::BackboneElement),
        // Resource references
        "Resource" => Some(ElementType::Resource("Resource".to_string())),
        _ => {
            // Check if it's a resource type reference (e.g., "Patient", "Observation")
            if code.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) {
                Some(ElementType::Resource(code.to_string()))
            } else {
                None
            }
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("Invalid JSON: {0}")]
    InvalidJson(String),

    #[error("Not a StructureDefinition resource")]
    NotStructureDefinition,

    #[error("Missing required field: {0}")]
    MissingField(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_structure_definition() {
        let sd_json = r#"{
            "resourceType": "StructureDefinition",
            "type": "Patient",
            "snapshot": {
                "element": [
                    {
                        "path": "Patient",
                        "min": 0,
                        "max": "*"
                    },
                    {
                        "path": "Patient.id",
                        "min": 0,
                        "max": "1",
                        "type": [{"code": "id"}]
                    },
                    {
                        "path": "Patient.active",
                        "min": 0,
                        "max": "1",
                        "type": [{"code": "boolean"}],
                        "short": "Whether this patient's record is in active use"
                    }
                ]
            }
        }"#;

        let elements = parse_structure_definition(sd_json).unwrap();

        // Should have 3 elements (Patient, Patient.id, Patient.active)
        assert_eq!(elements.len(), 3);

        // Check Patient.id
        let id_elem = elements.iter().find(|e| e.path == "Patient.id").unwrap();
        assert_eq!(id_elem.types.len(), 1);
        assert_eq!(id_elem.types[0], ElementType::Id);

        // Check Patient.active
        let active_elem = elements.iter().find(|e| e.path == "Patient.active").unwrap();
        assert_eq!(active_elem.types[0], ElementType::Boolean);
        assert!(active_elem.short.is_some());
    }

    #[test]
    fn test_parse_polymorphic_element() {
        let sd_json = r#"{
            "resourceType": "StructureDefinition",
            "type": "Observation",
            "snapshot": {
                "element": [
                    {
                        "path": "Observation.value[x]",
                        "min": 0,
                        "max": "1",
                        "type": [
                            {"code": "Quantity"},
                            {"code": "string"},
                            {"code": "boolean"}
                        ]
                    }
                ]
            }
        }"#;

        let elements = parse_structure_definition(sd_json).unwrap();

        assert_eq!(elements.len(), 1);
        let value_elem = &elements[0];
        assert_eq!(value_elem.path, "Observation.value[x]");
        assert_eq!(value_elem.types.len(), 3);
        assert!(value_elem.types.contains(&ElementType::Quantity));
        assert!(value_elem.types.contains(&ElementType::String));
        assert!(value_elem.types.contains(&ElementType::Boolean));
    }

    #[test]
    fn test_parse_cardinality() {
        let sd_json = r#"{
            "resourceType": "StructureDefinition",
            "type": "Patient",
            "snapshot": {
                "element": [
                    {
                        "path": "Patient.name",
                        "min": 0,
                        "max": "*",
                        "type": [{"code": "HumanName"}]
                    }
                ]
            }
        }"#;

        let elements = parse_structure_definition(sd_json).unwrap();

        assert_eq!(elements.len(), 1);
        let name_elem = &elements[0];
        assert_eq!(name_elem.min, 0);
        assert_eq!(name_elem.max, None); // * means unbounded
    }

    #[test]
    fn test_map_fhir_type_codes() {
        assert_eq!(
            map_fhir_type_code("boolean"),
            Some(ElementType::Boolean)
        );
        assert_eq!(map_fhir_type_code("string"), Some(ElementType::String));
        assert_eq!(
            map_fhir_type_code("Quantity"),
            Some(ElementType::Quantity)
        );
        assert_eq!(
            map_fhir_type_code("Reference"),
            Some(ElementType::Reference)
        );

        // Resource types
        assert_eq!(
            map_fhir_type_code("Patient"),
            Some(ElementType::Resource("Patient".to_string()))
        );
    }
}