//! Column type definitions for SQL on FHIR views.
//!
//! This module defines the types used to represent column information
//! in the generated SQL and result sets.

use serde::{Deserialize, Serialize};

/// Information about a column in a view result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnInfo {
    /// The column name.
    pub name: String,

    /// The column's data type.
    pub col_type: ColumnType,

    /// Whether this column can contain null values.
    pub nullable: bool,

    /// Human-readable description of the column.
    pub description: Option<String>,
}

impl ColumnInfo {
    /// Create a new column info with default settings.
    pub fn new(name: impl Into<String>, col_type: ColumnType) -> Self {
        Self {
            name: name.into(),
            col_type,
            nullable: true,
            description: None,
        }
    }

    /// Set whether this column is nullable.
    pub fn with_nullable(mut self, nullable: bool) -> Self {
        self.nullable = nullable;
        self
    }

    /// Set the column description.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }
}

/// Data types supported by SQL on FHIR columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ColumnType {
    /// String/text values.
    #[default]
    String,

    /// Integer values.
    Integer,

    /// Decimal/floating-point values.
    Decimal,

    /// Boolean values.
    Boolean,

    /// Date values (YYYY-MM-DD).
    Date,

    /// DateTime values (ISO 8601).
    DateTime,

    /// Instant values (precise timestamp).
    Instant,

    /// Time values (HH:MM:SS).
    Time,

    /// Base64 encoded binary data.
    Base64Binary,

    /// JSON/complex object (when collection=true or complex type).
    Json,
}

impl ColumnType {
    /// Parse a column type from a string.
    ///
    /// Returns `String` as the default type for unknown values.
    pub fn from_fhir_type(type_str: &str) -> Self {
        match type_str.to_lowercase().as_str() {
            "string" | "code" | "uri" | "url" | "canonical" | "id" | "oid" | "uuid"
            | "markdown" => Self::String,
            "integer" | "positiveint" | "unsignedint" | "integer64" => Self::Integer,
            "decimal" => Self::Decimal,
            "boolean" => Self::Boolean,
            "date" => Self::Date,
            "datetime" => Self::DateTime,
            "instant" => Self::Instant,
            "time" => Self::Time,
            "base64binary" => Self::Base64Binary,
            _ => Self::String, // Default to string for unknown types
        }
    }

    /// Get the SQL type name for this column type (PostgreSQL).
    pub fn sql_type(&self) -> &'static str {
        match self {
            Self::String => "TEXT",
            Self::Integer => "BIGINT",
            Self::Decimal => "NUMERIC",
            Self::Boolean => "BOOLEAN",
            Self::Date => "DATE",
            Self::DateTime | Self::Instant => "TIMESTAMPTZ",
            Self::Time => "TIME",
            Self::Base64Binary => "BYTEA",
            Self::Json => "JSONB",
        }
    }

    /// Get the default value representation for null values.
    pub fn null_representation(&self) -> &'static str {
        "NULL"
    }
}

impl std::fmt::Display for ColumnType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::String => write!(f, "string"),
            Self::Integer => write!(f, "integer"),
            Self::Decimal => write!(f, "decimal"),
            Self::Boolean => write!(f, "boolean"),
            Self::Date => write!(f, "date"),
            Self::DateTime => write!(f, "dateTime"),
            Self::Instant => write!(f, "instant"),
            Self::Time => write!(f, "time"),
            Self::Base64Binary => write!(f, "base64Binary"),
            Self::Json => write!(f, "json"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_column_type_from_fhir_type() {
        assert_eq!(ColumnType::from_fhir_type("string"), ColumnType::String);
        assert_eq!(ColumnType::from_fhir_type("code"), ColumnType::String);
        assert_eq!(ColumnType::from_fhir_type("integer"), ColumnType::Integer);
        assert_eq!(
            ColumnType::from_fhir_type("positiveInt"),
            ColumnType::Integer
        );
        assert_eq!(ColumnType::from_fhir_type("decimal"), ColumnType::Decimal);
        assert_eq!(ColumnType::from_fhir_type("boolean"), ColumnType::Boolean);
        assert_eq!(ColumnType::from_fhir_type("date"), ColumnType::Date);
        assert_eq!(ColumnType::from_fhir_type("dateTime"), ColumnType::DateTime);
        assert_eq!(ColumnType::from_fhir_type("instant"), ColumnType::Instant);
        assert_eq!(ColumnType::from_fhir_type("time"), ColumnType::Time);
        assert_eq!(
            ColumnType::from_fhir_type("base64Binary"),
            ColumnType::Base64Binary
        );
        // Unknown types default to string
        assert_eq!(
            ColumnType::from_fhir_type("UnknownType"),
            ColumnType::String
        );
    }

    #[test]
    fn test_column_type_sql_type() {
        assert_eq!(ColumnType::String.sql_type(), "TEXT");
        assert_eq!(ColumnType::Integer.sql_type(), "BIGINT");
        assert_eq!(ColumnType::Decimal.sql_type(), "NUMERIC");
        assert_eq!(ColumnType::Boolean.sql_type(), "BOOLEAN");
        assert_eq!(ColumnType::Date.sql_type(), "DATE");
        assert_eq!(ColumnType::DateTime.sql_type(), "TIMESTAMPTZ");
        assert_eq!(ColumnType::Json.sql_type(), "JSONB");
    }

    #[test]
    fn test_column_info_builder() {
        let col = ColumnInfo::new("test_col", ColumnType::String)
            .with_nullable(false)
            .with_description("A test column");

        assert_eq!(col.name, "test_col");
        assert_eq!(col.col_type, ColumnType::String);
        assert!(!col.nullable);
        assert_eq!(col.description, Some("A test column".to_string()));
    }
}
