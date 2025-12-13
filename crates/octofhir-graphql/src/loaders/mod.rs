//! DataLoaders for efficient batched data loading.
//!
//! This module provides DataLoader implementations for FHIR resource loading,
//! preventing N+1 query problems when resolving references in GraphQL queries.
//!
//! ## Overview
//!
//! DataLoaders batch and cache load requests within a single GraphQL execution:
//!
//! - [`ResourceLoader`] - Loads resources by (type, id) pairs
//! - [`ReferenceLoader`] - Parses FHIR references and resolves them to resources
//!
//! ## Usage
//!
//! DataLoaders are created per-request and added to the GraphQL context:
//!
//! ```ignore
//! use octofhir_graphql::loaders::{ResourceLoader, ReferenceLoader, DataLoaders};
//!
//! let loaders = DataLoaders::new(storage.clone());
//! // Add to GraphQL request data
//! ```

mod reference;
mod resource;

pub use reference::{ParsedReference, ReferenceKey, ReferenceLoader, ResolvedReference};
pub use resource::{ResourceKey, ResourceLoader};

use std::sync::Arc;

use async_graphql::dataloader::DataLoader;
use octofhir_storage::DynStorage;

/// Collection of all DataLoaders for a GraphQL request.
///
/// This struct holds all loaders needed for reference resolution and
/// is created once per request to ensure proper batching scope.
///
/// DataLoaders are wrapped in Arc for cheap cloning and shared access
/// across resolver contexts.
#[derive(Clone)]
pub struct DataLoaders {
    /// Loader for fetching resources by (type, id).
    pub resource_loader: Arc<DataLoader<ResourceLoader>>,

    /// Loader for resolving FHIR reference strings.
    pub reference_loader: Arc<DataLoader<ReferenceLoader>>,
}

impl DataLoaders {
    /// Creates a new set of DataLoaders.
    ///
    /// The storage is shared across all loaders. Each loader maintains
    /// its own batching and caching within the request scope.
    #[must_use]
    pub fn new(storage: DynStorage) -> Self {
        let resource_loader = ResourceLoader::new(storage.clone());
        let reference_loader = ReferenceLoader::new(storage);

        Self {
            resource_loader: Arc::new(DataLoader::new(resource_loader, tokio::spawn)),
            reference_loader: Arc::new(DataLoader::new(reference_loader, tokio::spawn)),
        }
    }

    /// Creates DataLoaders with custom batch delay.
    ///
    /// The delay parameter controls how long to wait for additional
    /// requests before executing a batch. Shorter delays reduce latency
    /// but may result in smaller batches.
    #[must_use]
    pub fn with_delay(storage: DynStorage, delay: std::time::Duration) -> Self {
        let resource_loader = ResourceLoader::new(storage.clone());
        let reference_loader = ReferenceLoader::new(storage);

        Self {
            resource_loader: Arc::new(DataLoader::new(resource_loader, tokio::spawn).delay(delay)),
            reference_loader: Arc::new(DataLoader::new(reference_loader, tokio::spawn).delay(delay)),
        }
    }
}

impl std::fmt::Debug for DataLoaders {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DataLoaders")
            .field("resource_loader", &"DataLoader<ResourceLoader>")
            .field("reference_loader", &"DataLoader<ReferenceLoader>")
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use crate::loaders::reference::ParsedReference;

    #[test]
    fn test_parsed_reference_relative() {
        let parsed = ParsedReference::parse("Patient/123");
        assert!(parsed.is_some());
        let parsed = parsed.unwrap();
        assert_eq!(parsed.resource_type, "Patient");
        assert_eq!(parsed.id, "123");
        assert!(!parsed.is_absolute);
        assert!(!parsed.is_contained);
    }

    #[test]
    fn test_parsed_reference_absolute() {
        let parsed = ParsedReference::parse("http://example.org/fhir/Patient/456");
        assert!(parsed.is_some());
        let parsed = parsed.unwrap();
        assert_eq!(parsed.resource_type, "Patient");
        assert_eq!(parsed.id, "456");
        assert!(parsed.is_absolute);
    }

    #[test]
    fn test_parsed_reference_contained() {
        let parsed = ParsedReference::parse("#contained-id");
        assert!(parsed.is_some());
        let parsed = parsed.unwrap();
        assert_eq!(parsed.id, "contained-id");
        assert!(parsed.is_contained);
    }

    #[test]
    fn test_parsed_reference_invalid() {
        assert!(ParsedReference::parse("").is_none());
        assert!(ParsedReference::parse("invalid").is_none());
    }
}
