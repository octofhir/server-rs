//! PostgreSQL Language Server implementation backed by the Mold SQL stack.

use super::formatter_config::LspFormatterConfig;
use async_lsp::lsp_types::notification::PublishDiagnostics;
use async_lsp::lsp_types::{
    CompletionItem as LspCompletionItem, CompletionItemKind as LspCompletionItemKind,
    CompletionOptions, CompletionParams, CompletionResponse, Diagnostic, DiagnosticSeverity,
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    DocumentFormattingParams, InitializeParams, InitializeResult, OneOf, Position,
    PublishDiagnosticsParams, Range, ServerCapabilities, TextDocumentSyncCapability,
    TextDocumentSyncKind, TextEdit, Url,
};
use async_lsp::{ClientSocket, LanguageServer, ResponseError};
use futures::future::BoxFuture;
use mold_completion::types::{CompletionItemKind, TableType};
use mold_completion::{CompletionRequest, FunctionProvider, SchemaProvider, complete};
use mold_hir::{
    ColumnInfo as HirColumnInfo, DataType as HirDataType, SchemaProvider as HirSchemaProvider,
    Severity as HirSeverity, TableInfo as HirTableInfo, TableType as HirTableType, analyze_query,
};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use text_size::{TextRange, TextSize};

use super::SchemaCache;

#[derive(Debug, Clone)]
struct DocumentState {
    text: String,
    version: i32,
}

pub struct PostgresLspServer {
    client: ClientSocket,
    documents: HashMap<Url, DocumentState>,
    schema_cache: Arc<SchemaCache>,
    model_snapshot: Arc<ModelSchemaSnapshot>,
}

impl PostgresLspServer {
    pub fn new(
        client: ClientSocket,
        db_pool: Arc<sqlx_postgres::PgPool>,
        octofhir_provider: Arc<crate::model_provider::OctoFhirModelProvider>,
    ) -> Self {
        let schema_cache = Arc::new(SchemaCache::new(db_pool));
        let model_snapshot = Arc::new(ModelSchemaSnapshot::build(octofhir_provider));
        Self {
            client,
            documents: HashMap::new(),
            schema_cache,
            model_snapshot,
        }
    }

    fn position_to_offset(text: &str, position: Position) -> usize {
        let mut offset = 0usize;
        let target_line = position.line as usize;
        let target_char = position.character as usize;

        for (line_index, line) in text.lines().enumerate() {
            if line_index < target_line {
                offset += line.len() + 1;
            } else if line_index == target_line {
                offset += target_char.min(line.len());
                break;
            }
        }

        offset
    }

    fn offset_to_position(text: &str, offset: usize) -> Position {
        let mut current_offset = 0usize;
        let mut last_line = 0u32;
        let mut last_line_len = 0usize;

        for (line_index, line_text) in text.lines().enumerate() {
            last_line = line_index as u32;
            last_line_len = line_text.len();

            let line_end = current_offset + line_text.len();
            if offset <= line_end {
                return Position {
                    line: line_index as u32,
                    character: (offset - current_offset) as u32,
                };
            }

            current_offset = line_end + 1; // +1 for newline
        }

        // Offset is past the end of processed lines
        // This happens when text ends with a newline or when text is empty
        if text.is_empty() {
            Position {
                line: 0,
                character: 0,
            }
        } else if text.ends_with('\n') {
            // Position is at the start of the line after the trailing newline
            Position {
                line: last_line + 1,
                character: 0,
            }
        } else {
            // Position is at the end of the last line
            Position {
                line: last_line,
                character: last_line_len as u32,
            }
        }
    }

    fn text_range_to_lsp_range(text: &str, range: TextRange) -> Range {
        let start = usize::from(range.start());
        let end = usize::from(range.end());
        Range {
            start: Self::offset_to_position(text, start),
            end: Self::offset_to_position(text, end),
        }
    }

    fn collect_parse_diagnostics(text: &str) -> (mold_syntax::Parse, Vec<Diagnostic>) {
        let parse = mold_parser::parse(text);
        let diagnostics = parse
            .errors()
            .iter()
            .map(|err| Diagnostic {
                range: Self::text_range_to_lsp_range(text, err.range),
                severity: Some(DiagnosticSeverity::ERROR),
                code: None,
                code_description: None,
                source: Some("mold".to_string()),
                message: err.message.clone(),
                related_information: None,
                tags: None,
                data: None,
            })
            .collect();
        (parse, diagnostics)
    }

