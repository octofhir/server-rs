//! SQL formatting
//!
//! This module provides mandatory SQL formatting based on sqlstyle.guide.
//! All formatting rules are hardcoded with no configuration options.
//!
//! Uses pg_query for 100% PostgreSQL syntax coverage with zero-loss guarantee.

pub mod style;
mod formatter;
mod post_processor;
mod postgres_extensions;

#[cfg(test)]
mod tests;

pub use formatter::*;
pub use post_processor::*;
pub use postgres_extensions::*;
