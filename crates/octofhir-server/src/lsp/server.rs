//! PostgreSQL Language Server implementation using tower-lsp.
//!
//! Provides SQL completion with FHIR-aware JSONB path suggestions.
//! Uses tree-sitter AST parsing from Supabase postgres-language-server for
//! accurate context detection.

use dashmap::DashMap;
use std::sync::Arc;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::{
    CompletionItem, CompletionItemKind, CompletionOptions, CompletionParams, CompletionResponse,
    DidChangeTextDocumentParams, DidOpenTextDocumentParams, Hover, HoverParams,
    HoverProviderCapability, InitializeParams, InitializeResult, ServerCapabilities,
    TextDocumentSyncCapability, TextDocumentSyncKind, Url,
};
use tower_lsp::{Client, LanguageServer};

use super::completion_filter::{CompletionFilter, CompletionRelevanceData};
use super::fhir_resolver::FhirResolver;
use super::parser::{CursorContext, SqlParser};
use super::schema_cache::SchemaCache;

// Tree-sitter imports for context-aware completions
use pgls_text_size::TextSize;
use pgls_treesitter::context::{TreeSitterContextParams, TreesitterContext};

/// Context information about a function call detected at cursor position.
#[derive(Debug, Clone)]
struct FunctionCallContext {
    /// Name of the function being called (e.g., "jsonb_path_exists")
    function_name: String,
    /// Index of the argument where cursor is positioned (0-based)
    arg_index: usize,
    /// Byte offset where the current argument starts
    arg_start_offset: usize,
}

/// PostgreSQL Language Server with FHIR-aware JSONB path completion.
pub struct PostgresLspServer {
    /// LSP client for sending notifications
    client: Client,
    /// Open document contents indexed by URI
    documents: DashMap<Url, String>,
    /// Database connection pool for schema introspection
    #[allow(dead_code)]
    db_pool: Arc<sqlx_postgres::PgPool>,
    /// Schema cache for table/column information
    schema_cache: Arc<SchemaCache>,
    /// FHIR element resolver for path completions
    fhir_resolver: Arc<FhirResolver>,
}

impl PostgresLspServer {
    /// Creates a new PostgreSQL LSP server.
    pub fn new(
        client: Client,
        db_pool: Arc<sqlx_postgres::PgPool>,
        octofhir_provider: Arc<crate::model_provider::OctoFhirModelProvider>,
    ) -> Self {
        let schema_cache = Arc::new(SchemaCache::new(db_pool.clone()));

        // Create FhirResolver with model provider (which contains schemas)
        let fhir_resolver = Arc::new(FhirResolver::with_model_provider(octofhir_provider));
        Self {
            client,
            documents: DashMap::new(),
            db_pool,
            schema_cache,
            fhir_resolver,
        }
    }

    /// Get completions using tree-sitter AST context.
    ///
    /// This method uses Supabase's tree-sitter grammar and context detection
    /// for accurate clause-based filtering of completions.
    ///
    /// Returns `None` if tree-sitter can't handle this context (e.g., JSONB paths)
    /// and the caller should fall back to the regex parser.
    async fn get_completions_with_treesitter(
        &self,
        text: &str,
        offset: usize,
    ) -> Option<Vec<CompletionItem>> {
        // Parse SQL with Supabase's PostgreSQL grammar
        let mut parser = tree_sitter::Parser::new();
        if parser
            .set_language(&pgls_treesitter_grammar::LANGUAGE.into())
            .is_err()
        {
            tracing::warn!("Failed to load tree-sitter grammar, falling back to regex parser");
            return None;
        }

        let Some(tree) = parser.parse(text, None) else {
            tracing::debug!("Tree-sitter parse failed");
            return None;
        };

        // Find the node at cursor position
        let root = tree.root_node();
        let cursor_node = root.descendant_for_byte_range(offset, offset)?;

        // HIGHEST PRIORITY: Check if cursor is inside a function call
        if let Some(func_ctx) = Self::detect_function_call_context(&cursor_node, text, offset) {
            tracing::debug!(
                "Function call context: function='{}', arg_index={}",
                func_ctx.function_name,
                func_ctx.arg_index
            );
            return Some(
                self.get_function_arg_completions(func_ctx, &tree, text, offset)
                    .await,
            );
        }

        // Check if we're in a JSONB path context using tree-sitter AST
        if let Some(jsonb_completions) = self.get_jsonb_completions_from_ast(text, offset, &tree) {
            tracing::debug!("Detected JSONB path context from tree-sitter AST");
            return Some(jsonb_completions);
        }

        // Create context at cursor position for general SQL completions
        let position = TextSize::new(offset as u32);
        let ctx = TreesitterContext::new(TreeSitterContextParams {
            position,
            text,
            tree: &tree,
        });

        // Get mentioned tables from the query for column filtering
        let mentioned_tables = self.get_mentioned_table_names(&ctx);

        let mut items = Vec::new();
        let mut has_non_keyword_items = false;

        // Add tables with filtering
        for table in self.schema_cache.get_tables() {
            let filter = CompletionFilter::from(CompletionRelevanceData::Table(&table));
            if filter.is_relevant(&ctx) {
                items.push(self.table_to_completion(&table));
                has_non_keyword_items = true;
            }
        }

        // Add columns with filtering - ONLY from mentioned tables in the query
        if !mentioned_tables.is_empty() {
            for table_name in &mentioned_tables {
                for col in self.schema_cache.get_columns(table_name) {
                    let filter = CompletionFilter::from(CompletionRelevanceData::Column(&col));
                    if filter.is_relevant(&ctx) {
                        items.push(self.column_to_completion(&col, None));
                        has_non_keyword_items = true;
                    }
                }
            }
        }

        // Add functions with filtering
        for func in self.schema_cache.get_functions() {
            let filter = CompletionFilter::from(CompletionRelevanceData::Function(&func));
            if filter.is_relevant(&ctx) {
                items.push(self.function_to_completion(&func));
                has_non_keyword_items = true;
            }
        }

        // Add schemas with filtering
        for schema in self.schema_cache.get_schemas() {
            let filter = CompletionFilter::from(CompletionRelevanceData::Schema(&schema.name));
            if filter.is_relevant(&ctx) {
                // User schemas get higher sort priority
                let sort_text = if schema.is_user_schema {
                    format!("0{}", schema.name)
                } else {
                    format!("1{}", schema.name)
                };

                items.push(CompletionItem {
                    label: schema.name.clone(),
                    kind: Some(CompletionItemKind::MODULE),
                    detail: Some("Schema".to_string()),
                    sort_text: Some(sort_text),
                    ..Default::default()
                });
                has_non_keyword_items = true;
            }
        }

        // Only add keywords if we have other relevant items
        // This prevents keyword-only responses that block JSONB path detection
        if has_non_keyword_items {
            items.extend(self.get_keyword_completions());
        }

        // Filter by prefix if there's a partial identifier at cursor
        if let Some(ref identifier) = ctx.identifier_qualifiers.1 {
            if !identifier.is_empty() {
                let prefix_lower = identifier.to_lowercase();
                items.retain(|item| item.label.to_lowercase().starts_with(&prefix_lower));
            }
        }

        // Return None if no relevant items found - let regex parser handle it
        if items.is_empty() {
            return None;
        }

        Some(items)
    }

    /// Check if the cursor is in a JSONB path context (after ->, ->>, etc.)
    fn is_jsonb_path_context(&self, text: &str, offset: usize) -> bool {
        let before_cursor = &text[..offset.min(text.len())];

        // Check for JSONB operators followed by optional quote
        // Patterns: `->`, `->>`, `#>`, `#>>`, then optionally `'` and partial path
        let jsonb_patterns = [
            "->>'", "->'", "#>>'", "#>'", // With opening quote
            "->>", "->", "#>>", "#>", // Without quote (might be typing)
        ];

        // Look for patterns near the end of the text before cursor
        let check_len = 50.min(before_cursor.len());
        let recent = &before_cursor[before_cursor.len() - check_len..];

        for pattern in jsonb_patterns {
            if recent.contains(pattern) {
                // Found a JSONB operator - check if cursor is after it
                if let Some(pos) = recent.rfind(pattern) {
                    let after_op = &recent[pos + pattern.len()..];
                    // If we're in quotes or right after the operator, it's JSONB context
                    if after_op.is_empty()
                        || after_op.starts_with('\'')
                        || !after_op.contains(|c: char| c == ')' || c == ';' || c == ',')
                    {
                        return true;
                    }
                }
            }
        }

        false
    }

    /// Extract JSONB path information from tree-sitter AST and provide completions.
    ///
    /// This method checks if the cursor is in a JSONB path context by analyzing the
    /// tree-sitter AST for JSONB operators (`->`, `->>`, `#>`, `#>>`).
    ///
    /// If a JSONB context is detected, it extracts:
    /// - The column being accessed (e.g., `resource`)
    /// - The path segments (e.g., `['name', 'given']` from `resource->'name'->'given'`)
    /// - The table context from the query
    ///
    /// Returns FHIR path completions if in JSONB context, None otherwise.
    fn get_jsonb_completions_from_ast(
        &self,
        text: &str,
        offset: usize,
        tree: &tree_sitter::Tree,
    ) -> Option<Vec<CompletionItem>> {
        // Find the node at cursor position
        let root = tree.root_node();
        let cursor_node = root.descendant_for_byte_range(offset, offset)?;

        // Walk up the tree to find a binary expression with JSONB operators
        let jsonb_expr = Self::find_jsonb_expression(&cursor_node, text)?;

        tracing::debug!(
            "Found JSONB expression: {}",
            jsonb_expr.utf8_text(text.as_bytes()).unwrap_or("<invalid>")
        );

        // Extract column name and path segments from the expression
        let (column, path_segments) = Self::extract_jsonb_path(&jsonb_expr, text)?;

        tracing::debug!(
            "Extracted JSONB path: column='{}', segments={:?}",
            column,
            path_segments
        );

        // Try to find the table name from the context
        let table = Self::find_table_for_column(&root, text, &column)?;

        tracing::debug!("Resolved table for column '{}': '{}'", column, table);

        // Create a synthetic quote context (tree-sitter handles quotes differently)
        // For now, we'll assume we need to add quotes if typing a new segment
        let quote_context = super::parser::JsonbQuoteContext {
            has_opening_quote: false, // Will be determined during completion
            cursor_inside_quotes: false,
            needs_quotes: true,
        };

        // Convert to tower_lsp Position (tree-sitter uses byte offsets, LSP uses line/char)
        let position = tower_lsp::lsp_types::Position {
            line: 0, // TODO: Calculate actual line/character from offset
            character: offset as u32,
        };

        // Get FHIR path completions using the extracted information
        let rt = tokio::runtime::Runtime::new().ok()?;
        let completions = rt.block_on(self.get_fhir_path_completions(
            &table,
            &path_segments,
            &quote_context,
            position,
            text,
        ));

        Some(completions)
    }

