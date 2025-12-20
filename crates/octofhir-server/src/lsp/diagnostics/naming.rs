//! SQL naming convention diagnostics based on sqlstyle.guide
//!
//! This module validates SQL identifiers (tables, columns, aliases) against
//! sqlstyle.guide naming conventions and produces LSP diagnostics (warnings).
//!
//! **Diagnostics only - no auto-fix**. Auto-fix is a future enhancement.
//!
//! ## Naming Rules (from sqlstyle.guide)
//!
//! - Max 30 bytes
//! - Start with letter, no trailing underscore
//! - Only letters, numbers, underscores
//! - snake_case (not camelCase)
//! - Avoid consecutive underscores
//! - No prefixes like sp_, tbl_
//! - Avoid quoted identifiers
//! - Tables: plural/collective, lowercase
//! - Columns: singular, lowercase, with standard suffixes (_id, _status, _total, etc.)
//! - Aliases: always use AS keyword
//! - No reserved keywords as identifiers

use async_lsp::lsp_types::{Diagnostic, DiagnosticSeverity, Position, Range};
use std::collections::HashSet;

use super::super::formatting::style;

/// Naming convention diagnostics checker
pub struct NamingDiagnostics {
    reserved_keywords: HashSet<&'static str>,
    preferred_suffixes: Vec<&'static str>,
    forbidden_prefixes: Vec<&'static str>,
}

/// Types of naming violations
#[derive(Debug, Clone, PartialEq)]
pub enum NamingViolation {
    /// Identifier exceeds 30 bytes
    TooLong { name: String, length: usize },

    /// Identifier doesn't start with a letter
    InvalidStart { name: String },

    /// Identifier has trailing underscore
    TrailingUnderscore { name: String },

    /// Identifier contains invalid characters
    InvalidCharacters { name: String, invalid_chars: String },

    /// Identifier uses a reserved keyword
    ReservedKeyword { name: String },

    /// Identifier uses camelCase instead of snake_case
    CamelCase { name: String, suggested: String },

    /// Identifier uses a forbidden prefix (sp_, tbl_, etc.)
    ForbiddenPrefix { name: String, prefix: String },

    /// Identifier has consecutive underscores
    ConsecutiveUnderscores { name: String },

    /// Identifier is quoted (discouraged)
    QuotedIdentifier { name: String },

    /// Column name doesn't use recommended suffix
    MissingSuffix {
        name: String,
        hint: String,
    },
}

impl NamingViolation {
    /// Get human-readable message for this violation
    pub fn message(&self) -> String {
        match self {
            Self::TooLong { name, length } => {
                format!(
                    "Identifier '{}' is too long ({} bytes). sqlstyle.guide recommends max 30 bytes.",
                    name, length
                )
            }
            Self::InvalidStart { name } => {
                format!(
                    "Identifier '{}' should start with a letter (sqlstyle.guide).",
                    name
                )
            }
            Self::TrailingUnderscore { name } => {
                format!(
                    "Identifier '{}' should not end with underscore (sqlstyle.guide).",
                    name
                )
            }
            Self::InvalidCharacters { name, invalid_chars } => {
                format!(
                    "Identifier '{}' contains invalid characters: '{}'. Use only letters, numbers, and underscores (sqlstyle.guide).",
                    name, invalid_chars
                )
            }
            Self::ReservedKeyword { name } => {
                format!(
                    "Identifier '{}' is a reserved keyword. Avoid using reserved words as identifiers (sqlstyle.guide).",
                    name
                )
            }
            Self::CamelCase { name, suggested } => {
                format!(
                    "Identifier '{}' uses camelCase. Use snake_case instead: '{}' (sqlstyle.guide).",
                    name, suggested
                )
            }
            Self::ForbiddenPrefix { name, prefix } => {
                format!(
                    "Identifier '{}' uses forbidden prefix '{}'. Avoid Hungarian notation and prefixes (sqlstyle.guide).",
                    name, prefix
                )
            }
            Self::ConsecutiveUnderscores { name } => {
                format!(
                    "Identifier '{}' has consecutive underscores. Use single underscores (sqlstyle.guide).",
                    name
                )
            }
            Self::QuotedIdentifier { name } => {
                format!(
                    "Identifier \"{}\" is quoted. Prefer unquoted lowercase identifiers (sqlstyle.guide).",
                    name
                )
            }
            Self::MissingSuffix { name, hint } => {
                format!(
                    "Column '{}' might benefit from a standard suffix. {}",
                    name, hint
                )
            }
        }
    }
}

