// FHIR element descriptors and type mapping
// Maps FHIR element types to PostgreSQL column types

use super::types::{ColumnDescriptor, PostgresType};
use serde::{Deserialize, Serialize};

/// Describes a FHIR element from StructureDefinition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElementDescriptor {
    /// FHIR path (e.g., "Patient.name", "Observation.value[x]")
    pub path: String,
    /// Element type(s) - can be multiple for polymorphic elements
    pub types: Vec<ElementType>,
    /// Minimum cardinality
    pub min: usize,
    /// Maximum cardinality (None means *)
    pub max: Option<usize>,
    /// Whether this element is a modifier element
    pub is_modifier: bool,
    /// Whether this element must be supported
    pub must_support: bool,
    /// Short description
    pub short: Option<String>,
    /// Definition text
    pub definition: Option<String>,
}

impl ElementDescriptor {
    pub fn new(path: String) -> Self {
        Self {
            path,
            types: Vec::new(),
            min: 0,
            max: Some(1),
            is_modifier: false,
            must_support: false,
            short: None,
            definition: None,
        }
    }

    pub fn with_type(mut self, element_type: ElementType) -> Self {
        self.types.push(element_type);
        self
    }

    pub fn with_cardinality(mut self, min: usize, max: Option<usize>) -> Self {
        self.min = min;
        self.max = max;
        self
    }

    pub fn required(mut self) -> Self {
        self.min = 1;
        self
    }

    pub fn is_required(&self) -> bool {
        self.min >= 1
    }

    pub fn is_array(&self) -> bool {
        match self.max {
            Some(max) => max > 1,
            None => true, // None means unbounded (*)
        }
    }

    pub fn is_polymorphic(&self) -> bool {
        self.types.len() > 1 || self.path.ends_with("[x]")
    }

    /// Map this FHIR element to a PostgreSQL column descriptor
    pub fn to_column_descriptor(&self, column_name: Option<String>) -> ColumnDescriptor {
        let name = column_name.unwrap_or_else(|| self.extract_column_name());

        // Determine PostgreSQL type based on FHIR types
        let pg_type = if self.is_polymorphic() || self.types.is_empty() {
            // Polymorphic or unknown types stored as JSONB
            PostgresType::Jsonb
        } else if self.is_array() {
            // Arrays stored as JSONB for flexibility
            PostgresType::Jsonb
        } else {
            // Map single type to PostgreSQL type
            self.types[0].to_postgres_type()
        };

        let mut column = ColumnDescriptor::new(name, pg_type)
            .with_fhir_path(self.path.clone())
            .with_cardinality(self.min, self.max);

        if self.is_required() {
            column = column.not_null();
        }

        column
    }

    /// Extract column name from FHIR path
    fn extract_column_name(&self) -> String {
        // Take the last part of the path and convert to snake_case
        let last_part = self
            .path
            .split('.')
            .last()
            .unwrap_or(&self.path)
            .replace("[x]", "");

        to_snake_case(&last_part)
    }
}

/// FHIR element types
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ElementType {
    // Primitive types
    Boolean,
    Integer,
    String,
    Decimal,
    Uri,
    Url,
    Canonical,
    Base64Binary,
    Instant,
    Date,
    DateTime,
    Time,
    Code,
    Oid,
    Id,
    Markdown,
    UnsignedInt,
    PositiveInt,
    Uuid,

    // Complex types
    HumanName,
    Address,
    ContactPoint,
    Identifier,
    CodeableConcept,
    Coding,
    Quantity,
    Range,
    Period,
    Ratio,
    Reference,
    Attachment,
    Meta,
    Narrative,
    Extension,

    // Special backbone elements
    BackboneElement,

    // Resource references
    Resource(String),
}

impl ElementType {
    /// Map FHIR element type to PostgreSQL type
    pub fn to_postgres_type(&self) -> PostgresType {
        match self {
            ElementType::Boolean => PostgresType::Boolean,
            ElementType::Integer | ElementType::UnsignedInt | ElementType::PositiveInt => {
                PostgresType::Integer
            }
            ElementType::Decimal => PostgresType::Numeric,
            ElementType::Date => PostgresType::Date,
            ElementType::DateTime | ElementType::Instant => PostgresType::Timestamptz,
            ElementType::Uuid => PostgresType::Uuid,
            ElementType::String
            | ElementType::Uri
            | ElementType::Url
            | ElementType::Canonical
            | ElementType::Code
            | ElementType::Oid
            | ElementType::Id
            | ElementType::Markdown => PostgresType::Text,

            // Complex types stored as JSONB
            ElementType::HumanName
            | ElementType::Address
            | ElementType::ContactPoint
            | ElementType::Identifier
            | ElementType::CodeableConcept
            | ElementType::Coding
            | ElementType::Quantity
            | ElementType::Range
            | ElementType::Period
            | ElementType::Ratio
            | ElementType::Reference
            | ElementType::Attachment
            | ElementType::Meta
            | ElementType::Narrative
            | ElementType::Extension
            | ElementType::BackboneElement
            | ElementType::Base64Binary
            | ElementType::Time
            | ElementType::Resource(_) => PostgresType::Jsonb,
        }
    }

