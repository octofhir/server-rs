//! SQL validation diagnostics
//!
//! This module validates SQL syntax and semantics, checking for duplicate clauses,
//! invalid table/column references, and missing FROM clauses.

use async_lsp::lsp_types::notification::PublishDiagnostics;
use async_lsp::lsp_types::{Diagnostic, DiagnosticSeverity, PublishDiagnosticsParams, Range};
use async_lsp::ClientSocket;
use pgls_text_size::TextSize;
use pgls_treesitter::context::{TreeSitterContextParams, TreesitterContext};
use std::collections::HashMap;
use std::sync::Arc;
use url::Url;

use crate::lsp::schema_cache::SchemaCache;
use crate::lsp::parser::table_resolver::TableResolver;
use super::publisher::byte_offset_to_position;

/// Publishes SQL validation diagnostics for invalid SQL patterns.
///
/// This function analyzes the parsed SQL AST to detect common errors and publishes
/// LSP diagnostics to provide real-time feedback to users. Currently detects:
///
/// - **Duplicate clauses**: Multiple WHERE, GROUP BY, HAVING, ORDER BY, LIMIT, or OFFSET clauses
/// - **Invalid table references**: Tables that don't exist in the schema
/// - **Invalid function references**: Functions that don't exist in the schema
/// - **Missing FROM clause**: SELECT statements with column references but no FROM
/// - **Invalid column references**: Columns that don't exist in referenced tables
///
/// # Arguments
///
/// * `client` - The LSP client socket for publishing diagnostics
/// * `uri` - The URI of the document being validated
/// * `text` - The full text of the SQL document
/// * `tree` - The parsed tree-sitter AST
/// * `schema_cache` - Cache of database schema information
pub async fn publish_sql_validation_diagnostics(
    client: ClientSocket,
    uri: Url,
    text: String,
    tree: tree_sitter::Tree,
    schema_cache: Arc<SchemaCache>,
) {
    tracing::info!(uri = %uri, "Collecting SQL validation diagnostics");

    let mut diagnostics = Vec::new();

    // Track seen clauses: clause_name -> (first_start_byte, first_end_byte)
    let mut seen_clauses: HashMap<String, (usize, usize)> = HashMap::new();

    // Walk the AST to find duplicate clauses
    visit_for_duplicates(tree.root_node(), &text, &mut seen_clauses, &mut diagnostics);

    // Validate table names against schema cache
    visit_for_table_names(tree.root_node(), &text, &schema_cache, &mut diagnostics);

    // Validate function names against schema cache
    visit_for_function_names(tree.root_node(), &text, &schema_cache, &mut diagnostics);

    // Validate SELECT statements have FROM clause (if they reference columns)
    visit_for_missing_from(tree.root_node(), &text, &mut diagnostics);

    // Get mentioned tables for column validation
    let mentioned_tables: Vec<String> = {
        let ctx = TreesitterContext::new(TreeSitterContextParams {
            position: TextSize::new(0),
            text: &text,
            tree: &tree,
        });

        let mut tables = Vec::new();
        if let Some(table_set) = ctx.get_mentioned_relations(&None) {
            tables.extend(table_set.iter().cloned());
        }
        if let Some(table_set) = ctx.get_mentioned_relations(&Some("public".to_string())) {
            tables.extend(table_set.iter().cloned());
        }
        tables
    };

    // Resolve table aliases to avoid false positives in column validation
    let table_resolver = TableResolver::resolve(&text, &tree);
    tracing::debug!(
        aliases = ?table_resolver.get_aliases(),
        "Resolved table aliases for column validation"
    );

    // Validate column references against known columns
    visit_for_column_references(
        tree.root_node(),
        &text,
        &schema_cache,
        &mentioned_tables,
        &table_resolver,
        &mut diagnostics,
    );

    // Publish diagnostics to LSP client
    tracing::info!(
        uri = %uri,
        diagnostic_count = diagnostics.len(),
        "Publishing SQL validation diagnostics to client"
    );

    let _ = client.notify::<PublishDiagnostics>(PublishDiagnosticsParams {
        uri,
        diagnostics,
        version: None,
    });
    tracing::info!("SQL validation diagnostics published");
}

