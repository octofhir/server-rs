//! Integration tests for SQL on FHIR implementation.
//!
//! These tests verify the full flow from ViewDefinition parsing to SQL generation.
//! Based on examples from the SQL on FHIR v2.1.0 specification.

use octofhir_sof::{SqlGenerator, ViewDefinition};
use serde_json::json;

/// Helper to create a ViewDefinition and generate SQL.
fn generate_sql(view_json: serde_json::Value) -> String {
    let view = ViewDefinition::from_json(&view_json).expect("Failed to parse ViewDefinition");
    let generator = SqlGenerator::new();
    let result = generator.generate(&view).expect("Failed to generate SQL");
    result.sql
}

/// Helper to create a ViewDefinition and check it parses correctly.
fn parse_view(view_json: serde_json::Value) -> ViewDefinition {
    ViewDefinition::from_json(&view_json).expect("Failed to parse ViewDefinition")
}

// =============================================================================
// Basic ViewDefinition Tests
// =============================================================================

#[test]
fn test_patient_demographics_view() {
    // Example from SQL on FHIR IG: Patient demographics view
    let view = json!({
        "resourceType": "ViewDefinition",
        "url": "http://example.org/views/patient-demographics",
        "name": "patient_demographics",
        "status": "active",
        "resource": "Patient",
        "description": "Basic patient demographics view",
        "select": [{
            "column": [
                {"name": "id", "path": "id"},
                {"name": "gender", "path": "gender"},
                {"name": "birth_date", "path": "birthDate", "type": "date"},
                {"name": "active", "path": "active", "type": "boolean"}
            ]
        }]
    });

    let sql = generate_sql(view);

    assert!(sql.contains("SELECT"), "SQL should have SELECT");
    assert!(
        sql.contains("FROM patient"),
        "SQL should query patient table"
    );
    assert!(sql.contains("base.id"), "SQL should select id");
    assert!(sql.contains("gender"), "SQL should select gender");
    assert!(sql.contains("birthDate"), "SQL should select birthDate");
    assert!(sql.contains("::date"), "SQL should cast birthDate to date");
    assert!(
        sql.contains("::boolean"),
        "SQL should cast active to boolean"
    );
}

#[test]
fn test_patient_name_expansion() {
    // Test forEach to expand name array
    let view = json!({
        "resourceType": "ViewDefinition",
        "name": "patient_names",
        "status": "active",
        "resource": "Patient",
        "select": [{
            "forEach": "name",
            "column": [
                {"name": "use", "path": "use"},
                {"name": "family", "path": "family"},
                {"name": "given", "path": "given.join(' ')"}
            ]
        }]
    });

    let sql = generate_sql(view);

    assert!(
        sql.contains("CROSS JOIN LATERAL"),
        "SQL should use LATERAL join for forEach"
    );
    assert!(
        sql.contains("jsonb_array_elements"),
        "SQL should expand array"
    );
    assert!(
        sql.contains("string_agg"),
        "SQL should use string_agg for join()"
    );
}

#[test]
fn test_patient_name_with_filter() {
    // Test where() function to filter names
    let view = json!({
        "resourceType": "ViewDefinition",
        "name": "patient_official_name",
        "status": "active",
        "resource": "Patient",
        "select": [{
            "column": [
                {"name": "id", "path": "id"},
                {"name": "official_family", "path": "name.where(use = 'official').first().family"}
            ]
        }]
    });

    let sql = generate_sql(view);

    assert!(
        sql.contains("jsonb_agg"),
        "SQL should use jsonb_agg for where()"
    );
    assert!(sql.contains("->0"), "SQL should use ->0 for first()");
    assert!(sql.contains("official"), "SQL should filter by 'official'");
}

