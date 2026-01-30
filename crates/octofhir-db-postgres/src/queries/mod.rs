//! SQL query modules for the PostgreSQL storage backend.
//!
//! This module contains the SQL query implementations organized by operation type.

pub mod crud;
pub mod history;
pub mod search;

// Re-export CRUD operations
pub use crud::{create, delete, read, read_raw, update, vread};

// Re-export history operations
pub use history::{get_history, get_system_history};

// Re-export search operations
pub use search::{
    SearchUnknownParamHandling, execute_search, execute_search_raw, execute_search_raw_with_config,
};
