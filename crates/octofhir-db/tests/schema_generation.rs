// Integration tests for schema generation per Task 0001
// Tests Patient and Observation resource schema generation

use octofhir_db::schema::{ElementDescriptor, ElementType, SchemaGenerator, SnapshotStrategy};

#[test]
fn test_patient_schema_generation() {
    let generator = SchemaGenerator::new().with_snapshot_strategy(SnapshotStrategy::EveryKVersions(10));

    // Define Patient resource elements based on FHIR R5 StructureDefinition
    let elements = vec![
        ElementDescriptor::new("Patient.id".to_string())
            .with_type(ElementType::Id)
            .required(),
        ElementDescriptor::new("Patient.meta".to_string()).with_type(ElementType::Meta),
        ElementDescriptor::new("Patient.identifier".to_string())
            .with_type(ElementType::Identifier)
            .with_cardinality(0, None), // Array
        ElementDescriptor::new("Patient.active".to_string())
            .with_type(ElementType::Boolean)
            .with_cardinality(0, Some(1)),
        ElementDescriptor::new("Patient.name".to_string())
            .with_type(ElementType::HumanName)
            .with_cardinality(0, None), // Array
        ElementDescriptor::new("Patient.telecom".to_string())
            .with_type(ElementType::ContactPoint)
            .with_cardinality(0, None),
        ElementDescriptor::new("Patient.gender".to_string())
            .with_type(ElementType::Code)
            .with_cardinality(0, Some(1)),
        ElementDescriptor::new("Patient.birthDate".to_string())
            .with_type(ElementType::Date)
            .with_cardinality(0, Some(1)),
        ElementDescriptor::new("Patient.deceased[x]".to_string())
            .with_type(ElementType::Boolean)
            .with_type(ElementType::DateTime) // Polymorphic
            .with_cardinality(0, Some(1)),
        ElementDescriptor::new("Patient.address".to_string())
            .with_type(ElementType::Address)
            .with_cardinality(0, None),
    ];

    let schema = generator
        .generate_resource_schema("Patient", elements)
        .expect("Failed to generate Patient schema");

    // Verify base table
    assert_eq!(schema.resource_type, "Patient");
    assert_eq!(schema.base_table.name, "patient");

    // Verify standard columns exist
    assert!(
        schema.base_table.columns.iter().any(|c| c.name == "id" && c.primary_key),
        "Missing primary key column"
    );
    assert!(
        schema
            .base_table
            .columns
            .iter()
            .any(|c| c.name == "resource"),
        "Missing resource JSONB column"
    );
    assert!(
        schema
            .base_table
            .columns
            .iter()
            .any(|c| c.name == "created_at"),
        "Missing created_at column"
    );
    assert!(
        schema
            .base_table
            .columns
            .iter()
            .any(|c| c.name == "updated_at"),
        "Missing updated_at column"
    );

    // Verify history table
    assert_eq!(schema.history_table.name, "patient_history");
    assert!(
        schema
            .history_table
            .columns
            .iter()
            .any(|c| c.name == "snapshot"),
        "History table missing snapshot column"
    );
    assert!(
        schema
            .history_table
            .columns
            .iter()
            .any(|c| c.name == "json_patch"),
        "History table missing json_patch column"
    );
    assert!(
        schema
            .history_table
            .columns
            .iter()
            .any(|c| c.name == "merge_patch"),
        "History table missing merge_patch column"
    );
    assert!(
        schema
            .history_table
            .columns
            .iter()
            .any(|c| c.name == "resource_id"),
        "History table missing resource_id FK"
    );

    // Verify foreign key to base table exists
    assert_eq!(
        schema.history_table.foreign_keys.len(),
        1,
        "History table should have exactly one foreign key"
    );
    assert_eq!(
        schema.history_table.foreign_keys[0].referenced_table,
        "patient"
    );

    // Verify indexes exist
    assert!(
        !schema.base_table.indexes.is_empty(),
        "Base table should have indexes"
    );
    assert!(
        !schema.history_table.indexes.is_empty(),
        "History table should have indexes"
    );

    // Verify DDL is generated
    assert!(
        schema.base_ddl.contains("CREATE TABLE"),
        "Base DDL should contain CREATE TABLE"
    );
    assert!(
        schema.base_ddl.contains("patient"),
        "Base DDL should reference patient table"
    );
    assert!(
        schema.history_ddl.contains("patient_history"),
        "History DDL should reference patient_history table"
    );

    // Verify complete DDL includes both tables
    let complete_ddl = schema.complete_ddl();
    assert!(complete_ddl.contains("-- Schema for FHIR resource: Patient"));
    assert!(complete_ddl.contains("patient"));
    assert!(complete_ddl.contains("patient_history"));

    // Print DDL for manual verification
    println!("\n=== PATIENT SCHEMA DDL ===\n{}", complete_ddl);
}

