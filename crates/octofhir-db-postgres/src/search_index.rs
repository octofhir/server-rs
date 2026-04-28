//! Search index writer for denormalized search tables.
//!
//! Writes extracted reference and date index rows to the
//! `search_idx_reference` and `search_idx_date`
//! partitioned tables. Uses batch UNNEST INSERT for efficiency.

use serde_json::Value;
use sqlx_postgres::PgTransaction;

use octofhir_core::fhir_reference::NormalizedRef;
use octofhir_core::search_index::{ExtractedDate, ExtractedReference};
use octofhir_search::{SearchParameterRegistry, SearchParameterType};
use octofhir_storage::StorageError;
use sqlx_core::query::query;
use sqlx_postgres::PgPool;

/// Extract denormalized search index rows for a resource using the registry.
pub fn extract_search_index_rows(
    registry: &SearchParameterRegistry,
    resource_type: &str,
    resource: &Value,
) -> (Vec<ExtractedReference>, Vec<ExtractedDate>) {
    let params = registry.get_all_for_type(resource_type);

    if params.is_empty() {
        return (Vec::new(), Vec::new());
    }

    let mut refs = Vec::new();
    let mut dates = Vec::new();

    for param in &params {
        let expression = match &param.expression {
            Some(e) => e.as_str(),
            None => continue,
        };

        match param.param_type {
            SearchParameterType::Reference => {
                refs.extend(octofhir_core::search_index::extract_references(
                    resource,
                    resource_type,
                    &param.code,
                    expression,
                    None,
                ));
            }
            SearchParameterType::Date => {
                dates.extend(octofhir_core::search_index::extract_dates(
                    resource,
                    resource_type,
                    &param.code,
                    expression,
                ));
            }
            _ => {}
        }
    }

    (refs, dates)
}

// ============================================================================
// Reference Index
// ============================================================================

/// Write reference index rows for a resource (pool-based).
///
/// 1. Deletes existing index rows for the resource
/// 2. Batch inserts new rows using UNNEST. `resource_type` / `resource_id`
///    are bound as scalars instead of being cloned once per row.
pub async fn write_reference_index(
    pool: &PgPool,
    resource_type: &str,
    resource_id: &str,
    refs: &[ExtractedReference],
) -> Result<(), StorageError> {
    query("DELETE FROM search_idx_reference WHERE resource_type = $1 AND resource_id = $2")
        .bind(resource_type)
        .bind(resource_id)
        .execute(pool)
        .await
        .map_err(|e| StorageError::internal(format!("Failed to delete reference index: {e}")))?;

    if refs.is_empty() {
        return Ok(());
    }

    let cols = build_reference_unnest_columns(refs);

    query(
        "INSERT INTO search_idx_reference (\
            resource_type, resource_id, param_code, ref_kind, \
            target_type, target_id, external_url, \
            canonical_url, canonical_version, \
            identifier_system, identifier_value, raw_reference\
        ) SELECT $1, $2, t.pc, t.rk, t.tt, t.ti, t.eu, t.cu, t.cv, t.is_, t.iv, t.rr \
        FROM UNNEST(\
            $3::text[], $4::int2[], \
            $5::text[], $6::text[], $7::text[], \
            $8::text[], $9::text[], \
            $10::text[], $11::text[], $12::text[]\
        ) AS t(pc, rk, tt, ti, eu, cu, cv, is_, iv, rr)",
    )
    .bind(resource_type)
    .bind(resource_id)
    .bind(&cols.param_codes)
    .bind(&cols.ref_kinds)
    .bind(&cols.target_types)
    .bind(&cols.target_ids)
    .bind(&cols.external_urls)
    .bind(&cols.canonical_urls)
    .bind(&cols.canonical_versions)
    .bind(&cols.identifier_systems)
    .bind(&cols.identifier_values)
    .bind(&cols.raw_references)
    .execute(pool)
    .await
    .map_err(|e| StorageError::internal(format!("Failed to write reference index: {e}")))?;

    Ok(())
}

/// Per-row column arrays for the reference UNNEST INSERT, without the
/// constant `resource_type` / `resource_id` columns.
struct ReferenceUnnestColumns {
    param_codes: Vec<String>,
    ref_kinds: Vec<i16>,
    target_types: Vec<Option<String>>,
    target_ids: Vec<Option<String>>,
    external_urls: Vec<Option<String>>,
    canonical_urls: Vec<Option<String>>,
    canonical_versions: Vec<Option<String>>,
    identifier_systems: Vec<Option<String>>,
    identifier_values: Vec<Option<String>>,
    raw_references: Vec<Option<String>>,
}

