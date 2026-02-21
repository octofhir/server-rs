//! Search query implementations.
//!
//! This module contains the SQL queries for FHIR search operations.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx_core::query_as::query_as;
use sqlx_core::query_scalar::query_scalar;
use sqlx_postgres::PgPool;
use time::OffsetDateTime;

use octofhir_search::{
    BuiltQuery, ParamsSearchConfig, PreparedQuery, QueryCache, QueryCacheKey, QueryParamKey,
    SearchParameterRegistry, SqlValue, UnknownParamHandling, build_query_from_params,
    build_query_from_params_with_config,
};
use octofhir_storage::{
    RawSearchResult, RawStoredResource, SearchParams, SearchResult, StorageError, StoredResource,
    TotalMode,
};

/// Re-export UnknownParamHandling for convenience.
pub use octofhir_search::UnknownParamHandling as SearchUnknownParamHandling;

use crate::error::is_undefined_table;
use crate::schema::SchemaManager;

/// Converts chrono DateTime to time OffsetDateTime.
fn chrono_to_time(dt: DateTime<Utc>) -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp(dt.timestamp()).unwrap_or(OffsetDateTime::UNIX_EPOCH)
        + time::Duration::nanoseconds(dt.timestamp_subsec_nanos() as i64)
}

/// Execute a FHIR search query and return results.
///
/// # Arguments
///
/// * `pool` - Database connection pool
/// * `resource_type` - The FHIR resource type to search
/// * `params` - Search parameters from the API
/// * `registry` - Optional search parameter registry for parameter validation
///
/// # Returns
///
/// Returns a `SearchResult` with matching resources, optional total count, and has_more flag.
pub async fn execute_search(
    pool: &PgPool,
    resource_type: &str,
    params: &SearchParams,
    registry: Option<&Arc<SearchParameterRegistry>>,
) -> Result<SearchResult, StorageError> {
    // Use default registry if none provided
    let empty_registry = SearchParameterRegistry::new();
    let registry = registry.map(|r| r.as_ref()).unwrap_or(&empty_registry);

    // Convert SearchParams to SQL query using the params converter
    let converted =
        build_query_from_params(resource_type, params, registry, "public").map_err(|e| {
            tracing::warn!(error = %e, "Failed to build search query");
            StorageError::invalid_resource(format!("Invalid search parameters: {e}"))
        })?;

    // Build the SQL query
    let built_query = converted.builder.build().map_err(|e| {
        tracing::warn!(error = %e, "Failed to build SQL");
        StorageError::internal(format!("Failed to build search SQL: {e}"))
    })?;

    tracing::debug!(
        resource_type = %resource_type,
        sql = %built_query.sql,
        params_count = built_query.params.len(),
        "Executing search query"
    );

    // Execute the main query
    let limit = params.count.unwrap_or(10) as usize;
    let entries = execute_query(pool, &built_query, resource_type).await?;

    // Determine if there are more results
    let has_more = entries.len() > limit;
    let entries: Vec<StoredResource> = entries.into_iter().take(limit).collect();

    // Execute count query if requested
    let total = if matches!(converted.total_mode, Some(TotalMode::Accurate)) {
        let count_query = converted.builder.build_count().map_err(|e| {
            tracing::warn!(error = %e, "Failed to build count SQL");
            StorageError::internal(format!("Failed to build count SQL: {e}"))
        })?;

        Some(execute_count_query(pool, &count_query).await?)
    } else {
        None
    };

    // Handle _include and _revinclude
    let mut all_entries = entries;

    if !converted.includes.is_empty() || !converted.revincludes.is_empty() {
        let included = resolve_includes_revincludes(
            pool,
            &all_entries,
            &converted.includes,
            &converted.revincludes,
        )
        .await?;
        all_entries.extend(included);
    }

    Ok(SearchResult {
        entries: all_entries,
        total,
        has_more,
    })
}