    /// Find a JSONB expression in the AST by walking up from the cursor node.
    ///
    /// Returns the node representing the JSONB expression if found.
    fn find_jsonb_expression<'a>(
        node: &tree_sitter::Node<'a>,
        text: &str,
    ) -> Option<tree_sitter::Node<'a>> {
        let mut current = *node;

        // Walk up the tree looking for nodes with JSONB operators
        loop {
            // Check if this node or its children contain JSONB operators
            if Self::contains_jsonb_operator(&current, text) {
                return Some(current);
            }

            // Move to parent
            if let Some(parent) = current.parent() {
                current = parent;
            } else {
                break;
            }

            // Stop if we've gone too far up (e.g., reached statement level)
            if current.kind() == "statement" || current.kind() == "program" {
                break;
            }
        }

        None
    }

    /// Check if a node contains JSONB operators.
    fn contains_jsonb_operator(node: &tree_sitter::Node, text: &str) -> bool {
        // Check current node
        if node.kind() == "op_other" {
            if let Ok(op_text) = node.utf8_text(text.as_bytes()) {
                if matches!(op_text, "->" | "->>" | "#>" | "#>>" | "@>" | "<@") {
                    return true;
                }
            }
        }

        // Check immediate children
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i as u32) {
                if child.kind() == "op_other" {
                    if let Ok(op_text) = child.utf8_text(text.as_bytes()) {
                        if matches!(op_text, "->" | "->>" | "#>" | "#>>" | "@>" | "<@") {
                            return true;
                        }
                    }
                }
            }
        }

        false
    }

    /// Extract column name and path segments from a JSONB expression.
    ///
    /// Example: `resource->'name'->'given'` → ("resource", ["name", "given"])
    fn extract_jsonb_path(node: &tree_sitter::Node, text: &str) -> Option<(String, Vec<String>)> {
        let mut path_segments = Vec::new();
        let mut column_name = None;

        // Traverse the expression tree to extract segments
        Self::traverse_jsonb_expr(node, text, &mut column_name, &mut path_segments);

        let column = column_name?;
        Some((column, path_segments))
    }

    /// Recursively traverse a JSONB expression to extract column and path segments.
    fn traverse_jsonb_expr(
        node: &tree_sitter::Node,
        text: &str,
        column_name: &mut Option<String>,
        path_segments: &mut Vec<String>,
    ) {
        // Base case: identifier (column name)
        if node.kind() == "identifier" || node.kind() == "column_reference" {
            if column_name.is_none() {
                if let Ok(name) = node.utf8_text(text.as_bytes()) {
                    *column_name = Some(name.to_string());
                }
            }
            return;
        }

        // String literal (path segment like 'name')
        if node.kind() == "string" || node.kind() == "literal" {
            if let Ok(segment) = node.utf8_text(text.as_bytes()) {
                // Remove surrounding quotes
                let cleaned = segment.trim_matches('\'').trim_matches('"');
                if !cleaned.is_empty() {
                    path_segments.push(cleaned.to_string());
                }
            }
            return;
        }

        // Number (array index)
        if node.kind() == "number" || node.kind() == "integer" {
            if let Ok(num) = node.utf8_text(text.as_bytes()) {
                path_segments.push(num.to_string());
            }
            return;
        }

        // Recurse into children
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i as u32) {
                // Skip operator nodes
                if child.kind() != "op_other" {
                    Self::traverse_jsonb_expr(&child, text, column_name, path_segments);
                }
            }
        }
    }

    /// Find the table name for a given column by looking at the query context.
    ///
    /// Searches the FROM clause and JOINs to find which table contains the column.
    fn find_table_for_column(
        root: &tree_sitter::Node,
        text: &str,
        _column: &str, // TODO: Use column to resolve correct table in multi-table queries
    ) -> Option<String> {
        // Simple heuristic: find the first table in FROM clause
        // In a more complex implementation, we'd resolve column -> table mapping
        Self::find_first_table_in_from(root, text)
    }

    /// Find the first table mentioned in a FROM clause.
    fn find_first_table_in_from(node: &tree_sitter::Node, text: &str) -> Option<String> {
        // Look for "from" node
        if node.kind() == "from" {
            // Find "relation" child
            for i in 0..node.child_count() {
                if let Some(child) = node.child(i as u32) {
                    if child.kind() == "relation" || child.kind() == "identifier" {
                        if let Ok(table_name) = child.utf8_text(text.as_bytes()) {
                            // Clean up table name (remove schema prefix if present)
                            let cleaned = table_name.split('.').last().unwrap_or(table_name);
                            return Some(cleaned.to_string());
                        }
                    }
                }
            }
        }

        // Recurse into children
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i as u32) {
                if let Some(table) = Self::find_first_table_in_from(&child, text) {
                    return Some(table);
                }
            }
        }

        None
    }

    /// Detect if cursor is inside a function call and return context information.
    ///
    /// Walks up the AST from the cursor node looking for a function call, then:
    /// - Extracts the function name
    /// - Determines which argument the cursor is in (by counting commas)
    /// - Finds the byte offset where the current argument starts
    ///
    /// Returns `None` if not inside a function call.
    fn detect_function_call_context(
        node: &tree_sitter::Node,
        text: &str,
        cursor_offset: usize,
    ) -> Option<FunctionCallContext> {
        let mut current = *node;
        let mut depth = 0;

        // Walk up the tree looking for a function call
        loop {
            let kind = current.kind();

            // Check if this is a function call node
            if kind == "invocation" || kind == "function_call" {
                // Extract function name from first identifier child
                let function_name = Self::extract_function_name(&current, text)?;

                // Count commas before cursor to determine argument index
                let (arg_index, arg_start_offset) =
                    Self::find_argument_position(&current, text, cursor_offset)?;

                return Some(FunctionCallContext {
                    function_name,
                    arg_index,
                    arg_start_offset,
                });
            }

            // Move to parent node
            let Some(parent) = current.parent() else {
                return None;
            };
            current = parent;
            depth += 1;

            if depth > 20 {
                tracing::warn!("Function call detection: max depth exceeded");
                return None;
            }
        }
    }

    /// Extract function name from a function call node.
    fn extract_function_name(node: &tree_sitter::Node, text: &str) -> Option<String> {
        // Look for the function name - usually in a function_reference child
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i as u32) {
                let kind = child.kind();
                // Function name is in function_reference node
                if kind == "function_reference" {
                    // Look for identifier inside function_reference
                    for j in 0..child.child_count() {
                        if let Some(id_node) = child.child(j as u32) {
                            if id_node.kind() == "identifier" || id_node.kind() == "any_identifier"
                            {
                                let name = id_node.utf8_text(text.as_bytes()).ok()?;
                                return Some(name.to_string());
                            }
                        }
                    }
                } else if kind == "identifier"
                    || kind == "any_identifier"
                    || kind == "function_name"
                {
                    let name = child.utf8_text(text.as_bytes()).ok()?;
                    return Some(name.to_string());
                }
            }
        }
        None
    }

    /// Find which argument the cursor is in and where that argument starts.
    ///
    /// Returns `(arg_index, arg_start_offset)` where:
    /// - `arg_index` is 0 for first argument, 1 for second, etc.
    /// - `arg_start_offset` is the byte position where the argument starts
    fn find_argument_position(
        func_node: &tree_sitter::Node,
        _text: &str,
        cursor_offset: usize,
    ) -> Option<(usize, usize)> {
        // In tree-sitter grammar, arguments are direct children of invocation node
        // Structure: invocation -> function_reference, (, term, ,, term, )
        // We don't have a separate "arguments" node!

        // Find opening parenthesis
        let mut paren_pos = None;
        for i in 0..func_node.child_count() {
            if let Some(child) = func_node.child(i as u32) {
                if child.kind() == "(" {
                    paren_pos = Some(child.end_byte());
                    break;
                }
            }
        }

        let arg_start = paren_pos?;

        // Count commas before cursor to determine argument index
        let mut arg_index = 0;
        let mut last_comma_pos = arg_start;

        for i in 0..func_node.child_count() {
            if let Some(child) = func_node.child(i as u32) {
                let child_start = child.start_byte();
                let child_end = child.end_byte();

                // If we found a comma before the cursor, increment arg_index
                if child.kind() == "," && child_start < cursor_offset {
                    arg_index += 1;
                    last_comma_pos = child_end;
                }

                // If cursor is before this child, we're in the previous argument
                if child_start > cursor_offset {
                    break;
                }
            }
        }

        // The argument starts after the last comma (or after opening paren if no comma)
        let arg_start_offset = if arg_index == 0 {
            arg_start
        } else {
            last_comma_pos
        };

        Some((arg_index, arg_start_offset))
    }

    /// Get the names of tables mentioned in the query (from FROM clause, JOINs, etc.)
    fn get_mentioned_table_names(&self, ctx: &TreesitterContext) -> Vec<String> {
        let mut tables = Vec::new();

        // Get tables from mentioned_relations (no schema = None key)
        if let Some(table_set) = ctx.get_mentioned_relations(&None) {
            tables.extend(table_set.iter().cloned());
        }

        // Also check for tables with explicit schema
        // We'd need to iterate all schema keys, but for now focus on public schema
        if let Some(table_set) = ctx.get_mentioned_relations(&Some("public".to_string())) {
            tables.extend(table_set.iter().cloned());
        }

        tables
    }

    /// Convert TableInfo to CompletionItem.
    fn table_to_completion(&self, table: &super::schema_cache::TableInfo) -> CompletionItem {
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
    fn column_to_completion(
        &self,
        col: &super::schema_cache::ColumnInfo,
        alias: Option<&str>,
    ) -> CompletionItem {
        let label = col.name.clone();
        let detail = format!(
            "{} ({}) - from {}",
            col.data_type,
            if col.is_nullable {
                "nullable"
            } else {
                "not null"
            },
            alias.unwrap_or(&col.table_name)
        );

        CompletionItem {
            label,
            kind: Some(CompletionItemKind::FIELD),
            detail: Some(detail),
            sort_text: Some(format!("0{}", col.name)),
            ..Default::default()
        }
    }

    /// Convert FunctionInfo to CompletionItem.
    fn function_to_completion(&self, func: &super::schema_cache::FunctionInfo) -> CompletionItem {
        use tower_lsp::lsp_types::InsertTextFormat;

        let insert_text = Self::generate_function_snippet(&func.name, &func.signature);

        // Add [FHIR-aware] suffix for JSONB functions
        let detail = if Self::is_jsonb_function(&func.name) {
            format!("{} - {} [FHIR-aware]", func.return_type, func.description)
        } else {
            format!("{} - {}", func.return_type, func.description)
        };

        CompletionItem {
            label: func.name.clone(),
            kind: Some(CompletionItemKind::FUNCTION),
            detail: Some(detail),
            documentation: Some(tower_lsp::lsp_types::Documentation::String(format!(
                "```sql\n{}\n```",
                func.signature
            ))),
            insert_text: Some(insert_text),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            sort_text: Some(format!("1{}", func.name)),
            ..Default::default()
        }
    }

    /// Convert LSP position (line/character) to byte offset.
    fn position_to_offset(text: &str, position: tower_lsp::lsp_types::Position) -> usize {
        let mut offset = 0;
        for (line_num, line) in text.lines().enumerate() {
            if line_num == position.line as usize {
                // Found the line, add character offset
                // LSP uses UTF-16 code units, but we'll use byte offset for simplicity
                let char_offset = position.character as usize;
                let line_bytes: Vec<char> = line.chars().collect();
                let byte_offset: usize = line_bytes
                    .iter()
                    .take(char_offset.min(line_bytes.len()))
                    .map(|c| c.len_utf8())
                    .sum();
                return offset + byte_offset;
            }
            offset += line.len() + 1; // +1 for newline
        }
        offset.min(text.len())
    }

    /// Get completions based on cursor context.
    async fn get_completions_for_context(
        &self,
        context: &CursorContext,
        position: tower_lsp::lsp_types::Position,
        document_text: &str,
    ) -> Vec<CompletionItem> {
        match context {
            CursorContext::Keyword { partial } => {
                self.filter_by_prefix(self.get_keyword_completions(), partial)
            }
            CursorContext::SelectColumns {
                partial, tables, ..
            } => {
                let mut items = Vec::new();

                // Add column completions from all tables in FROM clause
                for table_ref in tables {
                    let cols = self.get_column_completions_for_table(
                        &table_ref.table,
                        table_ref.alias.as_deref(),
                    );
                    items.extend(cols);
                }

                items.extend(self.get_function_completions());
                items.extend(self.get_keyword_completions());
                self.filter_by_prefix(items, partial)
            }
            CursorContext::FromClause { partial } => {
                // Add table and schema completions (like Supabase LSP)
                let mut items = self.get_table_completions();
                items.extend(self.get_schema_completions());
                items.extend(self.get_keyword_completions());
                self.filter_by_prefix(items, partial)
            }
            CursorContext::SchemaTableAccess { schema, partial } => {
                // Show only tables in the specified schema
                let tables = self.get_tables_in_schema_completions(schema);
                self.filter_by_prefix(tables, partial)
            }
            CursorContext::AliasColumnAccess {
                table,
                alias,
                partial,
                ..
            } => {
                // Show columns for the aliased table
                let items = self.get_column_completions_for_table(table, Some(alias));
                self.filter_by_prefix(items, partial)
            }
            CursorContext::WhereClause {
                partial, tables, ..
            } => {
                let mut items = Vec::new();

                // Add column completions from all tables
                for table_ref in tables {
                    let cols = self.get_column_completions_for_table(
                        &table_ref.table,
                        table_ref.alias.as_deref(),
                    );
                    items.extend(cols);
                }

                items.extend(self.get_keyword_completions());
                items.extend(self.get_function_completions());
                items.extend(self.get_jsonb_operator_completions());
                self.filter_by_prefix(items, partial)
            }
            CursorContext::JsonbPath {
                table,
                path,
                tables,
                quote_context,
                ..
            } => {
                // Get FHIR path completions from canonical manager
                // If table is empty, try to infer from the first table in FROM clause
                let resolved_table = if table.is_empty() && !tables.is_empty() {
                    // Use the first table from FROM clause
                    tables[0].table.clone()
                } else {
                    table.clone()
                };
                self.get_fhir_path_completions(
                    &resolved_table,
                    path,
                    quote_context,
                    position,
                    document_text,
                )
                .await
            }
            CursorContext::FunctionArgs { function, .. } => {
                // Return type hints for function arguments
                self.get_function_arg_hints(function)
            }
            CursorContext::GrantRole { partial } => {
                // Show role completions for GRANT ... TO
                let items = self.get_role_completions();
                self.filter_by_prefix(items, partial)
            }
            CursorContext::GrantTable { partial } => {
                // Show table completions for GRANT ON / POLICY ON
                let mut items = self.get_table_completions();
                items.extend(self.get_schema_completions());
                self.filter_by_prefix(items, partial)
            }
            CursorContext::PolicyDefinition { table, partial } => {
                // Show policy-related completions
                let mut items = Vec::new();

                // If we have a table, show policies for that table
                if let Some(table_name) = table {
                    let policies = self.schema_cache.get_policies_for_table(table_name);
                    for policy in policies {
                        items.push(CompletionItem {
                            label: policy.name.clone(),
                            kind: Some(CompletionItemKind::EVENT),
                            detail: Some(format!(
                                "Policy ({}, {})",
                                policy.command,
                                if policy.permissive {
                                    "PERMISSIVE"
                                } else {
                                    "RESTRICTIVE"
                                }
                            )),
                            ..Default::default()
                        });
                    }
                } else {
                    // Show all policies
                    items.extend(self.get_policy_completions());
                }

                // Also show roles for TO clause
                items.extend(self.get_role_completions());
                items.extend(self.get_keyword_completions());
                self.filter_by_prefix(items, partial)
            }
            CursorContext::CastType { partial } => {
                // Show type completions for CAST ... AS
                let mut items = self.get_type_completions();
                // Also add built-in type keywords
                items.extend(self.get_keyword_completions());
                self.filter_by_prefix(items, partial)
            }
            CursorContext::Unknown { partial } => {
                // Include all completions for unknown context
                let mut items = self.get_keyword_completions();
                items.extend(self.get_table_completions()); // Tables are useful everywhere
                items.extend(self.get_function_completions());
                items.extend(self.get_jsonb_operator_completions());
                self.filter_by_prefix(items, partial)
            }
        }
    }

    /// Get column completions for a specific table, optionally prefixed with alias.
    fn get_column_completions_for_table(
        &self,
        table_name: &str,
        alias: Option<&str>,
    ) -> Vec<CompletionItem> {
        self.schema_cache
            .get_columns(table_name)
            .into_iter()
            .map(|col| {
                let label = col.name.clone();
                let detail = format!(
                    "{} ({}) - from {}",
                    col.data_type,
                    if col.is_nullable {
                        "nullable"
                    } else {
                        "not null"
                    },
                    alias.unwrap_or(table_name)
                );

                CompletionItem {
                    label,
                    kind: Some(CompletionItemKind::FIELD),
                    detail: Some(detail),
                    // Sort columns higher than keywords
                    sort_text: Some(format!("0{}", col.name)),
                    ..Default::default()
                }
            })
            .collect()
    }

    /// Filter completion items by prefix.
    fn filter_by_prefix(&self, items: Vec<CompletionItem>, prefix: &str) -> Vec<CompletionItem> {
        if prefix.is_empty() {
            return items;
        }
        let prefix_lower = prefix.to_lowercase();
        items
            .into_iter()
            .filter(|item| item.label.to_lowercase().starts_with(&prefix_lower))
            .collect()
    }

    /// Get schema completions from schema cache.
    fn get_schema_completions(&self) -> Vec<CompletionItem> {
        self.schema_cache
            .get_schemas()
            .into_iter()
            .map(|schema| {
                // User schemas get higher sort priority
                let sort_text = if schema.is_user_schema {
                    format!("0{}", schema.name)
                } else {
                    format!("1{}", schema.name)
                };

                CompletionItem {
                    label: schema.name.clone(),
                    kind: Some(CompletionItemKind::MODULE),
                    detail: Some("Schema".to_string()),
                    sort_text: Some(sort_text),
                    ..Default::default()
                }
            })
            .collect()
    }

    /// Get table completions for a specific schema.
    fn get_tables_in_schema_completions(&self, schema: &str) -> Vec<CompletionItem> {
        self.schema_cache
            .get_tables_in_schema(schema)
            .into_iter()
            .map(|table| {
                let detail = if table.is_fhir_table {
                    format!(
                        "FHIR {}",
                        table.fhir_resource_type.as_deref().unwrap_or("Resource")
                    )
                } else {
                    table.table_type.clone()
                };

                CompletionItem {
                    label: table.name.clone(),
                    kind: Some(CompletionItemKind::CLASS),
                    detail: Some(detail),
                    ..Default::default()
                }
            })
            .collect()
    }

    /// Get table completions from schema cache.
    fn get_table_completions(&self) -> Vec<CompletionItem> {
        self.schema_cache
            .get_tables()
            .into_iter()
            .map(|table| {
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

                // Sorting: FHIR tables first (0), then public schema (1), then others (2)
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
            })
            .collect()
    }

    /// Get column completions for a specific table.
    #[allow(dead_code)]
    fn get_column_completions(&self, table_name: &str) -> Vec<CompletionItem> {
        self.schema_cache
            .get_columns(table_name)
            .into_iter()
            .map(|col| {
                let detail = format!(
                    "{} ({})",
                    col.data_type,
                    if col.is_nullable {
                        "nullable"
                    } else {
                        "not null"
                    }
                );

                CompletionItem {
                    label: col.name.clone(),
                    kind: Some(CompletionItemKind::FIELD),
                    detail: Some(detail),
                    ..Default::default()
                }
            })
            .collect()
    }

    /// Get policy completions from schema cache.
    fn get_policy_completions(&self) -> Vec<CompletionItem> {
        self.schema_cache
            .get_policies()
            .into_iter()
            .map(|policy| {
                let cmd_display = if policy.command == "ALL" {
                    "all".to_string()
                } else {
                    policy.command.to_lowercase()
                };
                let perm_display = if policy.permissive {
                    "PERMISSIVE"
                } else {
                    "RESTRICTIVE"
                };
                let detail = format!(
                    "Policy on {}.{} ({}, {})",
                    policy.schema, policy.table_name, cmd_display, perm_display
                );

                CompletionItem {
                    label: policy.name.clone(),
                    kind: Some(CompletionItemKind::EVENT), // Use EVENT kind for policies
                    detail: Some(detail),
                    sort_text: Some(format!("3{}", policy.name)), // Policies after tables and columns
                    ..Default::default()
                }
            })
            .collect()
    }

    /// Get role completions from schema cache.
    fn get_role_completions(&self) -> Vec<CompletionItem> {
        self.schema_cache
            .get_roles()
            .into_iter()
            .map(|role| {
                let mut attrs = Vec::new();
                if role.is_superuser {
                    attrs.push("superuser");
                }
                if role.can_login {
                    attrs.push("login");
                }
                if role.create_db {
                    attrs.push("createdb");
                }
                if role.create_role {
                    attrs.push("createrole");
                }
                let detail = if attrs.is_empty() {
                    "Role".to_string()
                } else {
                    format!("Role ({})", attrs.join(", "))
                };

                CompletionItem {
                    label: role.name.clone(),
                    kind: Some(CompletionItemKind::REFERENCE), // Use REFERENCE kind for roles
                    detail: Some(detail),
                    sort_text: Some(format!("4{}", role.name)), // Roles after policies
                    ..Default::default()
                }
            })
            .collect()
    }

    /// Get type completions from schema cache.
    fn get_type_completions(&self) -> Vec<CompletionItem> {
        self.schema_cache
            .get_types()
            .into_iter()
            .map(|type_info| {
                let detail = match type_info.category.as_str() {
                    "Enum" => {
                        if type_info.enum_labels.len() <= 5 {
                            format!("Enum: {}", type_info.enum_labels.join(", "))
                        } else {
                            format!(
                                "Enum: {}, ... ({} values)",
                                type_info.enum_labels[..3].join(", "),
                                type_info.enum_labels.len()
                            )
                        }
                    }
                    _ => format!("{} type", type_info.category),
                };

                CompletionItem {
                    label: type_info.name.clone(),
                    kind: Some(CompletionItemKind::TYPE_PARAMETER),
                    detail: Some(detail),
                    sort_text: Some(format!("5{}", type_info.name)), // Types after roles
                    ..Default::default()
                }
            })
            .collect()
    }

    /// Refresh the schema cache.
    async fn refresh_schema_cache(&self) {
        if let Err(e) = self.schema_cache.refresh().await {
            tracing::warn!(error = %e, "Failed to refresh LSP schema cache");
            self.client
                .log_message(
                    tower_lsp::lsp_types::MessageType::WARNING,
                    format!("Failed to refresh schema cache: {}", e),
                )
                .await;
        } else {
            tracing::info!("LSP schema cache refreshed successfully");
        }
    }

    /// Calculate TextEdit for JSONB path completion with smart quote handling.
    fn calculate_jsonb_text_edit(
        element_name: &str,
        quote_context: &super::parser::JsonbQuoteContext,
        position: tower_lsp::lsp_types::Position,
        document_text: &str,
    ) -> tower_lsp::lsp_types::TextEdit {
        use tower_lsp::lsp_types::{Position, Range, TextEdit};

        let line_text = document_text
            .lines()
            .nth(position.line as usize)
            .unwrap_or("");

        let cursor_char = position.character as usize;

        // Convert character position to byte position (handles Unicode properly)
        let cursor_byte = Self::char_to_byte_index(line_text, cursor_char);

        let (start_char, end_char, new_text) = if quote_context.cursor_inside_quotes {
            // Inside quotes: For JSONPath like '$.|' or '$.name.|', keep the path prefix
            // Find the last segment separator (. or [) and replace only the current segment
            let before_cursor = &line_text[..cursor_byte];

            // Find where the current segment starts (after last . or [)
            let segment_start_byte = before_cursor
                .rfind(|c| c == '.' || c == '[')
                .map(|p| p + 1) // Start after the separator
                .or_else(|| {
                    // No separator found, start after opening quote
                    before_cursor.rfind('\'').map(|p| p + 1)
                })
                .unwrap_or(cursor_byte);

            let segment_start_char = Self::byte_to_char_index(line_text, segment_start_byte);

            // Find where to stop replacing (end of segment or closing quote)
            let after_cursor = &line_text[cursor_byte..];
            let segment_end_byte = after_cursor
                .find(|c: char| c == '.' || c == '[' || c == ']' || c == '\'' || c.is_whitespace())
                .map(|p| cursor_byte + p)
                .unwrap_or(cursor_byte);

            let segment_end_char = Self::byte_to_char_index(line_text, segment_end_byte);

            (
                segment_start_char,
                segment_end_char,
                element_name.to_string(),
            )
        } else if quote_context.has_opening_quote {
            // After opening quote: resource->'| → insert without quotes
            (cursor_char, cursor_char, element_name.to_string())
        } else if quote_context.needs_quotes {
            // No quotes but needs them: resource->| → insert with quotes
            (cursor_char, cursor_char, format!("'{}'", element_name))
        } else {
            // No quotes and doesn't need them (e.g., array path syntax): resource #> '{name,| → insert without quotes
            (cursor_char, cursor_char, element_name.to_string())
        };

        TextEdit {
            range: Range {
                start: Position {
                    line: position.line,
                    character: start_char as u32,
                },
                end: Position {
                    line: position.line,
                    character: end_char as u32,
                },
            },
            new_text,
        }
    }

    /// Convert character index to byte index (handles Unicode properly)
    fn char_to_byte_index(text: &str, char_index: usize) -> usize {
        text.char_indices()
            .nth(char_index)
            .map(|(byte_idx, _)| byte_idx)
            .unwrap_or(text.len())
    }

    /// Convert byte index to character index (handles Unicode properly)
    fn byte_to_char_index(text: &str, byte_index: usize) -> usize {
        text.char_indices()
            .take_while(|(byte_idx, _)| *byte_idx < byte_index)
            .count()
    }

    /// Get FHIR path completions from the canonical manager.
    ///
    /// This method resolves FHIR element paths based on the table name (which maps
    /// to a resource type) and the current path context.
    ///
    /// Array indices in the path (like `identifier->0->system`) are filtered out
    /// when building the FHIR path, since FHIR paths don't include array indices.
    async fn get_fhir_path_completions(
        &self,
        table: &str,
        path: &[String],
        quote_context: &super::parser::JsonbQuoteContext,
        position: tower_lsp::lsp_types::Position,
        document_text: &str,
    ) -> Vec<CompletionItem> {
        // Try to get resource type from table name via schema cache
        let resource_type = self
            .schema_cache
            .get_fhir_resource_type(table)
            .unwrap_or_else(|| {
                // Fall back to PascalCase conversion
                Self::to_pascal_case(table)
            });

        // Filter out numeric segments (array indices) from the path
        // FHIR paths don't include array indices: identifier[0].system -> identifier.system
        let fhir_path_segments: Vec<&String> = path
            .iter()
            .filter(|s| !s.chars().all(|c| c.is_ascii_digit()))
            .collect();

        // Build the parent path from existing path segments
        // For nested paths like ['name', 'given'], we just pass 'name.given' to the resolver
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

                let text_edit = Self::calculate_jsonb_text_edit(
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
                        .map(|d| tower_lsp::lsp_types::Documentation::String(d)),
                    text_edit: Some(tower_lsp::lsp_types::CompletionTextEdit::Edit(text_edit)),
                    ..Default::default()
                }
            })
            .collect()
    }

    /// Route function argument completions based on function name and argument index.
    ///
    /// Provides context-aware completions for JSONB function arguments:
    /// - Arg 0: JSONB column names (for target argument)
    /// - Arg 1: JSONPath expressions (for path argument)
    /// - Arg 2+: Parameter hints (vars, silent flags)
    async fn get_function_arg_completions(
        &self,
        ctx: FunctionCallContext,
        tree: &tree_sitter::Tree,
        text: &str,
        cursor_offset: usize,
    ) -> Vec<CompletionItem> {
        let func_name_lower = ctx.function_name.to_lowercase();

        tracing::debug!(
            "Getting argument completions for function='{}', arg_index={}",
            func_name_lower,
            ctx.arg_index
        );

        // Check if this is a JSONB function
        if !Self::is_jsonb_function(&func_name_lower) {
            tracing::debug!("Not a JSONB function, returning empty");
            return Vec::new();
        }

        match ctx.arg_index {
            // First argument: JSONB column completions
            0 => {
                tracing::debug!("Providing JSONB column completions");
                self.get_jsonb_column_completions(tree, text)
            }
            // Second argument: JSONPath expression completions
            1 => {
                tracing::debug!("Providing JSONPath expression completions");
                self.get_jsonpath_expression_completions(
                    tree,
                    text,
                    cursor_offset,
                    ctx.arg_start_offset,
                )
                .await
            }
            // Other arguments: could add hints for vars, silent, etc. in the future
            _ => {
                tracing::debug!("No completions for argument index {}", ctx.arg_index);
                Vec::new()
            }
        }
    }

    /// Check if a function name is a JSONB function.
    ///
    /// Includes path query functions, manipulation functions, and other JSONB functions.
    fn is_jsonb_function(name: &str) -> bool {
        let name_lower = name.to_lowercase();
        matches!(
            name_lower.as_str(),
            // Path query functions
            "jsonb_path_exists"
                | "jsonb_path_match"
                | "jsonb_path_query"
                | "jsonb_path_query_array"
                | "jsonb_path_query_first"
                // Manipulation functions
                | "jsonb_set"
                | "jsonb_insert"
                | "jsonb_delete"
                | "jsonb_set_lax"
                // Other JSONB functions
                | "jsonb_build_array"
                | "jsonb_build_object"
                | "jsonb_object"
                | "jsonb_agg"
                | "jsonb_object_agg"
                | "jsonb_array_elements"
                | "jsonb_array_elements_text"
                | "jsonb_array_length"
                | "jsonb_each"
                | "jsonb_each_text"
                | "jsonb_extract_path"
                | "jsonb_extract_path_text"
                | "jsonb_populate_record"
                | "jsonb_populate_recordset"
                | "jsonb_to_record"
                | "jsonb_to_recordset"
                | "jsonb_strip_nulls"
                | "jsonb_typeof"
                | "jsonb_pretty"
        )
    }

    /// Get JSONB column completions from all tables in the FROM clause.
    ///
    /// Extracts table names from the query and suggests only JSONB-typed columns.
    fn get_jsonb_column_completions(
        &self,
        tree: &tree_sitter::Tree,
        text: &str,
    ) -> Vec<CompletionItem> {
        let root = tree.root_node();

        // Find all tables from FROM clause
        let tables = Self::extract_tables_from_ast(&root, text);

        tracing::info!(
            "Column completion: Found {} tables in FROM clause: {:?}",
            tables.len(),
            tables
        );

        let mut items = Vec::new();

        // For each table, get JSONB columns
        for table_name in tables {
            let jsonb_columns = self.schema_cache.get_jsonb_columns(&table_name);

            tracing::info!(
                "Table '{}' has {} JSONB columns",
                table_name,
                jsonb_columns.len()
            );

            for col in jsonb_columns {
                items.push(CompletionItem {
                    label: col.name.clone(),
                    kind: Some(CompletionItemKind::FIELD),
                    detail: Some(format!("jsonb - JSONB column from {}", table_name)),
                    sort_text: Some(format!("0{}", col.name)), // High priority
                    insert_text: Some(col.name.clone()), // Explicit insert text
                    documentation: Some(tower_lsp::lsp_types::Documentation::String(format!(
                        "JSONB column from table '{}'\n\nUsage example:\nSELECT jsonb_path_exists({}, '$.property') FROM {}",
                        table_name, col.name, table_name
                    ))),
                    ..Default::default()
                });
            }
        }

        tracing::info!("Returning {} JSONB column completions", items.len());
        items
    }

    /// Extract table names from FROM clause using AST.
    fn extract_tables_from_ast(node: &tree_sitter::Node, text: &str) -> Vec<String> {
        let mut tables = Vec::new();

        // Recursively search for FROM clause and table references
        if node.kind() == "from" || node.kind() == "from_clause" {
            // Look for table names in FROM clause
            for i in 0..node.child_count() {
                if let Some(child) = node.child(i as u32) {
                    Self::extract_table_names_recursive(&child, text, &mut tables);
                }
            }
        } else {
            // Continue searching in children
            for i in 0..node.child_count() {
                if let Some(child) = node.child(i as u32) {
                    let child_tables = Self::extract_tables_from_ast(&child, text);
                    tables.extend(child_tables);
                }
            }
        }

        tables
    }

    /// Recursively extract table names from AST nodes.
    fn extract_table_names_recursive(
        node: &tree_sitter::Node,
        text: &str,
        tables: &mut Vec<String>,
    ) {
        let kind = node.kind();

        // Table reference nodes contain the actual table name
        // From tree-sitter output: table_reference -> any_identifier
        // Note: We check for "table_reference" which is specific to table names,
        // not "object_reference" which can be column references too
        if kind == "relation" || kind == "table_reference" {
            // Look for identifier children
            for i in 0..node.child_count() {
                if let Some(child) = node.child(i as u32) {
                    if child.kind() == "identifier" || child.kind() == "any_identifier" {
                        if let Ok(table_name) = child.utf8_text(text.as_bytes()) {
                            tables.push(table_name.to_string());
                            tracing::debug!("Extracted table name: {}", table_name);
                        }
                    }
                }
            }
        }

        // Recurse into children
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i as u32) {
                Self::extract_table_names_recursive(&child, text, tables);
            }
        }
    }

    /// Get JSONPath expression completions for JSONB function path argument.
    ///
    /// Parses the JSONPath string, resolves the FHIR resource type, and provides
    /// FHIR-aware property suggestions.
    async fn get_jsonpath_expression_completions(
        &self,
        tree: &tree_sitter::Tree,
        text: &str,
        cursor_offset: usize,
        arg_start_offset: usize,
    ) -> Vec<CompletionItem> {
        // Extract and parse the JSONPath string
        let arg_text = &text[arg_start_offset..cursor_offset];
        let (path_segments, quote_context) = Self::parse_jsonpath_string(arg_text);

        tracing::info!(
            "Parsed JSONPath: segments={:?}, quote_ctx=(inside_quotes={}, has_opening={}, needs_quotes={})",
            path_segments,
            quote_context.cursor_inside_quotes,
            quote_context.has_opening_quote,
            quote_context.needs_quotes
        );

        // Extract column name from first function argument
        let root = tree.root_node();
        let cursor_node = root
            .descendant_for_byte_range(cursor_offset, cursor_offset)
            .unwrap_or(root);

        let Some(column_name) = Self::extract_first_arg_column(&cursor_node, text) else {
            tracing::debug!("Could not extract column from first arg");
            return Vec::new();
        };

        tracing::debug!("Extracted column from first arg: {}", column_name);

        // Find table that has this column
        let tables = Self::extract_tables_from_ast(&root, text);
        let mut table_name = None;

        for table in &tables {
            let columns = self.schema_cache.get_columns(table);
            if columns.iter().any(|c| c.name == column_name) {
                table_name = Some(table.clone());
                break;
            }
        }

        let Some(table) = table_name else {
            tracing::debug!("Could not resolve column '{}' to any table", column_name);
            return Vec::new();
        };

        tracing::debug!("Resolved column '{}' to table '{}'", column_name, table);

        // Get FHIR resource type from table
        let resource_type = self
            .schema_cache
            .get_fhir_resource_type(&table)
            .unwrap_or_else(|| Self::to_pascal_case(&table));

        tracing::debug!(
            "Resolved table '{}' to resource type '{}'",
            table,
            resource_type
        );

        // Calculate correct line/character position from byte offset
        let position = Self::offset_to_position(text, cursor_offset);

        tracing::debug!(
            "Cursor position: line={}, character={}, byte_offset={}",
            position.line,
            position.character,
            cursor_offset
        );

        // We're already in an async context, so just await directly
        let completions = self
            .get_fhir_path_completions(&table, &path_segments, &quote_context, position, text)
            .await;

        // Debug: log first few completion items to diagnose UI issues
        for (i, item) in completions.iter().take(3).enumerate() {
            tracing::info!(
                "Completion[{}]: label='{}', text_edit={:?}",
                i,
                item.label,
                item.text_edit.as_ref().map(|te| match te {
                    tower_lsp::lsp_types::CompletionTextEdit::Edit(edit) => format!(
                        "range={}:{}-{}:{} text='{}'",
                        edit.range.start.line,
                        edit.range.start.character,
                        edit.range.end.line,
                        edit.range.end.character,
                        edit.new_text
                    ),
                    _ => "InsertReplace".to_string(),
                })
            );
        }

        completions
    }

    /// Convert byte offset to LSP Position (line/character).
    fn offset_to_position(text: &str, offset: usize) -> tower_lsp::lsp_types::Position {
        let mut line = 0;
        let mut line_start = 0;

        for (i, ch) in text.char_indices() {
            if i >= offset {
                break;
            }
            if ch == '\n' {
                line += 1;
                line_start = i + 1;
            }
        }

        // Calculate character position on the line (not byte position!)
        let line_text = &text[line_start..];
        let bytes_into_line = offset.saturating_sub(line_start);
        let character = line_text
            .char_indices()
            .take_while(|(byte_idx, _)| *byte_idx < bytes_into_line)
            .count();

        tower_lsp::lsp_types::Position {
            line: line as u32,
            character: character as u32,
        }
    }

    /// Parse a JSONPath string from function argument.
    ///
    /// Extracts path segments and determines quote context.
    /// Supports array indexing: `$.name[0].given` → `["name", "0", "given"]`
    ///
    /// Returns `(path_segments, quote_context)`
    fn parse_jsonpath_string(arg_text: &str) -> (Vec<String>, super::parser::JsonbQuoteContext) {
        let trimmed = arg_text.trim();

        // Check for opening quote
        let has_opening_quote = trimmed.starts_with('\'') || trimmed.starts_with('"');
        let quote_char = if trimmed.starts_with('\'') {
            '\''
        } else if trimmed.starts_with('"') {
            '"'
        } else {
            '\0'
        };

        // Extract content between quotes (or rest of string if no quotes)
        let content = if has_opening_quote {
            let start_idx = 1;
            // Find closing quote
            if let Some(end_idx) = trimmed[start_idx..].find(quote_char) {
                &trimmed[start_idx..start_idx + end_idx]
            } else {
                // No closing quote - user is still typing
                &trimmed[start_idx..]
            }
        } else {
            trimmed
        };

        // Remove $. prefix if present
        let path_content = content.strip_prefix("$.").unwrap_or(content);
        let path_content = path_content.strip_prefix('$').unwrap_or(path_content);

        // Split by dots and extract segments (including array indices)
        let mut segments = Vec::new();

        if !path_content.is_empty() {
            let mut current_segment = String::new();
            let mut in_brackets = false;

            for ch in path_content.chars() {
                match ch {
                    '.' if !in_brackets => {
                        if !current_segment.is_empty() {
                            segments.push(current_segment.clone());
                            current_segment.clear();
                        }
                    }
                    '[' => {
                        if !current_segment.is_empty() {
                            segments.push(current_segment.clone());
                            current_segment.clear();
                        }
                        in_brackets = true;
                    }
                    ']' => {
                        if !current_segment.is_empty() {
                            segments.push(current_segment.clone());
                            current_segment.clear();
                        }
                        in_brackets = false;
                    }
                    _ => {
                        current_segment.push(ch);
                    }
                }
            }

            // Add final segment if present and not empty
            if !current_segment.is_empty() {
                segments.push(current_segment);
            }
        }

        tracing::debug!(
            "JSONPath parsing: input='{}', segments={:?}",
            arg_text,
            segments
        );

        let cursor_inside_quotes = has_opening_quote
            && !trimmed[1..]
                .chars()
                .take(trimmed.len() - 1)
                .any(|c| c == quote_char);

        let quote_context = super::parser::JsonbQuoteContext {
            has_opening_quote,
            cursor_inside_quotes,
            needs_quotes: !has_opening_quote,
        };

        (segments, quote_context)
    }

    /// Extract column name from first function argument.
    ///
    /// Walks up to find the function call, then extracts the identifier from the first argument.
    fn extract_first_arg_column(node: &tree_sitter::Node, text: &str) -> Option<String> {
        // Walk up to find the function call node
        let mut current = *node;
        loop {
            let kind = current.kind();
            if kind == "invocation" || kind == "function_call" {
                break;
            }
            current = current.parent()?;
        }

        // Arguments are direct children of invocation node
        // Find first argument (skip function_reference and opening paren)
        let mut found_paren = false;
        for i in 0..current.child_count() {
            if let Some(child) = current.child(i as u32) {
                let kind = child.kind();

                // Skip until we find the opening paren
                if kind == "(" {
                    found_paren = true;
                    continue;
                }

                // Skip punctuation
                if kind == ")" || kind == "," || kind == "function_reference" {
                    continue;
                }

                // First argument after opening paren
                if found_paren {
                    return Self::extract_identifier_from_expression(&child, text);
                }
            }
        }

        None
    }

    /// Extract identifier from an expression (handles simple identifiers and qualified names).
    fn extract_identifier_from_expression(node: &tree_sitter::Node, text: &str) -> Option<String> {
        let kind = node.kind();

        // Direct identifier (tree-sitter uses "any_identifier" in pgls grammar)
        if kind == "identifier" || kind == "any_identifier" {
            return node.utf8_text(text.as_bytes()).ok().map(String::from);
        }

        // Qualified name like "table.column" - extract the last part
        if kind == "qualified_name" || kind == "field_reference" || kind == "column_reference" {
            // Look for the last identifier child
            for i in (0..node.child_count()).rev() {
                if let Some(child) = node.child(i as u32) {
                    if child.kind() == "identifier" || child.kind() == "any_identifier" {
                        return child.utf8_text(text.as_bytes()).ok().map(String::from);
                    }
                }
            }
        }

        // Recurse into children
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i as u32) {
                if let Some(id) = Self::extract_identifier_from_expression(&child, text) {
                    return Some(id);
                }
            }
        }

        None
    }

    /// Convert a table name to PascalCase for FHIR resource type matching.
    fn to_pascal_case(s: &str) -> String {
        let mut result = String::new();
        let mut capitalize_next = true;

        for c in s.chars() {
            if c == '_' || c == '-' {
                capitalize_next = true;
            } else if capitalize_next {
                result.push(c.to_ascii_uppercase());
                capitalize_next = false;
            } else {
                result.push(c.to_ascii_lowercase());
            }
        }

        result
    }

    /// Get hints for function arguments.
    fn get_function_arg_hints(&self, function: &str) -> Vec<CompletionItem> {
        // Return type hints based on function
        let hints: Vec<(&str, &str)> = match function.to_lowercase().as_str() {
            "jsonb_extract_path" | "jsonb_extract_path_text" => {
                vec![("'path_element'", "Path element string")]
            }
            "jsonb_set" | "jsonb_insert" => {
                vec![("'{path}'", "JSONB path array"), ("'value'", "New value")]
            }
            _ => vec![],
        };

        hints
            .into_iter()
            .map(|(hint, desc)| CompletionItem {
                label: hint.to_string(),
                kind: Some(CompletionItemKind::SNIPPET),
                detail: Some(desc.to_string()),
                ..Default::default()
            })
            .collect()
    }

    /// Get SQL keyword completions with context-aware sorting.
    fn get_keyword_completions(&self) -> Vec<CompletionItem> {
        const SQL_KEYWORDS: &[(&str, &str, &str)] = &[
            // Statement starters (high priority)
            ("SELECT", "Select columns from table", "0"),
            ("INSERT INTO", "Insert data", "0"),
            ("UPDATE", "Update data", "0"),
            ("DELETE FROM", "Delete data", "0"),
            ("WITH", "Common table expression", "0"),
            // Clause keywords (medium priority)
            ("FROM", "Specify source table", "1"),
            ("WHERE", "Filter rows", "1"),
            ("JOIN", "Join tables", "1"),
            ("LEFT JOIN", "Left outer join", "1"),
            ("RIGHT JOIN", "Right outer join", "1"),
            ("INNER JOIN", "Inner join", "1"),
            ("FULL OUTER JOIN", "Full outer join", "1"),
            ("CROSS JOIN", "Cross join", "1"),
            ("ON", "Join condition", "1"),
            ("ORDER BY", "Sort results", "1"),
            ("GROUP BY", "Group rows", "1"),
            ("HAVING", "Filter groups", "1"),
            ("LIMIT", "Limit result count", "1"),
            ("OFFSET", "Skip rows", "1"),
            ("UNION", "Combine results", "1"),
            ("UNION ALL", "Combine all results", "1"),
            ("EXCEPT", "Subtract results", "1"),
            ("INTERSECT", "Intersect results", "1"),
            ("RETURNING", "Return modified rows", "1"),
            // Modifiers (medium-low priority)
            ("DISTINCT", "Remove duplicates", "2"),
            ("ALL", "Include all", "2"),
            ("AS", "Alias", "2"),
            ("ASC", "Ascending order", "2"),
            ("DESC", "Descending order", "2"),
            ("NULLS FIRST", "Nulls first in ordering", "2"),
            ("NULLS LAST", "Nulls last in ordering", "2"),
            // Logical operators (lower priority)
            ("AND", "Logical AND", "3"),
            ("OR", "Logical OR", "3"),
            ("NOT", "Logical NOT", "3"),
            ("IN", "In list", "3"),
            ("NOT IN", "Not in list", "3"),
            ("BETWEEN", "Between range", "3"),
            ("LIKE", "Pattern match (case-sensitive)", "3"),
            ("ILIKE", "Pattern match (case-insensitive)", "3"),
            ("SIMILAR TO", "Regex-like pattern match", "3"),
            ("IS NULL", "Check null", "3"),
            ("IS NOT NULL", "Check not null", "3"),
            ("IS DISTINCT FROM", "Null-safe inequality", "3"),
            ("IS NOT DISTINCT FROM", "Null-safe equality", "3"),
            ("EXISTS", "Check existence", "3"),
            ("ANY", "Compare to any array element", "3"),
            ("SOME", "Alias for ANY", "3"),
            // Conditional expressions (lower priority)
            ("CASE", "Conditional expression", "4"),
            ("WHEN", "Case condition", "4"),
            ("THEN", "Case result", "4"),
            ("ELSE", "Default case", "4"),
            ("END", "End case/block", "4"),
            // Type casting and coercion
            ("CAST", "Type cast", "4"),
            ("COALESCE", "First non-null", "4"),
            ("NULLIF", "Return null if equal", "4"),
            ("GREATEST", "Return largest value", "4"),
            ("LEAST", "Return smallest value", "4"),
            // Boolean literals
            ("TRUE", "Boolean true", "5"),
            ("FALSE", "Boolean false", "5"),
            ("NULL", "Null value", "5"),
            // Data types (for CAST)
            ("TEXT", "Text data type", "6"),
            ("INTEGER", "Integer data type", "6"),
            ("BIGINT", "Big integer data type", "6"),
            ("BOOLEAN", "Boolean data type", "6"),
            ("NUMERIC", "Numeric data type", "6"),
            ("TIMESTAMP", "Timestamp data type", "6"),
            ("TIMESTAMPTZ", "Timestamp with timezone", "6"),
            ("DATE", "Date data type", "6"),
            ("TIME", "Time data type", "6"),
            ("UUID", "UUID data type", "6"),
            ("JSONB", "JSONB data type", "6"),
            ("JSON", "JSON data type", "6"),
        ];

        SQL_KEYWORDS
            .iter()
            .map(|(keyword, detail, sort_priority)| CompletionItem {
                label: keyword.to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                detail: Some(detail.to_string()),
                sort_text: Some(format!("9{}{}", sort_priority, keyword)), // 9 prefix puts keywords after columns and functions
                ..Default::default()
            })
            .collect()
    }

    /// Get all PostgreSQL function completions from schema cache.
    fn get_function_completions(&self) -> Vec<CompletionItem> {
        use tower_lsp::lsp_types::InsertTextFormat;

        self.schema_cache
            .get_functions()
            .into_iter()
            .map(|func| {
                // Generate snippet with placeholders for function arguments
                let insert_text = Self::generate_function_snippet(&func.name, &func.signature);

                CompletionItem {
                    label: func.name.clone(),
                    kind: Some(CompletionItemKind::FUNCTION),
                    detail: Some(format!("{} - {}", func.return_type, func.description)),
                    documentation: Some(tower_lsp::lsp_types::Documentation::String(format!(
                        "```sql\n{}\n```",
                        func.signature
                    ))),
                    insert_text: Some(insert_text),
                    insert_text_format: Some(InsertTextFormat::SNIPPET),
                    // Sort functions after columns but before keywords
                    sort_text: Some(format!("1{}", func.name)),
                    ..Default::default()
                }
            })
            .collect()
    }

    /// Generate a snippet with placeholders for function arguments.
    fn generate_function_snippet(name: &str, signature: &str) -> String {
        // Special cases for JSONB functions with FHIR-aware snippets
        if Self::is_jsonb_function(name) {
            let name_lower = name.to_lowercase();

            // Path query functions get JSONPath template
            if matches!(
                name_lower.as_str(),
                "jsonb_path_exists"
                    | "jsonb_path_match"
                    | "jsonb_path_query"
                    | "jsonb_path_query_array"
                    | "jsonb_path_query_first"
            ) {
                return format!("{}(${{1:jsonb_column}}, '$.${{2:path}}')", name);
            }

            // Manipulation functions get path array template
            if name_lower == "jsonb_set" {
                return format!(
                    "{}(${{1:jsonb_column}}, '{{${{2:path}}}}', '${{3:value}}')",
                    name
                );
            }

            if name_lower == "jsonb_insert" {
                return format!(
                    "{}(${{1:jsonb_column}}, '{{${{2:path}}}}', '${{3:value}}', ${{4:false}})",
                    name
                );
            }
        }

        // Special cases for functions without parentheses
        if matches!(
            name,
            "current_timestamp"
                | "current_date"
                | "current_time"
                | "localtime"
                | "localtimestamp"
                | "current_user"
        ) {
            return name.to_string();
        }

        // Extract the part between parentheses from signature
        if let Some(start) = signature.find('(') {
            if let Some(end) = signature.rfind(')') {
                let args_str = &signature[start + 1..end];

                // Handle variadic and optional args by simplifying to simple placeholder
                if args_str.contains("VARIADIC") || args_str.contains("...") {
                    return format!("{}(${{1:}})", name);
                }

                // Handle special cases
                if args_str.is_empty() || args_str.trim() == "*" {
                    return format!("{}()", name);
                }

                // Parse arguments and create snippets
                let args: Vec<&str> = args_str.split(',').collect();
                let mut snippet_args = Vec::new();

                for (i, arg) in args.iter().enumerate() {
                    let arg = arg.trim();
                    // Skip optional args (those in [])
                    if arg.starts_with('[') {
                        continue;
                    }
                    // Extract argument name (first word before type or AS)
                    let arg_name = arg
                        .split_whitespace()
                        .next()
                        .unwrap_or("arg")
                        .trim_matches(|c| c == '[' || c == ']');
                    snippet_args.push(format!("${{{}:{}}}", i + 1, arg_name));
                }

                if snippet_args.is_empty() {
                    return format!("{}()", name);
                }

                return format!("{}({})", name, snippet_args.join(", "));
            }
        }

        // Fallback: just the function name with empty parens
        format!("{}()", name)
    }

    /// Get JSONB operator completions.
    fn get_jsonb_operator_completions(&self) -> Vec<CompletionItem> {
        const JSONB_OPERATORS: &[(&str, &str)] = &[
            ("->", "Get JSON object field by key (returns jsonb)"),
            ("->>", "Get JSON object field as text"),
            ("#>", "Get JSON object at path (returns jsonb)"),
            ("#>>", "Get JSON object at path as text"),
            ("@>", "Does left JSON contain right?"),
            ("<@", "Is left JSON contained in right?"),
            ("?", "Does key/element exist?"),
            ("?|", "Do any keys exist?"),
            ("?&", "Do all keys exist?"),
            ("||", "Concatenate JSONB values"),
            ("-", "Delete key or array element"),
            ("#-", "Delete at path"),
            ("@?", "Does JSONPath return any item?"),
            ("@@", "JSONPath predicate check"),
        ];

        JSONB_OPERATORS
            .iter()
            .map(|(op, detail)| CompletionItem {
                label: op.to_string(),
                kind: Some(CompletionItemKind::OPERATOR),
                detail: Some(detail.to_string()),
                ..Default::default()
            })
            .collect()
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for PostgresLspServer {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        tracing::debug!(
            client_name = ?params.client_info.as_ref().map(|c| &c.name),
            "LSP initialize request received"
        );
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                completion_provider: Some(CompletionOptions {
                    trigger_characters: Some(vec![
                        ".".into(),
                        "'".into(),
                        " ".into(),
                        ">".into(),
                        "-".into(),
                        "#".into(),
                        "?".into(),
                        "@".into(),
                    ]),
                    resolve_provider: Some(false),
                    ..Default::default()
                }),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                ..Default::default()
            },
            ..Default::default()
        })
    }

    async fn initialized(&self, _params: tower_lsp::lsp_types::InitializedParams) {
        tracing::debug!("PostgreSQL LSP server initialized");

        // Refresh schema cache on initialization
        self.refresh_schema_cache().await;

        self.client
            .log_message(
                tower_lsp::lsp_types::MessageType::INFO,
                "PostgreSQL LSP ready",
            )
            .await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        tracing::trace!(
            uri = %params.text_document.uri,
            language = ?params.text_document.language_id,
            text_len = params.text_document.text.len(),
            "LSP did_open received"
        );
        self.documents
            .insert(params.text_document.uri, params.text_document.text);
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        tracing::trace!(
            uri = %params.text_document.uri,
            "LSP did_change received"
        );
        if let Some(change) = params.content_changes.into_iter().next() {
            self.documents.insert(params.text_document.uri, change.text);
        }
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let uri = &params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;

        tracing::trace!(
            ?uri,
            line = position.line,
            character = position.character,
            "LSP completion request received"
        );

        let Some(text) = self.documents.get(uri) else {
            tracing::debug!("LSP completion: document not found for URI");
            return Ok(None);
        };

        // Calculate byte offset from position
        let offset = Self::position_to_offset(&text, position);

        // Try tree-sitter based completions first (Supabase approach)
        // Returns None if tree-sitter can't handle this context (e.g., JSONB paths)
        tracing::trace!("Trying tree-sitter based completions first");
        if let Some(items) = self.get_completions_with_treesitter(&text, offset).await {
            tracing::debug!(
                item_count = items.len(),
                "LSP completion using tree-sitter context (early return)"
            );
            return Ok(Some(CompletionResponse::Array(items)));
        }

        // Fall back to regex-based parser for JSONB paths and other special cases
        tracing::trace!("Tree-sitter returned None, falling back to SqlParser");
        let context = SqlParser::get_context(&text, position);
        tracing::debug!(
            ?context,
            line = position.line,
            character = position.character,
            "LSP completion context detected (works in SELECT, WHERE, JOIN, etc.)"
        );

        // Get completions based on context
        tracing::trace!("Getting completions for context");
        let items = self
            .get_completions_for_context(&context, position, &text)
            .await;

        tracing::debug!(item_count = items.len(), "LSP completion returning items");

        Ok(Some(CompletionResponse::Array(items)))
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        let Some(text) = self.documents.get(uri) else {
            return Ok(None);
        };

        // Get context at position
        let context = SqlParser::get_context(&text, position);

        // Generate hover based on context
        let hover_info = self.get_hover_for_context(&context).await;

        Ok(hover_info.map(|contents| Hover {
            contents: tower_lsp::lsp_types::HoverContents::Markup(
                tower_lsp::lsp_types::MarkupContent {
                    kind: tower_lsp::lsp_types::MarkupKind::Markdown,
                    value: contents,
                },
            ),
            range: None,
        }))
    }
}

