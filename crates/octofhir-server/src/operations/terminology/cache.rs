//! Terminology Resource Cache
//!
//! LRU cache for CodeSystem and ConceptMap resources loaded from the
//! canonical manager. Uses moka for TTL-based caching with size limits.
//!
//! This reduces repeated database/canonical manager queries for frequently
//! accessed terminology resources during $lookup, $subsumes, and $translate
//! operations.

use moka::future::Cache;
use serde_json::Value;
use std::sync::{Arc, LazyLock};
use std::time::Duration;

/// Default cache capacity (number of resources)
const DEFAULT_CACHE_CAPACITY: u64 = 200;

/// Default TTL for cached resources (1 hour)
const DEFAULT_TTL_SECS: u64 = 3600;

/// Cache key for CodeSystem lookups
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct CodeSystemKey {
    /// CodeSystem URL
    pub url: String,
    /// Optional version
    pub version: Option<String>,
}

impl CodeSystemKey {
    pub fn new(url: impl Into<String>, version: Option<String>) -> Self {
        Self {
            url: url.into(),
            version,
        }
    }
}

/// Cache key for ConceptMap lookups
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct ConceptMapKey {
    /// ConceptMap URL
    pub url: String,
    /// Optional version
    pub version: Option<String>,
}

impl ConceptMapKey {
    pub fn new(url: impl Into<String>, version: Option<String>) -> Self {
        Self {
            url: url.into(),
            version,
        }
    }
}

/// Global terminology resource cache
pub struct TerminologyResourceCache {
    /// Cache for CodeSystem resources by URL+version
    code_systems: Cache<CodeSystemKey, Option<Arc<Value>>>,
    /// Cache for ConceptMap resources by URL+version
    concept_maps: Cache<ConceptMapKey, Option<Arc<Value>>>,
}

impl TerminologyResourceCache {
    /// Create a new terminology resource cache with default settings.
    pub fn new() -> Self {
        Self::with_config(DEFAULT_CACHE_CAPACITY, DEFAULT_TTL_SECS)
    }

    /// Create a new cache with custom capacity and TTL.
    pub fn with_config(capacity: u64, ttl_secs: u64) -> Self {
        let ttl = Duration::from_secs(ttl_secs);

        Self {
            code_systems: Cache::builder()
                .max_capacity(capacity)
                .time_to_live(ttl)
                .build(),
            concept_maps: Cache::builder()
                .max_capacity(capacity / 2)
                .time_to_live(ttl)
                .build(),
        }
    }

    /// Get a cached CodeSystem by URL and optional version.
    pub async fn get_code_system(&self, key: &CodeSystemKey) -> Option<Arc<Value>> {
        self.code_systems.get(key).await.flatten()
    }

    /// Cache a CodeSystem resource.
    pub async fn insert_code_system(&self, key: CodeSystemKey, value: Option<Value>) {
        self.code_systems.insert(key, value.map(Arc::new)).await;
    }

    /// Get a cached ConceptMap by URL and optional version.
    pub async fn get_concept_map(&self, key: &ConceptMapKey) -> Option<Arc<Value>> {
        self.concept_maps.get(key).await.flatten()
    }

    /// Cache a ConceptMap resource.
    pub async fn insert_concept_map(&self, key: ConceptMapKey, value: Option<Value>) {
        self.concept_maps.insert(key, value.map(Arc::new)).await;
    }

    /// Clear all caches.
    pub fn clear(&self) {
        self.code_systems.invalidate_all();
        self.concept_maps.invalidate_all();
    }

    /// Get cache statistics.
    pub fn stats(&self) -> CacheStats {
        CacheStats {
            code_system_entries: self.code_systems.entry_count() as usize,
            concept_map_entries: self.concept_maps.entry_count() as usize,
        }
    }
}

impl Default for TerminologyResourceCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Cache statistics for monitoring.
#[derive(Debug, Clone)]
pub struct CacheStats {
    pub code_system_entries: usize,
    pub concept_map_entries: usize,
}

/// Global cache instance for terminology resources.
///
/// This is a singleton cache shared across all terminology operations.
/// Using a global cache ensures we don't load the same CodeSystem/ConceptMap
/// multiple times across different operation invocations.
pub static TERMINOLOGY_CACHE: LazyLock<TerminologyResourceCache> =
    LazyLock::new(TerminologyResourceCache::new);

/// Get a reference to the global terminology cache.
pub fn get_cache() -> &'static TerminologyResourceCache {
    &TERMINOLOGY_CACHE
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn test_code_system_cache() {
        let cache = TerminologyResourceCache::new();

        let key = CodeSystemKey::new("http://example.org/cs", None);
        let value = json!({"resourceType": "CodeSystem", "url": "http://example.org/cs"});

        // Initially empty
        assert!(cache.get_code_system(&key).await.is_none());

        // Insert and retrieve
        cache
            .insert_code_system(key.clone(), Some(value.clone()))
            .await;
        let cached = cache.get_code_system(&key).await;
        assert!(cached.is_some());
        assert_eq!(cached.unwrap().as_ref(), &value);

        // Test that we can retrieve multiple times (cache hit)
        let cached2 = cache.get_code_system(&key).await;
        assert!(cached2.is_some());
        assert_eq!(cached2.unwrap().as_ref(), &value);
    }

    #[tokio::test]
    async fn test_concept_map_cache() {
        let cache = TerminologyResourceCache::new();

        let key = ConceptMapKey::new("http://example.org/cm", Some("1.0".to_string()));
        let value = json!({"resourceType": "ConceptMap", "url": "http://example.org/cm"});

        cache
            .insert_concept_map(key.clone(), Some(value.clone()))
            .await;
        let cached = cache.get_concept_map(&key).await;
        assert!(cached.is_some());
        assert_eq!(cached.unwrap().as_ref(), &value);
    }

    #[tokio::test]
    async fn test_cache_clear() {
        let cache = TerminologyResourceCache::new();

        let cs_key = CodeSystemKey::new("http://example.org/cs-clear", None);
        let cm_key = ConceptMapKey::new("http://example.org/cm-clear", None);

        cache
            .insert_code_system(cs_key.clone(), Some(json!({"test": "cs"})))
            .await;
        cache
            .insert_concept_map(cm_key.clone(), Some(json!({"test": "cm"})))
            .await;

        // Verify entries were added
        assert!(cache.get_code_system(&cs_key).await.is_some());
        assert!(cache.get_concept_map(&cm_key).await.is_some());

        cache.clear();

        // After clear, entries should not be retrievable
        // Note: moka's invalidate_all marks entries for removal,
        // and they won't be returned by get() after invalidation
        assert!(cache.get_code_system(&cs_key).await.is_none());
        assert!(cache.get_concept_map(&cm_key).await.is_none());
    }
}