    fn collect_semantic_diagnostics(
        text: &str,
        parse: &mold_syntax::Parse,
        schema_cache: &SchemaCache,
        model_snapshot: &ModelSchemaSnapshot,
    ) -> Vec<Diagnostic> {
        let provider = HirSchemaProviderAdapter::new(schema_cache, model_snapshot);
        let analysis = analyze_query(parse, &provider);

        analysis
            .diagnostics
            .into_iter()
            .filter_map(|diag| {
                let range = diag.range.map(|r| Self::text_range_to_lsp_range(text, r))?;
                let severity = Some(match diag.severity {
                    HirSeverity::Error => DiagnosticSeverity::ERROR,
                    HirSeverity::Warning => DiagnosticSeverity::WARNING,
                    HirSeverity::Info => DiagnosticSeverity::INFORMATION,
                    HirSeverity::Hint => DiagnosticSeverity::HINT,
                    _ => DiagnosticSeverity::INFORMATION, // Handle future variants
                });

                Some(Diagnostic {
                    range,
                    severity,
                    code: None,
                    code_description: None,
                    source: Some("mold-hir".to_string()),
                    message: diag.message,
                    related_information: None,
                    tags: None,
                    data: None,
                })
            })
            .collect()
    }

    fn publish_diagnostics(&self, uri: Url, version: i32, text: String) {
        let (parse, mut diagnostics) = Self::collect_parse_diagnostics(&text);

        // Also run semantic analysis if parse succeeded (no critical errors)
        let semantic_diagnostics = Self::collect_semantic_diagnostics(
            &text,
            &parse,
            &self.schema_cache,
            &self.model_snapshot,
        );
        diagnostics.extend(semantic_diagnostics);

        let params = PublishDiagnosticsParams {
            uri,
            diagnostics,
            version: Some(version),
        };

        if let Err(err) = self.client.notify::<PublishDiagnostics>(params) {
            tracing::warn!(error = %err, "Failed to publish diagnostics");
        }
    }

    fn map_completion_kind(kind: CompletionItemKind) -> Option<LspCompletionItemKind> {
        match kind {
            CompletionItemKind::Table => Some(LspCompletionItemKind::CLASS),
            CompletionItemKind::View => Some(LspCompletionItemKind::STRUCT),
            CompletionItemKind::Column => Some(LspCompletionItemKind::FIELD),
            CompletionItemKind::Function => Some(LspCompletionItemKind::FUNCTION),
            CompletionItemKind::Keyword => Some(LspCompletionItemKind::KEYWORD),
            CompletionItemKind::Alias => Some(LspCompletionItemKind::VARIABLE),
            CompletionItemKind::Schema => Some(LspCompletionItemKind::MODULE),
            CompletionItemKind::JsonbPath => Some(LspCompletionItemKind::FIELD),
            CompletionItemKind::Snippet => Some(LspCompletionItemKind::SNIPPET),
            CompletionItemKind::Type => Some(LspCompletionItemKind::TYPE_PARAMETER),
            CompletionItemKind::Operator => Some(LspCompletionItemKind::OPERATOR),
            _ => None,
        }
    }

    fn to_lsp_completion_item(item: mold_completion::types::CompletionItem) -> LspCompletionItem {
        let insert_text_format = if matches!(item.kind, CompletionItemKind::Snippet) {
            Some(async_lsp::lsp_types::InsertTextFormat::SNIPPET)
        } else {
            None
        };

        LspCompletionItem {
            label: item.label,
            kind: Self::map_completion_kind(item.kind),
            detail: item.detail,
            documentation: item
                .documentation
                .map(async_lsp::lsp_types::Documentation::String),
            insert_text: item.insert_text,
            insert_text_format,
            sort_text: item.sort_key,
            filter_text: item.filter_text,
            ..Default::default()
        }
    }
}

impl LanguageServer for PostgresLspServer {
    type Error = ResponseError;
    type NotifyResult = std::ops::ControlFlow<async_lsp::Result<()>>;

