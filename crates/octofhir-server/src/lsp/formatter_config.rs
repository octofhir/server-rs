//! LSP Formatter configuration types.
//!
//! This module provides serializable configuration types that match the
//! mold_format configuration options, used for passing formatter settings
//! via LSP formatting requests.

use mold_format::{
    CommaStyle as MoldCommaStyle, FormatConfig, IdentifierCase as MoldIdentifierCase,
    IndentStyle, KeywordCase as MoldKeywordCase, PgFormatterConfig,
};
use serde::{Deserialize, Serialize};

// =============================================================================
// Default value functions for serde
// =============================================================================

fn default_true() -> bool {
    true
}

fn default_indent() -> usize {
    4
}

fn default_sqlstyle_max_width() -> usize {
    88
}

fn default_pg_max_width() -> usize {
    80
}

fn default_river_width() -> usize {
    10
}

fn default_upper() -> u8 {
    2
}

fn default_lower() -> u8 {
    1
}

// =============================================================================
// Enum types for SqlStyle config
// =============================================================================

/// Keyword case transformation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum KeywordCase {
    #[default]
    Upper,
    Lower,
    Preserve,
}

impl From<KeywordCase> for MoldKeywordCase {
    fn from(case: KeywordCase) -> Self {
        match case {
            KeywordCase::Upper => MoldKeywordCase::Upper,
            KeywordCase::Lower => MoldKeywordCase::Lower,
            KeywordCase::Preserve => MoldKeywordCase::Preserve,
        }
    }
}

/// Identifier case transformation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum IdentifierCase {
    #[default]
    Lower,
    Preserve,
}

impl From<IdentifierCase> for MoldIdentifierCase {
    fn from(case: IdentifierCase) -> Self {
        match case {
            IdentifierCase::Lower => MoldIdentifierCase::Lower,
            IdentifierCase::Preserve => MoldIdentifierCase::Preserve,
        }
    }
}

/// Comma placement style.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CommaStyle {
    #[default]
    Trailing,
    Leading,
}

impl From<CommaStyle> for MoldCommaStyle {
    fn from(style: CommaStyle) -> Self {
        match style {
            CommaStyle::Trailing => MoldCommaStyle::Trailing,
            CommaStyle::Leading => MoldCommaStyle::Leading,
        }
    }
}

// =============================================================================
// SqlStyle Configuration
// =============================================================================

/// SqlStyle configuration (matches mold_format::FormatConfig).
///
/// This formatter follows sqlstyle.guide conventions with river alignment.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SqlStyleConfig {
    /// Keyword case transformation (default: upper)
    #[serde(default)]
    pub keyword_case: KeywordCase,

    /// Identifier case transformation (default: lower)
    #[serde(default)]
    pub identifier_case: IdentifierCase,

    /// Number of spaces per indent level (default: 4)
    #[serde(default = "default_indent")]
    pub indent_spaces: usize,

    /// Use tabs instead of spaces (default: false)
    #[serde(default)]
    pub use_tabs: bool,

    /// Maximum line width (default: 88)
    #[serde(default = "default_sqlstyle_max_width")]
    pub max_width: usize,

    /// Use river alignment for keywords (default: true)
    #[serde(default = "default_true")]
    pub river_alignment: bool,

    /// Insert newline before AND/OR (default: true)
    #[serde(default = "default_true")]
    pub newline_before_logical: bool,

    /// Add spaces around operators (default: true)
    #[serde(default = "default_true")]
    pub spaces_around_operators: bool,

    /// Comma placement style (default: trailing)
    #[serde(default)]
    pub comma_style: CommaStyle,

    /// Add space inside parentheses (default: false)
    #[serde(default)]
    pub parentheses_spacing: bool,

    /// Align items in SELECT list (default: true)
    #[serde(default = "default_true")]
    pub align_select_items: bool,

    /// River width for alignment (default: 10)
    #[serde(default = "default_river_width")]
    pub river_width: usize,
}

