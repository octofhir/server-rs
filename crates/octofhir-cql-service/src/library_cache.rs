//! Library compilation cache with two-tier architecture
//!
//! Provides in-memory (L1) and optional Redis (L2) caching for compiled CQL libraries
//! to minimize compilation overhead.

use crate::error::CqlResult;
use dashmap::DashMap;
use octofhir_storage::DynStorage;
use serde_json::Value;
use std::sync::Arc;

/// Compiled CQL library representation
#[derive(Debug, Clone)]
pub struct CompiledLibrary {
    pub url: String,
    pub version: String,
    pub elm: Value, // ELM (Expression Logical Model) JSON
    pub cql_source: String,
}

/// Two-tier library cache (in-memory + optional Redis)
pub struct LibraryCache {
    /// L1: In-memory cache (DashMap for concurrent access)
    compiled: Arc<DashMap<String, Arc<CompiledLibrary>>>,

    /// Maximum number of libraries to cache in memory
    capacity: usize,

    /// L2: Optional Redis cache for horizontal scaling
    #[cfg(feature = "redis-cache")]
    redis_pool: Option<Arc<deadpool_redis::Pool>>,
}

impl LibraryCache {
    /// Create a new library cache
    pub fn new(capacity: usize) -> Self {
        Self {
            compiled: Arc::new(DashMap::new()),
            capacity,
            #[cfg(feature = "redis-cache")]
            redis_pool: None,
        }
    }

    /// Create a new library cache with Redis L2 cache
    #[cfg(feature = "redis-cache")]
    pub fn with_redis(capacity: usize, redis_pool: Arc<deadpool_redis::Pool>) -> Self {
        Self {
            compiled: Arc::new(DashMap::new()),
            capacity,
            redis_pool: Some(redis_pool),
        }
    }

    /// Get a library from cache
    pub fn get(&self, url: &str, version: &str) -> Option<Arc<CompiledLibrary>> {
        let cache_key = Self::make_cache_key(url, version);
        self.compiled.get(&cache_key).map(|entry| entry.clone())
    }

    /// Store a library in cache
    pub fn put(&self, library: Arc<CompiledLibrary>) {
        let cache_key = Self::make_cache_key(&library.url, &library.version);

        // Check capacity and evict if needed (simple FIFO for now)
        if self.compiled.len() >= self.capacity {
            if let Some(first_key) = self.compiled.iter().next().map(|e| e.key().clone()) {
                self.compiled.remove(&first_key);
                tracing::debug!(key = first_key, "Evicted library from cache");
            }
        }

        self.compiled.insert(cache_key, library);
    }

    /// Get or compile a library
    pub async fn get_or_compile(
        &self,
        url: &str,
        version: &str,
        storage: &DynStorage,
    ) -> CqlResult<Arc<CompiledLibrary>> {
        let cache_key = Self::make_cache_key(url, version);

        // L1: Check in-memory cache
        if let Some(lib) = self.compiled.get(&cache_key) {
            tracing::debug!(url = url, version = version, "Library found in L1 cache");
            return Ok(lib.clone());
        }

        // L2: Check Redis cache (if enabled)
        #[cfg(feature = "redis-cache")]
        if let Some(pool) = &self.redis_pool {
            if let Ok(Some(lib)) = self.get_from_redis(&cache_key, pool).await {
                tracing::debug!(url = url, version = version, "Library found in L2 cache");
                let lib_arc = Arc::new(lib);
                self.compiled.insert(cache_key.clone(), lib_arc.clone());
                return Ok(lib_arc);
            }
        }

        // Compile from source
        tracing::debug!(
            url = url,
            version = version,
            "Compiling library from source"
        );
        let library = self.compile_from_storage(url, version, storage).await?;
        let library_arc = Arc::new(library);

        // Store in caches
        self.put(library_arc.clone());

        #[cfg(feature = "redis-cache")]
        if let Some(pool) = &self.redis_pool {
            let _ = self.store_in_redis(&cache_key, &library_arc, pool).await;
        }

        Ok(library_arc)
    }