fn build_reference_unnest_columns(refs: &[ExtractedReference]) -> ReferenceUnnestColumns {
    let len = refs.len();
    let mut cols = ReferenceUnnestColumns {
        param_codes: Vec::with_capacity(len),
        ref_kinds: Vec::with_capacity(len),
        target_types: Vec::with_capacity(len),
        target_ids: Vec::with_capacity(len),
        external_urls: Vec::with_capacity(len),
        canonical_urls: Vec::with_capacity(len),
        canonical_versions: Vec::with_capacity(len),
        identifier_systems: Vec::with_capacity(len),
        identifier_values: Vec::with_capacity(len),
        raw_references: Vec::with_capacity(len),
    };

    for r in refs {
        cols.param_codes.push(r.param_code.clone());
        cols.ref_kinds.push(r.normalized.ref_kind());
        cols.raw_references.push(r.raw_reference.clone());

        match &r.normalized {
            NormalizedRef::Local {
                target_type,
                target_id,
            } => {
                cols.target_types.push(Some(target_type.clone()));
                cols.target_ids.push(Some(target_id.clone()));
                cols.external_urls.push(None);
                cols.canonical_urls.push(None);
                cols.canonical_versions.push(None);
                cols.identifier_systems.push(None);
                cols.identifier_values.push(None);
            }
            NormalizedRef::External { url } => {
                cols.target_types.push(None);
                cols.target_ids.push(None);
                cols.external_urls.push(Some(url.clone()));
                cols.canonical_urls.push(None);
                cols.canonical_versions.push(None);
                cols.identifier_systems.push(None);
                cols.identifier_values.push(None);
            }
            NormalizedRef::Canonical { url, version } => {
                cols.target_types.push(None);
                cols.target_ids.push(None);
                cols.external_urls.push(None);
                cols.canonical_urls.push(Some(url.clone()));
                cols.canonical_versions.push(version.clone());
                cols.identifier_systems.push(None);
                cols.identifier_values.push(None);
            }
            NormalizedRef::Identifier { system, value } => {
                cols.target_types.push(None);
                cols.target_ids.push(None);
                cols.external_urls.push(None);
                cols.canonical_urls.push(None);
                cols.canonical_versions.push(None);
                cols.identifier_systems.push(system.clone());
                cols.identifier_values.push(Some(value.clone()));
            }
        }
    }

    cols
}

/// Write reference index rows for a resource within a transaction.
///
/// `resource_type` / `resource_id` are bound as scalars; only the
/// per-row columns are sent as arrays.
pub async fn write_reference_index_with_tx(
    tx: &mut PgTransaction<'_>,
    resource_type: &str,
    resource_id: &str,
    refs: &[ExtractedReference],
) -> Result<(), StorageError> {
    query("DELETE FROM search_idx_reference WHERE resource_type = $1 AND resource_id = $2")
        .bind(resource_type)
        .bind(resource_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| StorageError::internal(format!("Failed to delete reference index: {e}")))?;

    if refs.is_empty() {
        return Ok(());
    }

    let cols = build_reference_unnest_columns(refs);

    query(
        "INSERT INTO search_idx_reference (\
            resource_type, resource_id, param_code, ref_kind, \
            target_type, target_id, external_url, \
            canonical_url, canonical_version, \
            identifier_system, identifier_value, raw_reference\
        ) SELECT $1, $2, t.pc, t.rk, t.tt, t.ti, t.eu, t.cu, t.cv, t.is_, t.iv, t.rr \
        FROM UNNEST(\
            $3::text[], $4::int2[], \
            $5::text[], $6::text[], $7::text[], \
            $8::text[], $9::text[], \
            $10::text[], $11::text[], $12::text[]\
        ) AS t(pc, rk, tt, ti, eu, cu, cv, is_, iv, rr)",
    )
    .bind(resource_type)
    .bind(resource_id)
    .bind(&cols.param_codes)
    .bind(&cols.ref_kinds)
    .bind(&cols.target_types)
    .bind(&cols.target_ids)
    .bind(&cols.external_urls)
    .bind(&cols.canonical_urls)
    .bind(&cols.canonical_versions)
    .bind(&cols.identifier_systems)
    .bind(&cols.identifier_values)
    .bind(&cols.raw_references)
    .execute(&mut **tx)
    .await
    .map_err(|e| StorageError::internal(format!("Failed to write reference index: {e}")))?;

    Ok(())
}

// ============================================================================
// Date Index
// ============================================================================

