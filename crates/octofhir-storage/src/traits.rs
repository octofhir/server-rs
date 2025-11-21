//! Storage traits for the FHIR storage abstraction layer.
//!
//! This module defines the core traits that all storage backends must implement.

use async_trait::async_trait;
use serde_json::Value;

use crate::error::StorageError;
use crate::types::{HistoryParams, HistoryResult, SearchParams, SearchResult, StoredResource};

/// The main storage trait that all FHIR storage backends must implement.
///
/// This trait defines the contract for CRUD operations, versioning, search,
/// and transaction support. Implementations must be thread-safe (`Send + Sync`).
///
/// # Example
///
/// ```ignore
/// use octofhir_storage::{FhirStorage, StorageError, StoredResource};
///
/// async fn get_patient(storage: &dyn FhirStorage, id: &str) -> Result<StoredResource, StorageError> {
///     storage
///         .read("Patient", id)
///         .await?
///         .ok_or_else(|| StorageError::not_found("Patient", id))
/// }
/// ```
#[async_trait]
pub trait FhirStorage: Send + Sync {
    // ==================== CRUD Operations ====================

    /// Creates a new resource in the storage.
    ///
    /// The resource must contain a `resourceType` field and may contain an `id` field.
    /// If no `id` is provided, the storage backend should generate one.
    ///
    /// # Errors
    ///
    /// Returns `StorageError::AlreadyExists` if a resource with the same type and ID exists.
    /// Returns `StorageError::InvalidResource` if the resource is malformed.
    async fn create(&self, resource: &Value) -> Result<StoredResource, StorageError>;

    /// Reads a resource by type and ID.
    ///
    /// Returns `None` if the resource does not exist.
    ///
    /// # Errors
    ///
    /// Returns an error only for infrastructure issues, not for missing resources.
    async fn read(
        &self,
        resource_type: &str,
        id: &str,
    ) -> Result<Option<StoredResource>, StorageError>;

    /// Updates an existing resource.
    ///
    /// The resource must contain `resourceType` and `id` fields.
    /// If `if_match` is provided, the update will only succeed if the current
    /// version matches the provided ETag (version ID).
    ///
    /// # Errors
    ///
    /// Returns `StorageError::NotFound` if the resource does not exist.
    /// Returns `StorageError::VersionConflict` if `if_match` is provided and doesn't match.
    /// Returns `StorageError::InvalidResource` if the resource is malformed.
    async fn update(
        &self,
        resource: &Value,
        if_match: Option<&str>,
    ) -> Result<StoredResource, StorageError>;

    /// Deletes a resource by type and ID.
    ///
    /// The deletion should be soft by default (keeping history) if the backend supports it.
    ///
    /// # Errors
    ///
    /// Returns `StorageError::NotFound` if the resource does not exist.
    async fn delete(&self, resource_type: &str, id: &str) -> Result<(), StorageError>;

    // ==================== Versioning ====================

    /// Reads a specific version of a resource.
    ///
    /// Returns `None` if the resource or version does not exist.
    ///
    /// # Errors
    ///
    /// Returns an error only for infrastructure issues.
    async fn vread(
        &self,
        resource_type: &str,
        id: &str,
        version: &str,
    ) -> Result<Option<StoredResource>, StorageError>;

    /// Returns the history of a resource or resource type.
    ///
    /// If `id` is `Some`, returns history for a specific resource.
    /// If `id` is `None`, returns history for all resources of the given type.
    ///
    /// # Errors
    ///
    /// Returns an error for infrastructure issues or invalid parameters.
    async fn history(
        &self,
        resource_type: &str,
        id: Option<&str>,
        params: &HistoryParams,
    ) -> Result<HistoryResult, StorageError>;

    // ==================== Search ====================

    /// Searches for resources of a given type.
    ///
    /// # Errors
    ///
    /// Returns `StorageError::InvalidResource` for unsupported search parameters.
    /// Returns an error for infrastructure issues.
    async fn search(
        &self,
        resource_type: &str,
        params: &SearchParams,
    ) -> Result<SearchResult, StorageError>;