impl NamingDiagnostics {
    /// Create a new naming diagnostics checker
    pub fn new() -> Self {
        Self {
            reserved_keywords: style::get_reserved_keywords(),
            preferred_suffixes: vec![
                "_id", "_status", "_total", "_num", "_name", "_seq",
                "_date", "_tally", "_size", "_addr", "_count", "_type",
                "_flag", "_code", "_ref", "_time", "_timestamp", "_at",
            ],
            forbidden_prefixes: vec![
                "sp_", "tbl_", "fn_", "vw_", "idx_", "pk_", "fk_",
            ],
        }
    }

    /// Check a SQL string for naming violations and return LSP diagnostics
    pub fn check_sql(&self, sql: &str) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        // For now, we'll use a simple heuristic to extract identifiers
        // Future enhancement: Use pg_query to parse AST and extract all identifiers
        let identifiers = self.extract_identifiers_simple(sql);

        for (identifier, line, col) in identifiers {
            let violations = self.check_identifier(&identifier);

            for violation in violations {
                // Create LSP diagnostic with HINT severity (optional style suggestions)
                let diagnostic = Diagnostic {
                    range: Range {
                        start: Position {
                            line: line as u32,
                            character: col as u32,
                        },
                        end: Position {
                            line: line as u32,
                            character: (col + identifier.len()) as u32,
                        },
                    },
                    severity: Some(DiagnosticSeverity::HINT),
                    code: None,
                    code_description: None,
                    source: Some("sqlstyle.guide".to_string()),
                    message: violation.message(),
                    related_information: None,
                    tags: None,
                    data: None,
                };

                diagnostics.push(diagnostic);
            }
        }