impl Default for SqlStyleConfig {
    fn default() -> Self {
        Self {
            keyword_case: KeywordCase::Upper,
            identifier_case: IdentifierCase::Lower,
            indent_spaces: 4,
            use_tabs: false,
            max_width: 88,
            river_alignment: true,
            newline_before_logical: true,
            spaces_around_operators: true,
            comma_style: CommaStyle::Trailing,
            parentheses_spacing: false,
            align_select_items: true,
            river_width: 10,
        }
    }
}

impl SqlStyleConfig {
    /// Convert to mold_format::FormatConfig.
    pub fn to_format_config(&self) -> FormatConfig {
        let indent = if self.use_tabs {
            IndentStyle::Tabs
        } else {
            IndentStyle::Spaces(self.indent_spaces)
        };

        FormatConfig {
            keyword_case: self.keyword_case.into(),
            identifier_case: self.identifier_case.into(),
            indent,
            max_width: self.max_width,
            river_alignment: self.river_alignment,
            newline_before_logical: self.newline_before_logical,
            spaces_around_operators: self.spaces_around_operators,
            comma_style: self.comma_style.into(),
            parentheses_spacing: self.parentheses_spacing,
            align_select_items: self.align_select_items,
            river_width: self.river_width,
        }
    }
}

// =============================================================================
// PgFormatter Configuration
// =============================================================================

/// PgFormatter configuration (matches mold_format::PgFormatterConfig).
///
/// This formatter is compatible with the pgFormatter tool.
/// Case values: 0=unchanged, 1=lower, 2=upper, 3=capitalize
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PgFormatterStyleConfig {
    // === Case Options ===
    /// Keyword case: 0=unchanged, 1=lower, 2=upper, 3=capitalize (default: 2)
    #[serde(default = "default_upper")]
    pub keyword_case: u8,

    /// Function name case: 0=unchanged, 1=lower, 2=upper, 3=capitalize (default: 0)
    #[serde(default)]
    pub function_case: u8,

    /// Data type case: 0=unchanged, 1=lower, 2=upper, 3=capitalize (default: 1)
    #[serde(default = "default_lower")]
    pub type_case: u8,

    // === Indentation ===
    /// Number of spaces per indent level (default: 4)
    #[serde(default = "default_indent")]
    pub spaces: usize,

    /// Use tabs instead of spaces (default: false)
    #[serde(default)]
    pub use_tabs: bool,

    // === Comma Placement ===
    /// Place comma at beginning of line (default: false)
    #[serde(default)]
    pub comma_start: bool,

    /// Place comma at end of line (default: true)
    #[serde(default = "default_true")]
    pub comma_end: bool,

    /// Add newline after each comma (default: false)
    #[serde(default)]
    pub comma_break: bool,

    // === Line Wrapping ===
    /// Wrap lines at N characters (default: None)
    #[serde(default)]
    pub wrap_limit: Option<usize>,

    /// Wrap lists after N items (default: None)
    #[serde(default)]
    pub wrap_after: Option<usize>,

    /// Apply wrap limit to comments (default: false)
    #[serde(default)]
    pub wrap_comment: bool,

    // === Content Handling ===
    /// Remove all comments (default: false)
    #[serde(default)]
    pub no_comment: bool,

    /// Keep original empty lines (default: false)
    #[serde(default)]
    pub keep_newline: bool,

    /// Do not add trailing newline at end (default: false)
    #[serde(default)]
    pub no_extra_line: bool,

    /// Add newline between statements (default: false)
    #[serde(default)]
    pub no_grouping: bool,

    // === Special Options ===
    /// Do not add space before function parentheses (default: false)
    #[serde(default)]
    pub no_space_function: bool,

    /// Keep redundant parentheses (default: false)
    #[serde(default)]
    pub redundant_parenthesis: bool,

    /// Regex pattern to protect from formatting (default: None)
    #[serde(default)]
    pub placeholder: Option<String>,

    // === Extra Options ===
    /// Maximum line width for formatting (default: 80)
    #[serde(default = "default_pg_max_width")]
    pub max_width: usize,

    /// Use river alignment for keywords (default: false)
    #[serde(default)]
    pub river_alignment: bool,
}

