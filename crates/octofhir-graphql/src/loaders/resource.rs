//! Resource DataLoader for batched resource loading.
//!
//! This loader batches requests for FHIR resources by (type, id) pairs,
//! reducing the number of database queries when resolving references.

use std::collections::HashMap;
use std::hash::Hash;
use std::sync::Arc;

use async_graphql::dataloader::Loader;
use octofhir_storage::DynStorage;
use tracing::{debug, instrument, trace};

use crate::error::GraphQLError;

/// Key for looking up a resource by type and ID.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ResourceKey {
    /// The FHIR resource type (e.g., "Patient", "Observation").
    pub resource_type: String,
    /// The resource ID.
    pub id: String,
}

impl ResourceKey {
    /// Creates a new resource key.
    #[must_use]
    pub fn new(resource_type: impl Into<String>, id: impl Into<String>) -> Self {
        Self {
            resource_type: resource_type.into(),
            id: id.into(),
        }
    }

    /// Creates a resource key from a reference string (e.g., "Patient/123").
    ///
    /// Returns `None` if the reference format is invalid.
    #[must_use]
    pub fn from_reference(reference: &str) -> Option<Self> {
        // Handle relative references like "Patient/123"
        let parts: Vec<&str> = reference.split('/').collect();
        if parts.len() >= 2 {
            // Take the last two parts for "Type/id" or longer URLs
            let type_index = parts.len() - 2;
            let id_index = parts.len() - 1;

            let resource_type = parts[type_index];
            let id = parts[id_index];

            // Validate resource type starts with uppercase (FHIR convention)
            if !resource_type.is_empty()
                && resource_type
                    .chars()
                    .next()
                    .map(|c| c.is_uppercase())
                    .unwrap_or(false)
                && !id.is_empty()
            {
                return Some(Self::new(resource_type, id));
            }
        }
        None
    }
}

impl std::fmt::Display for ResourceKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.resource_type, self.id)
    }
}

/// DataLoader for fetching FHIR resources by (type, id) pairs.
///
/// This loader batches multiple resource requests and groups them by
/// resource type for efficient database queries.
pub struct ResourceLoader {
    storage: DynStorage,
}

impl ResourceLoader {
    /// Creates a new resource loader.
    #[must_use]
    pub fn new(storage: DynStorage) -> Self {
        Self { storage }
    }
}

impl Loader<ResourceKey> for ResourceLoader {
    type Value = serde_json::Value;
    type Error = Arc<GraphQLError>;

    #[instrument(skip(self, keys), fields(key_count = keys.len()))]
    async fn load(
        &self,
        keys: &[ResourceKey],
    ) -> Result<HashMap<ResourceKey, Self::Value>, Self::Error> {
        debug!(key_count = keys.len(), "Loading resources batch");

        // Group keys by resource type for efficient batching
        let mut by_type: HashMap<&str, Vec<&ResourceKey>> = HashMap::new();
        for key in keys {
            by_type.entry(&key.resource_type).or_default().push(key);
        }

        let mut results: HashMap<ResourceKey, Self::Value> = HashMap::with_capacity(keys.len());

        // Fetch each resource type group
        // TODO: Consider using search with _id parameter for true batch loading
        // For now, we fetch individually but in parallel per type
        for (resource_type, type_keys) in by_type {
            trace!(
                resource_type = %resource_type,
                count = type_keys.len(),
                "Fetching resource type batch"
            );

            // Fetch resources for this type
            for key in type_keys {
                match self.storage.read(&key.resource_type, &key.id).await {
                    Ok(Some(stored)) => {
                        results.insert(key.clone(), stored.resource);
                    }
                    Ok(None) => {
                        // Resource not found - don't include in results
                        // DataLoader will return None for missing keys
                        trace!(key = %key, "Resource not found");
                    }
                    Err(e) => {
                        // Log error but continue with other resources
                        tracing::warn!(key = %key, error = %e, "Failed to load resource");
                    }
                }
            }
        }

        debug!(
            requested = keys.len(),
            found = results.len(),
            "Resource batch load complete"
        );

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resource_key_new() {
        let key = ResourceKey::new("Patient", "123");
        assert_eq!(key.resource_type, "Patient");
        assert_eq!(key.id, "123");
    }

    #[test]
    fn test_resource_key_from_reference_relative() {
        let key = ResourceKey::from_reference("Patient/123");
        assert!(key.is_some());
        let key = key.unwrap();
        assert_eq!(key.resource_type, "Patient");
        assert_eq!(key.id, "123");
    }

    #[test]
    fn test_resource_key_from_reference_absolute() {
        let key = ResourceKey::from_reference("http://example.org/fhir/Patient/456");
        assert!(key.is_some());
        let key = key.unwrap();
        assert_eq!(key.resource_type, "Patient");
        assert_eq!(key.id, "456");
    }

    #[test]
    fn test_resource_key_from_reference_invalid() {
        assert!(ResourceKey::from_reference("").is_none());
        assert!(ResourceKey::from_reference("invalid").is_none());
        assert!(ResourceKey::from_reference("patient/123").is_none()); // lowercase
        assert!(ResourceKey::from_reference("/123").is_none());
    }

    #[test]
    fn test_resource_key_display() {
        let key = ResourceKey::new("Patient", "123");
        assert_eq!(format!("{}", key), "Patient/123");
    }

    #[test]
    fn test_resource_key_hash() {
        use std::collections::HashSet;

        let key1 = ResourceKey::new("Patient", "123");
        let key2 = ResourceKey::new("Patient", "123");
        let key3 = ResourceKey::new("Patient", "456");

        let mut set = HashSet::new();
        set.insert(key1.clone());
        set.insert(key2); // Should not add duplicate
        set.insert(key3);

        assert_eq!(set.len(), 2);
        assert!(set.contains(&key1));
    }
}