impl PostgresLspServer {
    /// Get hover information based on cursor context.
    async fn get_hover_for_context(&self, context: &CursorContext) -> Option<String> {
        match context {
            CursorContext::JsonbPath {
                table,
                column,
                path,
                operator,
                tables,
                ..
            } => {
                // Get FHIR element hover info
                // If table is empty, try to infer from the first table in FROM clause
                let resolved_table = if table.is_empty() && !tables.is_empty() {
                    &tables[0].table
                } else {
                    table
                };
                self.get_fhir_path_hover(resolved_table, column, path, operator)
                    .await
            }
            CursorContext::FromClause { partial } if !partial.is_empty() => {
                // Get table hover info
                self.get_table_hover(partial)
            }
            CursorContext::SelectColumns {
                partial, tables, ..
            } if !partial.is_empty() => {
                // Try function hover first
                if let Some(info) = self.get_function_hover(partial) {
                    return Some(info);
                }
                // Try column hover from tables in FROM clause
                for table_ref in tables {
                    if let Some(info) = self.get_column_hover(&table_ref.table, partial) {
                        return Some(info);
                    }
                }
                // Try table hover
                self.get_table_hover(partial)
            }
            CursorContext::AliasColumnAccess { table, partial, .. } if !partial.is_empty() => {
                // Show column hover for alias.column pattern
                self.get_column_hover(table, partial)
            }
            CursorContext::WhereClause {
                partial, tables, ..
            } if !partial.is_empty() => {
                // Try function hover
                if let Some(info) = self.get_function_hover(partial) {
                    return Some(info);
                }
                // Try column hover from tables
                for table_ref in tables {
                    if let Some(info) = self.get_column_hover(&table_ref.table, partial) {
                        return Some(info);
                    }
                }
                // Try operator hover
                self.get_operator_hover(partial)
            }
            CursorContext::FunctionArgs {
                function,
                arg_index,
            } => self.get_function_arg_hover(function, *arg_index),
            CursorContext::GrantRole { partial } if !partial.is_empty() => {
                // Show role hover for GRANT/REVOKE TO role
                self.get_role_hover(partial)
            }
            CursorContext::GrantTable { partial } if !partial.is_empty() => {
                // Show table hover for GRANT ON table
                self.get_table_hover(partial)
            }
            CursorContext::CastType { partial } if !partial.is_empty() => {
                // Show type hover for CAST(... AS type)
                self.get_type_hover(partial)
            }
            CursorContext::SchemaTableAccess { partial, .. } if !partial.is_empty() => {
                // Show table hover for schema.table pattern
                self.get_table_hover(partial)
            }
            _ => None,
        }
    }

