//! Policy cache for efficient policy evaluation.
//!
//! This module provides an in-memory cache for active policies with automatic
//! refresh and resource type indexing for fast lookups.
//!
//! # Example
//!
//! ```ignore
//! use octofhir_auth::policy::cache::PolicyCache;
//! use std::sync::Arc;
//! use time::Duration;
//!
//! let storage: Arc<dyn PolicyStorage> = /* ... */;
//! let cache = PolicyCache::new(storage, Duration::minutes(5));
//!
//! // Get policies for Patient resources
//! let policies = cache.get_applicable_policies("Patient").await?;
//! ```

use std::collections::HashMap;
use std::sync::Arc;

use time::{Duration, OffsetDateTime};
use tokio::sync::RwLock;

use crate::AuthResult;
use crate::policy::resources::InternalPolicy;
use crate::storage::PolicyStorage;

// =============================================================================
// Policy Cache Error
// =============================================================================

/// Errors that can occur during policy cache operations.
#[derive(Debug, thiserror::Error)]
pub enum PolicyCacheError {
    /// Failed to refresh policies from storage.
    #[error("Failed to refresh policies: {0}")]
    RefreshFailed(String),

    /// Failed to convert policy to internal representation.
    #[error("Failed to convert policy: {0}")]
    ConversionFailed(String),
}

// =============================================================================
// Cached Policies
// =============================================================================

/// Internal cache structure holding all cached policy data.
struct CachedPolicies {
    /// All active policies ordered by priority.
    all_policies: Vec<InternalPolicy>,

    /// Index by resource type for faster lookup.
    /// Maps resource type name to indices in `all_policies`.
    by_resource_type: HashMap<String, Vec<usize>>,

    /// Indices of policies that apply to all resource types (wildcard or no filter).
    wildcard_policies: Vec<usize>,

    /// Last refresh timestamp.
    last_refresh: OffsetDateTime,

    /// Cache version (incremented on each refresh).
    version: u64,
}

impl Default for CachedPolicies {
    fn default() -> Self {
        Self {
            all_policies: Vec::new(),
            by_resource_type: HashMap::new(),
            wildcard_policies: Vec::new(),
            last_refresh: OffsetDateTime::UNIX_EPOCH,
            version: 0,
        }
    }
}

// =============================================================================
// Policy Cache
// =============================================================================

/// In-memory cache for active access policies.
///
/// The cache maintains:
/// - All active policies in priority order
/// - An index by resource type for fast lookups
/// - A list of wildcard policies that apply to all resources
///
/// The cache is automatically refreshed when the TTL expires.
pub struct PolicyCache {
    /// Underlying policy storage.
    storage: Arc<dyn PolicyStorage>,

    /// Cached policy data protected by a read-write lock.
    cache: Arc<RwLock<CachedPolicies>>,

    /// Time-to-live for cached data.
    ttl: Duration,
}

impl PolicyCache {
    /// Create a new policy cache.
    ///
    /// # Arguments
    ///
    /// * `storage` - Policy storage backend
    /// * `ttl` - How long cached data remains valid
    #[must_use]
    pub fn new(storage: Arc<dyn PolicyStorage>, ttl: Duration) -> Self {
        Self {
            storage,
            cache: Arc::new(RwLock::new(CachedPolicies::default())),
            ttl,
        }
    }

    /// Get policies applicable to the given resource type.
    ///
    /// Returns policies that either:
    /// - Have no resource type filter (apply to all)
    /// - Have a wildcard (*) resource type filter
    /// - Explicitly match the given resource type
    ///
    /// Results are sorted by priority (lower priority values first).
    ///
    /// # Errors
    ///
    /// Returns an error if cache refresh fails.
    pub async fn get_applicable_policies(
        &self,
        resource_type: &str,
    ) -> AuthResult<Vec<InternalPolicy>> {
        self.ensure_fresh().await?;

        let cache = self.cache.read().await;

        // Collect indices of applicable policies
        let mut indices: Vec<usize> = Vec::new();

        // Add wildcard policies (apply to all resource types)
        indices.extend(&cache.wildcard_policies);

        // Add resource-specific policies
        if let Some(type_indices) = cache.by_resource_type.get(resource_type) {
            indices.extend(type_indices);
        }

        // Deduplicate (a policy might be in both wildcard and specific lists)
        indices.sort_unstable();
        indices.dedup();

        // Sort by policy priority
        indices.sort_by_key(|&i| cache.all_policies[i].priority);

        // Clone the policies (can't return references due to async)
        Ok(indices
            .iter()
            .map(|&i| cache.all_policies[i].clone())
            .collect())
    }