#[test]
fn test_for_each_or_null() {
    // Test forEachOrNull for optional array expansion
    let view = json!({
        "resourceType": "ViewDefinition",
        "name": "patient_identifiers",
        "status": "active",
        "resource": "Patient",
        "select": [{
            "forEachOrNull": "identifier",
            "column": [
                {"name": "system", "path": "system"},
                {"name": "value", "path": "value"}
            ]
        }]
    });

    let sql = generate_sql(view);

    assert!(
        sql.contains("LEFT JOIN LATERAL"),
        "SQL should use LEFT JOIN for forEachOrNull"
    );
    assert!(
        sql.contains("ON true"),
        "SQL should have ON true for LEFT JOIN LATERAL"
    );
}

// =============================================================================
// Observation ViewDefinition Tests
// =============================================================================

#[test]
fn test_observation_view() {
    // Typical Observation view with subject reference
    let view = json!({
        "resourceType": "ViewDefinition",
        "name": "observations",
        "status": "active",
        "resource": "Observation",
        "select": [{
            "column": [
                {"name": "id", "path": "id"},
                {"name": "status", "path": "status"},
                {"name": "code", "path": "code.coding.first().code"},
                {"name": "code_display", "path": "code.coding.first().display"},
                {"name": "patient_id", "path": "subject.getReferenceKey(Patient)"},
                {"name": "effective_date", "path": "effectiveDateTime", "type": "dateTime"}
            ]
        }]
    });

    let sql = generate_sql(view);

    assert!(
        sql.contains("FROM observation"),
        "SQL should query observation table"
    );
    assert!(
        sql.contains("fhir_ref_id"),
        "SQL should use fhir_ref_id for reference"
    );
    assert!(
        sql.contains("fhir_ref_type"),
        "SQL should check reference type"
    );
    assert!(
        sql.contains("Patient"),
        "SQL should filter for Patient references"
    );
}

#[test]
fn test_observation_with_value_types() {
    // Test ofType() for polymorphic value[x]
    let view = json!({
        "resourceType": "ViewDefinition",
        "name": "observation_values",
        "status": "active",
        "resource": "Observation",
        "select": [{
            "column": [
                {"name": "id", "path": "id"},
                {"name": "value_quantity", "path": "value.ofType(Quantity).value", "type": "decimal"},
                {"name": "value_unit", "path": "value.ofType(Quantity).unit"},
                {"name": "value_string", "path": "value.ofType(String)"}
            ]
        }]
    });

    let sql = generate_sql(view);

    assert!(
        sql.contains("valueQuantity"),
        "SQL should access valueQuantity"
    );
    assert!(sql.contains("valueString"), "SQL should access valueString");
    assert!(
        sql.contains("::numeric"),
        "SQL should cast to numeric for decimal"
    );
}

// =============================================================================
// Extension Handling Tests
// =============================================================================

#[test]
fn test_extension_access() {
    // Test extension() function for US Core race extension
    let view = json!({
        "resourceType": "ViewDefinition",
        "name": "patient_race",
        "status": "active",
        "resource": "Patient",
        "select": [{
            "column": [
                {"name": "id", "path": "id"},
                {"name": "race_code", "path": "extension('http://hl7.org/fhir/us/core/StructureDefinition/us-core-race').extension('ombCategory').valueCoding.code"},
                {"name": "race_display", "path": "extension('http://hl7.org/fhir/us/core/StructureDefinition/us-core-race').extension('ombCategory').valueCoding.display"}
            ]
        }]
    });

    let sql = generate_sql(view);

    assert!(sql.contains("extension"), "SQL should access extension");
    assert!(
        sql.contains("http://hl7.org/fhir/us/core/StructureDefinition/us-core-race"),
        "SQL should filter by extension URL"
    );
}

// =============================================================================
// Constants and Substitution Tests
// =============================================================================

#[test]
fn test_constant_substitution() {
    // Test constant substitution in paths
    let view = json!({
        "resourceType": "ViewDefinition",
        "name": "loinc_observations",
        "status": "active",
        "resource": "Observation",
        "constant": [{
            "name": "loincSystem",
            "valueString": "http://loinc.org"
        }],
        "select": [{
            "column": [
                {"name": "id", "path": "id"},
                {"name": "code", "path": "code.coding.where(system = %loincSystem).first().code"}
            ]
        }]
    });

    let sql = generate_sql(view);

    assert!(
        sql.contains("http://loinc.org"),
        "SQL should have substituted constant"
    );
    assert!(
        !sql.contains("%loincSystem"),
        "SQL should not contain %constant reference"
    );
}

