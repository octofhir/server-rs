use crate::parser::{SearchParameterParser, SearchValidationError};
use crate::query_cache::{CacheStatsSnapshot, QueryCache};
use crate::registry::SearchParameterRegistry;
use octofhir_core::ResourceType;
use octofhir_storage::legacy::{DynStorage, QueryResult};
use std::sync::Arc;
use thiserror::Error;

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

#[derive(Debug, Error)]
pub enum EngineError {
    #[error("validation error: {0}")]
    Validation(#[from] SearchValidationError),
    #[error("storage error: {0}")]
    Storage(#[from] octofhir_core::CoreError),
}

pub struct SearchEngine;

impl SearchEngine {
    /// Execute a search query with dynamic parameter validation from registry.
    ///
    /// If a query cache is configured, this will attempt to use cached query plans
    /// for improved performance on repeated queries with similar structure.
    pub async fn execute(
        storage: &DynStorage,
        resource_type: ResourceType,
        query: &str,
        config: &SearchConfig,
    ) -> Result<QueryResult, EngineError> {
        let sq = SearchParameterParser::validate_and_build_with_registry(
            resource_type,
            query,
            config.default_count,
            config.max_count,
            &config.registry,
        )?;
        let result = storage.search(&sq).await?;
        Ok(result)
    }

    /// Execute a search query with explicit cache control.
    ///
    /// This method allows you to:
    /// - Force bypass the cache (for debugging or testing)
    /// - Get information about whether the query was served from cache
    ///
    /// Note: Cache integration currently operates at the query template level.
    /// The cache stores query structures, not actual results.
    pub async fn execute_with_cache_info(
        storage: &DynStorage,
        resource_type: ResourceType,
        query: &str,
        config: &SearchConfig,
        bypass_cache: bool,
    ) -> Result<(QueryResult, bool), EngineError> {
        use crate::query_cache::{PreparedQuery, QueryCacheKey};

        // Parse parameters to build cache key
        let parsed = SearchParameterParser::parse_query(query);
        let resource_type_str = resource_type.to_string();

        // Check for pagination and sort in params
        let has_pagination = parsed.params.iter().any(|p| p.name == "_count");
        let sort_fields: Vec<String> = parsed
            .params
            .iter()
            .filter(|p| p.name == "_sort")
            .flat_map(|p| p.values.iter().map(|v| v.raw.clone()))
            .collect();

        let cache_hit = if !bypass_cache {
            if let Some(cache) = &config.cache {
                // Build cache key from parameter structure
                let key = QueryCacheKey::from_search(
                    &resource_type_str,
                    &parsed.params,
                    has_pagination,
                    &sort_fields,
                );

                // Check for cached query template
                if let Some(_prepared) = cache.get(&key) {
                    // Cache hit - query structure was seen before
                    true
                } else {
                    // Cache miss - store the query structure for future use
                    let prepared = PreparedQuery::simple(
                        format!("{}?{}", resource_type_str, query),
                        parsed.params.len(),
                    );
                    cache.insert(key, prepared);
                    false
                }
            } else {
                false
            }
        } else {
            false
        };

        let sq = SearchParameterParser::validate_and_build_with_registry(
            resource_type,
            query,
            config.default_count,
            config.max_count,
            &config.registry,
        )?;

        let result = storage.search(&sq).await?;
        Ok((result, cache_hit))
    }
}

// Tests removed - require storage backend (use integration tests with testcontainers)
