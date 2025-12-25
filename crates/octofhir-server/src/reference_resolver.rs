//! Storage-backed reference resolver for FHIR reference validation.
//!
//! This module provides a `ReferenceResolver` implementation that checks
//! whether referenced resources exist in the FHIR storage.

use async_trait::async_trait;
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
    fn parse_reference(&self, reference: &str) -> Option<(String, String)> {
        // Skip contained references
        if reference.starts_with('#') {
            return None;
        }

        // Skip urn:uuid: and urn:oid: references
        if reference.starts_with("urn:") {
            return None;
        }

        // Handle absolute URLs
        let path = if reference.starts_with(&self.base_url) {
            // Same server - strip base URL
            reference[self.base_url.len()..].trim_start_matches('/')
        } else if reference.contains("://") {
            // Different server - cannot validate
            return None;
        } else {
            // Relative reference
            reference
        };

        // Parse "ResourceType/id" or "ResourceType/id/_history/version"
        let parts: Vec<&str> = path.split('/').collect();

        if parts.len() >= 2 {
            let (type_idx, id_idx) = if parts.len() >= 4 && parts.get(2) == Some(&"_history") {
                (0, 1) // ResourceType/id/_history/version
            } else {
                (0, 1) // ResourceType/id
            };

            let resource_type = parts.get(type_idx)?;
            let id = parts.get(id_idx)?;

            // Validate resource type looks valid (starts with capital letter)
            if resource_type
                .chars()
                .next()
                .map(|c| c.is_ascii_uppercase())
                .unwrap_or(false)
                && !id.is_empty()
            {
                return Some((resource_type.to_string(), id.to_string()));
            }
        }

        None
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
        let (resource_type, id) = match self.parse_reference(reference) {
            Some((rt, id)) => (rt, id),
            None => {
                // Cannot parse or external reference - skip validation
                return Ok(ReferenceResolutionResult::skipped());
            }
        };

        let exists = self.resource_exists(&resource_type, &id).await?;

        if exists {
            Ok(ReferenceResolutionResult::found(resource_type, id))
        } else {
            Ok(ReferenceResolutionResult::not_found())
        }
    }
}

// Tests commented out - InMemoryStorage was removed
// TODO: Re-enable tests with proper mock storage
// #[cfg(test)]
// mod tests { ... }