    /// Check if this type represents a reference to another resource
    pub fn is_reference(&self) -> bool {
        matches!(self, ElementType::Reference | ElementType::Resource(_))
    }
}

/// Convert PascalCase or camelCase to snake_case
/// Handles simple cases like "firstName" -> "first_name"
/// For acronyms and edge cases, elements can specify custom column names
fn to_snake_case(s: &str) -> String {
    let mut result = String::new();
    let chars: Vec<char> = s.chars().collect();

    for (i, &ch) in chars.iter().enumerate() {
        if ch.is_uppercase() {
            // Add underscore before uppercase if not first and previous was lowercase
            if i > 0 && chars[i - 1].is_lowercase() {
                result.push('_');
            }
            result.push(ch.to_ascii_lowercase());
        } else {
            result.push(ch);
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_element_descriptor_simple() {
        let element = ElementDescriptor::new("Patient.active".to_string())
            .with_type(ElementType::Boolean)
            .with_cardinality(0, Some(1));

        assert!(!element.is_required());
        assert!(!element.is_array());
        assert!(!element.is_polymorphic());
    }

    #[test]
    fn test_element_descriptor_required() {
        let element = ElementDescriptor::new("Patient.id".to_string())
            .with_type(ElementType::Id)
            .required();

        assert!(element.is_required());
    }

    #[test]
    fn test_element_descriptor_array() {
        let element = ElementDescriptor::new("Patient.name".to_string())
            .with_type(ElementType::HumanName)
            .with_cardinality(0, None);

        assert!(element.is_array());
    }

    #[test]
    fn test_element_descriptor_polymorphic() {
        let element = ElementDescriptor::new("Observation.value[x]".to_string())
            .with_type(ElementType::String)
            .with_type(ElementType::Quantity);

        assert!(element.is_polymorphic());
    }

    #[test]
    fn test_to_column_descriptor() {
        let element = ElementDescriptor::new("Patient.active".to_string())
            .with_type(ElementType::Boolean)
            .required();

        let column = element.to_column_descriptor(None);

        assert_eq!(column.name, "active");
        assert_eq!(column.data_type, PostgresType::Boolean);
        assert!(!column.nullable);
        assert_eq!(column.fhir_path, Some("Patient.active".to_string()));
    }

    #[test]
    fn test_element_type_to_postgres() {
        assert_eq!(ElementType::Boolean.to_postgres_type(), PostgresType::Boolean);
        assert_eq!(ElementType::Integer.to_postgres_type(), PostgresType::Integer);
        assert_eq!(ElementType::String.to_postgres_type(), PostgresType::Text);
        assert_eq!(ElementType::Date.to_postgres_type(), PostgresType::Date);
        assert_eq!(
            ElementType::DateTime.to_postgres_type(),
            PostgresType::Timestamptz
        );
        assert_eq!(ElementType::Uuid.to_postgres_type(), PostgresType::Uuid);
        assert_eq!(
            ElementType::HumanName.to_postgres_type(),
            PostgresType::Jsonb
        );
        assert_eq!(
            ElementType::CodeableConcept.to_postgres_type(),
            PostgresType::Jsonb
        );
    }

    #[test]
    fn test_to_snake_case() {
        // Standard cases
        assert_eq!(to_snake_case("firstName"), "first_name");
        assert_eq!(to_snake_case("versionId"), "version_id");
        assert_eq!(to_snake_case("Id"), "id");
        assert_eq!(to_snake_case("active"), "active");

        // Acronyms concatenate - this is acceptable for DB column names
        // Users can always specify custom column names via ElementDescriptor
        assert_eq!(to_snake_case("HTTPEndpoint"), "httpendpoint");
        assert_eq!(to_snake_case("birthDate"), "birth_date");
    }

    #[test]
    fn test_extract_column_name() {
        let element = ElementDescriptor::new("Patient.name.family".to_string());
        assert_eq!(element.extract_column_name(), "family");

        let element2 = ElementDescriptor::new("Observation.value[x]".to_string());
        assert_eq!(element2.extract_column_name(), "value");
    }

    #[test]
    fn test_is_reference() {
        assert!(ElementType::Reference.is_reference());
        assert!(ElementType::Resource("Patient".to_string()).is_reference());
        assert!(!ElementType::String.is_reference());
        assert!(!ElementType::CodeableConcept.is_reference());
    }
}