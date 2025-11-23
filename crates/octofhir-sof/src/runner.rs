//! View execution against PostgreSQL.
//!
//! This module provides the ViewRunner which executes generated SQL
//! against a PostgreSQL database and returns structured results.

use serde_json::Value;
use sqlx_core::row::Row;
use sqlx_postgres::{PgPool, PgRow};

use crate::column::{ColumnInfo, ColumnType};
use crate::sql_generator::{GeneratedColumn, SqlGenerator};
use crate::view_definition::ViewDefinition;
use crate::{Error, Result};

/// Executes ViewDefinitions against a PostgreSQL database.
pub struct ViewRunner {
    pool: PgPool,
    generator: SqlGenerator,
}

impl ViewRunner {
    /// Create a new ViewRunner with the given connection pool.
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            generator: SqlGenerator::new(),
        }
    }

    /// Create a new ViewRunner with a custom SQL generator.
    pub fn with_generator(pool: PgPool, generator: SqlGenerator) -> Self {
        Self { pool, generator }
    }

    /// Execute a ViewDefinition and return the results.
    ///
    /// # Errors
    ///
    /// Returns an error if SQL generation fails or the query fails to execute.
    pub async fn run(&self, view: &ViewDefinition) -> Result<ViewResult> {
        let generated = self.generator.generate(view)?;

        tracing::debug!(sql = %generated.sql, "Executing view");

        let rows = sqlx_core::query::query(&generated.sql)
            .fetch_all(&self.pool)
            .await
            .map_err(Error::Sql)?;

        let columns: Vec<ColumnInfo> = generated
            .columns
            .iter()
            .map(|c| ColumnInfo::new(c.alias.clone(), c.col_type))
            .collect();

        let data: Vec<Vec<Value>> = rows
            .iter()
            .map(|row| extract_row_values(row, &generated.columns))
            .collect();

        Ok(ViewResult {
            columns,
            data,
            row_count: rows.len(),
        })
    }

    /// Execute a ViewDefinition and return only the generated SQL.
    ///
    /// This is useful for debugging or explaining the query without executing it.
    pub fn explain(&self, view: &ViewDefinition) -> Result<String> {
        let generated = self.generator.generate(view)?;
        Ok(generated.sql)
    }

    /// Get a reference to the underlying connection pool.
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }
}

/// Extract values from a row based on column definitions.
fn extract_row_values(row: &PgRow, columns: &[GeneratedColumn]) -> Vec<Value> {
    columns
        .iter()
        .enumerate()
        .map(|(i, col)| extract_column_value(row, i, &col.col_type))
        .collect()
}

/// Extract a single column value from a row.
fn extract_column_value(row: &PgRow, index: usize, col_type: &ColumnType) -> Value {
    match col_type {
        ColumnType::Integer => row
            .try_get::<Option<i64>, _>(index)
            .ok()
            .flatten()
            .map(|v| Value::Number(v.into()))
            .unwrap_or(Value::Null),

        ColumnType::Decimal => row
            .try_get::<Option<f64>, _>(index)
            .ok()
            .flatten()
            .and_then(|v| serde_json::Number::from_f64(v).map(Value::Number))
            .unwrap_or(Value::Null),

        ColumnType::Boolean => row
            .try_get::<Option<bool>, _>(index)
            .ok()
            .flatten()
            .map(Value::Bool)
            .unwrap_or(Value::Null),

        ColumnType::Json => row
            .try_get::<Option<Value>, _>(index)
            .ok()
            .flatten()
            .unwrap_or(Value::Null),

        // String-like types
        ColumnType::String
        | ColumnType::Date
        | ColumnType::DateTime
        | ColumnType::Instant
        | ColumnType::Time
        | ColumnType::Base64Binary => row
            .try_get::<Option<String>, _>(index)
            .ok()
            .flatten()
            .map(Value::String)
            .unwrap_or(Value::Null),
    }
}

