//! Search query implementations.
//!
//! This module contains the SQL queries for FHIR search operations.

use sqlx_core::sql_str::AssertSqlSafe;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Instant,
};

use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx_core::query_as::query_as;
use sqlx_core::query_scalar::query_scalar;
use sqlx_postgres::{PgPool, PgTransaction};
use time::OffsetDateTime;

use octofhir_fhir_model::terminology::TerminologyProvider;
use octofhir_search::terminology::HybridTerminologyProvider;
use octofhir_search::terminology_preprocess::{
    pre_expand_subsumption_modifiers, pre_expand_terminology_modifiers,
};
use octofhir_search::{
    BuiltQuery, ParamsSearchConfig, PreparedQuery, QueryCache, QueryCacheKey, QueryParamKey,
    SearchParameterRegistry, SqlValue, UnknownParamHandling, build_jsonb_accessor,
    build_native_ir_query_from_params, build_native_ir_query_from_params_with_config,
    fhirpath_to_jsonb_path,
};
use octofhir_storage::{
    RawSearchDebug, RawSearchResult, RawStoredResource, SearchParams, SearchResult, StorageError,
    StoredResource, TotalMode,
};

/// Re-export UnknownParamHandling for convenience.
pub use octofhir_search::UnknownParamHandling as SearchUnknownParamHandling;

use crate::error::is_undefined_table;
use crate::schema::SchemaManager;

const INCLUDE_ITERATE_MAX_DEPTH: usize = 100;

/// Per-request raw search execution options.
#[derive(Debug, Clone, Copy, Default)]
pub struct RawSearchOptions {
    pub unknown_param_handling: Option<UnknownParamHandling>,
    pub collect_debug_plan: bool,
    pub collect_explain_plan: bool,
    pub collect_explain_analyze: bool,
}

/// Converts chrono DateTime to time OffsetDateTime.
fn chrono_to_time(dt: DateTime<Utc>) -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp(dt.timestamp()).unwrap_or(OffsetDateTime::UNIX_EPOCH)
        + time::Duration::nanoseconds(dt.timestamp_subsec_nanos() as i64)
}