/// Execute a FHIR search query and return raw JSON results for optimized serialization.
///
/// This is an optimized version of `execute_search` that returns resources as raw JSON
/// strings, avoiding the overhead of parsing JSONB into `serde_json::Value` when the
/// response will just serialize the JSON anyway.
///
/// **Performance benefit**: ~25-30% faster for search responses by skipping full JSON
/// tree construction.
///
/// # Arguments
///
/// * `pool` - Database connection pool
/// * `resource_type` - The FHIR resource type to search
/// * `params` - Search parameters from the API
/// * `registry` - Optional search parameter registry for parameter validation
///
/// # Returns
///
/// Returns a `RawSearchResult` with matching resources as raw JSON strings.
pub async fn execute_search_raw(
    pool: &PgPool,
    resource_type: &str,
    params: &SearchParams,
    registry: Option<&Arc<SearchParameterRegistry>>,
) -> Result<RawSearchResult, StorageError> {
    execute_search_raw_with_config(pool, resource_type, params, registry, None, None).await
}

/// Execute a FHIR search query with configurable unknown parameter handling.
///
/// This version allows specifying how to handle unknown search parameters:
/// - `Strict`: Return 400 error for unknown parameters
/// - `Lenient` (default): Ignore unknown parameters and continue
pub async fn execute_search_raw_with_config(
    pool: &PgPool,
    resource_type: &str,
    params: &SearchParams,
    registry: Option<&Arc<SearchParameterRegistry>>,
    unknown_param_handling: Option<UnknownParamHandling>,
    query_cache: Option<&QueryCache>,
) -> Result<RawSearchResult, StorageError> {
    // Use default registry if none provided
    let empty_registry = Arc::new(SearchParameterRegistry::new());
    let registry_arc = registry.unwrap_or(&empty_registry);
    let registry = registry_arc.as_ref();

    // Build search config
    let search_config = ParamsSearchConfig {
        unknown_param_handling: unknown_param_handling.unwrap_or_default(),
    };

    // Convert SearchParams to SQL query using the params converter
    let converted = build_query_from_params_with_config(
        resource_type,
        params,
        registry,
        "public",
        &search_config,
    )
    .map_err(|e| {
        tracing::warn!(error = %e, "Failed to build search query");
        StorageError::invalid_resource(format!("Invalid search parameters: {e}"))
    })?;

    // Collect unknown parameters as warnings
    let warnings: Vec<String> = converted
        .unknown_params
        .iter()
        .map(|p| format!("Unknown search parameter '{}' was ignored", p.name))
        .collect();

    if !warnings.is_empty() {
        tracing::info!(
            unknown_params = ?converted.unknown_params.iter().map(|p| &p.name).collect::<Vec<_>>(),
            "Search ignored unknown parameters (lenient mode)"
        );
    }

    // Build cache key for query template reuse
    let cache_key = query_cache.map(|_| {
        let param_keys: Vec<QueryParamKey> = params
            .parameters
            .iter()
            .map(|(key, values)| {
                let (name, modifier) = key
                    .split_once(':')
                    .map(|(n, m)| (n.to_string(), Some(m.to_string())))
                    .unwrap_or_else(|| (key.clone(), None));

                // SQL shape for token-like params can depend on raw value format
                // (e.g. identifier=value vs identifier=system|value). Encode this
                // in the cache key to avoid binding a text param to a cached JSON
                // template (or vice versa).
                let token_shape = {
                    let has_pipe = values.iter().any(|v| v.contains('|'));
                    let has_no_pipe = values.iter().any(|v| !v.contains('|'));
                    let has_system = values.iter().any(|v| {
                        v.split_once('|')
                            .map(|(left, _)| !left.is_empty())
                            .unwrap_or(false)
                    });
                    let has_empty_system = values.iter().any(|v| {
                        v.split_once('|')
                            .map(|(left, _)| left.is_empty())
                            .unwrap_or(false)
                    });

                    if has_pipe && has_no_pipe {
                        "token-mixed"
                    } else if has_pipe && has_system && !has_empty_system {
                        "token-system"
                    } else if has_pipe && has_empty_system && !has_system {
                        "token-nosystem"
                    } else if has_pipe {
                        "token-pipe-mixed"
                    } else {
                        "token-plain"
                    }
                };

                let cache_name = format!("{name}#{token_shape}");

                QueryParamKey {
                    name: cache_name,
                    modifier,
                    param_type: QueryCacheKey::infer_param_type(key),
                    value_count: values.len(),
                }
            })
            .collect();
        let sort_fields: Vec<String> = params
            .sort
            .as_ref()
            .map(|s| s.iter().map(|f| f.field.clone()).collect())
            .unwrap_or_default();
        QueryCacheKey::from_typed_params(
            resource_type,
            param_keys,
            params.count.is_some() || params.offset.is_some(),
            sort_fields,
        )
    });

    // Try cache hit for main query SQL template
    let cached_main = cache_key
        .as_ref()
        .and_then(|key| query_cache.and_then(|c| c.get(key)));

    // Build count query first (before consuming builder with with_raw_resource)
    let count_query = if matches!(converted.total_mode, Some(TotalMode::Accurate)) {
        Some(converted.builder.build_count().map_err(|e| {
            tracing::warn!(error = %e, "Failed to build count SQL");
            StorageError::internal(format!("Failed to build count SQL: {e}"))
        })?)
    } else {
        None
    };

    // Build or reuse the main SQL query
    let built_query = if let Some(cached) = cached_main {
        // Cache hit: reuse SQL template, extract fresh params from builder
        let fresh_params = converted.builder.extract_params();
        match cached.bind(fresh_params) {
            Ok(bq) => bq,
            Err(_) => {
                // Param count mismatch — fall back to full build
                converted
                    .builder
                    .with_raw_resource(true)
                    .build()
                    .map_err(|e| {
                        StorageError::internal(format!("Failed to build search SQL: {e}"))
                    })?
            }
        }
    } else {
        // Cache miss: build SQL and cache the template
        let bq = converted
            .builder
            .with_raw_resource(true)
            .build()
            .map_err(|e| {
                tracing::warn!(error = %e, "Failed to build SQL");
                StorageError::internal(format!("Failed to build search SQL: {e}"))
            })?;

        if let (Some(cache), Some(key)) = (query_cache, cache_key) {
            cache.insert(key, PreparedQuery::simple(bq.sql.clone(), bq.params.len()));
        }

        bq
    };

    tracing::debug!(
        resource_type = %resource_type,
        sql = %built_query.sql,
        params_count = built_query.params.len(),
        "Executing raw search query"
    );

    // Execute the main query with raw JSON (SQL already emits resource::text)
    let limit = params.count.unwrap_or(10) as usize;
    let entries = execute_query_raw(pool, &built_query, resource_type).await?;

    // Determine if there are more results
    let has_more = entries.len() > limit;
    let entries: Vec<RawStoredResource> = entries.into_iter().take(limit).collect();

    // Execute count query if requested
    let total = if let Some(cq) = count_query {
        Some(execute_count_query(pool, &cq).await?)
    } else {
        None
    };

    // Handle _include and _revinclude natively with raw results
    let included = if !converted.includes.is_empty() || !converted.revincludes.is_empty() {
        resolve_includes_revincludes_raw(
            pool,
            resource_type,
            &entries,
            &converted.includes,
            &converted.revincludes,
        )
        .await?
    } else {
        Vec::new()
    };

    Ok(RawSearchResult {
        entries,
        included,
        total,
        has_more,
        warnings,
    })
}

