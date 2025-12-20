//! SQL Linting Engine for PostgreSQL
//!
//! Provides pluggable linting rules for SQL queries with support for:
//! - Performance optimization rules
//! - Best practice enforcement
//! - FHIR-specific patterns
//! - Security vulnerability detection
//!
//! ## Architecture
//!
//! The linter uses a trait-based system where each rule implements the `LintRule` trait.
//! Rules can be enabled/disabled and configured via LSP initialization options.

use async_lsp::lsp_types::{Diagnostic, DiagnosticSeverity, Position, Range};
use dashmap::DashMap;
use std::sync::Arc;
use tree_sitter::{Node, Tree};

/// Severity level for linting rules
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleLevel {
    /// Critical error that must be fixed
    Error,
    /// Warning that should be addressed
    Warning,
    /// Informational message
    Info,
    /// Subtle hint for improvement
    Hint,
}

impl RuleLevel {
    /// Convert to LSP DiagnosticSeverity
    pub fn to_diagnostic_severity(self) -> DiagnosticSeverity {
        match self {
            RuleLevel::Error => DiagnosticSeverity::ERROR,
            RuleLevel::Warning => DiagnosticSeverity::WARNING,
            RuleLevel::Info => DiagnosticSeverity::INFORMATION,
            RuleLevel::Hint => DiagnosticSeverity::HINT,
        }
    }
}

/// Context provided to lint rules during execution
pub struct LintContext<'a> {
    /// The SQL source text
    pub source: &'a str,
    /// The tree-sitter parse tree
    pub tree: &'a Tree,
    /// Whether SQL is valid according to pg_query
    pub is_valid_sql: bool,
    /// Schema cache (if available)
    pub schema_cache: Option<&'a crate::lsp::SchemaCache>,
    /// FHIR resolver (if available)
    pub fhir_resolver: Option<&'a crate::lsp::FhirResolver>,
}

/// Trait for implementing SQL linting rules
pub trait LintRule: Send + Sync {
    /// Unique identifier for this rule (e.g., "no-select-star")
    fn id(&self) -> &'static str;

    /// Human-readable description of what this rule checks
    fn description(&self) -> &'static str;

    /// Default severity level for this rule
    fn default_level(&self) -> RuleLevel;

    /// Category of this rule (performance, best-practice, fhir-specific, security)
    fn category(&self) -> &'static str;

    /// Check the SQL and return diagnostics for violations
    fn check(&self, ctx: &LintContext) -> Vec<Diagnostic>;

    /// Whether this rule is enabled by default
    fn enabled_by_default(&self) -> bool {
        true
    }
}

/// Configuration for the SQL linter
#[derive(Debug, Clone)]
pub struct LinterConfig {
    /// Rules to enable (empty = all default rules)
    pub enabled_rules: Vec<String>,
    /// Rules to disable
    pub disabled_rules: Vec<String>,
    /// Override severity levels for specific rules
    pub severity_overrides: DashMap<String, RuleLevel>,
}

impl Default for LinterConfig {
    fn default() -> Self {
        Self {
            enabled_rules: Vec::new(),
            disabled_rules: Vec::new(),
            severity_overrides: DashMap::new(),
        }
    }
}

/// Main SQL linter that orchestrates all linting rules
pub struct SqlLinter {
    rules: Vec<Box<dyn LintRule>>,
    config: Arc<LinterConfig>,
}

impl SqlLinter {
    /// Create a new SqlLinter with default rules
    pub fn new() -> Self {
        let mut linter = Self {
            rules: Vec::new(),
            config: Arc::new(LinterConfig::default()),
        };

        // Register all default rules
        linter.register_performance_rules();
        linter.register_best_practice_rules();
        linter.register_fhir_rules();
        linter.register_security_rules();

        linter
    }

    /// Create a SqlLinter with custom configuration
    pub fn with_config(config: LinterConfig) -> Self {
        let mut linter = Self::new();
        linter.config = Arc::new(config);
        linter
    }

