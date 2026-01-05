//! Authentication context caching for performance optimization.
//!
//! This module provides a trait-based caching system for `AuthContext` objects,
//! designed to reduce database queries for token validation. The design is
//! Redis-ready, allowing future extension to a tiered L1+L2 cache.
//!
//! ## Architecture
//!
//! - **L1 (DashMap)**: In-memory cache with TTL, per-instance
//! - **Future L2 (Redis)**: Shared cache across instances (not yet implemented)
//!
//! ## Cache Key
//!
//! The JWT ID (`jti`) is used as the cache key, as it uniquely identifies
//! each access token.
//!
//! ## Invalidation
//!
//! When a token is revoked, `invalidate()` is called to immediately remove
//! it from the cache. This ensures revoked tokens cannot be used even if
//! they haven't expired yet.

use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use dashmap::DashMap;
use octofhir_auth::middleware::types::AuthContext;

/// Trait for authentication context caching.
///
/// This trait abstracts the cache implementation, allowing future extension
/// to support Redis or other backends without changing consumer code.
///
/// The cache stores `Arc<AuthContext>` internally to avoid expensive clones
/// of the `Client` and `UserContext` structures on every cache hit.
#[async_trait]
pub trait AuthContextCache: Send + Sync {
    /// Get a cached auth context by JWT ID.
    ///
    /// Returns `None` if the entry doesn't exist or has expired.
    /// Returns `Arc<AuthContext>` for cheap cloning on cache hits.
    async fn get(&self, jti: &str) -> Option<Arc<AuthContext>>;

    /// Insert an auth context into the cache.
    ///
    /// The entry will be wrapped in `Arc` and cached with the implementation's TTL.
    /// Returns the `Arc<AuthContext>` for use by the caller (avoids extra allocation).
    async fn insert(&self, jti: String, ctx: AuthContext) -> Arc<AuthContext>;

    /// Invalidate (remove) a cached auth context.
    ///
    /// Called when a token is revoked to ensure it cannot be used.
    async fn invalidate(&self, jti: &str);

    /// Clear all cached entries.
    ///
    /// Used for testing or administrative purposes.
    async fn clear(&self);

    /// Get cache statistics for monitoring.
    fn stats(&self) -> CacheStats;

    /// Clean up expired entries.
    ///
    /// This is called by the background cleanup task.
    /// Default implementation is a no-op (for backends like Redis with native TTL).
    fn cleanup_expired(&self) -> usize {
        0
    }
}

/// Cache statistics for monitoring.
#[derive(Debug, Clone, Default)]
pub struct CacheStats {
    /// Number of entries currently in the cache.
    pub size: usize,
    /// Number of cache hits.
    pub hits: u64,
    /// Number of cache misses.
    pub misses: u64,
    /// Number of entries evicted due to TTL expiration.
    pub evictions: u64,
}

impl CacheStats {
    /// Calculate hit rate as a percentage.
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            0.0
        } else {
            (self.hits as f64 / total as f64) * 100.0
        }
    }
}

/// Cached entry with expiration time.
struct CachedEntry {
    /// Auth context wrapped in Arc for cheap cloning on cache hits.
    context: Arc<AuthContext>,
    expires_at: Instant,
}

/// Local in-memory auth context cache using DashMap.
///
/// This is the L1 cache implementation with TTL-based expiration.
/// It's thread-safe and can be shared across async tasks.
pub struct LocalAuthCache {
    cache: DashMap<String, CachedEntry>,
    ttl: Duration,
    hits: std::sync::atomic::AtomicU64,
    misses: std::sync::atomic::AtomicU64,
    evictions: std::sync::atomic::AtomicU64,
}

