//! ViewDefinition parsing and types.
//!
//! This module defines the data structures for parsing FHIR ViewDefinition resources
//! as specified in the SQL on FHIR Implementation Guide.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::Error;

/// A ViewDefinition resource that defines a tabular view over FHIR data.
///
/// ViewDefinitions specify how to transform FHIR resources into flat,
/// tabular data suitable for SQL queries and analytics.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewDefinition {
    /// The FHIR resource type (always "ViewDefinition").
    pub resource_type: String,

    /// Canonical URL identifying this ViewDefinition.
    pub url: Option<String>,

    /// Human-readable name for the view.
    pub name: String,

    /// Publication status: draft | active | retired | unknown.
    pub status: String,

    /// The FHIR resource type this view is based on (e.g., "Patient", "Observation").
    pub resource: String,

    /// Description of the view's purpose.
    pub description: Option<String>,

    /// The columns and nested selects to include in the view.
    #[serde(default)]
    pub select: Vec<SelectColumn>,

    /// Filter conditions to apply to the view.
    /// Note: Named `where_` because `where` is a Rust reserved keyword.
    #[serde(default, rename = "where")]
    pub where_: Vec<WhereClause>,

    /// Constants that can be referenced in FHIRPath expressions.
    #[serde(default)]
    pub constant: Vec<Constant>,
}

/// A select clause that defines columns or nested structures.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectColumn {
    /// FHIRPath expression to evaluate for this select.
    pub path: Option<String>,

    /// Alias for this select (used as table prefix for nested selects).
    pub alias: Option<String>,

    /// Whether this select represents a collection.
    #[serde(default)]
    pub collection: bool,

    /// Nested select clauses.
    #[serde(default)]
    pub select: Vec<SelectColumn>,

    /// Column definitions at this level.
    pub column: Option<Vec<Column>>,

    /// FHIRPath expression for array expansion (creates one row per element).
    pub for_each: Option<String>,

    /// Like forEach, but includes a row with nulls if the array is empty.
    pub for_each_or_null: Option<String>,

    /// Union of multiple select clauses.
    pub union_all: Option<Vec<SelectColumn>>,
}

/// A column definition in a ViewDefinition.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Column {
    /// The column name in the output.
    pub name: String,

    /// FHIRPath expression to extract the column value.
    pub path: String,

    /// Expected data type of the column.
    #[serde(rename = "type")]
    pub col_type: Option<String>,

    /// Whether this column can contain multiple values.
    pub collection: Option<bool>,

    /// Human-readable description of the column.
    pub description: Option<String>,
}

/// A where clause for filtering rows.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WhereClause {
    /// FHIRPath expression that must evaluate to true for the row to be included.
    pub path: String,
}

/// A constant value that can be referenced in FHIRPath expressions.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Constant {
    /// Name of the constant (referenced as %name in FHIRPath).
    pub name: String,

    /// String value of the constant.
    pub value_string: Option<String>,

    /// Integer value of the constant.
    pub value_integer: Option<i64>,

    /// Boolean value of the constant.
    pub value_boolean: Option<bool>,

    /// Decimal value of the constant.
    pub value_decimal: Option<f64>,
}

impl ViewDefinition {
    /// Parse a ViewDefinition from a JSON Value.
    ///
    /// # Errors
    ///
    /// Returns an error if the JSON is not a valid ViewDefinition.
    pub fn from_json(value: &Value) -> Result<Self, Error> {
        serde_json::from_value(value.clone())
            .map_err(|e| Error::InvalidViewDefinition(e.to_string()))
    }

    /// Parse a ViewDefinition from a JSON string.
    ///
    /// # Errors
    ///
    /// Returns an error if the string is not valid JSON or not a valid ViewDefinition.
    pub fn parse(s: &str) -> Result<Self, Error> {
        serde_json::from_str(s).map_err(|e| Error::InvalidViewDefinition(e.to_string()))
    }

    /// Get the list of all column names defined in this view.
    pub fn column_names(&self) -> Vec<String> {
        let mut names = Vec::new();
        collect_column_names(&self.select, &mut names);
        names
    }
}

/// Recursively collect column names from select clauses.
fn collect_column_names(selects: &[SelectColumn], names: &mut Vec<String>) {
    for select in selects {
        if let Some(columns) = &select.column {
            for col in columns {
                names.push(col.name.clone());
            }
        }
        collect_column_names(&select.select, names);

        if let Some(union_selects) = &select.union_all {
            collect_column_names(union_selects, names);
        }
    }
}

