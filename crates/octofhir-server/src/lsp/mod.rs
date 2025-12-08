//! PostgreSQL Language Server Protocol implementation.
//!
//! This module provides a basic LSP server for SQL editing with PostgreSQL-specific
//! keyword completions and hover information.

mod server;
mod handler;

pub use handler::lsp_websocket_handler;
pub use server::PostgresLspServer;