/// Recursive function to walk AST and detect duplicate clauses
fn visit_for_duplicates(
    node: tree_sitter::Node,
    text: &str,
    seen: &mut HashMap<String, (usize, usize)>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // Check if this node represents a clause we want to track
    let clause_name = match node.kind() {
        "where" | "where_clause" => Some("WHERE"),
        "group_by" | "group_by_clause" => Some("GROUP BY"),
        "having" | "having_clause" => Some("HAVING"),
        "order_by" | "order_by_clause" => Some("ORDER BY"),
        "limit" | "limit_clause" => Some("LIMIT"),
        "offset" | "offset_clause" => Some("OFFSET"),
        _ => None,
    };

    if let Some(clause) = clause_name {
        let start_byte = node.start_byte();
        let end_byte = node.end_byte();

        if let Some((first_start, _first_end)) = seen.get(clause) {
            // Duplicate clause detected!
            let start_pos = byte_offset_to_position(text, start_byte);
            let end_pos = byte_offset_to_position(text, end_byte);
            let first_line = byte_offset_to_position(text, *first_start).line + 1;

            diagnostics.push(Diagnostic {
                range: Range {
                    start: start_pos,
                    end: end_pos,
                },
                severity: Some(DiagnosticSeverity::WARNING),
                code: None,
                source: Some("octofhir-lsp".into()),
                message: format!(
                    "Duplicate {} clause. First occurrence at line {}. PostgreSQL only allows one {} clause per query.",
                    clause, first_line, clause
                ),
                related_information: None,
                tags: None,
                code_description: None,
                data: None,
            });
        } else {
            // First occurrence of this clause
            seen.insert(clause.to_string(), (start_byte, end_byte));
        }
    }

    // Recursively visit all children
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        visit_for_duplicates(child, text, seen, diagnostics);
    }
}

/// Validate table names against schema cache
fn visit_for_table_names(
    node: tree_sitter::Node,
    text: &str,
    schema_cache: &SchemaCache,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // Log ALL nodes we encounter to debug AST structure
    if node.kind().contains("from") || node.kind().contains("relation") || node.kind().contains("table") {
        tracing::info!("[TABLE DEBUG] Found node: kind='{}', text={:?}",
            node.kind(),
            node.utf8_text(text.as_bytes()).ok().and_then(|s| if s.len() < 100 { Some(s) } else { None })
        );
    }

    // Check for table references - look for 'table_reference' nodes which contain table names
    if node.kind() == "table_reference" {
        if let Ok(table_name) = node.utf8_text(text.as_bytes()) {
            let table_name = table_name.trim();

            tracing::info!("[TABLE DEBUG] Checking table: '{}'", table_name);

            // Skip if it contains a dot (schema-qualified names)
            // Skip if it contains spaces or special chars (might be complex expression)
            if !table_name.contains('.') && !table_name.contains(' ') && !table_name.contains('(') {
                // Check if table exists in schema cache
                if !schema_cache.table_exists(table_name) {
                    tracing::info!("[TABLE DEBUG] Table '{}' NOT FOUND - creating diagnostic", table_name);

                    let start_pos = byte_offset_to_position(text, node.start_byte());
                    let end_pos = byte_offset_to_position(text, node.end_byte());

                    diagnostics.push(Diagnostic {
                        range: Range {
                            start: start_pos,
                            end: end_pos,
                        },
                        severity: Some(DiagnosticSeverity::ERROR),
                        code: None,
                        source: Some("octofhir-lsp".into()),
                        message: format!(
                            "Table '{}' does not exist in the database schema",
                            table_name
                        ),
                        related_information: None,
                        tags: None,
                        code_description: None,
                        data: None,
                    });
                } else {
                    tracing::info!("[TABLE DEBUG] Table '{}' exists - OK", table_name);
                }
            }
        }
    }

    // Recursively visit all children
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        visit_for_table_names(child, text, schema_cache, diagnostics);
    }
}

