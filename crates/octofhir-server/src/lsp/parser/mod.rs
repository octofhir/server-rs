//! SQL parsing utilities
//!
//! This module handles cursor context detection, JSONB operator detection,
//! and table alias resolution.

mod jsonb_detector;
pub mod table_resolver;

// Re-export legacy parser types (to be migrated gradually)
pub use super::parser_legacy::{CursorContext, JsonbOperator, JsonbQuoteContext, SqlParser, TableRef};

pub use jsonb_detector::*;
pub use table_resolver::*;