/// Execute a search query and return StoredResource entries.
async fn execute_query(
    pool: &PgPool,
    query: &BuiltQuery,
    resource_type: &str,
) -> Result<Vec<StoredResource>, StorageError> {
    // Build dynamic query with parameters
    // The query returns: resource, id, txid, created_at, updated_at
    let mut sqlx_query = sqlx_core::query::query::<sqlx_postgres::Postgres>(&query.sql);

    // Bind parameters
    for param in &query.params {
        sqlx_query = match param {
            SqlValue::Text(s) => sqlx_query.bind(s.clone()),
            SqlValue::Integer(i) => sqlx_query.bind(*i),
            SqlValue::Float(f) => sqlx_query.bind(*f),
            SqlValue::Boolean(b) => sqlx_query.bind(*b),
            SqlValue::Json(s) => sqlx_query.bind(s.clone()),
            SqlValue::Timestamp(s) => sqlx_query.bind(s.clone()),
            SqlValue::Null => sqlx_query.bind(None::<String>),
        };
    }

    // Execute and map results
    let rows: Vec<(Value, String, i64, DateTime<Utc>, DateTime<Utc>)> = query_as(&query.sql)
        .bind_all_params(&query.params)
        .fetch_all(pool)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, sql = %query.sql, "Search query failed");
            // Check for undefined table error (PostgreSQL 42P01)
            if is_undefined_table(&e) {
                return StorageError::internal(format!(
                    "Table for {} does not exist",
                    resource_type
                ));
            }
            StorageError::internal(format!("Search query failed: {e}"))
        })?;

    let entries: Vec<StoredResource> = rows
        .into_iter()
        .map(|(resource, id, txid, created_at, updated_at)| {
            let created_at_time = chrono_to_time(created_at);
            let updated_at_time = chrono_to_time(updated_at);
            StoredResource {
                id,
                version_id: txid.to_string(),
                resource_type: resource_type.to_string(),
                resource,
                last_updated: updated_at_time,
                created_at: created_at_time,
            }
        })
        .collect();

    Ok(entries)
}