    /// Register a custom linting rule
    pub fn register_rule(&mut self, rule: Box<dyn LintRule>) {
        self.rules.push(rule);
    }

    /// Run all enabled rules and collect diagnostics
    pub fn lint(&self, source: &str, tree: &Tree) -> Vec<Diagnostic> {
        // Check if SQL is valid according to pg_query (PostgreSQL parser)
        let is_valid_sql = pg_query::parse(source).is_ok();

        let ctx = LintContext {
            source,
            tree,
            is_valid_sql,
            schema_cache: None,
            fhir_resolver: None,
        };

        self.lint_with_context(&ctx)
    }

    /// Run all enabled rules with full context
    pub fn lint_with_context(&self, ctx: &LintContext) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        for rule in &self.rules {
            // Check if rule is enabled
            if !self.is_rule_enabled(rule.as_ref()) {
                continue;
            }

            // Run the rule
            let mut rule_diagnostics = rule.check(ctx);

            // Apply severity overrides
            let rule_id = rule.id();
            if let Some(severity) = self.config.severity_overrides.get(rule_id) {
                for diag in &mut rule_diagnostics {
                    diag.severity = Some(severity.to_diagnostic_severity());
                }
            }

            diagnostics.extend(rule_diagnostics);
        }

        diagnostics
    }

    /// Check if a rule is enabled based on configuration
    fn is_rule_enabled(&self, rule: &dyn LintRule) -> bool {
        let rule_id = rule.id();

        // Check explicit disable list
        if self.config.disabled_rules.contains(&rule_id.to_string()) {
            return false;
        }

        // If enabled_rules is empty, use default behavior
        if self.config.enabled_rules.is_empty() {
            return rule.enabled_by_default();
        }

        // Check explicit enable list
        self.config.enabled_rules.contains(&rule_id.to_string())
    }

    /// Register all performance-related rules
    fn register_performance_rules(&mut self) {
        // Use AST-based rules for better accuracy
        self.register_rule(Box::new(super::linter_ast::NoSelectStarRuleAst));
        self.register_rule(Box::new(super::linter_ast::InefficientLikeRuleAst));
        self.register_rule(Box::new(MissingIndexHintRule)); // Keep text-based (needs schema)
    }

    /// Register all best practice rules
    fn register_best_practice_rules(&mut self) {
        self.register_rule(Box::new(super::linter_ast::LimitWithoutOrderRuleAst));
        self.register_rule(Box::new(ExplicitInsertColumnsRule)); // TODO: AST version
        self.register_rule(Box::new(AlwaysQualifyJoinsRule)); // TODO: AST version
    }

    /// Register all FHIR-specific rules
    fn register_fhir_rules(&mut self) {
        self.register_rule(Box::new(super::linter_ast::FhirResourceFilterRuleAst));
        self.register_rule(Box::new(JsonbPathValidationRule)); // TODO: AST version
        self.register_rule(Box::new(super::linter_ast::PreferJsonbPathOpsRuleAst));
    }

    /// Register all security rules
    fn register_security_rules(&mut self) {
        self.register_rule(Box::new(super::linter_ast::SqlInjectionRiskRuleAst));
        self.register_rule(Box::new(NoSuperuserGrantsRule)); // Keep text-based
    }
}

impl Default for SqlLinter {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// PERFORMANCE RULES
// ============================================================================

/// Rule: no-select-star
/// Flags SELECT * and suggests explicit column lists
struct NoSelectStarRule;

impl LintRule for NoSelectStarRule {
    fn id(&self) -> &'static str {
        "no-select-star"
    }

