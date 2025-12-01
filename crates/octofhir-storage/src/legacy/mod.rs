//! Legacy storage types for backward compatibility.
//!
//! This module contains the legacy `Storage` trait and related types that were
//! originally in `octofhir-db-memory`. These are maintained for compatibility
//! with existing handlers that use `ResourceEnvelope`-based operations.

pub mod query;
pub mod storage;
pub mod transaction;

pub use query::{QueryFilter, QueryResult, SearchQuery};
pub use storage::{DynStorage, Storage};
pub use transaction::{
    Transaction, TransactionManager, TransactionOperation, TransactionOperationResult,
    TransactionState, TransactionStats,
};
