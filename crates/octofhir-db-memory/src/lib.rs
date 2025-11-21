//! In-memory FHIR storage backend for OctoFHIR server.
//!
//! This crate provides an in-memory implementation of the `FhirStorage` trait
//! from `octofhir-storage`, using papaya lock-free HashMap for concurrent access.
//!
//! # Example
//!
//! ```ignore
//! use octofhir_db_memory::InMemoryStorage;
//! use octofhir_storage::FhirStorage;
//!
//! let storage = InMemoryStorage::new();
//!
//! // Create a patient
//! let patient = serde_json::json!({
//!     "resourceType": "Patient",
//!     "name": [{"family": "Smith"}]
//! });
//! let created = storage.create(&patient).await?;
//! ```

pub mod factory;
mod fhir_impl;
pub mod query;
pub mod storage;
pub mod transaction;

// Re-export the FhirStorage trait for convenience
pub use octofhir_storage::{FhirStorage, StorageError, StoredResource};

// Legacy Storage trait (for backward compatibility)
pub use factory::{
    DynStorage, Storage, StorageBackend, StorageConfig, StorageOptions, create_storage,
};
pub use query::{QueryFilter, QueryResult, SearchQuery};
pub use storage::{InMemoryStorage, StorageKey};
pub use transaction::{
    Transaction, TransactionManager, TransactionOperation, TransactionOperationResult,
    TransactionState, TransactionStats,
};

/// Type alias for a shareable FhirStorage instance.
pub type DynFhirStorage = std::sync::Arc<dyn FhirStorage>;

/// Creates a new in-memory FhirStorage instance.
pub fn create_fhir_storage() -> DynFhirStorage {
    std::sync::Arc::new(InMemoryStorage::new())
}