    /// Get all cached policies.
    ///
    /// # Errors
    ///
    /// Returns an error if cache refresh fails.
    pub async fn get_all_policies(&self) -> AuthResult<Vec<InternalPolicy>> {
        self.ensure_fresh().await?;

        let cache = self.cache.read().await;
        Ok(cache.all_policies.clone())
    }

    /// Force refresh the cache from storage.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Storage fetch fails
    /// - Policy conversion fails
    pub async fn refresh(&self) -> AuthResult<()> {
        let policies = self.storage.list_active().await?;

        let mut internal_policies = Vec::with_capacity(policies.len());
        let mut by_resource_type: HashMap<String, Vec<usize>> = HashMap::new();
        let mut wildcard_policies = Vec::new();

        for (idx, policy) in policies.iter().enumerate() {
            // Convert to internal representation
            let internal = policy.to_internal_policy().map_err(|e| {
                crate::error::AuthError::internal(format!(
                    "Failed to convert policy '{}': {}",
                    policy.name, e
                ))
            })?;

            // Index by resource type
            if let Some(ref types) = internal.matchers.resource_types {
                for t in types {
                    if t == "*" {
                        wildcard_policies.push(idx);
                    } else {
                        by_resource_type.entry(t.clone()).or_default().push(idx);
                    }
                }
            } else {
                // No resource type filter = applies to all
                wildcard_policies.push(idx);
            }

            internal_policies.push(internal);
        }

        // Update cache
        let mut cache = self.cache.write().await;
        cache.all_policies = internal_policies;
        cache.by_resource_type = by_resource_type;
        cache.wildcard_policies = wildcard_policies;
        cache.last_refresh = OffsetDateTime::now_utc();
        cache.version += 1;

        tracing::info!(
            policies = cache.all_policies.len(),
            resource_types = cache.by_resource_type.len(),
            wildcards = cache.wildcard_policies.len(),
            version = cache.version,
            "Policy cache refreshed"
        );

        Ok(())
    }

    /// Ensure cache is fresh, refreshing if TTL has expired.
    async fn ensure_fresh(&self) -> AuthResult<()> {
        let needs_refresh = {
            let cache = self.cache.read().await;
            cache.last_refresh + self.ttl < OffsetDateTime::now_utc()
        };

        if needs_refresh {
            self.refresh().await?;
        }

        Ok(())
    }

    /// Invalidate the cache, forcing a refresh on next access.
    ///
    /// Call this after policy changes (create, update, delete).
    pub async fn invalidate(&self) {
        let mut cache = self.cache.write().await;
        cache.last_refresh = OffsetDateTime::UNIX_EPOCH;
        tracing::debug!(version = cache.version, "Policy cache invalidated");
    }

    /// Get the current cache version.
    ///
    /// The version is incremented on each successful refresh.
    pub async fn version(&self) -> u64 {
        self.cache.read().await.version
    }

    /// Get the last refresh timestamp.
    pub async fn last_refresh(&self) -> OffsetDateTime {
        self.cache.read().await.last_refresh
    }

    /// Check if the cache needs refresh.
    pub async fn needs_refresh(&self) -> bool {
        let cache = self.cache.read().await;
        cache.last_refresh + self.ttl < OffsetDateTime::now_utc()
    }

    /// Get the number of cached policies.
    pub async fn policy_count(&self) -> usize {
        self.cache.read().await.all_policies.len()
    }

    /// Get cache statistics.
    pub async fn stats(&self) -> PolicyCacheStats {
        let cache = self.cache.read().await;
        PolicyCacheStats {
            policy_count: cache.all_policies.len(),
            resource_type_count: cache.by_resource_type.len(),
            wildcard_count: cache.wildcard_policies.len(),
            version: cache.version,
            last_refresh: cache.last_refresh,
            ttl: self.ttl,
        }
    }
}

// =============================================================================
// Cache Statistics
// =============================================================================

/// Statistics about the policy cache.
#[derive(Debug, Clone)]
pub struct PolicyCacheStats {
    /// Number of policies in cache.
    pub policy_count: usize,

    /// Number of distinct resource types indexed.
    pub resource_type_count: usize,

    /// Number of wildcard policies.
    pub wildcard_count: usize,

    /// Current cache version.
    pub version: u64,

    /// Timestamp of last refresh.
    pub last_refresh: OffsetDateTime,