impl LocalAuthCache {
    /// Create a new local auth cache with the specified TTL.
    ///
    /// # Arguments
    ///
    /// * `ttl` - Time-to-live for cached entries
    ///
    /// # Example
    ///
    /// ```
    /// use std::time::Duration;
    /// use octofhir_server::cache::auth::LocalAuthCache;
    ///
    /// let cache = LocalAuthCache::new(Duration::from_secs(60));
    /// ```
    pub fn new(ttl: Duration) -> Self {
        Self {
            cache: DashMap::new(),
            ttl,
            hits: std::sync::atomic::AtomicU64::new(0),
            misses: std::sync::atomic::AtomicU64::new(0),
            evictions: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// Create a new local auth cache with the default TTL of 60 seconds.
    pub fn default_ttl() -> Self {
        Self::new(Duration::from_secs(60))
    }

    /// Clean up expired entries.
    ///
    /// This is called periodically to remove stale entries and free memory.
    /// Returns the number of entries removed.
    pub fn cleanup_expired(&self) -> usize {
        let now = Instant::now();
        let mut removed = 0;

        self.cache.retain(|_, entry| {
            if entry.expires_at <= now {
                removed += 1;
                false
            } else {
                true
            }
        });

        if removed > 0 {
            self.evictions
                .fetch_add(removed as u64, std::sync::atomic::Ordering::Relaxed);
        }

        removed
    }
}

#[async_trait]
impl AuthContextCache for LocalAuthCache {
    async fn get(&self, jti: &str) -> Option<Arc<AuthContext>> {
        let now = Instant::now();

        if let Some(entry) = self.cache.get(jti) {
            if entry.expires_at > now {
                self.hits
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                // Arc::clone is cheap - just increments reference count
                return Some(Arc::clone(&entry.context));
            }
            // Entry expired, remove it
            drop(entry);
            self.cache.remove(jti);
            self.evictions
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }

        self.misses
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        None
    }

    async fn insert(&self, jti: String, ctx: AuthContext) -> Arc<AuthContext> {
        // Wrap in Arc for cheap cloning on subsequent gets
        let arc_ctx = Arc::new(ctx);
        let entry = CachedEntry {
            context: Arc::clone(&arc_ctx),
            expires_at: Instant::now() + self.ttl,
        };
        self.cache.insert(jti, entry);
        arc_ctx
    }

    async fn invalidate(&self, jti: &str) {
        self.cache.remove(jti);
    }

    async fn clear(&self) {
        self.cache.clear();
    }

    fn stats(&self) -> CacheStats {
        CacheStats {
            size: self.cache.len(),
            hits: self.hits.load(std::sync::atomic::Ordering::Relaxed),
            misses: self.misses.load(std::sync::atomic::Ordering::Relaxed),
            evictions: self.evictions.load(std::sync::atomic::Ordering::Relaxed),
        }
    }

    fn cleanup_expired(&self) -> usize {
        LocalAuthCache::cleanup_expired(self)
    }
}

/// A no-op cache implementation for testing or when caching is disabled.
pub struct NoOpAuthCache;

#[async_trait]
impl AuthContextCache for NoOpAuthCache {
    async fn get(&self, _jti: &str) -> Option<Arc<AuthContext>> {
        None
    }

    async fn insert(&self, _jti: String, ctx: AuthContext) -> Arc<AuthContext> {
        // No-op cache, but still return Arc for consistency
        Arc::new(ctx)
    }

    async fn invalidate(&self, _jti: &str) {
        // No-op
    }

    async fn clear(&self) {
        // No-op
    }

    fn stats(&self) -> CacheStats {
        CacheStats::default()
    }
}

/// Create a shared auth cache instance.
///
/// Returns an `Arc<dyn AuthContextCache>` that can be shared across threads.
pub fn create_auth_cache(ttl: Duration) -> Arc<dyn AuthContextCache> {
    Arc::new(LocalAuthCache::new(ttl))
}

#[cfg(test)]
mod tests {
    use super::*;
    use octofhir_auth::middleware::types::UserContext;
    use octofhir_auth::token::jwt::AccessTokenClaims;
    use octofhir_auth::types::{Client, GrantType};
    use std::collections::HashMap;
    use std::sync::Arc;
    use uuid::Uuid;

    fn create_test_auth_context(jti: &str) -> AuthContext {
        AuthContext {
            token_claims: Arc::new(AccessTokenClaims {
                iss: "https://auth.example.com".to_string(),
                sub: "user123".to_string(),
                aud: vec!["https://fhir.example.com".to_string()],
                exp: 9999999999,
                iat: 1000000000,
                jti: jti.to_string(),
                scope: "openid patient/Patient.read".to_string(),
                client_id: "test-client".to_string(),
                patient: None,
                encounter: None,
                fhir_user: None,
                sid: None,
            }),
            client: Client {
                client_id: "test-client".to_string(),
                client_secret: None,
                name: "Test Client".to_string(),
                description: None,
                grant_types: vec![GrantType::AuthorizationCode],
                redirect_uris: vec!["https://app.example.com/callback".to_string()],
                post_logout_redirect_uris: vec![],
                scopes: vec![],
                confidential: false,
                active: true,
                access_token_lifetime: None,
                refresh_token_lifetime: None,
                pkce_required: None,
                allowed_origins: vec![],
                jwks: None,
                jwks_uri: None,
            },
            user: Some(UserContext {
                id: Uuid::new_v4().to_string(),
                username: "testuser".to_string(),
                name: Some("Test User".to_string()),
                email: Some("test@example.com".to_string()),
                fhir_user: Some("Practitioner/123".to_string()),
                roles: vec!["practitioner".to_string()],
                attributes: HashMap::new(),
            }),
            patient: None,
            encounter: None,
        }
    }

    #[tokio::test]
    async fn test_insert_and_get() {
        let cache = LocalAuthCache::new(Duration::from_secs(60));
        let jti = "test-jti-123";
        let ctx = create_test_auth_context(jti);

        cache.insert(jti.to_string(), ctx).await;

        let result = cache.get(jti).await;
        assert!(result.is_some());
        // Result is Arc<AuthContext>, so we can use it directly
        assert_eq!(result.unwrap().jti(), jti);

        let stats = cache.stats();
        assert_eq!(stats.size, 1);
        assert_eq!(stats.hits, 1);
        assert_eq!(stats.misses, 0);
    }

    #[tokio::test]
    async fn test_cache_miss() {
        let cache = LocalAuthCache::new(Duration::from_secs(60));

        let result = cache.get("nonexistent").await;
        assert!(result.is_none());

        let stats = cache.stats();
        assert_eq!(stats.hits, 0);
        assert_eq!(stats.misses, 1);
    }

    #[tokio::test]
    async fn test_invalidate() {
        let cache = LocalAuthCache::new(Duration::from_secs(60));
        let jti = "test-jti-456";
        let ctx = create_test_auth_context(jti);

        cache.insert(jti.to_string(), ctx).await;
        assert!(cache.get(jti).await.is_some());

        cache.invalidate(jti).await;
        assert!(cache.get(jti).await.is_none());
    }

    #[tokio::test]
    async fn test_expiration() {
        let cache = LocalAuthCache::new(Duration::from_millis(10));
        let jti = "test-jti-789";
        let ctx = create_test_auth_context(jti);

        cache.insert(jti.to_string(), ctx).await;
        assert!(cache.get(jti).await.is_some());

        // Wait for expiration
        tokio::time::sleep(Duration::from_millis(20)).await;

        // Should return None and increment evictions
        assert!(cache.get(jti).await.is_none());
        assert_eq!(cache.stats().evictions, 1);
    }

    #[tokio::test]
    async fn test_clear() {
        let cache = LocalAuthCache::new(Duration::from_secs(60));

        for i in 0..5 {
            let jti = format!("jti-{}", i);
            cache
                .insert(jti.clone(), create_test_auth_context(&jti))
                .await;
        }

        assert_eq!(cache.stats().size, 5);

        cache.clear().await;
        assert_eq!(cache.stats().size, 0);
    }

    #[tokio::test]
    async fn test_cleanup_expired() {
        let cache = LocalAuthCache::new(Duration::from_millis(10));

        for i in 0..3 {
            let jti = format!("jti-{}", i);
            cache
                .insert(jti.clone(), create_test_auth_context(&jti))
                .await;
        }

        assert_eq!(cache.stats().size, 3);

        // Wait for expiration
        tokio::time::sleep(Duration::from_millis(20)).await;

        let removed = cache.cleanup_expired();
        assert_eq!(removed, 3);
        assert_eq!(cache.stats().size, 0);
    }

    #[test]
    fn test_hit_rate_calculation() {
        let stats = CacheStats {
            size: 10,
            hits: 75,
            misses: 25,
            evictions: 5,
        };

        assert!((stats.hit_rate() - 75.0).abs() < 0.001);

        let empty_stats = CacheStats::default();
        assert!((empty_stats.hit_rate() - 0.0).abs() < 0.001);
    }
}
