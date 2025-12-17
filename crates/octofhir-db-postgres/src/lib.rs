//! PostgreSQL storage backend for OctoFHIR server.
//!
//! This crate provides a PostgreSQL implementation of the `FhirStorage` trait
//! from `octofhir-storage`, using sqlx for type-safe queries.
//!
//! # Example
//!
//! ```ignore
//! use octofhir_db_postgres::{PostgresStorage, PostgresConfig};
//! use octofhir_storage::FhirStorage;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let config = PostgresConfig::new("postgres://user:pass@localhost/octofhir")
//!     .with_pool_size(10)
//!     .with_run_migrations(true);
//!
//! let storage = PostgresStorage::new(config).await?;
//!
//! // Create a patient
//! let patient = serde_json::json!({
//!     "resourceType": "Patient",
//!     "name": [{"family": "Smith"}]
//! });
//! let created = storage.create(&patient).await?;
//! # Ok(())
//! # }
//! ```
//!
//! # Features
//!
//! - Full FHIR CRUD operations
//! - Resource versioning with history
//! - Search support (TODO)
//! - Transaction support (TODO)
//!
//! # Architecture
//!
//! The crate is organized into several modules:
//!
//! - [`config`]: Configuration types for the storage backend
//! - [`error`]: Error types specific to PostgreSQL operations
//! - [`pool`]: Connection pool management
//! - [`schema`]: Schema management (table creation, indexes)
//! - [`storage`]: Main `FhirStorage` implementation
//! - [`queries`]: SQL query implementations
//! - [`migrations`]: Database migration management

mod config;
mod error;
mod fcm_storage;
mod pool;
mod query_analyzer;
mod schema;
mod storage;
mod transaction;

/// Database migrations module.
pub mod migrations;

/// SQL query implementations.
pub mod queries;

// Re-export main types
pub use config::PostgresConfig;
pub use error::{PostgresError, Result};
pub use fcm_storage::PostgresPackageStore;
pub use query_analyzer::{
    AnalyzerConfig, AnalyzerError, AnalyzerStatsSnapshot, BufferStats, IndexSuggestion, IndexUsage,
    QueryAnalysis, QueryAnalyzer, SeqScanInfo, SlowQueryRecord, SuggestionImpact,
};
pub use schema::SchemaManager;
pub use storage::PostgresStorage;

// Re-export storage traits for convenience
pub use octofhir_storage::{ConformanceStorage, FhirStorage, StorageError, StoredResource};

/// Type alias for a shareable PostgresStorage instance.
pub type DynPostgresStorage = std::sync::Arc<PostgresStorage>;

/// Creates a new PostgreSQL storage instance with the given configuration.
///
/// This is a convenience function that creates a storage instance
/// wrapped in an `Arc` for sharing across threads.
///
/// # Errors
///
/// Returns an error if the connection pool cannot be created
/// or if migrations fail.
pub async fn create_storage(
    config: PostgresConfig,
) -> std::result::Result<DynPostgresStorage, StorageError> {
    let storage = PostgresStorage::new(config).await?;
    Ok(std::sync::Arc::new(storage))
}

/// Prelude module for convenient imports.
///
/// ```ignore
/// use octofhir_db_postgres::prelude::*;
/// ```
pub mod prelude {
    pub use crate::config::PostgresConfig;
    pub use crate::error::{PostgresError, Result};
    pub use crate::fcm_storage::PostgresPackageStore;
    pub use crate::query_analyzer::{
        AnalyzerConfig, IndexSuggestion, QueryAnalysis, QueryAnalyzer,
    };
    pub use crate::storage::PostgresStorage;
    pub use crate::{DynPostgresStorage, create_storage};
    pub use octofhir_storage::{ConformanceStorage, FhirStorage, StorageError, StoredResource};
}