        diagnostics
    }

    /// Check a single identifier for naming violations
    pub fn check_identifier(&self, name: &str) -> Vec<NamingViolation> {
        let mut violations = Vec::new();

        // Strip quotes if present (but mark as violation)
        let is_quoted = (name.starts_with('"') && name.ends_with('"'))
            || (name.starts_with('`') && name.ends_with('`'));

        let unquoted_name = if is_quoted {
            violations.push(NamingViolation::QuotedIdentifier {
                name: name.to_string(),
            });
            &name[1..name.len() - 1]
        } else {
            name
        };

        // Skip validation for system identifiers or empty names
        if unquoted_name.is_empty() || unquoted_name.starts_with('$') {
            return violations;
        }

        // Rule 1: Max 30 bytes
        if unquoted_name.len() > 30 {
            violations.push(NamingViolation::TooLong {
                name: unquoted_name.to_string(),
                length: unquoted_name.len(),
            });
        }

        // Rule 2: Must start with letter
        if let Some(first_char) = unquoted_name.chars().next() {
            if !first_char.is_ascii_alphabetic() {
                violations.push(NamingViolation::InvalidStart {
                    name: unquoted_name.to_string(),
                });
            }
        }

        // Rule 3: No trailing underscore
        if unquoted_name.ends_with('_') {
            violations.push(NamingViolation::TrailingUnderscore {
                name: unquoted_name.to_string(),
            });
        }

        // Rule 4: Only letters, numbers, underscores
        let invalid_chars: String = unquoted_name
            .chars()
            .filter(|c| !c.is_ascii_alphanumeric() && *c != '_')
            .collect();

        if !invalid_chars.is_empty() {
            violations.push(NamingViolation::InvalidCharacters {
                name: unquoted_name.to_string(),
                invalid_chars,
            });
        }

        // Rule 5: Check for consecutive underscores
        if unquoted_name.contains("__") {
            violations.push(NamingViolation::ConsecutiveUnderscores {
                name: unquoted_name.to_string(),
            });
        }

        // Rule 6: Check for camelCase
        if self.is_camel_case(unquoted_name) {
            let suggested = self.camel_to_snake(unquoted_name);
            violations.push(NamingViolation::CamelCase {
                name: unquoted_name.to_string(),
                suggested,
            });
        }

        // Rule 7: Check for forbidden prefixes
        for prefix in &self.forbidden_prefixes {
            if unquoted_name.to_lowercase().starts_with(prefix) {
                violations.push(NamingViolation::ForbiddenPrefix {
                    name: unquoted_name.to_string(),
                    prefix: prefix.to_string(),
                });
                break;
            }
        }

        // Rule 8: Check if it's a reserved keyword
        if self.reserved_keywords.contains(&unquoted_name.to_uppercase().as_str()) {
            violations.push(NamingViolation::ReservedKeyword {
                name: unquoted_name.to_string(),
            });
        }

        violations
    }

    /// Extract identifiers from SQL (simple heuristic)
    ///
    /// Future enhancement: Use pg_query to parse AST and extract all identifiers
    /// with proper context (table names, column names, aliases)
    fn extract_identifiers_simple(&self, sql: &str) -> Vec<(String, usize, usize)> {
        let mut identifiers = Vec::new();

        for (line_num, line) in sql.lines().enumerate() {
            let mut in_string = false;
            let mut string_delimiter = ' ';
            let mut current_word = String::new();
            let mut word_start_col = 0;

            for (col, ch) in line.chars().enumerate() {
                if (ch == '\'' || ch == '"' || ch == '`') && !in_string {
                    // Start of string
                    if !current_word.is_empty() {
                        if self.looks_like_identifier(&current_word) {
                            identifiers.push((current_word.clone(), line_num, word_start_col));
                        }
                        current_word.clear();
                    }
                    in_string = true;
                    string_delimiter = ch;

                    // If it's a quoted identifier (backticks or double quotes in some modes)
                    if ch == '"' || ch == '`' {
                        current_word.push(ch);
                        word_start_col = col;
                    }
                } else if in_string && ch == string_delimiter {
                    // End of string
                    if string_delimiter == '"' || string_delimiter == '`' {
                        current_word.push(ch);
                        identifiers.push((current_word.clone(), line_num, word_start_col));
                        current_word.clear();
                    }
                    in_string = false;
                } else if in_string {
                    // Inside string
                    if string_delimiter == '"' || string_delimiter == '`' {
                        current_word.push(ch);
                    }
                } else if ch.is_alphanumeric() || ch == '_' {
                    // Build word
                    if current_word.is_empty() {
                        word_start_col = col;
                    }
                    current_word.push(ch);
                } else {
                    // Non-alphanumeric character - end of word
                    if !current_word.is_empty() {
                        if self.looks_like_identifier(&current_word) {
                            identifiers.push((current_word.clone(), line_num, word_start_col));
                        }
                        current_word.clear();
                    }
                }
            }

            // Handle last word in line
            if !current_word.is_empty() && !in_string {
                if self.looks_like_identifier(&current_word) {
                    identifiers.push((current_word, line_num, word_start_col));
                }
            }
        }

        identifiers
    }

    /// Check if a word looks like an identifier (not a keyword, not a number)
    fn looks_like_identifier(&self, word: &str) -> bool {
        // Skip pure numbers
        if word.chars().all(|c| c.is_ascii_digit()) {
            return false;
        }

        // Skip common SQL keywords (we'll still check them, but don't extract as identifiers)
        // This is a heuristic - the real check happens in check_identifier
        let upper = word.to_uppercase();
        if self.reserved_keywords.contains(upper.as_str()) {
            // Reserved keywords are identifiers if used as identifiers, but we'll
            // skip them in extraction to reduce noise
            return false;
        }

        // Skip boolean literals
        if word.eq_ignore_ascii_case("true")
            || word.eq_ignore_ascii_case("false")
            || word.eq_ignore_ascii_case("null")
        {
            return false;
        }

        true
    }

    /// Check if identifier is in camelCase
    fn is_camel_case(&self, name: &str) -> bool {
        // camelCase has uppercase letters in the middle/end (not first char)
        let mut found_uppercase = false;
        for (i, ch) in name.chars().enumerate() {
            if i > 0 && ch.is_ascii_uppercase() {
                found_uppercase = true;
                break;
            }
        }

        found_uppercase && !name.contains('_')
    }

    /// Convert camelCase to snake_case
    fn camel_to_snake(&self, name: &str) -> String {
        let mut result = String::new();

        for (i, ch) in name.chars().enumerate() {
            if i > 0 && ch.is_ascii_uppercase() {
                result.push('_');
            }
            result.push(ch.to_ascii_lowercase());
        }

        result
    }
}