    /// Cache TTL.
    pub ttl: Duration,
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::resources::{AccessPolicy, EngineElement, MatcherElement, PolicyEngineType};
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicUsize, Ordering};

    // -------------------------------------------------------------------------
    // Mock Storage
    // -------------------------------------------------------------------------

    struct MockPolicyStorage {
        policies: Vec<AccessPolicy>,
        call_count: AtomicUsize,
    }

    impl MockPolicyStorage {
        fn new() -> Self {
            Self {
                policies: Vec::new(),
                call_count: AtomicUsize::new(0),
            }
        }

        fn with_policies(policies: Vec<AccessPolicy>) -> Self {
            Self {
                policies,
                call_count: AtomicUsize::new(0),
            }
        }
    }

    #[async_trait]
    impl PolicyStorage for MockPolicyStorage {
        async fn get(&self, id: &str) -> AuthResult<Option<AccessPolicy>> {
            Ok(self
                .policies
                .iter()
                .find(|p| p.id.as_deref() == Some(id))
                .cloned())
        }

        async fn list_active(&self) -> AuthResult<Vec<AccessPolicy>> {
            self.call_count.fetch_add(1, Ordering::SeqCst);
            Ok(self.policies.iter().filter(|p| p.active).cloned().collect())
        }

        async fn list_all(&self) -> AuthResult<Vec<AccessPolicy>> {
            Ok(self.policies.clone())
        }

        async fn create(&self, _policy: &AccessPolicy) -> AuthResult<AccessPolicy> {
            unimplemented!()
        }

        async fn update(&self, _id: &str, _policy: &AccessPolicy) -> AuthResult<AccessPolicy> {
            unimplemented!()
        }

        async fn delete(&self, _id: &str) -> AuthResult<()> {
            unimplemented!()
        }

        async fn find_applicable(
            &self,
            _resource_type: &str,
            _operation: crate::smart::scopes::FhirOperation,
        ) -> AuthResult<Vec<AccessPolicy>> {
            unimplemented!()
        }

        async fn get_by_ids(&self, _ids: &[String]) -> AuthResult<Vec<AccessPolicy>> {
            unimplemented!()
        }

        async fn search(
            &self,
            _params: &crate::storage::PolicySearchParams,
        ) -> AuthResult<Vec<AccessPolicy>> {
            unimplemented!()
        }

        async fn find_for_client(&self, _client_id: &str) -> AuthResult<Vec<AccessPolicy>> {
            unimplemented!()
        }

        async fn find_for_user(&self, _user_id: &str) -> AuthResult<Vec<AccessPolicy>> {
            unimplemented!()
        }

        async fn find_for_role(&self, _role: &str) -> AuthResult<Vec<AccessPolicy>> {
            unimplemented!()
        }
    }

    // -------------------------------------------------------------------------
    // Helper Functions
    // -------------------------------------------------------------------------

    fn create_policy(id: &str, priority: i32, resource_types: Option<Vec<&str>>) -> AccessPolicy {
        AccessPolicy {
            id: Some(id.to_string()),
            name: format!("Policy {}", id),
            priority,
            active: true,
            engine: EngineElement {
                engine_type: PolicyEngineType::Allow,
                script: None,
            },
            matcher: Some(MatcherElement {
                resource_types: resource_types.map(|v| v.into_iter().map(String::from).collect()),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    // -------------------------------------------------------------------------
    // Tests
    // -------------------------------------------------------------------------

    #[tokio::test]
    async fn test_cache_initially_empty() {
        let storage = Arc::new(MockPolicyStorage::new());
        let cache = PolicyCache::new(storage, Duration::minutes(5));

        assert_eq!(cache.version().await, 0);
        assert_eq!(cache.policy_count().await, 0);
    }

    #[tokio::test]
    async fn test_cache_refresh() {
        let storage = Arc::new(MockPolicyStorage::with_policies(vec![create_policy(
            "p1",
            100,
            Some(vec!["Patient"]),
        )]));
        let cache = PolicyCache::new(storage, Duration::minutes(5));

        // Initially version is 0
        assert_eq!(cache.version().await, 0);

        // Refresh loads policies
        cache.refresh().await.unwrap();

        assert_eq!(cache.version().await, 1);
        assert_eq!(cache.policy_count().await, 1);
    }

    #[tokio::test]
    async fn test_get_applicable_policies_by_resource_type() {
        let storage = Arc::new(MockPolicyStorage::with_policies(vec![
            create_policy("p1", 100, Some(vec!["Patient"])),
            create_policy("p2", 50, Some(vec!["Observation"])),
            create_policy("p3", 75, Some(vec!["Patient", "Encounter"])),
        ]));

        let cache = PolicyCache::new(storage, Duration::minutes(5));
        cache.refresh().await.unwrap();

        // Get Patient policies
        let policies = cache.get_applicable_policies("Patient").await.unwrap();
        assert_eq!(policies.len(), 2);

        // Should be sorted by priority
        assert_eq!(policies[0].id, "p3"); // Priority 75
        assert_eq!(policies[1].id, "p1"); // Priority 100

        // Get Observation policies
        let policies = cache.get_applicable_policies("Observation").await.unwrap();
        assert_eq!(policies.len(), 1);
        assert_eq!(policies[0].id, "p2");
    }

    #[tokio::test]
    async fn test_wildcard_policies() {
        let storage = Arc::new(MockPolicyStorage::with_policies(vec![
            create_policy("p1", 100, Some(vec!["Patient"])),
            create_policy("p2", 50, Some(vec!["*"])),
            create_policy("p3", 75, None), // No filter = wildcard
        ]));

        let cache = PolicyCache::new(storage, Duration::minutes(5));
        cache.refresh().await.unwrap();

        // Get Patient policies - should include wildcards
        let policies = cache.get_applicable_policies("Patient").await.unwrap();
        assert_eq!(policies.len(), 3);

        // p2 (priority 50), p3 (priority 75), p1 (priority 100)
        assert_eq!(policies[0].id, "p2");
        assert_eq!(policies[1].id, "p3");
        assert_eq!(policies[2].id, "p1");

        // Get Condition policies - should only have wildcards
        let policies = cache.get_applicable_policies("Condition").await.unwrap();
        assert_eq!(policies.len(), 2);
        assert_eq!(policies[0].id, "p2");
        assert_eq!(policies[1].id, "p3");
    }

    #[tokio::test]
    async fn test_cache_invalidation() {
        let storage = Arc::new(MockPolicyStorage::with_policies(vec![create_policy(
            "p1",
            100,
            Some(vec!["Patient"]),
        )]));

        let cache = PolicyCache::new(storage, Duration::minutes(5));

        cache.refresh().await.unwrap();
        let v1 = cache.version().await;
        assert!(!cache.needs_refresh().await);

        // Invalidate
        cache.invalidate().await;
        assert!(cache.needs_refresh().await);

        // Refresh again
        cache.refresh().await.unwrap();
        assert!(cache.version().await > v1);
    }

    #[tokio::test]
    async fn test_auto_refresh_on_ttl_expiry() {
        let storage = Arc::new(MockPolicyStorage::with_policies(vec![create_policy(
            "p1",
            100,
            Some(vec!["Patient"]),
        )]));

        // Create cache with 0 TTL (always expired)
        let cache = PolicyCache::new(storage.clone(), Duration::ZERO);

        // First access triggers refresh
        let _ = cache.get_applicable_policies("Patient").await.unwrap();
        assert_eq!(storage.call_count.load(Ordering::SeqCst), 1);

        // Second access also triggers refresh (TTL is 0)
        let _ = cache.get_applicable_policies("Patient").await.unwrap();
        assert_eq!(storage.call_count.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn test_cache_stats() {
        let storage = Arc::new(MockPolicyStorage::with_policies(vec![
            create_policy("p1", 100, Some(vec!["Patient"])),
            create_policy("p2", 50, Some(vec!["*"])),
            create_policy("p3", 75, Some(vec!["Observation", "DiagnosticReport"])),
        ]));

        let cache = PolicyCache::new(storage, Duration::minutes(5));
        cache.refresh().await.unwrap();

        let stats = cache.stats().await;

        assert_eq!(stats.policy_count, 3);
        assert_eq!(stats.wildcard_count, 1); // p2 with "*"
        // Patient, Observation, DiagnosticReport = 3 types
        assert_eq!(stats.resource_type_count, 3);
        assert_eq!(stats.version, 1);
    }

    #[tokio::test]
    async fn test_empty_storage() {
        let storage = Arc::new(MockPolicyStorage::new());
        let cache = PolicyCache::new(storage, Duration::minutes(5));

        cache.refresh().await.unwrap();

        let policies = cache.get_applicable_policies("Patient").await.unwrap();
        assert!(policies.is_empty());
    }

    #[tokio::test]
    async fn test_get_all_policies() {
        let storage = Arc::new(MockPolicyStorage::with_policies(vec![
            create_policy("p1", 100, Some(vec!["Patient"])),
            create_policy("p2", 50, Some(vec!["Observation"])),
        ]));

        let cache = PolicyCache::new(storage, Duration::minutes(5));
        cache.refresh().await.unwrap();

        let all = cache.get_all_policies().await.unwrap();
        assert_eq!(all.len(), 2);
    }
}