    fn initialize(
        &mut self,
        _params: InitializeParams,
    ) -> BoxFuture<'static, Result<InitializeResult, Self::Error>> {
        Box::pin(async move {
            Ok(InitializeResult {
                capabilities: ServerCapabilities {
                    text_document_sync: Some(TextDocumentSyncCapability::Kind(
                        TextDocumentSyncKind::FULL,
                    )),
                    completion_provider: Some(CompletionOptions {
                        trigger_characters: Some(vec![
                            ".".into(),
                            " ".into(),
                            "(".into(),
                            // JSONB operator triggers
                            ">".into(), // After -> or #>
                            "'".into(), // Inside string literals
                            "{".into(), // Inside array literals like '{name}'
                            ",".into(), // Between elements in array literals
                        ]),
                        resolve_provider: Some(false),
                        ..Default::default()
                    }),
                    document_formatting_provider: Some(OneOf::Left(true)),
                    ..Default::default()
                },
                ..Default::default()
            })
        })
    }

    fn initialized(
        &mut self,
        _params: async_lsp::lsp_types::InitializedParams,
    ) -> Self::NotifyResult {
        let schema_cache = self.schema_cache.clone();
        tokio::spawn(async move {
            if let Err(err) = schema_cache.refresh().await {
                tracing::warn!(error = %err, "Failed to refresh LSP schema cache");
            }
        });

        std::ops::ControlFlow::Continue(())
    }

    fn shutdown(&mut self, _params: ()) -> BoxFuture<'static, Result<(), Self::Error>> {
        Box::pin(async move { Ok(()) })
    }

    fn did_open(&mut self, params: DidOpenTextDocumentParams) -> Self::NotifyResult {
        let uri = params.text_document.uri;
        let text = params.text_document.text;
        let version = params.text_document.version;

        self.documents.insert(
            uri.clone(),
            DocumentState {
                text: text.clone(),
                version,
            },
        );

        self.publish_diagnostics(uri, version, text);

        std::ops::ControlFlow::Continue(())
    }

    fn did_change(&mut self, params: DidChangeTextDocumentParams) -> Self::NotifyResult {
        let uri = params.text_document.uri;
        let version = params.text_document.version;

        if let Some(state) = self.documents.get_mut(&uri)
            && let Some(change) = params.content_changes.into_iter().last()
        {
            state.text = change.text.clone();
            state.version = version;
            self.publish_diagnostics(uri, version, change.text);
        }

        std::ops::ControlFlow::Continue(())
    }

    fn did_close(&mut self, params: DidCloseTextDocumentParams) -> Self::NotifyResult {
        let uri = params.text_document.uri;
        self.documents.remove(&uri);

        let params = PublishDiagnosticsParams {
            uri,
            diagnostics: Vec::new(),
            version: None,
        };

        if let Err(err) = self.client.notify::<PublishDiagnostics>(params) {
            tracing::warn!(error = %err, "Failed to clear diagnostics");
        }

        std::ops::ControlFlow::Continue(())
    }

    fn completion(
        &mut self,
        params: CompletionParams,
    ) -> BoxFuture<'static, Result<Option<CompletionResponse>, Self::Error>> {
        let uri = params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;
        let doc_state = self.documents.get(&uri).cloned();
        let schema_cache = self.schema_cache.clone();
        let model_snapshot = self.model_snapshot.clone();

        Box::pin(async move {
            let Some(state) = doc_state else {
                return Ok(Some(CompletionResponse::List(
                    async_lsp::lsp_types::CompletionList {
                        is_incomplete: false,
                        items: Vec::new(),
                    },
                )));
            };

            if (schema_cache.get_tables().is_empty() || schema_cache.get_functions().is_empty())
                && let Err(err) = schema_cache.refresh().await
            {
                tracing::warn!(error = %err, "Failed to refresh LSP schema cache");
            }

            // Preload FHIR schemas for all known tables (async, before synchronous completion)
            model_snapshot.preload_fhir_schemas(&schema_cache).await;

            let offset = Self::position_to_offset(&state.text, position);
            let parse = mold_parser::parse(&state.text);
            let offset = TextSize::new(offset as u32);

            let provider = SchemaProviderAdapter::new(schema_cache, model_snapshot);
            let request = CompletionRequest::new(&state.text, offset)
                .with_parse(&parse)
                .with_schema_provider(&provider)
                .with_function_provider(&provider)
                .with_limit(0);

            let result = complete(request);
            let items = result
                .items
                .into_iter()
                .map(Self::to_lsp_completion_item)
                .collect();

            Ok(Some(CompletionResponse::List(
                async_lsp::lsp_types::CompletionList {
                    is_incomplete: result.is_incomplete,
                    items,
                },
            )))
        })
    }

    fn hover(
        &mut self,
        _params: async_lsp::lsp_types::HoverParams,
    ) -> BoxFuture<'static, Result<Option<async_lsp::lsp_types::Hover>, Self::Error>> {
        Box::pin(async move { Ok(None) })
    }

    fn formatting(
        &mut self,
        params: DocumentFormattingParams,
    ) -> BoxFuture<'static, Result<Option<Vec<TextEdit>>, Self::Error>> {
        let uri = params.text_document.uri;
        let doc_state = self.documents.get(&uri).cloned();
        let options = params.options;

        Box::pin(async move {
            let Some(state) = doc_state else {
                return Ok(None);
            };

            // Parse formatter config from LSP options
            let config = LspFormatterConfig::from_lsp_options(&options);
            let formatted = config.format(&state.text);

            let end_position = Self::offset_to_position(&state.text, state.text.len());
            let edit = TextEdit {
                range: Range {
                    start: Position {
                        line: 0,
                        character: 0,
                    },
                    end: end_position,
                },
                new_text: formatted,
            };

            Ok(Some(vec![edit]))
        })
    }
}

