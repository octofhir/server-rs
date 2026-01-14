//! Reloadable search configuration with hot-reload support.
//!
//! This module provides a wrapper around `SearchConfig` that supports
//! dynamic reloading of the search parameter registry without server restart.
//!
//! # Usage
//!
//! ```ignore
//! use octofhir_search::reloadable::{ReloadableSearchConfig, SearchConfig};
//! use octofhir_config::ConfigurationManager;
//!
//! // Create reloadable config
//! let search_config = ReloadableSearchConfig::new(canonical_manager, config).await?;
//!
//! // Subscribe to config changes
//! let rx = config_manager.subscribe();
//! search_config.subscribe_to_changes(rx, canonical_manager.clone()).await;
//!
//! // Use in search operations
//! let config = search_config.config().await;
//! // Use config.registry to look up search parameters
//! ```

use arc_swap::ArcSwap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info};

use octofhir_canonical_manager::CanonicalManager;

use crate::loader::{LoaderError, load_search_parameters};
use crate::query_cache::{CacheStatsSnapshot, QueryCache};
use crate::registry::SearchParameterRegistry;

/// Search configuration with dynamic parameter registry.
///
/// The registry is loaded from the FHIR canonical manager and contains all
/// search parameters from loaded packages (e.g., hl7.fhir.r4.core).
#[derive(Debug, Clone)]
pub struct SearchConfig {
    pub default_count: usize,
    pub max_count: usize,
    /// Search parameter registry loaded from canonical manager (REQUIRED)
    pub registry: Arc<SearchParameterRegistry>,
    /// Optional query cache for performance optimization
    pub cache: Option<Arc<QueryCache>>,
}

impl SearchConfig {
    /// Create a new search config with the given registry.
    pub fn new(registry: Arc<SearchParameterRegistry>) -> Self {
        Self {
            default_count: 10,
            max_count: 100,
            registry,
            cache: None,
        }
    }

    /// Create with custom count settings.
    pub fn with_counts(mut self, default_count: usize, max_count: usize) -> Self {
        self.default_count = default_count;
        self.max_count = max_count;
        self
    }

    /// Enable query caching with the given capacity.
    pub fn with_cache(mut self, capacity: usize) -> Self {
        self.cache = Some(Arc::new(QueryCache::new(capacity)));
        self
    }

    /// Enable query caching with a provided cache instance.
    pub fn with_shared_cache(mut self, cache: Arc<QueryCache>) -> Self {
        self.cache = Some(cache);
        self
    }

    /// Get cache statistics if caching is enabled.
    pub fn cache_stats(&self) -> Option<CacheStatsSnapshot> {
        self.cache.as_ref().map(|c| c.stats())
    }
}

/// Configuration options for search behavior.
#[derive(Debug, Clone)]
pub struct SearchOptions {
    /// Default number of results per page
    pub default_count: usize,
    /// Maximum allowed results per page
    pub max_count: usize,
    /// Query cache capacity (0 to disable)
    pub cache_capacity: usize,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            default_count: 10,
            max_count: 100,
            cache_capacity: 1000,
        }
    }
}

/// A reloadable search configuration wrapper.
///
/// Holds a `SearchConfig` behind an `ArcSwap` allowing for lock-free reads
/// and dynamic updates to the search parameter registry and configuration options.
#[derive(Clone)]
pub struct ReloadableSearchConfig {
    /// Inner config with atomic pointer swap (lock-free reads!)
    inner: Arc<ArcSwap<SearchConfig>>,
    /// Current options
    options: Arc<RwLock<SearchOptions>>,
    /// Shared query cache (survives reloads)
    cache: Option<Arc<QueryCache>>,
}

impl ReloadableSearchConfig {
    /// Create a new reloadable search config.
    ///
    /// # Arguments
    ///
    /// * `canonical_manager` - The canonical manager for loading search parameters
    /// * `options` - Search configuration options
    ///
    /// # Returns
    ///
    /// A new `ReloadableSearchConfig` or an error if loading fails.
    pub async fn new(
        canonical_manager: &CanonicalManager,
        options: SearchOptions,
    ) -> Result<Self, LoaderError> {
        // Load initial registry
        let registry = Arc::new(load_search_parameters(canonical_manager).await?);

        // Create shared cache if capacity > 0
        let cache = if options.cache_capacity > 0 {
            Some(Arc::new(QueryCache::new(options.cache_capacity)))
        } else {
            None
        };

        let config = SearchConfig {
            default_count: options.default_count,
            max_count: options.max_count,
            registry,
            cache: cache.clone(),
        };

        Ok(Self {
            inner: Arc::new(ArcSwap::from_pointee(config)),
            options: Arc::new(RwLock::new(options)),
            cache,
        })
    }

    /// Create with a pre-built registry (useful for testing).
    pub fn with_registry(registry: Arc<SearchParameterRegistry>, options: SearchOptions) -> Self {
        let cache = if options.cache_capacity > 0 {
            Some(Arc::new(QueryCache::new(options.cache_capacity)))
        } else {
            None
        };

        let config = SearchConfig {
            default_count: options.default_count,
            max_count: options.max_count,
            registry,
            cache: cache.clone(),
        };

        Self {
            inner: Arc::new(ArcSwap::from_pointee(config)),
            options: Arc::new(RwLock::new(options)),
            cache,
        }
    }