impl Default for PgFormatterStyleConfig {
    fn default() -> Self {
        Self {
            keyword_case: 2,   // upper
            function_case: 0,  // unchanged
            type_case: 1,      // lower
            spaces: 4,
            use_tabs: false,
            comma_start: false,
            comma_end: true,
            comma_break: false,
            wrap_limit: None,
            wrap_after: None,
            wrap_comment: false,
            no_comment: false,
            keep_newline: false,
            no_extra_line: false,
            no_grouping: false,
            no_space_function: false,
            redundant_parenthesis: false,
            placeholder: None,
            max_width: 80,
            river_alignment: false,
        }
    }
}

impl PgFormatterStyleConfig {
    /// Convert to mold_format::PgFormatterConfig.
    pub fn to_pg_formatter_config(&self) -> PgFormatterConfig {
        use mold_format::CaseOption;

        let to_case_option = |v: u8| -> CaseOption {
            CaseOption::from_value(v).unwrap_or(CaseOption::Upper)
        };

        PgFormatterConfig {
            keyword_case: to_case_option(self.keyword_case),
            function_case: to_case_option(self.function_case),
            type_case: to_case_option(self.type_case),
            spaces: self.spaces,
            use_tabs: self.use_tabs,
            comma_start: self.comma_start,
            comma_end: self.comma_end,
            comma_break: self.comma_break,
            wrap_limit: self.wrap_limit,
            wrap_after: self.wrap_after,
            wrap_comment: self.wrap_comment,
            no_comment: self.no_comment,
            keep_newline: self.keep_newline,
            no_extra_line: self.no_extra_line,
            no_grouping: self.no_grouping,
            no_space_function: self.no_space_function,
            redundant_parenthesis: self.redundant_parenthesis,
            placeholder: self.placeholder.clone(),
            max_width: self.max_width,
            river_alignment: self.river_alignment,
        }
    }
}

// =============================================================================
// Main Formatter Config Enum
// =============================================================================

/// LSP Formatter configuration.
///
/// This is a tagged union that supports multiple formatting styles:
/// - `sql_style`: sqlstyle.guide conventions with river alignment
/// - `pg_formatter`: pgFormatter-compatible style
/// - `compact`: Minimal whitespace style
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "style", rename_all = "snake_case")]
pub enum LspFormatterConfig {
    /// SqlStyle formatting (sqlstyle.guide conventions)
    SqlStyle(SqlStyleConfig),

    /// PgFormatter-compatible formatting
    PgFormatter(PgFormatterStyleConfig),

    /// Compact formatting with minimal whitespace
    Compact,
}

impl Default for LspFormatterConfig {
    fn default() -> Self {
        LspFormatterConfig::SqlStyle(SqlStyleConfig::default())
    }
}

impl LspFormatterConfig {
    /// Parse formatter config from LSP formatting options.
    ///
    /// The options are expected to contain custom properties with the formatter config.
    /// If parsing fails or options are empty, returns the default SqlStyle config.
    pub fn from_lsp_options(options: &async_lsp::lsp_types::FormattingOptions) -> Self {
        use async_lsp::lsp_types::FormattingProperty;

        // Convert FormattingProperty to serde_json::Value
        fn prop_to_json(prop: &FormattingProperty) -> serde_json::Value {
            match prop {
                FormattingProperty::Bool(b) => serde_json::Value::Bool(*b),
                FormattingProperty::Number(n) => serde_json::Value::Number((*n).into()),
                FormattingProperty::String(s) => serde_json::Value::String(s.clone()),
            }
        }

        // Check if properties are empty
        if options.properties.is_empty() {
            return LspFormatterConfig::default();
        }

        // Check if there's a "style" key
        if let Some(style_prop) = options.properties.get("style")
            && let FormattingProperty::String(style_str) = style_prop {
                match style_str.as_str() {
                    "compact" => return LspFormatterConfig::Compact,
                    "pg_formatter" => {
                        // Try to parse PgFormatter config from properties
                        let json_obj: serde_json::Map<String, serde_json::Value> = options
                            .properties
                            .iter()
                            .map(|(k, v)| (k.clone(), prop_to_json(v)))
                            .collect();

                        if let Ok(config) =
                            serde_json::from_value(serde_json::Value::Object(json_obj))
                        {
                            return LspFormatterConfig::PgFormatter(config);
                        }
                        return LspFormatterConfig::PgFormatter(PgFormatterStyleConfig::default());
                    }
                    "sql_style" => {
                        // Try to parse SqlStyle config from properties
                        let json_obj: serde_json::Map<String, serde_json::Value> = options
                            .properties
                            .iter()
                            .map(|(k, v)| (k.clone(), prop_to_json(v)))
                            .collect();

                        if let Ok(config) =
                            serde_json::from_value(serde_json::Value::Object(json_obj))
                        {
                            return LspFormatterConfig::SqlStyle(config);
                        }
                        return LspFormatterConfig::SqlStyle(SqlStyleConfig::default());
                    }
                    _ => {}
                }
            }

        // Try to parse the entire properties as a config
        let json_obj: serde_json::Map<String, serde_json::Value> = options
            .properties
            .iter()
            .map(|(k, v)| (k.clone(), prop_to_json(v)))
            .collect();

        if let Ok(config) =
            serde_json::from_value::<LspFormatterConfig>(serde_json::Value::Object(json_obj))
        {
            return config;
        }

        // Default to SqlStyle
        LspFormatterConfig::default()
    }

