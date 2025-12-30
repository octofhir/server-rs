//! JWT verification cache to avoid expensive signature verification.
//!
//! This cache stores verified JWT claims keyed by a hash of the token.
//! It allows skipping the expensive cryptographic signature verification
//! for tokens that have been recently verified.
//!
//! ## Security Considerations
//!
//! - The cache uses a hash of the token, not the token itself
//! - TTL is short (default 60s) to limit exposure if a token is revoked
//! - The JTI is still checked against revocation after cache hit
//! - Cache entries expire before the token's exp claim

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::DashMap;
use octofhir_auth::token::jwt::AccessTokenClaims;

/// Default maximum cache size to prevent unbounded memory growth.
const DEFAULT_MAX_SIZE: usize = 10_000;

/// Cache for verified JWT claims.
///
/// This cache stores the result of JWT signature verification to avoid
/// repeating expensive cryptographic operations for the same token.
///
/// The cache has a maximum size limit to prevent DoS attacks that
/// could exhaust memory by sending many unique tokens.
pub struct JwtVerificationCache {
    /// Cache entries keyed by token hash
    cache: DashMap<u64, CachedClaims>,
    /// Secondary index: JTI -> token hashes for O(1) invalidation by JTI
    jti_index: DashMap<String, Vec<u64>>,
    /// Time-to-live for cache entries
    ttl: Duration,
    /// Maximum number of entries to prevent unbounded growth
    max_size: usize,
    /// Cache hit counter
    hits: AtomicU64,
    /// Cache miss counter
    misses: AtomicU64,
    /// Eviction counter (entries removed due to size limit)
    evictions: AtomicU64,
}

/// Cached JWT claims with expiration.
struct CachedClaims {
    claims: Arc<AccessTokenClaims>,
    expires_at: Instant,
}

impl JwtVerificationCache {
    /// Create a new JWT verification cache with the specified TTL and max size.
    ///
    /// # Arguments
    ///
    /// * `ttl` - Time-to-live for cached entries. Should be shorter than
    ///           typical token lifetime to ensure revoked tokens don't
    ///           stay cached too long.
    /// * `max_size` - Maximum number of entries. When exceeded, expired entries
    ///                are cleaned up to make room.
    pub fn new(ttl: Duration, max_size: usize) -> Self {
        Self {
            cache: DashMap::new(),
            jti_index: DashMap::new(),
            ttl,
            max_size,
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
            evictions: AtomicU64::new(0),
        }
    }

    /// Create a new cache with the default TTL of 30 seconds and default max size.
    ///
    /// 30 seconds is a good balance between performance and security:
    /// - Long enough to cache most repeated requests
    /// - Short enough that revoked tokens are quickly invalidated
    ///
    /// Default max size of 10,000 entries provides protection against DoS
    /// while allowing caching for high-traffic deployments.
    pub fn default_ttl() -> Self {
        Self::new(Duration::from_secs(30), DEFAULT_MAX_SIZE)
    }

    /// Get cached claims for a token.
    ///
    /// Returns `Some(claims)` if the token was recently verified and
    /// the cached entry hasn't expired.
    pub fn get(&self, token: &str) -> Option<Arc<AccessTokenClaims>> {
        let hash = Self::hash_token(token);
        let now = Instant::now();

        if let Some(entry) = self.cache.get(&hash) {
            if entry.expires_at > now {
                self.hits.fetch_add(1, Ordering::Relaxed);
                return Some(entry.claims.clone());
            }
            // Entry expired, remove it
            drop(entry);
            self.cache.remove(&hash);
        }

        self.misses.fetch_add(1, Ordering::Relaxed);
        None
    }

