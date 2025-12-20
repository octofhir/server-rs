//! JSONB expression detection for PostgreSQL LSP.
//!
//! This module provides clean, tree-sitter-based detection of JSONB expressions
//! at cursor positions, enabling FHIR-aware autocompletion for JSONB paths.
//!
//! ## Why tree-sitter?
//!
//! Tree-sitter is ideal for LSP autocompletion because:
//! - Handles **incomplete SQL** (critical while user is typing)
//! - Fast incremental parsing
//! - Resilient error recovery
//!
//! While we use pg_query for semantic analysis, tree-sitter remains the best
//! choice for cursor-position-based JSONB detection in the LSP context.

use tree_sitter::Node;

use crate::lsp::server::JsonbContext;

/// JSONB expression detector using tree-sitter AST.
///
/// Provides clean detection of JSONB operators and path chains at cursor positions.
pub struct JsonbDetector;

impl JsonbDetector {
    /// Detect JSONB expression at cursor position.
    ///
    /// Returns JsonbContext with operator, path chain, and validity information.
    /// Works with both complete and incomplete SQL (while typing).
    pub fn detect<'a>(
        cursor_node: &Node<'a>,
        text: &str,
        offset: usize,
    ) -> Option<JsonbContext<'a>> {
        // Find the JSONB binary expression containing the cursor
        let expr_node = Self::find_jsonb_expression(cursor_node, text)?;

        // Extract operator and operands
        let (operator, left, right) = Self::parse_jsonb_expression(&expr_node, text)?;

        // Build path chain from nested expressions
        let path_chain = Self::extract_path_chain(expr_node, text);

        // Validate operator/operand combination
        let is_valid_syntax = Self::validate_syntax(&operator, right.as_ref(), text);

        Some(JsonbContext {
            expr_node,
            operator,
            left,
            right,
            cursor_offset: offset,
            path_chain,
            is_valid_syntax,
        })
    }

    /// Extract the full JSONB path as (column_name, path_segments).
    ///
    /// Example: `resource->'name'->'given'` → `("resource", ["name", "given"])`
    pub fn extract_path(node: &Node, text: &str) -> Option<(String, Vec<String>)> {
        // Find the root column reference
        let column = Self::find_root_column(node, text)?;

        // Extract all path segments
        let mut segments = Vec::new();
        Self::collect_path_segments(node, text, &mut segments);

        Some((column, segments))
    }

    /// Extract partial JSONB path for filtering completions.
    ///
    /// Returns the text being typed after the last JSONB operator.
    /// Example: Typing `resource->'na` → returns `Some("na")`
    pub fn extract_partial_path(
        ctx: &JsonbContext,
        text: &str,
        cursor_node: &Node,
    ) -> Option<String> {
        // If cursor is inside a string node, extract the partial text
        if let Some(string_node) = Self::find_string_at_cursor(cursor_node, ctx.cursor_offset) {
            let start = string_node.start_byte();
            let end = ctx.cursor_offset;

            if end > start {
                let partial = &text[start..end];
                // Remove quotes
                let partial = partial.trim_start_matches(['\'', '"']);
                return Some(partial.to_string());
            }
        }

        None
    }

    // ============================================================================
    // Private Helper Methods
    // ============================================================================

    /// Find JSONB expression node containing the cursor.
    fn find_jsonb_expression<'a>(cursor_node: &Node<'a>, text: &str) -> Option<Node<'a>> {
        let mut current = *cursor_node;

        // Walk up the AST to find a binary_expression with JSONB operator
        while current.kind() != "program" {
            if current.kind() == "binary_expression" {
                // Check if this is a JSONB operator
                if let Some(op_node) = current.child_by_field_name("operator") {
                    let op_text = op_node.utf8_text(text.as_bytes()).ok()?;
                    if Self::is_jsonb_operator(op_text) {
                        return Some(current);
                    }
                }
            }

            current = current.parent()?;
        }

        None
    }

    /// Parse JSONB expression into (operator, left_node, right_node).
    fn parse_jsonb_expression<'a>(
        expr: &Node<'a>,
        text: &str,
    ) -> Option<(String, Option<Node<'a>>, Option<Node<'a>>)> {
        let op_node = expr.child_by_field_name("operator")?;
        let operator = op_node.utf8_text(text.as_bytes()).ok()?.to_string();

        let left = expr.child_by_field_name("left");
        let right = expr.child_by_field_name("right");

        Some((operator, left, right))
    }

    /// Extract full path chain from nested JSONB expressions.
    ///
    /// Example: `resource->'name'->'given'` → `["resource", "name", "given"]`
    fn extract_path_chain(node: Node, text: &str) -> Vec<String> {
        let mut chain = Vec::new();
        Self::traverse_jsonb_expr(node, text, &mut chain);
        chain
    }

    /// Recursively traverse JSONB expression to build path chain.
    fn traverse_jsonb_expr(node: Node, text: &str, chain: &mut Vec<String>) {
        if node.kind() == "binary_expression" {
            // Check if this is a JSONB operator
            if let Some(op_node) = node.child_by_field_name("operator") {
                if let Ok(op_text) = op_node.utf8_text(text.as_bytes()) {
                    if Self::is_jsonb_operator(op_text) {
                        // Traverse left side first (builds chain left-to-right)
                        if let Some(left) = node.child_by_field_name("left") {
                            Self::traverse_jsonb_expr(left, text, chain);
                        }

                        // Add right side to chain
                        if let Some(right) = node.child_by_field_name("right") {
                            if let Ok(right_text) = right.utf8_text(text.as_bytes()) {
                                // Remove quotes from string literals
                                let segment = right_text.trim_matches(['\'', '"', '{', '}']);
                                chain.push(segment.to_string());
                            }
                        }

                        return;
                    }
                }
            }
        }

        // Base case: column identifier
        if let Ok(text) = node.utf8_text(text.as_bytes()) {
            chain.push(text.to_string());
        }
    }

    /// Find root column reference in JSONB expression.
    fn find_root_column(node: &Node, text: &str) -> Option<String> {
        let mut current = *node;

        // Walk left until we find the column reference
        while current.kind() == "binary_expression" {
            if let Some(left) = current.child_by_field_name("left") {
                current = left;
            } else {
                break;
            }
        }

        // Extract column name
        current.utf8_text(text.as_bytes()).ok().map(String::from)
    }

    /// Collect path segments (excluding column name).
    fn collect_path_segments(node: &Node, text: &str, segments: &mut Vec<String>) {
        if node.kind() == "binary_expression" {
            if let Some(op_node) = node.child_by_field_name("operator") {
                if let Ok(op_text) = op_node.utf8_text(text.as_bytes()) {
                    if Self::is_jsonb_operator(op_text) {
                        // Recursively process left side
                        if let Some(left) = node.child_by_field_name("left") {
                            Self::collect_path_segments(&left, text, segments);
                        }

                        // Add right side segment
                        if let Some(right) = node.child_by_field_name("right") {
                            if let Ok(right_text) = right.utf8_text(text.as_bytes()) {
                                let segment = right_text.trim_matches(['\'', '"', '{', '}']);
                                segments.push(segment.to_string());
                            }
                        }
                    }
                }
            }
        }
    }

    /// Validate JSONB operator/operand syntax.
    ///
    /// Example: `#>>` with single string is invalid (requires array)
    fn validate_syntax(operator: &str, right_node: Option<&Node>, text: &str) -> bool {
        let Some(right) = right_node else {
            // Incomplete expression (typing in progress) - consider valid
            return true;
        };

        let Ok(right_text) = right.utf8_text(text.as_bytes()) else {
            return true;
        };

        match operator {
            "#>" | "#>>" => {
                // Array operators require '{...}' format
                right_text.starts_with('{') && right_text.ends_with('}')
            }
            "->" | "->>" => {
                // Object operators accept string keys
                true
            }
            _ => true,
        }
    }

    /// Check if operator is a JSONB operator.
    fn is_jsonb_operator(op: &str) -> bool {
        matches!(op, "->" | "->>" | "#>" | "#>>")
    }

    /// Find string node at cursor position.
    fn find_string_at_cursor<'a>(cursor_node: &Node<'a>, offset: usize) -> Option<Node<'a>> {
        let mut current = *cursor_node;

        while current.kind() != "program" {
            if matches!(current.kind(), "string" | "string_literal") {
                let start = current.start_byte();
                let end = current.end_byte();

                if offset >= start && offset <= end {
                    return Some(current);
                }
            }

            current = current.parent()?;
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_jsonb_operator() {
        assert!(JsonbDetector::is_jsonb_operator("->"));
        assert!(JsonbDetector::is_jsonb_operator("->>"));
        assert!(JsonbDetector::is_jsonb_operator("#>"));
        assert!(JsonbDetector::is_jsonb_operator("#>>"));
        assert!(!JsonbDetector::is_jsonb_operator("="));
        assert!(!JsonbDetector::is_jsonb_operator("+"));
    }

    #[test]
    fn test_validate_syntax() {
        // Incomplete expressions (None) are always valid
        assert!(JsonbDetector::validate_syntax("#>", None, ""));
        assert!(JsonbDetector::validate_syntax("#>>", None, ""));
        assert!(JsonbDetector::validate_syntax("->", None, ""));
        assert!(JsonbDetector::validate_syntax("->>", None, ""));

        // Full validation would require real tree-sitter nodes from parsed SQL
        // (tested in integration tests with actual SQL parsing)
    }
}
