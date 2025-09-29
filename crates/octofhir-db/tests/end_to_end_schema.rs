// End-to-end test: StructureDefinition JSON -> Schema -> DDL
// This demonstrates the complete workflow for Task 0001

use octofhir_db::schema::{parse_structure_definition, SchemaGenerator};

#[test]
fn test_patient_structure_definition_to_ddl() {
    // Simplified Patient StructureDefinition (R5-like)
    let patient_sd = r#"{
        "resourceType": "StructureDefinition",
        "id": "Patient",
        "url": "http://hl7.org/fhir/StructureDefinition/Patient",
        "version": "5.0.0",
        "name": "Patient",
        "type": "Patient",
        "snapshot": {
            "element": [
                {
                    "path": "Patient",
                    "min": 0,
                    "max": "*",
                    "short": "Information about an individual or animal receiving care",
                    "definition": "Demographics and other administrative information about an individual or animal receiving care or other health-related services."
                },
                {
                    "path": "Patient.id",
                    "min": 0,
                    "max": "1",
                    "short": "Logical id of this artifact",
                    "type": [{"code": "id"}]
                },
                {
                    "path": "Patient.meta",
                    "min": 0,
                    "max": "1",
                    "short": "Metadata about the resource",
                    "type": [{"code": "Meta"}]
                },
                {
                    "path": "Patient.identifier",
                    "min": 0,
                    "max": "*",
                    "short": "An identifier for this patient",
                    "type": [{"code": "Identifier"}]
                },
                {
                    "path": "Patient.active",
                    "min": 0,
                    "max": "1",
                    "short": "Whether this patient's record is in active use",
                    "definition": "Whether this patient record is in active use.",
                    "type": [{"code": "boolean"}],
                    "mustSupport": true
                },
                {
                    "path": "Patient.name",
                    "min": 0,
                    "max": "*",
                    "short": "A name associated with the patient",
                    "type": [{"code": "HumanName"}]
                },
                {
                    "path": "Patient.telecom",
                    "min": 0,
                    "max": "*",
                    "short": "A contact detail for the individual",
                    "type": [{"code": "ContactPoint"}]
                },
                {
                    "path": "Patient.gender",
                    "min": 0,
                    "max": "1",
                    "short": "male | female | other | unknown",
                    "type": [{"code": "code"}]
                },
                {
                    "path": "Patient.birthDate",
                    "min": 0,
                    "max": "1",
                    "short": "The date of birth for the individual",
                    "type": [{"code": "date"}],
                    "mustSupport": true
                },
                {
                    "path": "Patient.deceased[x]",
                    "min": 0,
                    "max": "1",
                    "short": "Indicates if the individual is deceased or not",
                    "type": [
                        {"code": "boolean"},
                        {"code": "dateTime"}
                    ]
                },
                {
                    "path": "Patient.address",
                    "min": 0,
                    "max": "*",
                    "short": "An address for the individual",
                    "type": [{"code": "Address"}]
                },
                {
                    "path": "Patient.contact",
                    "min": 0,
                    "max": "*",
                    "short": "A contact party (e.g. guardian, partner, friend) for the patient",
                    "type": [{"code": "BackboneElement"}]
                }
            ]
        }
    }"#;

    // Step 1: Parse StructureDefinition
    println!("\n=== Step 1: Parsing StructureDefinition ===");
    let elements = parse_structure_definition(patient_sd)
        .expect("Failed to parse Patient StructureDefinition");

    println!("Parsed {} elements from Patient StructureDefinition", elements.len());
    assert!(elements.len() >= 10, "Should have at least 10 elements");

    // Verify key elements were parsed
    assert!(elements.iter().any(|e| e.path == "Patient.id"));
    assert!(elements.iter().any(|e| e.path == "Patient.active"));
    assert!(elements.iter().any(|e| e.path == "Patient.birthDate"));

    // Step 2: Generate schema from parsed elements
    println!("\n=== Step 2: Generating Schema ===");
    let generator = SchemaGenerator::new();
    let schema = generator
        .generate_resource_schema("Patient", elements)
        .expect("Failed to generate schema");

    println!("Generated schema for {}", schema.resource_type);
    println!("  Base table: {}", schema.base_table.name);
    println!("  Columns: {}", schema.base_table.columns.len());
    println!("  Indexes: {}", schema.base_table.indexes.len());
    println!("  History table: {}", schema.history_table.name);

    // Step 3: Verify DDL was generated
    println!("\n=== Step 3: Generated DDL ===");
    let complete_ddl = schema.complete_ddl();

    // Verify DDL contains expected elements
    assert!(complete_ddl.contains("CREATE TABLE IF NOT EXISTS public.patient"));
    assert!(complete_ddl.contains("id UUID PRIMARY KEY"));
    assert!(complete_ddl.contains("resource JSONB NOT NULL"));
    assert!(complete_ddl.contains("created_at TIMESTAMPTZ NOT NULL"));
    assert!(complete_ddl.contains("updated_at TIMESTAMPTZ NOT NULL"));
    assert!(complete_ddl.contains("patient_history"));
    assert!(complete_ddl.contains("snapshot JSONB"));
    assert!(complete_ddl.contains("json_patch JSONB"));
    assert!(complete_ddl.contains("merge_patch JSONB"));

    // Print the generated DDL for manual review
    println!("\n{}", complete_ddl);

    println!("\n=== Test PASSED: Successfully generated DDL from StructureDefinition ===");
}

