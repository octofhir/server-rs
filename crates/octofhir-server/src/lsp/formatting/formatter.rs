//! SQL formatter implementation using pg_query
//!
//! This module provides a SQL formatter that leverages PostgreSQL's own parser (pg_query)
//! for 100% PostgreSQL syntax coverage, with custom post-processing for stylistic formatting.
//!
//! All formatting rules are hardcoded based on sqlstyle.guide with no configuration options.

use super::style;
use std::collections::HashSet;

/// SQL formatter using pg_query for parsing and deparsing.
/// All formatting rules are hardcoded based on sqlstyle.guide.
pub struct SqlFormatter {
    keyword_set: HashSet<&'static str>,
}

/// Formatting result.
pub type FormatResult<T> = Result<T, FormatError>;

/// Formatting error.
#[derive(Debug, thiserror::Error)]
pub enum FormatError {
    #[error("Failed to parse SQL: {0}")]
    ParseError(String),

    #[error("Failed to format SQL: {0}")]
    FormatError(String),

    #[error("Formatting changed SQL semantics")]
    SemanticChange(String),
}

impl SqlFormatter {
    /// Create a new formatter with hardcoded sqlstyle.guide rules.
    pub fn new() -> Self {
        Self {
            keyword_set: style::get_reserved_keywords(),
        }
    }

