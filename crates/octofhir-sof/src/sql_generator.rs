//! SQL generation from ViewDefinitions.
//!
//! This module converts ViewDefinition resources into PostgreSQL queries
//! that can be executed against FHIR data stored in JSONB format.

use crate::column::ColumnType;
use crate::view_definition::{SelectColumn, ViewDefinition};
use crate::Error;

/// Generates SQL queries from ViewDefinitions.
pub struct SqlGenerator {
    /// The base table name pattern (e.g., "resource" for resource.data).
    table_pattern: String,
}

impl Default for SqlGenerator {
    fn default() -> Self {
        Self::new()
    }
}

impl SqlGenerator {
    /// Create a new SQL generator with default settings.
    pub fn new() -> Self {
        Self {
            table_pattern: "base".to_string(),
        }
    }

    /// Create a new SQL generator with a custom table pattern.
    pub fn with_table_pattern(table_pattern: impl Into<String>) -> Self {
        Self {
            table_pattern: table_pattern.into(),
        }
    }

    /// Generate SQL from a ViewDefinition.
    ///
    /// # Errors
    ///
    /// Returns an error if the ViewDefinition contains invalid paths or
    /// cannot be converted to SQL.
    pub fn generate(&self, view: &ViewDefinition) -> Result<GeneratedSql, Error> {
        let table = view.resource.to_lowercase();
        let mut columns = Vec::new();
        let mut joins = Vec::new();
        let mut where_clauses = Vec::new();
        let mut join_counter = 0;

        // Process select columns
        for select in &view.select {
            self.process_select(
                &mut columns,
                &mut joins,
                &mut join_counter,
                select,
                &self.table_pattern,
                "",
            )?;
        }

        // Process where clauses
        for where_clause in &view.where_ {
            let sql = self.fhirpath_to_sql(&where_clause.path, &self.table_pattern)?;
            where_clauses.push(sql);
        }

        // Build final SQL
        let column_sql: String = if columns.is_empty() {
            "*".to_string()
        } else {
            columns
                .iter()
                .map(|c| format!("{} AS \"{}\"", c.expression, c.alias))
                .collect::<Vec<_>>()
                .join(", ")
        };

        let mut sql = format!(
            "SELECT {} FROM {} {}",
            column_sql, table, self.table_pattern
        );

        for join in &joins {
            sql.push_str(&format!(" {}", join));
        }

        // Add base where clause for non-deleted resources
        sql.push_str(&format!(
            " WHERE {}.status != 'Deleted'",
            self.table_pattern
        ));

        // Add user-defined where clauses
        for clause in &where_clauses {
            sql.push_str(&format!(" AND ({})", clause));
        }

        Ok(GeneratedSql { sql, columns })
    }