    /// Get hover information for a FHIR element path.
    async fn get_fhir_path_hover(
        &self,
        table: &str,
        column: &str,
        path: &[String],
        operator: &super::parser::JsonbOperator,
    ) -> Option<String> {
        // Get resource type from table name
        let resource_type = self
            .schema_cache
            .get_fhir_resource_type(table)
            .unwrap_or_else(|| Self::to_pascal_case(table));

        // Build the path
        let element_path = if path.is_empty() {
            String::new()
        } else {
            path.join(".")
        };

        // Try to get element info from FHIR resolver
        if let Some(elem) = self
            .fhir_resolver
            .get_element(&resource_type, &element_path)
            .await
        {
            let cardinality = if elem.max == 0 {
                format!("{}..* (array)", elem.min)
            } else if elem.max == 1 {
                format!("{}..1", elem.min)
            } else {
                format!("{}..{}", elem.min, elem.max)
            };

            let mut hover = format!("### FHIR Element: `{}`\n\n", elem.path);
            hover.push_str(&format!("**Type:** `{}`\n\n", elem.type_code));
            hover.push_str(&format!("**Cardinality:** `{}`\n\n", cardinality));

            if let Some(short) = &elem.short {
                hover.push_str(&format!("**Summary:** {}\n\n", short));
            }

            if let Some(def) = &elem.definition {
                hover.push_str(&format!("**Definition:** {}\n\n", def));
            }

            // Add usage hint
            let op_str = match operator {
                super::parser::JsonbOperator::Arrow => "->",
                super::parser::JsonbOperator::DoubleArrow => "->>",
                super::parser::JsonbOperator::HashArrow => "#>",
                super::parser::JsonbOperator::HashDoubleArrow => "#>>",
                _ => "->",
            };
            hover.push_str(&format!(
                "---\n**Usage:** `{}.{}{}'{}'`",
                table, column, op_str, element_path
            ));

            return Some(hover);
        }

        // Fallback: just show the path info
        let path_display = if path.is_empty() {
            "(root)".to_string()
        } else {
            path.join(".")
        };
        Some(format!(
            "### JSONB Path\n\n**Table:** `{}`\n\n**Column:** `{}`\n\n**Path:** `{}`",
            table, column, path_display
        ))
    }

