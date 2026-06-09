//! Search index writer for denormalized search tables.
//!
//! Writes extracted reference and date index rows to the
//! `search_idx_reference` and `search_idx_date`
//! partitioned tables. Uses batch UNNEST INSERT for efficiency.

use serde_json::Value;
use sqlx_postgres::PgTransaction;

use octofhir_core::fhir_reference::NormalizedRef;
use octofhir_core::search_index::{
    ExtractedDate, ExtractedNumber, ExtractedQuantity, ExtractedReference, ExtractedString,
};
use octofhir_search::{SearchParameterRegistry, SearchParameterType};
use octofhir_storage::StorageError;
use sqlx_core::query::query;
use sqlx_postgres::PgPool;

use dashmap::DashSet;
use std::sync::OnceLock;

/// Cache of `(resource_type)` for which the search-index partitions have
/// already been created during this process lifetime. Avoids re-issuing
/// `CREATE TABLE ... PARTITION OF` (which takes AccessExclusiveLock on
/// the parent) on every CRUD insert.
fn partition_cache() -> &'static DashSet<String> {
    static CACHE: OnceLock<DashSet<String>> = OnceLock::new();
    CACHE.get_or_init(DashSet::new)
}

/// Ensure `search_idx_*_<rt>` partitions exist.
///
/// Bootstrap no longer pre-creates these partitions because each `CREATE
/// TABLE PARTITION OF` serialises on the parent's AccessExclusiveLock
/// (~6s for 171×2 partitions). Per-process cache means we pay the
/// ~17ms-per-resource-type cost exactly once, on the resource type's
/// first write.
async fn ensure_search_partition(pool: &PgPool, resource_type: &str) -> Result<(), StorageError> {
    let cache = partition_cache();
    if cache.contains(resource_type) {
        return Ok(());
    }
    let table = resource_type.to_lowercase();
    // See `ensure_search_partition_in_tx`: serialise concurrent first-write
    // partition creation per resource type to avoid the racy `CREATE TABLE IF NOT
    // EXISTS ... PARTITION OF` failing with `relation already exists` (42P07). The
    // whole multi-statement batch runs as one implicit transaction, so the
    // advisory lock is held across the CREATEs and released on commit.
    let sql = format!(
        "SELECT pg_advisory_xact_lock(hashtext('octofhir_search_partition:{resource_type}')); \
         CREATE TABLE IF NOT EXISTS \"search_idx_reference_{table}\" \
            PARTITION OF search_idx_reference FOR VALUES IN ('{resource_type}'); \
         CREATE TABLE IF NOT EXISTS \"search_idx_date_{table}\" \
            PARTITION OF search_idx_date FOR VALUES IN ('{resource_type}'); \
         CREATE TABLE IF NOT EXISTS \"search_idx_string_{table}\" \
            PARTITION OF search_idx_string FOR VALUES IN ('{resource_type}'); \
         CREATE TABLE IF NOT EXISTS \"search_idx_number_{table}\" \
            PARTITION OF search_idx_number FOR VALUES IN ('{resource_type}'); \
         CREATE TABLE IF NOT EXISTS \"search_idx_quantity_{table}\" \
            PARTITION OF search_idx_quantity FOR VALUES IN ('{resource_type}');"
    );
    sqlx_core::raw_sql::raw_sql(&sql)
        .execute(pool)
        .await
        .map_err(|e| {
            StorageError::internal(format!("ensure_search_partition({resource_type}): {e}"))
        })?;
    cache.insert(resource_type.to_string());
    Ok(())
}

