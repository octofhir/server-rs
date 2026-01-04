//! FHIR Schema to JSON Schema converter.
//!
//! Converts FHIR Schema definitions to JSON Schema format for use in Monaco editor
//! autocomplete and validation. The conversion follows JSON Schema Draft-07.

use octofhir_fhirschema::types::{FhirSchema, FhirSchemaElement};
use serde_json::{json, Map, Value};
use std::collections::HashMap;

/// FHIR primitive type regex patterns for validation
const FHIR_DATE_PATTERN: &str =
    r"^([0-9]([0-9]([0-9][1-9]|[1-9]0)|[1-9]00)|[1-9]000)(-(0[1-9]|1[0-2])(-(0[1-9]|[1-2][0-9]|3[0-1]))?)?$";
const FHIR_DATETIME_PATTERN: &str = r"^([0-9]([0-9]([0-9][1-9]|[1-9]0)|[1-9]00)|[1-9]000)(-(0[1-9]|1[0-2])(-(0[1-9]|[1-2][0-9]|3[0-1])(T([01][0-9]|2[0-3]):[0-5][0-9]:([0-5][0-9]|60)(\.[0-9]+)?(Z|(\+|-)((0[0-9]|1[0-3]):[0-5][0-9]|14:00)))?)?)?$";
const FHIR_TIME_PATTERN: &str = r"^([01][0-9]|2[0-3]):[0-5][0-9]:([0-5][0-9]|60)(\.[0-9]+)?$";
const FHIR_INSTANT_PATTERN: &str = r"^([0-9]([0-9]([0-9][1-9]|[1-9]0)|[1-9]00)|[1-9]000)-(0[1-9]|1[0-2])-(0[1-9]|[1-2][0-9]|3[0-1])T([01][0-9]|2[0-3]):[0-5][0-9]:([0-5][0-9]|60)(\.[0-9]+)?(Z|(\+|-)((0[0-9]|1[0-3]):[0-5][0-9]|14:00))$";
const FHIR_ID_PATTERN: &str = r"^[A-Za-z0-9\-\.]{1,64}$";
const FHIR_OID_PATTERN: &str = r"^urn:oid:[0-2](\.(0|[1-9][0-9]*))+$";
const FHIR_UUID_PATTERN: &str =
    r"^urn:uuid:[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$";
const FHIR_URI_PATTERN: &str = r"^\S*$";
const FHIR_CODE_PATTERN: &str = r"^[^\s]+(\s[^\s]+)*$";
const FHIR_POSITIVE_INT_PATTERN: &str = r"^[1-9][0-9]*$";
const FHIR_UNSIGNED_INT_PATTERN: &str = r"^[0]|([1-9][0-9]*)$";

/// Convert a FHIR Schema to JSON Schema format.
///
/// The resulting JSON Schema can be used for Monaco editor autocomplete and validation.
pub fn convert_fhir_to_json_schema(fhir_schema: &FhirSchema) -> Value {
    let mut json_schema = Map::new();

    // JSON Schema metadata
    json_schema.insert(
        "$schema".to_string(),
        json!("http://json-schema.org/draft-07/schema#"),
    );
    json_schema.insert(
        "$id".to_string(),
        json!(format!(
            "http://hl7.org/fhir/{}.schema.json",
            fhir_schema.name
        )),
    );
    json_schema.insert("title".to_string(), json!(fhir_schema.name));

    if let Some(ref desc) = fhir_schema.description {
        json_schema.insert("description".to_string(), json!(desc));
    }

    json_schema.insert("type".to_string(), json!("object"));

    // Build properties from elements
    let mut properties = Map::new();
    let mut required_props = Vec::new();
    let mut defs = Map::new();

    // Add resourceType for resources
    if fhir_schema.kind == "resource" {
        properties.insert(
            "resourceType".to_string(),
            json!({
                "const": fhir_schema.name,
                "description": "Type of resource"
            }),
        );
        required_props.push("resourceType".to_string());
    }

    // Process elements
    if let Some(ref elements) = fhir_schema.elements {
        // First, collect choice type definitions from the schema
        let choices = fhir_schema.choices.clone().unwrap_or_default();

        for (name, element) in elements {
            // Skip choice placeholder elements (they are expanded)
            if name.ends_with("[x]") {
                continue;
            }

            // Check if this element is part of a choice type
            if element.choice_of.is_some() {
                // This is an expanded choice element - include it as a concrete property
                let prop_schema =
                    convert_element_to_schema(element, &mut defs, &choices, &fhir_schema.name);
                properties.insert(name.clone(), prop_schema);
            } else if element.choices.is_some() {
                // This is a choice type placeholder - expand it to concrete properties
                if let Some(ref choice_types) = element.choices {
                    for choice_type in choice_types {
                        let expanded_name = format!(
                            "{}{}",
                            name.trim_end_matches("[x]"),
                            capitalize_first(choice_type)
                        );

                        // Create element schema for this choice
                        let choice_schema =
                            create_choice_element_schema(element, choice_type, &mut defs);
                        properties.insert(expanded_name, choice_schema);
                    }
                }
            } else {
                // Regular element
                let prop_schema =
                    convert_element_to_schema(element, &mut defs, &choices, &fhir_schema.name);
                properties.insert(name.clone(), prop_schema);
            }

            // Check if required
            if element.min.unwrap_or(0) >= 1
                && element.choice_of.is_none() && element.choices.is_none() {
                    required_props.push(name.clone());
                }
        }
    }

    // Handle schema-level required list
    if let Some(ref schema_required) = fhir_schema.required {
        for req in schema_required {
            if !required_props.contains(req) {
                required_props.push(req.clone());
            }
        }
    }

    json_schema.insert("properties".to_string(), Value::Object(properties));

    if !required_props.is_empty() {
        json_schema.insert("required".to_string(), json!(required_props));
    }

    // Add $defs for complex types
    if !defs.is_empty() {
        json_schema.insert("$defs".to_string(), Value::Object(defs));
    }

    // Allow additional properties (FHIR resources can have extensions)
    json_schema.insert("additionalProperties".to_string(), json!(true));

    Value::Object(json_schema)
}

