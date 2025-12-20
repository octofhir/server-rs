//! Completion provider for SQL and FHIR-aware JSONB paths.
//!
//! This module provides intelligent completions for:
//! - SQL keywords (context-aware based on clause)
//! - Tables, columns, and functions from database schema
//! - FHIR element paths in JSONB expressions
//! - Function arguments with type-specific suggestions

use async_lsp::lsp_types::{CompletionItem, CompletionItemKind, Position};
use pgls_text_size::TextSize;
use pgls_treesitter::context::{TreeSitterContextParams, TreesitterContext};
use std::sync::Arc;

use crate::lsp::completion_filter::{CompletionFilter, CompletionRelevanceData};
use crate::lsp::fhir_resolver::FhirResolver;
use crate::lsp::parser::JsonbDetector;
use crate::lsp::schema_cache::SchemaCache;
use crate::lsp::server::PostgresLspServer;

/// Helper context for completion operations inside BoxFuture.
///
/// This struct holds Arc-cloned references to allow completion logic to work
/// within 'static async blocks without requiring `&self`.
#[derive(Clone)]
pub(crate) struct CompletionContext {
    pub(crate) schema_cache: Arc<SchemaCache>,
    pub(crate) fhir_resolver: Arc<FhirResolver>,
}

impl CompletionContext {
    /// Create a new completion context from server state.
    pub(crate) fn from_server(server: &PostgresLspServer) -> Self {
        Self {
            schema_cache: server.schema_cache.clone(),
            fhir_resolver: server.fhir_resolver.clone(),
        }
    }

    /// Get completions using tree-sitter parsing and schema introspection.
    ///
    /// This is the main completion entry point that delegates to specialized handlers.
    pub(crate) async fn get_completions(
        &self,
        text: &str,
        position: Position,
    ) -> Vec<CompletionItem> {
        let offset = position_to_offset(text, position);

        // Try tree-sitter based completions first
        if let Some(items) = self.get_tree_sitter_completions(text, offset).await {
            return items;
        }

        // JSONB detection now uses hybrid AST approach (tree-sitter + pg_query)
        // No regex fallback needed - AST handles all cases including incomplete SQL
        // If tree-sitter fails to parse, return empty completions instead of regex fallback
        vec![]
    }

    /// Get completions using tree-sitter AST analysis.
    async fn get_tree_sitter_completions(
        &self,
        text: &str,
        offset: usize,
    ) -> Option<Vec<CompletionItem>> {
        // Parse SQL with tree-sitter
        let mut parser = tree_sitter::Parser::new();
        if parser
            .set_language(&pgls_treesitter_grammar::LANGUAGE.into())
            .is_err()
        {
            tracing::warn!("Failed to load tree-sitter grammar");
            return None;
        }

        let tree = parser.parse(text, None)?;
        let root = tree.root_node();
        let cursor_node = root.descendant_for_byte_range(offset, offset)?;

        // Check for function call context (highest priority)
        if let Some(func_completions) = self
            .get_function_completions_if_in_call(&cursor_node, text, offset, &tree)
            .await
        {
            return Some(func_completions);
        }

        // Check for JSONB path context
        if let Some(jsonb_completions) = self
            .get_jsonb_completions(&cursor_node, text, offset, &tree)
            .await
        {
            return Some(jsonb_completions);
        }

        // General SQL completions with schema introspection
        Some(self.get_schema_completions(text, &tree, offset).await)
    }

    /// Get completions for function arguments.
    async fn get_function_completions_if_in_call(
        &self,
        cursor_node: &tree_sitter::Node<'_>,
        text: &str,
        offset: usize,
        tree: &tree_sitter::Tree,
    ) -> Option<Vec<CompletionItem>> {
        // Detect if cursor is inside a function call
        let func_ctx = PostgresLspServer::detect_function_call_context(cursor_node, text, offset)?;

        tracing::debug!(
            "Function call detected: name='{}', arg_index={}",
            func_ctx.function_name,
            func_ctx.arg_index
        );

        // Check if this is a JSONB function
        let func_name_lower = func_ctx.function_name.to_lowercase();
        if !PostgresLspServer::is_jsonb_function(&func_name_lower) {
            return None;
        }

        // Provide completions based on argument index
        let completions = match func_ctx.arg_index {
            // First argument: JSONB column names
            0 => self.get_jsonb_column_completions(tree, text),
            // Second argument: JSONPath expressions
            1 => {
                self.get_jsonpath_expression_completions(
                    tree,
                    text,
                    offset,
                    func_ctx.arg_start_offset,
                )
                .await
            }
            // Other arguments: no specific completions
            _ => Vec::new(),
        };

        Some(completions)
    }