/// Validate function names against schema cache
fn visit_for_function_names(
    node: tree_sitter::Node,
    text: &str,
    schema_cache: &SchemaCache,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // Check for function calls
    if node.kind() == "function_call" || node.kind() == "invocation" {
        // The first child is typically the function name
        if let Some(name_node) = node.child(0) {
            if matches!(name_node.kind(), "identifier" | "function_name") {
                if let Ok(func_name) = name_node.utf8_text(text.as_bytes()) {
                    let func_name = func_name.trim();

                    // Skip if it contains a dot (schema-qualified names)
                    if func_name.contains('.') {
                        return;
                    }

                    // Check if function exists in schema cache
                    if !schema_cache.function_exists(func_name) {
                        let start_pos = byte_offset_to_position(text, name_node.start_byte());
                        let end_pos = byte_offset_to_position(text, name_node.end_byte());

                        diagnostics.push(Diagnostic {
                            range: Range {
                                start: start_pos,
                                end: end_pos,
                            },
                            severity: Some(DiagnosticSeverity::ERROR),
                            code: None,
                            source: Some("octofhir-lsp".into()),
                            message: format!(
                                "Function '{}' does not exist. Check the function name or ensure it's defined in your database.",
                                func_name
                            ),
                            related_information: None,
                            tags: None,
                            code_description: None,
                            data: None,
                        });
                    }
                }
            }
        }
    }

    // Recursively visit all children
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        visit_for_function_names(child, text, schema_cache, diagnostics);
    }
}

/// Validate SELECT statements have FROM clause (if they reference columns)
fn visit_for_missing_from(
    node: tree_sitter::Node,
    text: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // Look for SELECT statements
    if node.kind() == "select_statement" || node.kind() == "select" {
        let mut has_from = false;
        let mut has_column_refs = false;
        let mut select_pos = None;

        // Check children for FROM clause and column references
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "from" || child.kind() == "from_clause" {
                has_from = true;
            }
            // Check if we have column references (not just literals or *)
            if matches!(
                child.kind(),
                "column_identifier" | "column_reference" | "any_identifier"
            ) {
                // Check if it's not a function name
                if let Some(parent) = child.parent() {
                    if parent.kind() != "function_call"
                        && parent.kind() != "invocation"
                        && parent.kind() != "function_reference"
                    {
                        has_column_refs = true;
                    }
                }
            }
            if child.kind() == "select" && select_pos.is_none() {
                select_pos = Some(child.start_byte());
            }
        }

        // If we have column references but no FROM clause, warn
        if has_column_refs && !has_from {
            if let Some(pos) = select_pos {
                let start_pos = byte_offset_to_position(text, pos);
                let end_pos = byte_offset_to_position(text, pos + 6); // "SELECT".len()

                diagnostics.push(Diagnostic {
                    range: Range {
                        start: start_pos,
                        end: end_pos,
                    },
                    severity: Some(DiagnosticSeverity::WARNING),
                    code: None,
                    source: Some("octofhir-lsp".into()),
                    message: "SELECT statement references columns but has no FROM clause. Add a FROM clause to specify the table.".to_string(),
                    related_information: None,
                    tags: None,
                    code_description: None,
                    data: None,
                });
            }
        }
    }

    // Recursively visit children
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        visit_for_missing_from(child, text, diagnostics);
    }
}

