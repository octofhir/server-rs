//! Language Server Protocol implementations for OctoFHIR.
//!
//! Provides LSP support for:
//! - PostgreSQL SQL editing (pg-lsp), over the database or a SQL on FHIR
//!   query's dependencies (`?mode=sof`)
//! - FHIRPath expression editing
//! - FHIR Mapping Language (FML) maps, with the `fml-lsp` server

mod fhir_resolver;
mod formatter_config;
mod schema_cache;

// PostgreSQL LSP
mod pg_handler;
mod pg_server;
mod sof_catalog;

// FHIRPath LSP
mod fhirpath_handler;

// FML LSP
mod fml_handler;

pub use fhir_resolver::{ElementInfo, FhirResolver, LoadingState};
pub use fhirpath_handler::fhirpath_lsp_websocket_handler;
pub use fml_handler::fml_lsp_websocket_handler;
pub use formatter_config::{
    CommaStyle, IdentifierCase, KeywordCase, LspFormatterConfig, PgFormatterStyleConfig,
    SqlStyleConfig,
};
pub use pg_handler::pg_lsp_websocket_handler;
pub use pg_server::{LspMode, PostgresLspServer};
pub use schema_cache::{ColumnInfo, FunctionInfo, SchemaCache, TableInfo};