impl Constant {
    /// Get the value of this constant as a serde_json::Value.
    pub fn value(&self) -> Value {
        if let Some(s) = &self.value_string {
            Value::String(s.clone())
        } else if let Some(i) = self.value_integer {
            Value::Number(i.into())
        } else if let Some(b) = self.value_boolean {
            Value::Bool(b)
        } else if let Some(d) = self.value_decimal {
            serde_json::Number::from_f64(d)
                .map(Value::Number)
                .unwrap_or(Value::Null)
        } else {
            Value::Null
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_parse_simple_view_definition() {
        let json = json!({
            "resourceType": "ViewDefinition",
            "name": "patient_demographics",
            "status": "active",
            "resource": "Patient",
            "select": [{
                "column": [{
                    "name": "id",
                    "path": "id"
                }, {
                    "name": "gender",
                    "path": "gender"
                }]
            }]
        });

        let view = ViewDefinition::from_json(&json).unwrap();
        assert_eq!(view.name, "patient_demographics");
        assert_eq!(view.resource, "Patient");
        assert_eq!(view.select.len(), 1);

        let columns = view.select[0].column.as_ref().unwrap();
        assert_eq!(columns.len(), 2);
        assert_eq!(columns[0].name, "id");
        assert_eq!(columns[1].name, "gender");
    }

    #[test]
    fn test_parse_view_with_foreach() {
        let json = json!({
            "resourceType": "ViewDefinition",
            "name": "patient_names",
            "status": "active",
            "resource": "Patient",
            "select": [{
                "forEach": "name",
                "column": [{
                    "name": "family",
                    "path": "family"
                }, {
                    "name": "given",
                    "path": "given.first()"
                }]
            }]
        });

        let view = ViewDefinition::from_json(&json).unwrap();
        assert_eq!(view.select[0].for_each, Some("name".to_string()));
    }

    #[test]
    fn test_parse_view_with_where() {
        let json = json!({
            "resourceType": "ViewDefinition",
            "name": "active_patients",
            "status": "active",
            "resource": "Patient",
            "select": [{
                "column": [{
                    "name": "id",
                    "path": "id"
                }]
            }],
            "where": [{
                "path": "active = true"
            }]
        });

        let view = ViewDefinition::from_json(&json).unwrap();
        assert_eq!(view.where_.len(), 1);
        assert_eq!(view.where_[0].path, "active = true");
    }

    #[test]
    fn test_parse_view_with_constants() {
        let json = json!({
            "resourceType": "ViewDefinition",
            "name": "test_view",
            "status": "active",
            "resource": "Patient",
            "constant": [{
                "name": "statusFilter",
                "valueString": "active"
            }, {
                "name": "maxAge",
                "valueInteger": 65
            }],
            "select": [{
                "column": [{
                    "name": "id",
                    "path": "id"
                }]
            }]
        });

        let view = ViewDefinition::from_json(&json).unwrap();
        assert_eq!(view.constant.len(), 2);
        assert_eq!(view.constant[0].name, "statusFilter");
        assert_eq!(view.constant[0].value_string, Some("active".to_string()));
        assert_eq!(view.constant[1].value_integer, Some(65));
    }

    #[test]
    fn test_column_names() {
        let json = json!({
            "resourceType": "ViewDefinition",
            "name": "test_view",
            "status": "active",
            "resource": "Patient",
            "select": [{
                "column": [{
                    "name": "id",
                    "path": "id"
                }, {
                    "name": "gender",
                    "path": "gender"
                }]
            }, {
                "forEach": "name",
                "column": [{
                    "name": "family",
                    "path": "family"
                }]
            }]
        });

        let view = ViewDefinition::from_json(&json).unwrap();
        let names = view.column_names();
        assert_eq!(names, vec!["id", "gender", "family"]);
    }

    #[test]
    fn test_constant_values() {
        let string_const = Constant {
            name: "s".to_string(),
            value_string: Some("test".to_string()),
            value_integer: None,
            value_boolean: None,
            value_decimal: None,
        };
        assert_eq!(string_const.value(), Value::String("test".to_string()));

        let int_const = Constant {
            name: "i".to_string(),
            value_string: None,
            value_integer: Some(42),
            value_boolean: None,
            value_decimal: None,
        };
        assert_eq!(int_const.value(), json!(42));

        let bool_const = Constant {
            name: "b".to_string(),
            value_string: None,
            value_integer: None,
            value_boolean: Some(true),
            value_decimal: None,
        };
        assert_eq!(bool_const.value(), Value::Bool(true));
    }
}
