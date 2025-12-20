//! Diagnostic publishing for SQL validation
//!
//! This module handles publishing LSP diagnostics for SQL syntax, JSONB validation,
//! and comprehensive SQL linting.

use async_lsp::lsp_types::notification::PublishDiagnostics;
use async_lsp::lsp_types::{Diagnostic, DiagnosticSeverity, Position, PublishDiagnosticsParams, Range};
use async_lsp::ClientSocket;
use url::Url;

use super::linter::SqlLinter;
use super::naming::NamingDiagnostics;

/// Publishes comprehensive diagnostics for the given SQL document.
///
/// This function collects and publishes:
/// - JSONB syntax validation diagnostics
/// - SQL linting diagnostics (performance, best practices, FHIR-specific, security)
/// - Naming convention diagnostics (sqlstyle.guide, as HINT severity)
pub async fn publish_diagnostics(
    client: ClientSocket,
    uri: Url,
    text: String,
    tree: tree_sitter::Tree,
) {
    tracing::info!(uri = %uri, "Collecting diagnostics (JSONB validation + SQL linting)");

    let mut diagnostics = Vec::new();
    let root = tree.root_node();

    // =========================================================================
    // 1. JSONB SYNTAX VALIDATION
    // =========================================================================

    // Walk the tree and find all JSONB expressions (synchronous, fast)
    let mut visit_stack = vec![root];

    while let Some(node) = visit_stack.pop() {
        // Check if this is a binary expression with JSONB operator
        if node.kind() == "binary_expression" {
            if let Some(op_node) = node.child_by_field_name("binary_expr_operator") {
                if op_node.kind() == "op_other" {
                    if let Ok(op_text) = op_node.utf8_text(text.as_bytes()) {
                        if matches!(op_text, "->" | "->>" | "#>" | "#>>") {
                            let right = node.child_by_field_name("binary_expr_right");

                            // Check if the syntax is valid
                            if !is_valid_operand_for_jsonb_operator(op_text, right, &text) {
                                // Invalid syntax - create diagnostic
                                if let Some(right_node) = right {
                                    let diagnostic =
                                        create_jsonb_syntax_diagnostic(op_text, right_node, &text);
                                    diagnostics.push(diagnostic);
                                }
                            }
                        }
                    }
                }
            }
        }

        // Add children to visit stack (depth-first traversal)
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i as u32) {
                visit_stack.push(child);
            }
        }
    }

    tracing::info!(jsonb_diagnostics = diagnostics.len(), "JSONB validation complete");

    // =========================================================================
    // 2. SQL LINTING (AST-BASED RULES)
    // =========================================================================

    let linter = SqlLinter::new();
    let lint_diagnostics = linter.lint(&text, &tree);

    tracing::info!(
        lint_diagnostics = lint_diagnostics.len(),
        "SQL linting complete"
    );

    // Log linting results by category
    let mut performance_count = 0;
    let mut best_practice_count = 0;
    let mut fhir_count = 0;
    let mut security_count = 0;

    for diag in &lint_diagnostics {
        if let Some(code) = &diag.code {
            let code_str = match code {
                async_lsp::lsp_types::NumberOrString::String(s) => s.as_str(),
                _ => "",
            };

            // Categorize by rule ID
            if code_str.contains("select-star") || code_str.contains("like") || code_str.contains("index") {
                performance_count += 1;
            } else if code_str.contains("limit") || code_str.contains("insert") || code_str.contains("join") {
                best_practice_count += 1;
            } else if code_str.contains("fhir") || code_str.contains("jsonb-path") {
                fhir_count += 1;
            } else if code_str.contains("injection") || code_str.contains("superuser") {
                security_count += 1;
            }
        }
    }

    tracing::info!(
        performance = performance_count,
        best_practice = best_practice_count,
        fhir_specific = fhir_count,
        security = security_count,
        "Linting rules breakdown"
    );

    // Combine linting diagnostics
    diagnostics.extend(lint_diagnostics);

    // =========================================================================
    // 3. SQL NAMING CONVENTIONS (sqlstyle.guide)
    // =========================================================================

    let naming_checker = NamingDiagnostics::new();
    let naming_diagnostics = naming_checker.check_sql(&text);

    tracing::info!(
        naming_diagnostics = naming_diagnostics.len(),
        "SQL naming convention check complete"
    );

    // Combine naming diagnostics
    diagnostics.extend(naming_diagnostics);

    // =========================================================================
    // 4. PUBLISH ALL DIAGNOSTICS
    // =========================================================================

    tracing::info!(
        uri = %uri,
        total_diagnostics = diagnostics.len(),
        "Publishing all diagnostics to client"
    );

    for diag in &diagnostics {
        tracing::debug!(
            severity = ?diag.severity,
            range = ?diag.range,
            code = ?diag.code,
            message = %diag.message,
            "Diagnostic detail"
        );
    }

    // Publish via client notification
    let _ = client.notify::<PublishDiagnostics>(PublishDiagnosticsParams {
        uri,
        diagnostics,
        version: None,
    });

    tracing::info!("✅ All diagnostics published successfully");
}