/// Execute a search query and return raw JSON entries (optimized path).
///
/// Expects SQL that already selects `resource::text` (via `with_raw_resource(true)`
/// on the query builder), avoiding JSONB → Value deserialization overhead.
async fn execute_query_raw(
    pool: &PgPool,
    query: &BuiltQuery,
    resource_type: &str,
) -> Result<Vec<RawStoredResource>, StorageError> {
    // Execute and map results (SQL already selects resource::text)
    let rows: Vec<(String, String, i64, DateTime<Utc>, DateTime<Utc>)> = query_as::<
        _,
        (String, String, i64, DateTime<Utc>, DateTime<Utc>),
    >(&query.sql)
    .bind_all_params_raw(&query.params)
    .fetch_all(pool)
    .await
    .map_err(|e| {
        tracing::warn!(error = %e, sql = %query.sql, "Raw search query failed");
        // Check for undefined table error (PostgreSQL 42P01)
        if is_undefined_table(&e) {
            return StorageError::internal(format!("Table for {} does not exist", resource_type));
        }
        StorageError::internal(format!("Search query failed: {e}"))
    })?;

    let entries: Vec<RawStoredResource> = rows
        .into_iter()
        .map(|(resource_json, id, txid, created_at, updated_at)| {
            let created_at_time = chrono_to_time(created_at);
            let updated_at_time = chrono_to_time(updated_at);
            RawStoredResource {
                id,
                version_id: txid.to_string(),
                resource_type: resource_type.to_string(),
                resource_json,
                last_updated: updated_at_time,
                created_at: created_at_time,
            }
        })
        .collect();

    Ok(entries)
}

/// Execute a count query and return the total.
async fn execute_count_query(pool: &PgPool, query: &BuiltQuery) -> Result<u32, StorageError> {
    let count: i64 = query_scalar(&query.sql)
        .bind_all_params(&query.params)
        .fetch_one(pool)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, "Count query failed");
            StorageError::internal(format!("Count query failed: {e}"))
        })?;

    Ok(count as u32)
}

/// Resolve _include and _revinclude specifications.
///
/// Executes all include and revinclude queries in parallel for better latency.
async fn resolve_includes_revincludes(
    pool: &PgPool,
    main_results: &[StoredResource],
    includes: &[octofhir_search::IncludeSpec],
    revincludes: &[octofhir_search::RevIncludeSpec],
) -> Result<Vec<StoredResource>, StorageError> {
    use futures_util::future::try_join_all;

    // Build futures for all include queries
    let include_futures: Vec<_> = includes
        .iter()
        .map(|include| resolve_include(pool, main_results, include, &include.source_type))
        .collect();

    // Build futures for all revinclude queries
    let revinclude_futures: Vec<_> = revincludes
        .iter()
        .map(|revinclude| resolve_revinclude(pool, main_results, revinclude))
        .collect();

    // Execute all queries in parallel
    let (include_results, revinclude_results) = tokio::try_join!(
        try_join_all(include_futures),
        try_join_all(revinclude_futures)
    )?;

    // Flatten results
    let mut included: Vec<StoredResource> = include_results.into_iter().flatten().collect();
    included.extend(revinclude_results.into_iter().flatten());

    Ok(included)
}

