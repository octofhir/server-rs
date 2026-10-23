//! Native search with application snapshot predicates on one database snapshot.

use std::sync::Arc;

use octofhir_search::SearchParameterRegistry;
use octofhir_search::terminology::HybridTerminologyProvider;
use octofhir_storage::{
    RawSearchAuthorization, RawSearchResult, SearchParams, StorageError, TotalMode,
};
use sqlx_postgres::{PgPool, PgTransaction};

use super::{IncludeExecution, RawSearchOptions, SearchSource};

/// Terminology and grants are prepared before checking out the read connection.
/// Native predicates, visible pagination, totals and every inline hop then
/// observe one repeatable snapshot. Callers supply their overall read deadline.
pub async fn execute_system_search_raw_authorized(
    pool: &PgPool,
    resource_types: &[&str],
    params: &SearchParams,
    registry: &Arc<SearchParameterRegistry>,
    terminology: Option<&Arc<HybridTerminologyProvider>>,
    options: RawSearchOptions,
    authorization: RawSearchAuthorization<'_>,
) -> Result<RawSearchResult, StorageError> {
    let registry = Arc::new(registry.snapshot());
    let prepared = super::prepare_system_search_params(
        resource_types,
        params,
        &registry,
        terminology,
        options
            .max_valueset_expansion
            .unwrap_or(super::DEFAULT_MAX_EXPANSION_SIZE),
    )
    .await?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|error| StorageError::internal(format!("Search transaction failed: {error}")))?;
    sqlx_core::query::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
        .execute(&mut *tx)
        .await
        .map_err(|error| StorageError::internal(format!("Search snapshot failed: {error}")))?;
    let result = execute_system_search_raw_authorized_with_tx(
        &mut tx,
        resource_types,
        &prepared,
        &registry,
        options,
        authorization,
    )
    .await;
    tx.rollback()
        .await
        .map_err(|error| StorageError::internal(format!("Search rollback failed: {error}")))?;
    result
}

/// All predicates must already be expanded before a Bundle write connection is
/// acquired. No pool lookup or resource-by-resource SQL fallback is performed.
pub async fn execute_system_search_raw_authorized_with_tx(
    tx: &mut PgTransaction<'_>,
    resource_types: &[&str],
    params: &SearchParams,
    registry: &Arc<SearchParameterRegistry>,
    options: RawSearchOptions,
    authorization: RawSearchAuthorization<'_>,
) -> Result<RawSearchResult, StorageError> {
    let source = SearchSource::Connection(tokio::sync::Mutex::new(&mut **tx));
    let registry = Arc::new(registry.snapshot());
    execute_authorized_source(
        &source,
        resource_types,
        params,
        &registry,
        options,
        authorization,
    )
    .await
}

async fn execute_authorized_source(
    source: &SearchSource<'_>,
    types: &[&str],
    params: &SearchParams,
    registry: &Arc<SearchParameterRegistry>,
    options: RawSearchOptions,
    authorization: RawSearchAuthorization<'_>,
) -> Result<RawSearchResult, StorageError> {
    if authorization.main.is_none() && authorization.included.is_none() {
        return super::execute_system_search_raw_source(
            source, types, params, registry, None, options,
        )
        .await;
    }
    let mut scan = params.clone();
    scan.parameters.retain(|name, _| {
        !octofhir_search::include::is_include_parameter(name)
            && !octofhir_search::include::is_revinclude_parameter(name)
    });
    let mut result = if let Some(predicate) = authorization.main {
        let requested = params.count.unwrap_or(10) as usize;
        let offset = params.offset.unwrap_or(0) as usize;
        let count_only = params.is_count_only();
        let count_all = count_only
            || requested == 0
            || matches!(
                params.total,
                Some(TotalMode::Accurate | TotalMode::Estimate)
            );
        scan.count = Some(256);
        scan.offset = Some(0);
        scan.total = None;
        scan.parameters.remove("_summary");
        let mut visible = 0usize;
        let mut output = RawSearchResult::default();
        let mut first = true;
        'pages: loop {
            let page = super::execute_system_search_raw_source(
                source, types, &scan, registry, None, options,
            )
            .await?;
            if first {
                output.warnings = page.warnings;
                output.ignored_parameters = page.ignored_parameters;
                output.debug = page.debug;
                first = false;
            }
            let scanned = page.entries.len();
            for (index, row) in page.entries.into_iter().enumerate() {
                if predicate(&row)? {
                    if !count_only && visible >= offset && output.entries.len() < requested {
                        output.entries.push(row);
                    }
                    visible = visible
                        .checked_add(1)
                        .ok_or_else(|| StorageError::internal("Visible search count overflow"))?;
                    if !count_all && visible > offset.saturating_add(requested) {
                        break 'pages;
                    }
                }
                if index % 32 == 31 {
                    tokio::task::yield_now().await;
                }
            }
            if !page.has_more {
                break;
            }
            if scanned == 0 {
                return Err(StorageError::internal("Authorized search made no progress"));
            }
            scan.offset = Some(
                scan.offset
                    .unwrap_or(0)
                    .checked_add(
                        u32::try_from(scanned)
                            .map_err(|_| StorageError::internal("Search offset overflow"))?,
                    )
                    .ok_or_else(|| StorageError::internal("Search offset overflow"))?,
            );
        }
        output.has_more = !count_only && visible > offset.saturating_add(requested);
        output.total = if count_all {
            Some(
                u32::try_from(visible)
                    .map_err(|_| StorageError::internal("Visible total overflow"))?,
            )
        } else {
            None
        };
        output
    } else {
        super::execute_system_search_raw_source(source, types, &scan, registry, None, options)
            .await?
    };
    if !params.is_count_only() && !result.entries.is_empty() {
        // Parse inline specifications without querying. SQL predicates and
        // terminology grants retain their original, separately prepared types.
        let converted = super::prepare_raw_query(
            types
                .first()
                .copied()
                .ok_or_else(|| StorageError::invalid_resource("Empty search types"))?,
            params,
            registry,
            None,
            options,
        )
        .await?
        .0;
        result.included = super::resolve_includes_revincludes_raw(
            source,
            "",
            &result.entries,
            &converted.includes,
            &converted.revincludes,
            IncludeExecution {
                registry,
                limits: options.include_limits,
                visibility: params.included_visibility.as_ref(),
                snapshot: authorization.included,
            },
        )
        .await?;
    }
    Ok(result)
}