/// Convert a single FhirSchemaElement to JSON Schema property.
fn convert_element_to_schema(
    element: &FhirSchemaElement,
    defs: &mut Map<String, Value>,
    choices: &HashMap<String, Vec<String>>,
    resource_name: &str,
) -> Value {
    let is_array = element.array.unwrap_or(false);

    let base_schema = if let Some(ref type_name) = element.type_name {
        convert_type_to_schema(type_name, element, defs)
    } else if let Some(ref nested_elements) = element.elements {
        // Backbone element with nested structure
        convert_backbone_element(nested_elements, defs, choices, resource_name)
    } else {
        // Fallback
        json!({})
    };

    let mut schema = if is_array {
        let mut arr = Map::new();
        arr.insert("type".to_string(), json!("array"));
        arr.insert("items".to_string(), base_schema);
        if let Some(min) = element.min
            && min > 0 {
                arr.insert("minItems".to_string(), json!(min));
            }
        if let Some(max) = element.max {
            arr.insert("maxItems".to_string(), json!(max));
        }
        Value::Object(arr)
    } else {
        base_schema
    };

    // Add description
    if let Some(ref short) = element.short
        && let Value::Object(ref mut map) = schema {
            map.insert("description".to_string(), json!(short));
        }

    // Add reference targets to description
    if let Some(ref refers) = element.refers
        && !refers.is_empty() {
            let targets: Vec<&str> = refers
                .iter()
                .filter_map(|r| r.rsplit('/').next())
                .collect();
            if let Value::Object(ref mut map) = schema {
                let desc = map
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let new_desc = if desc.is_empty() {
                    format!("Reference to: {}", targets.join(", "))
                } else {
                    format!("{} (Reference to: {})", desc, targets.join(", "))
                };
                map.insert("description".to_string(), json!(new_desc));
            }
        }

    schema
}

