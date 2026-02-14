//! Integration tests for Redis caching infrastructure.
//!
//! These tests verify the two-tier caching system:
//! - L1 (DashMap): Local in-memory cache
//! - L2 (Redis): Shared cache across instances
//!
//! Tests use testcontainers to spin up a real Redis instance.

use octofhir_server::{CacheBackend, RedisConfig, create_cache_backend};
use std::sync::Arc;
use std::time::Duration;
use testcontainers::{ContainerAsync, runners::AsyncRunner};
use testcontainers_modules::redis::Redis;
use tokio::sync::OnceCell;

// Shared Redis container for all tests
static SHARED_REDIS: OnceCell<(ContainerAsync<Redis>, String)> = OnceCell::const_new();

/// Get or create the shared Redis container
async fn get_redis_url() -> String {
    let (_, url) = SHARED_REDIS
        .get_or_init(|| async {
            let container = Redis::default()
                .start()
                .await
                .expect("start redis container");

            let host_port = container.get_host_port_ipv4(6379).await.expect("get port");
            let url = format!("redis://127.0.0.1:{}", host_port);

            (container, url)
        })
        .await;

    url.clone()
}

#[tokio::test]
async fn test_local_cache_get_set() {
    let cache = CacheBackend::new_local();

    // Set a value
    cache
        .set("test_key", b"test_value".to_vec(), Duration::from_secs(60))
        .await;

    // Get the value back
    let value = cache.get("test_key").await;
    assert_eq!(value, Some(Arc::new(b"test_value".to_vec())));

    // Check stats
    let stats = cache.stats();
    assert_eq!(stats.mode, "local");
    assert_eq!(stats.l1_entries, 1);
}

#[tokio::test]
async fn test_local_cache_expiration() {
    let cache = CacheBackend::new_local();

    // Set with very short TTL
    cache
        .set(
            "expiring_key",
            b"value".to_vec(),
            Duration::from_millis(100),
        )
        .await;

    // Should be available immediately
    assert!(cache.get("expiring_key").await.is_some());

    // Wait for expiration
    tokio::time::sleep(Duration::from_millis(150)).await;

    // Should be expired now
    assert!(cache.get("expiring_key").await.is_none());
}

#[tokio::test]
async fn test_local_cache_invalidate() {
    let cache = CacheBackend::new_local();

    // Set a value
    cache
        .set(
            "key_to_invalidate",
            b"value".to_vec(),
            Duration::from_secs(60),
        )
        .await;

    // Verify it exists
    assert!(cache.get("key_to_invalidate").await.is_some());

    // Invalidate
    cache.invalidate("key_to_invalidate").await;

    // Should be gone
    assert!(cache.get("key_to_invalidate").await.is_none());
}

#[tokio::test]
async fn test_redis_cache_connection() {
    let redis_url = get_redis_url().await;

    let config = RedisConfig {
        enabled: true,
        url: redis_url,
        pool_size: 5,
        timeout_ms: 5000,
    };

    let cache = create_cache_backend(&config).await;

    // Should have connected to Redis
    assert!(cache.is_redis_available().await);

    let stats = cache.stats();
    assert_eq!(stats.mode, "redis");
}

#[tokio::test]
async fn test_redis_cache_get_set() {
    let redis_url = get_redis_url().await;

    let config = RedisConfig {
        enabled: true,
        url: redis_url,
        pool_size: 5,
        timeout_ms: 5000,
    };

    let cache = create_cache_backend(&config).await;

    // Set a value
    cache
        .set(
            "redis_test_key",
            b"redis_test_value".to_vec(),
            Duration::from_secs(60),
        )
        .await;

    // Wait a bit for async write to complete
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Get the value back (should hit L1 first)
    let value = cache.get("redis_test_key").await;
    assert_eq!(value, Some(Arc::new(b"redis_test_value".to_vec())));
}

#[tokio::test]
async fn test_redis_cache_l1_l2_promotion() {
    let redis_url = get_redis_url().await;

    let config = RedisConfig {
        enabled: true,
        url: redis_url.clone(),
        pool_size: 5,
        timeout_ms: 5000,
    };

    // Create first cache instance
    let cache1 = create_cache_backend(&config).await;

    // Set value in cache1
    cache1
        .set(
            "promotion_key",
            b"promotion_value".to_vec(),
            Duration::from_secs(60),
        )
        .await;

    // Wait for write to L2
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Create second cache instance (simulating another server)
    let cache2 = create_cache_backend(&config).await;

    // Get from cache2 - should retrieve from L2 (Redis) and promote to L1
    let value = cache2.get("promotion_key").await;
    assert_eq!(value, Some(Arc::new(b"promotion_value".to_vec())));

    // Second get should hit L1
    let value = cache2.get("promotion_key").await;
    assert_eq!(value, Some(Arc::new(b"promotion_value".to_vec())));
}

#[tokio::test]
async fn test_redis_cache_invalidation() {
    let redis_url = get_redis_url().await;

    let config = RedisConfig {
        enabled: true,
        url: redis_url,
        pool_size: 5,
        timeout_ms: 5000,
    };

    let cache = create_cache_backend(&config).await;

    // Set a value
    cache
        .set(
            "invalidate_test",
            b"value".to_vec(),
            Duration::from_secs(60),
        )
        .await;

    // Wait for write
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Verify exists
    assert!(cache.get("invalidate_test").await.is_some());

    // Invalidate
    cache.invalidate("invalidate_test").await;

    // Wait for invalidation
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Should be gone from both L1 and L2
    assert!(cache.get("invalidate_test").await.is_none());
}

#[tokio::test]
async fn test_graceful_degradation_invalid_url() {
    let config = RedisConfig {
        enabled: true,
        url: "redis://nonexistent:9999".to_string(),
        pool_size: 5,
        timeout_ms: 1000,
    };

    // Should fall back to local cache
    let cache = create_cache_backend(&config).await;

    // Should not be connected to Redis
    assert!(!cache.is_redis_available().await);

    // But should still work as local cache
    cache
        .set(
            "fallback_key",
            b"fallback_value".to_vec(),
            Duration::from_secs(60),
        )
        .await;

    let value = cache.get("fallback_key").await;
    assert_eq!(value, Some(Arc::new(b"fallback_value".to_vec())));

    let stats = cache.stats();
    assert_eq!(stats.mode, "local");
}

#[tokio::test]
async fn test_disabled_redis() {
    let config = RedisConfig {
        enabled: false,
        url: "redis://localhost:6379".to_string(),
        pool_size: 5,
        timeout_ms: 5000,
    };

    let cache = create_cache_backend(&config).await;

    // Should be local-only
    assert!(!cache.is_redis_available().await);

    let stats = cache.stats();
    assert_eq!(stats.mode, "local");
}
