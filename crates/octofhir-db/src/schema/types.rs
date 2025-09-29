// Type definitions for schema generation

use serde::{Deserialize, Serialize};
use std::fmt;

/// Describes a PostgreSQL table to be generated
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableDescriptor {
    /// Table name (e.g., "patient", "observation")
    pub name: String,
    /// FHIR resource type this table represents
    pub resource_type: String,
    /// Columns in this table
    pub columns: Vec<ColumnDescriptor>,
    /// Indexes to create
    pub indexes: Vec<IndexDescriptor>,
    /// Foreign key constraints
    pub foreign_keys: Vec<ForeignKeyDescriptor>,
}

impl TableDescriptor {
    pub fn new(name: String, resource_type: String) -> Self {
        Self {
            name,
            resource_type,
            columns: Vec::new(),
            indexes: Vec::new(),
            foreign_keys: Vec::new(),
        }
    }

    pub fn with_column(mut self, column: ColumnDescriptor) -> Self {
        self.columns.push(column);
        self
    }

    pub fn with_index(mut self, index: IndexDescriptor) -> Self {
        self.indexes.push(index);
        self
    }
}

/// Describes a column in a PostgreSQL table
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnDescriptor {
    /// Column name
    pub name: String,
    /// PostgreSQL data type
    pub data_type: PostgresType,
    /// Whether this column is nullable
    pub nullable: bool,
    /// Whether this is a primary key
    pub primary_key: bool,
    /// Default value expression (if any)
    pub default: Option<String>,
    /// FHIR element path this column maps to
    pub fhir_path: Option<String>,
    /// Cardinality constraints (min, max)
    pub cardinality: Option<(usize, Option<usize>)>,
}

impl ColumnDescriptor {
    pub fn new(name: String, data_type: PostgresType) -> Self {
        Self {
            name,
            data_type,
            nullable: true,
            primary_key: false,
            default: None,
            fhir_path: None,
            cardinality: None,
        }
    }

    pub fn not_null(mut self) -> Self {
        self.nullable = false;
        self
    }

    pub fn primary(mut self) -> Self {
        self.primary_key = true;
        self.nullable = false;
        self
    }

    pub fn with_default(mut self, default: String) -> Self {
        self.default = Some(default);
        self
    }

    pub fn with_fhir_path(mut self, path: String) -> Self {
        self.fhir_path = Some(path);
        self
    }

    pub fn with_cardinality(mut self, min: usize, max: Option<usize>) -> Self {
        self.cardinality = Some((min, max));
        self
    }
}

/// PostgreSQL data types supported for FHIR elements
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PostgresType {
    /// Variable-length text
    Text,
    /// JSON or JSONB (we use JSONB for performance)
    Jsonb,
    /// UUID for identifiers
    Uuid,
    /// Timestamp with timezone
    Timestamptz,
    /// Date (without time)
    Date,
    /// Boolean
    Boolean,
    /// Integer (4 bytes)
    Integer,
    /// Big integer (8 bytes)
    Bigint,
    /// Numeric/decimal with precision
    Numeric,
    /// Array of text
    TextArray,
}

impl fmt::Display for PostgresType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PostgresType::Text => write!(f, "TEXT"),
            PostgresType::Jsonb => write!(f, "JSONB"),
            PostgresType::Uuid => write!(f, "UUID"),
            PostgresType::Timestamptz => write!(f, "TIMESTAMPTZ"),
            PostgresType::Date => write!(f, "DATE"),
            PostgresType::Boolean => write!(f, "BOOLEAN"),
            PostgresType::Integer => write!(f, "INTEGER"),
            PostgresType::Bigint => write!(f, "BIGINT"),
            PostgresType::Numeric => write!(f, "NUMERIC"),
            PostgresType::TextArray => write!(f, "TEXT[]"),
        }
    }
}