/// Results from executing a ViewDefinition.
#[derive(Debug, Clone)]
pub struct ViewResult {
    /// Column metadata for the result set.
    pub columns: Vec<ColumnInfo>,

    /// Row data as JSON values.
    pub data: Vec<Vec<Value>>,

    /// Total number of rows returned.
    pub row_count: usize,
}

impl ViewResult {
    /// Check if the result set is empty.
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// Get the number of columns.
    pub fn column_count(&self) -> usize {
        self.columns.len()
    }

    /// Convert the result to a JSON array of objects.
    pub fn to_json_array(&self) -> Vec<Value> {
        self.data
            .iter()
            .map(|row| {
                let mut obj = serde_json::Map::new();
                for (i, col) in self.columns.iter().enumerate() {
                    if let Some(value) = row.get(i) {
                        obj.insert(col.name.clone(), value.clone());
                    }
                }
                Value::Object(obj)
            })
            .collect()
    }

    /// Get a single row by index as a JSON object.
    pub fn row_as_object(&self, index: usize) -> Option<Value> {
        self.data.get(index).map(|row| {
            let mut obj = serde_json::Map::new();
            for (i, col) in self.columns.iter().enumerate() {
                if let Some(value) = row.get(i) {
                    obj.insert(col.name.clone(), value.clone());
                }
            }
            Value::Object(obj)
        })
    }

    /// Get column values by name.
    pub fn column_values(&self, name: &str) -> Option<Vec<&Value>> {
        let col_index = self.columns.iter().position(|c| c.name == name)?;
        Some(
            self.data
                .iter()
                .filter_map(|row| row.get(col_index))
                .collect(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_view_result_to_json_array() {
        let result = ViewResult {
            columns: vec![
                ColumnInfo::new("id", ColumnType::String),
                ColumnInfo::new("name", ColumnType::String),
            ],
            data: vec![
                vec![json!("1"), json!("Alice")],
                vec![json!("2"), json!("Bob")],
            ],
            row_count: 2,
        };

        let json_array = result.to_json_array();
        assert_eq!(json_array.len(), 2);
        assert_eq!(json_array[0]["id"], "1");
        assert_eq!(json_array[0]["name"], "Alice");
        assert_eq!(json_array[1]["id"], "2");
        assert_eq!(json_array[1]["name"], "Bob");
    }

    #[test]
    fn test_view_result_row_as_object() {
        let result = ViewResult {
            columns: vec![
                ColumnInfo::new("id", ColumnType::String),
                ColumnInfo::new("active", ColumnType::Boolean),
            ],
            data: vec![vec![json!("123"), json!(true)]],
            row_count: 1,
        };

        let row = result.row_as_object(0).unwrap();
        assert_eq!(row["id"], "123");
        assert_eq!(row["active"], true);

        assert!(result.row_as_object(1).is_none());
    }

    #[test]
    fn test_view_result_column_values() {
        let result = ViewResult {
            columns: vec![
                ColumnInfo::new("id", ColumnType::String),
                ColumnInfo::new("value", ColumnType::Integer),
            ],
            data: vec![
                vec![json!("1"), json!(10)],
                vec![json!("2"), json!(20)],
                vec![json!("3"), json!(30)],
            ],
            row_count: 3,
        };

        let ids = result.column_values("id").unwrap();
        assert_eq!(ids.len(), 3);
        assert_eq!(*ids[0], json!("1"));

        let values = result.column_values("value").unwrap();
        assert_eq!(values.len(), 3);
        assert_eq!(*values[0], json!(10));

        assert!(result.column_values("nonexistent").is_none());
    }

    #[test]
    fn test_view_result_is_empty() {
        let empty_result = ViewResult {
            columns: vec![ColumnInfo::new("id", ColumnType::String)],
            data: vec![],
            row_count: 0,
        };
        assert!(empty_result.is_empty());

        let non_empty = ViewResult {
            columns: vec![ColumnInfo::new("id", ColumnType::String)],
            data: vec![vec![json!("1")]],
            row_count: 1,
        };
        assert!(!non_empty.is_empty());
    }
}