    /// Get hover information for a table.
    fn get_table_hover(&self, table_name: &str) -> Option<String> {
        // Use exact match only - no prefix matching for hover
        let table = self.schema_cache.get_table_by_name(table_name)?;

        let mut hover = format!("### Table: `{}.{}`\n\n", table.schema, table.name);
        hover.push_str(&format!("**Type:** {}\n\n", table.table_type));

        if table.is_fhir_table {
            if let Some(ref rt) = table.fhir_resource_type {
                hover.push_str(&format!("**FHIR Resource:** `{}`\n\n", rt));
                hover.push_str(
                    "This table stores FHIR resources. Use JSONB operators \
                     (`->`, `->>`, `#>`, `#>>`) to query element paths.\n\n",
                );
                hover.push_str(&format!(
                    "**Example:**\n```sql\nSELECT resource->>'id', resource->'name'->0->>'family'\nFROM {}\n```",
                    table.name
                ));
            }
        }

        // Get columns for this table
        let columns = self.schema_cache.get_columns(&table.name);
        if !columns.is_empty() {
            hover.push_str("\n\n**Columns:**\n");
            for col in columns.iter().take(10) {
                let nullable = if col.is_nullable {
                    "nullable"
                } else {
                    "not null"
                };
                hover.push_str(&format!(
                    "- `{}`: {} ({})\n",
                    col.name, col.data_type, nullable
                ));
            }
            if columns.len() > 10 {
                hover.push_str(&format!("- ... and {} more columns\n", columns.len() - 10));
            }
        }

        Some(hover)
    }

