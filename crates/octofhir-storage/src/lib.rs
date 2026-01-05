//! # octofhir-storage
//!
//! Storage abstraction layer for the OctoFHIR server.
//!
//! This crate defines the traits and types that all storage backends must implement.
//! It does not contain any implementations - those are provided by separate crates.
//!
//! ## Overview
//!
//! The main trait is [`FhirStorage`], which defines the contract for:
//! - CRUD operations (create, read, update, delete)
//! - Versioning (vread, history)
//! - Search
//! - Transactions
//!
//! ## Example
//!
//! ```ignore
//! use octofhir_storage::{FhirStorage, StorageError, SearchParams};
//!
//! async fn search_patients(
//!     storage: &dyn FhirStorage,
//!     name: &str,
//! ) -> Result<Vec<StoredResource>, StorageError> {
//!     let params = SearchParams::new()
//!         .with_param("name", name)
//!         .with_count(10);
//!
//!     let result = storage.search("Patient", &params).await?;
//!     Ok(result.entries)
//! }
//! ```
//!
//! ## Storage Backends
//!
//! To implement a storage backend, implement the [`FhirStorage`] trait:
//!
//! ```ignore
//! use async_trait::async_trait;
//! use octofhir_storage::{FhirStorage, StorageError, StoredResource};
//!
//! struct MyStorage {
//!     // ...
//! }
//!
//! #[async_trait]
//! impl FhirStorage for MyStorage {
//!     async fn create(&self, resource: &Value) -> Result<StoredResource, StorageError> {
//!         // Implementation
//!     }
//!     // ... other methods
//! }
//! ```

mod error;
pub mod evented;
mod traits;
mod types;

// Re-export everything from submodules
pub use error::{ErrorCategory, StorageError};
pub use evented::{EventedStorage, EventedTransaction};
pub use traits::{
    ConformanceChangeEvent, ConformanceChangeOp, ConformanceStorage, FhirStorage,
    StorageCapabilities, Transaction,
};
pub use types::{
    HistoryEntry, HistoryMethod, HistoryParams, HistoryResult, RawSearchResult, RawStoredResource,
    SearchParams, SearchResult, SortParam, StoredResource, TotalMode,
};

/// Type alias for a storage result.
pub type StorageResult<T> = Result<T, StorageError>;

/// Type alias for a boxed storage trait object.
pub type DynStorage = std::sync::Arc<dyn FhirStorage>;

/// Prelude module for convenient imports.
///
/// ```ignore
/// use octofhir_storage::prelude::*;
/// ```
pub mod prelude {
    pub use crate::error::{ErrorCategory, StorageError};
    pub use crate::evented::{EventedStorage, EventedTransaction};
    pub use crate::traits::{
        ConformanceChangeEvent, ConformanceChangeOp, ConformanceStorage, FhirStorage,
        StorageCapabilities, Transaction,
    };
    pub use crate::types::{
        HistoryEntry, HistoryMethod, HistoryParams, HistoryResult, RawSearchResult,
        RawStoredResource, SearchParams, SearchResult, SortParam, StoredResource, TotalMode,
    };
    pub use crate::{DynStorage, StorageResult};
}