    /// Get JSONB column completions for function arguments.
    /// Helper to extract mentioned table names from TreesitterContext.
    pub(crate) fn get_mentioned_table_names(ctx: &TreesitterContext) -> Vec<String> {
        let mut tables = Vec::new();

        // Get tables from mentioned_relations (no schema = None key)
        if let Some(table_set) = ctx.get_mentioned_relations(&None) {
            tables.extend(table_set.iter().cloned());
        }

        // Also check for tables with explicit schema
        if let Some(table_set) = ctx.get_mentioned_relations(&Some("public".to_string())) {
            tables.extend(table_set.iter().cloned());
        }

        tables
    }

    fn get_jsonb_column_completions(
        &self,
        tree: &tree_sitter::Tree,
        text: &str,
    ) -> Vec<CompletionItem> {
        // Get all tables mentioned in the query
        let ctx = TreesitterContext::new(TreeSitterContextParams {
            position: TextSize::new(0),
            text,
            tree,
        });
        let mentioned_tables = Self::get_mentioned_table_names(&ctx);

        let mut items = Vec::new();

        // Add JSONB columns from mentioned tables
        for table_name in &mentioned_tables {
            for col in self.schema_cache.get_columns(table_name) {
                // Only suggest JSONB columns
                if col.data_type.to_lowercase().contains("jsonb") {
                    items.push(CompletionItem {
                        label: col.name.clone(),
                        kind: Some(CompletionItemKind::FIELD),
                        detail: Some(format!(
                            "{}.{} ({})",
                            col.table_name, col.name, col.data_type
                        )),
                        ..Default::default()
                    });
                }
            }
        }

        items
    }

    /// Get JSONPath expression completions for function arguments.
    async fn get_jsonpath_expression_completions(
        &self,
        _tree: &tree_sitter::Tree,
        _text: &str,
        _offset: usize,
        _arg_start: usize,
    ) -> Vec<CompletionItem> {
        // Provide common JSONPath syntax examples
        vec![
            CompletionItem {
                label: "$.path".to_string(),
                kind: Some(CompletionItemKind::SNIPPET),
                detail: Some("JSONPath: root element".to_string()),
                ..Default::default()
            },
            CompletionItem {
                label: "$.path[*]".to_string(),
                kind: Some(CompletionItemKind::SNIPPET),
                detail: Some("JSONPath: all array elements".to_string()),
                ..Default::default()
            },
            CompletionItem {
                label: "$.path.to.element".to_string(),
                kind: Some(CompletionItemKind::SNIPPET),
                detail: Some("JSONPath: nested element".to_string()),
                ..Default::default()
            },
        ]
    }

    /// Detect if cursor is inside quotes and the quote context.
    ///
    /// Refactored to use the shared `find_string_node_at_cursor` helper for consistent behavior.
    fn detect_quote_context(
        cursor_node: &tree_sitter::Node<'_>,
        text: &str,
        offset: usize,
    ) -> super::super::parser::JsonbQuoteContext {
        // Use shared helper instead of inline logic
        if let Some(string_node) =
            PostgresLspServer::find_string_node_at_cursor(cursor_node, offset)
        {
            let start = string_node.start_byte();
            let end = string_node.end_byte();
            let node_text = &text[start..end];

            let has_opening_quote = node_text.starts_with('\'') || node_text.starts_with('"');
            let cursor_inside_quotes = has_opening_quote && offset > start && offset <= end;

            return super::super::parser::JsonbQuoteContext {
                has_opening_quote,
                cursor_inside_quotes,
                needs_quotes: !has_opening_quote,
            };
        }

        // Default: not in quotes, needs quotes
        super::super::parser::JsonbQuoteContext {
            has_opening_quote: false,
            cursor_inside_quotes: false,
            needs_quotes: true,
        }
    }

