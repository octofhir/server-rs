//! CRUD (Create, Read, Update, Delete) query implementations.
//!
//! This module contains the SQL queries for basic resource operations.

use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx_core::query::query;
use sqlx_core::query_as::query_as;
use sqlx_core::query_scalar::query_scalar;
use sqlx_core::sql_str::AssertSqlSafe;
use sqlx_postgres::{PgPool, PgTransaction};
use time::OffsetDateTime;
use uuid::Uuid;

use octofhir_search::ucum::canonical_quantities;
use octofhir_storage::{RawStoredResource, StorageError, StoredResource};

use crate::schema::SchemaManager;

fn mutation_error(error: sqlx_core::error::Error, context: &str) -> StorageError {
    if error
        .as_database_error()
        .and_then(|error| error.code())
        .as_deref()
        == Some("40001")
    {
        return StorageError::WriteConflict {
            message: "Resource changed after transaction planning; retry the request".into(),
        };
    }
    StorageError::internal(format!("{context}: {error}"))
}

/// Ordinary mutations share a database-wide gate; conditional interactions take
/// it exclusively before searching. A shared gate preserves ordinary write
/// concurrency while covering overlapping predicates and chained dependencies.
/// Plain INSERT/DELETE statements get the shared gate from the per-table
/// `octofhir_write_gate` statement trigger; an explicit call is only needed
/// before row locks taken ahead of the write (SELECT ... FOR UPDATE).
pub(crate) async fn lock_resource_writes(
    tx: &mut PgTransaction<'_>,
    exclusive: bool,
) -> Result<(), StorageError> {
    let sql = if exclusive {
        "SELECT pg_advisory_xact_lock(1330006098, 1)"
    } else {
        "SELECT pg_advisory_xact_lock_shared(1330006098, 1)"
    };
    query(sql).execute(&mut **tx).await.map_err(|error| {
        StorageError::transaction_error(format!("Write coordination failed: {error}"))
    })?;
    Ok(())
}

/// Converts chrono DateTime to time OffsetDateTime.
fn chrono_to_time(dt: DateTime<Utc>) -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp(dt.timestamp()).unwrap_or(OffsetDateTime::UNIX_EPOCH)
        + time::Duration::nanoseconds(dt.timestamp_subsec_nanos() as i64)
}

/// This backend issues canonical decimal BIGINT versions. Reject other spellings
/// without a database lookup, while keeping comparisons on the indexed column.
/// FHIR clients still treat versions as opaque: `01` must never alias `1`.
fn stored_version(version: &str) -> Option<i64> {
    let value = version.parse::<i64>().ok()?;
    (value.to_string() == version).then_some(value)
}

/// Creates a new transaction entry and returns its ID.
///
/// Transaction IDs (txid) are used as version IDs for FHIR resources.
pub async fn create_transaction(pool: &PgPool) -> Result<i64, StorageError> {
    let txid: i64 = query_scalar("SELECT nextval('_transaction_txid_seq') AS txid")
        .fetch_one(pool)
        .await
        .map_err(|e| StorageError::internal(format!("Failed to create transaction: {e}")))?;

    Ok(txid)
}