/// Describes an index on a table
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexDescriptor {
    /// Index name
    pub name: String,
    /// Columns included in this index
    pub columns: Vec<String>,
    /// Whether this is a unique index
    pub unique: bool,
    /// Index method (btree, gin, gist, etc.)
    pub method: IndexMethod,
    /// Optional WHERE clause for partial index
    pub where_clause: Option<String>,
}

impl IndexDescriptor {
    pub fn new(name: String, columns: Vec<String>) -> Self {
        Self {
            name,
            columns,
            unique: false,
            method: IndexMethod::BTree,
            where_clause: None,
        }
    }

    pub fn gin(mut self) -> Self {
        self.method = IndexMethod::Gin;
        self
    }

    pub fn unique(mut self) -> Self {
        self.unique = true;
        self
    }
}

/// PostgreSQL index methods
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IndexMethod {
    /// B-tree index (default, good for equality and range queries)
    BTree,
    /// GIN index (good for JSONB and arrays)
    Gin,
    /// GiST index (good for geometric data)
    Gist,
    /// Hash index (good for equality only)
    Hash,
}

impl fmt::Display for IndexMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IndexMethod::BTree => write!(f, "BTREE"),
            IndexMethod::Gin => write!(f, "GIN"),
            IndexMethod::Gist => write!(f, "GIST"),
            IndexMethod::Hash => write!(f, "HASH"),
        }
    }
}

/// Describes a foreign key constraint
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForeignKeyDescriptor {
    /// Name of the foreign key constraint
    pub name: String,
    /// Column(s) in this table
    pub columns: Vec<String>,
    /// Referenced table name
    pub referenced_table: String,
    /// Referenced column(s)
    pub referenced_columns: Vec<String>,
    /// ON DELETE action
    pub on_delete: ForeignKeyAction,
    /// ON UPDATE action
    pub on_update: ForeignKeyAction,
}

/// Foreign key constraint actions
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ForeignKeyAction {
    /// No action
    NoAction,
    /// Restrict (same as NO ACTION but checked immediately)
    Restrict,
    /// Cascade the change
    Cascade,
    /// Set to NULL
    SetNull,
    /// Set to default value
    SetDefault,
}

impl fmt::Display for ForeignKeyAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ForeignKeyAction::NoAction => write!(f, "NO ACTION"),
            ForeignKeyAction::Restrict => write!(f, "RESTRICT"),
            ForeignKeyAction::Cascade => write!(f, "CASCADE"),
            ForeignKeyAction::SetNull => write!(f, "SET NULL"),
            ForeignKeyAction::SetDefault => write!(f, "SET DEFAULT"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_table_descriptor_builder() {
        let table = TableDescriptor::new("patient".to_string(), "Patient".to_string())
            .with_column(
                ColumnDescriptor::new("id".to_string(), PostgresType::Uuid)
                    .primary()
                    .with_fhir_path("Patient.id".to_string()),
            )
            .with_column(
                ColumnDescriptor::new("resource".to_string(), PostgresType::Jsonb).not_null(),
            );

        assert_eq!(table.name, "patient");
        assert_eq!(table.resource_type, "Patient");
        assert_eq!(table.columns.len(), 2);
        assert!(table.columns[0].primary_key);
        assert!(!table.columns[1].nullable);
    }

    #[test]
    fn test_postgres_type_display() {
        assert_eq!(PostgresType::Text.to_string(), "TEXT");
        assert_eq!(PostgresType::Jsonb.to_string(), "JSONB");
        assert_eq!(PostgresType::Uuid.to_string(), "UUID");
        assert_eq!(PostgresType::Timestamptz.to_string(), "TIMESTAMPTZ");
    }

    #[test]
    fn test_index_descriptor() {
        let index = IndexDescriptor::new(
            "idx_patient_identifier".to_string(),
            vec!["identifier".to_string()],
        )
        .gin()
        .unique();

        assert_eq!(index.method, IndexMethod::Gin);
        assert!(index.unique);
    }
}