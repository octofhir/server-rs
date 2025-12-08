//! Schema cache for PostgreSQL database introspection.
//!
//! This module caches database schema information (tables, columns, functions)
//! for providing intelligent completions.

use dashmap::DashMap;
use sqlx_core::error::Error as SqlxError;
use sqlx_core::query_as::query_as;
use std::sync::Arc;

/// Information about a database table.
#[derive(Debug, Clone)]
pub struct TableInfo {
    /// Schema name (e.g., "public")
    pub schema: String,
    /// Table name
    pub name: String,
    /// Table type (BASE TABLE, VIEW, etc.)
    pub table_type: String,
    /// Whether this is a FHIR resource table
    pub is_fhir_table: bool,
    /// FHIR resource type if applicable (e.g., "Patient")
    pub fhir_resource_type: Option<String>,
}

/// Information about a database column.
#[derive(Debug, Clone)]
pub struct ColumnInfo {
    /// Table name this column belongs to
    pub table_name: String,
    /// Column name
    pub name: String,
    /// Data type (e.g., "jsonb", "text", "integer")
    pub data_type: String,
    /// Whether the column is nullable
    pub is_nullable: bool,
    /// Column default value if any
    pub default_value: Option<String>,
    /// Column description/comment if any
    pub description: Option<String>,
}

/// Information about a database function.
#[derive(Debug, Clone)]
pub struct FunctionInfo {
    /// Function name
    pub name: String,
    /// Return type
    pub return_type: String,
    /// Function signature/arguments
    pub signature: String,
    /// Description
    pub description: String,
}

/// Schema cache manager.
pub struct SchemaCache {
    /// Cached tables by schema.table_name
    tables: DashMap<String, TableInfo>,
    /// Cached columns by table_name
    columns: DashMap<String, Vec<ColumnInfo>>,
    /// Cached functions by name
    functions: DashMap<String, FunctionInfo>,
    /// Database connection pool
    db_pool: Arc<sqlx_postgres::PgPool>,
}

impl SchemaCache {
    /// Creates a new schema cache.
    pub fn new(db_pool: Arc<sqlx_postgres::PgPool>) -> Self {
        Self {
            tables: DashMap::new(),
            columns: DashMap::new(),
            functions: DashMap::new(),
            db_pool,
        }
    }

    /// Refresh the schema cache from the database.
    pub async fn refresh(&self) -> Result<(), SqlxError> {
        self.refresh_tables().await?;
        self.refresh_columns().await?;
        self.load_jsonb_functions();
        Ok(())
    }

    /// Refresh table information from information_schema.
    async fn refresh_tables(&self) -> Result<(), SqlxError> {
        let rows = query_as::<_, (String, String, String)>(
            r#"
            SELECT table_schema, table_name, table_type
            FROM information_schema.tables
            WHERE table_schema NOT IN ('pg_catalog', 'information_schema')
            ORDER BY table_schema, table_name
            "#,
        )
        .fetch_all(self.db_pool.as_ref())
        .await?;

        self.tables.clear();

        for (schema, name, table_type) in rows {
            // Check if this looks like a FHIR resource table
            let (is_fhir, resource_type) = Self::detect_fhir_table(&name);

            let key = format!("{}.{}", schema, name);
            self.tables.insert(
                key,
                TableInfo {
                    schema,
                    name,
                    table_type,
                    is_fhir_table: is_fhir,
                    fhir_resource_type: resource_type,
                },
            );
        }

        tracing::debug!("Refreshed {} tables in schema cache", self.tables.len());
        Ok(())
    }

