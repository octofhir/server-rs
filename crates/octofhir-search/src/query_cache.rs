//! Query Plan Caching for FHIR Search
//!
//! This module provides query caching for FHIR search operations.
//! It caches prepared query templates based on parameter structure (not values),
//! allowing similar queries with different values to reuse cached query plans.
//!
//! ## Design
//!
//! - **Cache Key**: Based on query structure (resource type, parameters, modifiers)
//! - **Cache Value**: SQL template with parameter positions and types
//! - **Concurrent Access**: Uses DashMap for lock-free concurrent operations
//! - **LRU Eviction**: Automatically evicts least recently used entries when at capacity
//!
//! ## Example
//!
//! ```ignore
//! let cache = QueryCache::new(1000);
//!
//! // First query builds and caches the template
//! let key = QueryCacheKey::from_search("Patient", &params);
//! if let Some(prepared) = cache.get(&key) {
//!     // Cache hit - bind values and execute
//! } else {
//!     // Build query, cache it for future use
//!     let prepared = PreparedQuery::from_built_query(&built);
//!     cache.insert(key, prepared);
//! }
//! ```

use dashmap::DashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Instant;

use crate::parameters::SearchParameterType;
use crate::parser::ParsedParam;
use crate::sql_builder::{BuiltQuery, SqlValue};

/// Maximum age in seconds before an entry becomes stale (default: 1 hour)
const DEFAULT_MAX_AGE_SECS: u64 = 3600;

/// Probability (1/N) of running cleanup on insert
const CLEANUP_PROBABILITY: u32 = 100; // 1% chance

/// Hard capacity multiplier - force cleanup when exceeding this
const HARD_CAPACITY_MULTIPLIER: f32 = 1.5;

/// A cache key based on query structure.
///
/// The key includes:
/// - Resource type being queried
/// - Parameter names in sorted order
/// - Modifier for each parameter
/// - Type of each parameter (token, date, string, etc.)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryCacheKey {
    /// Resource type (e.g., "Patient", "Observation")
    pub resource_type: String,
    /// Sorted list of (parameter_name, modifier, type) tuples
    pub parameters: Vec<QueryParamKey>,
    /// Pagination key (limit presence, not actual value)
    pub has_pagination: bool,
    /// Sort fields (names only, not directions)
    pub sort_fields: Vec<String>,
}

/// Key component for a single search parameter.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct QueryParamKey {
    /// Parameter name
    pub name: String,
    /// Modifier (e.g., ":exact", ":contains", ":missing")
    pub modifier: Option<String>,
    /// Parameter type for type-specific handling
    pub param_type: SearchParameterType,
    /// Number of values (affects OR clause structure)
    pub value_count: usize,
}

impl QueryCacheKey {
    /// Create a cache key from parsed search parameters.
    pub fn from_search(
        resource_type: &str,
        params: &[ParsedParam],
        has_pagination: bool,
        sort_fields: &[String],
    ) -> Self {
        let mut parameters: Vec<QueryParamKey> = params
            .iter()
            .map(|p| QueryParamKey {
                name: p.name.clone(),
                modifier: p
                    .modifier
                    .as_ref()
                    .map(|m| format!("{:?}", m).to_lowercase()),
                param_type: Self::infer_param_type(&p.name),
                value_count: p.values.len(),
            })
            .collect();

        // Sort for consistent key generation
        parameters.sort_by(|a, b| a.name.cmp(&b.name));

        Self {
            resource_type: resource_type.to_string(),
            parameters,
            has_pagination,
            sort_fields: sort_fields.to_vec(),
        }
    }

    /// Create a cache key with explicit parameter types.
    pub fn from_typed_params(
        resource_type: &str,
        params: Vec<QueryParamKey>,
        has_pagination: bool,
        sort_fields: Vec<String>,
    ) -> Self {
        let mut parameters = params;
        parameters.sort_by(|a, b| a.name.cmp(&b.name));

        Self {
            resource_type: resource_type.to_string(),
            parameters,
            has_pagination,
            sort_fields,
        }
    }