/// Write date index rows for a resource. `resource_type` / `resource_id`
/// are bound as scalars; only the per-row columns are sent as arrays.
pub async fn write_date_index(
    pool: &PgPool,
    resource_type: &str,
    resource_id: &str,
    dates: &[ExtractedDate],
) -> Result<(), StorageError> {
    query("DELETE FROM search_idx_date WHERE resource_type = $1 AND resource_id = $2")
        .bind(resource_type)
        .bind(resource_id)
        .execute(pool)
        .await
        .map_err(|e| StorageError::internal(format!("Failed to delete date index: {e}")))?;

    if dates.is_empty() {
        return Ok(());
    }

    let cols = build_date_unnest_columns(dates);

    query(
        "INSERT INTO search_idx_date (\
            resource_type, resource_id, param_code, range_start, range_end\
        ) SELECT \
            $1, $2, t.pc, t.rs::timestamptz, t.re::timestamptz \
        FROM UNNEST(\
            $3::text[], $4::text[], $5::text[]\
        ) AS t(pc, rs, re)",
    )
    .bind(resource_type)
    .bind(resource_id)
    .bind(&cols.param_codes)
    .bind(&cols.range_starts)
    .bind(&cols.range_ends)
    .execute(pool)
    .await
    .map_err(|e| StorageError::internal(format!("Failed to write date index: {e}")))?;

    Ok(())
}

/// Write date index rows for a resource within a transaction.
pub async fn write_date_index_with_tx(
    tx: &mut PgTransaction<'_>,
    resource_type: &str,
    resource_id: &str,
    dates: &[ExtractedDate],
) -> Result<(), StorageError> {
    query("DELETE FROM search_idx_date WHERE resource_type = $1 AND resource_id = $2")
        .bind(resource_type)
        .bind(resource_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| StorageError::internal(format!("Failed to delete date index: {e}")))?;

    if dates.is_empty() {
        return Ok(());
    }

    let cols = build_date_unnest_columns(dates);

    query(
        "INSERT INTO search_idx_date (\
            resource_type, resource_id, param_code, range_start, range_end\
        ) SELECT \
            $1, $2, t.pc, t.rs::timestamptz, t.re::timestamptz \
        FROM UNNEST(\
            $3::text[], $4::text[], $5::text[]\
        ) AS t(pc, rs, re)",
    )
    .bind(resource_type)
    .bind(resource_id)
    .bind(&cols.param_codes)
    .bind(&cols.range_starts)
    .bind(&cols.range_ends)
    .execute(&mut **tx)
    .await
    .map_err(|e| StorageError::internal(format!("Failed to write date index: {e}")))?;

    Ok(())
}

struct DateUnnestColumns {
    param_codes: Vec<String>,
    range_starts: Vec<String>,
    range_ends: Vec<String>,
}

fn build_date_unnest_columns(dates: &[ExtractedDate]) -> DateUnnestColumns {
    let len = dates.len();
    let mut cols = DateUnnestColumns {
        param_codes: Vec::with_capacity(len),
        range_starts: Vec::with_capacity(len),
        range_ends: Vec::with_capacity(len),
    };
    for d in dates {
        cols.param_codes.push(d.param_code.clone());
        cols.range_starts.push(d.range_start.clone());
        cols.range_ends.push(d.range_end.clone());
    }
    cols
}

// ============================================================================
// Batch Index Writers (across many resources)
// ============================================================================

/// One row destined for `search_idx_reference`, fully expanded so a Vec of these
/// can be flushed via a single `INSERT ... SELECT FROM UNNEST(...)`.
struct ReferenceIndexRow {
    resource_type: String,
    resource_id: String,
    param_code: String,
    ref_kind: i16,
    target_type: Option<String>,
    target_id: Option<String>,
    external_url: Option<String>,
    canonical_url: Option<String>,
    canonical_version: Option<String>,
    identifier_system: Option<String>,
    identifier_value: Option<String>,
    raw_reference: Option<String>,
}

fn flatten_reference(
    resource_type: &str,
    resource_id: &str,
    r: &ExtractedReference,
) -> ReferenceIndexRow {
    let (
        target_type,
        target_id,
        external_url,
        canonical_url,
        canonical_version,
        identifier_system,
        identifier_value,
    ) = match &r.normalized {
        NormalizedRef::Local {
            target_type,
            target_id,
        } => (
            Some(target_type.clone()),
            Some(target_id.clone()),
            None,
            None,
            None,
            None,
            None,
        ),
        NormalizedRef::External { url } => (None, None, Some(url.clone()), None, None, None, None),
        NormalizedRef::Canonical { url, version } => (
            None,
            None,
            None,
            Some(url.clone()),
            version.clone(),
            None,
            None,
        ),
        NormalizedRef::Identifier { system, value } => (
            None,
            None,
            None,
            None,
            None,
            system.clone(),
            Some(value.clone()),
        ),
    };

    ReferenceIndexRow {
        resource_type: resource_type.to_string(),
        resource_id: resource_id.to_string(),
        param_code: r.param_code.clone(),
        ref_kind: r.normalized.ref_kind(),
        target_type,
        target_id,
        external_url,
        canonical_url,
        canonical_version,
        identifier_system,
        identifier_value,
        raw_reference: r.raw_reference.clone(),
    }
}