struct ModelSchemaSnapshot {
    /// Cached JSONB schemas by resource type (lazy loaded)
    jsonb_cache: dashmap::DashMap<String, mold_completion::types::JsonbSchema>,
    /// The model provider for loading schemas
    model_provider: Arc<crate::model_provider::OctoFhirModelProvider>,
}

impl ModelSchemaSnapshot {
    /// Create a snapshot with lazy loading support.
    ///
    /// FHIR schemas are loaded on-demand from the model provider.
    fn build(provider: Arc<crate::model_provider::OctoFhirModelProvider>) -> Self {
        Self {
            jsonb_cache: dashmap::DashMap::new(),
            model_provider: provider,
        }
    }

    /// Get cached JSONB schema for a resource type.
    /// Returns None if not cached - call `preload_schema` first.
    fn get_jsonb_schema(&self, resource_type: &str) -> Option<mold_completion::types::JsonbSchema> {
        let key = resource_type.to_lowercase();
        self.jsonb_cache.get(&key).map(|s| s.clone())
    }

    /// Asynchronously preload JSONB schema for a resource type.
    /// The schema will be cached and available via `get_jsonb_schema`.
    async fn preload_schema(&self, resource_type: &str) {
        let key = resource_type.to_lowercase();

        // Skip if already cached
        if self.jsonb_cache.contains_key(&key) {
            return;
        }

        // Load from model provider
        if let Some(fhir_schema) = self.model_provider.get_schema(resource_type).await {
            let schema =
                Self::build_jsonb_schema_from_fhir(&fhir_schema, &self.model_provider).await;
            self.jsonb_cache.insert(key, schema);
        }
    }

    /// Preload schemas for all FHIR tables found in the schema cache.
    async fn preload_fhir_schemas(&self, schema_cache: &SchemaCache) {
        let tables = schema_cache.get_tables();
        for table in tables {
            if let Some(resource_type) = schema_cache.get_fhir_resource_type(&table.name) {
                self.preload_schema(&resource_type).await;
            }
        }
    }

    /// Build JSONB schema from FHIR schema (async version with type loading).
    async fn build_jsonb_schema_from_fhir(
        fhir_schema: &octofhir_fhirschema::FhirSchema,
        provider: &crate::model_provider::OctoFhirModelProvider,
    ) -> mold_completion::types::JsonbSchema {
        let mut seen = HashSet::new();
        if let Some(elements) = &fhir_schema.elements {
            Self::build_jsonb_schema_recursive(elements, provider, &mut seen, 0).await
        } else {
            mold_completion::types::JsonbSchema::new()
        }
    }