/// Resolve a single _include specification using the search_idx_reference index table.
///
/// Uses B-tree index scan on `search_idx_reference` to find referenced resource IDs
/// instead of parsing JSONB at runtime.
async fn resolve_include(
    pool: &PgPool,
    main_results: &[StoredResource],
    include: &octofhir_search::IncludeSpec,
    _source_type: &str,
) -> Result<Vec<StoredResource>, StorageError> {
    if main_results.is_empty() {
        return Ok(Vec::new());
    }

    let source_type = &include.source_type;
    let param_name = &include.param_name;

    // Collect source IDs
    let source_ids: Vec<&str> = main_results.iter().map(|r| r.id.as_str()).collect();

    let target_types = if let Some(target_type) = include.target_type.as_ref() {
        vec![target_type.clone()]
    } else {
        query_include_target_types(pool, source_type, param_name, &source_ids).await?
    };

    let mut entries = Vec::new();
    for target_type in target_types {
        let mut matched =
            query_include_for_target(pool, source_type, param_name, &source_ids, &target_type)
                .await?;
        entries.append(&mut matched);
    }

    Ok(entries)
}

async fn query_include_target_types(
    pool: &PgPool,
    source_type: &str,
    param_name: &str,
    source_ids: &[&str],
) -> Result<Vec<String>, StorageError> {
    let rows: Vec<Option<String>> = query_scalar(
        r#"SELECT DISTINCT sir.target_type
           FROM search_idx_reference sir
           WHERE sir.resource_type = $1
             AND sir.resource_id = ANY($2::text[])
             AND sir.param_code = $3
             AND sir.ref_kind = 1"#,
    )
    .bind(source_type)
    .bind(source_ids)
    .bind(param_name)
    .fetch_all(pool)
    .await
    .map_err(|e| StorageError::internal(format!("Include target type lookup failed: {e}")))?;

    Ok(rows.into_iter().flatten().collect())
}

async fn query_include_for_target(
    pool: &PgPool,
    source_type: &str,
    param_name: &str,
    source_ids: &[&str],
    target_type: &str,
) -> Result<Vec<StoredResource>, StorageError> {
    let table = SchemaManager::table_name(target_type);

    let sql = format!(
        r#"SELECT DISTINCT t.resource, t.id, t.txid, t.created_at, t.updated_at
           FROM search_idx_reference sir
           JOIN "{table}" t ON t.id = sir.target_id AND t.status != 'deleted'
           WHERE sir.resource_type = $1 AND sir.resource_id = ANY($2::text[])
           AND sir.param_code = $3 AND sir.ref_kind = 1
           AND sir.target_type = $4"#
    );

    let rows: Vec<(Value, String, i64, DateTime<Utc>, DateTime<Utc>)> = query_as(&sql)
        .bind(source_type)
        .bind(&source_ids)
        .bind(param_name)
        .bind(target_type)
        .fetch_all(pool)
        .await
        .map_err(|e| {
            if e.to_string().contains("does not exist") {
                return StorageError::internal(format!(
                    "Include target table {} not found",
                    target_type
                ));
            }
            StorageError::internal(format!("Include query failed: {e}"))
        })?;

    let entries: Vec<StoredResource> = rows
        .into_iter()
        .map(|(resource, id, txid, created_at, updated_at)| {
            let created_at_time = chrono_to_time(created_at);
            let updated_at_time = chrono_to_time(updated_at);
            StoredResource {
                id,
                version_id: txid.to_string(),
                resource_type: target_type.to_string(),
                resource,
                last_updated: updated_at_time,
                created_at: created_at_time,
            }
        })
        .collect();

    Ok(entries)
}