#[test]
fn test_integer_constant() {
    let view = json!({
        "resourceType": "ViewDefinition",
        "name": "test_int_constant",
        "status": "active",
        "resource": "Patient",
        "constant": [{
            "name": "maxCount",
            "valueInteger": 100
        }],
        "select": [{
            "column": [
                {"name": "id", "path": "id"}
            ]
        }]
    });

    let parsed = parse_view(view);
    assert_eq!(parsed.constant[0].value_integer, Some(100));
}

// =============================================================================
// Where Clause Tests
// =============================================================================

#[test]
fn test_where_clause() {
    // Test where clause filtering
    let view = json!({
        "resourceType": "ViewDefinition",
        "name": "active_patients",
        "status": "active",
        "resource": "Patient",
        "select": [{
            "column": [
                {"name": "id", "path": "id"},
                {"name": "name", "path": "name.first().family"}
            ]
        }],
        "where": [{
            "path": "active",
            "description": "Only include active patients"
        }]
    });

    let sql = generate_sql(view);

    assert!(sql.contains("WHERE"), "SQL should have WHERE clause");
    assert!(sql.contains("active"), "SQL should filter by active");
}

// =============================================================================
// FHIRPath Function Tests
// =============================================================================

#[test]
fn test_first_and_last_functions() {
    let view = json!({
        "resourceType": "ViewDefinition",
        "name": "test_first_last",
        "status": "active",
        "resource": "Patient",
        "select": [{
            "column": [
                {"name": "first_name", "path": "name.first().family"},
                {"name": "last_name", "path": "name.last().family"}
            ]
        }]
    });

    let sql = generate_sql(view);

    assert!(sql.contains("->0"), "SQL should use ->0 for first()");
    assert!(sql.contains("->-1"), "SQL should use ->-1 for last()");
}

#[test]
fn test_exists_function() {
    let view = json!({
        "resourceType": "ViewDefinition",
        "name": "test_exists",
        "status": "active",
        "resource": "Patient",
        "select": [{
            "column": [
                {"name": "has_name", "path": "name.exists()", "type": "boolean"}
            ]
        }]
    });

    let sql = generate_sql(view);

    assert!(
        sql.contains("IS NOT NULL"),
        "SQL should check IS NOT NULL for exists()"
    );
}

#[test]
fn test_empty_function() {
    let view = json!({
        "resourceType": "ViewDefinition",
        "name": "test_empty",
        "status": "active",
        "resource": "Patient",
        "select": [{
            "column": [
                {"name": "no_names", "path": "name.empty()", "type": "boolean"}
            ]
        }]
    });

    let sql = generate_sql(view);

    assert!(
        sql.contains("jsonb_array_length"),
        "SQL should use jsonb_array_length for empty()"
    );
}

#[test]
fn test_count_function() {
    let view = json!({
        "resourceType": "ViewDefinition",
        "name": "test_count",
        "status": "active",
        "resource": "Patient",
        "select": [{
            "column": [
                {"name": "name_count", "path": "name.count()", "type": "integer"}
            ]
        }]
    });

    let sql = generate_sql(view);

    assert!(
        sql.contains("jsonb_array_length"),
        "SQL should use jsonb_array_length for count()"
    );
}