/// Redact generated SQL for logs by preserving query shape while removing bind identities.
fn redact_sql_shape(sql: &str) -> String {
    let mut out = String::with_capacity(sql.len().min(4096));
    let mut chars = sql.chars().peekable();
    let mut previous_was_space = false;

    while let Some(ch) = chars.next() {
        if ch == '$' && chars.peek().is_some_and(|next| next.is_ascii_digit()) {
            out.push_str("$?");
            while chars.peek().is_some_and(|next| next.is_ascii_digit()) {
                chars.next();
            }
            previous_was_space = false;
            continue;
        }

        if ch.is_whitespace() {
            if !previous_was_space {
                out.push(' ');
                previous_was_space = true;
            }
        } else {
            out.push(ch);
            previous_was_space = false;
        }
    }

    out.trim().to_string()
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
    let requested_limit = params.count.unwrap_or(10) as usize;
    let mut effective_params = params.clone();
    effective_params.count = Some(params.count.unwrap_or(10).saturating_add(1));

    // Use default registry if none provided
    let empty_registry = SearchParameterRegistry::new();
    let registry = registry.map(|r| r.as_ref()).unwrap_or(&empty_registry);

    // Convert SearchParams to SQL query through the native-IR search path.
    let converted =
        build_native_ir_query_from_params(resource_type, &effective_params, registry, "public")
            .map_err(|e| {
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
        sql_shape = %redact_sql_shape(&built_query.sql),
        params_count = built_query.params.len(),
        "Executing search query"
    );

    // Execute the main query
    let entries = execute_query(pool, &built_query, resource_type).await?;

    // Determine if there are more results
    let has_more = entries.len() > requested_limit;
    let entries: Vec<StoredResource> = entries.into_iter().take(requested_limit).collect();

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
            registry,
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

/// Execute a FHIR search query within an existing PostgreSQL transaction.
///
/// This is used by native Bundle transactions so conditional operations can see
/// resources created or updated earlier in the same transaction.
pub async fn execute_search_with_tx(
    tx: &mut PgTransaction<'_>,
    resource_type: &str,
    params: &SearchParams,
    registry: Option<&Arc<SearchParameterRegistry>>,
) -> Result<SearchResult, StorageError> {
    let requested_limit = params.count.unwrap_or(10) as usize;
    let mut effective_params = params.clone();
    effective_params.count = Some(params.count.unwrap_or(10).saturating_add(1));

    let empty_registry = SearchParameterRegistry::new();
    let registry = registry.map(|r| r.as_ref()).unwrap_or(&empty_registry);

    let converted =
        build_native_ir_query_from_params(resource_type, &effective_params, registry, "public")
            .map_err(|e| {
                tracing::warn!(error = %e, "Failed to build transaction search query");
                StorageError::invalid_resource(format!("Invalid search parameters: {e}"))
            })?;

    let built_query = converted.builder.build().map_err(|e| {
        tracing::warn!(error = %e, "Failed to build transaction search SQL");
        StorageError::internal(format!("Failed to build search SQL: {e}"))
    })?;

    let entries = execute_query_with_tx(tx, &built_query, resource_type).await?;
    let has_more = entries.len() > requested_limit;
    let entries = entries.into_iter().take(requested_limit).collect();

    let total = if matches!(converted.total_mode, Some(TotalMode::Accurate)) {
        let count_query = converted.builder.build_count().map_err(|e| {
            tracing::warn!(error = %e, "Failed to build transaction count SQL");
            StorageError::internal(format!("Failed to build count SQL: {e}"))
        })?;

        Some(execute_count_query_with_tx(tx, &count_query).await?)
    } else {
        None
    };

    Ok(SearchResult {
        entries,
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
    execute_search_raw_with_config_inner(
        pool,
        resource_type,
        params,
        registry,
        query_cache,
        None,
        RawSearchOptions {
            unknown_param_handling,
            collect_debug_plan: false,
            collect_explain_plan: false,
            collect_explain_analyze: false,
        },
    )
    .await
}

/// Execute a FHIR search query with per-request search options.
pub async fn execute_search_raw_with_options(
    pool: &PgPool,
    resource_type: &str,
    params: &SearchParams,
    registry: Option<&Arc<SearchParameterRegistry>>,
    query_cache: Option<&QueryCache>,
    options: RawSearchOptions,
) -> Result<RawSearchResult, StorageError> {
    execute_search_raw_with_config_inner(
        pool,
        resource_type,
        params,
        registry,
        query_cache,
        None,
        options,
    )
    .await
}

/// Like `execute_search_raw_with_config`, but also pre-expands `:in` /
/// `:not-in` / `:above` / `:below` Token modifiers via the terminology
/// provider before building the SQL. See `octofhir_search::terminology_preprocess`
/// for spec details.
///
/// The concrete `HybridTerminologyProvider` is required because hierarchy
/// expansion (`:above` / `:below`) lives outside the `TerminologyProvider`
/// trait. Pass `None` if terminology is not configured; in that case
/// `:in`/`:not-in`/`:above`/`:below` fall through to the sync dispatcher
/// and return `NotImplemented`.
pub async fn execute_search_raw_with_terminology(
    pool: &PgPool,
    resource_type: &str,
    params: &SearchParams,
    registry: Option<&Arc<SearchParameterRegistry>>,
    unknown_param_handling: Option<UnknownParamHandling>,
    query_cache: Option<&QueryCache>,
    terminology: Option<&Arc<HybridTerminologyProvider>>,
) -> Result<RawSearchResult, StorageError> {
    execute_search_raw_with_config_inner(
        pool,
        resource_type,
        params,
        registry,
        query_cache,
        terminology,
        RawSearchOptions {
            unknown_param_handling,
            collect_debug_plan: false,
            collect_explain_plan: false,
            collect_explain_analyze: false,
        },
    )
    .await
}

/// Execute a FHIR search query with terminology expansion and per-request options.
pub async fn execute_search_raw_with_terminology_options(
    pool: &PgPool,
    resource_type: &str,
    params: &SearchParams,
    registry: Option<&Arc<SearchParameterRegistry>>,
    query_cache: Option<&QueryCache>,
    terminology: Option<&Arc<HybridTerminologyProvider>>,
    options: RawSearchOptions,
) -> Result<RawSearchResult, StorageError> {
    execute_search_raw_with_config_inner(
        pool,
        resource_type,
        params,
        registry,
        query_cache,
        terminology,
        options,
    )
    .await
}

async fn execute_search_raw_with_config_inner(
    pool: &PgPool,
    resource_type: &str,
    params: &SearchParams,
    registry: Option<&Arc<SearchParameterRegistry>>,
    query_cache: Option<&QueryCache>,
    terminology: Option<&Arc<HybridTerminologyProvider>>,
    options: RawSearchOptions,
) -> Result<RawSearchResult, StorageError> {
    let requested_limit = params.count.unwrap_or(10) as usize;
    let mut effective_params = params.clone();
    effective_params.count = Some(params.count.unwrap_or(10).saturating_add(1));

    // Use default registry if none provided
    let empty_registry = Arc::new(SearchParameterRegistry::new());
    let registry_arc = registry.unwrap_or(&empty_registry);
    let registry = registry_arc.as_ref();

    // Pre-expand FHIR token search modifiers that require a terminology
    // service so the sync SQL builder can treat them as ordinary Token OR
    // searches (`:in`/`:not-in` against a ValueSet, `:above`/`:below`
    // against a code-system hierarchy).
    if let Some(tx) = terminology {
        let trait_view: Arc<dyn TerminologyProvider> = tx.clone();
        pre_expand_terminology_modifiers(
            &mut effective_params,
            registry,
            resource_type,
            &trait_view,
        )
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, "Terminology pre-expansion failed");
            StorageError::invalid_resource(format!("Terminology expansion failed: {e}"))
        })?;

        pre_expand_subsumption_modifiers(&mut effective_params, registry, resource_type, tx)
            .await
            .map_err(|e| {
                tracing::warn!(error = %e, "Subsumption pre-expansion failed");
                StorageError::invalid_resource(format!("Subsumption expansion failed: {e}"))
            })?;
    }

    // Build search config
    let search_config = ParamsSearchConfig {
        unknown_param_handling: options.unknown_param_handling.unwrap_or_default(),
        collect_debug_plan: options.collect_debug_plan,
    };

    // Convert SearchParams to SQL query using the params converter
    let build_started = Instant::now();
    let converted = build_native_ir_query_from_params_with_config(
        resource_type,
        &effective_params,
        registry,
        "public",
        &search_config,
    )
    .map_err(|e| {
        tracing::warn!(error = %e, "Failed to build search query");
        StorageError::invalid_resource(format!("Invalid search parameters: {e}"))
    })?;

    if let Some(debug_plan) = &converted.debug_plan {
        let plan_json = serde_json::to_string(debug_plan).unwrap_or_else(|_| "null".to_string());
        tracing::debug!(
            resource_type = %resource_type,
            predicate_count = debug_plan.predicates.len(),
            search_plan = %plan_json,
            "Built search debug plan"
        );
    }

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
                // (e.g. identifier=value vs identifier=system|value vs identifier=system|).
                // Encode this in the cache key to avoid binding a text param to a cached
                // JSON template (or vice versa).
                //
                // Distinguish four shapes:
                //   - "system|value" → JSON containment (@>) with system+value
                //   - "system|"      → EXISTS with system-only text match
                //   - "|value"       → EXISTS with empty-system text match
                //   - "value"        → EXISTS with value-only text match
                let token_shape = {
                    let has_no_pipe = values.iter().any(|v| !v.contains('|'));
                    let has_system_with_value = values.iter().any(|v| {
                        v.split_once('|')
                            .map(|(left, right)| !left.is_empty() && !right.is_empty())
                            .unwrap_or(false)
                    });
                    let has_system_only = values.iter().any(|v| {
                        v.split_once('|')
                            .map(|(left, right)| !left.is_empty() && right.is_empty())
                            .unwrap_or(false)
                    });
                    let has_empty_system = values.iter().any(|v| {
                        v.split_once('|')
                            .map(|(left, right)| left.is_empty() && !right.is_empty())
                            .unwrap_or(false)
                    });

                    // Compose a tag that varies whenever any of the shape signals differ.
                    // Each signal contributes a distinct letter so different combinations
                    // (e.g. system|value alone vs alongside system|) produce different keys.
                    let mut tag = String::with_capacity(8);
                    tag.push_str("tok-");
                    if has_system_with_value {
                        tag.push_str("sv");
                    }
                    if has_system_only {
                        tag.push('s');
                    }
                    if has_empty_system {
                        tag.push_str("es");
                    }
                    if has_no_pipe {
                        tag.push('p');
                    }
                    tag
                };

                // `:missing` selects between structurally different SQL
                // templates (`IS NULL` vs `IS NOT NULL`, `NOT EXISTS` vs
                // `EXISTS`) based on the boolean value, which is NOT bound
                // as a parameter. Two requests `gender:missing=true` and
                // `gender:missing=false` must therefore not share a cache
                // entry — encode the polarity into the cache name.
                let missing_tag = if modifier.as_deref() == Some("missing") {
                    let is_true = values.iter().any(|v| v.eq_ignore_ascii_case("true"));
                    let is_false = values.iter().any(|v| !v.eq_ignore_ascii_case("true"));
                    match (is_true, is_false) {
                        (true, false) => "#miss-t",
                        (false, true) => "#miss-f",
                        (true, true) => "#miss-tf",
                        _ => "#miss-x",
                    }
                } else {
                    ""
                };

                let cache_name = format!("{name}#{token_shape}{missing_tag}");

                // Distinguish prefixes per value — date / number / quantity SQL
                // shape depends on `eq`/`gt`/`lt`/`ne`/`ge`/`le`/`sa`/`eb`/`ap`
                // so two queries with the same name+modifier but different
                // prefixes must NOT share a cached query template.
                let prefixes: Vec<Option<String>> = values
                    .iter()
                    .map(|v| {
                        // Detect the same prefix the parser would extract.
                        let lower = v.to_ascii_lowercase();
                        for p in ["eq", "ne", "gt", "lt", "ge", "le", "sa", "eb", "ap"] {
                            if lower.starts_with(p) && v.len() > 2 {
                                let rest = &v[2..];
                                let next = rest.chars().next();
                                if next.is_some_and(|c| c.is_ascii_digit() || c == '-') {
                                    return Some(p.to_string());
                                }
                            }
                        }
                        None
                    })
                    .collect();

                QueryParamKey {
                    name: cache_name,
                    modifier,
                    param_type: QueryCacheKey::infer_param_type(key),
                    value_count: values.len(),
                    prefixes,
                }
            })
            .collect();
        // Encode sort direction into the cache key — otherwise `_sort=birthdate`
        // and `_sort=-birthdate` collide on the same template and the second
        // query reuses the wrong ORDER BY clause.
        let sort_fields: Vec<String> = params
            .sort
            .as_ref()
            .map(|s| {
                s.iter()
                    .map(|f| {
                        if f.descending {
                            format!("-{}", f.field)
                        } else {
                            f.field.clone()
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();
        QueryCacheKey::from_typed_params(
            resource_type,
            param_keys,
            params.count.is_some() || params.offset.is_some(),
            sort_fields,
        )
        .with_pagination(effective_params.count, effective_params.offset)
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

    let build_elapsed = build_started.elapsed();

    tracing::debug!(
        resource_type = %resource_type,
        search_engine = "native-ir",
        sql_shape = %redact_sql_shape(&built_query.sql),
        params_count = built_query.params.len(),
        "Executing raw search query"
    );

    let mut explain_plan = None;
    if options.collect_explain_plan || options.collect_explain_analyze {
        let explain_started = Instant::now();
        let plan =
            explain_built_search_query_json(pool, &built_query, options.collect_explain_analyze)
                .await?;
        let explain_json = serde_json::to_string(&plan).unwrap_or_else(|_| "null".to_string());
        tracing::debug!(
            resource_type = %resource_type,
            search_engine = "native-ir",
            analyze = options.collect_explain_analyze,
            sql_shape = %redact_sql_shape(&built_query.sql),
            params_count = built_query.params.len(),
            explain_elapsed_ms = explain_started.elapsed().as_secs_f64() * 1000.0,
            explain_plan = %explain_json,
            "Search native IR EXPLAIN"
        );
        explain_plan = Some(plan);
    }

    // Execute the main query with raw JSON (SQL already emits resource::text)
    let execute_started = Instant::now();
    let entries = execute_query_raw(pool, &built_query, resource_type).await?;
    let execute_elapsed = execute_started.elapsed();

    if options.collect_debug_plan {
        tracing::debug!(
            resource_type = %resource_type,
            search_engine = "native-ir",
            sql_shape = %redact_sql_shape(&built_query.sql),
            params_count = built_query.params.len(),
            build_elapsed_ms = build_elapsed.as_secs_f64() * 1000.0,
            db_execute_elapsed_ms = execute_elapsed.as_secs_f64() * 1000.0,
            "Search native IR perf trace"
        );
    }

    // Determine if there are more results
    let has_more = entries.len() > requested_limit;
    let entries: Vec<RawStoredResource> = entries.into_iter().take(requested_limit).collect();

    // Execute count query if requested
    let total = if let Some(cq) = count_query {
        let count_started = Instant::now();
        let total = execute_count_query(pool, &cq).await?;
        if options.collect_debug_plan {
            tracing::debug!(
                resource_type = %resource_type,
                search_engine = "native-ir",
                sql_shape = %redact_sql_shape(&cq.sql),
                params_count = cq.params.len(),
                db_execute_elapsed_ms = count_started.elapsed().as_secs_f64() * 1000.0,
                "Search native IR count perf trace"
            );
        }
        Some(total)
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
            registry,
        )
        .await?
    } else {
        Vec::new()
    };

    let debug = options.collect_debug_plan.then(|| RawSearchDebug {
        sql_shape: Some(redact_sql_shape(&built_query.sql)),
        plan: converted
            .debug_plan
            .as_ref()
            .and_then(|plan| serde_json::to_value(plan).ok()),
        explain: explain_plan,
        analyze: options.collect_explain_analyze,
        build_elapsed_ms: Some(build_elapsed.as_secs_f64() * 1000.0),
        db_execute_elapsed_ms: Some(execute_elapsed.as_secs_f64() * 1000.0),
    });

    Ok(RawSearchResult {
        entries,
        included,
        total,
        has_more,
        warnings,
        debug,
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
    let mut sqlx_query = sqlx_core::query::query::<sqlx_postgres::Postgres>(AssertSqlSafe((&query.sql).to_string()));

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
    let rows: Vec<(Value, String, i64, DateTime<Utc>, DateTime<Utc>)> = query_as(AssertSqlSafe((&query.sql).to_string()))
        .bind_all_params(&query.params)
        .fetch_all(pool)
        .await
        .map_err(|e| {
            tracing::warn!(
                error = %e,
                sql_shape = %redact_sql_shape(&query.sql),
                "Search query failed"
            );
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

async fn execute_query_with_tx(
    tx: &mut PgTransaction<'_>,
    query: &BuiltQuery,
    resource_type: &str,
) -> Result<Vec<StoredResource>, StorageError> {
    let rows: Vec<(Value, String, i64, DateTime<Utc>, DateTime<Utc>)> = query_as(AssertSqlSafe((&query.sql).to_string()))
        .bind_all_params(&query.params)
        .fetch_all(&mut **tx)
        .await
        .map_err(|e| {
            tracing::warn!(
                error = %e,
                sql_shape = %redact_sql_shape(&query.sql),
                "Transaction search query failed"
            );
            if is_undefined_table(&e) {
                return StorageError::internal(format!(
                    "Table for {} does not exist",
                    resource_type
                ));
            }
            StorageError::internal(format!("Search query failed: {e}"))
        })?;

    let entries = rows
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
    >(AssertSqlSafe((&query.sql).to_string()))
    .bind_all_params_raw(&query.params)
    .fetch_all(pool)
    .await
    .map_err(|e| {
        tracing::warn!(
            error = %e,
            sql_shape = %redact_sql_shape(&query.sql),
            "Raw search query failed"
        );
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
    let count: i64 = query_scalar(AssertSqlSafe((&query.sql).to_string()))
        .bind_all_params(&query.params)
        .fetch_one(pool)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, "Count query failed");
            StorageError::internal(format!("Count query failed: {e}"))
        })?;

    Ok(count as u32)
}

/// Execute PostgreSQL EXPLAIN for an already-built search query and return FORMAT JSON output.
///
/// `analyze = true` runs the query via `EXPLAIN ANALYZE`; callers must keep that behind an
/// explicit internal/debug gate.
pub async fn explain_built_search_query_json(
    pool: &PgPool,
    query: &BuiltQuery,
    analyze: bool,
) -> Result<Value, StorageError> {
    let explain_sql = build_explain_sql(&query.sql, analyze);
    query_scalar(AssertSqlSafe((&explain_sql).to_string()))
        .bind_all_params(&query.params)
        .fetch_one(pool)
        .await
        .map_err(|e| {
            tracing::warn!(
                error = %e,
                sql_shape = %redact_sql_shape(&query.sql),
                analyze,
                "Search EXPLAIN query failed"
            );
            StorageError::internal(format!("Search EXPLAIN failed: {e}"))
        })
}

fn build_explain_sql(sql: &str, analyze: bool) -> String {
    let options = if analyze {
        "ANALYZE, BUFFERS, FORMAT JSON"
    } else {
        "FORMAT JSON"
    };
    format!("EXPLAIN ({options}) {sql}")
}

async fn execute_count_query_with_tx(
    tx: &mut PgTransaction<'_>,
    query: &BuiltQuery,
) -> Result<u32, StorageError> {
    let count: i64 = query_scalar(AssertSqlSafe((&query.sql).to_string()))
        .bind_all_params(&query.params)
        .fetch_one(&mut **tx)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, "Transaction count query failed");
            StorageError::internal(format!("Count query failed: {e}"))
        })?;

    Ok(count as u32)
}

/// Build the array-or-singleton JSONB expression locating a reference search
/// parameter's references on `resource_col`, derived from the parameter's
/// FHIRPath via the registry. Returns `None` when the parameter or its
/// expression is unknown (the include then yields nothing).
fn reference_array_sql(
    registry: &SearchParameterRegistry,
    resource_type: &str,
    param_name: &str,
    resource_col: &str,
) -> Option<String> {
    let def = registry.get(resource_type, param_name)?;
    let expr = def.expression.as_deref()?;
    let segments = fhirpath_to_jsonb_path(expr, resource_type);
    let obj = build_jsonb_accessor(resource_col, &segments, false);
    Some(format!(
        "CASE WHEN jsonb_typeof({obj}) = 'array' THEN {obj} \
         WHEN {obj} IS NULL THEN '[]'::jsonb ELSE jsonb_build_array({obj}) END"
    ))
}

/// Regex pulling (Type, id) out of a FHIR reference string, anchored at the end
/// so both relative (`Patient/123`) and absolute (`http://h/fhir/Patient/123`)
/// references match. Capture 1 = type, capture 2 = id.
const REFERENCE_TYPE_ID_RE: &str = r"([A-Za-z]+)/([A-Za-z0-9.-]{1,64})$";

/// Resolve _include and _revinclude specifications.
///
/// Executes all include and revinclude queries in parallel for better latency.
async fn resolve_includes_revincludes(
    pool: &PgPool,
    main_results: &[StoredResource],
    includes: &[octofhir_search::IncludeSpec],
    revincludes: &[octofhir_search::RevIncludeSpec],
    registry: &SearchParameterRegistry,
) -> Result<Vec<StoredResource>, StorageError> {
    use futures_util::future::try_join_all;

    // Build futures for all include queries
    let include_futures: Vec<_> = includes
        .iter()
        .map(|include| resolve_include(pool, main_results, include, registry))
        .collect();

    // Build futures for all revinclude queries
    let revinclude_futures: Vec<_> = revincludes
        .iter()
        .map(|revinclude| resolve_revinclude(pool, main_results, revinclude, registry))
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

/// Resolve a single _include specification by matching references in place over
/// the source resource JSONB (no sidecar index table).
async fn resolve_include(
    pool: &PgPool,
    main_results: &[StoredResource],
    include: &octofhir_search::IncludeSpec,
    registry: &SearchParameterRegistry,
) -> Result<Vec<StoredResource>, StorageError> {
    if include.iterate {
        return resolve_include_iterate(pool, main_results, include, registry).await;
    }

    resolve_include_once(pool, main_results, include, registry).await
}

async fn resolve_include_iterate(
    pool: &PgPool,
    main_results: &[StoredResource],
    include: &octofhir_search::IncludeSpec,
    registry: &SearchParameterRegistry,
) -> Result<Vec<StoredResource>, StorageError> {
    let mut visited: HashSet<(String, String)> = main_results
        .iter()
        .map(|r| (r.resource_type.clone(), r.id.clone()))
        .collect();
    let mut current: Vec<StoredResource> = main_results
        .iter()
        .filter(|r| r.resource_type == include.source_type)
        .cloned()
        .collect();
    let mut included = Vec::new();

    for _ in 0..INCLUDE_ITERATE_MAX_DEPTH {
        if current.is_empty() {
            break;
        }

        let next = resolve_include_once(pool, &current, include, registry).await?;
        current = Vec::new();
        for entry in next {
            let key = (entry.resource_type.clone(), entry.id.clone());
            if visited.insert(key) {
                if entry.resource_type == include.source_type {
                    current.push(entry.clone());
                }
                included.push(entry);
            }
        }
    }

    Ok(included)
}

async fn resolve_include_once(
    pool: &PgPool,
    main_results: &[StoredResource],
    include: &octofhir_search::IncludeSpec,
    registry: &SearchParameterRegistry,
) -> Result<Vec<StoredResource>, StorageError> {
    if main_results.is_empty() {
        return Ok(Vec::new());
    }

    let source_type = &include.source_type;
    let param_name = &include.param_name;

    // Collect source IDs for the declared source type only.
    let source_ids: Vec<&str> = main_results
        .iter()
        .filter(|r| r.resource_type == *source_type)
        .map(|r| r.id.as_str())
        .collect();
    if source_ids.is_empty() {
        return Ok(Vec::new());
    }

    // Candidate target types come from the parameter definition (or an explicit
    // :Type filter), not a sidecar table.
    let target_types = if let Some(target_type) = include.target_type.as_ref() {
        vec![target_type.clone()]
    } else {
        registry
            .get(source_type, param_name)
            .map(|p| p.target.clone())
            .unwrap_or_default()
    };

    let mut entries = Vec::new();
    for target_type in target_types {
        let mut matched =
            query_include_for_target(pool, source_type, param_name, &source_ids, &target_type, registry)
                .await?;
        entries.append(&mut matched);
    }

    Ok(entries)
}

async fn query_include_for_target(
    pool: &PgPool,
    source_type: &str,
    param_name: &str,
    source_ids: &[&str],
    target_type: &str,
    registry: &SearchParameterRegistry,
) -> Result<Vec<StoredResource>, StorageError> {
    let Some(ref_array) = reference_array_sql(registry, source_type, param_name, "s.resource")
    else {
        return Ok(Vec::new());
    };
    let source_table = SchemaManager::table_name(source_type);
    let table = SchemaManager::table_name(target_type);

    let sql = format!(
        r#"SELECT DISTINCT t.resource, t.id, t.txid, t.created_at, t.updated_at
           FROM "{source_table}" s
           CROSS JOIN LATERAL jsonb_array_elements({ref_array}) AS ref
           CROSS JOIN LATERAL (SELECT regexp_match(ref->>'reference', '{REFERENCE_TYPE_ID_RE}') AS m) x
           JOIN "{table}" t ON t.id = x.m[2] AND t.status != 'deleted'
           WHERE s.id = ANY($1::text[]) AND s.status != 'deleted' AND x.m[1] = $2"#
    );

    let rows: Vec<(Value, String, i64, DateTime<Utc>, DateTime<Utc>)> = query_as(AssertSqlSafe((&sql).to_string()))
        .bind(source_ids)
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

/// Resolve a single _revinclude specification by matching references in place
/// over the source resource JSONB (no sidecar index table).
async fn resolve_revinclude(
    pool: &PgPool,
    main_results: &[StoredResource],
    revinclude: &octofhir_search::RevIncludeSpec,
    registry: &SearchParameterRegistry,
) -> Result<Vec<StoredResource>, StorageError> {
    if revinclude.iterate {
        return resolve_revinclude_iterate(pool, main_results, revinclude, registry).await;
    }

    if main_results.is_empty() {
        return Ok(Vec::new());
    }

    let main_type = main_results
        .first()
        .map(|r| r.resource_type.as_str())
        .unwrap_or("");

    resolve_revinclude_once(pool, main_type, main_results, revinclude, registry).await
}

async fn resolve_revinclude_iterate(
    pool: &PgPool,
    main_results: &[StoredResource],
    revinclude: &octofhir_search::RevIncludeSpec,
    registry: &SearchParameterRegistry,
) -> Result<Vec<StoredResource>, StorageError> {
    let mut visited: HashSet<(String, String)> = main_results
        .iter()
        .map(|r| (r.resource_type.clone(), r.id.clone()))
        .collect();
    let mut current = main_results.to_vec();
    let mut included = Vec::new();

    for _ in 0..INCLUDE_ITERATE_MAX_DEPTH {
        if current.is_empty() {
            break;
        }

        let mut groups: HashMap<String, Vec<StoredResource>> = HashMap::new();
        for entry in current {
            groups
                .entry(entry.resource_type.clone())
                .or_default()
                .push(entry);
        }

        current = Vec::new();
        for (target_type, targets) in groups {
            let next =
                resolve_revinclude_once(pool, &target_type, &targets, revinclude, registry).await?;
            for entry in next {
                let key = (entry.resource_type.clone(), entry.id.clone());
                if visited.insert(key) {
                    current.push(entry.clone());
                    included.push(entry);
                }
            }
        }
    }

    Ok(included)
}

async fn resolve_revinclude_once(
    pool: &PgPool,
    target_type: &str,
    target_results: &[StoredResource],
    revinclude: &octofhir_search::RevIncludeSpec,
    registry: &SearchParameterRegistry,
) -> Result<Vec<StoredResource>, StorageError> {
    if revinclude
        .target_type
        .as_ref()
        .is_some_and(|expected| expected != target_type)
    {
        return Ok(Vec::new());
    }

    let target_ids: Vec<&str> = target_results.iter().map(|r| r.id.as_str()).collect();
    if target_ids.is_empty() {
        return Ok(Vec::new());
    }

    let source_type = &revinclude.source_type;
    let table = SchemaManager::table_name(source_type);
    let param_name = &revinclude.param_name;

    let Some(ref_array) = reference_array_sql(registry, source_type, param_name, "s.resource")
    else {
        return Ok(Vec::new());
    };

    // Find source resources whose reference (in place) points at any target id.
    let sql = format!(
        r#"SELECT DISTINCT s.resource, s.id, s.txid, s.created_at, s.updated_at
           FROM "{table}" s
           WHERE s.status != 'deleted'
           AND EXISTS (
             SELECT 1 FROM jsonb_array_elements({ref_array}) AS ref
             CROSS JOIN LATERAL (SELECT regexp_match(ref->>'reference', '{REFERENCE_TYPE_ID_RE}') AS m) x
             WHERE x.m[1] = $1 AND x.m[2] = ANY($2::text[])
           )"#
    );

    let rows: Vec<(Value, String, i64, DateTime<Utc>, DateTime<Utc>)> = query_as(AssertSqlSafe((&sql).to_string()))
        .bind(target_type)
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
    registry: &SearchParameterRegistry,
) -> Result<Vec<RawStoredResource>, StorageError> {
    use futures_util::future::try_join_all;

    let include_futures: Vec<_> = includes
        .iter()
        .map(|include| resolve_include_raw(pool, main_results, include, registry))
        .collect();

    let revinclude_futures: Vec<_> = revincludes
        .iter()
        .map(|revinclude| {
            resolve_revinclude_raw(pool, main_resource_type, main_results, revinclude, registry)
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

/// Resolve a single _include specification using raw JSON, matching references
/// in place over the source resource JSONB (no sidecar index table).
async fn resolve_include_raw(
    pool: &PgPool,
    main_results: &[RawStoredResource],
    include: &octofhir_search::IncludeSpec,
    registry: &SearchParameterRegistry,
) -> Result<Vec<RawStoredResource>, StorageError> {
    if include.iterate {
        return resolve_include_iterate_raw(pool, main_results, include, registry).await;
    }

    resolve_include_once_raw(pool, main_results, include, registry).await
}

async fn resolve_include_iterate_raw(
    pool: &PgPool,
    main_results: &[RawStoredResource],
    include: &octofhir_search::IncludeSpec,
    registry: &SearchParameterRegistry,
) -> Result<Vec<RawStoredResource>, StorageError> {
    let mut visited: HashSet<(String, String)> = main_results
        .iter()
        .map(|r| (r.resource_type.clone(), r.id.clone()))
        .collect();
    let mut current: Vec<RawStoredResource> = main_results
        .iter()
        .filter(|r| r.resource_type == include.source_type)
        .cloned()
        .collect();
    let mut included = Vec::new();

    for _ in 0..INCLUDE_ITERATE_MAX_DEPTH {
        if current.is_empty() {
            break;
        }

        let next = resolve_include_once_raw(pool, &current, include, registry).await?;
        current = Vec::new();
        for entry in next {
            let key = (entry.resource_type.clone(), entry.id.clone());
            if visited.insert(key) {
                if entry.resource_type == include.source_type {
                    current.push(entry.clone());
                }
                included.push(entry);
            }
        }
    }

    Ok(included)
}

async fn resolve_include_once_raw(
    pool: &PgPool,
    main_results: &[RawStoredResource],
    include: &octofhir_search::IncludeSpec,
    registry: &SearchParameterRegistry,
) -> Result<Vec<RawStoredResource>, StorageError> {
    if main_results.is_empty() {
        return Ok(Vec::new());
    }

    let source_type = &include.source_type;
    let param_name = &include.param_name;

    // Collect source IDs for the declared source type only.
    let source_ids: Vec<&str> = main_results
        .iter()
        .filter(|r| r.resource_type == *source_type)
        .map(|r| r.id.as_str())
        .collect();
    if source_ids.is_empty() {
        return Ok(Vec::new());
    }

    let target_types = if let Some(target_type) = include.target_type.as_ref() {
        vec![target_type.clone()]
    } else {
        registry
            .get(source_type, param_name)
            .map(|p| p.target.clone())
            .unwrap_or_default()
    };

    let mut entries = Vec::new();
    for target_type in target_types {
        let mut matched = query_include_for_target_raw(
            pool,
            source_type,
            param_name,
            &source_ids,
            &target_type,
            registry,
        )
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
    registry: &SearchParameterRegistry,
) -> Result<Vec<RawStoredResource>, StorageError> {
    let Some(ref_array) = reference_array_sql(registry, source_type, param_name, "s.resource")
    else {
        return Ok(Vec::new());
    };
    let source_table = SchemaManager::table_name(source_type);
    let table = SchemaManager::table_name(target_type);

    let sql = format!(
        r#"SELECT DISTINCT t.resource::text, t.id, t.txid, t.created_at, t.updated_at
           FROM "{source_table}" s
           CROSS JOIN LATERAL jsonb_array_elements({ref_array}) AS ref
           CROSS JOIN LATERAL (SELECT regexp_match(ref->>'reference', '{REFERENCE_TYPE_ID_RE}') AS m) x
           JOIN "{table}" t ON t.id = x.m[2] AND t.status != 'deleted'
           WHERE s.id = ANY($1::text[]) AND s.status != 'deleted' AND x.m[1] = $2"#
    );

    let rows: Vec<(String, String, i64, DateTime<Utc>, DateTime<Utc>)> = query_as(AssertSqlSafe((&sql).to_string()))
        .bind(source_ids)
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

/// Resolve a single _revinclude specification using raw JSON, matching
/// references in place over the source resource JSONB (no sidecar index table).
async fn resolve_revinclude_raw(
    pool: &PgPool,
    main_resource_type: &str,
    main_results: &[RawStoredResource],
    revinclude: &octofhir_search::RevIncludeSpec,
    registry: &SearchParameterRegistry,
) -> Result<Vec<RawStoredResource>, StorageError> {
    if revinclude.iterate {
        return resolve_revinclude_iterate_raw(pool, main_results, revinclude, registry).await;
    }

    if main_results.is_empty() {
        return Ok(Vec::new());
    }

    resolve_revinclude_once_raw(pool, main_resource_type, main_results, revinclude, registry).await
}

async fn resolve_revinclude_iterate_raw(
    pool: &PgPool,
    main_results: &[RawStoredResource],
    revinclude: &octofhir_search::RevIncludeSpec,
    registry: &SearchParameterRegistry,
) -> Result<Vec<RawStoredResource>, StorageError> {
    let mut visited: HashSet<(String, String)> = main_results
        .iter()
        .map(|r| (r.resource_type.clone(), r.id.clone()))
        .collect();
    let mut current = main_results.to_vec();
    let mut included = Vec::new();

    for _ in 0..INCLUDE_ITERATE_MAX_DEPTH {
        if current.is_empty() {
            break;
        }

        let mut groups: HashMap<String, Vec<RawStoredResource>> = HashMap::new();
        for entry in current {
            groups
                .entry(entry.resource_type.clone())
                .or_default()
                .push(entry);
        }

        current = Vec::new();
        for (target_type, targets) in groups {
            let next =
                resolve_revinclude_once_raw(pool, &target_type, &targets, revinclude, registry)
                    .await?;
            for entry in next {
                let key = (entry.resource_type.clone(), entry.id.clone());
                if visited.insert(key) {
                    current.push(entry.clone());
                    included.push(entry);
                }
            }
        }
    }

    Ok(included)
}

async fn resolve_revinclude_once_raw(
    pool: &PgPool,
    target_type: &str,
    target_results: &[RawStoredResource],
    revinclude: &octofhir_search::RevIncludeSpec,
    registry: &SearchParameterRegistry,
) -> Result<Vec<RawStoredResource>, StorageError> {
    if revinclude
        .target_type
        .as_ref()
        .is_some_and(|expected| expected != target_type)
    {
        return Ok(Vec::new());
    }

    let target_ids: Vec<&str> = target_results.iter().map(|r| r.id.as_str()).collect();
    if target_ids.is_empty() {
        return Ok(Vec::new());
    }

    let source_type = &revinclude.source_type;
    let table = SchemaManager::table_name(source_type);
    let param_name = &revinclude.param_name;

    let Some(ref_array) = reference_array_sql(registry, source_type, param_name, "s.resource")
    else {
        return Ok(Vec::new());
    };

    // Find source resources whose reference (in place) points at any target id.
    let sql = format!(
        r#"SELECT DISTINCT s.resource::text, s.id, s.txid, s.created_at, s.updated_at
           FROM "{table}" s
           WHERE s.status != 'deleted'
           AND EXISTS (
             SELECT 1 FROM jsonb_array_elements({ref_array}) AS ref
             CROSS JOIN LATERAL (SELECT regexp_match(ref->>'reference', '{REFERENCE_TYPE_ID_RE}') AS m) x
             WHERE x.m[1] = $1 AND x.m[2] = ANY($2::text[])
           )"#
    );

    let rows: Vec<(String, String, i64, DateTime<Utc>, DateTime<Utc>)> = query_as(AssertSqlSafe((&sql).to_string()))
        .bind(target_type)
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

impl<'q> BindAllParams<'q>
    for sqlx_core::query_scalar::QueryScalar<
        'q,
        sqlx_postgres::Postgres,
        Value,
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

    #[test]
    fn test_redact_sql_shape_removes_bind_indices_and_normalizes_space() {
        let sql = "SELECT *\nFROM patient r WHERE r.id = $1 AND r.updated_at >= $23";

        assert_eq!(
            redact_sql_shape(sql),
            "SELECT * FROM patient r WHERE r.id = $? AND r.updated_at >= $?"
        );
    }

    #[test]
    fn test_build_explain_sql_uses_json_format_and_optional_analyze() {
        let sql = "SELECT * FROM patient WHERE id = $1";

        assert_eq!(
            build_explain_sql(sql, false),
            "EXPLAIN (FORMAT JSON) SELECT * FROM patient WHERE id = $1"
        );
        assert_eq!(
            build_explain_sql(sql, true),
            "EXPLAIN (ANALYZE, BUFFERS, FORMAT JSON) SELECT * FROM patient WHERE id = $1"
        );
    }
}
