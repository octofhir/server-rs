//! Search index writer for denormalized search tables.
//!
//! Writes extracted reference and date index rows to the
//! `search_idx_reference` and `search_idx_date`
//! partitioned tables. Uses batch UNNEST INSERT for efficiency.

use octofhir_core::fhir_reference::NormalizedRef;
use octofhir_core::search_index::{ExtractedDate, ExtractedReference};
use octofhir_storage::StorageError;
use sqlx_core::query::query;
use sqlx_postgres::PgPool;

// ============================================================================
// Reference Index
// ============================================================================

/// Write reference index rows for a resource (pool-based).
///
/// 1. Deletes existing index rows for the resource
/// 2. Batch inserts new rows using UNNEST
pub async fn write_reference_index(
    pool: &PgPool,
    resource_type: &str,
    resource_id: &str,
    refs: &[ExtractedReference],
) -> Result<(), StorageError> {
    // Delete old index rows
    query("DELETE FROM search_idx_reference WHERE resource_type = $1 AND resource_id = $2")
        .bind(resource_type)
        .bind(resource_id)
        .execute(pool)
        .await
        .map_err(|e| StorageError::internal(format!("Failed to delete reference index: {e}")))?;

    if refs.is_empty() {
        return Ok(());
    }

    // Build arrays for UNNEST batch insert
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
        resource_types.push(resource_type.to_string());
        resource_ids.push(resource_id.to_string());
        param_codes.push(r.param_code.clone());
        ref_kinds.push(r.normalized.ref_kind());
        raw_references.push(r.raw_reference.clone());

        match &r.normalized {
            NormalizedRef::Local {
                target_type,
                target_id,
            } => {
                target_types.push(Some(target_type.clone()));
                target_ids.push(Some(target_id.clone()));
                external_urls.push(None);
                canonical_urls.push(None);
                canonical_versions.push(None);
                identifier_systems.push(None);
                identifier_values.push(None);
            }
            NormalizedRef::External { url } => {
                target_types.push(None);
                target_ids.push(None);
                external_urls.push(Some(url.clone()));
                canonical_urls.push(None);
                canonical_versions.push(None);
                identifier_systems.push(None);
                identifier_values.push(None);
            }
            NormalizedRef::Canonical { url, version } => {
                target_types.push(None);
                target_ids.push(None);
                external_urls.push(None);
                canonical_urls.push(Some(url.clone()));
                canonical_versions.push(version.clone());
                identifier_systems.push(None);
                identifier_values.push(None);
            }
            NormalizedRef::Identifier { system, value } => {
                target_types.push(None);
                target_ids.push(None);
                external_urls.push(None);
                canonical_urls.push(None);
                canonical_versions.push(None);
                identifier_systems.push(system.clone());
                identifier_values.push(Some(value.clone()));
            }
        }
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
    .execute(pool)
    .await
    .map_err(|e| StorageError::internal(format!("Failed to write reference index: {e}")))?;

    Ok(())
}

// ============================================================================
// Date Index
// ============================================================================

/// Write date index rows for a resource.
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

    let len = dates.len();
    let mut resource_types = Vec::with_capacity(len);
    let mut resource_ids = Vec::with_capacity(len);
    let mut param_codes = Vec::with_capacity(len);
    let mut range_starts = Vec::with_capacity(len);
    let mut range_ends = Vec::with_capacity(len);

    for d in dates {
        resource_types.push(resource_type.to_string());
        resource_ids.push(resource_id.to_string());
        param_codes.push(d.param_code.clone());
        range_starts.push(d.range_start.clone());
        range_ends.push(d.range_end.clone());
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
    .execute(pool)
    .await
    .map_err(|e| StorageError::internal(format!("Failed to write date index: {e}")))?;

    Ok(())
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
