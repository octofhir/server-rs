//! Hover information provider
//!
//! This module provides hover tooltips for SQL elements including tables, columns,
//! functions, and FHIR-specific JSONB paths.

use async_lsp::lsp_types::{Hover, HoverContents, MarkupContent, MarkupKind, Position};
use pgls_text_size::TextSize;
use pgls_treesitter::context::{TreeSitterContextParams, TreesitterContext};
use std::sync::Arc;

use crate::lsp::fhir_resolver::FhirResolver;
use crate::lsp::parser::{CursorContext, JsonbDetector};
use crate::lsp::schema_cache::SchemaCache;
use crate::lsp::server::PostgresLspServer;

use super::super::completion::CompletionContext;

/// Context for hover operations.
///
/// Holds references to schema cache and FHIR resolver for providing
/// hover information about SQL elements.
pub struct HoverContext {
    schema_cache: Arc<SchemaCache>,
    fhir_resolver: Arc<FhirResolver>,
}

impl HoverContext {
    /// Create a new hover context from server state.
    pub fn from_server(server: &PostgresLspServer) -> Self {
        Self {
            schema_cache: server.schema_cache.clone(),
            fhir_resolver: server.fhir_resolver.clone(),
        }
    }

    /// Get hover information for the position in the document.
    pub async fn get_hover(&self, text: &str, position: Position) -> Option<Hover> {
        let offset = position_to_offset(text, position);

        // Try tree-sitter based hover first
        if let Some(hover) = self.get_tree_sitter_hover(text, offset).await {
            return Some(hover);
        }

        // JSONB detection now uses hybrid AST approach (tree-sitter + pg_query)
        // No regex fallback needed - AST handles all cases including incomplete SQL
        // If tree-sitter fails to parse, return None instead of regex fallback
        None
    }

    /// Get hover information using tree-sitter AST analysis.
    async fn get_tree_sitter_hover(&self, text: &str, offset: usize) -> Option<Hover> {
        // Parse SQL with tree-sitter
        let mut parser = tree_sitter::Parser::new();
        if parser
            .set_language(&pgls_treesitter_grammar::LANGUAGE.into())
            .is_err()
        {
            return None;
        }

        let tree = parser.parse(text, None)?;
        let root = tree.root_node();
        let cursor_node = root.descendant_for_byte_range(offset, offset)?;

        // Check for JSONB path hover (show FHIR element info)
        if let Some(jsonb_hover) = self
            .get_jsonb_path_hover(&cursor_node, text, offset, &tree)
            .await
        {
            return Some(jsonb_hover);
        }

        // Check for table hover
        if let Some(table_hover) = self.get_table_hover(&cursor_node, text).await {
            return Some(table_hover);
        }

        // Check for column hover
        if let Some(column_hover) = self.get_column_hover(&cursor_node, text, &tree).await {
            return Some(column_hover);
        }

        // Check for function hover
        if let Some(function_hover) = self.get_function_hover(&cursor_node, text).await {
            return Some(function_hover);
        }

        None
    }