    /// Recursively build JSONB schema from FHIR elements.
    fn build_jsonb_schema_recursive<'a>(
        elements: &'a std::collections::HashMap<String, octofhir_fhirschema::FhirSchemaElement>,
        provider: &'a crate::model_provider::OctoFhirModelProvider,
        seen: &'a mut HashSet<String>,
        depth: usize,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = mold_completion::types::JsonbSchema> + Send + 'a>,
    > {
        Box::pin(async move {
            // Limit recursion depth to avoid infinite loops and excessive loading
            const MAX_DEPTH: usize = 5;
            if depth >= MAX_DEPTH {
                return mold_completion::types::JsonbSchema::new();
            }

            let mut schema = mold_completion::types::JsonbSchema::new();

            for (name, element) in elements {
                let field_type = Self::jsonb_field_type(element);
                let mut field = mold_completion::types::JsonbField::new(name.clone(), field_type);

                if let Some(description) = &element.short {
                    field = field.with_description(description.clone());
                }

                // Handle inline nested elements (BackboneElement)
                if let Some(nested) = element.elements.as_ref().filter(|v| !v.is_empty()) {
                    let nested_schema =
                        Self::build_jsonb_schema_recursive(nested, provider, seen, depth + 1).await;
                    field = field.with_nested(nested_schema);
                }
                // Handle type references - dynamically load schema for any non-primitive type
                else if let Some(type_name) = element.type_name.as_deref() {
                    // Skip primitive types that definitely won't have schemas
                    let is_primitive = Self::is_primitive_type(type_name);
                    let already_seen = !seen.insert(type_name.to_string());

                    if !is_primitive && !already_seen {
                        // Try to load schema from model provider - if type doesn't exist, returns None
                        if let Some(type_schema) = provider.get_schema(type_name).await {
                            if let Some(type_elements) = &type_schema.elements {
                                tracing::debug!(
                                    type_name,
                                    element_count = type_elements.len(),
                                    "Building nested schema for type"
                                );
                                let nested_schema = Self::build_jsonb_schema_recursive(
                                    type_elements,
                                    provider,
                                    seen,
                                    depth + 1,
                                )
                                .await;
                                field = field.with_nested(nested_schema);
                            } else {
                                tracing::debug!(type_name, "Type schema has no elements");
                            }
                        } else {
                            tracing::debug!(type_name, "Failed to load schema for type");
                        }
                        seen.remove(type_name);
                    } else if is_primitive {
                        tracing::trace!(type_name, "Skipping primitive type");
                    } else if already_seen {
                        tracing::trace!(type_name, "Skipping already-seen type");
                    }
                }

                schema = schema.with_field(field);
            }

            schema
        })
    }

    /// Check if a FHIR type is a primitive type (no nested elements).
    /// These types don't have schemas with elements in the database.
    fn is_primitive_type(type_name: &str) -> bool {
        matches!(
            type_name,
            // FHIR primitive types (from FHIR spec)
            "boolean"
                | "integer"
                | "integer64"
                | "string"
                | "decimal"
                | "uri"
                | "url"
                | "canonical"
                | "base64Binary"
                | "instant"
                | "date"
                | "dateTime"
                | "time"
                | "code"
                | "oid"
                | "id"
                | "markdown"
                | "unsignedInt"
                | "positiveInt"
                | "uuid"
                | "xhtml"
                // Also skip "Element" as it's the base type
                | "Element"
                // Skip "Resource" base types
                | "Resource"
                | "DomainResource"
                // Skip primitive type wrappers (these are the same as primitives)
                | "http://hl7.org/fhirpath/System.String"
                | "http://hl7.org/fhirpath/System.Boolean"
                | "http://hl7.org/fhirpath/System.Integer"
                | "http://hl7.org/fhirpath/System.Decimal"
                | "http://hl7.org/fhirpath/System.DateTime"
                | "http://hl7.org/fhirpath/System.Date"
                | "http://hl7.org/fhirpath/System.Time"
        )
    }

    /// Get field names at a specific JSONB path for FHIR path validation.
    ///
    /// Returns:
    /// - `Some(vec![...])` - fields exist at this path
    /// - `Some(vec![])` - path exists but has no known nested fields (we know this path)
    /// - `None` - resource type not found or path doesn't exist in schema
    fn get_fields_at_path(&self, resource_type: &str, path: &[&str]) -> Option<Vec<String>> {
        let schema = self.get_jsonb_schema(resource_type)?;

        // First check if the path exists in the schema
        if !path.is_empty() {
            // Check each path segment exists
            // Skip numeric segments (array indices like "0", "1") as they don't affect field structure
            let mut current_fields = &schema.fields;
            for segment in path {
                // Skip numeric segments (array indices)
                if segment.parse::<i32>().is_ok() {
                    continue;
                }

                if let Some(field) = current_fields
                    .iter()
                    .find(|f| f.name.eq_ignore_ascii_case(segment))
                {
                    if let Some(ref nested) = field.nested {
                        current_fields = &nested.fields;
                    } else {
                        // Field exists but has no nested schema
                        // Return empty vec to indicate "path exists, no nested fields known"
                        return Some(Vec::new());
                    }
                } else {
                    // Path segment doesn't exist - return None to indicate unknown
                    return None;
                }
            }
            Some(current_fields.iter().map(|f| f.name.clone()).collect())
        } else {
            // Root level - return all fields
            Some(schema.fields.iter().map(|f| f.name.clone()).collect())
        }
    }