    // ==================== Transactions ====================

    /// Begins a new transaction.
    ///
    /// Returns a `Transaction` object that can be used to perform operations
    /// atomically. The transaction must be either committed or rolled back.
    ///
    /// # Errors
    ///
    /// Returns `StorageError::TransactionError` if transactions are not supported
    /// or if a transaction cannot be started.
    async fn begin_transaction(&self) -> Result<Box<dyn Transaction>, StorageError>;

    // ==================== Metadata ====================

    /// Returns whether this storage backend supports transactions.
    fn supports_transactions(&self) -> bool;

    /// Returns the name of this storage backend for logging/debugging.
    fn backend_name(&self) -> &'static str;
}

/// A transaction for performing atomic operations.
///
/// Operations within a transaction are isolated from other operations until
/// the transaction is committed. If an error occurs or `rollback` is called,
/// all operations are undone.
///
/// # Example
///
/// ```ignore
/// use octofhir_storage::FhirStorage;
///
/// async fn atomic_update(storage: &dyn FhirStorage) -> Result<(), StorageError> {
///     let mut tx = storage.begin_transaction().await?;
///
///     tx.create(&patient_json).await?;
///     tx.create(&observation_json).await?;
///
///     tx.commit().await?;
///     Ok(())
/// }
/// ```
#[async_trait]
pub trait Transaction: Send + Sync {
    /// Commits all operations in this transaction.
    ///
    /// After commit, the transaction is consumed and cannot be used again.
    ///
    /// # Errors
    ///
    /// Returns `StorageError::TransactionError` if the commit fails.
    async fn commit(self: Box<Self>) -> Result<(), StorageError>;

    /// Rolls back all operations in this transaction.
    ///
    /// After rollback, the transaction is consumed and cannot be used again.
    ///
    /// # Errors
    ///
    /// Returns `StorageError::TransactionError` if the rollback fails.
    async fn rollback(self: Box<Self>) -> Result<(), StorageError>;

    /// Creates a new resource within this transaction.
    ///
    /// See `FhirStorage::create` for details.
    async fn create(&mut self, resource: &Value) -> Result<StoredResource, StorageError>;

    /// Updates an existing resource within this transaction.
    ///
    /// See `FhirStorage::update` for details.
    async fn update(&mut self, resource: &Value) -> Result<StoredResource, StorageError>;

    /// Deletes a resource within this transaction.
    ///
    /// See `FhirStorage::delete` for details.
    async fn delete(&mut self, resource_type: &str, id: &str) -> Result<(), StorageError>;

    /// Reads a resource within this transaction.
    ///
    /// This read sees uncommitted changes made within this transaction.
    async fn read(
        &self,
        resource_type: &str,
        id: &str,
    ) -> Result<Option<StoredResource>, StorageError>;
}

/// Extension trait for storage with capability queries.
///
/// This trait provides additional methods for querying storage capabilities.
pub trait StorageCapabilities {
    /// Returns whether this storage supports version reads (`vread`).
    fn supports_vread(&self) -> bool {
        true
    }

    /// Returns whether this storage supports history queries.
    fn supports_history(&self) -> bool {
        true
    }

    /// Returns the supported search parameters for a resource type.
    ///
    /// Returns `None` if the resource type is not supported or if
    /// all standard FHIR search parameters are supported.
    fn supported_search_params(&self, _resource_type: &str) -> Option<Vec<String>> {
        None
    }
}

// Ensure traits are object-safe by using them as trait objects
#[cfg(test)]
mod tests {
    use super::*;

    // Compile-time test that FhirStorage is object-safe
    fn _assert_storage_object_safe(_: &dyn FhirStorage) {}

    // Compile-time test that Transaction is object-safe
    fn _assert_transaction_object_safe(_: &dyn Transaction) {}

    // Compile-time test that StorageCapabilities is object-safe
    fn _assert_capabilities_object_safe(_: &dyn StorageCapabilities) {}
}