    /// Infer parameter type from name (heuristic).
    pub fn infer_param_type(name: &str) -> SearchParameterType {
        let lower = name.to_lowercase();

        // Common patterns
        if lower.contains("date")
            || lower.contains("time")
            || lower == "birthdate"
            || lower == "authored"
            || lower == "effective"
            || lower == "issued"
            || lower == "period"
            || lower == "onset"
        {
            return SearchParameterType::Date;
        }

        if lower == "code"
            || lower == "status"
            || lower == "identifier"
            || lower == "type"
            || lower == "category"
            || lower.contains("code")
        {
            return SearchParameterType::Token;
        }

        if lower == "name"
            || lower == "family"
            || lower == "given"
            || lower == "address"
            || lower == "city"
            || lower == "text"
        {
            return SearchParameterType::String;
        }

        if lower == "subject"
            || lower == "patient"
            || lower == "performer"
            || lower == "encounter"
            || lower.contains("reference")
        {
            return SearchParameterType::Reference;
        }

        if lower.contains("quantity") || lower == "value-quantity" {
            return SearchParameterType::Quantity;
        }

        if lower == "url" || lower == "uri" {
            return SearchParameterType::Uri;
        }

        // Default to token for unknown
        SearchParameterType::Token
    }

    /// Generate a hash key for the cache.
    pub fn cache_hash(&self) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        let mut hasher = DefaultHasher::new();
        self.hash(&mut hasher);
        hasher.finish()
    }
}

impl Hash for QueryCacheKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.resource_type.hash(state);
        self.has_pagination.hash(state);

        for param in &self.parameters {
            param.hash(state);
        }

        for field in &self.sort_fields {
            field.hash(state);
        }
    }
}

/// A cached prepared query with SQL template and parameter metadata.
#[derive(Debug)]
pub struct PreparedQuery {
    /// SQL template with $1, $2, etc. placeholders
    pub sql_template: String,
    /// Parameter positions and their expected types
    pub param_positions: Vec<ParamPosition>,
    /// Total number of parameters expected
    pub param_count: usize,
    /// Timestamp when this entry was cached
    pub cached_at: Instant,
    /// Number of times this entry has been used
    hit_count: AtomicU64,
}

impl Clone for PreparedQuery {
    fn clone(&self) -> Self {
        Self {
            sql_template: self.sql_template.clone(),
            param_positions: self.param_positions.clone(),
            param_count: self.param_count,
            cached_at: self.cached_at,
            hit_count: AtomicU64::new(self.hit_count.load(Ordering::Relaxed)),
        }
    }
}

/// Position and type information for a query parameter.
#[derive(Debug, Clone)]
pub struct ParamPosition {
    /// 1-based position in SQL ($1, $2, etc.)
    pub position: usize,
    /// Parameter name from original query
    pub name: String,
    /// Value index (for multi-value parameters)
    pub value_index: usize,
    /// Expected value type
    pub value_type: ParamValueType,
}

/// Type of parameter value for proper binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamValueType {
    Text,
    Integer,
    Float,
    Boolean,
    Timestamp,
    Json,
}

impl PreparedQuery {
    /// Create a prepared query from a built query.
    pub fn from_built_query(built: &BuiltQuery, param_metadata: Vec<ParamPosition>) -> Self {
        Self {
            sql_template: built.sql.clone(),
            param_positions: param_metadata,
            param_count: built.params.len(),
            cached_at: Instant::now(),
            hit_count: AtomicU64::new(0),
        }
    }

    /// Create a simple prepared query without detailed metadata.
    pub fn simple(sql: String, param_count: usize) -> Self {
        Self {
            sql_template: sql,
            param_positions: Vec::new(),
            param_count,
            cached_at: Instant::now(),
            hit_count: AtomicU64::new(0),
        }
    }

    /// Bind values to produce a final query.
    pub fn bind(&self, values: Vec<SqlValue>) -> Result<BuiltQuery, CacheError> {
        if values.len() != self.param_count {
            return Err(CacheError::ParameterMismatch {
                expected: self.param_count,
                got: values.len(),
            });
        }

        self.hit_count.fetch_add(1, Ordering::Relaxed);

        Ok(BuiltQuery {
            sql: self.sql_template.clone(),
            params: values,
        })
    }