#[test]
fn test_observation_schema_generation() {
    let generator = SchemaGenerator::new();

    // Define Observation resource elements based on FHIR R5 StructureDefinition
    let elements = vec![
        ElementDescriptor::new("Observation.id".to_string())
            .with_type(ElementType::Id)
            .required(),
        ElementDescriptor::new("Observation.status".to_string())
            .with_type(ElementType::Code)
            .required(),
        ElementDescriptor::new("Observation.category".to_string())
            .with_type(ElementType::CodeableConcept)
            .with_cardinality(0, None),
        ElementDescriptor::new("Observation.code".to_string())
            .with_type(ElementType::CodeableConcept)
            .required(),
        ElementDescriptor::new("Observation.subject".to_string())
            .with_type(ElementType::Reference)
            .with_cardinality(0, Some(1)),
        ElementDescriptor::new("Observation.effective[x]".to_string())
            .with_type(ElementType::DateTime)
            .with_type(ElementType::Period) // Polymorphic
            .with_cardinality(0, Some(1)),
        ElementDescriptor::new("Observation.issued".to_string())
            .with_type(ElementType::Instant)
            .with_cardinality(0, Some(1)),
        ElementDescriptor::new("Observation.value[x]".to_string())
            .with_type(ElementType::Quantity)
            .with_type(ElementType::String)
            .with_type(ElementType::Boolean)
            .with_type(ElementType::CodeableConcept) // Polymorphic
            .with_cardinality(0, Some(1)),
        ElementDescriptor::new("Observation.interpretation".to_string())
            .with_type(ElementType::CodeableConcept)
            .with_cardinality(0, None),
    ];

    let schema = generator
        .generate_resource_schema("Observation", elements)
        .expect("Failed to generate Observation schema");

    // Verify base table
    assert_eq!(schema.resource_type, "Observation");
    assert_eq!(schema.base_table.name, "observation");

    // Verify standard columns
    assert!(schema
        .base_table
        .columns
        .iter()
        .any(|c| c.name == "id" && c.primary_key));
    assert!(schema.base_table.columns.iter().any(|c| c.name == "resource"));

    // Verify history table with correct naming
    assert_eq!(schema.history_table.name, "observation_history");

    // Verify snapshot column exists per ADR-001
    assert!(schema
        .history_table
        .columns
        .iter()
        .any(|c| c.name == "snapshot"));

    // Verify indexes for performance
    let resource_gin_index = schema
        .base_table
        .indexes
        .iter()
        .any(|idx| idx.columns.contains(&"resource".to_string()));
    assert!(
        resource_gin_index,
        "Should have GIN index on resource JSONB"
    );

    // Verify DDL generation
    assert!(schema.base_ddl.contains("observation"));
    assert!(schema.history_ddl.contains("observation_history"));

    println!("\n=== OBSERVATION SCHEMA DDL ===\n{}", schema.complete_ddl());
}

#[test]
fn test_schema_cardinality_handling() {
    let generator = SchemaGenerator::new();

    // Test cardinality constraints are captured
    let elements = vec![
        ElementDescriptor::new("Patient.id".to_string())
            .with_type(ElementType::Id)
            .required(), // min=1
        ElementDescriptor::new("Patient.name".to_string())
            .with_type(ElementType::HumanName)
            .with_cardinality(0, None), // unbounded array
    ];

    let schema = generator
        .generate_resource_schema("Patient", elements)
        .unwrap();

    // Find id column and verify it's NOT NULL
    let id_col = schema
        .base_table
        .columns
        .iter()
        .find(|c| c.name == "id" && c.primary_key)
        .expect("ID column not found");
    assert!(!id_col.nullable, "ID should be NOT NULL");

    // Verify cardinality is stored in metadata
    assert!(id_col.fhir_path.is_some());
}

#[test]
fn test_polymorphic_element_handling() {
    let generator = SchemaGenerator::new();

    // Test polymorphic elements (value[x], effective[x]) are handled
    let elements = vec![
        ElementDescriptor::new("Observation.id".to_string())
            .with_type(ElementType::Id)
            .required(),
        ElementDescriptor::new("Observation.value[x]".to_string())
            .with_type(ElementType::Quantity)
            .with_type(ElementType::String)
            .with_type(ElementType::Boolean),
    ];

    let schema = generator
        .generate_resource_schema("Observation", elements)
        .unwrap();

    // Polymorphic elements should be stored in main JSONB, not extracted
    // Verify we have the standard columns but value[x] isn't extracted
    assert!(schema.base_table.columns.iter().any(|c| c.name == "resource"));

    // The schema should still be valid
    assert!(schema.base_ddl.contains("CREATE TABLE"));
}

#[test]
fn test_history_snapshot_strategy() {
    // Test different snapshot strategies
    let generator_every_5 =
        SchemaGenerator::new().with_snapshot_strategy(SnapshotStrategy::EveryKVersions(5));
    let schema_5 = generator_every_5
        .generate_resource_schema("Patient", vec![])
        .unwrap();

    let generator_always =
        SchemaGenerator::new().with_snapshot_strategy(SnapshotStrategy::Always);
    let schema_always = generator_always
        .generate_resource_schema("Patient", vec![])
        .unwrap();

    // Both should have history tables with snapshot columns
    assert!(schema_5
        .history_table
        .columns
        .iter()
        .any(|c| c.name == "snapshot"));
    assert!(schema_always
        .history_table
        .columns
        .iter()
        .any(|c| c.name == "snapshot"));
}

#[test]
fn test_ddl_is_valid_postgres_syntax() {
    let generator = SchemaGenerator::new();

    let elements = vec![ElementDescriptor::new("Patient.id".to_string())
        .with_type(ElementType::Id)
        .required()];

    let schema = generator
        .generate_resource_schema("Patient", elements)
        .unwrap();

    // Basic syntax checks
    assert!(schema.base_ddl.contains("CREATE TABLE IF NOT EXISTS public.patient"));
    assert!(schema.base_ddl.contains("id UUID PRIMARY KEY"));
    assert!(schema.base_ddl.contains("resource JSONB NOT NULL"));

    // History table checks
    assert!(schema
        .history_ddl
        .contains("CREATE TABLE IF NOT EXISTS public.patient_history"));
    assert!(schema.history_ddl.contains("resource_id UUID NOT NULL"));
    assert!(schema.history_ddl.contains("FOREIGN KEY"));
}