/// Accumulates extracted index rows across many resources and flushes them
/// to PostgreSQL inside one big UNNEST INSERT. Used by bulk-import paths so
/// 1000 resources cost one round-trip per index instead of N.
#[derive(Default)]
pub struct BatchIndexBuffer {
    refs: Vec<ReferenceIndexRow>,
    dates: Vec<DateIndexRow>,
}

struct DateIndexRow {
    resource_type: String,
    resource_id: String,
    param_code: String,
    range_start: String,
    range_end: String,
}

impl BatchIndexBuffer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Push extracted reference/date index rows for one resource.
    pub fn extend_with(
        &mut self,
        resource_type: &str,
        resource_id: &str,
        refs: &[ExtractedReference],
        dates: &[ExtractedDate],
    ) {
        self.refs.reserve(refs.len());
        for r in refs {
            self.refs
                .push(flatten_reference(resource_type, resource_id, r));
        }
        self.dates.reserve(dates.len());
        for d in dates {
            self.dates.push(DateIndexRow {
                resource_type: resource_type.to_string(),
                resource_id: resource_id.to_string(),
                param_code: d.param_code.clone(),
                range_start: d.range_start.clone(),
                range_end: d.range_end.clone(),
            });
        }
    }

    pub fn is_empty(&self) -> bool {
        self.refs.is_empty() && self.dates.is_empty()
    }

    /// Flush all accumulated rows to the search index tables in one pair of
    /// UNNEST inserts (reference + date). Caller is responsible for any
    /// upstream `DELETE` of stale rows; for fresh CREATEs none is needed.
    pub async fn flush_with_tx(self, tx: &mut PgTransaction<'_>) -> Result<(), StorageError> {
        let BatchIndexBuffer { refs, dates } = self;

        if !refs.is_empty() {
            let len = refs.len();
            let mut resource_types = Vec::with_capacity(len);
            let mut resource_ids = Vec::with_capacity(len);
            let mut param_codes = Vec::with_capacity(len);
            let mut ref_kinds = Vec::with_capacity(len);
            let mut target_types: Vec<Option<String>> = Vec::with_capacity(len);
            let mut target_ids: Vec<Option<String>> = Vec::with_capacity(len);
            let mut external_urls: Vec<Option<String>> = Vec::with_capacity(len);
            let mut canonical_urls: Vec<Option<String>> = Vec::with_capacity(len);
            let mut canonical_versions: Vec<Option<String>> = Vec::with_capacity(len);
            let mut identifier_systems: Vec<Option<String>> = Vec::with_capacity(len);
            let mut identifier_values: Vec<Option<String>> = Vec::with_capacity(len);
            let mut raw_references: Vec<Option<String>> = Vec::with_capacity(len);

            for r in refs {
                resource_types.push(r.resource_type);
                resource_ids.push(r.resource_id);
                param_codes.push(r.param_code);
                ref_kinds.push(r.ref_kind);
                target_types.push(r.target_type);
                target_ids.push(r.target_id);
                external_urls.push(r.external_url);
                canonical_urls.push(r.canonical_url);
                canonical_versions.push(r.canonical_version);
                identifier_systems.push(r.identifier_system);
                identifier_values.push(r.identifier_value);
                raw_references.push(r.raw_reference);
            }

            query(
                "INSERT INTO search_idx_reference (\
                    resource_type, resource_id, param_code, ref_kind, \
                    target_type, target_id, external_url, \
                    canonical_url, canonical_version, \
                    identifier_system, identifier_value, raw_reference\
                ) SELECT * FROM UNNEST(\
                    $1::text[], $2::text[], $3::text[], $4::int2[], \
                    $5::text[], $6::text[], $7::text[], \
                    $8::text[], $9::text[], \
                    $10::text[], $11::text[], $12::text[]\
                )",
            )
            .bind(&resource_types)
            .bind(&resource_ids)
            .bind(&param_codes)
            .bind(&ref_kinds)
            .bind(&target_types)
            .bind(&target_ids)
            .bind(&external_urls)
            .bind(&canonical_urls)
            .bind(&canonical_versions)
            .bind(&identifier_systems)
            .bind(&identifier_values)
            .bind(&raw_references)
            .execute(&mut **tx)
            .await
            .map_err(|e| {
                StorageError::internal(format!("Failed to batch-write reference index: {e}"))
            })?;
        }

        if !dates.is_empty() {
            let len = dates.len();
            let mut resource_types = Vec::with_capacity(len);
            let mut resource_ids = Vec::with_capacity(len);
            let mut param_codes = Vec::with_capacity(len);
            let mut range_starts = Vec::with_capacity(len);
            let mut range_ends = Vec::with_capacity(len);

            for d in dates {
                resource_types.push(d.resource_type);
                resource_ids.push(d.resource_id);
                param_codes.push(d.param_code);
                range_starts.push(d.range_start);
                range_ends.push(d.range_end);
            }

            query(
                "INSERT INTO search_idx_date (\
                    resource_type, resource_id, param_code, range_start, range_end\
                ) SELECT \
                    t.rt, t.rid, t.pc, t.rs::timestamptz, t.re::timestamptz \
                FROM UNNEST(\
                    $1::text[], $2::text[], $3::text[], $4::text[], $5::text[]\
                ) AS t(rt, rid, pc, rs, re)",
            )
            .bind(&resource_types)
            .bind(&resource_ids)
            .bind(&param_codes)
            .bind(&range_starts)
            .bind(&range_ends)
            .execute(&mut **tx)
            .await
            .map_err(|e| {
                StorageError::internal(format!("Failed to batch-write date index: {e}"))
            })?;
        }

        Ok(())
    }
}