    /// Process a single select clause and add columns/joins.
    fn process_select(
        &self,
        columns: &mut Vec<GeneratedColumn>,
        joins: &mut Vec<String>,
        join_counter: &mut usize,
        select: &SelectColumn,
        table_alias: &str,
        prefix: &str,
    ) -> Result<(), Error> {
        // Handle forEach (array expansion)
        if let Some(for_each) = &select.for_each {
            let alias = format!("fe_{}", join_counter);
            *join_counter += 1;

            let path_sql = self.fhirpath_to_jsonb_array_path(for_each)?;

            joins.push(format!(
                "CROSS JOIN LATERAL jsonb_array_elements({}.resource->{}) AS {}(elem)",
                table_alias, path_sql, alias
            ));

            // Process columns with the new context
            if let Some(cols) = &select.column {
                for col in cols {
                    let expression = self.fhirpath_to_sql_in_context(&col.path, &alias, "elem")?;
                    let alias_name = self.make_column_alias(&col.name, prefix);
                    let col_type = col
                        .col_type
                        .as_ref()
                        .map(|t| ColumnType::from_fhir_type(t))
                        .unwrap_or(ColumnType::String);

                    columns.push(GeneratedColumn {
                        name: col.name.clone(),
                        expression,
                        alias: alias_name,
                        col_type,
                    });
                }
            }

            // Process nested selects with new context
            for nested in &select.select {
                self.process_select(columns, joins, join_counter, nested, &alias, prefix)?;
            }

            return Ok(());
        }

        // Handle forEachOrNull (array expansion with null row for empty arrays)
        if let Some(for_each) = &select.for_each_or_null {
            let alias = format!("feon_{}", join_counter);
            *join_counter += 1;

            let path_sql = self.fhirpath_to_jsonb_array_path(for_each)?;

            joins.push(format!(
                "LEFT JOIN LATERAL jsonb_array_elements({}.resource->{}) AS {}(elem) ON true",
                table_alias, path_sql, alias
            ));

            // Process columns with the new context
            if let Some(cols) = &select.column {
                for col in cols {
                    let expression = self.fhirpath_to_sql_in_context(&col.path, &alias, "elem")?;
                    let alias_name = self.make_column_alias(&col.name, prefix);
                    let col_type = col
                        .col_type
                        .as_ref()
                        .map(|t| ColumnType::from_fhir_type(t))
                        .unwrap_or(ColumnType::String);

                    columns.push(GeneratedColumn {
                        name: col.name.clone(),
                        expression,
                        alias: alias_name,
                        col_type,
                    });
                }
            }

            // Process nested selects
            for nested in &select.select {
                self.process_select(columns, joins, join_counter, nested, &alias, prefix)?;
            }

            return Ok(());
        }

        // Handle direct columns
        if let Some(cols) = &select.column {
            for col in cols {
                let expression = self.fhirpath_to_sql(&col.path, table_alias)?;
                let alias_name = self.make_column_alias(&col.name, prefix);
                let col_type = col
                    .col_type
                    .as_ref()
                    .map(|t| ColumnType::from_fhir_type(t))
                    .unwrap_or(ColumnType::String);

                columns.push(GeneratedColumn {
                    name: col.name.clone(),
                    expression,
                    alias: alias_name,
                    col_type,
                });
            }
        }

        // Handle nested selects
        let new_prefix = if let Some(alias) = &select.alias {
            if prefix.is_empty() {
                alias.clone()
            } else {
                format!("{}_{}", prefix, alias)
            }
        } else {
            prefix.to_string()
        };

        for nested in &select.select {
            self.process_select(
                columns,
                joins,
                join_counter,
                nested,
                table_alias,
                &new_prefix,
            )?;
        }

        // Handle unionAll
        if let Some(union_selects) = &select.union_all {
            for union_select in union_selects {
                self.process_select(
                    columns,
                    joins,
                    join_counter,
                    union_select,
                    table_alias,
                    prefix,
                )?;
            }
        }

        Ok(())
    }

    /// Convert a FHIRPath expression to SQL for the base resource.
    fn fhirpath_to_sql(&self, path: &str, table_alias: &str) -> Result<String, Error> {
        if path.is_empty() {
            return Err(Error::InvalidPath("Empty path".to_string()));
        }

        // Handle special cases
        if path == "id" {
            return Ok(format!("{}.id", table_alias));
        }

        // Handle getResourceKey() function
        if path == "getResourceKey()" {
            return Ok(format!(
                "{}.resource_type || '/' || {}.id",
                table_alias, table_alias
            ));
        }

        // Parse path into parts
        let parts = self.parse_fhirpath(path)?;

        if parts.is_empty() {
            return Ok(format!("{}.resource", table_alias));
        }

        // Build the JSON path expression
        let mut sql = format!("{}.resource", table_alias);

        for (i, part) in parts.iter().enumerate() {
            if i == parts.len() - 1 {
                // Last element - use ->> for text extraction
                sql = format!("{}->>'{}'", sql, part);
            } else {
                // Intermediate element - use -> for JSON traversal
                sql = format!("{}->'{}'", sql, part);
            }
        }

        Ok(sql)
    }