    /// Cache verified claims for a token.
    ///
    /// The claims are cached with the configured TTL, but will also
    /// respect the token's exp claim to avoid caching expired tokens.
    ///
    /// If the cache exceeds max_size, expired entries are cleaned up first.
    /// This prevents unbounded memory growth from DoS attacks.
    pub fn insert(&self, token: &str, claims: Arc<AccessTokenClaims>) {
        let hash = Self::hash_token(token);
        let now = Instant::now();

        // Calculate expiration: min of TTL and token exp
        let token_exp_secs = claims.exp - time::OffsetDateTime::now_utc().unix_timestamp();
        let token_exp_duration = if token_exp_secs > 0 {
            Duration::from_secs(token_exp_secs as u64)
        } else {
            // Token already expired, don't cache
            return;
        };

        // Check size limit and cleanup if needed
        if self.cache.len() >= self.max_size {
            let removed = self.cleanup_expired();
            self.evictions.fetch_add(removed as u64, Ordering::Relaxed);

            // If still at capacity after cleanup, skip insertion
            // This prevents unbounded growth under attack
            if self.cache.len() >= self.max_size {
                tracing::warn!(
                    max_size = self.max_size,
                    "JWT cache at capacity, skipping insertion"
                );
                return;
            }
        }

        let cache_ttl = self.ttl.min(token_exp_duration);
        let jti = claims.jti.clone();

        self.cache.insert(
            hash,
            CachedClaims {
                claims,
                expires_at: now + cache_ttl,
            },
        );

        // Update JTI index for O(1) invalidation by JTI
        self.jti_index
            .entry(jti)
            .or_default()
            .push(hash);
    }

    /// Invalidate cache entry for a token.
    ///
    /// Called when a token is revoked to ensure it's removed from cache.
    pub fn invalidate(&self, token: &str) {
        let hash = Self::hash_token(token);
        if let Some((_, entry)) = self.cache.remove(&hash) {
            // Also remove from JTI index
            if let Some(mut hashes) = self.jti_index.get_mut(&entry.claims.jti) {
                hashes.retain(|h| *h != hash);
            }
        }
    }

    /// Invalidate by JTI - removes any cached entry with matching JTI.
    ///
    /// Uses a secondary index for O(1) lookup instead of O(n) scan.
    pub fn invalidate_by_jti(&self, jti: &str) {
        if let Some((_, hashes)) = self.jti_index.remove(jti) {
            for hash in hashes {
                self.cache.remove(&hash);
            }
        }
    }

    /// Get cache statistics.
    pub fn stats(&self) -> JwtCacheStats {
        JwtCacheStats {
            size: self.cache.len(),
            max_size: self.max_size,
            hits: self.hits.load(Ordering::Relaxed),
            misses: self.misses.load(Ordering::Relaxed),
            evictions: self.evictions.load(Ordering::Relaxed),
        }
    }

    /// Clean up expired entries.
    ///
    /// Returns the number of entries removed.
    pub fn cleanup_expired(&self) -> usize {
        let now = Instant::now();

        // Collect expired entries: (hash, jti)
        let expired: Vec<(u64, String)> = self
            .cache
            .iter()
            .filter(|entry| entry.expires_at <= now)
            .map(|entry| (*entry.key(), entry.claims.jti.clone()))
            .collect();

        // Remove from cache and update JTI index
        for (hash, jti) in &expired {
            self.cache.remove(hash);
            if let Some(mut hashes) = self.jti_index.get_mut(jti) {
                hashes.retain(|h| h != hash);
            }
        }

        // Clean up empty JTI index entries
        self.jti_index.retain(|_, hashes| !hashes.is_empty());

        expired.len()
    }

    /// Hash a token to use as cache key.
    ///
    /// Uses a fast non-cryptographic hash since we're not protecting
    /// against adversarial inputs - we just need a quick lookup key.
    #[inline]
    fn hash_token(token: &str) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        token.hash(&mut hasher);
        hasher.finish()
    }
}

/// Statistics for JWT verification cache.
#[derive(Debug, Clone, Default)]
pub struct JwtCacheStats {
    /// Number of entries currently in the cache.
    pub size: usize,
    /// Maximum allowed entries.
    pub max_size: usize,
    /// Number of cache hits (signature verification skipped).
    pub hits: u64,
    /// Number of cache misses (signature verification required).
    pub misses: u64,
    /// Number of entries evicted due to size limit or expiration.
    pub evictions: u64,
}