    /// Refresh column information from information_schema.
    async fn refresh_columns(&self) -> Result<(), SqlxError> {
        let rows = query_as::<_, (String, String, String, String, Option<String>)>(
            r#"
            SELECT
                table_name,
                column_name,
                data_type,
                is_nullable,
                column_default
            FROM information_schema.columns
            WHERE table_schema NOT IN ('pg_catalog', 'information_schema')
            ORDER BY table_name, ordinal_position
            "#,
        )
        .fetch_all(self.db_pool.as_ref())
        .await?;

        self.columns.clear();

        for (table_name, column_name, data_type, is_nullable, default_value) in rows {
            let column = ColumnInfo {
                table_name: table_name.clone(),
                name: column_name,
                data_type,
                is_nullable: is_nullable == "YES",
                default_value,
                description: None,
            };

            self.columns
                .entry(table_name)
                .or_insert_with(Vec::new)
                .push(column);
        }

        tracing::debug!(
            "Refreshed columns for {} tables in schema cache",
            self.columns.len()
        );
        Ok(())
    }

    /// Load built-in JSONB functions.
    fn load_jsonb_functions(&self) {
        const JSONB_FUNCTIONS: &[(&str, &str, &str, &str)] = &[
            (
                "jsonb_extract_path",
                "jsonb",
                "jsonb_extract_path(from_json jsonb, VARIADIC path_elems text[])",
                "Extract JSON sub-object at path",
            ),
            (
                "jsonb_extract_path_text",
                "text",
                "jsonb_extract_path_text(from_json jsonb, VARIADIC path_elems text[])",
                "Extract JSON sub-object as text",
            ),
            (
                "jsonb_array_elements",
                "setof jsonb",
                "jsonb_array_elements(from_json jsonb)",
                "Expand JSONB array to set of rows",
            ),
            (
                "jsonb_array_elements_text",
                "setof text",
                "jsonb_array_elements_text(from_json jsonb)",
                "Expand JSONB array as text rows",
            ),
            (
                "jsonb_object_keys",
                "setof text",
                "jsonb_object_keys(from_json jsonb)",
                "Get set of keys in outermost object",
            ),
            (
                "jsonb_typeof",
                "text",
                "jsonb_typeof(from_json jsonb)",
                "Get type of outermost JSON value",
            ),
            (
                "jsonb_agg",
                "jsonb",
                "jsonb_agg(expression anyelement)",
                "Aggregate values as JSONB array",
            ),
            (
                "jsonb_build_object",
                "jsonb",
                "jsonb_build_object(VARIADIC args \"any\")",
                "Build JSONB object from arguments",
            ),
            (
                "jsonb_build_array",
                "jsonb",
                "jsonb_build_array(VARIADIC args \"any\")",
                "Build JSONB array from arguments",
            ),
            (
                "jsonb_set",
                "jsonb",
                "jsonb_set(target jsonb, path text[], new_value jsonb [, create_if_missing boolean])",
                "Set value at path",
            ),
            (
                "jsonb_insert",
                "jsonb",
                "jsonb_insert(target jsonb, path text[], new_value jsonb [, insert_after boolean])",
                "Insert value at path",
            ),
            (
                "jsonb_path_query",
                "setof jsonb",
                "jsonb_path_query(target jsonb, path jsonpath [, vars jsonb [, silent boolean]])",
                "Execute JSONPath query",
            ),
            (
                "jsonb_path_query_array",
                "jsonb",
                "jsonb_path_query_array(target jsonb, path jsonpath [, vars jsonb [, silent boolean]])",
                "JSONPath query as array",
            ),
            (
                "jsonb_path_query_first",
                "jsonb",
                "jsonb_path_query_first(target jsonb, path jsonpath [, vars jsonb [, silent boolean]])",
                "First JSONPath result",
            ),
            (
                "jsonb_path_exists",
                "boolean",
                "jsonb_path_exists(target jsonb, path jsonpath [, vars jsonb [, silent boolean]])",
                "Check if JSONPath returns items",
            ),
            (
                "jsonb_strip_nulls",
                "jsonb",
                "jsonb_strip_nulls(from_json jsonb)",
                "Remove null values recursively",
            ),
            (
                "jsonb_pretty",
                "text",
                "jsonb_pretty(from_json jsonb)",
                "Pretty print JSONB",
            ),
            (
                "jsonb_each",
                "setof record",
                "jsonb_each(from_json jsonb)",
                "Expand to key-value pairs",
            ),
            (
                "jsonb_each_text",
                "setof record",
                "jsonb_each_text(from_json jsonb)",
                "Expand to key-text pairs",
            ),
            (
                "jsonb_populate_record",
                "anyelement",
                "jsonb_populate_record(base anyelement, from_json jsonb)",
                "Populate record from JSONB",
            ),
            (
                "jsonb_to_record",
                "record",
                "jsonb_to_record(from_json jsonb)",
                "Convert JSONB to record",
            ),
            (
                "to_jsonb",
                "jsonb",
                "to_jsonb(anyelement)",
                "Convert to JSONB",
            ),
            (
                "jsonb_array_length",
                "integer",
                "jsonb_array_length(from_json jsonb)",
                "Get length of JSONB array",
            ),
            (
                "jsonb_object",
                "jsonb",
                "jsonb_object(keys text[], values text[])",
                "Build JSONB object from arrays",
            ),
        ];

        self.functions.clear();

        for (name, return_type, signature, description) in JSONB_FUNCTIONS {
            self.functions.insert(
                name.to_string(),
                FunctionInfo {
                    name: name.to_string(),
                    return_type: return_type.to_string(),
                    signature: signature.to_string(),
                    description: description.to_string(),
                },
            );
        }
    }