    /// Convert a FHIRPath expression to SQL within a forEach context.
    fn fhirpath_to_sql_in_context(
        &self,
        path: &str,
        _table_alias: &str,
        elem_alias: &str,
    ) -> Result<String, Error> {
        if path.is_empty() {
            return Ok(elem_alias.to_string());
        }

        // Parse path into parts
        let parts = self.parse_fhirpath(path)?;

        if parts.is_empty() {
            return Ok(elem_alias.to_string());
        }

        // Build the JSON path expression starting from the element
        let mut sql = elem_alias.to_string();

        for (i, part) in parts.iter().enumerate() {
            if i == parts.len() - 1 {
                // Last element - use ->> for text extraction
                sql = format!("{}->>'{}'", sql, part);
            } else {
                // Intermediate element - use -> for JSON traversal
                sql = format!("{}->'{}'", sql, part);
            }
        }

        Ok(sql)
    }

    /// Convert a FHIRPath expression to a JSONB path for array access.
    fn fhirpath_to_jsonb_array_path(&self, path: &str) -> Result<String, Error> {
        let parts = self.parse_fhirpath(path)?;

        if parts.is_empty() {
            return Err(Error::InvalidPath(format!("Empty forEach path: {}", path)));
        }

        // Build path as a chain of -> operators
        let path_sql = parts
            .iter()
            .map(|p| format!("'{}'", p))
            .collect::<Vec<_>>()
            .join("->");

        Ok(path_sql)
    }

    /// Parse a FHIRPath expression into path segments.
    fn parse_fhirpath(&self, path: &str) -> Result<Vec<String>, Error> {
        let mut parts = Vec::new();

        // Remove any leading resource type (e.g., "Patient.name" -> "name")
        let path = if let Some(dot_pos) = path.find('.') {
            let first_part = &path[..dot_pos];
            // Check if the first part is a resource type (starts with uppercase)
            if first_part
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_uppercase())
            {
                &path[dot_pos + 1..]
            } else {
                path
            }
        } else {
            // Single element - check if it's a resource type
            if path.chars().next().is_some_and(|c| c.is_ascii_uppercase()) && !path.contains('(') {
                return Ok(vec![]);
            }
            path
        };

        // Split by '.' but handle function calls and array indexing
        let mut current = String::new();
        let mut paren_depth = 0;
        let mut bracket_depth = 0;

        for c in path.chars() {
            match c {
                '(' => {
                    paren_depth += 1;
                    current.push(c);
                }
                ')' => {
                    paren_depth -= 1;
                    current.push(c);
                }
                '[' => {
                    bracket_depth += 1;
                    current.push(c);
                }
                ']' => {
                    bracket_depth -= 1;
                    current.push(c);
                }
                '.' if paren_depth == 0 && bracket_depth == 0 => {
                    if !current.is_empty() {
                        // Handle function calls - skip them for now
                        if !current.contains('(') {
                            parts.push(current.clone());
                        }
                        current.clear();
                    }
                }
                _ => {
                    current.push(c);
                }
            }
        }

        if !current.is_empty() && !current.contains('(') {
            parts.push(current);
        }

        Ok(parts)
    }

    /// Create a column alias with optional prefix.
    fn make_column_alias(&self, name: &str, prefix: &str) -> String {
        if prefix.is_empty() {
            name.to_string()
        } else {
            format!("{}_{}", prefix, name)
        }
    }
}

/// Generated SQL with column metadata.
#[derive(Debug, Clone)]
pub struct GeneratedSql {
    /// The generated SQL query.
    pub sql: String,

    /// Column information for the result set.
    pub columns: Vec<GeneratedColumn>,
}

/// A generated column with its SQL expression and metadata.
#[derive(Debug, Clone)]
pub struct GeneratedColumn {
    /// Original column name from the ViewDefinition.
    pub name: String,

    /// SQL expression that produces this column's value.
    pub expression: String,

    /// Alias used in the SQL SELECT clause.
    pub alias: String,

