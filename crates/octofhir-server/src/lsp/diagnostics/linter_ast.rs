//! AST-based linting rules using tree-sitter
//!
//! This module provides robust linting rules that use tree-sitter AST traversal
//! instead of fragile text-based pattern matching.

use super::linter::{LintContext, LintRule, RuleLevel};
use async_lsp::lsp_types::{Diagnostic, DiagnosticSeverity, Position, Range};
use tree_sitter::{Node, Query, QueryCursor};

/// Helper to extract text for a node
fn node_text<'a>(node: Node, source: &'a str) -> &'a str {
    &source[node.byte_range()]
}

/// Convert tree-sitter node position to LSP Position
fn ts_point_to_position(point: tree_sitter::Point) -> Position {
    Position {
        line: point.row as u32,
        character: point.column as u32,
    }
}

/// Convert tree-sitter node to LSP Range
fn node_to_range(node: Node) -> Range {
    Range {
        start: ts_point_to_position(node.start_position()),
        end: ts_point_to_position(node.end_position()),
    }
}

/// Walk the tree and find all nodes matching a predicate
fn find_nodes<'a, F>(node: Node<'a>, predicate: &F) -> Vec<Node<'a>>
where
    F: Fn(Node) -> bool,
{
    let mut results = Vec::new();
    let mut cursor = node.walk();

    fn walk_recursive<'a, F>(
        cursor: &mut tree_sitter::TreeCursor<'a>,
        predicate: &F,
        results: &mut Vec<Node<'a>>,
    ) where
        F: Fn(Node) -> bool,
    {
        let node = cursor.node();
        if predicate(node) {
            results.push(node);
        }

        if cursor.goto_first_child() {
            loop {
                walk_recursive(cursor, predicate, results);
                if !cursor.goto_next_sibling() {
                    break;
                }
            }
            cursor.goto_parent();
        }
    }

    walk_recursive(&mut cursor, predicate, &mut results);
    results
}

// ============================================================================
// AST-BASED RULES
// ============================================================================

/// Rule: no-select-star (AST-based)
/// Detects SELECT * using tree-sitter AST
pub struct NoSelectStarRuleAst;

impl LintRule for NoSelectStarRuleAst {
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
        let root = ctx.tree.root_node();

        // Find all_fields nodes (SELECT *)
        let all_fields_nodes = find_nodes(root, &|node| node.kind() == "all_fields");

        for node in all_fields_nodes {
            diagnostics.push(Diagnostic {
                range: node_to_range(node),
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
        }

        diagnostics
    }
}

/// Rule: limit-without-order (AST-based)
/// Detects LIMIT without ORDER BY using tree-sitter
pub struct LimitWithoutOrderRuleAst;

impl LintRule for LimitWithoutOrderRuleAst {
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
        let root = ctx.tree.root_node();

        // Find all statements that have LIMIT
        let statements = find_nodes(root, &|node| node.kind() == "statement");

        for statement in statements {
            let mut has_limit = false;
            let mut has_order_by = false;
            let mut limit_node: Option<Node> = None;

            // Check all children recursively for LIMIT and ORDER BY
            let children = find_nodes(statement, &|n| n.kind() == "limit" || n.kind() == "order_by");

            for child in children {
                match child.kind() {
                    "limit" => {
                        has_limit = true;
                        limit_node = Some(child);
                    }
                    "order_by" => {
                        has_order_by = true;
                    }
                    _ => {}
                }
            }

            if has_limit && !has_order_by {
                if let Some(node) = limit_node {
                    diagnostics.push(Diagnostic {
                        range: node_to_range(node),
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
                }
            }
        }

        diagnostics
    }
}

/// Rule: inefficient-like (AST-based)
/// Detects LIKE patterns with leading wildcards
pub struct InefficientLikeRuleAst;

impl LintRule for InefficientLikeRuleAst {
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
        let root = ctx.tree.root_node();

        // Find all binary expressions that contain LIKE
        let binary_exprs = find_nodes(root, &|node| node.kind() == "binary_expression");