/// Tx-bound variant: no process cache because a rollback would
/// leave the cache out of sync with the catalog. `CREATE TABLE IF
/// NOT EXISTS` is a cheap catalog lookup once the partition exists.
async fn ensure_search_partition_in_tx(
    conn: &mut sqlx_postgres::PgConnection,
    resource_type: &str,
) -> Result<(), StorageError> {
    if partition_cache().contains(resource_type) {
        return Ok(());
    }
    let table = resource_type.to_lowercase();

    // `CREATE TABLE IF NOT EXISTS ... PARTITION OF` is NOT race-safe: concurrent
    // transactions both pass the catalog existence check and then one fails with
    // `relation "..." already exists` (42P07), which aborts that transaction and
    // surfaces as a 500. Serialise the first-write-per-type creation with a
    // transaction-scoped advisory lock keyed on the resource type. Only contended
    // on the cold path (before the partition exists / is cached); once created the
    // early-return above skips this entirely.
    let lock_sql = format!(
        r#"SELECT pg_advisory_xact_lock(hashtext('octofhir_search_partition:{resource_type}'))"#
    );
    query(&lock_sql).execute(&mut *conn).await.map_err(|e| {
        StorageError::internal(format!(
            "ensure_search_partition_in_tx(lock/{resource_type}): {e}"
        ))
    })?;

    let ref_sql = format!(
        r#"CREATE TABLE IF NOT EXISTS "search_idx_reference_{table}" PARTITION OF search_idx_reference FOR VALUES IN ('{resource_type}')"#
    );
    let date_sql = format!(
        r#"CREATE TABLE IF NOT EXISTS "search_idx_date_{table}" PARTITION OF search_idx_date FOR VALUES IN ('{resource_type}')"#
    );
    let string_sql = format!(
        r#"CREATE TABLE IF NOT EXISTS "search_idx_string_{table}" PARTITION OF search_idx_string FOR VALUES IN ('{resource_type}')"#
    );
    let number_sql = format!(
        r#"CREATE TABLE IF NOT EXISTS "search_idx_number_{table}" PARTITION OF search_idx_number FOR VALUES IN ('{resource_type}')"#
    );
    let quantity_sql = format!(
        r#"CREATE TABLE IF NOT EXISTS "search_idx_quantity_{table}" PARTITION OF search_idx_quantity FOR VALUES IN ('{resource_type}')"#
    );
    query(&ref_sql).execute(&mut *conn).await.map_err(|e| {
        StorageError::internal(format!(
            "ensure_search_partition_in_tx(ref/{resource_type}): {e}"
        ))
    })?;
    query(&date_sql).execute(&mut *conn).await.map_err(|e| {
        StorageError::internal(format!(
            "ensure_search_partition_in_tx(date/{resource_type}): {e}"
        ))
    })?;
    query(&string_sql).execute(&mut *conn).await.map_err(|e| {
        StorageError::internal(format!(
            "ensure_search_partition_in_tx(string/{resource_type}): {e}"
        ))
    })?;
    query(&number_sql).execute(&mut *conn).await.map_err(|e| {
        StorageError::internal(format!(
            "ensure_search_partition_in_tx(number/{resource_type}): {e}"
        ))
    })?;
    query(&quantity_sql)
        .execute(&mut *conn)
        .await
        .map_err(|e| {
            StorageError::internal(format!(
                "ensure_search_partition_in_tx(quantity/{resource_type}): {e}"
            ))
        })?;
    Ok(())
}

/// Denormalized search-index rows extracted for one resource.
///
/// One entry per FHIR value: arrays like `HumanName.given[]` or `Timing.event[]`
/// produce multiple rows.
#[derive(Debug, Default)]
pub struct ExtractedIndexRows {
    pub refs: Vec<ExtractedReference>,
    pub dates: Vec<ExtractedDate>,
    pub strings: Vec<ExtractedString>,
    pub numbers: Vec<ExtractedNumber>,
    pub quantities: Vec<ExtractedQuantity>,
}

/// Extract denormalised search index rows for a resource using the registry.
pub fn extract_search_index_rows(
    registry: &SearchParameterRegistry,
    resource_type: &str,
    resource: &Value,
) -> ExtractedIndexRows {
    let params = registry.get_all_for_type(resource_type);

    if params.is_empty() {
        return ExtractedIndexRows::default();
    }

    let mut refs = Vec::new();
    let mut dates = Vec::new();
    let mut strings = Vec::new();
    let mut numbers = Vec::new();
    let mut quantities = Vec::new();

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
            SearchParameterType::String => {
                strings.extend(octofhir_core::search_index::extract_strings(
                    resource,
                    resource_type,
                    &param.code,
                    expression,
                ));
            }
            SearchParameterType::Number => {
                numbers.extend(octofhir_core::search_index::extract_numbers(
                    resource,
                    resource_type,
                    &param.code,
                    expression,
                ));
            }
            SearchParameterType::Quantity => {
                quantities.extend(octofhir_core::search_index::extract_quantities(
                    resource,
                    resource_type,
                    &param.code,
                    expression,
                ));
            }
            _ => {}
        }
    }

    ExtractedIndexRows {
        refs,
        dates,
        strings,
        numbers,
        quantities,
    }
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
    ensure_search_partition(pool, resource_type).await?;
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
    // Lazy partition create inside the same tx — no process-global cache
    // here because a tx-rollback would invalidate it. `CREATE TABLE IF
    // NOT EXISTS` is cheap (catalog lookup) when the partition already
    // exists.
    ensure_search_partition_in_tx(tx, resource_type).await?;
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
    ensure_search_partition(pool, resource_type).await?;
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
    ensure_search_partition_in_tx(tx, resource_type).await?;
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
// String Index
// ============================================================================

