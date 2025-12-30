//! Language Server Protocol implementations for OctoFHIR.
//!
//! Provides LSP support for:
//! - PostgreSQL SQL editing (pg-lsp)
//! - FHIRPath expression editing

mod fhir_resolver;
mod formatter_config;
mod schema_cache;

// PostgreSQL LSP
mod pg_handler;
mod pg_server;

// FHIRPath LSP
mod fhirpath_handler;

pub use fhir_resolver::{ElementInfo, FhirResolver, LoadingState};
pub use formatter_config::{
    CommaStyle, IdentifierCase, KeywordCase, LspFormatterConfig, PgFormatterStyleConfig,
    SqlStyleConfig,
};
pub use pg_handler::pg_lsp_websocket_handler;
pub use fhirpath_handler::fhirpath_lsp_websocket_handler;
pub use schema_cache::{ColumnInfo, FunctionInfo, SchemaCache, TableInfo};
pub use pg_server::PostgresLspServer;
