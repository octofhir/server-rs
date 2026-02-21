//! FHIR resource read caching for performance optimization.
//!
//! Caches raw FHIR resource JSON to avoid database round-trips for reads.
//! Uses the existing `CacheBackend` (DashMap L1 + optional Redis L2).
//!
//! ## Cache Key Format
//!
//! `res:{resource_type}:{id}` — e.g. `res:Patient:123`
//!
//! ## Invalidation
//!
//! Invalidated on create, update, delete via `invalidate()`.
//! Cross-instance invalidation via Redis pub/sub (handled by CacheBackend).

use std::time::Duration;

use super::backend::CacheBackend;
use octofhir_storage::RawStoredResource;

/// Cached resource entry serialized as MessagePack for compact storage.
#[derive(serde::Serialize, serde::Deserialize)]
struct CachedResource {
    id: String,
    version_id: String,
    resource_type: String,
    resource_json: String,
    last_updated_ts: i64,
    created_at_ts: i64,
}

impl CachedResource {
    fn from_stored(stored: &RawStoredResource) -> Self {
        Self {
            id: stored.id.clone(),
            version_id: stored.version_id.clone(),
            resource_type: stored.resource_type.clone(),
            resource_json: stored.resource_json.clone(),
            last_updated_ts: stored.last_updated.unix_timestamp(),
            created_at_ts: stored.created_at.unix_timestamp(),
        }
    }

    fn into_stored(self) -> RawStoredResource {
        RawStoredResource {
            id: self.id,
            version_id: self.version_id,
            resource_type: self.resource_type,
            resource_json: self.resource_json,
            last_updated: time::OffsetDateTime::from_unix_timestamp(self.last_updated_ts)
                .unwrap_or(time::OffsetDateTime::UNIX_EPOCH),
            created_at: time::OffsetDateTime::from_unix_timestamp(self.created_at_ts)
                .unwrap_or(time::OffsetDateTime::UNIX_EPOCH),
        }
    }
}

/// FHIR resource read cache.
pub struct ResourceCache {
    backend: CacheBackend,
    ttl: Duration,
}

impl ResourceCache {
    /// Create a new resource cache with the given backend and TTL.
    pub fn new(backend: CacheBackend, ttl: Duration) -> Self {
        Self { backend, ttl }
    }

    /// Generate cache key for a resource.
    #[inline]
    fn cache_key(resource_type: &str, id: &str) -> String {
        format!("res:{resource_type}:{id}")
    }

    /// Get a cached resource by type and ID.
    pub async fn get(&self, resource_type: &str, id: &str) -> Option<RawStoredResource> {
        let key = Self::cache_key(resource_type, id);
        let data = self.backend.get(&key).await?;
        match rmp_serde::from_slice::<CachedResource>(&data) {
            Ok(cached) => Some(cached.into_stored()),
            Err(e) => {
                tracing::warn!(key = %key, error = %e, "Failed to deserialize cached resource");
                self.backend.invalidate(&key).await;
                None
            }
        }
    }

    /// Cache a resource after a successful read.
    pub async fn set(&self, stored: &RawStoredResource) {
        let key = Self::cache_key(&stored.resource_type, &stored.id);
        let cached = CachedResource::from_stored(stored);
        match rmp_serde::to_vec(&cached) {
            Ok(data) => {
                self.backend.set(&key, data, self.ttl).await;
            }
            Err(e) => {
                tracing::warn!(key = %key, error = %e, "Failed to serialize resource for cache");
            }
        }
    }

    /// Invalidate a cached resource (on create/update/delete).
    pub async fn invalidate(&self, resource_type: &str, id: &str) {
        let key = Self::cache_key(resource_type, id);
        self.backend.invalidate(&key).await;
    }

    /// Get cache statistics.
    pub fn stats(&self) -> super::backend::CacheStats {
        self.backend.stats()
    }
}