#[test]
fn test_string_functions() {
    let view = json!({
        "resourceType": "ViewDefinition",
        "name": "test_string_funcs",
        "status": "active",
        "resource": "Patient",
        "select": [{
            "column": [
                {"name": "contains_test", "path": "name.first().family.contains('son')", "type": "boolean"},
                {"name": "starts_test", "path": "name.first().family.startsWith('Dr')", "type": "boolean"},
                {"name": "ends_test", "path": "name.first().family.endsWith('Jr')", "type": "boolean"}
            ]
        }]
    });

    let sql = generate_sql(view);

    assert!(
        sql.contains("LIKE '%son%'"),
        "SQL should use LIKE for contains()"
    );
    assert!(
        sql.contains("LIKE 'Dr%'"),
        "SQL should use LIKE for startsWith()"
    );
    assert!(
        sql.contains("LIKE '%Jr'"),
        "SQL should use LIKE for endsWith()"
    );
}

#[test]
fn test_matches_function() {
    let view = json!({
        "resourceType": "ViewDefinition",
        "name": "test_matches",
        "status": "active",
        "resource": "Patient",
        "select": [{
            "column": [
                {"name": "is_uppercase", "path": "name.first().family.matches('[A-Z][a-z]+')", "type": "boolean"}
            ]
        }]
    });

    let sql = generate_sql(view);

    assert!(sql.contains("~"), "SQL should use ~ for regex matching");
    assert!(
        sql.contains("[A-Z][a-z]+"),
        "SQL should contain regex pattern"
    );
}

#[test]
fn test_distinct_function() {
    let view = json!({
        "resourceType": "ViewDefinition",
        "name": "test_distinct",
        "status": "active",
        "resource": "Patient",
        "select": [{
            "column": [
                {"name": "unique_identifiers", "path": "identifier.distinct()"}
            ]
        }]
    });

    let sql = generate_sql(view);

    assert!(
        sql.contains("DISTINCT"),
        "SQL should use DISTINCT for distinct()"
    );
    assert!(
        sql.contains("jsonb_agg"),
        "SQL should use jsonb_agg for distinct()"
    );
}

#[test]
fn test_not_function() {
    let view = json!({
        "resourceType": "ViewDefinition",
        "name": "test_not",
        "status": "active",
        "resource": "Patient",
        "select": [{
            "column": [
                {"name": "is_inactive", "path": "active.not()", "type": "boolean"}
            ]
        }]
    });

    let sql = generate_sql(view);

    assert!(sql.contains("NOT"), "SQL should use NOT for not()");
}

#[test]
fn test_has_value_function() {
    let view = json!({
        "resourceType": "ViewDefinition",
        "name": "test_has_value",
        "status": "active",
        "resource": "Patient",
        "select": [{
            "column": [
                {"name": "has_gender", "path": "gender.hasValue()", "type": "boolean"}
            ]
        }]
    });

    let sql = generate_sql(view);

    assert!(
        sql.contains("IS NOT NULL"),
        "SQL should check IS NOT NULL for hasValue()"
    );
}

// =============================================================================
// Repeat Expression Tests
// =============================================================================

#[test]
fn test_repeat_expression() {
    // Test repeat for hierarchical extension traversal
    let view = json!({
        "resourceType": "ViewDefinition",
        "name": "nested_extensions",
        "status": "active",
        "resource": "Patient",
        "select": [{
            "forEach": "extension",
            "repeat": ["extension"],
            "column": [
                {"name": "url", "path": "url"},
                {"name": "value", "path": "valueString"}
            ]
        }]
    });

    let sql = generate_sql(view);

    assert!(
        sql.contains("WITH RECURSIVE"),
        "SQL should use recursive CTE for repeat"
    );
    assert!(
        sql.contains("UNION ALL"),
        "SQL should have UNION ALL for recursion"
    );
    assert!(sql.contains("depth < 10"), "SQL should have depth limit");
}

// =============================================================================
// Column Type Casting Tests
// =============================================================================