    /// Get JSONB path completions.
    async fn get_jsonb_completions(
        &self,
        cursor_node: &tree_sitter::Node<'_>,
        text: &str,
        offset: usize,
        tree: &tree_sitter::Tree,
    ) -> Option<Vec<CompletionItem>> {
        tracing::info!("=== get_jsonb_completions: START ===");

        // Use the JSONB expression detector
        let jsonb_ctx = match JsonbDetector::detect(cursor_node, text, offset) {
            Some(ctx) => ctx,
            None => {
                tracing::info!("=== get_jsonb_completions: JsonbDetector::detect returned None ===");
                return None;
            }
        };

        // Don't provide completions for invalid JSONB syntax
        if !jsonb_ctx.is_valid_syntax {
            tracing::debug!(
                "Skipping completions for invalid JSONB syntax: operator={}, has_right={}",
                jsonb_ctx.operator,
                jsonb_ctx.right.is_some()
            );
            return None;
        }

        tracing::debug!(
            "Found valid JSONB expression: operator={}, path={:?}",
            jsonb_ctx.operator,
            jsonb_ctx.path_chain
        );

        // Extract column name and path segments from the expression
        let (column, path_segments) = match JsonbDetector::extract_path(&jsonb_ctx.expr_node, text) {
            Some(path) => path,
            None => {
                tracing::info!("=== get_jsonb_completions: JsonbDetector::extract_path returned None ===");
                return None;
            }
        };

        tracing::debug!(
            "Extracted JSONB path: column='{}', segments={:?}",
            column,
            path_segments
        );

        // Try to find the table name from the context
        let root = tree.root_node();
        let table = match PostgresLspServer::find_table_for_column(&root, text, &column) {
            Some(t) => t,
            None => {
                tracing::info!(
                    "=== get_jsonb_completions: find_table_for_column returned None ==="
                );
                return None;
            }
        };

        tracing::info!(
            "JSONB completion context: column='{}', table='{}', path_segments={:?}",
            column,
            table,
            path_segments
        );

        // Detect actual quote context from cursor position
        let quote_context = Self::detect_quote_context(cursor_node, text, offset);

        tracing::debug!(
            "Quote context: inside_quotes={}, has_opening={}, needs_quotes={}",
            quote_context.cursor_inside_quotes,
            quote_context.has_opening_quote,
            quote_context.needs_quotes
        );

        // Extract partial path for filtering completions
        let partial_filter = JsonbDetector::extract_partial_path(&jsonb_ctx, text, cursor_node);

        if let Some(partial) = &partial_filter {
            tracing::debug!("Filtering completions with partial: '{}'", partial);
        }

        // Convert byte offset to LSP Position
        let position = position_to_lsp_position(text, offset);

        // Get FHIR path completions using the resolver
        let mut completions = self
            .get_fhir_path_completions_ctx(&table, &path_segments, &quote_context, position, text)
            .await;

        // Filter by partial input if we have it
        if let Some(partial) = partial_filter {
            if !partial.is_empty() {
                completions.retain(|item| {
                    item.label
                        .to_lowercase()
                        .starts_with(&partial.to_lowercase())
                });
                tracing::debug!(
                    "Filtered to {} completions matching '{}'",
                    completions.len(),
                    partial
                );
            }
        }

        Some(completions)
    }