/// Validate column references against known columns
#[allow(clippy::too_many_lines)]
fn visit_for_column_references(
    node: tree_sitter::Node,
    text: &str,
    schema_cache: &SchemaCache,
    mentioned_tables: &[String],
    table_resolver: &TableResolver,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // Look for column identifiers
    if matches!(node.kind(), "column_identifier" | "any_identifier" | "identifier") {
        // Check if this is actually a column reference (not a table name or function name)
        if let Some(parent) = node.parent() {
            // Skip if it's part of a FROM clause or table reference
            if matches!(
                parent.kind(),
                "from" | "from_clause" | "table_reference" | "function_reference"
            ) {
                return;
            }

            // Special handling for function arguments
            // Check if we're inside a function call (invocation node)
            let mut is_function_arg = false;
            let mut arg_index = 0;
            let mut function_name = String::new();

            // Walk up to find if we're in a function call
            let mut current = parent;
            let mut depth = 0;
            while depth < 10 {
                if current.kind() == "invocation" || current.kind() == "function_call" {
                    is_function_arg = true;

                    // Extract function name
                    for i in 0..current.child_count() {
                        if let Some(child) = current.child(i as u32) {
                            if child.kind() == "function_reference" {
                                for j in 0..child.child_count() {
                                    if let Some(name_node) = child.child(j as u32) {
                                        if matches!(
                                            name_node.kind(),
                                            "identifier" | "any_identifier"
                                        ) {
                                            if let Ok(name) =
                                                name_node.utf8_text(text.as_bytes())
                                            {
                                                function_name = name.to_string();
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // Count which argument we are (count commas before node)
                    let node_start = node.start_byte();
                    for i in 0..current.child_count() {
                        if let Some(child) = current.child(i as u32) {
                            if child.start_byte() >= node_start {
                                break;
                            }
                            if child.kind() == "," {
                                arg_index += 1;
                            }
                        }
                    }
                    break;
                }

                if let Some(p) = current.parent() {
                    current = p;
                    depth += 1;
                } else {
                    break;
                }
            }

            // If this is the first argument of a JSONB function, validate it
            let should_validate = if is_function_arg {
                let func_lower = function_name.to_lowercase();
                // Only validate first argument (column reference) of JSONB functions
                arg_index == 0
                    && (func_lower.starts_with("jsonb_")
                        || func_lower.starts_with("json_")
                        || func_lower == "to_jsonb")
            } else {
                // Not in a function, so validate normally
                true
            };

            if !should_validate {
                return;
            }

            // Get the column name
            if let Ok(col_name) = node.utf8_text(text.as_bytes()) {
                let col_name = col_name.trim();

                // Skip special identifiers
                if col_name == "*" || col_name.is_empty() {
                    return;
                }

                // Skip SQL keywords
                let keywords = [
                    "select", "from", "where", "and", "or", "not", "in", "like", "between",
                    "null", "true", "false", "as", "on", "join", "left", "right", "inner",
                    "outer", "cross", "group", "by", "having", "order", "limit", "offset",
                ];
                if keywords
                    .iter()
                    .any(|&kw| kw.eq_ignore_ascii_case(col_name))
                {
                    return;
                }

                // Skip table aliases - check if this identifier is a known table alias
                if table_resolver.is_alias(col_name) {
                    tracing::debug!(
                        alias = col_name,
                        "Skipping column validation for table alias"
                    );
                    return;
                }

                // Check if this column exists in any of the mentioned tables
                let mut found = false;
                for table_name in mentioned_tables {
                    let columns = schema_cache.get_columns(table_name);
                    if columns.iter().any(|c| c.name == col_name) {
                        found = true;
                        break;
                    }
                }

                // If not found and we have mentioned tables, warn
                if !found && !mentioned_tables.is_empty() {
                    let start_pos = byte_offset_to_position(text, node.start_byte());
                    let end_pos = byte_offset_to_position(text, node.end_byte());

                    let table_list = mentioned_tables.join(", ");
                    let context_note = if is_function_arg {
                        format!(" (in function '{}')", function_name)
                    } else {
                        String::new()
                    };

                    diagnostics.push(Diagnostic {
                        range: Range {
                            start: start_pos,
                            end: end_pos,
                        },
                        severity: Some(DiagnosticSeverity::ERROR),
                        code: None,
                        source: Some("octofhir-lsp".into()),
                        message: format!(
                            "Column '{}' not found in table(s): {}{}. Available columns can be seen via autocomplete.",
                            col_name, table_list, context_note
                        ),
                        related_information: None,
                        tags: None,
                        code_description: None,
                        data: None,
                    });
                }
            }
        }
    }

    // Recursively visit children
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        visit_for_column_references(child, text, schema_cache, mentioned_tables, table_resolver, diagnostics);
    }
}