    /// Check if a field at a specific JSONB path is an array.
    ///
    /// The path should point to the field to check (e.g., `["name"]` to check if `name` is an array).
    ///
    /// Returns:
    /// - `Some(true)` - field is an array
    /// - `Some(false)` - field exists but is not an array
    /// - `None` - resource type not found or path doesn't exist in schema
    fn is_field_array(&self, resource_type: &str, path: &[&str]) -> Option<bool> {
        let schema = self.get_jsonb_schema(resource_type)?;

        if path.is_empty() {
            // Root level is not an array
            return Some(false);
        }

        let mut current_fields = &schema.fields;
        let mut last_field: Option<&mold_completion::types::JsonbField> = None;

        for segment in path {
            // Skip numeric segments (array indices)
            if segment.parse::<i32>().is_ok() {
                continue;
            }

            if let Some(field) = current_fields
                .iter()
                .find(|f| f.name.eq_ignore_ascii_case(segment))
            {
                last_field = Some(field);
                if let Some(ref nested) = field.nested {
                    current_fields = &nested.fields;
                }
            } else {
                // Path segment doesn't exist
                return None;
            }
        }

        // Check if the last field we found is an array
        last_field.map(|f| f.field_type == mold_completion::types::JsonbFieldType::Array)
    }

    /// Determine JSONB field type from FHIR schema element.
    fn jsonb_field_type(
        element: &octofhir_fhirschema::FhirSchemaElement,
    ) -> mold_completion::types::JsonbFieldType {
        let is_array = element.array.unwrap_or(false);
        let is_object = element.elements.is_some();

        let base_type = if is_object {
            mold_completion::types::JsonbFieldType::Object
        } else {
            let type_name = element.type_name.as_deref().unwrap_or("Any");
            match type_name {
                "boolean" => mold_completion::types::JsonbFieldType::Boolean,
                "integer" | "decimal" | "unsignedInt" | "positiveInt" => {
                    mold_completion::types::JsonbFieldType::Number
                }
                "string" | "code" | "id" | "uri" | "url" | "canonical" | "markdown"
                | "base64Binary" | "date" | "dateTime" | "instant" | "time" | "oid" | "uuid" => {
                    mold_completion::types::JsonbFieldType::String
                }
                _ => mold_completion::types::JsonbFieldType::Unknown,
            }
        };

        if is_array {
            mold_completion::types::JsonbFieldType::Array
        } else {
            base_type
        }
    }
}

struct SchemaProviderAdapter {
    schema_cache: Arc<SchemaCache>,
    model_snapshot: Arc<ModelSchemaSnapshot>,
}

impl SchemaProviderAdapter {
    fn new(schema_cache: Arc<SchemaCache>, model_snapshot: Arc<ModelSchemaSnapshot>) -> Self {
        Self {
            schema_cache,
            model_snapshot,
        }
    }
}

impl SchemaProvider for SchemaProviderAdapter {
    fn tables(&self) -> Vec<mold_completion::types::TableInfo> {
        self.schema_cache
            .get_tables()
            .into_iter()
            .map(|table| {
                let table_type = match table.table_type.as_str() {
                    "VIEW" => TableType::View,
                    "MATERIALIZED VIEW" => TableType::MaterializedView,
                    "FOREIGN TABLE" => TableType::ForeignTable,
                    _ => TableType::Table,
                };

                let mut info = mold_completion::types::TableInfo::new(table.name)
                    .with_schema(table.schema)
                    .with_type(table_type);

                if let Some(resource_type) = table.fhir_resource_type {
                    info = info.with_description(format!("FHIR resource: {}", resource_type));
                }

                info
            })
            .collect()
    }

    fn columns(
        &self,
        schema: Option<&str>,
        table: &str,
    ) -> Vec<mold_completion::types::ColumnInfo> {
        let columns = match schema {
            Some(schema) => self.schema_cache.get_columns_in_schema(schema, table),
            None => self.schema_cache.get_columns(table),
        };
        columns
            .into_iter()
            .enumerate()
            .map(|(index, column)| {
                let mut info =
                    mold_completion::types::ColumnInfo::new(column.name, column.data_type)
                        .with_nullable(column.is_nullable)
                        .with_ordinal(index);

                if let Some(description) = column.description {
                    info = info.with_description(description);
                }

                info
            })
            .collect()
    }

    fn jsonb_schema(
        &self,
        _schema: Option<&str>,
        table: &str,
        column: &str,
    ) -> Option<mold_completion::types::JsonbSchema> {
        if column != "resource" {
            return None;
        }

        let resource_type = self.schema_cache.get_fhir_resource_type(table)?;
        self.model_snapshot.get_jsonb_schema(&resource_type)
    }
}