// ============================================================================
// Delete All Indexes
// ============================================================================

/// Delete all search index rows for a resource.
pub async fn delete_search_indexes(
    pool: &PgPool,
    resource_type: &str,
    resource_id: &str,
) -> Result<(), StorageError> {
    // Delete from all index tables
    let del_ref =
        query("DELETE FROM search_idx_reference WHERE resource_type = $1 AND resource_id = $2")
            .bind(resource_type)
            .bind(resource_id)
            .execute(pool);

    let del_date =
        query("DELETE FROM search_idx_date WHERE resource_type = $1 AND resource_id = $2")
            .bind(resource_type)
            .bind(resource_id)
            .execute(pool);

    // Execute deletes concurrently
    let (r1, r2) = tokio::join!(del_ref, del_date);
    r1.map_err(|e| StorageError::internal(format!("Failed to delete reference index: {e}")))?;
    r2.map_err(|e| StorageError::internal(format!("Failed to delete date index: {e}")))?;

    Ok(())
}

/// Delete all search index rows for a resource within a transaction.
pub async fn delete_search_indexes_with_tx(
    tx: &mut PgTransaction<'_>,
    resource_type: &str,
    resource_id: &str,
) -> Result<(), StorageError> {
    query("DELETE FROM search_idx_reference WHERE resource_type = $1 AND resource_id = $2")
        .bind(resource_type)
        .bind(resource_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| StorageError::internal(format!("Failed to delete reference index: {e}")))?;

    query("DELETE FROM search_idx_date WHERE resource_type = $1 AND resource_id = $2")
        .bind(resource_type)
        .bind(resource_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| StorageError::internal(format!("Failed to delete date index: {e}")))?;

    Ok(())
}

/// Delete index rows for many ids of one `resource_type` in two statements
/// (one per index table). Caller groups by `resource_type` so partition
/// pruning stays effective.
pub async fn delete_search_indexes_batch_with_tx(
    tx: &mut PgTransaction<'_>,
    resource_type: &str,
    resource_ids: &[String],
) -> Result<(), StorageError> {
    if resource_ids.is_empty() {
        return Ok(());
    }

    query(
        "DELETE FROM search_idx_reference \
         WHERE resource_type = $1 AND resource_id = ANY($2)",
    )
    .bind(resource_type)
    .bind(resource_ids)
    .execute(&mut **tx)
    .await
    .map_err(|e| StorageError::internal(format!("Failed to batch-delete reference index: {e}")))?;

    query(
        "DELETE FROM search_idx_date \
         WHERE resource_type = $1 AND resource_id = ANY($2)",
    )
    .bind(resource_type)
    .bind(resource_ids)
    .execute(&mut **tx)
    .await
    .map_err(|e| StorageError::internal(format!("Failed to batch-delete date index: {e}")))?;

    Ok(())
}
