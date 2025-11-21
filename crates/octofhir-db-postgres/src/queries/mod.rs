//! SQL query modules for the PostgreSQL storage backend.
//!
//! This module contains the SQL query implementations organized by operation type.

// Allow unused for now - these are placeholder modules for future implementation
#![allow(dead_code)]

pub mod crud;
pub mod history;
pub mod search;

// Re-exports will be used when implementations are complete
#[allow(unused_imports)]
pub use crud::CrudQueries;
#[allow(unused_imports)]
pub use history::HistoryQueries;
#[allow(unused_imports)]
pub use search::SearchQueries;