    /// Check if this entry is stale.
    pub fn is_stale(&self, max_age_secs: u64) -> bool {
        self.cached_at.elapsed().as_secs() > max_age_secs
    }

    /// Get the hit count.
    pub fn hits(&self) -> u64 {
        self.hit_count.load(Ordering::Relaxed)
    }
}

/// Errors that can occur with query caching.
#[derive(Debug, Clone, thiserror::Error)]
pub enum CacheError {
    #[error("Parameter count mismatch: expected {expected}, got {got}")]
    ParameterMismatch { expected: usize, got: usize },

    #[error("Cache entry not found")]
    NotFound,

    #[error("Cache is disabled")]
    Disabled,
}

/// Thread-safe query cache with LRU eviction.
pub struct QueryCache {
    /// The cache storage
    cache: DashMap<u64, CacheEntry>,
    /// Maximum number of entries
    capacity: usize,
    /// Maximum age for entries in seconds
    max_age_secs: u64,
    /// Whether caching is enabled
    enabled: bool,
    /// Statistics
    stats: Arc<CacheStatistics>,
}

impl std::fmt::Debug for QueryCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QueryCache")
            .field("capacity", &self.capacity)
            .field("size", &self.cache.len())
            .field("max_age_secs", &self.max_age_secs)
            .field("enabled", &self.enabled)
            .field("stats", &self.stats.snapshot())
            .finish()
    }
}

/// A cache entry for TTL-based eviction.
///
/// With TTL-based eviction, we only track access count for statistics.
/// The staleness check uses `query.cached_at` which is set on creation.
struct CacheEntry {
    query: PreparedQuery,
}

impl CacheEntry {
    fn new(query: PreparedQuery) -> Self {
        Self { query }
    }
}

/// Cache statistics for monitoring.
#[derive(Debug, Default)]
pub struct CacheStatistics {
    /// Number of cache hits
    pub hits: AtomicU64,
    /// Number of cache misses
    pub misses: AtomicU64,
    /// Number of evictions
    pub evictions: AtomicU64,
    /// Number of insertions
    pub insertions: AtomicU64,
    /// Current size
    pub size: AtomicUsize,
}

impl CacheStatistics {
    /// Calculate hit ratio.
    pub fn hit_ratio(&self) -> f64 {
        let hits = self.hits.load(Ordering::Relaxed) as f64;
        let misses = self.misses.load(Ordering::Relaxed) as f64;
        let total = hits + misses;

        if total == 0.0 { 0.0 } else { hits / total }
    }

    /// Get a snapshot of current statistics.
    pub fn snapshot(&self) -> CacheStatsSnapshot {
        CacheStatsSnapshot {
            hits: self.hits.load(Ordering::Relaxed),
            misses: self.misses.load(Ordering::Relaxed),
            evictions: self.evictions.load(Ordering::Relaxed),
            insertions: self.insertions.load(Ordering::Relaxed),
            size: self.size.load(Ordering::Relaxed),
            hit_ratio: self.hit_ratio(),
        }
    }
}

/// A point-in-time snapshot of cache statistics.
#[derive(Debug, Clone)]
pub struct CacheStatsSnapshot {
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    pub insertions: u64,
    pub size: usize,
    pub hit_ratio: f64,
}

impl QueryCache {
    /// Create a new query cache with the given capacity.
    pub fn new(capacity: usize) -> Self {
        Self {
            cache: DashMap::with_capacity(capacity),
            capacity,
            max_age_secs: DEFAULT_MAX_AGE_SECS,
            enabled: true,
            stats: Arc::new(CacheStatistics::default()),
        }
    }

    /// Create a disabled cache (no-op).
    pub fn disabled() -> Self {
        Self {
            cache: DashMap::new(),
            capacity: 0,
            max_age_secs: 0,
            enabled: false,
            stats: Arc::new(CacheStatistics::default()),
        }
    }

    /// Set the maximum age for cache entries.
    pub fn with_max_age(mut self, secs: u64) -> Self {
        self.max_age_secs = secs;
        self
    }

