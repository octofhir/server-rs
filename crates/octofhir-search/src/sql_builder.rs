//! SQL Builder for generating PostgreSQL JSONB queries.
//!
//! This module provides a builder pattern for constructing parameterized
//! SQL queries that search within JSONB resource data.

use thiserror::Error;

/// Errors that can occur during SQL building.
#[derive(Debug, Error)]
pub enum SqlBuilderError {
    #[error("Invalid modifier '{0}' for parameter type")]
    InvalidModifier(String),

    #[error("Invalid search value: {0}")]
    InvalidSearchValue(String),

    #[error("Feature not implemented: {0}")]
    NotImplemented(String),

    #[error("Invalid JSON path: {0}")]
    InvalidPath(String),
}

/// A builder for constructing SQL WHERE clauses with parameterized values.
///
/// The builder accumulates conditions and parameters, then generates
/// a complete WHERE clause with numbered parameter placeholders ($1, $2, etc.).
#[derive(Debug, Default)]
pub struct SqlBuilder {
    conditions: Vec<String>,
    params: Vec<SqlParam>,
    resource_col: String,
    param_offset: usize,
}

/// A SQL parameter value with its type information.
#[derive(Debug, Clone)]
pub enum SqlParam {
    Text(String),
    Integer(i64),
    Float(f64),
    Boolean(bool),
    Json(String),
    Timestamp(String),
}

impl SqlParam {
    /// Get the value as a string for binding.
    pub fn as_str(&self) -> String {
        match self {
            Self::Text(s) | Self::Json(s) | Self::Timestamp(s) => s.clone(),
            Self::Integer(i) => i.to_string(),
            Self::Float(f) => f.to_string(),
            Self::Boolean(b) => b.to_string(),
        }
    }
}

impl SqlBuilder {
    /// Create a new SQL builder with the default resource column name.
    pub fn new() -> Self {
        Self {
            conditions: Vec::new(),
            params: Vec::new(),
            resource_col: "resource".to_string(),
            param_offset: 0,
        }
    }

    /// Create a new SQL builder with a custom resource column name.
    pub fn with_resource_column(column: impl Into<String>) -> Self {
        Self {
            conditions: Vec::new(),
            params: Vec::new(),
            resource_col: column.into(),
            param_offset: 0,
        }
    }

    /// Set the parameter offset for numbering (useful when combining with other queries).
    pub fn with_param_offset(mut self, offset: usize) -> Self {
        self.param_offset = offset;
        self
    }

    /// Get the resource column name.
    pub fn resource_column(&self) -> &str {
        &self.resource_col
    }

    /// Add a raw SQL condition with placeholders.
    ///
    /// Use `{}` as placeholder which will be replaced with `$N` parameter references.
    pub fn add_condition(&mut self, condition: impl Into<String>) {
        self.conditions.push(condition.into());
    }

    /// Add a text parameter and return its placeholder number.
    pub fn add_text_param(&mut self, value: impl Into<String>) -> usize {
        self.params.push(SqlParam::Text(value.into()));
        self.param_offset + self.params.len()
    }

    /// Add a JSON parameter and return its placeholder number.
    pub fn add_json_param(&mut self, value: impl Into<String>) -> usize {
        self.params.push(SqlParam::Json(value.into()));
        self.param_offset + self.params.len()
    }

    /// Add a float parameter and return its placeholder number.
    pub fn add_float_param(&mut self, value: f64) -> usize {
        self.params.push(SqlParam::Float(value));
        self.param_offset + self.params.len()
    }

    /// Add a timestamp parameter and return its placeholder number.
    pub fn add_timestamp_param(&mut self, value: impl Into<String>) -> usize {
        self.params.push(SqlParam::Timestamp(value.into()));
        self.param_offset + self.params.len()
    }

    /// Get the current parameter count.
    pub fn param_count(&self) -> usize {
        self.params.len()
    }

    /// Get all parameters.
    pub fn params(&self) -> &[SqlParam] {
        &self.params
    }

    /// Get all conditions.
    pub fn conditions(&self) -> &[String] {
        &self.conditions
    }

    /// Build the final WHERE clause by joining conditions with AND.
    ///
    /// Returns `None` if there are no conditions.
    pub fn build_where_clause(&self) -> Option<String> {
        if self.conditions.is_empty() {
            return None;
        }

        let clause = self.conditions.join(" AND ");
        Some(clause)
    }