/// Write string index rows for a resource. `resource_type` / `resource_id`
/// are bound as scalars; only per-row columns are sent as arrays.
pub async fn write_string_index(
    pool: &PgPool,
    resource_type: &str,
    resource_id: &str,
    strings: &[ExtractedString],
) -> Result<(), StorageError> {
    ensure_search_partition(pool, resource_type).await?;
    query("DELETE FROM search_idx_string WHERE resource_type = $1 AND resource_id = $2")
        .bind(resource_type)
        .bind(resource_id)
        .execute(pool)
        .await
        .map_err(|e| StorageError::internal(format!("Failed to delete string index: {e}")))?;

    if strings.is_empty() {
        return Ok(());
    }

    let cols = build_string_unnest_columns(strings);

    query(
        "INSERT INTO search_idx_string (\
            resource_type, resource_id, param_code, value_norm, value_exact\
        ) SELECT \
            $1, $2, t.pc, t.vn, t.ve \
        FROM UNNEST(\
            $3::text[], $4::text[], $5::text[]\
        ) AS t(pc, vn, ve)",
    )
    .bind(resource_type)
    .bind(resource_id)
    .bind(&cols.param_codes)
    .bind(&cols.value_norms)
    .bind(&cols.value_exacts)
    .execute(pool)
    .await
    .map_err(|e| StorageError::internal(format!("Failed to write string index: {e}")))?;

    Ok(())
}

/// Write string index rows for a resource within a transaction.
pub async fn write_string_index_with_tx(
    tx: &mut PgTransaction<'_>,
    resource_type: &str,
    resource_id: &str,
    strings: &[ExtractedString],
) -> Result<(), StorageError> {
    ensure_search_partition_in_tx(tx, resource_type).await?;
    query("DELETE FROM search_idx_string WHERE resource_type = $1 AND resource_id = $2")
        .bind(resource_type)
        .bind(resource_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| StorageError::internal(format!("Failed to delete string index: {e}")))?;

    if strings.is_empty() {
        return Ok(());
    }

    let cols = build_string_unnest_columns(strings);

    query(
        "INSERT INTO search_idx_string (\
            resource_type, resource_id, param_code, value_norm, value_exact\
        ) SELECT \
            $1, $2, t.pc, t.vn, t.ve \
        FROM UNNEST(\
            $3::text[], $4::text[], $5::text[]\
        ) AS t(pc, vn, ve)",
    )
    .bind(resource_type)
    .bind(resource_id)
    .bind(&cols.param_codes)
    .bind(&cols.value_norms)
    .bind(&cols.value_exacts)
    .execute(&mut **tx)
    .await
    .map_err(|e| StorageError::internal(format!("Failed to write string index: {e}")))?;

    Ok(())
}

struct StringUnnestColumns {
    param_codes: Vec<String>,
    value_norms: Vec<String>,
    value_exacts: Vec<String>,
}

fn build_string_unnest_columns(strings: &[ExtractedString]) -> StringUnnestColumns {
    let len = strings.len();
    let mut cols = StringUnnestColumns {
        param_codes: Vec::with_capacity(len),
        value_norms: Vec::with_capacity(len),
        value_exacts: Vec::with_capacity(len),
    };
    for s in strings {
        cols.param_codes.push(s.param_code.clone());
        cols.value_norms.push(s.value_normalized.clone());
        cols.value_exacts.push(s.value_exact.clone());
    }
    cols
}

// ============================================================================
// Number / Quantity Indexes
// ============================================================================

