// Schema generator for FHIR resources using StructureDefinition metadata
// Implements ADR-002: DDL Generation from StructureDefinition

use super::ddl::DdlGenerator;
use super::element::ElementDescriptor;
use super::history::{HistoryTableDescriptor, SnapshotStrategy};
use super::types::{ColumnDescriptor, IndexDescriptor, PostgresType, TableDescriptor};
use serde::{Deserialize, Serialize};

/// Metadata about generated schema
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaMetadata {
    /// Resource type this schema is for
    pub resource_type: String,
    /// FHIR version
    pub fhir_version: String,
    /// Package ID (if from a package)
    pub package_id: Option<String>,
    /// Package version
    pub package_version: Option<String>,
    /// Hash of the StructureDefinition used to generate this schema
    pub structure_definition_hash: Option<String>,
    /// Timestamp when schema was generated
    pub generated_at: String,
}

/// Main schema generator
pub struct SchemaGenerator {
    /// Schema name for PostgreSQL
    schema_name: String,
    /// DDL generator
    ddl_generator: DdlGenerator,
    /// Snapshot strategy for history tables
    snapshot_strategy: SnapshotStrategy,
}

impl Default for SchemaGenerator {
    fn default() -> Self {
        Self {
            schema_name: "public".to_string(),
            ddl_generator: DdlGenerator::new(),
            snapshot_strategy: SnapshotStrategy::default(),
        }
    }
}

impl SchemaGenerator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_schema(mut self, schema: String) -> Self {
        self.schema_name = schema.clone();
        self.ddl_generator = self.ddl_generator.with_schema(schema);
        self
    }

    pub fn with_snapshot_strategy(mut self, strategy: SnapshotStrategy) -> Self {
        self.snapshot_strategy = strategy;
        self
    }

    /// Generate schema for a FHIR resource from element descriptors
    ///
    /// This is the main entry point for schema generation. It creates:
    /// 1. Base resource table with columns for key elements
    /// 2. History table following ADR-001
    /// 3. Appropriate indexes for common queries
    pub fn generate_resource_schema(
        &self,
        resource_type: &str,
        elements: Vec<ElementDescriptor>,
    ) -> Result<GeneratedSchema, SchemaGenerationError> {
        let table_name = to_table_name(resource_type);

        // Build base resource table
        let mut table = self.build_base_table(&table_name, resource_type, &elements)?;

        // Add standard FHIR resource columns
        self.add_standard_resource_columns(&mut table);

        // Add indexes for search and performance
        self.add_standard_indexes(&mut table);

        // Generate history table descriptor
        let history_descriptor =
            HistoryTableDescriptor::new(table_name.clone(), resource_type.to_string())
                .with_strategy(self.snapshot_strategy);
        let history_table = history_descriptor.to_table_descriptor();

        // Generate DDL
        let base_ddl = self.ddl_generator.generate_table_ddl(&table)?;
        let history_ddl = self.ddl_generator.generate_table_ddl(&history_table)?;

        Ok(GeneratedSchema {
            resource_type: resource_type.to_string(),
            base_table: table,
            history_table,
            base_ddl,
            history_ddl,
            metadata: SchemaMetadata {
                resource_type: resource_type.to_string(),
                fhir_version: "R5".to_string(), // TODO: make configurable
                package_id: None,
                package_version: None,
                structure_definition_hash: None,
                generated_at: chrono::Utc::now().to_rfc3339(),
            },
        })
    }

    /// Build base resource table from element descriptors
    ///
    /// Note: We don't extract individual columns from elements because FHIR resources
    /// are complex with nested arrays, polymorphic types, and dynamic structures.
    /// Everything is stored in the JSONB `resource` column.
    fn build_base_table(
        &self,
        table_name: &str,
        resource_type: &str,
        _elements: &[ElementDescriptor],
    ) -> Result<TableDescriptor, SchemaGenerationError> {
        let table = TableDescriptor::new(table_name.to_string(), resource_type.to_string());

        // All FHIR resource data goes into the JSONB resource column
        // Individual columns are added by add_standard_resource_columns()

        Ok(table)
    }

    /// Add standard columns that every FHIR resource table should have
    ///
    /// Simple schema: id, resource (JSONB), created_at, updated_at
    /// This avoids the complexity of FHIR's nested arrays and polymorphic types
    fn add_standard_resource_columns(&self, table: &mut TableDescriptor) {
        // Primary key (logical ID) - maps to FHIR resource.id
        table.columns.push(
            ColumnDescriptor::new("id".to_string(), PostgresType::Uuid)
                .primary()
                .with_default("gen_random_uuid()".to_string())
                .with_fhir_path(format!("{}.id", table.resource_type)),
        );

        // Full resource as JSONB (canonical storage for all FHIR data)
        table.columns.push(
            ColumnDescriptor::new("resource".to_string(), PostgresType::Jsonb)
                .not_null()
                .with_fhir_path(format!("{}", table.resource_type)),
        );

        // Creation timestamp (when the resource was first created)
        table.columns.push(
            ColumnDescriptor::new("created_at".to_string(), PostgresType::Timestamptz)
                .not_null()
                .with_default("CURRENT_TIMESTAMP".to_string()),
        );

        // Update timestamp (when the resource was last modified)
        table.columns.push(
            ColumnDescriptor::new("updated_at".to_string(), PostgresType::Timestamptz)
                .not_null()
                .with_default("CURRENT_TIMESTAMP".to_string())
                .with_fhir_path(format!("{}.meta.lastUpdated", table.resource_type)),
        );
    }

    /// Add standard indexes for common FHIR queries
    fn add_standard_indexes(&self, table: &mut TableDescriptor) {
        // GIN index on resource JSONB for flexible queries and search parameters
        // This enables queries like: WHERE resource @> '{"status": "active"}'
        table.indexes.push(
            IndexDescriptor::new(
                format!("idx_{}_resource_gin", table.name),
                vec!["resource".to_string()],
            )
            .gin(),
        );

        // B-tree index on updated_at for _lastUpdated search parameter
        table.indexes.push(IndexDescriptor::new(
            format!("idx_{}_updated_at", table.name),
            vec!["updated_at".to_string()],
        ));

        // B-tree index on created_at for temporal queries
        table.indexes.push(IndexDescriptor::new(
            format!("idx_{}_created_at", table.name),
            vec!["created_at".to_string()],
        ));
    }

    /// Determine if an element should be extracted to its own column
    /// vs. stored only in the main JSONB resource column
    ///
    /// For simplicity and correctness, we don't extract any columns.
    /// FHIR resources have complex nested structures, arrays, and polymorphic types
    /// that are better queried via JSONB operators and GIN indexes.
    fn should_extract_column(&self, _element: &ElementDescriptor) -> bool {
        false // Never extract - always use JSONB
    }
}

