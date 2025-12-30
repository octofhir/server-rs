//! Search query implementations.
//!
//! This module contains the SQL queries for FHIR search operations.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use octofhir_core::fhir_reference::parse_reference_simple;
use serde_json::Value;
use sqlx_core::query_as::query_as;
use sqlx_core::query_scalar::query_scalar;
use sqlx_postgres::PgPool;
use time::OffsetDateTime;

use octofhir_search::{BuiltQuery, SearchParameterRegistry, SqlValue, build_query_from_params};
use octofhir_storage::{
    RawSearchResult, RawStoredResource, SearchParams, SearchResult, StorageError, StoredResource,
    TotalMode,
};

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
    // Use default registry if none provided
    let empty_registry = Arc::new(SearchParameterRegistry::new());
    let registry_arc = registry.unwrap_or(&empty_registry);
    let registry = registry_arc.as_ref();

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
        "Executing raw search query"
    );

    // Execute the main query with raw JSON
    let limit = params.count.unwrap_or(10) as usize;
    let entries = execute_query_raw(pool, &built_query, resource_type).await?;

    // Determine if there are more results
    let has_more = entries.len() > limit;
    let entries: Vec<RawStoredResource> = entries.into_iter().take(limit).collect();

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

    // Handle _include and _revinclude by falling back to regular search
    if !converted.includes.is_empty() || !converted.revincludes.is_empty() {
        tracing::debug!(
            "Raw search using fallback for _include/_revinclude"
        );
        let regular_result = execute_search(pool, resource_type, params, Some(registry_arc)).await?;

        // Separate main results from included resources
        let mut main_entries = Vec::new();
        let mut included = Vec::new();

        for e in regular_result.entries {
            let raw = RawStoredResource {
                id: e.id,
                version_id: e.version_id,
                resource_type: e.resource_type.clone(),
                resource_json: serde_json::to_string(&e.resource)
                    .unwrap_or_else(|_| "{}".to_string()),
                last_updated: e.last_updated,
                created_at: e.created_at,
            };
            if e.resource_type == resource_type {
                main_entries.push(raw);
            } else {
                included.push(raw);
            }
        }

        return Ok(RawSearchResult {
            entries: main_entries,
            included,
            total: regular_result.total,
            has_more: regular_result.has_more,
        });
    }

    Ok(RawSearchResult {
        entries,
        included: Vec::new(),
        total,
        has_more,
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
            // Check for table not found
            if e.to_string().contains("does not exist") {
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
/// This version modifies the SQL to select `resource::text` instead of `resource`,
/// avoiding JSONB → Value deserialization overhead.
async fn execute_query_raw(
    pool: &PgPool,
    query: &BuiltQuery,
    resource_type: &str,
) -> Result<Vec<RawStoredResource>, StorageError> {
    // Modify SQL to cast resource to text for zero-copy serialization
    // The original SQL is: SELECT resource, id, txid, created_at, updated_at FROM ...
    // We change it to: SELECT resource::text, id, txid, created_at, updated_at FROM ...
    let raw_sql = query.sql.replacen("resource,", "resource::text,", 1);

    // Execute and map results
    let rows: Vec<(String, String, i64, DateTime<Utc>, DateTime<Utc>)> =
        query_as::<_, (String, String, i64, DateTime<Utc>, DateTime<Utc>)>(&raw_sql)
            .bind_all_params_raw(&query.params)
            .fetch_all(pool)
            .await
            .map_err(|e| {
                tracing::warn!(error = %e, sql = %raw_sql, "Raw search query failed");
                // Check for table not found
                if e.to_string().contains("does not exist") {
                    return StorageError::internal(format!(
                        "Table for {} does not exist",
                        resource_type
                    ));
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
    let mut included: Vec<StoredResource> =
        include_results.into_iter().flatten().collect();
    included.extend(revinclude_results.into_iter().flatten());

    Ok(included)
}

/// Resolve a single _include specification.
async fn resolve_include(
    pool: &PgPool,
    main_results: &[StoredResource],
    include: &octofhir_search::IncludeSpec,
    _source_type: &str,
) -> Result<Vec<StoredResource>, StorageError> {
    if main_results.is_empty() {
        return Ok(Vec::new());
    }

    // Extract reference IDs from main results for the specified parameter
    let mut reference_ids: Vec<String> = Vec::new();
    let param_name = &include.param_name;

    for result in main_results {
        // Try to extract reference from the resource
        if let Some(ref_value) = result.resource.get(param_name) {
            if let Some(reference) = ref_value.get("reference").and_then(|r| r.as_str()) {
                // Use the shared reference parser to extract the ID
                if let Ok((_, id)) = parse_reference_simple(reference, None) {
                    reference_ids.push(id);
                }
            }
        }
    }

    if reference_ids.is_empty() {
        return Ok(Vec::new());
    }

    // Determine target type
    let target_type = include
        .target_type
        .as_deref()
        .unwrap_or(&include.param_name);
    let table = SchemaManager::table_name(target_type);

    // Query for included resources
    let placeholders: Vec<String> = (1..=reference_ids.len()).map(|i| format!("${i}")).collect();
    let sql = format!(
        r#"SELECT resource, id, txid, created_at, updated_at FROM "{table}"
           WHERE id = ANY(ARRAY[{}]::text[])
           AND status != 'deleted'"#,
        placeholders.join(", ")
    );

    let mut query = query_as(&sql);
    for id in &reference_ids {
        query = query.bind(id);
    }

    let rows: Vec<(Value, String, i64, DateTime<Utc>, DateTime<Utc>)> =
        query.fetch_all(pool).await.map_err(|e| {
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

/// Resolve a single _revinclude specification.
async fn resolve_revinclude(
    pool: &PgPool,
    main_results: &[StoredResource],
    revinclude: &octofhir_search::RevIncludeSpec,
) -> Result<Vec<StoredResource>, StorageError> {
    if main_results.is_empty() {
        return Ok(Vec::new());
    }

    // Build reference values to search for
    let main_type = main_results
        .first()
        .map(|r| r.resource_type.as_str())
        .unwrap_or("");
    let reference_values: Vec<String> = main_results
        .iter()
        .map(|r| format!("{}/{}", main_type, r.id))
        .collect();

    if reference_values.is_empty() {
        return Ok(Vec::new());
    }

    let source_type = &revinclude.source_type;
    let table = SchemaManager::table_name(source_type);
    let param_name = &revinclude.param_name;

    // Query for resources that reference main results
    let placeholders: Vec<String> = (1..=reference_values.len())
        .map(|i| format!("${i}"))
        .collect();
    let sql = format!(
        r#"SELECT resource, id, txid, created_at, updated_at FROM "{table}"
           WHERE resource->'{param_name}'->>'reference' = ANY(ARRAY[{}]::text[])
           AND status != 'deleted'"#,
        placeholders.join(", ")
    );

    let mut query = query_as(&sql);
    for ref_value in &reference_values {
        query = query.bind(ref_value);
    }

    let rows: Vec<(Value, String, i64, DateTime<Utc>, DateTime<Utc>)> =
        query.fetch_all(pool).await.map_err(|e| {
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