/// Checks if an operand is valid for a given JSONB operator.
fn is_valid_operand_for_jsonb_operator(
    operator: &str,
    right_node: Option<tree_sitter::Node>,
    text: &str,
) -> bool {
    let Some(right) = right_node else {
        // Incomplete expression (no right operand) - consider valid for autocomplete
        return true;
    };

    match operator {
        // Single-key operators: accept string literals, identifiers, numbers
        "->" | "->>" => {
            matches!(
                right.kind(),
                "literal" | "string" | "identifier" | "number" | "integer"
            )
        }
        // Array path operators: require array literals or array constructors
        "#>" | "#>>" => {
            // Check if it's an array literal starting with '{'
            if let Ok(right_text) = right.utf8_text(text.as_bytes()) {
                let trimmed = right_text.trim();
                // PostgreSQL array literals: '{name,given}' or ARRAY['name','given']
                trimmed.starts_with('{') || trimmed.to_uppercase().starts_with("ARRAY")
            } else {
                false
            }
        }
        _ => true, // Unknown operators - assume valid
    }
}

/// Creates an LSP diagnostic for invalid JSONB operator/operand combination.
fn create_jsonb_syntax_diagnostic(
    operator: &str,
    right_node: tree_sitter::Node,
    _text: &str,
) -> Diagnostic {
    let start_pos = Position {
        line: right_node.start_position().row as u32,
        character: right_node.start_position().column as u32,
    };
    let end_pos = Position {
        line: right_node.end_position().row as u32,
        character: right_node.end_position().column as u32,
    };

    let message = match operator {
        "#>" | "#>>" => {
            format!(
                "Invalid operand for {} operator. Expected PostgreSQL array literal like '{{name,given}}' or ARRAY['name','given']",
                operator
            )
        }
        "->" | "->>" => {
            format!(
                "Invalid operand for {} operator. Expected a string literal, identifier, or number",
                operator
            )
        }
        _ => format!("Invalid operand for {} operator", operator),
    };

    Diagnostic {
        range: Range {
            start: start_pos,
            end: end_pos,
        },
        severity: Some(DiagnosticSeverity::ERROR),
        code: None,
        code_description: None,
        source: Some("octofhir-lsp".into()),
        message,
        related_information: None,
        tags: None,
        data: None,
    }
}

/// Helper to convert byte offset to LSP Position
pub fn byte_offset_to_position(text: &str, offset: usize) -> Position {
    let mut line = 0;
    let mut character = 0;

    for (i, ch) in text.char_indices() {
        if i >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            character = 0;
        } else {
            character += 1;
        }
    }

    Position {
        line: line as u32,
        character: character as u32,
    }
}