/// Result of schema generation
#[derive(Debug, Clone)]
pub struct GeneratedSchema {
    pub resource_type: String,
    pub base_table: TableDescriptor,
    pub history_table: TableDescriptor,
    pub base_ddl: String,
    pub history_ddl: String,
    pub metadata: SchemaMetadata,
}

impl GeneratedSchema {
    /// Get complete DDL for both base and history tables
    pub fn complete_ddl(&self) -> String {
        format!(
            "-- Schema for FHIR resource: {}\n-- Generated at: {}\n\n{}\n\n{}",
            self.resource_type,
            self.metadata.generated_at,
            self.base_ddl,
            self.history_ddl
        )
    }
}

/// Errors that can occur during schema generation
#[derive(Debug, thiserror::Error)]
pub enum SchemaGenerationError {
    #[error("Invalid resource type: {0}")]
    InvalidResourceType(String),

    #[error("DDL generation failed: {0}")]
    DdlGenerationFailed(#[from] std::fmt::Error),

    #[error("Missing required elements for resource type: {0}")]
    MissingRequiredElements(String),
}

/// Convert FHIR resource type to table name (lowercase)
fn to_table_name(resource_type: &str) -> String {
    resource_type.to_lowercase()
}

// Placeholder for chrono until we add it
mod chrono {
    pub struct Utc;
    impl Utc {
        pub fn now() -> DateTime {
            DateTime
        }
    }
    pub struct DateTime;
    impl DateTime {
        pub fn to_rfc3339(&self) -> String {
            "2025-01-01T00:00:00Z".to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::ElementType;

    #[test]
    fn test_to_table_name() {
        assert_eq!(to_table_name("Patient"), "patient");
        assert_eq!(to_table_name("Observation"), "observation");
        assert_eq!(to_table_name("DiagnosticReport"), "diagnosticreport");
    }

    #[test]
    fn test_generate_simple_resource_schema() {
        let generator = SchemaGenerator::new();

        let elements = vec![
            ElementDescriptor::new("Patient.id".to_string())
                .with_type(ElementType::Id)
                .required(),
            ElementDescriptor::new("Patient.active".to_string())
                .with_type(ElementType::Boolean)
                .with_cardinality(0, Some(1)),
            ElementDescriptor::new("Patient.name".to_string())
                .with_type(ElementType::HumanName)
                .with_cardinality(0, None),
        ];

        let schema = generator
            .generate_resource_schema("Patient", elements)
            .unwrap();

        assert_eq!(schema.resource_type, "Patient");
        assert_eq!(schema.base_table.name, "patient");
        assert_eq!(schema.history_table.name, "patient_history");

        // Check standard columns exist
        assert!(schema.base_table.columns.iter().any(|c| c.name == "id"));
        assert!(schema
            .base_table
            .columns
            .iter()
            .any(|c| c.name == "resource"));
        assert!(schema
            .base_table
            .columns
            .iter()
            .any(|c| c.name == "created_at"));
        assert!(schema
            .base_table
            .columns
            .iter()
            .any(|c| c.name == "updated_at"));

        // Check indexes exist
        assert!(!schema.base_table.indexes.is_empty());

        // Check DDL was generated
        assert!(schema.base_ddl.contains("CREATE TABLE"));
        assert!(schema.history_ddl.contains("CREATE TABLE"));
        assert!(schema.history_ddl.contains("patient_history"));
    }

    #[test]
    fn test_complete_ddl() {
        let generator = SchemaGenerator::new();
        let elements = vec![ElementDescriptor::new("Patient.id".to_string())
            .with_type(ElementType::Id)];

        let schema = generator
            .generate_resource_schema("Patient", elements)
            .unwrap();
        let complete = schema.complete_ddl();

        assert!(complete.contains("-- Schema for FHIR resource: Patient"));
        assert!(complete.contains("CREATE TABLE"));
    }
}