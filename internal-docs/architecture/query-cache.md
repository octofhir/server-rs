# Query Cache Architecture

## Overview

The query cache provides performance optimization by caching SQL query templates and avoiding repeated query planning for similar searches.

## Components

### QueryCache (`octofhir-search/src/query_cache.rs`)

```rust
pub struct QueryCache {
    cache: DashMap<u64, CacheEntry>,
    capacity: usize,
    max_age_secs: u64,
    enabled: bool,
    stats: Arc<CacheStatistics>,
}
```

### Cache Key (`QueryCacheKey`)

Cache keys are based on query structure, not values:

```rust
pub struct QueryCacheKey {
    resource_type: String,
    parameters: Vec<QueryParamKey>,  // Param name + modifier + type
    has_pagination: bool,
    sort_fields: Vec<String>,
}
```

This allows different searches with the same structure to reuse cached plans.

### Cache Entry

```rust
struct CacheEntry {
    query: PreparedQuery,
    created_at: Instant,
    last_accessed: AtomicU64,
    access_count: AtomicU64,
}

pub struct PreparedQuery {
    sql_template: String,
    param_order: Vec<String>,
}
```

## Cache Operations

### Lookup Flow

```
1. Parse search parameters
2. Generate QueryCacheKey from parameters
3. Hash key to u64
4. Look up in DashMap
5. If found and not expired:
   - Update last_accessed
   - Increment hit counter
   - Return PreparedQuery
6. If not found:
   - Increment miss counter
   - Build SQL query
   - Store in cache (with LRU eviction if full)
```

### LRU Eviction

When cache reaches capacity:

1. Find entry with oldest `last_accessed`
2. Remove that entry
3. Insert new entry

### Cache Statistics

```rust
pub struct CacheStatistics {
    hits: AtomicU64,
    misses: AtomicU64,
    evictions: AtomicU64,
}
```

## Integration

### SearchEngine Integration

```rust
// In engine.rs
pub async fn execute_with_cache_info(
    storage: &dyn Storage,
    resource_type: &str,
    query: &str,
    config: &SearchConfig,
    count_only: bool,
) -> Result<(SearchResult, bool), SearchError> {
    let cache_key = build_cache_key(resource_type, &parsed_params, config);

    if let Some(prepared) = config.query_cache().get(&cache_key) {
        // Cache hit - bind values to prepared query
        let sql = bind_values(&prepared, &parsed_params);
        let result = execute_sql(sql).await?;
        return Ok((result, true));
    }

    // Cache miss - build and cache
    let sql = build_sql(resource_type, &parsed_params);
    let prepared = PreparedQuery::from_sql(&sql, &parsed_params);
    config.query_cache().insert(cache_key, prepared);

    let result = execute_sql(sql).await?;
    Ok((result, false))
}
```

## Configuration

```toml
[search]
cache_enabled = true
cache_capacity = 1000
cache_max_age_secs = 3600
```

## Performance Characteristics

| Operation | Complexity | Notes |
|-----------|------------|-------|
| Lookup | O(1) | DashMap provides lock-free reads |
| Insert | O(1) amortized | May trigger eviction |
| Eviction | O(n) | Full scan for LRU, but rare |

## Thread Safety

- `DashMap` provides concurrent read/write access
- `AtomicU64` for counters avoids locks
- No global locks during normal operation

## Memory Usage

Estimated per entry:
- Key hash: 8 bytes
- SQL template: ~500-2000 bytes
- Param order: ~100-500 bytes
- Metadata: ~32 bytes

For 1000 entries: ~1-3 MB

## Monitoring

### Metrics Exposed

```
octofhir_query_cache_hits_total
octofhir_query_cache_misses_total
octofhir_query_cache_evictions_total
octofhir_query_cache_size
octofhir_query_cache_hit_ratio
```

### Health Indicators

- Hit ratio < 50%: Consider increasing capacity
- Frequent evictions: Consider larger cache
- Memory usage high: Consider shorter TTL

## Testing

Unit tests in `query_cache.rs`:
- `test_cache_hit` - Verifies cache lookup
- `test_cache_miss` - Verifies miss handling
- `test_lru_eviction` - Verifies eviction behavior
- `test_ttl_expiration` - Verifies TTL enforcement
- `test_concurrent_access` - Thread safety test
