//! PostgreSQL Language Server Protocol implementation.
//!
//! This module provides a basic LSP server for SQL editing with PostgreSQL-specific
//! keyword completions and hover information.

mod fhir_resolver;
mod handler;
mod parser;
mod schema_cache;
mod server;

pub use fhir_resolver::{ElementInfo, FhirResolver};
pub use handler::lsp_websocket_handler;
pub use parser::{CursorContext, JsonbOperator, SqlParser};
pub use schema_cache::{ColumnInfo, FunctionInfo, SchemaCache, TableInfo};
pub use server::PostgresLspServer;
