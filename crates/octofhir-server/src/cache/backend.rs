//! Cache backend implementation with L1 (DashMap) and L2 (Redis) tiers.

use dashmap::DashMap;
use deadpool_redis::Pool;
use redis::AsyncCommands;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// A cached entry with TTL support.
///
/// The data is wrapped in `Arc` to allow cheap cloning on cache hits,
/// avoiding expensive copies of potentially large FHIR bundles.
#[derive(Clone, Debug)]
pub struct CachedEntry {
    pub data: Arc<Vec<u8>>,
    pub cached_at: Instant,
    pub ttl: Duration,
}

impl CachedEntry {
    /// Create a new cached entry.
    pub fn new(data: Vec<u8>, ttl: Duration) -> Self {
        Self {
            data: Arc::new(data),
            cached_at: Instant::now(),
            ttl,
        }
    }

    /// Check if this entry has expired.
    pub fn is_expired(&self) -> bool {
        self.cached_at.elapsed() > self.ttl
    }
}

/// Two-tier cache backend: L1 (DashMap) + L2 (Redis).
///
/// ## Cache Modes
///
/// - **Local**: Single-instance mode using only DashMap
/// - **Redis**: Multi-instance mode with DashMap (L1) + Redis (L2)
///
/// ## Performance Characteristics
///
/// | Operation | Local Mode | Redis Mode (L1 hit) | Redis Mode (L2 hit) |
/// |-----------|------------|---------------------|---------------------|
/// | GET       | <1µs       | <1µs                | ~5ms                |
/// | SET       | <1µs       | <1µs                | ~5ms (async)        |
/// | DELETE    | <1µs       | <1µs                | ~5ms (async)        |
#[derive(Clone)]
pub enum CacheBackend {
    /// Single-instance: local DashMap only
    Local(Arc<DashMap<String, CachedEntry>>),

    /// Multi-instance: Redis + local L1
    Redis {
        redis: Pool,
        local: Arc<DashMap<String, CachedEntry>>,
    },
}

impl CacheBackend {
    /// Create a new local-only cache backend.
    pub fn new_local() -> Self {
        CacheBackend::Local(Arc::new(DashMap::new()))
    }

    /// Create a new Redis-backed cache backend.
    pub fn new_redis(redis_pool: Pool) -> Self {
        CacheBackend::Redis {
            redis: redis_pool,
            local: Arc::new(DashMap::new()),
        }
    }

    /// Get a value from the cache.
    ///
    /// ## Lookup Order
    ///
    /// 1. Check L1 (DashMap) - microsecond latency
    /// 2. Check L2 (Redis) - millisecond latency
    /// 3. Return None if not found
    ///
    /// If found in L2, the value is promoted to L1.
    ///
    /// Returns `Arc<Vec<u8>>` for zero-copy access to cached data.
    pub async fn get(&self, key: &str) -> Option<Arc<Vec<u8>>> {
        match self {
            CacheBackend::Local(map) => {
                let result = map
                    .get(key)
                    .filter(|entry| !entry.is_expired())
                    .map(|entry| Arc::clone(&entry.data));

                // Record cache metrics
                if result.is_some() {
                    crate::metrics::record_cache_hit("L1");
                } else {
                    crate::metrics::record_cache_miss();
                }

                result
            }
            CacheBackend::Redis { redis, local } => {
                // 1. Check L1 (local DashMap)
                if let Some(entry) = local.get(key) {
                    if !entry.is_expired() {
                        tracing::debug!(key = %key, "cache hit (L1)");
                        crate::metrics::record_cache_hit("L1");
                        return Some(Arc::clone(&entry.data));
                    } else {
                        // Remove expired entry
                        drop(entry);
                        local.remove(key);
                    }
                }

                // 2. Check L2 (Redis)
                match redis.get().await {
                    Ok(mut conn) => match conn.get::<_, Option<Vec<u8>>>(key).await {
                        Ok(Some(data)) => {
                            tracing::debug!(key = %key, "cache hit (L2)");
                            crate::metrics::record_cache_hit("L2");

                            // Wrap in Arc and promote to L1 (use default TTL for L1)
                            let entry = CachedEntry::new(data, Duration::from_secs(3600));
                            let data_arc = Arc::clone(&entry.data);
                            local.insert(key.to_string(), entry);

                            Some(data_arc)
                        }
                        Ok(None) => {
                            tracing::debug!(key = %key, "cache miss");
                            crate::metrics::record_cache_miss();
                            None
                        }
                        Err(e) => {
                            tracing::warn!(key = %key, error = %e, "Redis GET error");
                            crate::metrics::record_cache_miss();
                            None
                        }
                    },
                    Err(e) => {
                        tracing::warn!(error = %e, "Failed to get Redis connection");
                        crate::metrics::record_cache_miss();
                        None
                    }
                }
            }
        }
    }