impl FunctionProvider for SchemaProviderAdapter {
    fn functions(&self) -> Vec<mold_completion::types::FunctionInfo> {
        self.schema_cache
            .get_functions()
            .into_iter()
            .map(|func| {
                mold_completion::types::FunctionInfo::new(func.name, func.return_type)
                    .with_description(func.description)
            })
            .collect()
    }
}

// =============================================================================
// HIR Schema Provider (for semantic analysis / diagnostics)
// =============================================================================

struct HirSchemaProviderAdapter<'a> {
    schema_cache: &'a SchemaCache,
    model_snapshot: &'a ModelSchemaSnapshot,
}

impl<'a> HirSchemaProviderAdapter<'a> {
    fn new(schema_cache: &'a SchemaCache, model_snapshot: &'a ModelSchemaSnapshot) -> Self {
        Self {
            schema_cache,
            model_snapshot,
        }
    }
}

impl HirSchemaProvider for HirSchemaProviderAdapter<'_> {
    fn lookup_table(&self, schema: Option<&str>, name: &str) -> Option<HirTableInfo> {
        let tables = self.schema_cache.get_tables();
        tables
            .into_iter()
            .find(|t| {
                t.name.eq_ignore_ascii_case(name)
                    && (schema.is_none() || t.schema.eq_ignore_ascii_case(schema.unwrap_or("")))
            })
            .map(|t| HirTableInfo {
                schema: Some(t.schema),
                name: t.name,
                table_type: match t.table_type.as_str() {
                    "VIEW" => HirTableType::View,
                    "MATERIALIZED VIEW" => HirTableType::MaterializedView,
                    "FOREIGN TABLE" => HirTableType::ForeignTable,
                    _ => HirTableType::Table,
                },
            })
    }

    fn lookup_columns(&self, schema: Option<&str>, table: &str) -> Vec<HirColumnInfo> {
        let columns = match schema {
            Some(s) => self.schema_cache.get_columns_in_schema(s, table),
            None => self.schema_cache.get_columns(table),
        };

        columns
            .into_iter()
            .enumerate()
            .map(|(i, col)| HirColumnInfo {
                name: col.name,
                data_type: parse_data_type(&col.data_type),
                nullable: col.is_nullable,
                ordinal: i,
            })
            .collect()
    }

    fn table_exists(&self, schema: Option<&str>, name: &str) -> bool {
        self.lookup_table(schema, name).is_some()
    }

    fn schema_exists(&self, schema: &str) -> bool {
        let tables = self.schema_cache.get_tables();
        tables.iter().any(|t| t.schema.eq_ignore_ascii_case(schema))
    }

    fn all_table_names(&self) -> Vec<String> {
        self.schema_cache
            .get_tables()
            .into_iter()
            .map(|t| t.name)
            .collect()
    }

    fn all_schema_names(&self) -> Vec<String> {
        let tables = self.schema_cache.get_tables();
        let mut schemas: Vec<String> = tables.into_iter().map(|t| t.schema).collect();
        schemas.sort();
        schemas.dedup();
        schemas
    }

    fn lookup_jsonb_fields(
        &self,
        _schema: Option<&str>,
        table: &str,
        column: &str,
        path: &[&str],
    ) -> Option<Vec<String>> {
        // Only support 'resource' column for FHIR tables
        if column != "resource" {
            return None;
        }

        // Get the FHIR resource type for this table
        let resource_type = self.schema_cache.get_fhir_resource_type(table)?;

        // Get fields at the specified path
        self.model_snapshot.get_fields_at_path(&resource_type, path)
    }

    fn jsonb_field_is_array(
        &self,
        _schema: Option<&str>,
        table: &str,
        column: &str,
        path: &[&str],
    ) -> Option<bool> {
        // Only support 'resource' column for FHIR tables
        if column != "resource" {
            return None;
        }

        // Get the FHIR resource type for this table
        let resource_type = self.schema_cache.get_fhir_resource_type(table)?;

        // Check if the field at the specified path is an array
        self.model_snapshot.is_field_array(&resource_type, path)
    }
}