    /// Get hover for JSONB path expressions showing FHIR element information.
    async fn get_jsonb_path_hover(
        &self,
        cursor_node: &tree_sitter::Node<'_>,
        text: &str,
        offset: usize,
        tree: &tree_sitter::Tree,
    ) -> Option<Hover> {
        // Detect JSONB operator context
        let jsonb_ctx = JsonbDetector::detect(cursor_node, text, offset)?;

        // Get table name from query
        let ctx = TreesitterContext::new(TreeSitterContextParams {
            position: TextSize::new(offset as u32),
            text,
            tree,
        });
        let mentioned_tables = CompletionContext::get_mentioned_table_names(&ctx);
        let table_name = mentioned_tables.first()?;

        // Get resource type from table
        let resource_type = self
            .schema_cache
            .get_fhir_resource_type(table_name)
            .unwrap_or_else(|| PostgresLspServer::to_pascal_case(table_name));

        // Build FHIR path (filter out numeric segments from path_chain)
        let fhir_path_segments: Vec<&String> = jsonb_ctx
            .path_chain
            .iter()
            .skip(1) // Skip column name
            .filter(|s| !s.chars().all(|c| c.is_ascii_digit()))
            .collect();

        let full_path = if fhir_path_segments.is_empty() {
            resource_type.clone()
        } else {
            format!(
                "{}.{}",
                resource_type,
                fhir_path_segments
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(".")
            )
        };

        // Get element info from FHIR resolver
        let parent_path = if fhir_path_segments.len() > 1 {
            fhir_path_segments[..fhir_path_segments.len() - 1]
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(".")
        } else {
            String::new()
        };

        let children = self
            .fhir_resolver
            .get_children(&resource_type, &parent_path)
            .await;
        let current_element_name = fhir_path_segments.last().map(|s| s.as_str()).unwrap_or("");
        let element = children.iter().find(|e| e.name == current_element_name)?;

        let min_card = element.min;
        let max_card_str = if element.max == 0 {
            "*".to_string()
        } else {
            element.max.to_string()
        };
        let cardinality = format!("[{}..{}]", min_card, max_card_str);

        let mut hover_text = format!("**{}**\n\n", full_path);
        hover_text.push_str(&format!("Type: `{}`\n\n", element.type_code));
        hover_text.push_str(&format!("Cardinality: `{}`\n\n", cardinality));

        if let Some(short) = &element.short {
            hover_text.push_str(&format!("{}\n\n", short));
        }

        if let Some(definition) = &element.definition {
            hover_text.push_str(&format!("---\n\n{}\n", definition));
        }

        Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: hover_text,
            }),
            range: None,
        })
    }

    /// Get hover for table names showing table metadata.
    async fn get_table_hover(&self, cursor_node: &tree_sitter::Node<'_>, text: &str) -> Option<Hover> {
        // Check if cursor is on a table identifier
        if !matches!(cursor_node.kind(), "table_identifier" | "any_identifier") {
            return None;
        }

        let table_name = cursor_node.utf8_text(text.as_bytes()).ok()?;
        let tables = self.schema_cache.get_tables();
        let table = tables.iter().find(|t| t.name == table_name)?;

        let mut hover_text = format!("**Table: {}.{}**\n\n", table.schema, table.name);
        hover_text.push_str(&format!("Type: `{}`\n\n", table.table_type));

        if table.is_fhir_table {
            if let Some(resource_type) = &table.fhir_resource_type {
                hover_text.push_str(&format!("FHIR Resource: `{}`\n\n", resource_type));
            }
        }

        Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: hover_text,
            }),
            range: None,
        })
    }

    /// Get hover for column names showing column metadata.
    async fn get_column_hover(
        &self,
        cursor_node: &tree_sitter::Node<'_>,
        text: &str,
        tree: &tree_sitter::Tree,
    ) -> Option<Hover> {
        // Check if cursor is on a column identifier
        if !matches!(cursor_node.kind(), "column_identifier" | "any_identifier") {
            return None;
        }

        let column_name = cursor_node.utf8_text(text.as_bytes()).ok()?;

        // Get mentioned tables to find the column
        let ctx = TreesitterContext::new(TreeSitterContextParams {
            position: TextSize::new(cursor_node.start_byte() as u32),
            text,
            tree,
        });
        let mentioned_tables = CompletionContext::get_mentioned_table_names(&ctx);

        for table_name in &mentioned_tables {
            let columns = self.schema_cache.get_columns(table_name);
            if let Some(col) = columns.iter().find(|c| c.name == column_name) {
                let mut hover_text = format!("**Column: {}.{}**\n\n", col.table_name, col.name);
                hover_text.push_str(&format!("Type: `{}`\n\n", col.data_type));
                hover_text.push_str(&format!("Nullable: `{}`\n\n", col.is_nullable));

                return Some(Hover {
                    contents: HoverContents::Markup(MarkupContent {
                        kind: MarkupKind::Markdown,
                        value: hover_text,
                    }),
                    range: None,
                });
            }
        }

        None
    }

    /// Get hover for function names showing function signatures.
    async fn get_function_hover(&self, cursor_node: &tree_sitter::Node<'_>, text: &str) -> Option<Hover> {
        // Check if cursor is on a function identifier
        if !matches!(cursor_node.kind(), "function_identifier" | "any_identifier") {
            return None;
        }

        let function_name = cursor_node.utf8_text(text.as_bytes()).ok()?;
        let functions = self.schema_cache.get_functions();
        let func = functions
            .iter()
            .find(|f| f.name.eq_ignore_ascii_case(function_name))?;

        let mut hover_text = format!("**Function: {}**\n\n", func.name);
        hover_text.push_str(&format!("```sql\n{}\n```\n\n", func.signature));

        if PostgresLspServer::is_jsonb_function(&func.name.to_lowercase()) {
            hover_text.push_str("*This function has FHIR-aware completions*\n\n");
        }

        Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: hover_text,
            }),
            range: None,
        })
    }

    /// Get hover based on regex-parsed context (fallback).
    pub async fn get_hover_for_context(
        &self,
        context: &CursorContext,
        _text: &str,
        _position: Position,
    ) -> Option<Hover> {
        let hover_text = match context {
            CursorContext::SelectColumns { .. } => "SELECT clause - choose columns to retrieve",
            CursorContext::FromClause { .. } => "FROM clause - specify tables or views",
            CursorContext::WhereClause { .. } => "WHERE clause - filter rows",
            CursorContext::SchemaTableAccess { schema, .. } => {
                return Some(Hover {
                    contents: HoverContents::Markup(MarkupContent {
                        kind: MarkupKind::Markdown,
                        value: format!("**Schema: {}**\n\nChoose a table from this schema", schema),
                    }),
                    range: None,
                });
            }
            _ => "SQL statement",
        };

        Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: hover_text.to_string(),
            }),
            range: None,
        })
    }
}

/// Helper to convert LSP Position to byte offset
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