    /// Set a value in the cache with TTL.
    ///
    /// ## Write Strategy
    ///
    /// - **Local mode**: Write to DashMap only
    /// - **Redis mode**: Write to both L1 (DashMap) and L2 (Redis)
    ///
    /// Redis writes are fire-and-forget (we don't wait for confirmation).
    pub async fn set(&self, key: &str, value: Vec<u8>, ttl: Duration) {
        match self {
            CacheBackend::Local(map) => {
                map.insert(key.to_string(), CachedEntry::new(value, ttl));
            }
            CacheBackend::Redis { redis, local } => {
                // Create entry and clone Arc for Redis write (no full data clone)
                let entry = CachedEntry::new(value, ttl);
                let data_for_redis = Arc::clone(&entry.data);

                // Store in L1
                local.insert(key.to_string(), entry);

                // Store in L2 (Redis) with TTL - fire and forget
                let redis = redis.clone();
                let key = key.to_string();
                let ttl_secs = ttl.as_secs();
                tokio::spawn(async move {
                    if let Ok(mut conn) = redis.get().await {
                        if let Err(e) = conn
                            .set_ex::<_, _, ()>(&key, &*data_for_redis, ttl_secs)
                            .await
                        {
                            tracing::warn!(key = %key, error = %e, "Redis SET error");
                        } else {
                            tracing::debug!(key = %key, ttl_secs = %ttl_secs, "cache set (L1+L2)");
                        }
                    }
                });
            }
        }
    }

    /// Invalidate a cache entry.
    ///
    /// ## Invalidation Strategy
    ///
    /// - **Local mode**: Remove from DashMap
    /// - **Redis mode**: Remove from L1 and L2, then publish invalidation event
    pub async fn invalidate(&self, key: &str) {
        match self {
            CacheBackend::Local(map) => {
                map.remove(key);
                tracing::debug!(key = %key, "cache invalidated (local)");
            }
            CacheBackend::Redis { redis, local } => {
                // Remove from L1
                local.remove(key);

                // Remove from L2 and publish invalidation - fire and forget
                let redis = redis.clone();
                let key = key.to_string();
                tokio::spawn(async move {
                    if let Ok(mut conn) = redis.get().await {
                        // Delete from Redis
                        if let Err(e) = conn.del::<_, ()>(&key).await {
                            tracing::warn!(key = %key, error = %e, "Redis DEL error");
                        }

                        // Publish invalidation to other instances
                        if let Err(e) = conn.publish::<_, _, ()>("cache:invalidate", &key).await {
                            tracing::warn!(key = %key, error = %e, "Redis PUBLISH error");
                        } else {
                            tracing::debug!(key = %key, "cache invalidated (L1+L2+pub/sub)");
                        }
                    }
                });
            }
        }
    }

    /// Get cache statistics (L1 only).
    pub fn stats(&self) -> CacheStats {
        match self {
            CacheBackend::Local(map) => CacheStats {
                l1_entries: map.len(),
                mode: "local".to_string(),
            },
            CacheBackend::Redis { local, .. } => CacheStats {
                l1_entries: local.len(),
                mode: "redis".to_string(),
            },
        }
    }

    /// Check if Redis is available (for health checks).
    pub async fn is_redis_available(&self) -> bool {
        match self {
            CacheBackend::Local(_) => false,
            CacheBackend::Redis { redis, .. } => redis.get().await.is_ok(),
        }
    }

    /// Get the local cache reference (for testing/internal use).
    pub fn local_cache(&self) -> Option<&Arc<DashMap<String, CachedEntry>>> {
        match self {
            CacheBackend::Local(map) => Some(map),
            CacheBackend::Redis { local, .. } => Some(local),
        }
    }
}

/// Cache statistics.
#[derive(Debug, Clone)]
pub struct CacheStats {
    pub l1_entries: usize,
    pub mode: String,
}