    /// Detect if a table name corresponds to a FHIR resource.
    fn detect_fhir_table(table_name: &str) -> (bool, Option<String>) {
        // Common FHIR resource types (in PascalCase, lowercase, or snake_case)
        const FHIR_RESOURCES: &[&str] = &[
            "Patient",
            "Practitioner",
            "Organization",
            "Encounter",
            "Observation",
            "Condition",
            "Procedure",
            "Medication",
            "MedicationRequest",
            "MedicationAdministration",
            "MedicationDispense",
            "MedicationStatement",
            "AllergyIntolerance",
            "Immunization",
            "DiagnosticReport",
            "CarePlan",
            "CareTeam",
            "Goal",
            "ServiceRequest",
            "Appointment",
            "Schedule",
            "Slot",
            "Device",
            "DeviceRequest",
            "DeviceUseStatement",
            "Location",
            "Specimen",
            "ImagingStudy",
            "Coverage",
            "Claim",
            "ClaimResponse",
            "ExplanationOfBenefit",
            "DocumentReference",
            "Binary",
            "Bundle",
            "Composition",
            "Consent",
            "Contract",
            "DetectedIssue",
            "FamilyMemberHistory",
            "Flag",
            "Group",
            "HealthcareService",
            "Invoice",
            "List",
            "MeasureReport",
            "NutritionOrder",
            "OperationOutcome",
            "Person",
            "Provenance",
            "Questionnaire",
            "QuestionnaireResponse",
            "RelatedPerson",
            "RequestGroup",
            "RiskAssessment",
            "SupplyDelivery",
            "SupplyRequest",
            "Task",
        ];

        // Normalize table name to PascalCase for comparison
        let normalized = Self::to_pascal_case(table_name);

        for resource in FHIR_RESOURCES {
            if normalized.eq_ignore_ascii_case(resource) {
                return (true, Some(resource.to_string()));
            }
        }

        (false, None)
    }

    /// Convert a table name to PascalCase.
    fn to_pascal_case(s: &str) -> String {
        let mut result = String::new();
        let mut capitalize_next = true;

        for c in s.chars() {
            if c == '_' || c == '-' {
                capitalize_next = true;
            } else if capitalize_next {
                result.push(c.to_ascii_uppercase());
                capitalize_next = false;
            } else {
                result.push(c.to_ascii_lowercase());
            }
        }

        result
    }

    /// Get all tables.
    pub fn get_tables(&self) -> Vec<TableInfo> {
        self.tables.iter().map(|r| r.value().clone()).collect()
    }