    /// Get schema-based completions (tables, columns, functions).
    /// Get context-aware schema completions (tables, columns, functions, keywords).
    ///
    /// Uses tree-sitter context analysis to filter completions based on SQL clause
    /// and node type, following the Supabase postgres-language-server approach.
    async fn get_schema_completions(
        &self,
        text: &str,
        tree: &tree_sitter::Tree,
        offset: usize,
    ) -> Vec<CompletionItem> {
        use pgls_text_size::TextSize;
        use pgls_treesitter::context::{TreeSitterContextParams, TreesitterContext};

        // Create tree-sitter context for filtering
        let ctx = TreesitterContext::new(TreeSitterContextParams {
            position: TextSize::new(offset as u32),
            text,
            tree,
        });

        let mut items = Vec::new();

        // Add filtered tables
        let tables = self.schema_cache.get_tables();
        for table in &tables {
            let filter = CompletionFilter::from(CompletionRelevanceData::Table(table));
            if filter.is_relevant(&ctx) {
                items.push(Self::table_to_completion_item(table));
            }
        }

        // Add filtered columns from mentioned tables
        let mentioned_tables = Self::get_mentioned_table_names(&ctx);
        for table_name in &mentioned_tables {
            let columns = self.schema_cache.get_columns(table_name);
            for col in &columns {
                let filter = CompletionFilter::from(CompletionRelevanceData::Column(col));
                if filter.is_relevant(&ctx) {
                    items.push(Self::column_to_completion_item(col, None));
                }
            }
        }

        // Add filtered functions
        let functions = self.schema_cache.get_functions();
        for func in &functions {
            let filter = CompletionFilter::from(CompletionRelevanceData::Function(func));
            if filter.is_relevant(&ctx) {
                items.push(Self::function_to_completion_item(func));
            }
        }

        // Add context-appropriate SQL keywords
        let keywords = Self::get_keywords_for_context(&ctx);
        for kw in keywords {
            let filter = CompletionFilter::from(CompletionRelevanceData::Keyword(kw));
            if filter.is_relevant(&ctx) {
                items.push(CompletionItem {
                    label: kw.to_string(),
                    kind: Some(CompletionItemKind::KEYWORD),
                    ..Default::default()
                });
            }
        }

        tracing::debug!(
            "Schema completions: {} items at offset {} (context: node={}, clause={:?})",
            items.len(),
            offset,
            ctx.node_under_cursor.kind(),
            ctx.wrapping_clause_type
        );

        items
    }

    /// Convert TableInfo to CompletionItem.
    fn table_to_completion_item(table: &super::super::schema_cache::TableInfo) -> CompletionItem {
        let detail = if table.is_fhir_table {
            format!(
                "FHIR {} ({}.{})",
                table.fhir_resource_type.as_deref().unwrap_or("Resource"),
                table.schema,
                table.table_type
            )
        } else {
            format!("{}.{}", table.schema, table.table_type)
        };

        let sort_prefix = if table.is_fhir_table {
            "0"
        } else if table.schema == "public" {
            "1"
        } else {
            "2"
        };

        CompletionItem {
            label: table.name.clone(),
            kind: Some(CompletionItemKind::CLASS),
            detail: Some(detail),
            sort_text: Some(format!("{}{}", sort_prefix, table.name)),
            ..Default::default()
        }
    }

    /// Convert ColumnInfo to CompletionItem.
    fn column_to_completion_item(
        col: &super::super::schema_cache::ColumnInfo,
        alias: Option<&str>,
    ) -> CompletionItem {
        let label = col.name.clone();
        let detail = if let Some(alias) = alias {
            format!("{}.{} ({})", alias, col.name, col.data_type)
        } else {
            format!("{}.{} ({})", col.table_name, col.name, col.data_type)
        };

        CompletionItem {
            label,
            kind: Some(CompletionItemKind::FIELD),
            detail: Some(detail),
            ..Default::default()
        }
    }

    /// Convert FunctionInfo to CompletionItem.
    fn function_to_completion_item(
        func: &super::super::schema_cache::FunctionInfo,
    ) -> CompletionItem {
        use async_lsp::lsp_types::InsertTextFormat;

        let insert_text = Self::generate_function_snippet(&func.name, &func.signature);
        let is_jsonb_func = PostgresLspServer::is_jsonb_function(&func.name.to_lowercase());

        let detail = if is_jsonb_func {
            format!("{} [FHIR-aware]", func.signature)
        } else {
            func.signature.clone()
        };

        CompletionItem {
            label: func.name.clone(),
            kind: Some(CompletionItemKind::FUNCTION),
            detail: Some(detail),
            insert_text: Some(insert_text),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            ..Default::default()
        }
    }

