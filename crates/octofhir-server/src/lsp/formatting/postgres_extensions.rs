//! PostgreSQL-specific formatting extensions
//!
//! Handles PostgreSQL-specific syntax that requires special formatting:
//! - JSONB operators (->, ->>, #>, #>>, @>, <@, ?, ?|, ?&, etc.)
//! - Window functions (OVER, PARTITION BY, ORDER BY within windows)
//! - Common Table Expressions (CTEs with WITH clauses)
//! - Array operators (ARRAY[...], ANY(), ALL(), &&, etc.)
//!
//! All formatting follows sqlstyle.guide principles with PostgreSQL adaptations.

use super::formatter::FormatResult;
use regex::Regex;

/// PostgreSQL extension formatter for specialized syntax
pub struct PostgresExtensionFormatter;

impl PostgresExtensionFormatter {
    /// Create a new PostgreSQL extension formatter
    pub fn new() -> Self {
        Self
    }

    /// Apply all PostgreSQL-specific formatting transformations
    pub fn format(&self, sql: &str) -> FormatResult<String> {
        let mut result = sql.to_string();

        // Apply each transformation in sequence
        result = self.format_jsonb_operators(&result)?;
        result = self.format_window_functions(&result)?;
        result = self.format_ctes(&result)?;
        result = self.format_array_operators(&result)?;

        Ok(result)
    }

    /// Format JSONB operators with consistent spacing
    ///
    /// Examples:
    /// - `resource->'name'` → `resource -> 'name'`
    /// - `resource->>'given'` → `resource ->> 'given'`
    /// - `data#>'{a,b}'` → `data #> '{a,b}'`
    /// - `data@>'{}'` → `data @> '{}'`
    fn format_jsonb_operators(&self, sql: &str) -> FormatResult<String> {
        let mut result = sql.to_string();

        // Two-character operators: ->, ->>, #>, #>>, @>, <@, ?|, ?&, ||
        let two_char_ops = vec![
            ("->", " -> "),
            ("->>", " ->> "),
            ("#>", " #> "),
            ("#>>", " #>> "),
            ("@>", " @> "),
            ("<@", " <@ "),
            ("?|", " ?| "),
            ("?&", " ?& "),
            // Note: || is also string concatenation, so we handle it too
            ("||", " || "),
        ];

        for (op, replacement) in two_char_ops {
            // Replace operator with spaced version, but avoid double-spacing
            // Pattern: word-char or ) or ] or ' followed by operator followed by word-char or ( or [ or '
            let pattern = format!(r"(\w|\)|\]|'){}(\w|\(|\[|')", regex::escape(op));
            let re = Regex::new(&pattern).map_err(|e| {
                super::formatter::FormatError::FormatError(format!("Regex error: {}", e))
            })?;

            result = re
                .replace_all(&result, |caps: &regex::Captures| {
                    format!("{}{}{}", &caps[1], replacement, &caps[2])
                })
                .to_string();
        }

        // Single-character operator: ?
        // Pattern: word-char or ) or ] followed by ? followed by word-char or ( or [ or '
        let pattern = r"(\w|\)|\])\?(\w|\(|\[|')";
        let re = Regex::new(pattern).map_err(|e| {
            super::formatter::FormatError::FormatError(format!("Regex error: {}", e))
        })?;

        result = re
            .replace_all(&result, |caps: &regex::Captures| {
                format!("{} ? {}", &caps[1], &caps[2])
            })
            .to_string();

        Ok(result)
    }

    /// Format window functions with proper indentation and line breaks
    ///
    /// Example:
    /// ```sql
    /// ROW_NUMBER() OVER (
    ///     PARTITION BY status
    ///     ORDER BY created_at
    /// )
    /// ```
    fn format_window_functions(&self, sql: &str) -> FormatResult<String> {
        // For now, we rely on pg_query's deparsing to handle window functions
        // Future enhancement: Custom formatting for OVER clauses
        Ok(sql.to_string())
    }