    /// Compile a library from storage
    async fn compile_from_storage(
        &self,
        url: &str,
        version: &str,
        storage: &DynStorage,
    ) -> CqlResult<CompiledLibrary> {
        // 1. Search for Library resource by URL and version
        let library_resource = self.fetch_library_resource(url, version, storage).await?;

        // 2. Extract CQL source from content field
        let cql_source = Self::extract_cql_content(&library_resource)?;

        // 3. Parse and compile to ELM (placeholder for now - needs octofhir-cql integration)
        // TODO: Use octofhir-cql::parse() and generate ELM
        let elm = serde_json::json!({
            "library": {
                "identifier": {
                    "id": url,
                    "version": version
                },
                "statements": {
                    "def": []
                }
            }
        });

        Ok(CompiledLibrary {
            url: url.to_string(),
            version: version.to_string(),
            elm,
            cql_source,
        })
    }

    /// Fetch Library resource from storage
    async fn fetch_library_resource(
        &self,
        url: &str,
        version: &str,
        storage: &DynStorage,
    ) -> CqlResult<Value> {
        use octofhir_storage::SearchParams;

        // Search for Library by canonical URL and version
        let mut search_params = SearchParams::new();
        search_params = search_params.with_param("url", url);
        if version != "latest" {
            search_params = search_params.with_param("version", version);
        }
        search_params = search_params.with_count(1);

        let result = storage
            .search("Library", &search_params)
            .await
            .map_err(|e| {
                crate::error::CqlError::LibraryNotFound(format!(
                    "Failed to search for Library {}: {}",
                    url, e
                ))
            })?;

        if result.entries.is_empty() {
            return Err(crate::error::CqlError::LibraryNotFound(format!(
                "Library not found: {} version {}",
                url, version
            )));
        }

        Ok(result.entries[0].resource.clone())
    }

    /// Extract CQL source code from Library resource
    fn extract_cql_content(library_resource: &Value) -> CqlResult<String> {
        // FHIR Library.content[].data contains base64-encoded CQL source
        let content_array = library_resource
            .get("content")
            .and_then(|c| c.as_array())
            .ok_or_else(|| {
                crate::error::CqlError::CompilationError(
                    "Library resource missing content field".to_string(),
                )
            })?;

        // Find CQL content (contentType = text/cql)
        for content in content_array {
            let content_type = content
                .get("contentType")
                .and_then(|ct| ct.as_str())
                .unwrap_or("");

            if content_type == "text/cql" || content_type == "text/cql-expression" {
                let data_base64 =
                    content
                        .get("data")
                        .and_then(|d| d.as_str())
                        .ok_or_else(|| {
                            crate::error::CqlError::CompilationError(
                                "CQL content missing data field".to_string(),
                            )
                        })?;

                // Decode base64
                use base64::Engine;
                let data_bytes = base64::engine::general_purpose::STANDARD
                    .decode(data_base64)
                    .map_err(|e| {
                        crate::error::CqlError::CompilationError(format!(
                            "Failed to decode base64 CQL content: {}",
                            e
                        ))
                    })?;

                let cql_source = String::from_utf8(data_bytes).map_err(|e| {
                    crate::error::CqlError::CompilationError(format!(
                        "Failed to decode UTF-8 CQL content: {}",
                        e
                    ))
                })?;

                return Ok(cql_source);
            }
        }

        Err(crate::error::CqlError::CompilationError(
            "No CQL content found in Library resource".to_string(),
        ))
    }

    /// Make cache key from URL and version
    fn make_cache_key(url: &str, version: &str) -> String {
        format!("{}|{}", url, version)
    }