impl Default for NamingDiagnostics {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_too_long() {
        let checker = NamingDiagnostics::new();
        let name = "this_is_a_very_long_identifier_name_that_exceeds_thirty_bytes";
        let violations = checker.check_identifier(name);

        assert!(violations.iter().any(|v| matches!(v, NamingViolation::TooLong { .. })));
    }

    #[test]
    fn test_invalid_start() {
        let checker = NamingDiagnostics::new();
        let violations = checker.check_identifier("_invalid");

        assert!(violations.iter().any(|v| matches!(v, NamingViolation::InvalidStart { .. })));
    }

    #[test]
    fn test_trailing_underscore() {
        let checker = NamingDiagnostics::new();
        let violations = checker.check_identifier("invalid_");

        assert!(violations.iter().any(|v| matches!(v, NamingViolation::TrailingUnderscore { .. })));
    }

    #[test]
    fn test_camel_case() {
        let checker = NamingDiagnostics::new();
        let violations = checker.check_identifier("myTableName");

        assert!(violations.iter().any(|v| matches!(v, NamingViolation::CamelCase { .. })));
    }

    #[test]
    fn test_consecutive_underscores() {
        let checker = NamingDiagnostics::new();
        let violations = checker.check_identifier("my__table");

        assert!(violations.iter().any(|v| matches!(v, NamingViolation::ConsecutiveUnderscores { .. })));
    }

    #[test]
    fn test_forbidden_prefix() {
        let checker = NamingDiagnostics::new();
        let violations = checker.check_identifier("sp_get_user");

        assert!(violations.iter().any(|v| matches!(v, NamingViolation::ForbiddenPrefix { .. })));
    }

    #[test]
    fn test_reserved_keyword() {
        let checker = NamingDiagnostics::new();
        let violations = checker.check_identifier("select");

        assert!(violations.iter().any(|v| matches!(v, NamingViolation::ReservedKeyword { .. })));
    }

    #[test]
    fn test_quoted_identifier() {
        let checker = NamingDiagnostics::new();
        let violations = checker.check_identifier("\"MyTable\"");

        assert!(violations.iter().any(|v| matches!(v, NamingViolation::QuotedIdentifier { .. })));
    }

    #[test]
    fn test_valid_identifier() {
        let checker = NamingDiagnostics::new();
        let violations = checker.check_identifier("patient_id");

        // Should have no violations (valid snake_case with _id suffix)
        assert!(violations.is_empty());
    }

    #[test]
    fn test_camel_to_snake() {
        let checker = NamingDiagnostics::new();
        assert_eq!(checker.camel_to_snake("myTableName"), "my_table_name");
        assert_eq!(checker.camel_to_snake("patientID"), "patient_i_d");
        assert_eq!(checker.camel_to_snake("firstName"), "first_name");
    }

    #[test]
    fn test_check_sql() {
        let checker = NamingDiagnostics::new();
        let sql = "SELECT myColumn FROM myTable WHERE userId = 1";
        let diagnostics = checker.check_sql(sql);

        // Should detect camelCase violations
        assert!(!diagnostics.is_empty());
    }
}