#[test]
fn test_all_column_types() {
    let view = json!({
        "resourceType": "ViewDefinition",
        "name": "typed_columns",
        "status": "active",
        "resource": "Patient",
        "select": [{
            "column": [
                {"name": "str_col", "path": "gender", "type": "string"},
                {"name": "int_col", "path": "multipleBirthInteger", "type": "integer"},
                {"name": "dec_col", "path": "extension.valueDecimal", "type": "decimal"},
                {"name": "bool_col", "path": "active", "type": "boolean"},
                {"name": "date_col", "path": "birthDate", "type": "date"},
                {"name": "datetime_col", "path": "meta.lastUpdated", "type": "dateTime"},
                {"name": "instant_col", "path": "meta.lastUpdated", "type": "instant"},
                {"name": "time_col", "path": "extension.valueTime", "type": "time"}
            ]
        }]
    });

    let sql = generate_sql(view);

    // String columns don't need casting
    assert!(
        !sql.contains("str_col") || !sql.contains("::text"),
        "String columns should not be explicitly cast"
    );
    assert!(sql.contains("::bigint"), "Integer should cast to bigint");
    assert!(sql.contains("::numeric"), "Decimal should cast to numeric");
    assert!(sql.contains("::boolean"), "Boolean should cast to boolean");
    assert!(sql.contains("::date"), "Date should cast to date");
    assert!(
        sql.contains("::timestamptz"),
        "DateTime should cast to timestamptz"
    );
    assert!(sql.contains("::time"), "Time should cast to time");
}

// =============================================================================
// Complex Real-World ViewDefinition Tests
// =============================================================================

#[test]
fn test_us_core_patient_view() {
    // Realistic US Core Patient view
    let view = json!({
        "resourceType": "ViewDefinition",
        "url": "http://example.org/views/us-core-patient",
        "name": "us_core_patient",
        "status": "active",
        "resource": "Patient",
        "profile": ["http://hl7.org/fhir/us/core/StructureDefinition/us-core-patient"],
        "select": [{
            "column": [
                {"name": "id", "path": "id"},
                {"name": "resource_key", "path": "getResourceKey()"},
                {"name": "gender", "path": "gender"},
                {"name": "birth_date", "path": "birthDate", "type": "date"},
                {"name": "deceased", "path": "deceasedBoolean", "type": "boolean"},
                {"name": "mrn", "path": "identifier.where(type.coding.code = 'MR').first().value"},
                {"name": "official_family", "path": "name.where(use = 'official').first().family"},
                {"name": "official_given", "path": "name.where(use = 'official').first().given.join(' ')"}
            ]
        }]
    });

    let parsed = parse_view(view.clone());
    assert_eq!(
        parsed.profile,
        vec!["http://hl7.org/fhir/us/core/StructureDefinition/us-core-patient"]
    );

    let sql = generate_sql(view);

    assert!(
        sql.contains("resource_type || '/' ||"),
        "SQL should generate resource key"
    );
    assert!(sql.contains("jsonb_agg"), "SQL should filter with where()");
    assert!(sql.contains("string_agg"), "SQL should join given names");
}

#[test]
fn test_condition_view() {
    // Condition view with references and code filtering
    let view = json!({
        "resourceType": "ViewDefinition",
        "name": "conditions",
        "status": "active",
        "resource": "Condition",
        "constant": [{
            "name": "snomedSystem",
            "valueString": "http://snomed.info/sct"
        }],
        "select": [{
            "column": [
                {"name": "id", "path": "id"},
                {"name": "patient_id", "path": "subject.getReferenceKey(Patient)"},
                {"name": "encounter_id", "path": "encounter.getReferenceKey()"},
                {"name": "code_snomed", "path": "code.coding.where(system = %snomedSystem).first().code"},
                {"name": "code_display", "path": "code.text"},
                {"name": "onset_date", "path": "onsetDateTime", "type": "dateTime"},
                {"name": "clinical_status", "path": "clinicalStatus.coding.first().code"},
                {"name": "verification_status", "path": "verificationStatus.coding.first().code"}
            ]
        }],
        "where": [{
            "path": "clinicalStatus.coding.code = 'active'",
            "description": "Only active conditions"
        }]
    });

    let sql = generate_sql(view);

    assert!(
        sql.contains("FROM condition"),
        "SQL should query condition table"
    );
    assert!(
        sql.contains("http://snomed.info/sct"),
        "SQL should substitute SNOMED constant"
    );
    assert!(sql.contains("WHERE"), "SQL should have WHERE clause");
}