/// Convert a FHIR type name to JSON Schema.
fn convert_type_to_schema(
    type_name: &str,
    element: &FhirSchemaElement,
    defs: &mut Map<String, Value>,
) -> Value {
    match type_name {
        // Primitive types
        "boolean" => json!({"type": "boolean"}),

        "integer" | "integer64" => json!({"type": "integer"}),

        "positiveInt" => json!({
            "type": "integer",
            "minimum": 1,
            "pattern": FHIR_POSITIVE_INT_PATTERN
        }),

        "unsignedInt" => json!({
            "type": "integer",
            "minimum": 0,
            "pattern": FHIR_UNSIGNED_INT_PATTERN
        }),

        "decimal" => json!({"type": "number"}),

        "string" | "markdown" | "xhtml" => json!({"type": "string"}),

        "id" => json!({
            "type": "string",
            "pattern": FHIR_ID_PATTERN
        }),

        "code" => {
            // Check for required binding with enum values
            if let Some(ref binding) = element.binding {
                if binding.strength == "required" {
                    // Could expand enum values here if ValueSet is available
                    // For now, just use pattern
                    json!({
                        "type": "string",
                        "pattern": FHIR_CODE_PATTERN
                    })
                } else {
                    json!({
                        "type": "string",
                        "pattern": FHIR_CODE_PATTERN
                    })
                }
            } else {
                json!({
                    "type": "string",
                    "pattern": FHIR_CODE_PATTERN
                })
            }
        }

        "uri" | "url" | "canonical" => json!({
            "type": "string",
            "format": "uri",
            "pattern": FHIR_URI_PATTERN
        }),

        "oid" => json!({
            "type": "string",
            "pattern": FHIR_OID_PATTERN
        }),

        "uuid" => json!({
            "type": "string",
            "pattern": FHIR_UUID_PATTERN
        }),

        "date" => json!({
            "type": "string",
            "pattern": FHIR_DATE_PATTERN
        }),

        "dateTime" => json!({
            "type": "string",
            "pattern": FHIR_DATETIME_PATTERN
        }),

        "time" => json!({
            "type": "string",
            "pattern": FHIR_TIME_PATTERN
        }),

        "instant" => json!({
            "type": "string",
            "pattern": FHIR_INSTANT_PATTERN
        }),

        "base64Binary" => json!({
            "type": "string",
            "contentEncoding": "base64"
        }),

        // Complex types - reference $defs
        complex_type => {
            // Add to $defs if not already present
            if !defs.contains_key(complex_type) {
                defs.insert(
                    complex_type.to_string(),
                    create_complex_type_placeholder(complex_type),
                );
            }
            json!({"$ref": format!("#/$defs/{}", complex_type)})
        }
    }
}