    /// Data type of the column.
    pub col_type: ColumnType,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn create_test_view(json: serde_json::Value) -> ViewDefinition {
        ViewDefinition::from_json(&json).unwrap()
    }

    #[test]
    fn test_generate_simple_sql() {
        let view = create_test_view(json!({
            "resourceType": "ViewDefinition",
            "name": "patient_demo",
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
        }));

        let generator = SqlGenerator::new();
        let result = generator.generate(&view).unwrap();

        assert!(result.sql.contains("SELECT"));
        assert!(result.sql.contains("FROM patient"));
        assert!(result.sql.contains("base.id"));
        assert!(result.sql.contains("gender"));
        assert_eq!(result.columns.len(), 2);
    }

    #[test]
    fn test_generate_sql_with_nested_path() {
        let view = create_test_view(json!({
            "resourceType": "ViewDefinition",
            "name": "patient_name",
            "status": "active",
            "resource": "Patient",
            "select": [{
                "column": [{
                    "name": "family",
                    "path": "name.family"
                }]
            }]
        }));

        let generator = SqlGenerator::new();
        let result = generator.generate(&view).unwrap();

        // Should have nested JSON access
        assert!(result.sql.contains("resource->'name'->>'family'"));
    }

    #[test]
    fn test_generate_sql_with_foreach() {
        let view = create_test_view(json!({
            "resourceType": "ViewDefinition",
            "name": "patient_names",
            "status": "active",
            "resource": "Patient",
            "select": [{
                "forEach": "name",
                "column": [{
                    "name": "family",
                    "path": "family"
                }]
            }]
        }));

        let generator = SqlGenerator::new();
        let result = generator.generate(&view).unwrap();

        // Should have LATERAL join for array expansion
        assert!(result.sql.contains("CROSS JOIN LATERAL"));
        assert!(result.sql.contains("jsonb_array_elements"));
    }

    #[test]
    fn test_generate_sql_with_where() {
        let view = create_test_view(json!({
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
                "path": "active"
            }]
        }));

        let generator = SqlGenerator::new();
        let result = generator.generate(&view).unwrap();

        // Should have WHERE clause
        assert!(result.sql.contains("WHERE"));
        assert!(result.sql.contains("active"));
    }

    #[test]
    fn test_parse_fhirpath_simple() {
        let generator = SqlGenerator::new();

        let parts = generator.parse_fhirpath("name").unwrap();
        assert_eq!(parts, vec!["name"]);

        let parts = generator.parse_fhirpath("name.family").unwrap();
        assert_eq!(parts, vec!["name", "family"]);

        let parts = generator.parse_fhirpath("Patient.name.family").unwrap();
        assert_eq!(parts, vec!["name", "family"]);
    }

    #[test]
    fn test_fhirpath_to_sql() {
        let generator = SqlGenerator::new();

        let sql = generator.fhirpath_to_sql("id", "base").unwrap();
        assert_eq!(sql, "base.id");

        let sql = generator.fhirpath_to_sql("gender", "base").unwrap();
        assert_eq!(sql, "base.resource->>'gender'");

        let sql = generator.fhirpath_to_sql("name.family", "base").unwrap();
        assert_eq!(sql, "base.resource->'name'->>'family'");
    }

    #[test]
    fn test_column_types() {
        let view = create_test_view(json!({
            "resourceType": "ViewDefinition",
            "name": "typed_view",
            "status": "active",
            "resource": "Patient",
            "select": [{
                "column": [{
                    "name": "birth_date",
                    "path": "birthDate",
                    "type": "date"
                }, {
                    "name": "active",
                    "path": "active",
                    "type": "boolean"
                }]
            }]
        }));

        let generator = SqlGenerator::new();
        let result = generator.generate(&view).unwrap();

        assert_eq!(result.columns[0].col_type, ColumnType::Date);
        assert_eq!(result.columns[1].col_type, ColumnType::Boolean);
    }
}
