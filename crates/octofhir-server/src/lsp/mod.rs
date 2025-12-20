//! PostgreSQL Language Server Protocol implementation.
//!
//! This module provides a basic LSP server for SQL editing with PostgreSQL-specific
//! keyword completions and hover information.
//!
//! ## Architecture
//!
//! Uses tree-sitter AST parsing from Supabase postgres-language-server (MIT licensed)
//! for accurate SQL context detection, enabling intelligent clause-based filtering
//! of completion suggestions.

// New modular structure
pub mod completion;
pub mod diagnostics;
pub mod formatting;
pub mod hover;

// Existing modules
mod completion_filter;
mod fhir_resolver;
mod handler;
pub mod parser; // Now a module directory
mod parser_legacy; // Temporarily keep old parser.rs
mod schema_cache;
pub mod semantic_analyzer;
mod server;

pub use completion_filter::{CompletionFilter, CompletionRelevanceData};
pub use fhir_resolver::{ElementInfo, FhirResolver};
pub use handler::lsp_websocket_handler;
pub use parser::{CursorContext, JsonbOperator, SqlParser};
pub use schema_cache::{ColumnInfo, FunctionInfo, SchemaCache, TableInfo};
pub use server::{JsonbCompletionContext, JsonbContext, JsonbDetectionResult, PostgresLspServer};