    /// Get library from Redis (L2 cache)
    #[cfg(feature = "redis-cache")]
    async fn get_from_redis(
        &self,
        key: &str,
        pool: &deadpool_redis::Pool,
    ) -> CqlResult<Option<CompiledLibrary>> {
        use redis::AsyncCommands;

        let mut conn = pool.get().await.map_err(|e| {
            crate::error::CqlError::CacheError(format!("Redis connection error: {}", e))
        })?;

        let data: Option<String> = conn
            .get(format!("cql:library:{}", key))
            .await
            .map_err(|e| crate::error::CqlError::CacheError(format!("Redis get error: {}", e)))?;

        match data {
            Some(json) => {
                let library: CompiledLibrary = serde_json::from_str(&json)?;
                Ok(Some(library))
            }
            None => Ok(None),
        }
    }

    /// Store library in Redis (L2 cache)
    #[cfg(feature = "redis-cache")]
    async fn store_in_redis(
        &self,
        key: &str,
        library: &CompiledLibrary,
        pool: &deadpool_redis::Pool,
    ) -> CqlResult<()> {
        use redis::AsyncCommands;

        let json = serde_json::to_string(library)?;
        let mut conn = pool.get().await.map_err(|e| {
            crate::error::CqlError::CacheError(format!("Redis connection error: {}", e))
        })?;

        conn.set_ex(format!("cql:library:{}", key), json, 3600) // 1 hour TTL
            .await
            .map_err(|e| crate::error::CqlError::CacheError(format!("Redis set error: {}", e)))?;

        Ok(())
    }

    /// Clear all cached libraries
    pub fn clear(&self) {
        self.compiled.clear();
        tracing::info!("Cleared library cache");
    }

    /// Get cache statistics
    pub fn stats(&self) -> CacheStats {
        CacheStats {
            size: self.compiled.len(),
            capacity: self.capacity,
        }
    }
}

/// Cache statistics
#[derive(Debug, Clone)]
pub struct CacheStats {
    pub size: usize,
    pub capacity: usize,
}

// Implement Serialize/Deserialize for CompiledLibrary for Redis serialization
#[cfg(feature = "redis-cache")]
impl serde::Serialize for CompiledLibrary {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("CompiledLibrary", 4)?;
        state.serialize_field("url", &self.url)?;
        state.serialize_field("version", &self.version)?;
        state.serialize_field("elm", &self.elm)?;
        state.serialize_field("cql_source", &self.cql_source)?;
        state.end()
    }
}

#[cfg(feature = "redis-cache")]
impl<'de> serde::Deserialize<'de> for CompiledLibrary {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(serde::Deserialize)]
        struct CompiledLibraryHelper {
            url: String,
            version: String,
            elm: Value,
            cql_source: String,
        }

        let helper = CompiledLibraryHelper::deserialize(deserializer)?;
        Ok(CompiledLibrary {
            url: helper.url,
            version: helper.version,
            elm: helper.elm,
            cql_source: helper.cql_source,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_make_cache_key() {
        let key = LibraryCache::make_cache_key("http://example.org/Library/test", "1.0.0");
        assert_eq!(key, "http://example.org/Library/test|1.0.0");
    }

    #[test]
    fn test_extract_cql_content() {
        use base64::Engine;
        let library = serde_json::json!({
            "resourceType": "Library",
            "url": "http://example.org/Library/test",
            "version": "1.0.0",
            "content": [{
                "contentType": "text/cql",
                "data": base64::engine::general_purpose::STANDARD
                    .encode("library TestLibrary version '1.0.0'")
            }]
        });

        let result = LibraryCache::extract_cql_content(&library).unwrap();
        assert_eq!(result, "library TestLibrary version '1.0.0'");
    }

    #[test]
    fn test_extract_cql_content_missing() {
        let library = serde_json::json!({
            "resourceType": "Library",
            "url": "http://example.org/Library/test"
        });

        let result = LibraryCache::extract_cql_content(&library);
        assert!(result.is_err());
    }
}