    /// Format CTEs (Common Table Expressions) with WITH clauses
    ///
    /// Example:
    /// ```sql
    /// WITH cte AS (
    ///     SELECT * FROM table
    /// )
    /// SELECT * FROM cte
    /// ```
    fn format_ctes(&self, sql: &str) -> FormatResult<String> {
        // For now, we rely on pg_query's deparsing to handle CTEs
        // Future enhancement: Custom CTE formatting with proper indentation
        Ok(sql.to_string())
    }

    /// Format array operators and constructors
    ///
    /// Examples:
    /// - `ARRAY[1,2,3]` → `ARRAY[1, 2, 3]`
    /// - `arr&&other` → `arr && other`
    /// - `val=ANY(arr)` → `val = ANY(arr)`
    fn format_array_operators(&self, sql: &str) -> FormatResult<String> {
        let mut result = sql.to_string();

        // Format && operator (array overlap)
        let pattern = r"(\w|\)|\])&&(\w|\(|\[)";
        let re = Regex::new(pattern).map_err(|e| {
            super::formatter::FormatError::FormatError(format!("Regex error: {}", e))
        })?;

        result = re
            .replace_all(&result, |caps: &regex::Captures| {
                format!("{} && {}", &caps[1], &caps[2])
            })
            .to_string();

        // Format commas in ARRAY[] constructors: ARRAY[1,2,3] → ARRAY[1, 2, 3]
        // This is a simple heuristic - look for comma without space after it inside brackets
        let pattern = r"(\[)([^]]+)(\])";
        let re = Regex::new(pattern).map_err(|e| {
            super::formatter::FormatError::FormatError(format!("Regex error: {}", e))
        })?;

        result = re
            .replace_all(&result, |caps: &regex::Captures| {
                let inner = &caps[2];
                let formatted_inner = inner.replace(",", ", ").replace(",  ", ", ");
                format!("{}{}{}", &caps[1], formatted_inner, &caps[3])
            })
            .to_string();

        Ok(result)
    }
}

impl Default for PostgresExtensionFormatter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jsonb_arrow_operator() {
        let formatter = PostgresExtensionFormatter::new();
        let sql = "resource->'name'";
        let result = formatter.format_jsonb_operators(sql).unwrap();
        assert!(result.contains(" -> "));
    }

    #[test]
    fn test_jsonb_double_arrow_operator() {
        let formatter = PostgresExtensionFormatter::new();
        let sql = "resource->>'given'";
        let result = formatter.format_jsonb_operators(sql).unwrap();
        assert!(result.contains(" ->> "));
    }

    #[test]
    fn test_jsonb_contains_operator() {
        let formatter = PostgresExtensionFormatter::new();
        let sql = "data@>'{\"active\":true}'";
        let result = formatter.format_jsonb_operators(sql).unwrap();
        assert!(result.contains(" @> "));
    }

    #[test]
    fn test_jsonb_path_operators() {
        let formatter = PostgresExtensionFormatter::new();
        let sql = "data#>'{a,b}'";
        let result = formatter.format_jsonb_operators(sql).unwrap();
        assert!(result.contains(" #> "));

        let sql = "data#>>'{a,b}'";
        let result = formatter.format_jsonb_operators(sql).unwrap();
        assert!(result.contains(" #>> "));
    }

    #[test]
    fn test_array_overlap_operator() {
        let formatter = PostgresExtensionFormatter::new();
        let sql = "arr1&&arr2";
        let result = formatter.format_array_operators(sql).unwrap();
        assert!(result.contains(" && "));
    }

    #[test]
    fn test_array_constructor_spacing() {
        let formatter = PostgresExtensionFormatter::new();
        let sql = "ARRAY[1,2,3]";
        let result = formatter.format_array_operators(sql).unwrap();
        assert!(result.contains("ARRAY[1, 2, 3]"));
    }

    #[test]
    fn test_complex_jsonb_expression() {
        let formatter = PostgresExtensionFormatter::new();
        let sql = "resource->'name'->>'given'";
        let result = formatter.format_jsonb_operators(sql).unwrap();
        assert!(result.contains(" -> "));
        assert!(result.contains(" ->> "));
    }
}
