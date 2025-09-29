// Schema generation module for PostgreSQL DDL
// Implements ADR-001 (Diff History Strategy) and ADR-002 (DDL Generation from StructureDefinition)

pub mod canonical_integration;
pub mod ddl;
pub mod element;
pub mod fhir_parser;
pub mod generator;
pub mod history;
pub mod types;

pub use canonical_integration::{CanonicalSchemaManager, SchemaManagerError};
pub use ddl::DdlGenerator;
pub use element::{ElementDescriptor, ElementType};
pub use fhir_parser::{parse_structure_definition, ParseError};
pub use generator::{GeneratedSchema, SchemaGenerationError, SchemaGenerator, SchemaMetadata};
pub use history::{HistoryTableDescriptor, SnapshotStrategy};
pub use types::{ColumnDescriptor, TableDescriptor};