    /// Build a WHERE clause for OR conditions within a group.
    ///
    /// This is useful for comma-separated values (OR semantics within same parameter).
    pub fn build_or_clause(conditions: &[String]) -> String {
        if conditions.len() == 1 {
            conditions[0].clone()
        } else {
            format!("({})", conditions.join(" OR "))
        }
    }
}

/// Helper to convert a FHIRPath expression to a JSONB path.
///
/// This is a simplified conversion that handles common patterns.
/// More complex paths may need the full FHIRPath engine.
pub fn fhirpath_to_jsonb_path(expression: &str, resource_type: &str) -> Vec<String> {
    // Remove resource type prefix if present
    let expr = expression
        .strip_prefix(&format!("{resource_type}."))
        .or_else(|| expression.strip_prefix("Resource."))
        .or_else(|| expression.strip_prefix("DomainResource."))
        .unwrap_or(expression);

    // Split by '.' and handle special cases
    expr.split('.')
        .filter(|s| !s.is_empty())
        .map(|s| {
            // Handle array access like "name[0]"
            if let Some(base) = s.strip_suffix(']')
                && let Some((name, _idx)) = base.split_once('[')
            {
                return name.to_string();
            }
            s.to_string()
        })
        .collect()
}

/// Build a JSONB accessor chain from path segments.
///
/// For example: `["name", "family"]` becomes `resource->'name'->'family'`
pub fn build_jsonb_accessor(resource_col: &str, path: &[String], as_text: bool) -> String {
    if path.is_empty() {
        return resource_col.to_string();
    }

    let mut result = resource_col.to_string();

    for (i, segment) in path.iter().enumerate() {
        if i == path.len() - 1 && as_text {
            // Last segment: use ->> for text extraction
            result = format!("{result}->>'{segment}'");
        } else {
            // Intermediate segments: use -> for JSON traversal
            result = format!("{result}->'{segment}'");
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fhirpath_to_jsonb_path() {
        let path = fhirpath_to_jsonb_path("Patient.name.family", "Patient");
        assert_eq!(path, vec!["name", "family"]);

        let path = fhirpath_to_jsonb_path("Resource.id", "Patient");
        assert_eq!(path, vec!["id"]);

        let path = fhirpath_to_jsonb_path("Patient.identifier", "Patient");
        assert_eq!(path, vec!["identifier"]);
    }

    #[test]
    fn test_build_jsonb_accessor() {
        let path = vec!["name".to_string(), "family".to_string()];

        let accessor = build_jsonb_accessor("resource", &path, true);
        assert_eq!(accessor, "resource->'name'->>'family'");

        let accessor = build_jsonb_accessor("resource", &path, false);
        assert_eq!(accessor, "resource->'name'->'family'");

        let accessor = build_jsonb_accessor("resource", &[], true);
        assert_eq!(accessor, "resource");
    }

    #[test]
    fn test_sql_builder_basic() {
        let mut builder = SqlBuilder::new();

        let p1 = builder.add_text_param("John");
        builder.add_condition(format!(
            "LOWER({}->>'name') LIKE LOWER(${})",
            builder.resource_column(),
            p1
        ));

        assert_eq!(builder.param_count(), 1);
        let clause = builder.build_where_clause();
        assert!(clause.is_some());
        assert_eq!(clause.unwrap(), "LOWER(resource->>'name') LIKE LOWER($1)");
    }

    #[test]
    fn test_sql_builder_multiple_conditions() {
        let mut builder = SqlBuilder::new();

        let p1 = builder.add_text_param("John%");
        builder.add_condition(format!("LOWER(resource->>'name') LIKE LOWER(${})", p1));

        let p2 = builder.add_text_param("active");
        builder.add_condition(format!("resource->>'status' = ${}", p2));

        let clause = builder.build_where_clause().unwrap();
        assert!(clause.contains("AND"));
        assert!(clause.contains("$1"));
        assert!(clause.contains("$2"));
    }

    #[test]
    fn test_sql_builder_or_clause() {
        let conditions = vec![
            "resource->>'status' = 'active'".to_string(),
            "resource->>'status' = 'completed'".to_string(),
        ];

        let or_clause = SqlBuilder::build_or_clause(&conditions);
        assert_eq!(
            or_clause,
            "(resource->>'status' = 'active' OR resource->>'status' = 'completed')"
        );
    }

    #[test]
    fn test_sql_builder_empty() {
        let builder = SqlBuilder::new();
        assert!(builder.build_where_clause().is_none());
    }
}