/// Create a placeholder schema for a complex type.
fn create_complex_type_placeholder(type_name: &str) -> Value {
    // Common FHIR complex types with basic structure
    match type_name {
        "Reference" => json!({
            "type": "object",
            "properties": {
                "reference": {"type": "string", "description": "Literal reference, Relative, internal or absolute URL"},
                "type": {"type": "string", "description": "Type the reference refers to (e.g. 'Patient')"},
                "identifier": {"$ref": "#/$defs/Identifier"},
                "display": {"type": "string", "description": "Text alternative for the resource"}
            }
        }),
        "Identifier" => json!({
            "type": "object",
            "properties": {
                "use": {"type": "string", "enum": ["usual", "official", "temp", "secondary", "old"]},
                "type": {"$ref": "#/$defs/CodeableConcept"},
                "system": {"type": "string", "format": "uri"},
                "value": {"type": "string"},
                "period": {"$ref": "#/$defs/Period"},
                "assigner": {"$ref": "#/$defs/Reference"}
            }
        }),
        "CodeableConcept" => json!({
            "type": "object",
            "properties": {
                "coding": {
                    "type": "array",
                    "items": {"$ref": "#/$defs/Coding"}
                },
                "text": {"type": "string"}
            }
        }),
        "Coding" => json!({
            "type": "object",
            "properties": {
                "system": {"type": "string", "format": "uri"},
                "version": {"type": "string"},
                "code": {"type": "string"},
                "display": {"type": "string"},
                "userSelected": {"type": "boolean"}
            }
        }),
        "Period" => json!({
            "type": "object",
            "properties": {
                "start": {"type": "string", "pattern": FHIR_DATETIME_PATTERN},
                "end": {"type": "string", "pattern": FHIR_DATETIME_PATTERN}
            }
        }),
        "HumanName" => json!({
            "type": "object",
            "properties": {
                "use": {"type": "string", "enum": ["usual", "official", "temp", "nickname", "anonymous", "old", "maiden"]},
                "text": {"type": "string"},
                "family": {"type": "string"},
                "given": {"type": "array", "items": {"type": "string"}},
                "prefix": {"type": "array", "items": {"type": "string"}},
                "suffix": {"type": "array", "items": {"type": "string"}},
                "period": {"$ref": "#/$defs/Period"}
            }
        }),
        "Address" => json!({
            "type": "object",
            "properties": {
                "use": {"type": "string", "enum": ["home", "work", "temp", "old", "billing"]},
                "type": {"type": "string", "enum": ["postal", "physical", "both"]},
                "text": {"type": "string"},
                "line": {"type": "array", "items": {"type": "string"}},
                "city": {"type": "string"},
                "district": {"type": "string"},
                "state": {"type": "string"},
                "postalCode": {"type": "string"},
                "country": {"type": "string"},
                "period": {"$ref": "#/$defs/Period"}
            }
        }),
        "ContactPoint" => json!({
            "type": "object",
            "properties": {
                "system": {"type": "string", "enum": ["phone", "fax", "email", "pager", "url", "sms", "other"]},
                "value": {"type": "string"},
                "use": {"type": "string", "enum": ["home", "work", "temp", "old", "mobile"]},
                "rank": {"type": "integer", "minimum": 1},
                "period": {"$ref": "#/$defs/Period"}
            }
        }),
        "Quantity" => json!({
            "type": "object",
            "properties": {
                "value": {"type": "number"},
                "comparator": {"type": "string", "enum": ["<", "<=", ">=", ">", "ad"]},
                "unit": {"type": "string"},
                "system": {"type": "string", "format": "uri"},
                "code": {"type": "string"}
            }
        }),
        "Meta" => json!({
            "type": "object",
            "properties": {
                "versionId": {"type": "string"},
                "lastUpdated": {"type": "string", "pattern": FHIR_INSTANT_PATTERN},
                "source": {"type": "string", "format": "uri"},
                "profile": {"type": "array", "items": {"type": "string", "format": "uri"}},
                "security": {"type": "array", "items": {"$ref": "#/$defs/Coding"}},
                "tag": {"type": "array", "items": {"$ref": "#/$defs/Coding"}}
            }
        }),
        "Narrative" => json!({
            "type": "object",
            "properties": {
                "status": {"type": "string", "enum": ["generated", "extensions", "additional", "empty"]},
                "div": {"type": "string", "description": "Limited xhtml content"}
            },
            "required": ["status", "div"]
        }),
        "Extension" => json!({
            "type": "object",
            "properties": {
                "url": {"type": "string", "format": "uri"},
                "valueString": {"type": "string"},
                "valueBoolean": {"type": "boolean"},
                "valueInteger": {"type": "integer"},
                "valueDecimal": {"type": "number"},
                "valueCode": {"type": "string"},
                "valueUri": {"type": "string", "format": "uri"},
                "valueDateTime": {"type": "string"},
                "valueCoding": {"$ref": "#/$defs/Coding"},
                "valueCodeableConcept": {"$ref": "#/$defs/CodeableConcept"},
                "valueReference": {"$ref": "#/$defs/Reference"}
            },
            "required": ["url"]
        }),
        "Attachment" => json!({
            "type": "object",
            "properties": {
                "contentType": {"type": "string"},
                "language": {"type": "string"},
                "data": {"type": "string", "contentEncoding": "base64"},
                "url": {"type": "string", "format": "uri"},
                "size": {"type": "integer"},
                "hash": {"type": "string", "contentEncoding": "base64"},
                "title": {"type": "string"},
                "creation": {"type": "string", "pattern": FHIR_DATETIME_PATTERN}
            }
        }),
        "Annotation" => json!({
            "type": "object",
            "properties": {
                "authorReference": {"$ref": "#/$defs/Reference"},
                "authorString": {"type": "string"},
                "time": {"type": "string", "pattern": FHIR_DATETIME_PATTERN},
                "text": {"type": "string"}
            },
            "required": ["text"]
        }),
        "Range" => json!({
            "type": "object",
            "properties": {
                "low": {"$ref": "#/$defs/Quantity"},
                "high": {"$ref": "#/$defs/Quantity"}
            }
        }),
        "Ratio" => json!({
            "type": "object",
            "properties": {
                "numerator": {"$ref": "#/$defs/Quantity"},
                "denominator": {"$ref": "#/$defs/Quantity"}
            }
        }),
        "Age" | "Distance" | "Duration" | "Count" | "SimpleQuantity" | "Money" => json!({
            "type": "object",
            "properties": {
                "value": {"type": "number"},
                "comparator": {"type": "string", "enum": ["<", "<=", ">=", ">", "ad"]},
                "unit": {"type": "string"},
                "system": {"type": "string", "format": "uri"},
                "code": {"type": "string"}
            }
        }),
        "Timing" => json!({
            "type": "object",
            "properties": {
                "event": {"type": "array", "items": {"type": "string", "pattern": FHIR_DATETIME_PATTERN}},
                "repeat": {"type": "object"},
                "code": {"$ref": "#/$defs/CodeableConcept"}
            }
        }),
        "Dosage" => json!({
            "type": "object",
            "properties": {
                "sequence": {"type": "integer"},
                "text": {"type": "string"},
                "additionalInstruction": {"type": "array", "items": {"$ref": "#/$defs/CodeableConcept"}},
                "patientInstruction": {"type": "string"},
                "timing": {"$ref": "#/$defs/Timing"},
                "asNeededBoolean": {"type": "boolean"},
                "asNeededCodeableConcept": {"$ref": "#/$defs/CodeableConcept"},
                "site": {"$ref": "#/$defs/CodeableConcept"},
                "route": {"$ref": "#/$defs/CodeableConcept"},
                "method": {"$ref": "#/$defs/CodeableConcept"}
            }
        }),
        // Default fallback for unknown types
        _ => json!({
            "type": "object",
            "additionalProperties": true
        }),
    }
}