#[test]
fn test_observation_structure_definition_to_ddl() {
    let observation_sd = r#"{
        "resourceType": "StructureDefinition",
        "id": "Observation",
        "url": "http://hl7.org/fhir/StructureDefinition/Observation",
        "version": "5.0.0",
        "name": "Observation",
        "type": "Observation",
        "snapshot": {
            "element": [
                {
                    "path": "Observation",
                    "min": 0,
                    "max": "*"
                },
                {
                    "path": "Observation.id",
                    "min": 0,
                    "max": "1",
                    "type": [{"code": "id"}]
                },
                {
                    "path": "Observation.status",
                    "min": 1,
                    "max": "1",
                    "short": "registered | preliminary | final | amended +",
                    "type": [{"code": "code"}],
                    "isModifier": true,
                    "mustSupport": true
                },
                {
                    "path": "Observation.category",
                    "min": 0,
                    "max": "*",
                    "short": "Classification of type of observation",
                    "type": [{"code": "CodeableConcept"}]
                },
                {
                    "path": "Observation.code",
                    "min": 1,
                    "max": "1",
                    "short": "Type of observation (code / type)",
                    "type": [{"code": "CodeableConcept"}],
                    "mustSupport": true
                },
                {
                    "path": "Observation.subject",
                    "min": 0,
                    "max": "1",
                    "short": "Who and/or what the observation is about",
                    "type": [{"code": "Reference"}]
                },
                {
                    "path": "Observation.effective[x]",
                    "min": 0,
                    "max": "1",
                    "short": "Clinically relevant time/time-period for observation",
                    "type": [
                        {"code": "dateTime"},
                        {"code": "Period"},
                        {"code": "instant"}
                    ]
                },
                {
                    "path": "Observation.value[x]",
                    "min": 0,
                    "max": "1",
                    "short": "Actual result",
                    "type": [
                        {"code": "Quantity"},
                        {"code": "CodeableConcept"},
                        {"code": "string"},
                        {"code": "boolean"},
                        {"code": "integer"},
                        {"code": "Range"},
                        {"code": "Ratio"}
                    ]
                }
            ]
        }
    }"#;

    // Parse and generate schema
    let elements = parse_structure_definition(observation_sd)
        .expect("Failed to parse Observation StructureDefinition");

    assert!(elements.len() >= 7);

    // Verify required elements
    let status_elem = elements.iter().find(|e| e.path == "Observation.status").unwrap();
    assert_eq!(status_elem.min, 1); // Required field
    assert!(status_elem.is_modifier);
    assert!(status_elem.must_support);

    // Verify polymorphic elements
    let value_elem = elements.iter().find(|e| e.path == "Observation.value[x]").unwrap();
    assert!(value_elem.types.len() >= 5); // Multiple types

    let generator = SchemaGenerator::new();
    let schema = generator
        .generate_resource_schema("Observation", elements)
        .expect("Failed to generate schema");

    assert_eq!(schema.base_table.name, "observation");
    assert_eq!(schema.history_table.name, "observation_history");

    let ddl = schema.complete_ddl();
    assert!(ddl.contains("observation"));
    assert!(ddl.contains("observation_history"));

    println!("\n=== Observation Schema DDL ===");
    println!("{}", ddl);
}

#[test]
fn test_differential_parsing() {
    // Test with differential instead of snapshot
    let profile_sd = r#"{
        "resourceType": "StructureDefinition",
        "id": "CustomPatient",
        "type": "Patient",
        "derivation": "constraint",
        "differential": {
            "element": [
                {
                    "path": "Patient.identifier",
                    "min": 1,
                    "max": "*",
                    "mustSupport": true
                },
                {
                    "path": "Patient.name",
                    "min": 1,
                    "max": "*",
                    "mustSupport": true
                }
            ]
        }
    }"#;

    let elements = parse_structure_definition(profile_sd)
        .expect("Failed to parse differential");

    assert_eq!(elements.len(), 2);
    assert!(elements.iter().all(|e| e.must_support));

    println!("Successfully parsed differential with {} elements", elements.len());
}

#[test]
fn test_complete_workflow_with_metadata() {
    // This test demonstrates the complete workflow including metadata tracking
    let sd_json = r#"{
        "resourceType": "StructureDefinition",
        "id": "Patient",
        "url": "http://hl7.org/fhir/StructureDefinition/Patient",
        "version": "5.0.0",
        "name": "Patient",
        "type": "Patient",
        "snapshot": {
            "element": [
                {"path": "Patient", "min": 0, "max": "*"},
                {"path": "Patient.id", "min": 0, "max": "1", "type": [{"code": "id"}]},
                {"path": "Patient.birthDate", "min": 0, "max": "1", "type": [{"code": "date"}]}
            ]
        }
    }"#;

    // Parse
    let elements = parse_structure_definition(sd_json).unwrap();

    // Generate
    let generator = SchemaGenerator::new();
    let schema = generator.generate_resource_schema("Patient", elements).unwrap();

    // Verify metadata is captured
    assert_eq!(schema.metadata.resource_type, "Patient");
    assert_eq!(schema.metadata.fhir_version, "R5");
    assert!(!schema.metadata.generated_at.is_empty());

    println!("Metadata: {:?}", schema.metadata);
    println!("\n✅ Complete workflow test passed");
}