/// Resolve a single _revinclude specification using the search_idx_reference index table.
///
/// Uses B-tree index scan on `search_idx_reference` to find resources that reference
/// the main results, instead of runtime JSONB ->> extraction.
async fn resolve_revinclude(
    pool: &PgPool,
    main_results: &[StoredResource],
    revinclude: &octofhir_search::RevIncludeSpec,
) -> Result<Vec<StoredResource>, StorageError> {
    if main_results.is_empty() {
        return Ok(Vec::new());
    }

    let main_type = main_results
        .first()
        .map(|r| r.resource_type.as_str())
        .unwrap_or("");
    let target_ids: Vec<&str> = main_results.iter().map(|r| r.id.as_str()).collect();

    let source_type = &revinclude.source_type;
    let table = SchemaManager::table_name(source_type);
    let param_name = &revinclude.param_name;

    // Use index table: find source resources that reference any of the target IDs
    let sql = format!(
        r#"SELECT DISTINCT s.resource, s.id, s.txid, s.created_at, s.updated_at
           FROM search_idx_reference sir
           JOIN "{table}" s ON s.id = sir.resource_id AND s.status != 'deleted'
           WHERE sir.resource_type = $1 AND sir.param_code = $2
           AND sir.ref_kind = 1 AND sir.target_type = $3
           AND sir.target_id = ANY($4::text[])"#
    );

    let rows: Vec<(Value, String, i64, DateTime<Utc>, DateTime<Utc>)> = query_as(&sql)
        .bind(source_type)
        .bind(param_name)
        .bind(main_type)
        .bind(&target_ids)
        .fetch_all(pool)
        .await
        .map_err(|e| {
            if e.to_string().contains("does not exist") {
                return StorageError::internal(format!(
                    "RevInclude source table {} not found",
                    source_type
                ));
            }
            StorageError::internal(format!("RevInclude query failed: {e}"))
        })?;

    let entries: Vec<StoredResource> = rows
        .into_iter()
        .map(|(resource, id, txid, created_at, updated_at)| {
            let created_at_time = chrono_to_time(created_at);
            let updated_at_time = chrono_to_time(updated_at);
            StoredResource {
                id,
                version_id: txid.to_string(),
                resource_type: source_type.to_string(),
                resource,
                last_updated: updated_at_time,
                created_at: created_at_time,
            }
        })
        .collect();

    Ok(entries)
}

/// Resolve _include and _revinclude specifications for raw search results.
///
/// Uses `resource::text` to avoid JSONB -> Value -> String round-trip.
async fn resolve_includes_revincludes_raw(
    pool: &PgPool,
    main_resource_type: &str,
    main_results: &[RawStoredResource],
    includes: &[octofhir_search::IncludeSpec],
    revincludes: &[octofhir_search::RevIncludeSpec],
) -> Result<Vec<RawStoredResource>, StorageError> {
    use futures_util::future::try_join_all;

    let include_futures: Vec<_> = includes
        .iter()
        .map(|include| resolve_include_raw(pool, main_results, include))
        .collect();

    let revinclude_futures: Vec<_> = revincludes
        .iter()
        .map(|revinclude| {
            resolve_revinclude_raw(pool, main_resource_type, main_results, revinclude)
        })
        .collect();

    let (include_results, revinclude_results) = tokio::try_join!(
        try_join_all(include_futures),
        try_join_all(revinclude_futures)
    )?;

    let mut included: Vec<RawStoredResource> = include_results.into_iter().flatten().collect();
    included.extend(revinclude_results.into_iter().flatten());

    Ok(included)
}