    /// Generate function snippet for LSP completion.
    fn generate_function_snippet(name: &str, signature: &str) -> String {
        // Extract argument count from signature
        let arg_count = signature.matches(',').count() + 1;

        if arg_count == 0 || signature.contains("()") {
            format!("{}()", name)
        } else {
            let args: Vec<String> = (1..=arg_count)
                .map(|i| format!("${{{}:arg{}}}", i, i))
                .collect();
            format!("{}({})", name, args.join(", "))
        }
    }

    /// Get relevant SQL keywords for the current context.
    fn get_keywords_for_context(ctx: &TreesitterContext) -> Vec<&'static str> {
        use pgls_treesitter::context::WrappingClause;

        match ctx.wrapping_clause_type.as_ref() {
            Some(WrappingClause::Select) => vec![
                "SELECT", "DISTINCT", "FROM", "WHERE", "GROUP BY", "HAVING", "ORDER BY", "LIMIT",
                "OFFSET", "AS", "AND", "OR", "NOT", "IN", "LIKE",
            ],
            Some(WrappingClause::From) => vec![
                "FROM",
                "JOIN",
                "LEFT JOIN",
                "RIGHT JOIN",
                "INNER JOIN",
                "OUTER JOIN",
                "CROSS JOIN",
                "ON",
                "USING",
                // Allow transitioning to other clauses from FROM
                "WHERE",
                "GROUP BY",
                "ORDER BY",
                "LIMIT",
                "OFFSET",
            ],
            Some(WrappingClause::Where) => vec![
                "AND",
                "OR",
                "NOT",
                "IN",
                "LIKE",
                "BETWEEN",
                "IS NULL",
                "IS NOT NULL",
                "EXISTS",
                // Allow transitioning to other clauses from WHERE
                "GROUP BY",
                "ORDER BY",
                "LIMIT",
                "OFFSET",
            ],
            Some(WrappingClause::Join { .. }) => vec!["JOIN", "ON", "USING", "AND", "OR"],
            Some(WrappingClause::Insert) => vec!["INSERT INTO", "VALUES", "SELECT", "RETURNING"],
            Some(WrappingClause::Update) => vec!["UPDATE", "SET", "WHERE", "RETURNING"],
            Some(WrappingClause::Delete) => vec!["DELETE FROM", "WHERE", "RETURNING"],
            // Handle DDL and other statement types
            Some(WrappingClause::ColumnDefinitions)
            | Some(WrappingClause::AlterTable)
            | Some(WrappingClause::DropTable)
            | Some(WrappingClause::DropColumn)
            | Some(WrappingClause::AlterColumn)
            | Some(WrappingClause::RenameColumn)
            | Some(WrappingClause::SetStatement)
            | Some(WrappingClause::AlterRole)
            | Some(WrappingClause::DropRole)
            | Some(WrappingClause::RevokeStatement)
            | Some(WrappingClause::GrantStatement)
            | Some(WrappingClause::CreatePolicy)
            | Some(WrappingClause::AlterPolicy)
            | Some(WrappingClause::DropPolicy)
            | Some(WrappingClause::CheckOrUsingClause) => {
                vec!["CREATE", "ALTER", "DROP", "GRANT", "REVOKE", "SET"]
            }
            None => vec![
                "SELECT", "INSERT", "UPDATE", "DELETE", "FROM", "WHERE", "CREATE", "ALTER", "DROP",
                "WITH",
            ],
        }
    }

    /// Get completions based on regex-parsed context.
    pub(crate) async fn get_completions_for_context(
        &self,
        context: &super::super::parser::CursorContext,
        _text: &str,
        _position: Position,
    ) -> Vec<CompletionItem> {
        // Basic context-based completions
        let keywords = match context {
            super::super::parser::CursorContext::SelectColumns { .. } => {
                vec!["SELECT", "FROM", "WHERE", "GROUP BY", "ORDER BY"]
            }
            super::super::parser::CursorContext::FromClause { .. } => {
                vec!["FROM", "JOIN", "LEFT JOIN", "INNER JOIN"]
            }
            super::super::parser::CursorContext::WhereClause { .. } => {
                vec!["WHERE", "AND", "OR", "IN", "LIKE"]
            }
            _ => vec!["SELECT", "FROM", "WHERE", "INSERT", "UPDATE", "DELETE"],
        };

        keywords
            .into_iter()
            .map(|kw| CompletionItem {
                label: kw.to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                ..Default::default()
            })
            .collect()
    }

    /// Get FHIR path completions for JSONB columns.
    async fn get_fhir_path_completions_ctx(
        &self,
        table: &str,
        path: &[String],
        quote_context: &super::super::parser::JsonbQuoteContext,
        position: Position,
        document_text: &str,
    ) -> Vec<CompletionItem> {
        // Try to get resource type from table name via schema cache
        let resource_type = self
            .schema_cache
            .get_fhir_resource_type(table)
            .unwrap_or_else(|| PostgresLspServer::to_pascal_case(table));

        // Filter out numeric segments (array indices) from the path
        // FHIR paths don't include array indices: identifier[0].system -> identifier.system
        let fhir_path_segments: Vec<&String> = path
            .iter()
            .filter(|s| !s.chars().all(|c| c.is_ascii_digit()))
            .collect();

        // Build the parent path from existing path segments
        let parent_path = if fhir_path_segments.is_empty() {
            String::new()
        } else {
            fhir_path_segments
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(".")
        };

        tracing::debug!(
            "FHIR path completion: resource_type={}, parent_path='{}', raw_path={:?}",
            resource_type,
            parent_path,
            path
        );

        // Get children from FHIR resolver
        let children = self
            .fhir_resolver
            .get_children(&resource_type, &parent_path)
            .await;

        tracing::debug!(
            "FHIR resolver returned {} children for {}.{}",
            children.len(),
            resource_type,
            parent_path
        );

        // Convert to completion items
        children
            .into_iter()
            .map(|elem| {
                let kind = if elem.is_backbone {
                    CompletionItemKind::STRUCT
                } else if elem.is_array {
                    CompletionItemKind::PROPERTY
                } else {
                    CompletionItemKind::FIELD
                };

                let cardinality = if elem.max == 0 {
                    format!("{}..* (array)", elem.min)
                } else if elem.max == 1 {
                    format!("{}..1", elem.min)
                } else {
                    format!("{}..{}", elem.min, elem.max)
                };

                let detail = format!(
                    "{}: {} [{}]",
                    elem.short.as_deref().unwrap_or(""),
                    elem.type_code,
                    cardinality
                );

                let text_edit = PostgresLspServer::calculate_jsonb_text_edit(
                    &elem.name,
                    quote_context,
                    position,
                    document_text,
                );

                CompletionItem {
                    label: elem.name.clone(),
                    kind: Some(kind),
                    detail: Some(detail),
                    documentation: elem
                        .definition
                        .map(|d| async_lsp::lsp_types::Documentation::String(d)),
                    text_edit: Some(async_lsp::lsp_types::CompletionTextEdit::Edit(text_edit)),
                    ..Default::default()
                }
            })
            .collect()
    }
}

/// Helper to convert LSP Position to byte offset.
fn position_to_offset(text: &str, position: Position) -> usize {
    let mut offset = 0;
    let mut current_line = 0;
    let target_line = position.line as usize;
    let target_character = position.character as usize;

    for (i, ch) in text.char_indices() {
        if current_line == target_line {
            if offset >= target_character {
                return i;
            }
            if ch != '\n' {
                offset += 1;
            }
        }
        if ch == '\n' {
            current_line += 1;
            offset = 0;
        }
    }

    text.len()
}

/// Convert byte offset to LSP Position.
fn position_to_lsp_position(text: &str, offset: usize) -> Position {
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

    Position::new(line, character)
}
