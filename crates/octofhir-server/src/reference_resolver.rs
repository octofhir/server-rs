//! Storage-backed reference resolver for FHIR reference validation.
//!
//! This module provides a `ReferenceResolver` implementation that checks
//! whether referenced resources exist in the FHIR storage.

use async_trait::async_trait;
use octofhir_core::fhir_reference::{FhirReference, parse_reference};
use octofhir_fhirschema::reference::{
    ReferenceError, ReferenceResolutionResult, ReferenceResolver, ReferenceResult,
};
use octofhir_storage::DynStorage;

/// Reference resolver backed by FHIR storage.
///
/// Uses the storage layer to check if referenced resources exist.
pub struct StorageReferenceResolver {
    storage: DynStorage,
    /// Base URL for resolving absolute references
    base_url: String,
}

impl StorageReferenceResolver {
    /// Create a new storage-backed reference resolver.
    ///
    /// # Arguments
    /// * `storage` - The FHIR storage backend
    /// * `base_url` - Base URL for the FHIR server (e.g., "http://localhost:8888/fhir")
    pub fn new(storage: DynStorage, base_url: String) -> Self {
        Self { storage, base_url }
    }

    /// Parse a reference string into (resource_type, id).
    ///
    /// Returns None for references that cannot be resolved locally:
    /// - Contained references (#id)
    /// - urn:uuid: or urn:oid: references
    /// - External server references
    fn parse_reference(&self, reference: &str) -> Option<FhirReference> {
        parse_reference(reference, Some(&self.base_url)).ok()
    }
}

#[async_trait]
impl ReferenceResolver for StorageReferenceResolver {
    async fn resource_exists(&self, resource_type: &str, id: &str) -> ReferenceResult<bool> {
        match self.storage.read(resource_type, id).await {
            Ok(Some(_)) => Ok(true),
            Ok(None) => Ok(false),
            Err(e) => Err(ReferenceError::ServiceUnavailable {
                message: e.to_string(),
            }),
        }
    }

    async fn resolve_reference(
        &self,
        reference: &str,
    ) -> ReferenceResult<ReferenceResolutionResult> {
        let parsed = match self.parse_reference(reference) {
            Some(r) => r,
            None => {
                // Cannot parse or external reference - skip validation
                return Ok(ReferenceResolutionResult::skipped());
            }
        };

        let exists = self
            .resource_exists(&parsed.resource_type, &parsed.id)
            .await?;

        if exists {
            Ok(ReferenceResolutionResult::found(
                parsed.resource_type,
                parsed.id,
            ))
        } else {
            Ok(ReferenceResolutionResult::not_found())
        }
    }
}

// Tests commented out - InMemoryStorage was removed
// TODO: Re-enable tests with proper mock storage
// #[cfg(test)]
// mod tests { ... }