        for expr in binary_exprs {
            // Check if this binary expression contains a LIKE keyword
            let has_like = find_nodes(expr, &|n| n.kind() == "keyword_like").len() > 0;
            if !has_like {
                continue;
            }

            // Find literal nodes (the pattern)
            let literals = find_nodes(expr, &|n| n.kind() == "literal");

            for lit in literals {
                let lit_text = node_text(lit, ctx.source);
                // Check if starts with '%' (after removing quotes)
                let trimmed = lit_text.trim_matches('\'').trim_matches('"');
                if trimmed.starts_with('%') {
                    diagnostics.push(Diagnostic {
                        range: node_to_range(expr),
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
                    break;
                }
            }
        }

        diagnostics
    }
}

/// Rule: FHIR resource filter suggestion (AST-based)
pub struct FhirResourceFilterRuleAst;

impl LintRule for FhirResourceFilterRuleAst {
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
        let root = ctx.tree.root_node();

        // Find all SELECT statements
        let selects = find_nodes(root, &|node| node.kind() == "select_statement");

        for select_node in selects {
            let sql_text = node_text(select_node, ctx.source).to_lowercase();

            // Check if querying FHIR tables
            let fhir_tables = ["resource", "fhir_resource", "resources"];
            let has_fhir_table = fhir_tables.iter().any(|t| sql_text.contains(t));

            if !has_fhir_table {
                continue;
            }

            // Check if WHERE clause mentions resource_type
            if sql_text.contains("resource_type") {
                continue;
            }

            // Suggest adding resource_type filter
            diagnostics.push(Diagnostic {
                range: node_to_range(select_node),
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

/// Rule: Prefer JSONB path operators (AST-based)
pub struct PreferJsonbPathOpsRuleAst;

impl LintRule for PreferJsonbPathOpsRuleAst {
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
        let root = ctx.tree.root_node();

        // Find binary expressions with -> operator
        let binary_exprs = find_nodes(root, &|node| node.kind() == "binary_expression");

        for expr in binary_exprs {
            let text = node_text(expr, ctx.source);
            // Count -> operators
            let arrow_count = text.matches("->").count();

            // If chained 3+ times, suggest #>
            if arrow_count >= 3 {
                diagnostics.push(Diagnostic {
                    range: node_to_range(expr),
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

        diagnostics
    }
}

/// Rule: SQL injection risk detection (AST-based)
pub struct SqlInjectionRiskRuleAst;

impl LintRule for SqlInjectionRiskRuleAst {
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
        let root = ctx.tree.root_node();

        // Find WHERE clauses
        let where_nodes = find_nodes(root, &|node| node.kind() == "where");

        for where_node in where_nodes {
            // Find ||  operator (op_other containing ||)
            let concat_ops = find_nodes(where_node, &|node| {
                if node.kind() == "op_other" {
                    node_text(node, ctx.source).contains("||")
                } else {
                    false
                }
            });

            for op_node in concat_ops {
                // Get the parent binary expression for better range
                let range = if let Some(parent) = op_node.parent() {
                    node_to_range(parent)
                } else {
                    node_to_range(op_node)
                };

                diagnostics.push(Diagnostic {
                    range,
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

            // Also check for CONCAT function calls
            let concat_fns = find_nodes(where_node, &|node| {
                if node.kind() == "function_call" {
                    node_text(node, ctx.source).to_uppercase().contains("CONCAT")
                } else {
                    false
                }
            });

            for fn_node in concat_fns {
                diagnostics.push(Diagnostic {
                    range: node_to_range(fn_node),
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

        diagnostics
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tree_sitter::Parser;

    fn parse_sql(sql: &str) -> tree_sitter::Tree {
        let mut parser = Parser::new();
        parser
            .set_language(&pgls_treesitter_grammar::LANGUAGE.into())
            .unwrap();
        parser.parse(sql, None).unwrap()
    }

    fn print_tree(node: tree_sitter::Node, source: &str, depth: usize) {
        let indent = "  ".repeat(depth);
        let text = &source[node.byte_range()];
        let text_preview = if text.len() > 40 {
            format!("{}...", &text[..40])
        } else {
            text.to_string()
        };
        println!("{}{} | {}", indent, node.kind(), text_preview);

        let mut cursor = node.walk();
        if cursor.goto_first_child() {
            loop {
                print_tree(cursor.node(), source, depth + 1);
                if !cursor.goto_next_sibling() {
                    break;
                }
            }
        }
    }

    #[test]
    fn debug_tree_structure() {
        let sql = "SELECT * FROM patient LIMIT 10";
        let tree = parse_sql(sql);
        println!("\n=== Tree structure for: {} ===", sql);
        print_tree(tree.root_node(), sql, 0);
    }

    #[test]
    fn debug_like_structure() {
        let sql = "SELECT * FROM patient WHERE name LIKE '%smith'";
        let tree = parse_sql(sql);
        println!("\n=== Tree structure for LIKE: {} ===", sql);
        print_tree(tree.root_node(), sql, 0);
    }

    #[test]
    fn debug_sql_injection_structure() {
        let sql = "SELECT * FROM patient WHERE id = 'foo' || bar";
        let tree = parse_sql(sql);
        println!("\n=== Tree structure for SQL injection: {} ===", sql);
        print_tree(tree.root_node(), sql, 0);
    }

    #[test]
    fn debug_select_star_with_alias() {
        let sql = "SELECT * FROM patient as p WHERE p.resource->>'{}' = 'test'";
        let tree = parse_sql(sql);
        println!("\n=== Tree structure for SELECT * with alias: {} ===", sql);
        print_tree(tree.root_node(), sql, 0);
    }

    #[test]
    fn test_no_select_star_ast() {
        let sql = "SELECT * FROM patient";
        let tree = parse_sql(sql);

        let rule = NoSelectStarRuleAst;
        let ctx = LintContext {
            source: sql,
            tree: &tree,
            is_valid_sql: true,
            schema_cache: None,
            fhir_resolver: None,
        };

        let diagnostics = rule.check(&ctx);
        assert!(!diagnostics.is_empty(), "Should detect SELECT *");
    }

    #[test]
    fn test_limit_without_order_ast() {
        let sql = "SELECT id FROM patient LIMIT 10";
        let tree = parse_sql(sql);

        let rule = LimitWithoutOrderRuleAst;
        let ctx = LintContext {
            source: sql,
            tree: &tree,
            is_valid_sql: true,
            schema_cache: None,
            fhir_resolver: None,
        };

        let diagnostics = rule.check(&ctx);
        assert!(!diagnostics.is_empty(), "Should detect LIMIT without ORDER BY");
    }

    #[test]
    fn test_inefficient_like_ast() {
        let sql = "SELECT * FROM patient WHERE name LIKE '%smith'";
        let tree = parse_sql(sql);

        let rule = InefficientLikeRuleAst;
        let ctx = LintContext {
            source: sql,
            tree: &tree,
            is_valid_sql: true,
            schema_cache: None,
            fhir_resolver: None,
        };

        let diagnostics = rule.check(&ctx);
        assert!(!diagnostics.is_empty(), "Should detect leading wildcard");
    }

    #[test]
    fn test_select_star_with_alias() {
        let sql = "SELECT * FROM patient as p WHERE p.resource->>'{}' = 'test'";
        let tree = parse_sql(sql);

        let rule = NoSelectStarRuleAst;
        let ctx = LintContext {
            source: sql,
            tree: &tree,
            is_valid_sql: true,
            schema_cache: None,
            fhir_resolver: None,
        };

        let diagnostics = rule.check(&ctx);
        assert!(
            !diagnostics.is_empty(),
            "Should detect SELECT * even with table alias"
        );

        // Verify the diagnostic contains the expected code
        assert!(diagnostics.iter().any(|d| {
            if let Some(async_lsp::lsp_types::NumberOrString::String(code)) = &d.code {
                code.contains("no-select-star")
            } else {
                false
            }
        }));

        // Verify the diagnostic message
        assert!(diagnostics[0].message.contains("Avoid SELECT *"));
    }

    #[test]
    fn test_full_linter_with_alias() {
        // Integration test: verify the full linter runs with aliased query
        let sql = "SELECT * FROM patient as p WHERE p.resource->>'{}' = 'test'";
        let tree = parse_sql(sql);

        let linter = super::super::linter::SqlLinter::new();
        let diagnostics = linter.lint(sql, &tree);

        // Should have at least the SELECT * diagnostic
        assert!(
            !diagnostics.is_empty(),
            "Linter should produce diagnostics for SELECT * with alias"
        );

        // Verify we have the SELECT * warning
        let has_select_star = diagnostics.iter().any(|d| {
            if let Some(async_lsp::lsp_types::NumberOrString::String(code)) = &d.code {
                code.contains("no-select-star")
            } else {
                false
            }
        });

        assert!(
            has_select_star,
            "Linter should detect SELECT * in aliased query. Found diagnostics: {:?}",
            diagnostics.iter().map(|d| &d.code).collect::<Vec<_>>()
        );
    }
}