    fn description(&self) -> &'static str {
        "Avoid SELECT * in production queries. Use explicit column lists for better performance and maintainability."
    }

    fn default_level(&self) -> RuleLevel {
        RuleLevel::Warning
    }

    fn category(&self) -> &'static str {
        "performance"
    }

    fn check(&self, ctx: &LintContext) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        // Text-based detection for SELECT *
        // TODO: Use pg_query AST walking for more accurate detection in future
        for (line_num, line) in ctx.source.lines().enumerate() {
            if line.to_uppercase().contains("SELECT *") || line.to_uppercase().contains("SELECT\t*") {
                if let Some(pos) = line.to_uppercase().find("SELECT") {
                    diagnostics.push(Diagnostic {
                        range: Range {
                            start: Position {
                                line: line_num as u32,
                                character: pos as u32,
                            },
                            end: Position {
                                line: line_num as u32,
                                character: (pos + 8) as u32,  // "SELECT *"
                            },
                        },
                        severity: Some(self.default_level().to_diagnostic_severity()),
                        code: Some(async_lsp::lsp_types::NumberOrString::String(
                            self.id().to_string(),
                        )),
                        source: Some("octofhir-linter".to_string()),
                        message: format!(
                            "{} Consider listing specific columns instead of using *.",
                            self.description()
                        ),
                        ..Default::default()
                    });
                    break;
                }
            }
        }

        diagnostics
    }
}

/// Rule: inefficient-like
/// Detects LIKE patterns with leading wildcards
struct InefficientLikeRule;

impl LintRule for InefficientLikeRule {
    fn id(&self) -> &'static str {
        "inefficient-like"
    }

    fn description(&self) -> &'static str {
        "LIKE patterns starting with '%' cannot use indexes efficiently"
    }

    fn default_level(&self) -> RuleLevel {
        RuleLevel::Warning
    }

    fn category(&self) -> &'static str {
        "performance"
    }

    fn check(&self, ctx: &LintContext) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        // Simple text-based detection for now
        // TODO: Use tree-sitter query for more accurate detection
        for (line_num, line) in ctx.source.lines().enumerate() {
            if let Some(pos) = line.to_uppercase().find("LIKE") {
                // Check if the pattern after LIKE starts with '%'
                let after_like = &line[pos + 4..].trim_start();
                if after_like.starts_with("'%") || after_like.starts_with("\"'%\"") {
                    let range = Range {
                        start: Position {
                            line: line_num as u32,
                            character: pos as u32,
                        },
                        end: Position {
                            line: line_num as u32,
                            character: (pos + 4) as u32,
                        },
                    };

                    diagnostics.push(Diagnostic {
                        range,
                        severity: Some(self.default_level().to_diagnostic_severity()),
                        code: Some(async_lsp::lsp_types::NumberOrString::String(
                            self.id().to_string(),
                        )),
                        source: Some("octofhir-linter".to_string()),
                        message: format!(
                            "{}. Consider using full-text search or pg_trgm for better performance.",
                            self.description()
                        ),
                        ..Default::default()
                    });
                }
            }
        }

        diagnostics
    }
}

/// Rule: missing-index-hint
/// Warns about WHERE clauses on potentially non-indexed columns
struct MissingIndexHintRule;

impl LintRule for MissingIndexHintRule {
    fn id(&self) -> &'static str {
        "missing-index-hint"
    }

    fn description(&self) -> &'static str {
        "WHERE clause on column that may not be indexed"
    }

    fn default_level(&self) -> RuleLevel {
        RuleLevel::Info
    }

    fn category(&self) -> &'static str {
        "performance"
    }

    fn enabled_by_default(&self) -> bool {
        false // Disabled by default (requires schema analysis)
    }

    fn check(&self, ctx: &LintContext) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        // TODO: Implement with schema cache integration
        // For now, this is a placeholder that requires schema context

        if ctx.schema_cache.is_some() {
            // Future implementation: check if WHERE columns have indexes
        }

        diagnostics
    }
}

// ============================================================================
// BEST PRACTICE RULES
// ============================================================================