pub async fn write_number_index(
    pool: &PgPool,
    resource_type: &str,
    resource_id: &str,
    numbers: &[ExtractedNumber],
) -> Result<(), StorageError> {
    ensure_search_partition(pool, resource_type).await?;
    query("DELETE FROM search_idx_number WHERE resource_type = $1 AND resource_id = $2")
        .bind(resource_type)
        .bind(resource_id)
        .execute(pool)
        .await
        .map_err(|e| StorageError::internal(format!("Failed to delete number index: {e}")))?;

    if numbers.is_empty() {
        return Ok(());
    }

    let (param_codes, values): (Vec<_>, Vec<_>) = numbers
        .iter()
        .map(|n| (n.param_code.clone(), n.value.clone()))
        .unzip();

    query(
        "INSERT INTO search_idx_number (\
            resource_type, resource_id, param_code, value_num\
        ) SELECT \
            $1, $2, t.pc, t.vn::numeric \
        FROM UNNEST($3::text[], $4::text[]) AS t(pc, vn)",
    )
    .bind(resource_type)
    .bind(resource_id)
    .bind(&param_codes)
    .bind(&values)
    .execute(pool)
    .await
    .map_err(|e| StorageError::internal(format!("Failed to write number index: {e}")))?;

    Ok(())
}

pub async fn write_number_index_with_tx(
    tx: &mut PgTransaction<'_>,
    resource_type: &str,
    resource_id: &str,
    numbers: &[ExtractedNumber],
) -> Result<(), StorageError> {
    ensure_search_partition_in_tx(tx, resource_type).await?;
    query("DELETE FROM search_idx_number WHERE resource_type = $1 AND resource_id = $2")
        .bind(resource_type)
        .bind(resource_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| StorageError::internal(format!("Failed to delete number index: {e}")))?;

    if numbers.is_empty() {
        return Ok(());
    }

    let (param_codes, values): (Vec<_>, Vec<_>) = numbers
        .iter()
        .map(|n| (n.param_code.clone(), n.value.clone()))
        .unzip();

    query(
        "INSERT INTO search_idx_number (\
            resource_type, resource_id, param_code, value_num\
        ) SELECT \
            $1, $2, t.pc, t.vn::numeric \
        FROM UNNEST($3::text[], $4::text[]) AS t(pc, vn)",
    )
    .bind(resource_type)
    .bind(resource_id)
    .bind(&param_codes)
    .bind(&values)
    .execute(&mut **tx)
    .await
    .map_err(|e| StorageError::internal(format!("Failed to write number index: {e}")))?;

    Ok(())
}

pub async fn write_quantity_index(
    pool: &PgPool,
    resource_type: &str,
    resource_id: &str,
    quantities: &[ExtractedQuantity],
) -> Result<(), StorageError> {
    ensure_search_partition(pool, resource_type).await?;
    query("DELETE FROM search_idx_quantity WHERE resource_type = $1 AND resource_id = $2")
        .bind(resource_type)
        .bind(resource_id)
        .execute(pool)
        .await
        .map_err(|e| StorageError::internal(format!("Failed to delete quantity index: {e}")))?;

    if quantities.is_empty() {
        return Ok(());
    }

    let cols = build_quantity_unnest_columns(quantities);
    query(
        "INSERT INTO search_idx_quantity (\
            resource_type, resource_id, param_code, value_num, system, code, unit\
        ) SELECT \
            $1, $2, t.pc, t.vn::numeric, t.sys, t.code, t.unit \
        FROM UNNEST(\
            $3::text[], $4::text[], $5::text[], $6::text[], $7::text[]\
        ) AS t(pc, vn, sys, code, unit)",
    )
    .bind(resource_type)
    .bind(resource_id)
    .bind(&cols.param_codes)
    .bind(&cols.values)
    .bind(&cols.systems)
    .bind(&cols.codes)
    .bind(&cols.units)
    .execute(pool)
    .await
    .map_err(|e| StorageError::internal(format!("Failed to write quantity index: {e}")))?;

    Ok(())
}