/// Resolve a single _include specification using raw JSON and the index table.
///
/// Uses B-tree index scan on `search_idx_reference` to find referenced resource IDs
/// instead of parsing JSONB at runtime. Returns resources as raw JSON strings.
async fn resolve_include_raw(
    pool: &PgPool,
    main_results: &[RawStoredResource],
    include: &octofhir_search::IncludeSpec,
) -> Result<Vec<RawStoredResource>, StorageError> {
    if main_results.is_empty() {
        return Ok(Vec::new());
    }

    let source_type = &include.source_type;
    let param_name = &include.param_name;

    // Collect source IDs
    let source_ids: Vec<&str> = main_results.iter().map(|r| r.id.as_str()).collect();

    let target_types = if let Some(target_type) = include.target_type.as_ref() {
        vec![target_type.clone()]
    } else {
        query_include_target_types(pool, source_type, param_name, &source_ids).await?
    };

    let mut entries = Vec::new();
    for target_type in target_types {
        let mut matched =
            query_include_for_target_raw(pool, source_type, param_name, &source_ids, &target_type)
                .await?;
        entries.append(&mut matched);
    }

    Ok(entries)
}

async fn query_include_for_target_raw(
    pool: &PgPool,
    source_type: &str,
    param_name: &str,
    source_ids: &[&str],
    target_type: &str,
) -> Result<Vec<RawStoredResource>, StorageError> {
    let table = SchemaManager::table_name(target_type);

    let sql = format!(
        r#"SELECT DISTINCT t.resource::text, t.id, t.txid, t.created_at, t.updated_at
           FROM search_idx_reference sir
           JOIN "{table}" t ON t.id = sir.target_id AND t.status != 'deleted'
           WHERE sir.resource_type = $1 AND sir.resource_id = ANY($2::text[])
           AND sir.param_code = $3 AND sir.ref_kind = 1
           AND sir.target_type = $4"#
    );

    let rows: Vec<(String, String, i64, DateTime<Utc>, DateTime<Utc>)> = query_as(&sql)
        .bind(source_type)
        .bind(&source_ids)
        .bind(param_name)
        .bind(target_type)
        .fetch_all(pool)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, sql = %sql, "Include query failed");
            if e.to_string().contains("does not exist") {
                return StorageError::internal(format!(
                    "Include target table {} not found",
                    target_type
                ));
            }
            StorageError::internal(format!("Include query failed: {e}"))
        })?;

    let entries: Vec<RawStoredResource> = rows
        .into_iter()
        .map(|(resource_json, id, txid, created_at, updated_at)| {
            let created_at_time = chrono_to_time(created_at);
            let updated_at_time = chrono_to_time(updated_at);
            RawStoredResource {
                id,
                version_id: txid.to_string(),
                resource_type: target_type.to_string(),
                resource_json,
                last_updated: updated_at_time,
                created_at: created_at_time,
            }
        })
        .collect();

    Ok(entries)
}

/// Resolve a single _revinclude specification using raw JSON and the index table.
///
/// Uses B-tree index scan on `search_idx_reference` to find resources that reference
/// the main results, instead of runtime JSONB ->> extraction.
async fn resolve_revinclude_raw(
    pool: &PgPool,
    main_resource_type: &str,
    main_results: &[RawStoredResource],
    revinclude: &octofhir_search::RevIncludeSpec,
) -> Result<Vec<RawStoredResource>, StorageError> {
    if main_results.is_empty() {
        return Ok(Vec::new());
    }

    let target_ids: Vec<&str> = main_results.iter().map(|r| r.id.as_str()).collect();

    let source_type = &revinclude.source_type;
    let table = SchemaManager::table_name(source_type);
    let param_name = &revinclude.param_name;

    // Use index table: find source resources that reference any of the target IDs
    let sql = format!(
        r#"SELECT DISTINCT s.resource::text, s.id, s.txid, s.created_at, s.updated_at
           FROM search_idx_reference sir
           JOIN "{table}" s ON s.id = sir.resource_id AND s.status != 'deleted'
           WHERE sir.resource_type = $1 AND sir.param_code = $2
           AND sir.ref_kind = 1 AND sir.target_type = $3
           AND sir.target_id = ANY($4::text[])"#
    );

    let rows: Vec<(String, String, i64, DateTime<Utc>, DateTime<Utc>)> = query_as(&sql)
        .bind(source_type)
        .bind(param_name)
        .bind(main_resource_type)
        .bind(&target_ids)
        .fetch_all(pool)
        .await
        .map_err(|e| {
            if e.to_string().contains("does not exist") {
                return StorageError::internal(format!(
                    "RevInclude source table {} not found",
                    source_type
                ));
            }
            StorageError::internal(format!("RevInclude query failed: {e}"))
        })?;

    let entries: Vec<RawStoredResource> = rows
        .into_iter()
        .map(|(resource_json, id, txid, created_at, updated_at)| {
            let created_at_time = chrono_to_time(created_at);
            let updated_at_time = chrono_to_time(updated_at);
            RawStoredResource {
                id,
                version_id: txid.to_string(),
                resource_type: source_type.to_string(),
                resource_json,
                last_updated: updated_at_time,
                created_at: created_at_time,
            }
        })
        .collect();

    Ok(entries)
}