impl JwtCacheStats {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_claims(jti: &str, exp_offset_secs: i64) -> Arc<AccessTokenClaims> {
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        Arc::new(AccessTokenClaims {
            iss: "https://auth.example.com".to_string(),
            sub: "user123".to_string(),
            aud: vec!["https://fhir.example.com".to_string()],
            exp: now + exp_offset_secs,
            iat: now,
            jti: jti.to_string(),
            scope: "openid".to_string(),
            client_id: "test-client".to_string(),
            patient: None,
            encounter: None,
            fhir_user: None,
            sid: None,
        })
    }

    #[test]
    fn test_insert_and_get() {
        let cache = JwtVerificationCache::new(Duration::from_secs(60), 100);
        let token = "eyJ0eXAiOiJKV1QiLCJhbGciOiJSUzM4NCJ9.test";
        let claims = create_test_claims("jti-123", 3600);

        cache.insert(token, claims.clone());

        let result = cache.get(token);
        assert!(result.is_some());
        assert_eq!(result.unwrap().jti, "jti-123");

        let stats = cache.stats();
        assert_eq!(stats.size, 1);
        assert_eq!(stats.hits, 1);
        assert_eq!(stats.misses, 0);
    }

    #[test]
    fn test_cache_miss() {
        let cache = JwtVerificationCache::new(Duration::from_secs(60), 100);

        let result = cache.get("nonexistent-token");
        assert!(result.is_none());

        let stats = cache.stats();
        assert_eq!(stats.hits, 0);
        assert_eq!(stats.misses, 1);
    }

    #[test]
    fn test_invalidate() {
        let cache = JwtVerificationCache::new(Duration::from_secs(60), 100);
        let token = "test-token";
        let claims = create_test_claims("jti-456", 3600);

        cache.insert(token, claims);
        assert!(cache.get(token).is_some());

        cache.invalidate(token);
        assert!(cache.get(token).is_none());
    }

    #[test]
    fn test_invalidate_by_jti() {
        let cache = JwtVerificationCache::new(Duration::from_secs(60), 100);

        cache.insert("token1", create_test_claims("jti-1", 3600));
        cache.insert("token2", create_test_claims("jti-2", 3600));
        cache.insert("token3", create_test_claims("jti-1", 3600)); // Same JTI

        assert_eq!(cache.stats().size, 3);

        cache.invalidate_by_jti("jti-1");

        assert_eq!(cache.stats().size, 1);
        assert!(cache.get("token2").is_some());
    }

    #[test]
    fn test_expired_token_not_cached() {
        let cache = JwtVerificationCache::new(Duration::from_secs(60), 100);
        let token = "expired-token";
        let claims = create_test_claims("jti-expired", -10); // Already expired

        cache.insert(token, claims);

        // Should not be cached since token is expired
        assert_eq!(cache.stats().size, 0);
    }

    #[test]
    fn test_max_size_limit() {
        // Create cache with max size of 3
        let cache = JwtVerificationCache::new(Duration::from_secs(3600), 3);

        // Insert 3 entries (at capacity)
        cache.insert("token1", create_test_claims("jti-1", 3600));
        cache.insert("token2", create_test_claims("jti-2", 3600));
        cache.insert("token3", create_test_claims("jti-3", 3600));
        assert_eq!(cache.stats().size, 3);

        // Try to insert 4th entry - should be skipped (entries not expired)
        cache.insert("token4", create_test_claims("jti-4", 3600));
        assert_eq!(cache.stats().size, 3);

        // token4 should not be in cache
        assert!(cache.get("token4").is_none());
    }

    #[test]
    fn test_hit_rate() {
        let stats = JwtCacheStats {
            size: 10,
            max_size: 100,
            hits: 80,
            misses: 20,
            evictions: 0,
        };

        assert!((stats.hit_rate() - 80.0).abs() < 0.001);
    }
}
