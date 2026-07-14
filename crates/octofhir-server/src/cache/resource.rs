//! FHIR resource read caching for performance optimization.
//!
//! Caches raw FHIR resources in-process to avoid database round-trips for reads.
//! Backed by a bounded `moka` cache (size-capped + TTL) keyed by
//! `res:{resource_type}:{id}`. Entries are stored as `Arc<RawStoredResource>`,
//! so a hit is a refcount bump with no deserialization.
//!
//! Invalidated on create, update, and delete via [`ResourceCache::invalidate`].

use std::sync::Arc;
use std::time::Duration;

use moka::future::Cache;

use octofhir_storage::RawStoredResource;

/// FHIR resource read cache.
pub struct ResourceCache {
    cache: Cache<String, Arc<RawStoredResource>>,
}

impl ResourceCache {
    /// Create a new resource cache bounded to `max_capacity` entries with the
    /// given per-entry TTL.
    pub fn new(max_capacity: u64, ttl: Duration) -> Self {
        Self {
            cache: Cache::builder()
                .max_capacity(max_capacity)
                .time_to_live(ttl)
                .build(),
        }
    }

    #[inline]
    fn cache_key(resource_type: &str, id: &str) -> String {
        format!("res:{resource_type}:{id}")
    }

    /// Get a cached resource by type and ID.
    pub async fn get(&self, resource_type: &str, id: &str) -> Option<Arc<RawStoredResource>> {
        let key = Self::cache_key(resource_type, id);
        self.cache.get(&key).await
    }

    /// Cache a resource after a successful read.
    pub async fn set(&self, stored: &RawStoredResource) {
        let key = Self::cache_key(&stored.resource_type, &stored.id);
        self.cache.insert(key, Arc::new(stored.clone())).await;
    }

    /// Invalidate a cached resource (on create/update/delete).
    pub async fn invalidate(&self, resource_type: &str, id: &str) {
        let key = Self::cache_key(resource_type, id);
        self.cache.invalidate(&key).await;
    }

    /// Current number of cached entries.
    pub fn entry_count(&self) -> u64 {
        self.cache.entry_count()
    }
}