/// Creates a new FHIR resource in the database.
///
/// # Arguments
///
/// * `pool` - Database connection pool
/// * `schema` - Schema manager for ensuring table exists
/// * `resource` - The FHIR resource JSON to create
///
/// # Returns
///
/// Returns the created `StoredResource` with generated ID and version.
pub async fn create(pool: &PgPool, resource: &Value) -> Result<StoredResource, StorageError> {
    let resource_type = resource["resourceType"]
        .as_str()
        .ok_or_else(|| StorageError::invalid_resource("Missing or invalid resourceType field"))?;

    // Tables are created at startup by bootstrap_conformance_if_postgres()

    // Generate ID if not provided, validate if provided
    let id = if let Some(provided_id) = resource["id"].as_str() {
        // Validate FHIR ID format
        octofhir_core::validate_id(provided_id)
            .map_err(|e| StorageError::invalid_resource(format!("Invalid resource ID: {e}")))?;
        provided_id.to_string()
    } else {
        // Generate UUID by default
        octofhir_core::generate_id()
    };

    // One statement: txid from the sequence + INSERT with id/meta stamped in
    // SQL. INSERTs carry no row trigger; see `octofhir_set_resource_metadata`.
    let table = SchemaManager::table_name(resource_type);
    let sql = format!(
        r#"WITH new_tx AS (
               SELECT nextval('_transaction_txid_seq') AS txid
           ), stamp AS (SELECT octofhir_resource_timestamp() AS at)
           INSERT INTO "{table}" (id, txid, created_at, updated_at, resource, status, ucum)
           SELECT $1, new_tx.txid, stamp.at, stamp.at,
               $2::jsonb || jsonb_build_object('id', $1::text, 'meta', COALESCE($2::jsonb -> 'meta', '{{}}'::jsonb) || jsonb_build_object('versionId', new_tx.txid::text, 'lastUpdated', to_char(stamp.at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"'))),
               'created', $3::jsonb
           FROM new_tx, stamp
           RETURNING id, txid, created_at, updated_at, resource"#
    );

    let row: (String, i64, DateTime<Utc>, DateTime<Utc>, Value) =
        query_as(AssertSqlSafe(sql.to_string()))
            .bind(&id)
            .bind(resource)
            .bind(canonical_quantities(resource))
            .fetch_one(pool)
            .await
            .map_err(|e| {
                // Check for unique constraint violation
                if e.to_string().contains("duplicate key") {
                    StorageError::already_exists(resource_type, &id)
                } else {
                    mutation_error(e, "Failed to create resource")
                }
            })?;

    // Use the resource returned from DB (with meta already set)
    let resource = row.4;

    let created_at_time = chrono_to_time(row.2);
    let updated_at_time = chrono_to_time(row.3);

    Ok(StoredResource {
        id,
        version_id: row.1.to_string(),
        resource_type: resource_type.to_string(),
        resource,
        last_updated: updated_at_time,
        created_at: created_at_time,
    })
}

/// Creates a new FHIR resource and returns the result as raw JSON string.
///
/// Like `create()` but uses `resource::text` in the RETURNING clause to avoid
/// JSONB → serde_json::Value deserialization. The raw JSON string can be sent
/// directly to the HTTP response body.
pub async fn create_raw(
    pool: &PgPool,
    resource: &Value,
) -> Result<RawStoredResource, StorageError> {
    let resource_type = resource["resourceType"]
        .as_str()
        .ok_or_else(|| StorageError::invalid_resource("Missing or invalid resourceType field"))?;

    let id = if let Some(provided_id) = resource["id"].as_str() {
        octofhir_core::validate_id(provided_id)
            .map_err(|e| StorageError::invalid_resource(format!("Invalid resource ID: {e}")))?;
        provided_id.to_string()
    } else {
        octofhir_core::generate_id()
    };

    let table = SchemaManager::table_name(resource_type);
    let sql = format!(
        r#"WITH new_tx AS (
               SELECT nextval('_transaction_txid_seq') AS txid
           ), stamp AS (SELECT octofhir_resource_timestamp() AS at)
           INSERT INTO "{table}" (id, txid, created_at, updated_at, resource, status, ucum)
           SELECT $1, new_tx.txid, stamp.at, stamp.at,
               $2::jsonb || jsonb_build_object('id', $1::text, 'meta', COALESCE($2::jsonb -> 'meta', '{{}}'::jsonb) || jsonb_build_object('versionId', new_tx.txid::text, 'lastUpdated', to_char(stamp.at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"'))),
               'created', $3::jsonb
           FROM new_tx, stamp
           RETURNING id, txid, created_at, updated_at, resource::text"#
    );

    let row: (String, i64, DateTime<Utc>, DateTime<Utc>, String) =
        query_as(AssertSqlSafe(sql.to_string()))
            .bind(&id)
            .bind(resource)
            .bind(canonical_quantities(resource))
            .fetch_one(pool)
            .await
            .map_err(|e| {
                if e.to_string().contains("duplicate key") {
                    StorageError::already_exists(resource_type, &id)
                } else {
                    mutation_error(e, "Failed to create resource")
                }
            })?;

    let created_at_time = chrono_to_time(row.2);
    let updated_at_time = chrono_to_time(row.3);

    Ok(RawStoredResource {
        id,
        version_id: row.1.to_string(),
        resource_type: resource_type.to_string(),
        resource_json: row.4,
        last_updated: updated_at_time,
        created_at: created_at_time,
    })
}

/// Checks whether a live (non-deleted) resource exists, without reading its body.
pub async fn exists(pool: &PgPool, resource_type: &str, id: &str) -> Result<bool, StorageError> {
    let table = SchemaManager::table_name(resource_type);

    let sql =
        format!(r#"SELECT EXISTS(SELECT 1 FROM "{table}" WHERE id = $1 AND status != 'deleted')"#);

    let exists: (bool,) = query_as(AssertSqlSafe(sql.to_string()))
        .bind(id)
        .fetch_one(pool)
        .await
        .map_err(|e| {
            if e.to_string().contains("does not exist") {
                return StorageError::internal(format!("Table does not exist: {e}"));
            }
            StorageError::internal(format!("Failed to check resource existence: {e}"))
        })?;

    Ok(exists.0)
}

/// Batch existence check: returns the subset of `ids` present and live (not
/// deleted) in `{resource_type}`. One `id = ANY($1)` round-trip for the whole
/// slice instead of one query per id.
pub async fn exists_many(
    pool: &PgPool,
    resource_type: &str,
    ids: &[String],
) -> Result<std::collections::HashSet<String>, StorageError> {
    if ids.is_empty() {
        return Ok(std::collections::HashSet::new());
    }
    let table = SchemaManager::table_name(resource_type);
    let sql = format!(r#"SELECT id FROM "{table}" WHERE id = ANY($1) AND status != 'deleted'"#);

    let rows: Vec<(String,)> = query_as(AssertSqlSafe(sql.to_string()))
        .bind(ids)
        .fetch_all(pool)
        .await
        .map_err(|e| {
            // Missing table = none of the ids exist; not an error for validation.
            if e.to_string().contains("does not exist") {
                return StorageError::internal(format!("Table does not exist: {e}"));
            }
            StorageError::internal(format!("Failed to batch-check existence: {e}"))
        })?;

    Ok(rows.into_iter().map(|(id,)| id).collect())
}

/// Batch existence across several resource types in one round-trip.
///
/// `groups` pairs each resource type with the ids to check. Returns the found
/// references as `"Type/id"`. The per-type existence queries are combined with
/// `UNION ALL` so N distinct referenced types cost one DB round-trip (one pool
/// checkout) instead of N — which matters under write load, where the serial
/// per-type checks dominated latency and pool pressure.
///
/// If the union query fails (e.g. one referenced type has no table yet), it
/// falls back to per-type `exists_many`, treating a missing table as "none
/// exist" — matching the single-type path's behavior.
pub async fn exists_many_grouped(
    pool: &PgPool,
    groups: &[(String, Vec<String>)],
) -> Result<std::collections::HashSet<String>, StorageError> {
    // Only groups with ids to check contribute a query branch.
    let groups: Vec<&(String, Vec<String>)> =
        groups.iter().filter(|(_, ids)| !ids.is_empty()).collect();
    if groups.is_empty() {
        return Ok(std::collections::HashSet::new());
    }

    // One type: the single-table path already does one round-trip.
    if groups.len() == 1 {
        let (rt, ids) = groups[0];
        return Ok(exists_many(pool, rt, ids)
            .await?
            .into_iter()
            .map(|id| format!("{rt}/{id}"))
            .collect());
    }

    // Build `UNION ALL` of one `id = ANY($n)` existence query per type. Each
    // branch tags its rows with the group index (a bound-free integer literal),
    // so no resource-type string is interpolated into the SQL.
    let mut sql = String::new();
    for (i, (rt, _ids)) in groups.iter().enumerate() {
        let table = SchemaManager::table_name(rt);
        if i > 0 {
            sql.push_str(" UNION ALL ");
        }
        sql.push_str(&format!(
            r#"SELECT {i}::int AS grp, id FROM "{table}" WHERE id = ANY(${n}) AND status != 'deleted'"#,
            n = i + 1
        ));
    }

    let mut q = query_as::<_, (i32, String)>(AssertSqlSafe(sql));
    for (_, ids) in &groups {
        q = q.bind(ids);
    }

    match q.fetch_all(pool).await {
        Ok(rows) => Ok(rows
            .into_iter()
            .filter_map(|(grp, id)| groups.get(grp as usize).map(|(rt, _)| format!("{rt}/{id}")))
            .collect()),
        Err(_) => {
            // A branch table may not exist. Fall back to per-type checks, where
            // a missing table is treated as "none of those ids exist".
            let mut found = std::collections::HashSet::new();
            for (rt, ids) in &groups {
                match exists_many(pool, rt, ids).await {
                    Ok(ids_found) => {
                        for id in ids_found {
                            found.insert(format!("{rt}/{id}"));
                        }
                    }
                    Err(e) => {
                        tracing::debug!(resource_type = %rt, error = %e, "grouped existence fallback: skipping type");
                    }
                }
            }
            Ok(found)
        }
    }
}

/// Batch existence check on an already checked-out transaction.
///
/// The caller keeps this transaction for the following create/update, so
/// reference validation and the write consume one pool checkout.
/// Hold references stable through the following write. Sort lock acquisition
/// across tables and rows; shared locks also protect soft-delete status changes.
pub async fn lock_references_with_tx(
    tx: &mut PgTransaction<'_>,
    groups: &[(String, Vec<String>)],
) -> Result<std::collections::HashSet<String>, StorageError> {
    let mut groups: Vec<_> = groups.iter().filter(|(_, ids)| !ids.is_empty()).collect();
    groups.sort_by(|left, right| left.0.cmp(&right.0));
    let mut found = std::collections::HashSet::new();
    for (resource_type, ids) in groups {
        let table = octofhir_search::sql_builder::escape_identifier(&SchemaManager::table_name(
            resource_type,
        ))
        .map_err(|error| StorageError::invalid_resource(error.to_string()))?;
        // No row lock: many concurrent writers reference the same hot targets
        // (one Organization/Practitioner per thousands of resources), and
        // FOR SHARE from several transactions turns every such row into a
        // MultiXact. A target deleted between this check and commit is the
        // same accepted race as any reference check outside the write.
        let sql = format!(r#"SELECT id FROM {table} WHERE id = ANY($1) AND status != 'deleted'"#);
        let rows: Vec<String> = query_scalar(AssertSqlSafe(sql))
            .bind(ids)
            .fetch_all(&mut **tx)
            .await
            .map_err(|error| {
                StorageError::internal(format!("Failed to lock references: {error}"))
            })?;
        found.extend(rows.into_iter().map(|id| format!("{resource_type}/{id}")));
    }
    Ok(found)
}

/// An exact historical version remains a valid target after the live row has
/// changed or been deleted. Lock either its current row or retained history,
/// preserving exact opaque version spelling rather than accepting numeric aliases.
pub async fn lock_reference_versions_with_tx(
    tx: &mut PgTransaction<'_>,
    references: &[octofhir_core::FhirReference],
) -> Result<std::collections::HashSet<String>, StorageError> {
    if references
        .iter()
        .any(|reference| reference.version.is_none())
    {
        return Err(StorageError::invalid_resource(
            "Versioned reference requires a version",
        ));
    }
    Ok(lock_reference_snapshots_with_tx(tx, references)
        .await?
        .into_keys()
        .collect())
}

/// Compare the actual locked JSON with the body used for targetProfile checks.
/// Also covers direct SQL changes which need not allocate a new FHIR version.
pub async fn lock_reference_snapshot_with_tx(
    tx: &mut PgTransaction<'_>,
    reference: &octofhir_core::FhirReference,
) -> Result<Option<Value>, StorageError> {
    Ok(
        lock_reference_snapshots_with_tx(tx, std::slice::from_ref(reference))
            .await?
            .remove(&reference.to_versioned()),
    )
}

/// Each type uses one set-based live-row query and, when needed, one history
/// query. Duplicate references never produce an additional round trip.
/// Body snapshots back a validator verdict (targetProfile conformance read the
/// target body), so they stay row-locked until commit: a concurrent change must
/// not turn a validated reference into a non-conforming one. Plain existence
/// checks (`lock_references_with_tx`) deliberately take no locks.
pub async fn lock_reference_snapshots_with_tx(
    tx: &mut PgTransaction<'_>,
    references: &[octofhir_core::FhirReference],
) -> Result<std::collections::HashMap<String, Value>, StorageError> {
    use std::collections::{BTreeMap, BTreeSet, HashMap};
    let mut groups: BTreeMap<&str, BTreeSet<(&str, Option<i64>)>> = BTreeMap::new();
    for reference in references {
        let requested = groups.entry(&reference.resource_type).or_default();
        octofhir_core::validate_id(&reference.id)
            .map_err(|error| StorageError::invalid_resource(error.to_string()))?;
        let version = if let Some(version) = &reference.version {
            octofhir_core::validate_id(version)
                .map_err(|error| StorageError::invalid_resource(error.to_string()))?;
            let Some(value) = stored_version(version) else {
                continue;
            };
            Some(value)
        } else {
            None
        };
        requested.insert((&reference.id, version));
    }
    let mut snapshots = HashMap::new();
    let mut locked = false;
    for (resource_type, requested) in groups {
        let table_name = SchemaManager::table_name(resource_type);
        let table = octofhir_search::sql_builder::escape_identifier(&table_name)
            .map_err(|error| StorageError::invalid_resource(error.to_string()))?;
        if requested.is_empty() {
            continue;
        }
        if !locked {
            lock_resource_writes(tx, false).await?;
            locked = true;
        }
        let ids: Vec<_> = requested.iter().map(|(id, _)| *id).collect();
        let versions: Vec<_> = requested.iter().map(|(_, version)| *version).collect();
        let sql = format!(
            "SELECT r.id, wanted.version, r.resource FROM {table} r
             JOIN UNNEST($1::text[], $2::bigint[]) AS wanted(id, version)
               ON r.id=wanted.id AND (wanted.version IS NULL OR r.txid=wanted.version)
             WHERE r.status!='deleted' ORDER BY r.id, r.txid FOR SHARE OF r"
        );
        let rows: Vec<(String, Option<i64>, Value)> = query_as(AssertSqlSafe(sql))
            .bind(&ids)
            .bind(&versions)
            .fetch_all(&mut **tx)
            .await
            .map_err(|error| {
                StorageError::internal(format!("Failed to lock reference bodies: {error}"))
            })?;
        for (id, version, body) in rows {
            let reference = octofhir_core::FhirReference {
                resource_type: resource_type.into(),
                id,
                version: version.map(|value| value.to_string()),
            };
            snapshots.insert(reference.to_versioned(), body);
        }
        let missing: Vec<_> = requested
            .iter()
            .filter_map(|(id, version)| {
                let version = (*version)?;
                (!snapshots.contains_key(&format!("{resource_type}/{id}/_history/{version}")))
                    .then_some((*id, version))
            })
            .collect();
        if missing.is_empty() {
            continue;
        }
        let history =
            octofhir_search::sql_builder::escape_identifier(&format!("{table_name}_history"))
                .map_err(|error| StorageError::invalid_resource(error.to_string()))?;
        let exists: bool = query_scalar("SELECT to_regclass($1) IS NOT NULL")
            .bind(format!("public.{history}"))
            .fetch_one(&mut **tx)
            .await
            .map_err(|error| {
                StorageError::internal(format!("Failed to locate reference history: {error}"))
            })?;
        if !exists {
            continue;
        }
        let ids: Vec<_> = missing.iter().map(|(id, _)| *id).collect();
        let versions: Vec<_> = missing.iter().map(|(_, version)| *version).collect();
        let sql = format!(
            "SELECT r.id, r.txid::text, r.resource FROM {history} r
             JOIN UNNEST($1::text[], $2::bigint[]) AS wanted(id, version)
               ON r.id=wanted.id AND r.txid=wanted.version
             WHERE r.status!='deleted' ORDER BY r.id, r.txid FOR SHARE OF r"
        );
        let rows: Vec<(String, String, Value)> = query_as(AssertSqlSafe(sql))
            .bind(&ids)
            .bind(&versions)
            .fetch_all(&mut **tx)
            .await
            .map_err(|error| {
                StorageError::internal(format!("Failed to lock reference history: {error}"))
            })?;
        for (id, version, body) in rows {
            snapshots.insert(format!("{resource_type}/{id}/_history/{version}"), body);
        }
    }
    Ok(snapshots)
}

pub async fn exists_many_grouped_with_tx(
    tx: &mut PgTransaction<'_>,
    groups: &[(String, Vec<String>)],
) -> Result<std::collections::HashSet<String>, StorageError> {
    let groups: Vec<&(String, Vec<String>)> =
        groups.iter().filter(|(_, ids)| !ids.is_empty()).collect();
    if groups.is_empty() {
        return Ok(std::collections::HashSet::new());
    }

    let mut sql = String::new();
    for (i, (resource_type, _ids)) in groups.iter().enumerate() {
        let table = SchemaManager::table_name(resource_type);
        if i > 0 {
            sql.push_str(" UNION ALL ");
        }
        sql.push_str(&format!(
            r#"SELECT {i}::int AS grp, id FROM "{table}" WHERE id = ANY(${n}) AND status != 'deleted'"#,
            n = i + 1
        ));
    }

    let mut query = query_as::<_, (i32, String)>(AssertSqlSafe(sql));
    for (_, ids) in &groups {
        query = query.bind(ids);
    }

    let rows = query
        .fetch_all(&mut **tx)
        .await
        .map_err(|e| StorageError::internal(format!("Failed to batch-check references: {e}")))?;

    Ok(rows
        .into_iter()
        .filter_map(|(group, id)| {
            groups
                .get(group as usize)
                .map(|(resource_type, _)| format!("{resource_type}/{id}"))
        })
        .collect())
}

/// Batch read of the profiles referenced resources declare, without reading
/// their bodies.
///
/// Returns `"Type/id" -> meta.profile` for every live row that declares at
/// least one profile. Rows with no declared profile are simply absent — the
/// caller cannot distinguish them from missing ids here, and does not need to:
/// existence is settled separately, and an unprofiled target falls back to the
/// slow path either way.
///
/// This backs `targetProfile` conformance. The declared canonicals are the
/// whole answer for that check, and they live in the generated `profile`
/// column, so the query is served entirely from
/// `idx_{table}_profile_cov` — the `profile IS NOT NULL` predicate is what
/// makes that partial index eligible. Reading the row instead would drag the
/// full `resource` JSONB (plus TOAST) through the heap for a set-membership
/// test.
pub async fn profiles_many_grouped(
    pool: &PgPool,
    groups: &[(String, Vec<String>)],
) -> Result<std::collections::HashMap<String, Vec<String>>, StorageError> {
    let groups: Vec<&(String, Vec<String>)> =
        groups.iter().filter(|(_, ids)| !ids.is_empty()).collect();
    if groups.is_empty() {
        return Ok(std::collections::HashMap::new());
    }

    let mut sql = String::new();
    for (i, (resource_type, _ids)) in groups.iter().enumerate() {
        let table = SchemaManager::table_name(resource_type);
        if i > 0 {
            sql.push_str(" UNION ALL ");
        }
        sql.push_str(&format!(
            r#"SELECT {i}::int AS grp, id, profile FROM "{table}" WHERE id = ANY(${n}) AND profile IS NOT NULL AND status != 'deleted'"#,
            n = i + 1
        ));
    }

    let mut query = query_as::<_, (i32, String, Vec<String>)>(AssertSqlSafe(sql));
    for (_, ids) in &groups {
        query = query.bind(ids);
    }

    // A missing table (or any other failure) degrades to "no profiles known",
    // which sends every reference down the fetch-and-validate path — the
    // behaviour before this fast path existed. A fast path must never be able
    // to turn a storage hiccup into a validation error.
    let rows = match query.fetch_all(pool).await {
        Ok(rows) => rows,
        Err(e) => {
            tracing::debug!(error = %e, "batch profile read failed; falling back to full validation");
            return Ok(std::collections::HashMap::new());
        }
    };

    Ok(rows
        .into_iter()
        .filter_map(|(group, id, profiles)| {
            groups
                .get(group as usize)
                .map(|(resource_type, _)| (format!("{resource_type}/{id}"), profiles))
        })
        .collect())
}

/// Batch read of live (non-deleted) resources for `ids` in `{resource_type}`.
/// One `id = ANY($1)` round-trip; missing/deleted ids are omitted. Each returned
/// resource preserves its saved JSON, including versionId and lastUpdated.
pub async fn read_many(
    pool: &PgPool,
    resource_type: &str,
    ids: &[String],
) -> Result<Vec<StoredResource>, StorageError> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let table = SchemaManager::table_name(resource_type);
    let sql = format!(
        r#"SELECT id, txid, created_at, updated_at, resource
           FROM "{table}"
           WHERE id = ANY($1) AND status != 'deleted'"#
    );

    let rows: Vec<(String, i64, DateTime<Utc>, DateTime<Utc>, Value)> =
        query_as(AssertSqlSafe(sql.to_string()))
            .bind(ids)
            .fetch_all(pool)
            .await
            .map_err(|e| {
                if e.to_string().contains("does not exist") {
                    return StorageError::internal(format!("Table does not exist: {e}"));
                }
                StorageError::internal(format!("Failed to batch-read resources: {e}"))
            })?;

    let out = rows
        .into_iter()
        .map(|(row_id, txid, created_at, updated_at, resource)| {
            let created_at_time = chrono_to_time(created_at);
            let updated_at_time = chrono_to_time(updated_at);
            // Preserve the JSON of this stored version, including its metadata.
            StoredResource {
                id: row_id,
                version_id: txid.to_string(),
                resource_type: resource_type.to_string(),
                resource,
                last_updated: updated_at_time,
                created_at: created_at_time,
            }
        })
        .collect();

    Ok(out)
}

/// Reads a FHIR resource by type and ID.
///
/// Returns `None` if the resource doesn't exist.
/// Returns `StorageError::Deleted` if the resource has been soft-deleted.
pub async fn read(
    pool: &PgPool,
    resource_type: &str,
    id: &str,
) -> Result<Option<StoredResource>, StorageError> {
    let table = SchemaManager::table_name(resource_type);

    // Query including status to detect deleted resources
    let sql = format!(
        r#"SELECT id, txid, created_at, updated_at, resource, status::text
           FROM "{table}"
           WHERE id = $1"#
    );

    let row: Option<(String, i64, DateTime<Utc>, DateTime<Utc>, Value, String)> =
        query_as(AssertSqlSafe(sql.to_string()))
            .bind(id)
            .fetch_optional(pool)
            .await
            .map_err(|e| {
                // Table might not exist - return None instead of error
                if e.to_string().contains("does not exist") {
                    return StorageError::internal(format!("Table does not exist: {e}"));
                }
                StorageError::internal(format!("Failed to read resource: {e}"))
            })?;

    match row {
        Some((row_id, txid, created_at, updated_at, resource, status)) => {
            // Check if the resource is soft-deleted
            if status == "deleted" {
                return Err(StorageError::deleted(resource_type, id));
            }

            let created_at_time = chrono_to_time(created_at);
            let updated_at_time = chrono_to_time(updated_at);

            // The stored JSON already contains this version's server metadata.
            Ok(Some(StoredResource {
                id: row_id,
                version_id: txid.to_string(),
                resource_type: resource_type.to_string(),
                resource,
                last_updated: updated_at_time,
                created_at: created_at_time,
            }))
        }
        // Absent from the live table: never created (404) or deleted (410).
        // Deletes archive their versions to history, so a history hit means Gone.
        None if history_has(pool, &table, id).await? => {
            Err(StorageError::deleted(resource_type, id))
        }
        None => Ok(None),
    }
}

/// Reads a FHIR resource as raw JSON string, avoiding serde_json::Value round-trip.
///
/// Uses `resource::text` to get the JSON directly as a string from PostgreSQL,
/// skipping JSONB → serde_json::Value deserialization entirely.
/// The resource JSON in the DB already contains correct meta (versionId, lastUpdated)
/// from create/update operations.
pub async fn read_raw(
    pool: &PgPool,
    resource_type: &str,
    id: &str,
) -> Result<Option<RawStoredResource>, StorageError> {
    let table = SchemaManager::table_name(resource_type);

    let sql = format!(
        r#"SELECT id, txid, created_at, updated_at, resource::text, status::text
           FROM "{table}"
           WHERE id = $1"#
    );

    let row: Option<(String, i64, DateTime<Utc>, DateTime<Utc>, String, String)> =
        query_as(AssertSqlSafe(sql.to_string()))
            .bind(id)
            .fetch_optional(pool)
            .await
            .map_err(|e| {
                if e.to_string().contains("does not exist") {
                    return StorageError::internal(format!("Table does not exist: {e}"));
                }
                StorageError::internal(format!("Failed to read resource: {e}"))
            })?;

    match row {
        Some((row_id, txid, created_at, updated_at, resource_json, status)) => {
            if status == "deleted" {
                return Err(StorageError::deleted(resource_type, id));
            }

            let created_at_time = chrono_to_time(created_at);
            let updated_at_time = chrono_to_time(updated_at);

            Ok(Some(RawStoredResource {
                id: row_id,
                version_id: txid.to_string(),
                resource_type: resource_type.to_string(),
                resource_json,
                last_updated: updated_at_time,
                created_at: created_at_time,
            }))
        }
        // Absent from the live table: never created (404) or deleted (410).
        // Deletes archive their versions to history, so a history hit means Gone.
        None if history_has(pool, &table, id).await? => {
            Err(StorageError::deleted(resource_type, id))
        }
        None => Ok(None),
    }
}

/// True if any version of `id` exists in `{table}_history` — i.e. the resource
/// once existed and was deleted (the live row is gone). Used to return 410 Gone
/// instead of 404, since deletes physically remove the live row and archive it
/// to history.
async fn history_has(pool: &PgPool, table: &str, id: &str) -> Result<bool, StorageError> {
    let sql = format!(r#"SELECT EXISTS(SELECT 1 FROM "{table}_history" WHERE id = $1)"#);
    let exists: bool = query_scalar(AssertSqlSafe(sql))
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap_or(false);
    Ok(exists)
}

/// Updates a live resource in one statement: the previous row is selected
/// `FOR UPDATE` in a CTE and returned next to the new one, so callers get
/// the replaced body without a surrounding transaction. `If-Match` is folded
/// into the join; a version mismatch is told apart from a missing row by a
/// second lookup only on that failure path.
pub async fn update_snapshot(
    pool: &PgPool,
    resource: &Value,
    if_match: Option<&str>,
) -> Result<octofhir_storage::RawUpdatedResource, StorageError> {
    let resource_type = resource["resourceType"]
        .as_str()
        .ok_or_else(|| StorageError::invalid_resource("Missing or invalid resourceType field"))?;
    let id = resource["id"]
        .as_str()
        .ok_or_else(|| StorageError::invalid_resource("Missing id field"))?;
    let table = SchemaManager::table_name(resource_type);
    let version_check = match if_match {
        Some(version) => match stored_version(version) {
            Some(version) => format!("AND p.txid = {version}::bigint"),
            None => "AND FALSE".into(),
        },
        None => String::new(),
    };
    let sql = format!(
        r#"WITH previous AS MATERIALIZED (
               SELECT id, txid, created_at, updated_at, resource FROM "{table}"
               WHERE id = $1 AND status != 'deleted' FOR UPDATE
           ), new_tx AS MATERIALIZED (SELECT nextval('_transaction_txid_seq') AS txid FROM previous)
           UPDATE "{table}" AS t SET txid = new_tx.txid, resource = $2::jsonb, ucum = $3::jsonb, status = 'updated'
           FROM previous AS p, new_tx WHERE t.id = p.id {version_check}
           RETURNING t.id, t.txid, t.created_at, t.updated_at, t.resource::text,
                     p.txid, p.created_at, p.updated_at, p.resource"#
    );
    type Row = (
        String,
        i64,
        DateTime<Utc>,
        DateTime<Utc>,
        String,
        i64,
        DateTime<Utc>,
        DateTime<Utc>,
        Value,
    );
    let row: Option<Row> = query_as(AssertSqlSafe(sql))
        .bind(id)
        .bind(resource)
        .bind(canonical_quantities(resource))
        .fetch_optional(pool)
        .await
        .map_err(|error| mutation_error(error, "Failed to update resource"))?;
    if let Some((
        id,
        version,
        created,
        updated,
        resource_json,
        old_version,
        old_created,
        old_updated,
        old_resource,
    )) = row
    {
        return Ok(octofhir_storage::RawUpdatedResource {
            previous: StoredResource {
                id: id.clone(),
                version_id: old_version.to_string(),
                resource_type: resource_type.into(),
                resource: old_resource,
                created_at: chrono_to_time(old_created),
                last_updated: chrono_to_time(old_updated),
            },
            stored: RawStoredResource {
                id,
                version_id: version.to_string(),
                resource_type: resource_type.into(),
                resource_json,
                created_at: chrono_to_time(created),
                last_updated: chrono_to_time(updated),
            },
        });
    }
    if let Some(expected) = if_match {
        let current: Option<i64> = query_scalar(AssertSqlSafe(format!(
            r#"SELECT txid FROM "{table}" WHERE id = $1 AND status != 'deleted'"#
        )))
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(|error| mutation_error(error, "Failed to check update target"))?;
        if let Some(current) = current {
            return Err(StorageError::version_conflict(
                expected,
                current.to_string(),
            ));
        }
    }
    Err(StorageError::not_found(resource_type, id))
}

pub async fn update(
    pool: &PgPool,
    resource: &Value,
    if_match: Option<&str>,
) -> Result<StoredResource, StorageError> {
    let updated = update_snapshot(pool, resource, if_match).await?;
    let stored = updated.stored;
    Ok(StoredResource {
        resource: serde_json::from_str(&stored.resource_json)
            .map_err(|error| StorageError::internal(format!("Invalid stored JSON: {error}")))?,
        id: stored.id,
        version_id: stored.version_id,
        resource_type: stored.resource_type,
        created_at: stored.created_at,
        last_updated: stored.last_updated,
    })
}

pub async fn update_raw(
    pool: &PgPool,
    resource: &Value,
    if_match: Option<&str>,
) -> Result<RawStoredResource, StorageError> {
    Ok(update_snapshot(pool, resource, if_match).await?.stored)
}

/// Soft deletes a FHIR resource.
///
/// The row is physically removed from the live table; the `BEFORE DELETE`
/// history trigger archives the final version into `{table}_history`, which is
/// what makes a later GET return 410 Gone (row absent from live, present in
/// history) rather than 404. Keeping deleted rows in the live table would let
/// them accumulate without bound (they are live rows, not dead tuples, so
/// autovacuum cannot reclaim them), bloating the heap and the GIN index and
/// degrading every read and write over time.
///
/// Per FHIR spec, delete is idempotent - deleting a non-existent or
/// already deleted resource returns success (204 No Content).
pub async fn delete(pool: &PgPool, resource_type: &str, id: &str) -> Result<(), StorageError> {
    delete_returning_previous(pool, resource_type, id)
        .await
        .map(|_| ())
}

/// Same single DELETE statement, returning the live body it removed so event
/// consumers see the previous state. None = nothing was live (idempotent).
pub async fn delete_returning_previous(
    pool: &PgPool,
    resource_type: &str,
    id: &str,
) -> Result<Option<StoredResource>, StorageError> {
    let table = SchemaManager::table_name(resource_type);
    let sql = format!(
        r#"DELETE FROM "{table}" WHERE id = $1 RETURNING id, txid, created_at, updated_at, resource"#
    );
    let row: Option<(String, i64, DateTime<Utc>, DateTime<Utc>, Value)> =
        query_as(AssertSqlSafe(sql))
            .bind(id)
            .fetch_optional(pool)
            .await
            .map_err(|e| {
                if e.to_string().contains("does not exist") {
                    return StorageError::internal(format!("Table does not exist: {e}"));
                }
                mutation_error(e, "Failed to delete resource")
            })?;
    Ok(row.map(
        |(id, txid, created_at, updated_at, resource)| StoredResource {
            id,
            version_id: txid.to_string(),
            resource_type: resource_type.into(),
            resource,
            created_at: chrono_to_time(created_at),
            last_updated: chrono_to_time(updated_at),
        },
    ))
}

/// Internal retained snapshot, including deletion attributes for exact policy
/// checks. Deletion snapshots are not representations returned by vread.
pub async fn version_snapshot<'c, E>(
    executor: E,
    resource_type: &str,
    id: &str,
    version: &str,
) -> Result<Option<octofhir_storage::RawHistoryEntry>, StorageError>
where
    E: sqlx_core::executor::Executor<'c, Database = sqlx_postgres::Postgres>,
{
    resource_snapshot(executor, resource_type, id, Some(version)).await
}

pub async fn resource_snapshot<'c, E>(
    executor: E,
    resource_type: &str,
    id: &str,
    version: Option<&str>,
) -> Result<Option<octofhir_storage::RawHistoryEntry>, StorageError>
where
    E: sqlx_core::executor::Executor<'c, Database = sqlx_postgres::Postgres>,
{
    if let Some(version) = version {
        octofhir_core::validate_id(version).map_err(|error| {
            StorageError::invalid_resource(format!("Invalid version ID: {error}"))
        })?;
    }
    let stored = version.and_then(stored_version);
    if version.is_some() && stored.is_none() {
        return Ok(None);
    }
    let table = SchemaManager::table_name(resource_type);
    let sql = if version.is_none() {
        format!(
            r#"SELECT id, txid, created_at, updated_at, resource::text, status FROM (
        SELECT id, txid, created_at, updated_at, resource, status::text, true AS current FROM "{table}" WHERE id=$1
        UNION ALL SELECT id, txid, created_at, updated_at, resource, 'deleted'::text AS status, false AS current FROM "{table}_history" WHERE id=$1
        ) AS snapshots ORDER BY current DESC, txid DESC LIMIT 1"#
        )
    } else {
        format!(
            r#"SELECT id, txid, created_at, updated_at, resource::text, status::text FROM "{table}" WHERE id=$1 AND txid=$2
        UNION ALL SELECT id, txid, created_at, updated_at, resource::text, status::text FROM "{table}_history" WHERE id=$1 AND txid=$2 LIMIT 1"#
        )
    };
    type Row = (String, i64, DateTime<Utc>, DateTime<Utc>, String, String);
    let query = query_as(AssertSqlSafe(sql)).bind(id);
    let query = if let Some(version) = stored {
        query.bind(version)
    } else {
        query
    };
    let row: Option<Row> = query.fetch_optional(executor).await.map_err(|error| {
        StorageError::internal(format!("Version snapshot read failed: {error}"))
    })?;
    Ok(row.map(
        |(id, txid, created_at, updated_at, resource_json, status)| {
            octofhir_storage::RawHistoryEntry {
                resource: RawStoredResource {
                    id,
                    version_id: txid.to_string(),
                    resource_type: resource_type.into(),
                    resource_json,
                    last_updated: chrono_to_time(updated_at),
                    created_at: chrono_to_time(created_at),
                },
                method: match status.as_str() {
                    "created" => octofhir_storage::HistoryMethod::Create,
                    "deleted" => octofhir_storage::HistoryMethod::Delete,
                    _ => octofhir_storage::HistoryMethod::Update,
                },
            }
        },
    ))
}

fn live_version(
    snapshot: Option<octofhir_storage::RawHistoryEntry>,
    resource_type: &str,
    id: &str,
) -> Result<Option<RawStoredResource>, StorageError> {
    match snapshot {
        Some(snapshot) if snapshot.method == octofhir_storage::HistoryMethod::Delete => {
            Err(StorageError::deleted(resource_type, id))
        }
        Some(snapshot) => Ok(Some(snapshot.resource)),
        None => Ok(None),
    }
}

fn typed_version(raw: Option<RawStoredResource>) -> Result<Option<StoredResource>, StorageError> {
    raw.map(|row| {
        Ok(StoredResource {
            resource: serde_json::from_str(&row.resource_json)
                .map_err(|error| StorageError::internal(error.to_string()))?,
            id: row.id,
            version_id: row.version_id,
            resource_type: row.resource_type,
            last_updated: row.last_updated,
            created_at: row.created_at,
        })
    })
    .transpose()
}

/// Read an exact retained live revision; deletion revisions return Gone.
pub async fn vread(
    pool: &PgPool,
    resource_type: &str,
    id: &str,
    version: &str,
) -> Result<Option<StoredResource>, StorageError> {
    typed_version(vread_raw(pool, resource_type, id, version).await?)
}

pub async fn vread_with_tx(
    tx: &mut PgTransaction<'_>,
    resource_type: &str,
    id: &str,
    version: &str,
) -> Result<Option<StoredResource>, StorageError> {
    typed_version(live_version(
        version_snapshot(&mut **tx, resource_type, id, version).await?,
        resource_type,
        id,
    )?)
}

/// Raw JSON remains exactly as saved, including historical revisions.
pub async fn vread_raw(
    pool: &PgPool,
    resource_type: &str,
    id: &str,
    version: &str,
) -> Result<Option<RawStoredResource>, StorageError> {
    live_version(
        version_snapshot(pool, resource_type, id, version).await?,
        resource_type,
        id,
    )
}

// ============================================================================
// Transaction-aware CRUD operations
// ============================================================================

/// Creates a new resource within a transaction.
///
/// This variant uses an existing transaction instead of the pool,
/// allowing multiple operations to be grouped atomically.
/// Uses SQL `jsonb_set` to inject id/meta without cloning the resource in Rust.
///
/// When `txid` is `Some(t)`, the provided transaction id is reused (no new
/// `_transaction` row is created). This is the hot path for Bundle imports —
/// one txid is allocated once for the whole bundle and shared across all
/// resource inserts. When `txid` is `None`, a new `_transaction` row is
/// allocated via CTE (legacy single-resource behaviour).
pub async fn create_with_tx(
    tx: &mut PgTransaction<'_>,
    resource: &Value,
    txid: Option<i64>,
) -> Result<StoredResource, StorageError> {
    let resource_type = resource["resourceType"]
        .as_str()
        .ok_or_else(|| StorageError::invalid_resource("Missing or invalid resourceType field"))?;

    // Generate ID if not provided. v7 keeps the TEXT primary key time-ordered
    // so inserts stay at the right edge of the btree.
    let id = resource["id"]
        .as_str()
        .map(String::from)
        .unwrap_or_else(|| Uuid::now_v7().to_string());

    let table = SchemaManager::table_name(resource_type);
    let sql = if let Some(t) = txid {
        // A version reserved before this statement could trail a revision
        // another writer retained in the meantime (delete + resurrect); the
        // anti-join on the history key refuses such an out-of-order insert.
        let guard = if SchemaManager::skips_history(&table) {
            String::new()
        } else {
            format!(
                r#"WHERE NOT EXISTS (SELECT 1 FROM "{table}_history" h WHERE h.id = $1 AND h.txid >= {t}::bigint)"#
            )
        };
        format!(
            r#"WITH stamp AS (SELECT octofhir_resource_timestamp() AS at)
               INSERT INTO "{table}" (id, txid, created_at, updated_at, resource, status, ucum)
               SELECT $1, {t}::bigint, stamp.at, stamp.at,
                   $2::jsonb || jsonb_build_object('id', $1::text, 'meta', COALESCE($2::jsonb -> 'meta', '{{}}'::jsonb) || jsonb_build_object('versionId', '{t}', 'lastUpdated', to_char(stamp.at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"'))),
                   'created', $3::jsonb
               FROM stamp
               {guard}
               RETURNING id, txid, created_at, updated_at, resource"#
        )
    } else {
        format!(
            r#"WITH new_tx AS (
                   SELECT nextval('_transaction_txid_seq') AS txid
               ), stamp AS (SELECT octofhir_resource_timestamp() AS at)
               INSERT INTO "{table}" (id, txid, created_at, updated_at, resource, status, ucum)
               SELECT $1, new_tx.txid, stamp.at, stamp.at,
                   $2::jsonb || jsonb_build_object('id', $1::text, 'meta', COALESCE($2::jsonb -> 'meta', '{{}}'::jsonb) || jsonb_build_object('versionId', new_tx.txid::text, 'lastUpdated', to_char(stamp.at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"'))),
                   'created', $3::jsonb
               FROM new_tx, stamp
               RETURNING id, txid, created_at, updated_at, resource"#
        )
    };

    let row: (String, i64, DateTime<Utc>, DateTime<Utc>, Value) =
        query_as(AssertSqlSafe(sql.to_string()))
            .bind(&id)
            .bind(resource)
            .bind(canonical_quantities(resource))
            .fetch_optional(&mut **tx)
            .await
            .map_err(|e| {
                if e.to_string().contains("duplicate key") {
                    StorageError::already_exists(resource_type, &id)
                } else {
                    mutation_error(e, "Failed to create resource")
                }
            })?
            .ok_or_else(|| StorageError::WriteConflict {
                message: "Resource changed after transaction planning; retry the request".into(),
            })?;

    let resource = row.4;
    let created_at_time = chrono_to_time(row.2);
    let updated_at_time = chrono_to_time(row.3);

    Ok(StoredResource {
        id,
        version_id: row.1.to_string(),
        resource_type: resource_type.to_string(),
        resource,
        last_updated: updated_at_time,
        created_at: created_at_time,
    })
}

/// Bulk-creates many resources of the same type within a transaction.
///
/// All resources must share the same `resourceType`. The provided `txid` is
/// reused for every row (no `_transaction` rows are allocated). One `INSERT
/// ... SELECT FROM UNNEST(...)` round-trip is used for the whole batch and
/// no JSONB is pulled back over the wire — `id` / `meta` are injected
/// client-side so the returned `StoredResource` reflects what's stored.
///
/// IDs are taken from `resource["id"]` if present, otherwise generated.
pub async fn create_batch_with_tx(
    tx: &mut PgTransaction<'_>,
    resource_type: &str,
    txid: i64,
    resources: Vec<Value>,
) -> Result<Vec<StoredResource>, StorageError> {
    if resources.is_empty() {
        return Ok(Vec::new());
    }

    // id/meta are stamped by the row trigger; bodies go to Postgres as received.
    let ids: Vec<String> = resources
        .iter()
        .map(|r| {
            r["id"]
                .as_str()
                .map(String::from)
                .unwrap_or_else(|| Uuid::now_v7().to_string())
        })
        .collect();
    let payloads: &[Value] = &resources;

    let table = SchemaManager::table_name(resource_type);
    let guard = if SchemaManager::skips_history(&table) {
        String::new()
    } else {
        format!(
            r#"WHERE NOT EXISTS (SELECT 1 FROM "{table}_history" h WHERE h.id = t.id AND h.txid >= {txid}::bigint)"#
        )
    };
    let sql = format!(
        r#"WITH stamp AS MATERIALIZED (
               SELECT at, to_char(at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"') AS last_updated
               FROM (SELECT octofhir_resource_timestamp() AS at) now
           )
           INSERT INTO "{table}" (id, txid, created_at, updated_at, resource, status, ucum)
           SELECT t.id, {txid}::bigint, stamp.at, stamp.at,
               -- Bundle bodies already carry their reserved id/meta; rebuild
               -- the document only when they do not.
               CASE WHEN t.resource ->> 'id' = t.id
                     AND t.resource -> 'meta' ->> 'versionId' = '{txid}'
                     AND t.resource -> 'meta' ->> 'lastUpdated' = stamp.last_updated
                    THEN t.resource
                    ELSE t.resource || jsonb_build_object('id', t.id, 'meta',
                       COALESCE(t.resource -> 'meta', '{{}}'::jsonb) || jsonb_build_object(
                           'versionId', '{txid}', 'lastUpdated', stamp.last_updated))
               END,
               'created', t.ucum
           FROM UNNEST($1::text[], $2::jsonb[], $3::jsonb[]) AS t(id, resource, ucum), stamp
           {guard}"#
    );

    // The trigger only adds id/meta.versionId/meta.lastUpdated, all of which
    // come back as columns. Rebuilding the body from the request avoids
    // shipping and re-parsing every stored document of the batch.
    let sql = format!("{sql} RETURNING id, txid, created_at, updated_at");
    let rows: Vec<(String, i64, DateTime<Utc>, DateTime<Utc>)> = query_as(AssertSqlSafe(sql))
        .bind(&ids)
        .bind(payloads)
        .bind(
            payloads
                .iter()
                .map(canonical_quantities)
                .collect::<Vec<_>>(),
        )
        .fetch_all(&mut **tx)
        .await
        .map_err(|e| {
            if e.to_string().contains("duplicate key") {
                StorageError::already_exists(resource_type, "<batch>")
            } else {
                mutation_error(e, "Failed to batch-create resources")
            }
        })?;
    if rows.len() != ids.len() {
        // Only the history anti-join can shorten the result.
        return Err(StorageError::WriteConflict {
            message: "Resource changed after transaction planning; retry the request".into(),
        });
    }
    let position: std::collections::HashMap<String, usize> = ids
        .into_iter()
        .enumerate()
        .map(|(index, id)| (id, index))
        .collect();
    let mut bodies: Vec<Option<Value>> = resources.into_iter().map(Some).collect();
    rows.into_iter()
        .map(|(id, version, created_at, updated_at)| {
            let source = position
                .get(id.as_str())
                .and_then(|index| bodies[*index].take())
                .ok_or_else(|| StorageError::internal("Batch insert returned an unknown id"))?;
            let resource = stamp_stored_body(source, &id, version, updated_at)?;
            Ok(StoredResource {
                id,
                version_id: version.to_string(),
                resource_type: resource_type.to_owned(),
                resource,
                created_at: chrono_to_time(created_at),
                last_updated: chrono_to_time(updated_at),
            })
        })
        .collect()
}

/// Mirror of `octofhir_set_resource_metadata`: top-level id and meta are
/// replaced, meta's other members are kept.
fn stamp_stored_body(
    mut body: Value,
    id: &str,
    version: i64,
    updated_at: DateTime<Utc>,
) -> Result<Value, StorageError> {
    let object = body
        .as_object_mut()
        .ok_or_else(|| StorageError::invalid_resource("Resource must be a JSON object"))?;
    object.insert("id".into(), Value::String(id.to_owned()));
    let mut meta = match object.remove("meta") {
        Some(Value::Object(meta)) => meta,
        _ => serde_json::Map::new(),
    };
    meta.insert("versionId".into(), Value::String(version.to_string()));
    meta.insert(
        "lastUpdated".into(),
        Value::String(updated_at.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()),
    );
    object.insert("meta".into(), Value::Object(meta));
    Ok(body)
}

/// Bulk-updates many resources of the same type within a transaction.
///
/// One `UPDATE ... FROM UNNEST(...)` round-trip replaces N single-row updates.
/// The caller must guarantee the ids are distinct: a set-based UPDATE touches
/// each row once, so two revisions of the same id would silently collapse.
///
/// Ids that match no live row are returned in `missing` rather than raising
/// `NotFound`, so the caller can apply FHIR update-as-create semantics with a
/// follow-up `create_batch_with_tx`.
///
/// Optimistic locking is not supported here — entries carrying `If-Match` must
/// go through [`update_with_tx_if_match`], which reports version conflicts
/// per resource.
pub async fn update_batch_with_tx(
    tx: &mut PgTransaction<'_>,
    resource_type: &str,
    txid: i64,
    resources: &[Value],
) -> Result<(Vec<StoredResource>, Vec<StoredResource>, Vec<String>), StorageError> {
    lock_resource_writes(tx, false).await?;
    if resources.is_empty() {
        return Ok((Vec::new(), Vec::new(), Vec::new()));
    }

    let mut ids: Vec<String> = Vec::with_capacity(resources.len());
    let mut payloads: Vec<Value> = Vec::with_capacity(resources.len());
    for r in resources {
        let id = r["id"]
            .as_str()
            .ok_or_else(|| StorageError::invalid_resource("Missing id field"))?;
        ids.push(id.to_string());
        payloads.push(r.clone());
    }

    let table = SchemaManager::table_name(resource_type);

    // `updated_at` is maintained by the `{table}_update_timestamp` BEFORE UPDATE
    // trigger; the same trigger chain archives the previous version to history.
    let sql = format!(
        r#"WITH previous AS MATERIALIZED (
               SELECT id, txid, created_at, updated_at, resource FROM "{table}"
               WHERE id = ANY($1::text[]) AND status != 'deleted' ORDER BY id FOR UPDATE
           )
           UPDATE "{table}" t
           SET txid = {txid}::bigint,
               resource = u.resource,
               ucum = u.ucum,
               status = 'updated'
           FROM UNNEST($1::text[], $2::jsonb[], $3::jsonb[]) AS u(id, resource, ucum), previous p
           WHERE t.id = u.id AND t.id = p.id
           RETURNING t.id, t.created_at, t.updated_at, t.resource, p.txid, p.created_at, p.updated_at, p.resource"#
    );

    type UpdateRow = (
        String,
        DateTime<Utc>,
        DateTime<Utc>,
        Value,
        i64,
        DateTime<Utc>,
        DateTime<Utc>,
        Value,
    );
    let rows: Vec<UpdateRow> = query_as(AssertSqlSafe(sql.to_string()))
        .bind(&ids)
        .bind(&payloads)
        .bind(
            payloads
                .iter()
                .map(canonical_quantities)
                .collect::<Vec<_>>(),
        )
        .fetch_all(&mut **tx)
        .await
        .map_err(|e| mutation_error(e, "Failed to batch-update resources"))?;

    let matched: std::collections::HashSet<&str> = rows.iter().map(|r| r.0.as_str()).collect();
    let missing: Vec<String> = ids
        .iter()
        .filter(|id| !matched.contains(id.as_str()))
        .cloned()
        .collect();

    let version_id = txid.to_string();
    let mut updated = Vec::with_capacity(rows.len());
    let mut previous = Vec::with_capacity(rows.len());
    for (
        id,
        created_at,
        updated_at,
        resource,
        old_txid,
        old_created_at,
        old_updated_at,
        old_resource,
    ) in rows
    {
        previous.push(StoredResource {
            id: id.clone(),
            version_id: old_txid.to_string(),
            resource_type: resource_type.into(),
            resource: old_resource,
            last_updated: chrono_to_time(old_updated_at),
            created_at: chrono_to_time(old_created_at),
        });
        updated.push(StoredResource {
            id,
            version_id: version_id.clone(),
            resource_type: resource_type.into(),
            resource,
            last_updated: chrono_to_time(updated_at),
            created_at: chrono_to_time(created_at),
        });
    }
    Ok((updated, previous, missing))
}

/// Bulk-deletes many resources of the same type within a transaction.
///
/// One `DELETE ... WHERE id = ANY(...)` round-trip replaces N single-row
/// deletes. As with [`delete_with_tx`], the live row is physically removed and
/// the BEFORE DELETE trigger archives the final version to history. Ids that
/// do not exist are ignored — FHIR delete is idempotent.
/// RETURNING supplies actual deleted identities without a separate lookup.
pub async fn delete_batch_with_tx(
    tx: &mut PgTransaction<'_>,
    resource_type: &str,
    ids: &[String],
) -> Result<Vec<String>, StorageError> {
    Ok(delete_batch_snapshots_with_tx(tx, resource_type, ids)
        .await?
        .into_iter()
        .map(|deleted| deleted.previous.id)
        .collect())
}

pub async fn delete_batch_snapshots_with_tx(
    tx: &mut PgTransaction<'_>,
    resource_type: &str,
    ids: &[String],
) -> Result<Vec<octofhir_storage::DeletedResource>, StorageError> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }

    let table = SchemaManager::table_name(resource_type);
    let sql = format!(
        r#"DELETE FROM "{table}" WHERE id = ANY($1::text[]) RETURNING id, txid, created_at, updated_at, resource, current_setting('octofhir.write_version')::bigint, octofhir_resource_timestamp()"#
    );

    type DeletedRow = (
        String,
        i64,
        DateTime<Utc>,
        DateTime<Utc>,
        Value,
        i64,
        DateTime<Utc>,
    );
    let rows: Vec<DeletedRow> = query_as(AssertSqlSafe(sql))
        .bind(ids)
        .fetch_all(&mut **tx)
        .await
        .map_err(|e| {
            if e.to_string().contains("does not exist") {
                return StorageError::internal(format!("Table does not exist: {e}"));
            }
            mutation_error(e, "Failed to batch-delete resources")
        })?;
    Ok(rows
        .into_iter()
        .map(
            |(id, txid, created_at, updated_at, resource, deletion_version, deletion_timestamp)| {
                octofhir_storage::DeletedResource {
                    previous: StoredResource {
                        id,
                        version_id: txid.to_string(),
                        resource_type: resource_type.into(),
                        resource,
                        created_at: chrono_to_time(created_at),
                        last_updated: chrono_to_time(updated_at),
                    },
                    version_id: deletion_version.to_string(),
                    last_updated: chrono_to_time(deletion_timestamp),
                }
            },
        )
        .collect())
}

/// Updates a resource within a transaction.
///
/// Uses SQL `jsonb_set` to inject meta without cloning the resource in Rust.
pub async fn update_with_tx(
    tx: &mut PgTransaction<'_>,
    resource: &Value,
) -> Result<StoredResource, StorageError> {
    update_with_tx_if_match(tx, resource, None, None).await
}

/// Updates a resource within a transaction with optional optimistic locking
/// and an optional pre-allocated `txid`.
///
/// When `txid` is `Some(t)`, the provided transaction id is reused (no new
/// `_transaction` row is created). When `txid` is `None`, a new `_transaction`
/// row is allocated via CTE.
pub async fn update_with_tx_if_match(
    tx: &mut PgTransaction<'_>,
    resource: &Value,
    if_match: Option<&str>,
    txid: Option<i64>,
) -> Result<StoredResource, StorageError> {
    Ok(update_with_tx_snapshot(tx, resource, if_match, txid)
        .await?
        .stored)
}

/// The materialized, locked source returns the actual preceding snapshot in
/// the same native UPDATE statement, independent of authorization prefetches.
pub async fn update_with_tx_snapshot(
    tx: &mut PgTransaction<'_>,
    resource: &Value,
    if_match: Option<&str>,
    txid: Option<i64>,
) -> Result<octofhir_storage::UpdatedResource, StorageError> {
    lock_resource_writes(tx, false).await?;
    let resource_type = resource["resourceType"]
        .as_str()
        .ok_or_else(|| StorageError::invalid_resource("Missing resourceType"))?;
    let id = resource["id"]
        .as_str()
        .ok_or_else(|| StorageError::invalid_resource("Missing id"))?;
    let table = SchemaManager::table_name(resource_type);
    let version_check = match if_match {
        Some(version) => match stored_version(version) {
            Some(version) => format!("AND p.txid = {version}::bigint"),
            None => "AND FALSE".into(),
        },
        None => String::new(),
    };
    let version = txid
        .map(|version| format!("{version}::bigint"))
        .unwrap_or_else(|| "nextval('_transaction_txid_seq')".into());
    let sql = format!(
        r#"WITH previous AS MATERIALIZED (
        SELECT id, txid, created_at, updated_at, resource FROM "{table}" WHERE id=$1 AND status!='deleted' FOR UPDATE
    ), new_tx AS MATERIALIZED (SELECT {version} AS txid FROM previous)
    UPDATE "{table}" AS t SET txid=new_tx.txid,
        resource = $2::jsonb, ucum = $3::jsonb, status = 'updated'
    FROM previous AS p, new_tx WHERE t.id=p.id {version_check}
    RETURNING t.id, t.txid, t.created_at, t.updated_at, t.resource, p.txid, p.created_at, p.updated_at, p.resource"#
    );
    type Row = (
        String,
        i64,
        DateTime<Utc>,
        DateTime<Utc>,
        Value,
        i64,
        DateTime<Utc>,
        DateTime<Utc>,
        Value,
    );
    let row: Option<Row> = query_as(AssertSqlSafe(sql))
        .bind(id)
        .bind(resource)
        .bind(canonical_quantities(resource))
        .fetch_optional(&mut **tx)
        .await
        .map_err(|error| mutation_error(error, "Failed to update resource"))?;
    if let Some((
        id,
        version,
        created,
        updated,
        resource,
        old_version,
        old_created,
        old_updated,
        old_resource,
    )) = row
    {
        return Ok(octofhir_storage::UpdatedResource {
            previous: StoredResource {
                id: id.clone(),
                version_id: old_version.to_string(),
                resource_type: resource_type.into(),
                resource: old_resource,
                created_at: chrono_to_time(old_created),
                last_updated: chrono_to_time(old_updated),
            },
            stored: StoredResource {
                id,
                version_id: version.to_string(),
                resource_type: resource_type.into(),
                resource,
                created_at: chrono_to_time(created),
                last_updated: chrono_to_time(updated),
            },
        });
    }
    if let Some(expected) = if_match {
        let current: Option<i64> = query_scalar(AssertSqlSafe(format!(
            r#"SELECT txid FROM "{table}" WHERE id=$1 AND status!='deleted'"#
        )))
        .bind(id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|error| mutation_error(error, "Failed to check update target"))?;
        if let Some(current) = current {
            return Err(StorageError::version_conflict(
                expected,
                current.to_string(),
            ));
        }
    }
    Err(StorageError::not_found(resource_type, id))
}

/// Deletes a resource within a transaction.
///
/// When `txid` is `Some(t)`, the provided transaction id is reused. Otherwise
/// a new `_transaction` row is allocated via CTE.
pub async fn delete_with_tx(
    tx: &mut PgTransaction<'_>,
    resource_type: &str,
    id: &str,
    txid: Option<i64>,
) -> Result<(), StorageError> {
    let table = SchemaManager::table_name(resource_type);

    // Physically remove the live row; the BEFORE DELETE trigger archives the
    // final version to history (see `delete` for why deleted rows must not stay
    // in the live table). `txid` is unused now — the archived version keeps its
    // own version id and nothing new is written to the live table.
    let _ = txid;
    let sql = format!(r#"DELETE FROM "{table}" WHERE id = $1"#);

    let _result = query(AssertSqlSafe(sql.to_string()))
        .bind(id)
        .execute(&mut **tx)
        .await
        .map_err(|e| {
            if e.to_string().contains("does not exist") {
                return StorageError::internal(format!("Table does not exist: {e}"));
            }
            mutation_error(e, "Failed to delete resource")
        })?;

    // Per FHIR spec: delete is idempotent
    Ok(())
}

/// Reads a resource within a transaction.
///
/// This read sees uncommitted changes made within the same transaction.
pub async fn read_with_tx(
    tx: &mut PgTransaction<'_>,
    resource_type: &str,
    id: &str,
) -> Result<Option<StoredResource>, StorageError> {
    read_with_tx_lock(tx, resource_type, id, false).await
}

/// Lock the selected live row for an authorization-aware mutation or dry run.
pub async fn read_with_tx_for_update(
    tx: &mut PgTransaction<'_>,
    resource_type: &str,
    id: &str,
) -> Result<Option<StoredResource>, StorageError> {
    lock_resource_writes(tx, false).await?;
    read_with_tx_lock(tx, resource_type, id, true).await
}

pub async fn read_many_with_tx_for_update(
    tx: &mut PgTransaction<'_>,
    resource_type: &str,
    ids: &[String],
) -> Result<Vec<StoredResource>, StorageError> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let table =
        octofhir_search::sql_builder::escape_identifier(&SchemaManager::table_name(resource_type))
            .map_err(|error| StorageError::invalid_resource(error.to_string()))?;
    lock_resource_writes(tx, false).await?;
    let sql = format!(
        "SELECT id, txid, created_at, updated_at, resource FROM {table}
         WHERE id=ANY($1::text[]) AND status!='deleted' ORDER BY id FOR UPDATE"
    );
    let rows: Vec<(String, i64, DateTime<Utc>, DateTime<Utc>, Value)> =
        query_as(AssertSqlSafe(sql))
            .bind(ids)
            .fetch_all(&mut **tx)
            .await
            .map_err(|error| {
                StorageError::internal(format!("Failed to lock write snapshots: {error}"))
            })?;
    Ok(rows
        .into_iter()
        .map(
            |(id, txid, created_at, updated_at, resource)| StoredResource {
                id,
                version_id: txid.to_string(),
                resource_type: resource_type.into(),
                resource,
                last_updated: chrono_to_time(updated_at),
                created_at: chrono_to_time(created_at),
            },
        )
        .collect())
}

async fn read_with_tx_lock(
    tx: &mut PgTransaction<'_>,
    resource_type: &str,
    id: &str,
    lock: bool,
) -> Result<Option<StoredResource>, StorageError> {
    let table = SchemaManager::table_name(resource_type);

    // Query including status to detect deleted resources
    let sql = format!(
        r#"SELECT id, txid, created_at, updated_at, resource, status::text
           FROM "{table}"
           WHERE id = $1{locking}"#,
        locking = if lock { " FOR UPDATE" } else { "" }
    );

    let row: Option<(String, i64, DateTime<Utc>, DateTime<Utc>, Value, String)> =
        query_as(AssertSqlSafe(sql.to_string()))
            .bind(id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(|e| {
                if e.to_string().contains("does not exist") {
                    return StorageError::internal(format!("Table does not exist: {e}"));
                }
                StorageError::internal(format!("Failed to read resource: {e}"))
            })?;

    match row {
        Some((row_id, txid, created_at, updated_at, resource, status)) => {
            // Check if the resource is soft-deleted
            if status == "deleted" {
                return Err(StorageError::deleted(resource_type, id));
            }

            let created_at_time = chrono_to_time(created_at);
            let updated_at_time = chrono_to_time(updated_at);
            Ok(Some(StoredResource {
                id: row_id,
                version_id: txid.to_string(),
                resource_type: resource_type.to_string(),
                resource,
                last_updated: updated_at_time,
                created_at: created_at_time,
            }))
        }
        None => Ok(None),
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