/// Rule: limit-without-order
/// Flags LIMIT without ORDER BY (non-deterministic results)
struct LimitWithoutOrderRule;

impl LintRule for LimitWithoutOrderRule {
    fn id(&self) -> &'static str {
        "limit-without-order"
    }

    fn description(&self) -> &'static str {
        "LIMIT without ORDER BY produces non-deterministic results"
    }

    fn default_level(&self) -> RuleLevel {
        RuleLevel::Warning
    }

    fn category(&self) -> &'static str {
        "best-practice"
    }

    fn check(&self, ctx: &LintContext) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        // Text-based detection for LIMIT without ORDER BY
        let has_limit = ctx.source.to_uppercase().contains("LIMIT");
        let has_order_by = ctx.source.to_uppercase().contains("ORDER BY");

        if has_limit && !has_order_by {
            // Find LIMIT position
            for (line_num, line) in ctx.source.lines().enumerate() {
                if let Some(pos) = line.to_uppercase().find("LIMIT") {
                    let range = Range {
                        start: Position {
                            line: line_num as u32,
                            character: pos as u32,
                        },
                        end: Position {
                            line: line_num as u32,
                            character: (pos + 5) as u32,
                        },
                    };

                    diagnostics.push(Diagnostic {
                        range,
                        severity: Some(self.default_level().to_diagnostic_severity()),
                        code: Some(async_lsp::lsp_types::NumberOrString::String(
                            self.id().to_string(),
                        )),
                        source: Some("octofhir-linter".to_string()),
                        message: format!(
                            "{}. Add ORDER BY to ensure consistent results.",
                            self.description()
                        ),
                        ..Default::default()
                    });
                    break; // Only report once per query
                }
            }
        }

        diagnostics
    }
}

/// Rule: explicit-insert-columns
/// Requires column list in INSERT statements
struct ExplicitInsertColumnsRule;

impl LintRule for ExplicitInsertColumnsRule {
    fn id(&self) -> &'static str {
        "explicit-insert-columns"
    }

    fn description(&self) -> &'static str {
        "INSERT statements should explicitly list columns"
    }

    fn default_level(&self) -> RuleLevel {
        RuleLevel::Info
    }

    fn category(&self) -> &'static str {
        "best-practice"
    }

    fn enabled_by_default(&self) -> bool {
        false // Disabled by default (can be noisy)
    }

    fn check(&self, ctx: &LintContext) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        // TODO: Implement with tree-sitter query
        // Detect: INSERT INTO table VALUES (...) without column list

        diagnostics
    }
}

/// Rule: always-qualify-joins
/// Encourages explicit JOIN syntax over implicit comma joins
struct AlwaysQualifyJoinsRule;

impl LintRule for AlwaysQualifyJoinsRule {
    fn id(&self) -> &'static str {
        "always-qualify-joins"
    }

    fn description(&self) -> &'static str {
        "Use explicit JOIN syntax instead of comma-separated tables"
    }

    fn default_level(&self) -> RuleLevel {
        RuleLevel::Info
    }

    fn category(&self) -> &'static str {
        "best-practice"
    }

    fn check(&self, ctx: &LintContext) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        // TODO: Implement detection of implicit joins (FROM table1, table2)

        diagnostics
    }
}

// ============================================================================
// FHIR-SPECIFIC RULES
// ============================================================================

/// Rule: fhir-resource-filter
/// Suggests filtering on resource_type for FHIR resource tables
struct FhirResourceFilterRule;