    /// Get a snapshot of the current search configuration (LOCK-FREE!).
    ///
    /// Returns an Arc to the current config for use in search operations.
    /// This operation is extremely fast (single atomic load) and never blocks.
    pub fn config(&self) -> Arc<SearchConfig> {
        self.inner.load_full()
    }

    /// Reload the search parameter registry from the canonical manager.
    ///
    /// This rebuilds the registry from scratch, useful when:
    /// - FHIR packages are added or updated
    /// - Search configuration changes require a fresh registry
    ///
    /// Uses atomic pointer swap - old readers continue with old config, new readers get new config.
    /// This operation does NOT block concurrent readers!
    pub async fn reload_registry(
        &self,
        canonical_manager: &CanonicalManager,
    ) -> Result<(), LoaderError> {
        info!("Reloading search parameter registry");

        // Load new registry (happens in background, doesn't block readers)
        let new_registry = Arc::new(load_search_parameters(canonical_manager).await?);

        // Get current config
        let current = self.inner.load_full();

        // Create new config with new registry
        let new_config = SearchConfig {
            default_count: current.default_count,
            max_count: current.max_count,
            registry: new_registry,
            cache: self.cache.clone(),
        };

        // Atomic swap - old readers continue with old config, new readers get new config
        self.inner.store(Arc::new(new_config));

        // Clear query cache on registry reload (cached queries may reference old params)
        if let Some(cache) = &self.cache {
            cache.clear();
            debug!("Cleared query cache after registry reload");
        }

        info!("Search parameter registry reloaded successfully");
        Ok(())
    }

    /// Update search options (count limits).
    ///
    /// Updates the default and max count settings without reloading the registry.
    /// Uses atomic swap to update configuration without blocking readers.
    pub async fn update_options(&self, new_options: SearchOptions) {
        info!(
            default_count = new_options.default_count,
            max_count = new_options.max_count,
            "Updating search options"
        );

        // Update options
        {
            let mut options = self.options.write().await;
            *options = new_options.clone();
        }

        // Get current config and create new one with updated options
        let current = self.inner.load_full();
        let new_config = SearchConfig {
            default_count: new_options.default_count,
            max_count: new_options.max_count,
            registry: current.registry.clone(),
            cache: self.cache.clone(),
        };

        // Atomic swap
        self.inner.store(Arc::new(new_config));
    }

    /// Get current options.
    pub async fn options(&self) -> SearchOptions {
        self.options.read().await.clone()
    }

    /// Get cache statistics if caching is enabled.
    pub fn cache_stats(&self) -> Option<crate::query_cache::CacheStatsSnapshot> {
        self.cache.as_ref().map(|c| c.stats())
    }

    /// Clear the query cache.
    pub fn clear_cache(&self) {
        if let Some(cache) = &self.cache {
            cache.clear();
            debug!("Query cache cleared");
        }
    }
}

impl std::fmt::Debug for ReloadableSearchConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReloadableSearchConfig")
            .field("cache_enabled", &self.cache.is_some())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_search_options_default() {
        let options = SearchOptions::default();
        assert_eq!(options.default_count, 10);
        assert_eq!(options.max_count, 100);
        assert_eq!(options.cache_capacity, 1000);
    }

    #[tokio::test]
    async fn test_reloadable_config_with_registry() {
        let registry = Arc::new(SearchParameterRegistry::new());
        let options = SearchOptions {
            default_count: 20,
            max_count: 200,
            cache_capacity: 500,
        };

        let config = ReloadableSearchConfig::with_registry(registry, options);

        let snapshot = config.config();
        assert_eq!(snapshot.default_count, 20);
        assert_eq!(snapshot.max_count, 200);
    }

    #[tokio::test]
    async fn test_update_options() {
        let registry = Arc::new(SearchParameterRegistry::new());
        let options = SearchOptions::default();

        let config = ReloadableSearchConfig::with_registry(registry, options);

        // Update options
        config
            .update_options(SearchOptions {
                default_count: 50,
                max_count: 500,
                cache_capacity: 1000,
            })
            .await;

        let snapshot = config.config();
        assert_eq!(snapshot.default_count, 50);
        assert_eq!(snapshot.max_count, 500);
    }

    #[tokio::test]
    async fn test_cache_operations() {
        let registry = Arc::new(SearchParameterRegistry::new());
        let options = SearchOptions {
            cache_capacity: 100,
            ..Default::default()
        };

        let config = ReloadableSearchConfig::with_registry(registry, options);

        // Cache should be enabled
        assert!(config.cache_stats().is_some());

        // Clear should not panic
        config.clear_cache();
    }

    #[tokio::test]
    async fn test_no_cache() {
        let registry = Arc::new(SearchParameterRegistry::new());
        let options = SearchOptions {
            cache_capacity: 0,
            ..Default::default()
        };

        let config = ReloadableSearchConfig::with_registry(registry, options);

        // Cache should be disabled
        assert!(config.cache_stats().is_none());
    }
}