    /// Get tables matching a prefix.
    pub fn get_tables_matching(&self, prefix: &str) -> Vec<TableInfo> {
        let prefix_lower = prefix.to_lowercase();
        self.tables
            .iter()
            .filter(|r| r.value().name.to_lowercase().starts_with(&prefix_lower))
            .map(|r| r.value().clone())
            .collect()
    }

    /// Get FHIR resource tables.
    pub fn get_fhir_tables(&self) -> Vec<TableInfo> {
        self.tables
            .iter()
            .filter(|r| r.value().is_fhir_table)
            .map(|r| r.value().clone())
            .collect()
    }

    /// Get columns for a table.
    pub fn get_columns(&self, table_name: &str) -> Vec<ColumnInfo> {
        self.columns
            .get(table_name)
            .map(|r| r.value().clone())
            .unwrap_or_default()
    }

    /// Get columns matching a prefix for a table.
    pub fn get_columns_matching(&self, table_name: &str, prefix: &str) -> Vec<ColumnInfo> {
        let prefix_lower = prefix.to_lowercase();
        self.columns
            .get(table_name)
            .map(|r| {
                r.value()
                    .iter()
                    .filter(|c| c.name.to_lowercase().starts_with(&prefix_lower))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get JSONB columns for a table.
    pub fn get_jsonb_columns(&self, table_name: &str) -> Vec<ColumnInfo> {
        self.columns
            .get(table_name)
            .map(|r| {
                r.value()
                    .iter()
                    .filter(|c| c.data_type == "jsonb")
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get all JSONB functions.
    pub fn get_functions(&self) -> Vec<FunctionInfo> {
        self.functions.iter().map(|r| r.value().clone()).collect()
    }

    /// Get functions matching a prefix.
    pub fn get_functions_matching(&self, prefix: &str) -> Vec<FunctionInfo> {
        let prefix_lower = prefix.to_lowercase();
        self.functions
            .iter()
            .filter(|r| r.value().name.to_lowercase().starts_with(&prefix_lower))
            .map(|r| r.value().clone())
            .collect()
    }

    /// Get a specific function by name.
    pub fn get_function(&self, name: &str) -> Option<FunctionInfo> {
        self.functions.get(name).map(|r| r.value().clone())
    }

    /// Check if a table is a FHIR resource table.
    pub fn is_fhir_table(&self, table_name: &str) -> bool {
        self.tables
            .iter()
            .any(|r| r.value().name == table_name && r.value().is_fhir_table)
    }

    /// Get the FHIR resource type for a table.
    pub fn get_fhir_resource_type(&self, table_name: &str) -> Option<String> {
        self.tables
            .iter()
            .find(|r| r.value().name == table_name)
            .and_then(|r| r.value().fhir_resource_type.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_pascal_case() {
        assert_eq!(SchemaCache::to_pascal_case("patient"), "Patient");
        assert_eq!(SchemaCache::to_pascal_case("Patient"), "Patient");
        // Note: to_pascal_case capitalizes first letter of each segment (split by _ or -)
        // but the FHIR table detection uses case-insensitive comparison
        assert_eq!(
            SchemaCache::to_pascal_case("medication_request"),
            "MedicationRequest"
        );
        assert_eq!(
            SchemaCache::to_pascal_case("allergy_intolerance"),
            "AllergyIntolerance"
        );
    }

    #[test]
    fn test_detect_fhir_table() {
        assert_eq!(
            SchemaCache::detect_fhir_table("patient"),
            (true, Some("Patient".to_string()))
        );
        assert_eq!(
            SchemaCache::detect_fhir_table("Patient"),
            (true, Some("Patient".to_string()))
        );
        assert_eq!(
            SchemaCache::detect_fhir_table("observation"),
            (true, Some("Observation".to_string()))
        );
        // With underscore - should still match via case-insensitive comparison
        assert_eq!(
            SchemaCache::detect_fhir_table("medication_request"),
            (true, Some("MedicationRequest".to_string()))
        );
        assert_eq!(SchemaCache::detect_fhir_table("some_random_table"), (false, None));
    }
}