impl LintRule for FhirResourceFilterRule {
    fn id(&self) -> &'static str {
        "fhir-resource-filter"
    }

    fn description(&self) -> &'static str {
        "Consider filtering on resource_type when querying FHIR resource tables"
    }

    fn default_level(&self) -> RuleLevel {
        RuleLevel::Info
    }

    fn category(&self) -> &'static str {
        "fhir-specific"
    }

    fn check(&self, ctx: &LintContext) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        // Check if query references common FHIR table names
        let fhir_tables = ["resource", "fhir_resource", "resources"];
        let has_fhir_table = fhir_tables
            .iter()
            .any(|table| ctx.source.to_lowercase().contains(table));

        if has_fhir_table && !ctx.source.to_lowercase().contains("resource_type") {
            // Suggest adding resource_type filter
            diagnostics.push(Diagnostic {
                range: Range {
                    start: Position {
                        line: 0,
                        character: 0,
                    },
                    end: Position {
                        line: 0,
                        character: 6,
                    },
                },
                severity: Some(self.default_level().to_diagnostic_severity()),
                code: Some(async_lsp::lsp_types::NumberOrString::String(
                    self.id().to_string(),
                )),
                source: Some("octofhir-linter".to_string()),
                message: format!(
                    "{}. Add WHERE resource_type = 'ResourceType' to improve query performance.",
                    self.description()
                ),
                ..Default::default()
            });
        }

        diagnostics
    }
}

/// Rule: jsonb-path-validation
/// Validates JSONB paths against FHIR schema (catches typos)
struct JsonbPathValidationRule;

impl LintRule for JsonbPathValidationRule {
    fn id(&self) -> &'static str {
        "jsonb-path-validation"
    }

    fn description(&self) -> &'static str {
        "Validate JSONB paths against FHIR schema"
    }

    fn default_level(&self) -> RuleLevel {
        RuleLevel::Warning
    }

    fn category(&self) -> &'static str {
        "fhir-specific"
    }

    fn enabled_by_default(&self) -> bool {
        false // Requires FHIR resolver context
    }

    fn check(&self, ctx: &LintContext) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        // TODO: Implement with FHIR resolver integration
        // Extract JSONB paths and validate against FHIR schemas

        if ctx.fhir_resolver.is_some() {
            // Future implementation: validate paths like resource->'name'->'given'
        }

        diagnostics
    }
}

/// Rule: prefer-jsonb-path-ops
/// Suggests using #> for nested paths instead of chained ->
struct PreferJsonbPathOpsRule;

impl LintRule for PreferJsonbPathOpsRule {
    fn id(&self) -> &'static str {
        "prefer-jsonb-path-ops"
    }

    fn description(&self) -> &'static str {
        "Use #> operator for nested JSONB paths instead of chaining -> operators"
    }

    fn default_level(&self) -> RuleLevel {
        RuleLevel::Hint
    }

    fn category(&self) -> &'static str {
        "fhir-specific"
    }

    fn check(&self, ctx: &LintContext) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        // Detect chained -> operators (3 or more in a row)
        let mut arrow_count = 0;
        let mut last_arrow_pos = None;

        for (line_num, line) in ctx.source.lines().enumerate() {
            let mut chars = line.chars().enumerate().peekable();
            while let Some((pos, ch)) = chars.next() {
                if ch == '-' {
                    if let Some((_, '>')) = chars.peek() {
                        arrow_count += 1;
                        last_arrow_pos = Some((line_num, pos));
                        chars.next(); // consume '>'
                    }
                } else if !ch.is_whitespace() && ch != '\'' && ch != '"' {
                    // Reset count if we hit non-whitespace/non-quote
                    if arrow_count >= 3 {
                        // Suggest using #> instead
                        if let Some((line, col)) = last_arrow_pos {
                            diagnostics.push(Diagnostic {
                                range: Range {
                                    start: Position {
                                        line: line as u32,
                                        character: col as u32,
                                    },
                                    end: Position {
                                        line: line as u32,
                                        character: (col + 2) as u32,
                                    },
                                },
                                severity: Some(self.default_level().to_diagnostic_severity()),
                                code: Some(async_lsp::lsp_types::NumberOrString::String(
                                    self.id().to_string(),
                                )),
                                source: Some("octofhir-linter".to_string()),
                                message: format!(
                                    "{}. Example: data#>'{{name,given,0}}' instead of data->'name'->'given'->0",
                                    self.description()
                                ),
                                ..Default::default()
                            });
                        }
                    }
                    arrow_count = 0;
                }
            }
        }

        diagnostics
    }
}