    /// Get hover information for a column.
    fn get_column_hover(&self, table_name: &str, column_name: &str) -> Option<String> {
        let columns = self.schema_cache.get_columns(table_name);
        let column = columns
            .iter()
            .find(|c| c.name.eq_ignore_ascii_case(column_name))?;

        let mut hover = format!("### Column: `{}.{}`\n\n", table_name, column.name);
        hover.push_str(&format!("**Type:** `{}`\n\n", column.data_type));
        hover.push_str(&format!(
            "**Nullable:** {}\n\n",
            if column.is_nullable { "Yes" } else { "No" }
        ));

        if let Some(ref default) = column.default_value {
            hover.push_str(&format!("**Default:** `{}`\n\n", default));
        }

        if let Some(ref desc) = column.description {
            hover.push_str(&format!("**Description:** {}\n\n", desc));
        }

        // If this is a JSONB column in a FHIR table, add usage hints
        if column.data_type == "jsonb" {
            if self.schema_cache.is_fhir_table(table_name) {
                let resource_type = self
                    .schema_cache
                    .get_fhir_resource_type(table_name)
                    .unwrap_or_else(|| Self::to_pascal_case(table_name));

                hover.push_str(&format!("---\n**FHIR Resource:** `{}`\n\n", resource_type));
                hover.push_str("**Usage:**\n```sql\n");
                hover.push_str(&format!(
                    "{col}->>'id'              -- Get resource ID as text\n",
                    col = column.name
                ));
                hover.push_str(&format!(
                    "{col}->'name'->0->>'family' -- Nested path access\n",
                    col = column.name
                ));
                hover.push_str(&format!(
                    "{col} @> '{{\"gender\": \"male\"}}' -- Contains check\n",
                    col = column.name
                ));
                hover.push_str("```");
            }
        }

        Some(hover)
    }