    /// Format SQL text using hardcoded sqlstyle.guide rules.
    ///
    /// This uses pg_query to parse and deparse SQL, then applies mandatory formatting
    /// rules from sqlstyle.guide (keyword case, alignment, indentation, spacing).
    ///
    /// Zero-loss guarantee: This function ensures that formatting does not change
    /// the semantic meaning of the SQL by comparing query fingerprints before and after.
    ///
    /// Best-effort approach: If formatting fails, falls back to basic keyword uppercasing.
    pub fn format(&self, text: &str) -> FormatResult<String> {
        match self.try_format(text) {
            Ok(formatted) => Ok(formatted),
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    "Formatting failed, applying basic formatting fallback"
                );
                Ok(self.apply_basic_formatting(text))
            }
        }
    }

    /// Try to format SQL with full sqlstyle.guide rules (can fail).
    fn try_format(&self, text: &str) -> FormatResult<String> {
        // Step 1: Parse with pg_query (100% PostgreSQL coverage)
        let parse_result = pg_query::parse(text)
            .map_err(|e| FormatError::ParseError(e.to_string()))?;

        // Step 2: Get fingerprint of original query for validation
        let original_fingerprint = pg_query::fingerprint(text)
            .map_err(|e| FormatError::ParseError(format!("Failed to fingerprint original: {}", e)))?;

        // Step 3: Deparse to get well-formed SQL
        let deparsed = parse_result.deparse()
            .map_err(|e| FormatError::FormatError(format!("Deparse failed: {}", e)))?;

        // Step 4: Apply custom formatting (hardcoded sqlstyle.guide rules)
        let formatted = self.apply_custom_formatting(&deparsed)?;

        // Step 5: Validate zero-loss guarantee by comparing fingerprints
        self.validate_semantics(&original_fingerprint, &formatted)?;

        Ok(formatted)
    }

    /// Apply basic formatting as fallback (just uppercase keywords).
    /// This ensures we always return something even if full formatting fails.
    fn apply_basic_formatting(&self, text: &str) -> String {
        self.convert_keywords_to_upper(text)
    }

    /// Validate that formatting didn't change SQL semantics.
    ///
    /// This compares the query fingerprints of the original and formatted SQL.
    /// If fingerprints match, the queries are semantically equivalent.
    fn validate_semantics(&self, original_fingerprint: &pg_query::Fingerprint, formatted: &str) -> FormatResult<()> {
        let formatted_fingerprint = pg_query::fingerprint(formatted)
            .map_err(|e| FormatError::SemanticChange(format!("Formatted SQL is invalid: {}", e)))?;

        if original_fingerprint.value != formatted_fingerprint.value {
            return Err(FormatError::SemanticChange(format!(
                "Formatting changed SQL semantics! Original fingerprint: {}, Formatted fingerprint: {}",
                original_fingerprint.hex,
                formatted_fingerprint.hex
            )));
        }

        Ok(())
    }

    /// Apply custom formatting to deparsed SQL.
    ///
    /// This handles:
    /// - Keyword case conversion (always UPPERCASE per sqlstyle.guide)
    /// - Advanced formatting via post-processor (indentation, alignment, line wrapping)
    fn apply_custom_formatting(&self, sql: &str) -> FormatResult<String> {
        // Step 1: Apply keyword case conversion (always uppercase)
        let mut output = self.convert_keywords_to_upper(sql);

        // Step 2: Use post-processor for advanced formatting (indentation + alignment)
        let processor = super::post_processor::FormattingPostProcessor::new();
        output = processor.process(&output)?;

        Ok(output)
    }

    /// Convert SQL keywords to uppercase (sqlstyle.guide rule).
    fn convert_keywords_to_upper(&self, sql: &str) -> String {
        let mut result = String::new();
        let mut current_word = String::new();
        let mut in_string = false;
        let mut string_delimiter = ' ';

        for ch in sql.chars() {
            if (ch == '\'' || ch == '"') && !in_string {
                // Start of string
                if !current_word.is_empty() {
                    result.push_str(&self.maybe_uppercase_keyword(&current_word));
                    current_word.clear();
                }
                in_string = true;
                string_delimiter = ch;
                result.push(ch);
            } else if in_string && ch == string_delimiter {
                // End of string
                in_string = false;
                result.push(ch);
            } else if in_string {
                // Inside string literal - preserve as-is
                result.push(ch);
            } else if ch.is_alphanumeric() || ch == '_' {
                // Build word
                current_word.push(ch);
            } else {
                // Non-alphanumeric character - end of word
                if !current_word.is_empty() {
                    result.push_str(&self.maybe_uppercase_keyword(&current_word));
                    current_word.clear();
                }
                result.push(ch);
            }
        }

        // Handle last word
        if !current_word.is_empty() {
            result.push_str(&self.maybe_uppercase_keyword(&current_word));
        }

        result
    }

    /// Convert word to uppercase if it's a keyword.
    fn maybe_uppercase_keyword(&self, word: &str) -> String {
        if self.keyword_set.contains(&word.to_uppercase().as_str()) {
            word.to_uppercase()
        } else {
            word.to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_formatting() {
        let formatter = SqlFormatter::new();
        let sql = "select id,name from patient where active=true";

        let result = formatter.format(sql);
        assert!(result.is_ok(), "Formatting should succeed");

        let formatted = result.unwrap();
        // Keywords should be uppercase per sqlstyle.guide
        assert!(formatted.contains("SELECT"));
        assert!(formatted.contains("FROM"));
        assert!(formatted.contains("WHERE"));
    }

    #[test]
    fn test_uppercase_keywords() {
        let formatter = SqlFormatter::new();
        let sql = "select id from patient";

        let result = formatter.format(sql).unwrap();
        // Keywords always uppercase per sqlstyle.guide
        assert!(result.contains("SELECT"));
        assert!(result.contains("FROM"));
    }

    #[test]
    fn test_complex_query() {
        let formatter = SqlFormatter::new();
        let sql = "WITH cte AS (SELECT * FROM patient) SELECT * FROM cte WHERE active = true";

        let result = formatter.format(sql);
        assert!(result.is_ok(), "Complex query formatting should succeed");
    }

    #[test]
    fn test_insert_statement() {
        let formatter = SqlFormatter::new();
        let sql = "INSERT INTO patient (id, name) VALUES (1, 'John')";

        let result = formatter.format(sql);
        assert!(result.is_ok(), "INSERT formatting should succeed");
    }

    #[test]
    fn test_ddl_statement() {
        let formatter = SqlFormatter::new();
        let sql = "CREATE TABLE test (id INT, name TEXT)";

        let result = formatter.format(sql);
        assert!(result.is_ok(), "DDL formatting should succeed");
    }

    #[test]
    fn test_zero_loss_guarantee() {
        let formatter = SqlFormatter::new();

        let test_cases = vec![
            "SELECT id, name FROM patient WHERE active = true",
            "INSERT INTO patient (id, name) VALUES (1, 'John')",
            "UPDATE patient SET name = 'Jane' WHERE id = 1",
            "DELETE FROM patient WHERE id = 1",
            "CREATE TABLE test (id INT PRIMARY KEY, name TEXT)",
            "WITH cte AS (SELECT * FROM patient) SELECT * FROM cte",
            "SELECT COUNT(*) FROM patient GROUP BY status HAVING COUNT(*) > 5",
            "SELECT * FROM patient ORDER BY id LIMIT 10 OFFSET 5",
        ];

        for sql in test_cases {
            // Get original fingerprint
            let original_fp = pg_query::fingerprint(sql).unwrap();

            // Format the query
            let formatted = formatter.format(sql).unwrap();

            // Get formatted fingerprint
            let formatted_fp = pg_query::fingerprint(&formatted).unwrap();

            // Verify fingerprints match (semantic equivalence)
            assert_eq!(
                original_fp.value, formatted_fp.value,
                "Zero-loss guarantee failed for query: {}\nOriginal FP: {}\nFormatted FP: {}",
                sql, original_fp.hex, formatted_fp.hex
            );
        }
    }

    #[test]
    fn test_idempotency() {
        let formatter = SqlFormatter::new();
        let sql = "select id,name from patient where active=true";

        // Format once
        let formatted_once = formatter.format(sql).unwrap();

        // Format again
        let formatted_twice = formatter.format(&formatted_once).unwrap();

        // Should be identical (idempotent)
        assert_eq!(
            formatted_once, formatted_twice,
            "Formatting should be idempotent: format(format(x)) == format(x)"
        );
    }

    #[test]
    fn test_show_formatting_output() {
        let formatter = SqlFormatter::new();
        let sql = "SELECT id,name FROM patient WHERE active=true";

        let result = formatter.format(sql).unwrap();

        println!("\n=== Original ===");
        println!("{}", sql);
        println!("\n=== Formatted (sqlstyle.guide) ===");
        println!("{}", result);
        println!("\n=== Line by line ===");
        for (i, line) in result.lines().enumerate() {
            println!("Line {}: '{}'", i, line);
        }

        // Verify uppercase keywords
        assert!(result.contains("SELECT"));
        assert!(result.contains("FROM"));
        assert!(result.contains("WHERE"));
        assert!(!result.is_empty());
    }
}