    /// Format SQL text using this configuration.
    pub fn format(&self, text: &str) -> String {
        match self {
            LspFormatterConfig::SqlStyle(config) => {
                mold_format::format(text, &config.to_format_config())
            }
            LspFormatterConfig::PgFormatter(config) => {
                mold_format::format_with_pgformatter(text, &config.to_pg_formatter_config())
            }
            LspFormatterConfig::Compact => mold_format::format_compact(text),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sqlstyle_config_default() {
        let config = SqlStyleConfig::default();
        assert_eq!(config.keyword_case, KeywordCase::Upper);
        assert_eq!(config.indent_spaces, 4);
        assert!(config.river_alignment);
    }

    #[test]
    fn test_pgformatter_config_default() {
        let config = PgFormatterStyleConfig::default();
        assert_eq!(config.keyword_case, 2); // upper
        assert_eq!(config.function_case, 0); // unchanged
        assert_eq!(config.type_case, 1); // lower
        assert_eq!(config.spaces, 4);
    }

    #[test]
    fn test_sqlstyle_to_format_config() {
        let config = SqlStyleConfig::default();
        let format_config = config.to_format_config();
        assert_eq!(format_config.keyword_case, MoldKeywordCase::Upper);
        assert!(format_config.river_alignment);
    }

    #[test]
    fn test_pgformatter_to_config() {
        let config = PgFormatterStyleConfig::default();
        let pg_config = config.to_pg_formatter_config();
        assert_eq!(pg_config.spaces, 4);
        assert!(!pg_config.comma_start);
        assert!(pg_config.comma_end);
    }

    #[test]
    fn test_lsp_formatter_config_serialize() {
        let config = LspFormatterConfig::SqlStyle(SqlStyleConfig::default());
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("sql_style"));

        let config = LspFormatterConfig::PgFormatter(PgFormatterStyleConfig::default());
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("pg_formatter"));

        let config = LspFormatterConfig::Compact;
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("compact"));
    }

    #[test]
    fn test_lsp_formatter_config_deserialize() {
        let json = r#"{"style": "sql_style", "keywordCase": "lower", "indentSpaces": 2}"#;
        let config: LspFormatterConfig = serde_json::from_str(json).unwrap();

        match config {
            LspFormatterConfig::SqlStyle(c) => {
                assert_eq!(c.keyword_case, KeywordCase::Lower);
                assert_eq!(c.indent_spaces, 2);
            }
            _ => panic!("Expected SqlStyle config"),
        }
    }

    #[test]
    fn test_format_with_sqlstyle() {
        let config = LspFormatterConfig::SqlStyle(SqlStyleConfig::default());
        let formatted = config.format("select id from users");
        assert!(formatted.contains("SELECT"));
        assert!(formatted.contains("FROM"));
    }

    #[test]
    fn test_format_with_compact() {
        let config = LspFormatterConfig::Compact;
        let formatted = config.format("select id from users");
        assert!(formatted.contains("SELECT"));
    }
}