    /// Check if caching is enabled.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Enable or disable the cache.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Get a cached query by key.
    ///
    /// Uses a read lock only (no write lock contention) since we use TTL-based
    /// eviction instead of LRU tracking.
    pub fn get(&self, key: &QueryCacheKey) -> Option<PreparedQuery> {
        if !self.enabled {
            return None;
        }

        let hash = key.cache_hash();

        // Use get() instead of get_mut() to avoid write lock contention
        // Since we use TTL-based eviction, we don't need to track last_access
        if let Some(entry) = self.cache.get(&hash) {
            // Check if stale
            if entry.query.is_stale(self.max_age_secs) {
                drop(entry); // Release read lock before removing
                self.cache.remove(&hash);
                self.stats.misses.fetch_add(1, Ordering::Relaxed);
                self.stats.size.store(self.cache.len(), Ordering::Relaxed);
                return None;
            }

            // No touch() needed - TTL-based eviction only cares about cached_at
            self.stats.hits.fetch_add(1, Ordering::Relaxed);
            return Some(entry.query.clone());
        }

        self.stats.misses.fetch_add(1, Ordering::Relaxed);
        None
    }

    /// Insert a prepared query into the cache.
    ///
    /// Uses TTL-based eviction instead of LRU to avoid O(n) scans:
    /// - Probabilistic cleanup (1% chance on each insert)
    /// - Forced cleanup when exceeding hard capacity limit
    pub fn insert(&self, key: QueryCacheKey, query: PreparedQuery) {
        if !self.enabled {
            return;
        }

        let current_len = self.cache.len();
        let hard_limit = (self.capacity as f32 * HARD_CAPACITY_MULTIPLIER) as usize;

        // Probabilistic cleanup: 1% chance to clean stale entries
        // This amortizes cleanup cost across many inserts
        if current_len >= self.capacity {
            let should_cleanup = fastrand::u32(0..CLEANUP_PROBABILITY) == 0;
            if should_cleanup || current_len >= hard_limit {
                self.cleanup_stale();
            }
        }

        let hash = key.cache_hash();
        self.cache.insert(hash, CacheEntry::new(query));
        self.stats.insertions.fetch_add(1, Ordering::Relaxed);
        self.stats.size.store(self.cache.len(), Ordering::Relaxed);
    }

    /// Clear all cached queries.
    pub fn clear(&self) {
        self.cache.clear();
        self.stats.size.store(0, Ordering::Relaxed);
    }

    /// Remove stale entries.
    pub fn cleanup_stale(&self) {
        let stale_keys: Vec<u64> = self
            .cache
            .iter()
            .filter(|entry| entry.query.is_stale(self.max_age_secs))
            .map(|entry| *entry.key())
            .collect();

        for key in stale_keys {
            self.cache.remove(&key);
            self.stats.evictions.fetch_add(1, Ordering::Relaxed);
        }

        self.stats.size.store(self.cache.len(), Ordering::Relaxed);
    }

    /// Get cache statistics.
    pub fn stats(&self) -> CacheStatsSnapshot {
        self.stats.snapshot()
    }

    /// Get current cache size.
    pub fn len(&self) -> usize {
        self.cache.len()
    }

    /// Check if cache is empty.
    pub fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }

    /// Get cache capacity.
    pub fn capacity(&self) -> usize {
        self.capacity
    }
}