pub async fn write_quantity_index_with_tx(
    tx: &mut PgTransaction<'_>,
    resource_type: &str,
    resource_id: &str,
    quantities: &[ExtractedQuantity],
) -> Result<(), StorageError> {
    ensure_search_partition_in_tx(tx, resource_type).await?;
    query("DELETE FROM search_idx_quantity WHERE resource_type = $1 AND resource_id = $2")
        .bind(resource_type)
        .bind(resource_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| StorageError::internal(format!("Failed to delete quantity index: {e}")))?;

    if quantities.is_empty() {
        return Ok(());
    }

    let cols = build_quantity_unnest_columns(quantities);
    query(
        "INSERT INTO search_idx_quantity (\
            resource_type, resource_id, param_code, value_num, system, code, unit\
        ) SELECT \
            $1, $2, t.pc, t.vn::numeric, t.sys, t.code, t.unit \
        FROM UNNEST(\
            $3::text[], $4::text[], $5::text[], $6::text[], $7::text[]\
        ) AS t(pc, vn, sys, code, unit)",
    )
    .bind(resource_type)
    .bind(resource_id)
    .bind(&cols.param_codes)
    .bind(&cols.values)
    .bind(&cols.systems)
    .bind(&cols.codes)
    .bind(&cols.units)
    .execute(&mut **tx)
    .await
    .map_err(|e| StorageError::internal(format!("Failed to write quantity index: {e}")))?;

    Ok(())
}

struct QuantityUnnestColumns {
    param_codes: Vec<String>,
    values: Vec<String>,
    systems: Vec<Option<String>>,
    codes: Vec<Option<String>>,
    units: Vec<Option<String>>,
}

