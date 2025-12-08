//! PostgreSQL Language Server implementation using tower-lsp.
//!
//! Provides SQL completion with FHIR-aware JSONB path suggestions.

use dashmap::DashMap;
use std::sync::Arc;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::{
    CompletionItem, CompletionItemKind, CompletionOptions, CompletionParams, CompletionResponse,
    DidChangeTextDocumentParams, DidOpenTextDocumentParams, Hover, HoverParams,
    HoverProviderCapability, InitializeParams, InitializeResult,
    ServerCapabilities, TextDocumentSyncCapability, TextDocumentSyncKind, Url,
};
use tower_lsp::{Client, LanguageServer};

use super::fhir_resolver::FhirResolver;
use super::parser::{CursorContext, SqlParser};
use super::schema_cache::SchemaCache;

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
    pub fn new(client: Client, db_pool: Arc<sqlx_postgres::PgPool>) -> Self {
        let schema_cache = Arc::new(SchemaCache::new(db_pool.clone()));
        let fhir_resolver = Arc::new(FhirResolver::new());
        Self {
            client,
            documents: DashMap::new(),
            db_pool,
            schema_cache,
            fhir_resolver,
        }
    }

    /// Get completions based on cursor context.
    async fn get_completions_for_context(&self, context: &CursorContext) -> Vec<CompletionItem> {
        match context {
            CursorContext::Keyword { partial } => {
                self.filter_by_prefix(self.get_keyword_completions(), partial)
            }
            CursorContext::SelectColumns { partial, .. } => {
                let mut items = self.get_keyword_completions();
                items.extend(self.get_jsonb_function_completions());
                self.filter_by_prefix(items, partial)
            }
            CursorContext::FromClause { partial } => {
                // Add table completions from schema cache
                let mut items = self.get_table_completions();
                items.extend(self.get_keyword_completions());
                self.filter_by_prefix(items, partial)
            }
            CursorContext::WhereClause { partial, .. } => {
                let mut items = self.get_keyword_completions();
                items.extend(self.get_jsonb_function_completions());
                items.extend(self.get_jsonb_operator_completions());
                self.filter_by_prefix(items, partial)
            }
            CursorContext::JsonbPath { table, path, .. } => {
                // Get FHIR path completions from canonical manager
                self.get_fhir_path_completions(table, path).await
            }
            CursorContext::FunctionArgs { function, .. } => {
                // Return type hints for function arguments
                self.get_function_arg_hints(function)
            }
            CursorContext::Unknown { partial } => {
                let mut items = self.get_keyword_completions();
                items.extend(self.get_jsonb_function_completions());
                items.extend(self.get_jsonb_operator_completions());
                self.filter_by_prefix(items, partial)
            }
        }
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

                CompletionItem {
                    label: table.name.clone(),
                    kind: Some(CompletionItemKind::CLASS),
                    detail: Some(detail),
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
                    if col.is_nullable { "nullable" } else { "not null" }
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

    /// Get FHIR path completions from the canonical manager.
    ///
    /// This method resolves FHIR element paths based on the table name (which maps
    /// to a resource type) and the current path context.
    async fn get_fhir_path_completions(&self, table: &str, path: &[String]) -> Vec<CompletionItem> {
        // Try to get resource type from table name via schema cache
        let resource_type = self
            .schema_cache
            .get_fhir_resource_type(table)
            .unwrap_or_else(|| {
                // Fall back to PascalCase conversion
                Self::to_pascal_case(table)
            });

        // Build the parent path from existing path segments
        let parent_path = if path.is_empty() {
            String::new()
        } else {
            format!("{}.{}", resource_type, path.join("."))
        };

        // Get children from FHIR resolver
        let children = self
            .fhir_resolver
            .get_children(&resource_type, &parent_path)
            .await;

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

                CompletionItem {
                    label: elem.name.clone(),
                    kind: Some(kind),
                    detail: Some(detail),
                    documentation: elem.definition.map(|d| {
                        tower_lsp::lsp_types::Documentation::String(d)
                    }),
                    insert_text: Some(format!("'{}'", elem.name)),
                    ..Default::default()
                }
            })
            .collect()
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
                vec![
                    ("'{path}'", "JSONB path array"),
                    ("'value'", "New value"),
                ]
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

    /// Get SQL keyword completions.
    fn get_keyword_completions(&self) -> Vec<CompletionItem> {
        const SQL_KEYWORDS: &[(&str, &str)] = &[
            ("SELECT", "Select columns from table"),
            ("FROM", "Specify source table"),
            ("WHERE", "Filter rows"),
            ("JOIN", "Join tables"),
            ("LEFT JOIN", "Left outer join"),
            ("RIGHT JOIN", "Right outer join"),
            ("INNER JOIN", "Inner join"),
            ("ORDER BY", "Sort results"),
            ("GROUP BY", "Group rows"),
            ("HAVING", "Filter groups"),
            ("LIMIT", "Limit result count"),
            ("OFFSET", "Skip rows"),
            ("INSERT INTO", "Insert data"),
            ("UPDATE", "Update data"),
            ("DELETE FROM", "Delete data"),
            ("WITH", "Common table expression"),
            ("UNION", "Combine results"),
            ("DISTINCT", "Remove duplicates"),
            ("AS", "Alias"),
            ("AND", "Logical AND"),
            ("OR", "Logical OR"),
            ("NOT", "Logical NOT"),
            ("IN", "In list"),
            ("BETWEEN", "Between range"),
            ("LIKE", "Pattern match"),
            ("IS NULL", "Check null"),
            ("IS NOT NULL", "Check not null"),
            ("EXISTS", "Check existence"),
            ("CASE", "Conditional expression"),
            ("WHEN", "Case condition"),
            ("THEN", "Case result"),
            ("ELSE", "Default case"),
            ("END", "End case/block"),
            ("CAST", "Type cast"),
            ("COALESCE", "First non-null"),
            ("NULLIF", "Return null if equal"),
        ];

        SQL_KEYWORDS
            .iter()
            .map(|(keyword, detail)| CompletionItem {
                label: keyword.to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                detail: Some(detail.to_string()),
                ..Default::default()
            })
            .collect()
    }

    /// Get JSONB function completions.
    fn get_jsonb_function_completions(&self) -> Vec<CompletionItem> {
        const JSONB_FUNCTIONS: &[(&str, &str, &str)] = &[
            ("jsonb_extract_path", "jsonb_extract_path(from_json, VARIADIC path_elems)", "Extract JSON sub-object at path"),
            ("jsonb_extract_path_text", "jsonb_extract_path_text(from_json, VARIADIC path_elems)", "Extract JSON sub-object as text"),
            ("jsonb_array_elements", "jsonb_array_elements(from_json)", "Expand JSONB array to set of rows"),
            ("jsonb_array_elements_text", "jsonb_array_elements_text(from_json)", "Expand JSONB array as text"),
            ("jsonb_object_keys", "jsonb_object_keys(from_json)", "Get set of keys in outermost object"),
            ("jsonb_typeof", "jsonb_typeof(from_json)", "Get type of outermost JSON value"),
            ("jsonb_agg", "jsonb_agg(expression)", "Aggregate values as JSONB array"),
            ("jsonb_build_object", "jsonb_build_object(VARIADIC args)", "Build JSONB object from arguments"),
            ("jsonb_build_array", "jsonb_build_array(VARIADIC args)", "Build JSONB array from arguments"),
            ("jsonb_set", "jsonb_set(target, path, new_value [, create_if_missing])", "Set value at path"),
            ("jsonb_insert", "jsonb_insert(target, path, new_value [, insert_after])", "Insert value at path"),
            ("jsonb_path_query", "jsonb_path_query(target, path [, vars [, silent]])", "Execute JSONPath query"),
            ("jsonb_path_query_array", "jsonb_path_query_array(target, path [, vars [, silent]])", "JSONPath query as array"),
            ("jsonb_path_query_first", "jsonb_path_query_first(target, path [, vars [, silent]])", "First JSONPath result"),
            ("jsonb_path_exists", "jsonb_path_exists(target, path [, vars [, silent]])", "Check if JSONPath returns items"),
            ("jsonb_strip_nulls", "jsonb_strip_nulls(from_json)", "Remove null values recursively"),
            ("jsonb_pretty", "jsonb_pretty(from_json)", "Pretty print JSONB"),
            ("jsonb_each", "jsonb_each(from_json)", "Expand to key-value pairs"),
            ("jsonb_each_text", "jsonb_each_text(from_json)", "Expand to key-text pairs"),
            ("jsonb_populate_record", "jsonb_populate_record(base, from_json)", "Populate record from JSONB"),
            ("jsonb_to_record", "jsonb_to_record(from_json)", "Convert JSONB to record"),
            ("to_jsonb", "to_jsonb(anyelement)", "Convert to JSONB"),
        ];

        JSONB_FUNCTIONS
            .iter()
            .map(|(name, signature, detail)| CompletionItem {
                label: name.to_string(),
                kind: Some(CompletionItemKind::FUNCTION),
                detail: Some(detail.to_string()),
                insert_text: Some(signature.to_string()),
                ..Default::default()
            })
            .collect()
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
    async fn initialize(&self, _params: InitializeParams) -> Result<InitializeResult> {
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
        tracing::info!("PostgreSQL LSP server initialized");

        // Refresh schema cache on initialization
        self.refresh_schema_cache().await;

        self.client
            .log_message(tower_lsp::lsp_types::MessageType::INFO, "PostgreSQL LSP ready")
            .await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        self.documents.insert(
            params.text_document.uri,
            params.text_document.text,
        );
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        if let Some(change) = params.content_changes.into_iter().next() {
            self.documents.insert(
                params.text_document.uri,
                change.text,
            );
        }
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let uri = &params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;

        let Some(text) = self.documents.get(uri) else {
            return Ok(None);
        };

        // Use parser to detect cursor context
        let context = SqlParser::get_context(&text, position);

        tracing::debug!(?context, "LSP completion context detected");

        // Get completions based on context
        let items = self.get_completions_for_context(&context).await;

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
            } => {
                // Get FHIR element hover info
                self.get_fhir_path_hover(table, column, path, operator).await
            }
            CursorContext::FromClause { partial } if !partial.is_empty() => {
                // Get table hover info
                self.get_table_hover(partial)
            }
            CursorContext::SelectColumns { partial, .. } if !partial.is_empty() => {
                // Try function hover first
                if let Some(info) = self.get_function_hover(partial) {
                    return Some(info);
                }
                // Try table hover
                self.get_table_hover(partial)
            }
            CursorContext::WhereClause { partial, .. } if !partial.is_empty() => {
                // Try function hover
                if let Some(info) = self.get_function_hover(partial) {
                    return Some(info);
                }
                // Try operator hover
                self.get_operator_hover(partial)
            }
            CursorContext::FunctionArgs { function, arg_index } => {
                self.get_function_arg_hover(function, *arg_index)
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
        if let Some(elem) = self.fhir_resolver.get_element(&resource_type, &element_path).await {
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
        let tables = self.schema_cache.get_tables_matching(table_name);
        let table = tables.first()?;

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
                let nullable = if col.is_nullable { "nullable" } else { "not null" };
                hover.push_str(&format!("- `{}`: {} ({})\n", col.name, col.data_type, nullable));
            }
            if columns.len() > 10 {
                hover.push_str(&format!("- ... and {} more columns\n", columns.len() - 10));
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
        let (func_name, args_info): (&str, Vec<(&str, &str)>) = match function.to_lowercase().as_str() {
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
                    ("create_if_missing", "Create path if missing (default: true)"),
                ],
            ),
            "jsonb_insert" => (
                "jsonb_insert",
                vec![
                    ("target", "JSONB value to modify"),
                    ("path", "Text array specifying insertion point"),
                    ("new_value", "JSONB value to insert"),
                    ("insert_after", "Insert after (true) or before (false) path position"),
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
}