/// Convert backbone element with nested structure.
fn convert_backbone_element(
    nested_elements: &HashMap<String, FhirSchemaElement>,
    defs: &mut Map<String, Value>,
    choices: &HashMap<String, Vec<String>>,
    resource_name: &str,
) -> Value {
    let mut properties = Map::new();
    let mut required_props = Vec::new();

    for (name, element) in nested_elements {
        // Skip choice placeholders
        if name.ends_with("[x]") {
            continue;
        }

        if element.choice_of.is_some() {
            // Expanded choice element
            let prop_schema = convert_element_to_schema(element, defs, choices, resource_name);
            properties.insert(name.clone(), prop_schema);
        } else if element.choices.is_some() {
            // Choice type - expand
            if let Some(ref choice_types) = element.choices {
                for choice_type in choice_types {
                    let expanded_name = format!(
                        "{}{}",
                        name.trim_end_matches("[x]"),
                        capitalize_first(choice_type)
                    );
                    let choice_schema = create_choice_element_schema(element, choice_type, defs);
                    properties.insert(expanded_name, choice_schema);
                }
            }
        } else {
            let prop_schema = convert_element_to_schema(element, defs, choices, resource_name);
            properties.insert(name.clone(), prop_schema);
        }

        if element.min.unwrap_or(0) >= 1
            && element.choice_of.is_none()
            && element.choices.is_none()
        {
            required_props.push(name.clone());
        }
    }

    let mut backbone = Map::new();
    backbone.insert("type".to_string(), json!("object"));
    backbone.insert("properties".to_string(), Value::Object(properties));
    if !required_props.is_empty() {
        backbone.insert("required".to_string(), json!(required_props));
    }

    Value::Object(backbone)
}

/// Create schema for a specific choice type expansion.
fn create_choice_element_schema(
    element: &FhirSchemaElement,
    choice_type: &str,
    defs: &mut Map<String, Value>,
) -> Value {
    let is_array = element.array.unwrap_or(false);

    let base_schema = convert_type_to_schema(
        choice_type,
        &FhirSchemaElement::default(), // No bindings for choice expansions
        defs,
    );

    if is_array {
        let mut arr = Map::new();
        arr.insert("type".to_string(), json!("array"));
        arr.insert("items".to_string(), base_schema);
        Value::Object(arr)
    } else {
        base_schema
    }
}

/// Capitalize the first character of a string (for choice type expansion).
fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capitalize_first() {
        assert_eq!(capitalize_first("string"), "String");
        assert_eq!(capitalize_first("boolean"), "Boolean");
        assert_eq!(capitalize_first("dateTime"), "DateTime");
        assert_eq!(capitalize_first(""), "");
    }

    #[test]
    fn test_convert_primitive_types() {
        let mut defs = Map::new();
        let element = FhirSchemaElement::default();

        let bool_schema = convert_type_to_schema("boolean", &element, &mut defs);
        assert_eq!(bool_schema, json!({"type": "boolean"}));

        let int_schema = convert_type_to_schema("integer", &element, &mut defs);
        assert_eq!(int_schema, json!({"type": "integer"}));

        let string_schema = convert_type_to_schema("string", &element, &mut defs);
        assert_eq!(string_schema, json!({"type": "string"}));
    }

    #[test]
    fn test_convert_date_types() {
        let mut defs = Map::new();
        let element = FhirSchemaElement::default();

        let date_schema = convert_type_to_schema("date", &element, &mut defs);
        assert!(date_schema.get("pattern").is_some());

        let datetime_schema = convert_type_to_schema("dateTime", &element, &mut defs);
        assert!(datetime_schema.get("pattern").is_some());
    }

    #[test]
    fn test_convert_complex_type_reference() {
        let mut defs = Map::new();
        let element = FhirSchemaElement::default();

        let ref_schema = convert_type_to_schema("Reference", &element, &mut defs);
        assert!(ref_schema.get("$ref").is_some());
        assert!(defs.contains_key("Reference"));
    }
}