/// Parse a PostgreSQL data type string into an HIR DataType.
fn parse_data_type(type_str: &str) -> HirDataType {
    let type_lower = type_str.to_lowercase();
    match type_lower.as_str() {
        "integer" | "int" | "int4" | "serial" => HirDataType::Integer,
        "bigint" | "int8" | "bigserial" => HirDataType::BigInt,
        "smallint" | "int2" | "smallserial" => HirDataType::SmallInt,
        "boolean" | "bool" => HirDataType::Boolean,
        "text" | "varchar" | "character varying" | "char" | "character" => HirDataType::Text,
        "jsonb" => HirDataType::Jsonb,
        "json" => HirDataType::Json,
        "uuid" => HirDataType::Uuid,
        "timestamp" | "timestamp without time zone" => HirDataType::Timestamp {
            with_timezone: false,
        },
        "timestamptz" | "timestamp with time zone" => HirDataType::Timestamp {
            with_timezone: true,
        },
        "date" => HirDataType::Date,
        "time" | "time without time zone" => HirDataType::Time {
            with_timezone: false,
        },
        "timetz" | "time with time zone" => HirDataType::Time {
            with_timezone: true,
        },
        "interval" => HirDataType::Interval,
        "numeric" | "decimal" => HirDataType::Numeric {
            precision: None,
            scale: None,
        },
        "real" | "float4" => HirDataType::Real,
        "double precision" | "float8" => HirDataType::DoublePrecision,
        "bytea" => HirDataType::ByteA,
        _ => {
            // Check for array types
            if type_lower.ends_with("[]") {
                HirDataType::Array(Box::new(parse_data_type(&type_str[..type_str.len() - 2])))
            } else {
                HirDataType::Custom(type_str.to_string())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_offset_to_position_simple() {
        // "hello" - 5 chars, no newline
        let text = "hello";
        assert_eq!(
            PostgresLspServer::offset_to_position(text, 0),
            Position {
                line: 0,
                character: 0
            }
        );
        assert_eq!(
            PostgresLspServer::offset_to_position(text, 3),
            Position {
                line: 0,
                character: 3
            }
        );
        assert_eq!(
            PostgresLspServer::offset_to_position(text, 5),
            Position {
                line: 0,
                character: 5
            }
        );
    }

    #[test]
    fn test_offset_to_position_with_trailing_newline() {
        // "hello\n" - 6 chars, ends with newline
        // This is the critical case that was causing the formatter bug
        let text = "hello\n";
        assert_eq!(
            PostgresLspServer::offset_to_position(text, 0),
            Position {
                line: 0,
                character: 0
            }
        );
        assert_eq!(
            PostgresLspServer::offset_to_position(text, 5),
            Position {
                line: 0,
                character: 5
            }
        );
        // Offset 6 is after the newline - should be at line 1, char 0
        assert_eq!(
            PostgresLspServer::offset_to_position(text, 6),
            Position {
                line: 1,
                character: 0
            }
        );
    }

    #[test]
    fn test_offset_to_position_multiline() {
        // "hello\nworld" - 11 chars
        let text = "hello\nworld";
        assert_eq!(
            PostgresLspServer::offset_to_position(text, 0),
            Position {
                line: 0,
                character: 0
            }
        );
        assert_eq!(
            PostgresLspServer::offset_to_position(text, 5),
            Position {
                line: 0,
                character: 5
            }
        );
        // Offset 6 is 'w' in "world"
        assert_eq!(
            PostgresLspServer::offset_to_position(text, 6),
            Position {
                line: 1,
                character: 0
            }
        );
        assert_eq!(
            PostgresLspServer::offset_to_position(text, 11),
            Position {
                line: 1,
                character: 5
            }
        );
    }

    #[test]
    fn test_offset_to_position_multiline_trailing_newline() {
        // "hello\nworld\n" - 12 chars
        let text = "hello\nworld\n";
        assert_eq!(
            PostgresLspServer::offset_to_position(text, 11),
            Position {
                line: 1,
                character: 5
            }
        );
        // Offset 12 is after the trailing newline
        assert_eq!(
            PostgresLspServer::offset_to_position(text, 12),
            Position {
                line: 2,
                character: 0
            }
        );
    }

    #[test]
    fn test_offset_to_position_empty() {
        let text = "";
        assert_eq!(
            PostgresLspServer::offset_to_position(text, 0),
            Position {
                line: 0,
                character: 0
            }
        );
    }

    #[test]
    fn test_offset_to_position_just_newline() {
        let text = "\n";
        assert_eq!(
            PostgresLspServer::offset_to_position(text, 0),
            Position {
                line: 0,
                character: 0
            }
        );
        assert_eq!(
            PostgresLspServer::offset_to_position(text, 1),
            Position {
                line: 1,
                character: 0
            }
        );
    }
}