// ============================================================================
// SECURITY RULES
// ============================================================================

/// Rule: sql-injection-risk
/// Flags string concatenation patterns that may indicate SQL injection
struct SqlInjectionRiskRule;

impl LintRule for SqlInjectionRiskRule {
    fn id(&self) -> &'static str {
        "sql-injection-risk"
    }

    fn description(&self) -> &'static str {
        "Potential SQL injection risk detected"
    }

    fn default_level(&self) -> RuleLevel {
        RuleLevel::Error
    }

    fn category(&self) -> &'static str {
        "security"
    }

    fn check(&self, ctx: &LintContext) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        // Look for patterns like: "... WHERE id = " || variable
        // This is a simplified check; real implementation needs AST analysis
        let dangerous_patterns = ["||", "CONCAT("];

        for pattern in &dangerous_patterns {
            for (line_num, line) in ctx.source.lines().enumerate() {
                if line.contains(pattern) && line.to_uppercase().contains("WHERE") {
                    if let Some(pos) = line.find(pattern) {
                        diagnostics.push(Diagnostic {
                            range: Range {
                                start: Position {
                                    line: line_num as u32,
                                    character: pos as u32,
                                },
                                end: Position {
                                    line: line_num as u32,
                                    character: (pos + pattern.len()) as u32,
                                },
                            },
                            severity: Some(self.default_level().to_diagnostic_severity()),
                            code: Some(async_lsp::lsp_types::NumberOrString::String(
                                self.id().to_string(),
                            )),
                            source: Some("octofhir-linter".to_string()),
                            message: format!(
                                "{}. Use parameterized queries ($1, $2, etc.) instead of string concatenation.",
                                self.description()
                            ),
                            ..Default::default()
                        });
                    }
                }
            }
        }

        diagnostics
    }
}

/// Rule: no-superuser-grants
/// Warns about grants to postgres superuser role
struct NoSuperuserGrantsRule;

impl LintRule for NoSuperuserGrantsRule {
    fn id(&self) -> &'static str {
        "no-superuser-grants"
    }

    fn description(&self) -> &'static str {
        "Avoid granting privileges to postgres superuser role"
    }

    fn default_level(&self) -> RuleLevel {
        RuleLevel::Warning
    }

    fn category(&self) -> &'static str {
        "security"
    }

    fn check(&self, ctx: &LintContext) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        // Check for GRANT ... TO postgres
        if ctx.source.to_uppercase().contains("GRANT") {
            for (line_num, line) in ctx.source.lines().enumerate() {
                let upper = line.to_uppercase();
                if upper.contains("GRANT") && upper.contains("TO POSTGRES") {
                    if let Some(pos) = upper.find("GRANT") {
                        diagnostics.push(Diagnostic {
                            range: Range {
                                start: Position {
                                    line: line_num as u32,
                                    character: pos as u32,
                                },
                                end: Position {
                                    line: line_num as u32,
                                    character: (pos + 5) as u32,
                                },
                            },
                            severity: Some(self.default_level().to_diagnostic_severity()),
                            code: Some(async_lsp::lsp_types::NumberOrString::String(
                                self.id().to_string(),
                            )),
                            source: Some("octofhir-linter".to_string()),
                            message: format!(
                                "{}. Create dedicated roles instead.",
                                self.description()
                            ),
                            ..Default::default()
                        });
                    }
                }
            }
        }

        diagnostics
    }
}

// ============================================================================
// HELPER FUNCTIONS
// ============================================================================