// =============================================================================
// Column Metadata Tests
// =============================================================================

#[test]
fn test_column_tags() {
    let view = json!({
        "resourceType": "ViewDefinition",
        "name": "tagged_columns",
        "status": "active",
        "resource": "Patient",
        "select": [{
            "column": [{
                "name": "mrn",
                "path": "identifier.where(type.coding.code = 'MR').first().value",
                "tag": [
                    {"name": "primaryKey", "value": "true"},
                    {"name": "indexed", "value": "true"}
                ]
            }]
        }]
    });

    let parsed = parse_view(view);
    let column = &parsed.select[0].column.as_ref().unwrap()[0];
    assert_eq!(column.tag.len(), 2);
    assert_eq!(column.tag[0].name, "primaryKey");
    assert_eq!(column.tag[0].value, Some("true".to_string()));
}

#[test]
fn test_where_clause_description() {
    let view = json!({
        "resourceType": "ViewDefinition",
        "name": "described_where",
        "status": "active",
        "resource": "Patient",
        "select": [{
            "column": [{"name": "id", "path": "id"}]
        }],
        "where": [{
            "path": "active = true",
            "description": "Filter to only active patients for HIPAA compliance"
        }]
    });

    let parsed = parse_view(view);
    assert_eq!(
        parsed.where_[0].description,
        Some("Filter to only active patients for HIPAA compliance".to_string())
    );
}

// =============================================================================
// Edge Cases and Error Handling
// =============================================================================

#[test]
fn test_empty_select() {
    let view = json!({
        "resourceType": "ViewDefinition",
        "name": "empty_select",
        "status": "active",
        "resource": "Patient",
        "select": []
    });

    let sql = generate_sql(view);
    assert!(
        sql.contains("SELECT *"),
        "Empty select should result in SELECT *"
    );
}

#[test]
fn test_fhir_version_field() {
    let view = json!({
        "resourceType": "ViewDefinition",
        "name": "versioned_view",
        "status": "active",
        "resource": "Patient",
        "fhirVersion": ["4.0.1", "4.3.0"],
        "select": [{
            "column": [{"name": "id", "path": "id"}]
        }]
    });

    let parsed = parse_view(view);
    assert_eq!(parsed.fhir_version, vec!["4.0.1", "4.3.0"]);
}

#[test]
fn test_view_url_and_description() {
    let view = json!({
        "resourceType": "ViewDefinition",
        "url": "http://example.org/views/test",
        "name": "test_view",
        "status": "active",
        "resource": "Patient",
        "description": "A test view for patients",
        "select": [{
            "column": [{"name": "id", "path": "id"}]
        }]
    });

    let parsed = parse_view(view);
    assert_eq!(
        parsed.url,
        Some("http://example.org/views/test".to_string())
    );
    assert_eq!(
        parsed.description,
        Some("A test view for patients".to_string())
    );
}

// =============================================================================
// SQL Generation Column Order and Structure Tests
// =============================================================================

#[test]
fn test_generated_columns_metadata() {
    let view_json = json!({
        "resourceType": "ViewDefinition",
        "name": "column_metadata",
        "status": "active",
        "resource": "Patient",
        "select": [{
            "column": [
                {"name": "patient_id", "path": "id"},
                {"name": "gender", "path": "gender"},
                {"name": "birth_date", "path": "birthDate", "type": "date"}
            ]
        }]
    });

    let view = ViewDefinition::from_json(&view_json).unwrap();
    let generator = SqlGenerator::new();
    let result = generator.generate(&view).unwrap();

    assert_eq!(result.columns.len(), 3);
    assert_eq!(result.columns[0].name, "patient_id");
    assert_eq!(result.columns[1].name, "gender");
    assert_eq!(result.columns[2].name, "birth_date");
    assert_eq!(result.columns[2].col_type, octofhir_sof::ColumnType::Date);
}