fn build_quantity_unnest_columns(quantities: &[ExtractedQuantity]) -> QuantityUnnestColumns {
    let len = quantities.len();
    let mut cols = QuantityUnnestColumns {
        param_codes: Vec::with_capacity(len),
        values: Vec::with_capacity(len),
        systems: Vec::with_capacity(len),
        codes: Vec::with_capacity(len),
        units: Vec::with_capacity(len),
    };
    for q in quantities {
        cols.param_codes.push(q.param_code.clone());
        cols.values.push(q.value.clone());
        cols.systems.push(q.system.clone());
        cols.codes.push(q.code.clone());
        cols.units.push(q.unit.clone());
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
    strings: Vec<StringIndexRow>,
    numbers: Vec<NumberIndexRow>,
    quantities: Vec<QuantityIndexRow>,
}

struct DateIndexRow {
    resource_type: String,
    resource_id: String,
    param_code: String,
    range_start: String,
    range_end: String,
}

struct StringIndexRow {
    resource_type: String,
    resource_id: String,
    param_code: String,
    value_norm: String,
    value_exact: String,
}

struct NumberIndexRow {
    resource_type: String,
    resource_id: String,
    param_code: String,
    value: String,
}

struct QuantityIndexRow {
    resource_type: String,
    resource_id: String,
    param_code: String,
    value: String,
    system: Option<String>,
    code: Option<String>,
    unit: Option<String>,
}

impl BatchIndexBuffer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Push extracted index rows for one resource.
    pub fn extend_with(
        &mut self,
        resource_type: &str,
        resource_id: &str,
        rows: &ExtractedIndexRows,
    ) {
        self.refs.reserve(rows.refs.len());
        for r in &rows.refs {
            self.refs
                .push(flatten_reference(resource_type, resource_id, r));
        }
        self.dates.reserve(rows.dates.len());
        for d in &rows.dates {
            self.dates.push(DateIndexRow {
                resource_type: resource_type.to_string(),
                resource_id: resource_id.to_string(),
                param_code: d.param_code.clone(),
                range_start: d.range_start.clone(),
                range_end: d.range_end.clone(),
            });
        }
        self.strings.reserve(rows.strings.len());
        for s in &rows.strings {
            self.strings.push(StringIndexRow {
                resource_type: resource_type.to_string(),
                resource_id: resource_id.to_string(),
                param_code: s.param_code.clone(),
                value_norm: s.value_normalized.clone(),
                value_exact: s.value_exact.clone(),
            });
        }
        self.numbers.reserve(rows.numbers.len());
        for n in &rows.numbers {
            self.numbers.push(NumberIndexRow {
                resource_type: resource_type.to_string(),
                resource_id: resource_id.to_string(),
                param_code: n.param_code.clone(),
                value: n.value.clone(),
            });
        }
        self.quantities.reserve(rows.quantities.len());
        for q in &rows.quantities {
            self.quantities.push(QuantityIndexRow {
                resource_type: resource_type.to_string(),
                resource_id: resource_id.to_string(),
                param_code: q.param_code.clone(),
                value: q.value.clone(),
                system: q.system.clone(),
                code: q.code.clone(),
                unit: q.unit.clone(),
            });
        }
    }

    pub fn is_empty(&self) -> bool {
        self.refs.is_empty()
            && self.dates.is_empty()
            && self.strings.is_empty()
            && self.numbers.is_empty()
            && self.quantities.is_empty()
    }

    /// Flush all accumulated rows to the search index tables in one UNNEST
    /// insert per sidecar (reference, date, string). Caller is responsible
    /// for any upstream `DELETE` of stale rows; for fresh CREATEs none is
    /// needed.
    pub async fn flush_with_tx(self, tx: &mut PgTransaction<'_>) -> Result<(), StorageError> {
        let BatchIndexBuffer {
            refs,
            dates,
            strings,
            numbers,
            quantities,
        } = self;

        // Ensure list partitions exist for every resource_type in this batch.
        // Without this the batched flush path fails with "no partition of
        // relation found for row" when a resource type was never touched by
        // the single-resource writers yet.
        let mut seen_types: std::collections::HashSet<String> = std::collections::HashSet::new();
        for r in &refs {
            seen_types.insert(r.resource_type.clone());
        }
        for d in &dates {
            seen_types.insert(d.resource_type.clone());
        }
        for s in &strings {
            seen_types.insert(s.resource_type.clone());
        }
        for n in &numbers {
            seen_types.insert(n.resource_type.clone());
        }
        for q in &quantities {
            seen_types.insert(q.resource_type.clone());
        }
        for rt in &seen_types {
            ensure_search_partition_in_tx(tx, rt).await?;
        }

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

        if !strings.is_empty() {
            let len = strings.len();
            let mut resource_types = Vec::with_capacity(len);
            let mut resource_ids = Vec::with_capacity(len);
            let mut param_codes = Vec::with_capacity(len);
            let mut value_norms = Vec::with_capacity(len);
            let mut value_exacts = Vec::with_capacity(len);

            for s in strings {
                resource_types.push(s.resource_type);
                resource_ids.push(s.resource_id);
                param_codes.push(s.param_code);
                value_norms.push(s.value_norm);
                value_exacts.push(s.value_exact);
            }

            query(
                "INSERT INTO search_idx_string (\
                    resource_type, resource_id, param_code, value_norm, value_exact\
                ) SELECT \
                    t.rt, t.rid, t.pc, t.vn, t.ve \
                FROM UNNEST(\
                    $1::text[], $2::text[], $3::text[], $4::text[], $5::text[]\
                ) AS t(rt, rid, pc, vn, ve)",
            )
            .bind(&resource_types)
            .bind(&resource_ids)
            .bind(&param_codes)
            .bind(&value_norms)
            .bind(&value_exacts)
            .execute(&mut **tx)
            .await
            .map_err(|e| {
                StorageError::internal(format!("Failed to batch-write string index: {e}"))
            })?;
        }

        if !numbers.is_empty() {
            let len = numbers.len();
            let mut resource_types = Vec::with_capacity(len);
            let mut resource_ids = Vec::with_capacity(len);
            let mut param_codes = Vec::with_capacity(len);
            let mut values = Vec::with_capacity(len);

            for n in numbers {
                resource_types.push(n.resource_type);
                resource_ids.push(n.resource_id);
                param_codes.push(n.param_code);
                values.push(n.value);
            }

            query(
                "INSERT INTO search_idx_number (\
                    resource_type, resource_id, param_code, value_num\
                ) SELECT \
                    t.rt, t.rid, t.pc, t.vn::numeric \
                FROM UNNEST(\
                    $1::text[], $2::text[], $3::text[], $4::text[]\
                ) AS t(rt, rid, pc, vn)",
            )
            .bind(&resource_types)
            .bind(&resource_ids)
            .bind(&param_codes)
            .bind(&values)
            .execute(&mut **tx)
            .await
            .map_err(|e| {
                StorageError::internal(format!("Failed to batch-write number index: {e}"))
            })?;
        }

        if !quantities.is_empty() {
            let len = quantities.len();
            let mut resource_types = Vec::with_capacity(len);
            let mut resource_ids = Vec::with_capacity(len);
            let mut param_codes = Vec::with_capacity(len);
            let mut values = Vec::with_capacity(len);
            let mut systems: Vec<Option<String>> = Vec::with_capacity(len);
            let mut codes: Vec<Option<String>> = Vec::with_capacity(len);
            let mut units: Vec<Option<String>> = Vec::with_capacity(len);

            for q in quantities {
                resource_types.push(q.resource_type);
                resource_ids.push(q.resource_id);
                param_codes.push(q.param_code);
                values.push(q.value);
                systems.push(q.system);
                codes.push(q.code);
                units.push(q.unit);
            }

            query(
                "INSERT INTO search_idx_quantity (\
                    resource_type, resource_id, param_code, value_num, system, code, unit\
                ) SELECT \
                    t.rt, t.rid, t.pc, t.vn::numeric, t.sys, t.code, t.unit \
                FROM UNNEST(\
                    $1::text[], $2::text[], $3::text[], $4::text[], \
                    $5::text[], $6::text[], $7::text[]\
                ) AS t(rt, rid, pc, vn, sys, code, unit)",
            )
            .bind(&resource_types)
            .bind(&resource_ids)
            .bind(&param_codes)
            .bind(&values)
            .bind(&systems)
            .bind(&codes)
            .bind(&units)
            .execute(&mut **tx)
            .await
            .map_err(|e| {
                StorageError::internal(format!("Failed to batch-write quantity index: {e}"))
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

    let del_string =
        query("DELETE FROM search_idx_string WHERE resource_type = $1 AND resource_id = $2")
            .bind(resource_type)
            .bind(resource_id)
            .execute(pool);
    let del_number =
        query("DELETE FROM search_idx_number WHERE resource_type = $1 AND resource_id = $2")
            .bind(resource_type)
            .bind(resource_id)
            .execute(pool);
    let del_quantity =
        query("DELETE FROM search_idx_quantity WHERE resource_type = $1 AND resource_id = $2")
            .bind(resource_type)
            .bind(resource_id)
            .execute(pool);

    let (r1, r2, r3, r4, r5) =
        tokio::join!(del_ref, del_date, del_string, del_number, del_quantity);
    r1.map_err(|e| StorageError::internal(format!("Failed to delete reference index: {e}")))?;
    r2.map_err(|e| StorageError::internal(format!("Failed to delete date index: {e}")))?;
    r3.map_err(|e| StorageError::internal(format!("Failed to delete string index: {e}")))?;
    r4.map_err(|e| StorageError::internal(format!("Failed to delete number index: {e}")))?;
    r5.map_err(|e| StorageError::internal(format!("Failed to delete quantity index: {e}")))?;

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

    query("DELETE FROM search_idx_string WHERE resource_type = $1 AND resource_id = $2")
        .bind(resource_type)
        .bind(resource_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| StorageError::internal(format!("Failed to delete string index: {e}")))?;

    query("DELETE FROM search_idx_number WHERE resource_type = $1 AND resource_id = $2")
        .bind(resource_type)
        .bind(resource_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| StorageError::internal(format!("Failed to delete number index: {e}")))?;

    query("DELETE FROM search_idx_quantity WHERE resource_type = $1 AND resource_id = $2")
        .bind(resource_type)
        .bind(resource_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| StorageError::internal(format!("Failed to delete quantity index: {e}")))?;

    Ok(())
}

/// Delete index rows for many ids of one `resource_type`, one statement per
/// sidecar. Caller groups by `resource_type` so partition pruning stays
/// effective.
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

    query(
        "DELETE FROM search_idx_string \
         WHERE resource_type = $1 AND resource_id = ANY($2)",
    )
    .bind(resource_type)
    .bind(resource_ids)
    .execute(&mut **tx)
    .await
    .map_err(|e| StorageError::internal(format!("Failed to batch-delete string index: {e}")))?;

    query(
        "DELETE FROM search_idx_number \
         WHERE resource_type = $1 AND resource_id = ANY($2)",
    )
    .bind(resource_type)
    .bind(resource_ids)
    .execute(&mut **tx)
    .await
    .map_err(|e| StorageError::internal(format!("Failed to batch-delete number index: {e}")))?;

    query(
        "DELETE FROM search_idx_quantity \
         WHERE resource_type = $1 AND resource_id = ANY($2)",
    )
    .bind(resource_type)
    .bind(resource_ids)
    .execute(&mut **tx)
    .await
    .map_err(|e| StorageError::internal(format!("Failed to batch-delete quantity index: {e}")))?;

    Ok(())
}