/// Convert tree-sitter node to LSP Range
fn node_to_range(node: Node, source: &str) -> Range {
    let start = node.start_position();
    let end = node.end_position();

    Range {
        start: Position {
            line: start.row as u32,
            character: start.column as u32,
        },
        end: Position {
            line: end.row as u32,
            character: end.column as u32,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_select_star_rule() {
        let sql = "SELECT * FROM patient";
        let linter = SqlLinter::new();

        // Parse with tree-sitter
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&pgls_treesitter_grammar::LANGUAGE.into())
            .unwrap();
        let tree = parser.parse(sql, None).unwrap();

        let diagnostics = linter.lint(sql, &tree);

        // Should have at least one diagnostic for SELECT *
        assert!(
            !diagnostics.is_empty(),
            "Expected diagnostic for SELECT *"
        );
        assert!(diagnostics.iter().any(|d| {
            if let Some(async_lsp::lsp_types::NumberOrString::String(code)) = &d.code {
                code.contains("no-select-star")
            } else {
                false
            }
        }));
    }

    #[test]
    fn test_limit_without_order_rule() {
        let sql = "SELECT id, name FROM patient LIMIT 10";
        let linter = SqlLinter::new();

        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&pgls_treesitter_grammar::LANGUAGE.into())
            .unwrap();
        let tree = parser.parse(sql, None).unwrap();

        let diagnostics = linter.lint(sql, &tree);

        // Should warn about LIMIT without ORDER BY
        assert!(
            diagnostics.iter().any(|d| {
                if let Some(async_lsp::lsp_types::NumberOrString::String(code)) = &d.code {
                    code.contains("limit-without-order")
                } else {
                    false
                }
            }),
            "Expected diagnostic for LIMIT without ORDER BY"
        );
    }

    #[test]
    fn test_inefficient_like_rule() {
        let sql = "SELECT * FROM patient WHERE name LIKE '%smith'";
        let linter = SqlLinter::new();

        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&pgls_treesitter_grammar::LANGUAGE.into())
            .unwrap();
        let tree = parser.parse(sql, None).unwrap();

        let diagnostics = linter.lint(sql, &tree);

        // Should warn about leading wildcard
        assert!(
            diagnostics.iter().any(|d| {
                if let Some(async_lsp::lsp_types::NumberOrString::String(code)) = &d.code {
                    code.contains("inefficient-like")
                } else {
                    false
                }
            }),
            "Expected diagnostic for inefficient LIKE pattern"
        );
    }

    #[test]
    fn test_linter_config_disable_rule() {
        let mut config = LinterConfig::default();
        config.disabled_rules.push("no-select-star".to_string());

        let linter = SqlLinter::with_config(config);

        let sql = "SELECT * FROM patient";
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&pgls_treesitter_grammar::LANGUAGE.into())
            .unwrap();
        let tree = parser.parse(sql, None).unwrap();

        let diagnostics = linter.lint(sql, &tree);

        // Should not have diagnostic for SELECT * (rule disabled)
        assert!(
            !diagnostics.iter().any(|d| {
                if let Some(async_lsp::lsp_types::NumberOrString::String(code)) = &d.code {
                    code.contains("no-select-star")
                } else {
                    false
                }
            }),
            "Rule should be disabled"
        );
    }

    #[test]
    fn test_sql_injection_risk_rule() {
        let sql = "SELECT * FROM patient WHERE id = 'foo' || bar";
        let linter = SqlLinter::new();

        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&pgls_treesitter_grammar::LANGUAGE.into())
            .unwrap();
        let tree = parser.parse(sql, None).unwrap();

        let diagnostics = linter.lint(sql, &tree);

        // Should error on potential SQL injection
        assert!(
            diagnostics.iter().any(|d| {
                if let Some(async_lsp::lsp_types::NumberOrString::String(code)) = &d.code {
                    code.contains("sql-injection-risk")
                } else {
                    false
                }
            }),
            "Expected diagnostic for SQL injection risk"
        );
    }
}