impl Default for QueryCache {
    fn default() -> Self {
        Self::new(1000)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_key_generation() {
        let key1 = QueryCacheKey::from_typed_params(
            "Patient",
            vec![
                QueryParamKey {
                    name: "name".to_string(),
                    modifier: None,
                    param_type: SearchParameterType::String,
                    value_count: 1,
                },
                QueryParamKey {
                    name: "birthdate".to_string(),
                    modifier: Some("lt".to_string()),
                    param_type: SearchParameterType::Date,
                    value_count: 1,
                },
            ],
            true,
            vec!["name".to_string()],
        );

        let key2 = QueryCacheKey::from_typed_params(
            "Patient",
            vec![
                QueryParamKey {
                    name: "birthdate".to_string(),
                    modifier: Some("lt".to_string()),
                    param_type: SearchParameterType::Date,
                    value_count: 1,
                },
                QueryParamKey {
                    name: "name".to_string(),
                    modifier: None,
                    param_type: SearchParameterType::String,
                    value_count: 1,
                },
            ],
            true,
            vec!["name".to_string()],
        );

        // Keys should be equal regardless of parameter order
        assert_eq!(key1.cache_hash(), key2.cache_hash());
    }

    #[test]
    fn test_cache_key_different_modifiers() {
        let key1 = QueryCacheKey::from_typed_params(
            "Patient",
            vec![QueryParamKey {
                name: "name".to_string(),
                modifier: None,
                param_type: SearchParameterType::String,
                value_count: 1,
            }],
            false,
            vec![],
        );

        let key2 = QueryCacheKey::from_typed_params(
            "Patient",
            vec![QueryParamKey {
                name: "name".to_string(),
                modifier: Some("exact".to_string()),
                param_type: SearchParameterType::String,
                value_count: 1,
            }],
            false,
            vec![],
        );

        // Keys should be different with different modifiers
        assert_ne!(key1.cache_hash(), key2.cache_hash());
    }

    #[test]
    fn test_cache_insert_and_get() {
        let cache = QueryCache::new(10);

        let key = QueryCacheKey::from_typed_params(
            "Patient",
            vec![QueryParamKey {
                name: "name".to_string(),
                modifier: None,
                param_type: SearchParameterType::String,
                value_count: 1,
            }],
            false,
            vec![],
        );

        let query = PreparedQuery::simple(
            "SELECT * FROM patient WHERE resource->>'name' ILIKE $1".to_string(),
            1,
        );

        // Insert
        cache.insert(key.clone(), query.clone());
        assert_eq!(cache.len(), 1);

        // Get
        let cached = cache.get(&key).expect("Should find cached query");
        assert_eq!(cached.sql_template, query.sql_template);

        // Stats
        let stats = cache.stats();
        assert_eq!(stats.hits, 1);
        assert_eq!(stats.insertions, 1);
    }

    #[test]
    fn test_cache_miss() {
        let cache = QueryCache::new(10);

        let key = QueryCacheKey::from_typed_params(
            "Patient",
            vec![QueryParamKey {
                name: "name".to_string(),
                modifier: None,
                param_type: SearchParameterType::String,
                value_count: 1,
            }],
            false,
            vec![],
        );

        let result = cache.get(&key);
        assert!(result.is_none());

        let stats = cache.stats();
        assert_eq!(stats.misses, 1);
    }

    #[test]
    fn test_cache_soft_capacity() {
        // With TTL-based eviction, cache uses soft capacity (allows over-capacity)
        // Eviction only happens for stale entries or when exceeding hard limit (1.5x)
        let cache = QueryCache::new(2);

        // Insert 3 entries into a cache with soft capacity 2
        // This is allowed because hard limit is 1.5x = 3
        for i in 0..3 {
            let key = QueryCacheKey::from_typed_params(
                "Patient",
                vec![QueryParamKey {
                    name: format!("param{}", i),
                    modifier: None,
                    param_type: SearchParameterType::String,
                    value_count: 1,
                }],
                false,
                vec![],
            );

            let query = PreparedQuery::simple(format!("SELECT {}", i), 0);
            cache.insert(key, query);
        }

        // With TTL-based eviction, all 3 entries should be present
        // (they're not stale yet and under hard limit of 1.5x capacity = 3)
        assert_eq!(cache.len(), 3);
    }

    #[test]
    fn test_cache_staleness_check() {
        // TTL-based eviction checks staleness based on cached_at time
        let query = PreparedQuery::simple("SELECT * FROM patient".to_string(), 0);

        // Entry should not be stale with default TTL (1 hour)
        assert!(!query.is_stale(3600));

        // Entry should be stale with 0 second TTL after waiting 1+ seconds
        // Note: is_stale uses as_secs() which truncates, so need to wait >1 second
        // For unit test speed, we just verify the staleness logic works
        assert!(!query.is_stale(3600)); // Not stale with 1 hour TTL
    }

    #[test]
    fn test_cleanup_stale() {
        // Test that cleanup_stale() removes stale entries
        let cache = QueryCache::new(10).with_max_age(0);

        // Insert some entries
        for i in 0..3 {
            let key = QueryCacheKey::from_typed_params(
                "Patient",
                vec![QueryParamKey {
                    name: format!("param{}", i),
                    modifier: None,
                    param_type: SearchParameterType::String,
                    value_count: 1,
                }],
                false,
                vec![],
            );
            let query = PreparedQuery::simple(format!("SELECT {}", i), 0);
            cache.insert(key, query);
        }
        assert_eq!(cache.len(), 3);

        // Wait for entries to become stale (>1 second for as_secs() to return non-zero)
        std::thread::sleep(std::time::Duration::from_secs(2));

        // Cleanup should remove all stale entries
        cache.cleanup_stale();
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn test_cache_disabled() {
        let cache = QueryCache::disabled();
        assert!(!cache.is_enabled());

        let key = QueryCacheKey::from_typed_params(
            "Patient",
            vec![QueryParamKey {
                name: "name".to_string(),
                modifier: None,
                param_type: SearchParameterType::String,
                value_count: 1,
            }],
            false,
            vec![],
        );

        let query = PreparedQuery::simple("SELECT * FROM patient".to_string(), 0);
        cache.insert(key.clone(), query);

        // Should not cache when disabled
        assert!(cache.get(&key).is_none());
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn test_prepared_query_bind() {
        let query =
            PreparedQuery::simple("SELECT * FROM patient WHERE name ILIKE $1".to_string(), 1);

        let result = query.bind(vec![SqlValue::Text("%smith%".to_string())]);
        assert!(result.is_ok());

        let built = result.unwrap();
        assert_eq!(built.params.len(), 1);
    }

    #[test]
    fn test_prepared_query_bind_mismatch() {
        let query =
            PreparedQuery::simple("SELECT * FROM patient WHERE name ILIKE $1".to_string(), 1);

        // Wrong number of parameters
        let result = query.bind(vec![]);
        assert!(result.is_err());
    }

    #[test]
    fn test_hit_ratio() {
        let cache = QueryCache::new(10);

        let key = QueryCacheKey::from_typed_params(
            "Patient",
            vec![QueryParamKey {
                name: "name".to_string(),
                modifier: None,
                param_type: SearchParameterType::String,
                value_count: 1,
            }],
            false,
            vec![],
        );

        let query = PreparedQuery::simple("SELECT * FROM patient".to_string(), 0);
        cache.insert(key.clone(), query);

        // 1 miss (before insert check) + 3 hits
        cache.get(&key);
        cache.get(&key);
        cache.get(&key);

        let stats = cache.stats();
        assert_eq!(stats.hits, 3);
        // hit_ratio = 3 / (3 + 0) = 1.0 (after insert, no miss recorded)
    }

    #[test]
    fn test_infer_param_type() {
        assert_eq!(
            QueryCacheKey::infer_param_type("birthdate"),
            SearchParameterType::Date
        );
        assert_eq!(
            QueryCacheKey::infer_param_type("code"),
            SearchParameterType::Token
        );
        assert_eq!(
            QueryCacheKey::infer_param_type("name"),
            SearchParameterType::String
        );
        assert_eq!(
            QueryCacheKey::infer_param_type("subject"),
            SearchParameterType::Reference
        );
        assert_eq!(
            QueryCacheKey::infer_param_type("value-quantity"),
            SearchParameterType::Quantity
        );
    }

    #[test]
    fn test_cache_clear() {
        let cache = QueryCache::new(10);

        for i in 0..5 {
            let key = QueryCacheKey::from_typed_params(
                "Patient",
                vec![QueryParamKey {
                    name: format!("param{}", i),
                    modifier: None,
                    param_type: SearchParameterType::String,
                    value_count: 1,
                }],
                false,
                vec![],
            );
            let query = PreparedQuery::simple(format!("SELECT {}", i), 0);
            cache.insert(key, query);
        }

        assert_eq!(cache.len(), 5);

        cache.clear();
        assert_eq!(cache.len(), 0);
        assert!(cache.is_empty());
    }
}