/// Helper trait to bind all params to a query.
trait BindAllParams<'q> {
    fn bind_all_params(self, params: &'q [SqlValue]) -> Self;
}

impl<'q> BindAllParams<'q>
    for sqlx_core::query_as::QueryAs<
        'q,
        sqlx_postgres::Postgres,
        (Value, String, i64, DateTime<Utc>, DateTime<Utc>),
        sqlx_postgres::PgArguments,
    >
{
    fn bind_all_params(mut self, params: &'q [SqlValue]) -> Self {
        for param in params {
            self = match param {
                SqlValue::Text(s) => self.bind(s.as_str()),
                SqlValue::Integer(i) => self.bind(*i),
                SqlValue::Float(f) => self.bind(*f),
                SqlValue::Boolean(b) => self.bind(*b),
                SqlValue::Json(s) => self.bind(s.as_str()),
                SqlValue::Timestamp(s) => self.bind(s.as_str()),
                SqlValue::Null => self.bind(None::<String>),
            };
        }
        self
    }
}

impl<'q> BindAllParams<'q>
    for sqlx_core::query_scalar::QueryScalar<
        'q,
        sqlx_postgres::Postgres,
        i64,
        sqlx_postgres::PgArguments,
    >
{
    fn bind_all_params(mut self, params: &'q [SqlValue]) -> Self {
        for param in params {
            self = match param {
                SqlValue::Text(s) => self.bind(s.as_str()),
                SqlValue::Integer(i) => self.bind(*i),
                SqlValue::Float(f) => self.bind(*f),
                SqlValue::Boolean(b) => self.bind(*b),
                SqlValue::Json(s) => self.bind(s.as_str()),
                SqlValue::Timestamp(s) => self.bind(s.as_str()),
                SqlValue::Null => self.bind(None::<String>),
            };
        }
        self
    }
}

/// Helper trait to bind all params to a raw query (resource as text).
trait BindAllParamsRaw<'q> {
    fn bind_all_params_raw(self, params: &'q [SqlValue]) -> Self;
}

impl<'q> BindAllParamsRaw<'q>
    for sqlx_core::query_as::QueryAs<
        'q,
        sqlx_postgres::Postgres,
        (String, String, i64, DateTime<Utc>, DateTime<Utc>),
        sqlx_postgres::PgArguments,
    >
{
    fn bind_all_params_raw(mut self, params: &'q [SqlValue]) -> Self {
        for param in params {
            self = match param {
                SqlValue::Text(s) => self.bind(s.as_str()),
                SqlValue::Integer(i) => self.bind(*i),
                SqlValue::Float(f) => self.bind(*f),
                SqlValue::Boolean(b) => self.bind(*b),
                SqlValue::Json(s) => self.bind(s.as_str()),
                SqlValue::Timestamp(s) => self.bind(s.as_str()),
                SqlValue::Null => self.bind(None::<String>),
            };
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chrono_to_time_conversion() {
        let chrono_dt = Utc::now();
        let time_dt = chrono_to_time(chrono_dt);

        // Verify the conversion is approximately correct (within 1 second)
        let chrono_ts = chrono_dt.timestamp();
        let time_ts = time_dt.unix_timestamp();
        assert!((chrono_ts - time_ts).abs() <= 1);
    }
}