    /// Get hover information for a JSONB function.
    fn get_function_hover(&self, name: &str) -> Option<String> {
        let func = self.schema_cache.get_function(name)?;

        let mut hover = format!("### Function: `{}`\n\n", func.name);
        hover.push_str(&format!("```sql\n{}\n```\n\n", func.signature));
        hover.push_str(&format!("**Returns:** `{}`\n\n", func.return_type));
        hover.push_str(&format!("**Description:** {}", func.description));

        Some(hover)
    }

    /// Get hover information for a JSONB operator.
    fn get_operator_hover(&self, op: &str) -> Option<String> {
        let info = match op {
            "->" => (
                "->",
                "Get JSON object field by key",
                "Returns the field as `jsonb`. Use for nested access.",
                "resource->'name'->0->'given'",
            ),
            "->>" => (
                "->>",
                "Get JSON object field as text",
                "Returns the field as `text`. Use for final value extraction.",
                "resource->>'id'",
            ),
            "#>" => (
                "#>",
                "Get JSON object at path",
                "Returns the value at path as `jsonb`. Path is a text array.",
                "resource#>'{name,0,family}'",
            ),
            "#>>" => (
                "#>>",
                "Get JSON object at path as text",
                "Returns the value at path as `text`. Path is a text array.",
                "resource#>>'{name,0,family}'",
            ),
            "@>" => (
                "@>",
                "Contains",
                "Does the left JSON value contain the right JSON path/value?",
                "resource @> '{\"gender\": \"male\"}'",
            ),
            "<@" => (
                "<@",
                "Contained by",
                "Is the left JSON value contained within the right?",
                "'{\"active\": true}' <@ resource",
            ),
            "?" => (
                "?",
                "Key exists",
                "Does the string exist as a top-level key within the JSON value?",
                "resource ? 'name'",
            ),
            "?|" => (
                "?|",
                "Any key exists",
                "Do any of the strings in the text array exist as top-level keys?",
                "resource ?| array['name', 'gender']",
            ),
            "?&" => (
                "?&",
                "All keys exist",
                "Do all of the strings in the text array exist as top-level keys?",
                "resource ?& array['name', 'gender']",
            ),
            "||" => (
                "||",
                "Concatenate",
                "Concatenate two JSONB values into a new JSONB value.",
                "resource || '{\"note\": \"updated\"}'",
            ),
            "@?" => (
                "@?",
                "JSONPath exists",
                "Does the JSONPath return any item for the JSON value?",
                "resource @? '$.name[*].family'",
            ),
            "@@" => (
                "@@",
                "JSONPath predicate",
                "Returns the result of a JSONPath predicate check.",
                "resource @@ '$.gender == \"male\"'",
            ),
            _ => return None,
        };

        let mut hover = format!("### JSONB Operator: `{}`\n\n", info.0);
        hover.push_str(&format!("**{}**\n\n", info.1));
        hover.push_str(&format!("{}\n\n", info.2));
        hover.push_str(&format!("**Example:**\n```sql\n{}\n```", info.3));

        Some(hover)
    }

    /// Get hover information for function arguments.
    fn get_function_arg_hover(&self, function: &str, arg_index: usize) -> Option<String> {
        let (func_name, args_info): (&str, Vec<(&str, &str)>) =
            match function.to_lowercase().as_str() {
                "jsonb_extract_path" | "jsonb_extract_path_text" => (
                    function,
                    vec![
                        ("from_json", "JSONB value to extract from"),
                        ("path_elems...", "Variable path elements as text strings"),
                    ],
                ),
                "jsonb_set" => (
                    "jsonb_set",
                    vec![
                        ("target", "JSONB value to modify"),
                        ("path", "Text array specifying path to set"),
                        ("new_value", "JSONB value to insert"),
                        (
                            "create_if_missing",
                            "Create path if missing (default: true)",
                        ),
                    ],
                ),
                "jsonb_insert" => (
                    "jsonb_insert",
                    vec![
                        ("target", "JSONB value to modify"),
                        ("path", "Text array specifying insertion point"),
                        ("new_value", "JSONB value to insert"),
                        (
                            "insert_after",
                            "Insert after (true) or before (false) path position",
                        ),
                    ],
                ),
                "jsonb_path_query" | "jsonb_path_query_array" | "jsonb_path_query_first" => (
                    function,
                    vec![
                        ("target", "JSONB value to query"),
                        ("path", "JSONPath expression"),
                        ("vars", "Optional JSONB object with path variables"),
                        ("silent", "Suppress errors on missing path (default: false)"),
                    ],
                ),
                _ => return None,
            };

        let mut hover = format!("### Argument {} of `{}`\n\n", arg_index + 1, func_name);

        if arg_index < args_info.len() {
            let (name, desc) = args_info[arg_index];
            hover.push_str(&format!("**`{}`:** {}\n\n", name, desc));
        }

        hover.push_str("**All arguments:**\n");
        for (i, (name, desc)) in args_info.iter().enumerate() {
            let marker = if i == arg_index { "→ " } else { "  " };
            hover.push_str(&format!("{}{}: {}: {}\n", marker, i + 1, name, desc));
        }

        Some(hover)
    }

    /// Get hover information for a role.
    fn get_role_hover(&self, role_name: &str) -> Option<String> {
        let role = self.schema_cache.get_role(role_name)?;

        let mut hover = format!("### Role: `{}`\n\n", role.name);

        let mut attrs = Vec::new();
        if role.is_superuser {
            attrs.push("SUPERUSER");
        }
        if role.can_login {
            attrs.push("LOGIN");
        }
        if role.create_db {
            attrs.push("CREATEDB");
        }
        if role.create_role {
            attrs.push("CREATEROLE");
        }

        if !attrs.is_empty() {
            hover.push_str(&format!("**Attributes:** {}\n\n", attrs.join(", ")));
        }

        if !role.member_of.is_empty() {
            hover.push_str(&format!("**Member of:** {}\n\n", role.member_of.join(", ")));
        }

        hover.push_str("---\n");
        hover.push_str("**Usage:**\n```sql\n");
        hover.push_str(&format!("GRANT SELECT ON table TO {};\n", role.name));
        hover.push_str(&format!("SET ROLE {};\n", role.name));
        hover.push_str("```");

        Some(hover)
    }

    /// Get hover information for a policy.
    /// Note: This requires both table and policy name which needs more context detection.
    #[allow(dead_code)]
    fn get_policy_hover(&self, table_name: &str, policy_name: &str) -> Option<String> {
        let policy = self.schema_cache.get_policy(table_name, policy_name)?;

        let mut hover = format!("### Policy: `{}`\n\n", policy.name);
        hover.push_str(&format!(
            "**Table:** `{}.{}`\n\n",
            policy.schema, policy.table_name
        ));
        hover.push_str(&format!("**Command:** {}\n\n", policy.command));
        hover.push_str(&format!(
            "**Type:** {}\n\n",
            if policy.permissive {
                "PERMISSIVE"
            } else {
                "RESTRICTIVE"
            }
        ));
        hover.push_str(&format!("**Roles:** {}\n\n", policy.roles.join(", ")));

        if let Some(ref using) = policy.using_expr {
            hover.push_str(&format!("**USING:**\n```sql\n{}\n```\n\n", using));
        }

        if let Some(ref with_check) = policy.with_check_expr {
            hover.push_str(&format!("**WITH CHECK:**\n```sql\n{}\n```\n", with_check));
        }

        Some(hover)
    }

    /// Get hover information for a type.
    fn get_type_hover(&self, type_name: &str) -> Option<String> {
        let type_info = self.schema_cache.get_type_by_name(type_name)?;

        let mut hover = format!("### Type: `{}.{}`\n\n", type_info.schema, type_info.name);
        hover.push_str(&format!("**Category:** {}\n\n", type_info.category));

        match type_info.category.as_str() {
            "Enum" => {
                hover.push_str("**Values:**\n");
                for label in &type_info.enum_labels {
                    hover.push_str(&format!("- `{}`\n", label));
                }
                hover.push_str("\n**Usage:**\n```sql\n");
                hover.push_str(&format!(
                    "SELECT * FROM table WHERE column = '{}'::{};\n",
                    type_info
                        .enum_labels
                        .first()
                        .map(|s| s.as_str())
                        .unwrap_or("value"),
                    type_info.name
                ));
                hover.push_str("```");
            }
            "Composite" => {
                if !type_info.attributes.is_empty() {
                    hover.push_str("**Attributes:**\n");
                    for (name, typ) in &type_info.attributes {
                        hover.push_str(&format!("- `{}`: {}\n", name, typ));
                    }
                }
            }
            _ => {}
        }

        if let Some(ref desc) = type_info.description {
            hover.push_str(&format!("\n**Description:** {}\n", desc));
        }

        Some(hover)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_jsonpath_string_simple() {
        let (segments, ctx) = PostgresLspServer::parse_jsonpath_string(" '$.name");
        assert_eq!(segments, vec!["name"]);
        assert!(ctx.has_opening_quote);
        assert!(ctx.cursor_inside_quotes);
    }

    #[test]
    fn test_parse_jsonpath_string_nested() {
        let (segments, _ctx) = PostgresLspServer::parse_jsonpath_string(" '$.name.given");
        assert_eq!(segments, vec!["name", "given"]);
    }

    #[test]
    fn test_parse_jsonpath_string_with_array_index() {
        let (segments, _ctx) = PostgresLspServer::parse_jsonpath_string(" '$.name[0].given");
        assert_eq!(segments, vec!["name", "0", "given"]);
    }

    #[test]
    fn test_parse_jsonpath_string_trailing_dot() {
        let (segments, _ctx) = PostgresLspServer::parse_jsonpath_string(" '$.name.");
        assert_eq!(segments, vec!["name"]);
    }

    #[test]
    fn test_parse_jsonpath_string_empty() {
        let (segments, ctx) = PostgresLspServer::parse_jsonpath_string(" '$.");
        assert!(segments.is_empty());
        assert!(ctx.has_opening_quote);
    }

    #[test]
    fn test_is_jsonb_function() {
        // Path query functions
        assert!(PostgresLspServer::is_jsonb_function("jsonb_path_exists"));
        assert!(PostgresLspServer::is_jsonb_function("jsonb_path_query"));
        assert!(PostgresLspServer::is_jsonb_function("JSONB_PATH_EXISTS")); // case insensitive

        // Manipulation functions
        assert!(PostgresLspServer::is_jsonb_function("jsonb_set"));
        assert!(PostgresLspServer::is_jsonb_function("jsonb_insert"));

        // Not JSONB functions
        assert!(!PostgresLspServer::is_jsonb_function("json_extract"));
        assert!(!PostgresLspServer::is_jsonb_function("count"));
        assert!(!PostgresLspServer::is_jsonb_function("sum"));
    }

    #[test]
    fn test_detect_function_call_context_simple() {
        let sql = "SELECT jsonb_path_exists(resource, '$.name') FROM patient";

        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&pgls_treesitter_grammar::LANGUAGE.into())
            .expect("Failed to load grammar");

        let tree = parser.parse(sql, None).expect("Failed to parse");
        let root = tree.root_node();

        // Test cursor at first argument position (after opening paren)
        let offset = sql.find("(").unwrap() + 1;
        let cursor_node = root
            .descendant_for_byte_range(offset, offset)
            .expect("Failed to find node");

        let ctx = PostgresLspServer::detect_function_call_context(&cursor_node, sql, offset);
        assert!(ctx.is_some(), "Should detect function call context");

        let ctx = ctx.unwrap();
        assert_eq!(ctx.function_name, "jsonb_path_exists");
        assert_eq!(ctx.arg_index, 0, "Should be at first argument");
    }

    #[test]
    fn test_detect_function_call_context_second_arg() {
        let sql = "SELECT jsonb_path_exists(resource, '$.name') FROM patient";

        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&pgls_treesitter_grammar::LANGUAGE.into())
            .expect("Failed to load grammar");

        let tree = parser.parse(sql, None).expect("Failed to parse");
        let root = tree.root_node();

        // Test cursor at second argument position (after comma)
        let offset = sql.find(", '").unwrap() + 2;
        let cursor_node = root
            .descendant_for_byte_range(offset, offset)
            .expect("Failed to find node");

        let ctx = PostgresLspServer::detect_function_call_context(&cursor_node, sql, offset);
        assert!(ctx.is_some(), "Should detect function call context");

        let ctx = ctx.unwrap();
        assert_eq!(ctx.function_name, "jsonb_path_exists");
        assert_eq!(ctx.arg_index, 1, "Should be at second argument");
    }

    #[test]
    fn test_detect_function_call_context_in_where_clause() {
        let sql = "SELECT * FROM patient WHERE jsonb_path_exists(resource, '$.')";

        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&pgls_treesitter_grammar::LANGUAGE.into())
            .expect("Failed to load grammar");

        let tree = parser.parse(sql, None).expect("Failed to parse");
        let root = tree.root_node();

        // Test cursor at first argument in WHERE clause
        let offset = sql.find("(resource").unwrap() + 1;
        let cursor_node = root
            .descendant_for_byte_range(offset, offset)
            .expect("Failed to find node");

        let ctx = PostgresLspServer::detect_function_call_context(&cursor_node, sql, offset);
        assert!(
            ctx.is_some(),
            "Should detect function call context in WHERE clause"
        );

        let ctx = ctx.unwrap();
        assert_eq!(ctx.function_name, "jsonb_path_exists");
        assert_eq!(ctx.arg_index, 0);
    }

    #[test]
    fn test_extract_tables_from_ast() {
        let sql = "SELECT * FROM patient WHERE id = '123'";

        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&pgls_treesitter_grammar::LANGUAGE.into())
            .expect("Failed to load grammar");

        let tree = parser.parse(sql, None).expect("Failed to parse");
        let root = tree.root_node();

        let tables = PostgresLspServer::extract_tables_from_ast(&root, sql);
        assert_eq!(tables.len(), 1);
        assert_eq!(tables[0], "patient");
    }

    #[test]
    fn test_extract_tables_from_ast_multiple() {
        let sql = "SELECT * FROM patient p JOIN observation o ON p.id = o.subject";

        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&pgls_treesitter_grammar::LANGUAGE.into())
            .expect("Failed to load grammar");

        let tree = parser.parse(sql, None).expect("Failed to parse");
        let root = tree.root_node();

        let tables = PostgresLspServer::extract_tables_from_ast(&root, sql);
        assert!(
            tables.contains(&"patient".to_string()),
            "Should contain patient"
        );
        assert!(
            tables.contains(&"observation".to_string()),
            "Should contain observation"
        );
    }

    #[test]
    fn test_extract_first_arg_column() {
        let sql = "SELECT jsonb_path_exists(resource, '$.name') FROM patient";

        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&pgls_treesitter_grammar::LANGUAGE.into())
            .expect("Failed to load grammar");

        let tree = parser.parse(sql, None).expect("Failed to parse");
        let root = tree.root_node();

        // Get cursor in second argument to test extraction of first arg
        let offset = sql.find("'$.name'").unwrap();
        let cursor_node = root
            .descendant_for_byte_range(offset, offset)
            .expect("Failed to find node");

        let column = PostgresLspServer::extract_first_arg_column(&cursor_node, sql);
        assert_eq!(column, Some("resource".to_string()));
    }

    #[test]
    fn test_to_pascal_case() {
        assert_eq!(PostgresLspServer::to_pascal_case("patient"), "Patient");
        assert_eq!(
            PostgresLspServer::to_pascal_case("patient_encounter"),
            "PatientEncounter"
        );
        assert_eq!(
            PostgresLspServer::to_pascal_case("observation"),
            "Observation"
        );
        assert_eq!(PostgresLspServer::to_pascal_case("care_plan"), "CarePlan");
    }
}
