//! CRUD (Create, Read, Update, Delete) query implementations.
//!
//! This module contains the SQL queries for basic resource operations.

use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx_core::query::query;
use sqlx_core::query_as::query_as;
use sqlx_core::query_scalar::query_scalar;
use sqlx_postgres::{PgPool, PgTransaction};
use time::OffsetDateTime;
use uuid::Uuid;

use octofhir_storage::{RawStoredResource, StorageError, StoredResource};

use crate::schema::SchemaManager;

/// Converts chrono DateTime to time OffsetDateTime.
fn chrono_to_time(dt: DateTime<Utc>) -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp(dt.timestamp()).unwrap_or(OffsetDateTime::UNIX_EPOCH)
        + time::Duration::nanoseconds(dt.timestamp_subsec_nanos() as i64)
}

/// Creates a new transaction entry and returns its ID.
///
/// Transaction IDs (txid) are used as version IDs for FHIR resources.
pub async fn create_transaction(pool: &PgPool) -> Result<i64, StorageError> {
    let txid: i64 =
        query_scalar("INSERT INTO _transaction (status) VALUES ('committed') RETURNING txid")
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

    let now = Utc::now();

    // Insert into table with CTE for transaction creation (single query instead of two)
    // This combines the transaction creation and resource insertion atomically
    // We use jsonb_set to add id, meta.versionId and meta.lastUpdated in the SQL
    // This avoids cloning the entire resource JSON just to inject the id field
    let table = SchemaManager::table_name(resource_type);
    let sql = format!(
        r#"WITH new_tx AS (
               INSERT INTO _transaction (status) VALUES ('committed') RETURNING txid
           )
           INSERT INTO "{table}" (id, txid, created_at, updated_at, resource, status)
           SELECT
               $1,
               new_tx.txid,
               $2,
               $2,
               jsonb_set(
                   jsonb_set(
                       jsonb_set($3::jsonb, '{{id}}', to_jsonb($1::text)),
                       '{{meta}}', '{{}}'::jsonb, true
                   ),
                   '{{meta}}',
                   jsonb_build_object(
                       'versionId', new_tx.txid::text,
                       'lastUpdated', to_char($2 AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"')
                   ),
                   true
               ),
               'created'
           FROM new_tx
           RETURNING id, txid, created_at, updated_at, resource"#
    );

    let row: (String, i64, DateTime<Utc>, DateTime<Utc>, Value) = query_as(&sql)
        .bind(&id)
        .bind(now)
        .bind(resource)
        .fetch_one(pool)
        .await
        .map_err(|e| {
            // Check for unique constraint violation
            if e.to_string().contains("duplicate key") {
                StorageError::already_exists(resource_type, &id)
            } else {
                StorageError::internal(format!("Failed to create resource: {e}"))
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

    let now = Utc::now();

    let table = SchemaManager::table_name(resource_type);
    let sql = format!(
        r#"WITH new_tx AS (
               INSERT INTO _transaction (status) VALUES ('committed') RETURNING txid
           )
           INSERT INTO "{table}" (id, txid, created_at, updated_at, resource, status)
           SELECT
               $1,
               new_tx.txid,
               $2,
               $2,
               jsonb_set(
                   jsonb_set(
                       jsonb_set($3::jsonb, '{{id}}', to_jsonb($1::text)),
                       '{{meta}}', '{{}}'::jsonb, true
                   ),
                   '{{meta}}',
                   jsonb_build_object(
                       'versionId', new_tx.txid::text,
                       'lastUpdated', to_char($2 AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"')
                   ),
                   true
               ),
               'created'
           FROM new_tx
           RETURNING id, txid, created_at, updated_at, resource::text"#
    );

    let row: (String, i64, DateTime<Utc>, DateTime<Utc>, String) = query_as(&sql)
        .bind(&id)
        .bind(now)
        .bind(resource)
        .fetch_one(pool)
        .await
        .map_err(|e| {
            if e.to_string().contains("duplicate key") {
                StorageError::already_exists(resource_type, &id)
            } else {
                StorageError::internal(format!("Failed to create resource: {e}"))
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

    let row: Option<(String, i64, DateTime<Utc>, DateTime<Utc>, Value, String)> = query_as(&sql)
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
        Some((row_id, txid, created_at, updated_at, mut resource, status)) => {
            // Check if the resource is soft-deleted
            if status == "deleted" {
                return Err(StorageError::deleted(resource_type, id));
            }

            let created_at_time = chrono_to_time(created_at);
            let updated_at_time = chrono_to_time(updated_at);

            // Merge timestamps into meta field
            if let Some(meta) = resource.get_mut("meta") {
                if let Some(meta_obj) = meta.as_object_mut() {
                    meta_obj.insert("versionId".to_string(), serde_json::json!(txid.to_string()));
                    meta_obj.insert(
                        "lastUpdated".to_string(),
                        serde_json::json!(updated_at.to_rfc3339()),
                    );
                }
            } else {
                resource["meta"] = serde_json::json!({
                    "versionId": txid.to_string(),
                    "lastUpdated": updated_at.to_rfc3339()
                });
            }

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

    let row: Option<(String, i64, DateTime<Utc>, DateTime<Utc>, String, String)> = query_as(&sql)
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
        None => Ok(None),
    }
}

/// Updates an existing FHIR resource.
///
/// If `if_match` is provided, the update will only succeed if the current
/// version matches the expected version (optimistic locking).
pub async fn update(
    pool: &PgPool,
    resource: &Value,
    if_match: Option<&str>,
) -> Result<StoredResource, StorageError> {
    let resource_type = resource["resourceType"]
        .as_str()
        .ok_or_else(|| StorageError::invalid_resource("Missing or invalid resourceType field"))?;

    let id = resource["id"]
        .as_str()
        .ok_or_else(|| StorageError::invalid_resource("Missing id field"))?;

    // Tables are created at startup by bootstrap_conformance_if_postgres()

    let table = SchemaManager::table_name(resource_type);
    let now = Utc::now();

    // Single query with CTE: create transaction + update resource + set meta atomically
    // Version check is done in the WHERE clause if if_match is provided
    let (update_sql, has_version_check) = if let Some(expected_version) = if_match {
        // Parse expected version as i64
        let expected_txid: i64 = expected_version.parse().map_err(|_| {
            StorageError::invalid_resource(format!("Invalid version format: {expected_version}"))
        })?;

        (
            format!(
                r#"WITH new_tx AS (
                       INSERT INTO _transaction (status) VALUES ('committed') RETURNING txid
                   ),
                   current AS (
                       SELECT id, txid, created_at FROM "{table}"
                       WHERE id = $1 AND status != 'deleted'
                   )
                   UPDATE "{table}" t
                   SET txid = new_tx.txid,
                       resource = jsonb_set(
                           jsonb_set($2::jsonb, '{{meta}}', '{{}}'::jsonb, true),
                           '{{meta}}',
                           jsonb_build_object(
                               'versionId', new_tx.txid::text,
                               'lastUpdated', to_char($3 AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"')
                           ),
                           true
                       ),
                       status = 'updated'
                   FROM new_tx, current
                   WHERE t.id = $1
                     AND t.status != 'deleted'
                     AND t.txid = {expected_txid}
                   RETURNING t.id, new_tx.txid, t.created_at, t.updated_at, t.resource,
                             (SELECT txid FROM current) as old_txid"#
            ),
            true,
        )
    } else {
        (
            format!(
                r#"WITH new_tx AS (
                       INSERT INTO _transaction (status) VALUES ('committed') RETURNING txid
                   )
                   UPDATE "{table}" t
                   SET txid = new_tx.txid,
                       resource = jsonb_set(
                           jsonb_set($2::jsonb, '{{meta}}', '{{}}'::jsonb, true),
                           '{{meta}}',
                           jsonb_build_object(
                               'versionId', new_tx.txid::text,
                               'lastUpdated', to_char($3 AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"')
                           ),
                           true
                       ),
                       status = 'updated'
                   FROM new_tx
                   WHERE t.id = $1 AND t.status != 'deleted'
                   RETURNING t.id, new_tx.txid, t.created_at, t.updated_at, t.resource, 0::bigint as old_txid"#
            ),
            false,
        )
    };

    let row: Option<(String, i64, DateTime<Utc>, DateTime<Utc>, Value, i64)> =
        query_as(&update_sql)
            .bind(id)
            .bind(&resource)
            .bind(now)
            .fetch_optional(pool)
            .await
            .map_err(|e| StorageError::internal(format!("Failed to update resource: {e}")))?;

    match row {
        Some((
            _returned_id,
            returned_txid,
            created_at,
            updated_at,
            updated_resource,
            _old_txid,
        )) => {
            let created_at_time = chrono_to_time(created_at);
            let updated_at_time = chrono_to_time(updated_at);
            Ok(StoredResource {
                id: id.to_string(),
                version_id: returned_txid.to_string(),
                resource_type: resource_type.to_string(),
                resource: updated_resource,
                last_updated: updated_at_time,
                created_at: created_at_time,
            })
        }
        None => {
            // If version check was used and no row returned, need to determine why
            if has_version_check {
                // Check if resource exists with different version
                let check_sql =
                    format!(r#"SELECT txid FROM "{table}" WHERE id = $1 AND status != 'deleted'"#);
                let current: Option<i64> = query_scalar(&check_sql)
                    .bind(id)
                    .fetch_optional(pool)
                    .await
                    .map_err(|e| {
                        StorageError::internal(format!("Failed to check resource: {e}"))
                    })?;

                match current {
                    Some(v) => Err(StorageError::version_conflict(
                        if_match.unwrap_or(""),
                        v.to_string(),
                    )),
                    None => Err(StorageError::not_found(resource_type, id)),
                }
            } else {
                Err(StorageError::not_found(resource_type, id))
            }
        }
    }
}

/// Updates an existing FHIR resource and returns the result as raw JSON string.
///
/// Like `update()` but uses `resource::text` in the RETURNING clause to avoid
/// JSONB → serde_json::Value deserialization.
pub async fn update_raw(
    pool: &PgPool,
    resource: &Value,
    if_match: Option<&str>,
) -> Result<RawStoredResource, StorageError> {
    let resource_type = resource["resourceType"]
        .as_str()
        .ok_or_else(|| StorageError::invalid_resource("Missing or invalid resourceType field"))?;

    let id = resource["id"]
        .as_str()
        .ok_or_else(|| StorageError::invalid_resource("Missing id field"))?;

    let table = SchemaManager::table_name(resource_type);
    let now = Utc::now();

    let (update_sql, has_version_check) = if let Some(expected_version) = if_match {
        let expected_txid: i64 = expected_version.parse().map_err(|_| {
            StorageError::invalid_resource(format!("Invalid version format: {expected_version}"))
        })?;

        (
            format!(
                r#"WITH new_tx AS (
                       INSERT INTO _transaction (status) VALUES ('committed') RETURNING txid
                   ),
                   current AS (
                       SELECT id, txid, created_at FROM "{table}"
                       WHERE id = $1 AND status != 'deleted'
                   )
                   UPDATE "{table}" t
                   SET txid = new_tx.txid,
                       resource = jsonb_set(
                           jsonb_set($2::jsonb, '{{meta}}', '{{}}'::jsonb, true),
                           '{{meta}}',
                           jsonb_build_object(
                               'versionId', new_tx.txid::text,
                               'lastUpdated', to_char($3 AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"')
                           ),
                           true
                       ),
                       status = 'updated'
                   FROM new_tx, current
                   WHERE t.id = $1
                     AND t.status != 'deleted'
                     AND t.txid = {expected_txid}
                   RETURNING t.id, new_tx.txid, t.created_at, t.updated_at, t.resource::text,
                             (SELECT txid FROM current) as old_txid"#
            ),
            true,
        )
    } else {
        (
            format!(
                r#"WITH new_tx AS (
                       INSERT INTO _transaction (status) VALUES ('committed') RETURNING txid
                   )
                   UPDATE "{table}" t
                   SET txid = new_tx.txid,
                       resource = jsonb_set(
                           jsonb_set($2::jsonb, '{{meta}}', '{{}}'::jsonb, true),
                           '{{meta}}',
                           jsonb_build_object(
                               'versionId', new_tx.txid::text,
                               'lastUpdated', to_char($3 AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"')
                           ),
                           true
                       ),
                       status = 'updated'
                   FROM new_tx
                   WHERE t.id = $1 AND t.status != 'deleted'
                   RETURNING t.id, new_tx.txid, t.created_at, t.updated_at, t.resource::text, 0::bigint as old_txid"#
            ),
            false,
        )
    };

    let row: Option<(String, i64, DateTime<Utc>, DateTime<Utc>, String, i64)> =
        query_as(&update_sql)
            .bind(id)
            .bind(&resource)
            .bind(now)
            .fetch_optional(pool)
            .await
            .map_err(|e| StorageError::internal(format!("Failed to update resource: {e}")))?;

    match row {
        Some((_returned_id, returned_txid, created_at, updated_at, resource_json, _old_txid)) => {
            let created_at_time = chrono_to_time(created_at);
            let updated_at_time = chrono_to_time(updated_at);
            Ok(RawStoredResource {
                id: id.to_string(),
                version_id: returned_txid.to_string(),
                resource_type: resource_type.to_string(),
                resource_json,
                last_updated: updated_at_time,
                created_at: created_at_time,
            })
        }
        None => {
            if has_version_check {
                let check_sql =
                    format!(r#"SELECT txid FROM "{table}" WHERE id = $1 AND status != 'deleted'"#);
                let current: Option<i64> = query_scalar(&check_sql)
                    .bind(id)
                    .fetch_optional(pool)
                    .await
                    .map_err(|e| {
                        StorageError::internal(format!("Failed to check resource: {e}"))
                    })?;

                match current {
                    Some(v) => Err(StorageError::version_conflict(
                        if_match.unwrap_or(""),
                        v.to_string(),
                    )),
                    None => Err(StorageError::not_found(resource_type, id)),
                }
            } else {
                Err(StorageError::not_found(resource_type, id))
            }
        }
    }
}

/// Soft deletes a FHIR resource.
///
/// The resource is marked as deleted but not physically removed,
/// preserving history and allowing for potential recovery.
///
/// Per FHIR spec, delete is idempotent - deleting a non-existent or
/// already deleted resource returns success (204 No Content).
pub async fn delete(pool: &PgPool, resource_type: &str, id: &str) -> Result<(), StorageError> {
    let table = SchemaManager::table_name(resource_type);

    // Single CTE query: create transaction + update resource atomically
    // Eliminates a separate DB round-trip for create_transaction()
    let sql = format!(
        r#"WITH new_tx AS (
               INSERT INTO _transaction (status) VALUES ('committed') RETURNING txid
           )
           UPDATE "{table}"
           SET txid = new_tx.txid, status = 'deleted'
           FROM new_tx
           WHERE id = $1 AND status != 'deleted'"#
    );

    let _result = query(&sql)
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| {
            // Table might not exist, but that's OK for idempotent delete
            if e.to_string().contains("does not exist") {
                return StorageError::internal(format!("Table does not exist: {e}"));
            }
            StorageError::internal(format!("Failed to delete resource: {e}"))
        })?;

    // Per FHIR spec: delete is idempotent, so success regardless of whether
    // any rows were affected (resource didn't exist or was already deleted)
    Ok(())
}

/// Reads a specific version of a FHIR resource.
///
/// First checks the current table, then falls back to the history table.
pub async fn vread(
    pool: &PgPool,
    resource_type: &str,
    id: &str,
    version: &str,
) -> Result<Option<StoredResource>, StorageError> {
    let table = SchemaManager::table_name(resource_type);
    let history_table = format!("{}_history", table);

    // Parse version as i64
    let version_id: i64 = version
        .parse()
        .map_err(|e| StorageError::invalid_resource(format!("Invalid version ID: {e}")))?;

    // First check current table
    let current_sql = format!(
        r#"SELECT id, txid, created_at, updated_at, resource
           FROM "{table}"
           WHERE id = $1 AND txid = $2"#
    );

    let row: Option<(String, i64, DateTime<Utc>, DateTime<Utc>, Value)> = query_as(&current_sql)
        .bind(id)
        .bind(version_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| {
            if e.to_string().contains("does not exist") {
                return StorageError::internal(format!("Table does not exist: {e}"));
            }
            StorageError::internal(format!("Failed to read version: {e}"))
        })?;

    if let Some((row_id, txid, created_at, updated_at, resource)) = row {
        let created_at_time = chrono_to_time(created_at);
        let updated_at_time = chrono_to_time(updated_at);
        return Ok(Some(StoredResource {
            id: row_id,
            version_id: txid.to_string(),
            resource_type: resource_type.to_string(),
            resource,
            last_updated: updated_at_time,
            created_at: created_at_time,
        }));
    }

    // Check history table
    let history_sql = format!(
        r#"SELECT id, txid, created_at, updated_at, resource
           FROM "{history_table}"
           WHERE id = $1 AND txid = $2"#
    );

    let row: Option<(String, i64, DateTime<Utc>, DateTime<Utc>, Value)> = query_as(&history_sql)
        .bind(id)
        .bind(version_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| {
            if e.to_string().contains("does not exist") {
                return StorageError::internal(format!("History table does not exist: {e}"));
            }
            StorageError::internal(format!("Failed to read version from history: {e}"))
        })?;

    match row {
        Some((row_id, txid, created_at, updated_at, resource)) => {
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

/// Reads a specific version of a FHIR resource as raw JSON string.
///
/// Like `vread()` but uses `resource::text` to avoid the JSONB → Value round-trip.
/// First checks the current table, then falls back to the history table.
pub async fn vread_raw(
    pool: &PgPool,
    resource_type: &str,
    id: &str,
    version: &str,
) -> Result<Option<RawStoredResource>, StorageError> {
    let table = SchemaManager::table_name(resource_type);
    let history_table = format!("{}_history", table);

    let version_id: i64 = version
        .parse()
        .map_err(|e| StorageError::invalid_resource(format!("Invalid version ID: {e}")))?;

    // First check current table
    let current_sql = format!(
        r#"SELECT id, txid, created_at, updated_at, resource::text
           FROM "{table}"
           WHERE id = $1 AND txid = $2"#
    );

    let row: Option<(String, i64, DateTime<Utc>, DateTime<Utc>, String)> =
        query_as(&current_sql)
            .bind(id)
            .bind(version_id)
            .fetch_optional(pool)
            .await
            .map_err(|e| {
                if e.to_string().contains("does not exist") {
                    return StorageError::internal(format!("Table does not exist: {e}"));
                }
                StorageError::internal(format!("Failed to read version: {e}"))
            })?;

    if let Some((row_id, txid, created_at, updated_at, resource_json)) = row {
        let created_at_time = chrono_to_time(created_at);
        let updated_at_time = chrono_to_time(updated_at);
        return Ok(Some(RawStoredResource {
            id: row_id,
            version_id: txid.to_string(),
            resource_type: resource_type.to_string(),
            resource_json,
            last_updated: updated_at_time,
            created_at: created_at_time,
        }));
    }

    // Check history table
    let history_sql = format!(
        r#"SELECT id, txid, created_at, updated_at, resource::text
           FROM "{history_table}"
           WHERE id = $1 AND txid = $2"#
    );

    let row: Option<(String, i64, DateTime<Utc>, DateTime<Utc>, String)> =
        query_as(&history_sql)
            .bind(id)
            .bind(version_id)
            .fetch_optional(pool)
            .await
            .map_err(|e| {
                if e.to_string().contains("does not exist") {
                    return StorageError::internal(format!("History table does not exist: {e}"));
                }
                StorageError::internal(format!("Failed to read version from history: {e}"))
            })?;

    match row {
        Some((row_id, txid, created_at, updated_at, resource_json)) => {
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
        None => Ok(None),
    }
}

// ============================================================================
// Transaction-aware CRUD operations
// ============================================================================

/// Creates a new resource within a transaction.
///
/// This variant uses an existing transaction instead of the pool,
/// allowing multiple operations to be grouped atomically.
/// Uses SQL `jsonb_set` to inject id/meta without cloning the resource in Rust.
pub async fn create_with_tx(
    tx: &mut PgTransaction<'_>,
    resource: &Value,
) -> Result<StoredResource, StorageError> {
    let resource_type = resource["resourceType"]
        .as_str()
        .ok_or_else(|| StorageError::invalid_resource("Missing or invalid resourceType field"))?;

    // Generate ID if not provided
    let id = resource["id"]
        .as_str()
        .map(String::from)
        .unwrap_or_else(|| Uuid::new_v4().to_string());

    let now = Utc::now();

    // Use SQL jsonb_set to inject id and meta atomically, avoiding Rust-side resource.clone()
    let table = SchemaManager::table_name(resource_type);
    let sql = format!(
        r#"WITH new_tx AS (
               INSERT INTO _transaction (status) VALUES ('committed') RETURNING txid
           )
           INSERT INTO "{table}" (id, txid, created_at, updated_at, resource, status)
           SELECT
               $1,
               new_tx.txid,
               $2,
               $2,
               jsonb_set(
                   jsonb_set(
                       jsonb_set($3::jsonb, '{{id}}', to_jsonb($1::text)),
                       '{{meta}}', '{{}}'::jsonb, true
                   ),
                   '{{meta}}',
                   jsonb_build_object(
                       'versionId', new_tx.txid::text,
                       'lastUpdated', to_char($2 AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"')
                   ),
                   true
               ),
               'created'
           FROM new_tx
           RETURNING id, txid, created_at, updated_at, resource"#
    );

    let row: (String, i64, DateTime<Utc>, DateTime<Utc>, Value) = query_as(&sql)
        .bind(&id)
        .bind(now)
        .bind(resource)
        .fetch_one(&mut **tx)
        .await
        .map_err(|e| {
            if e.to_string().contains("duplicate key") {
                StorageError::already_exists(resource_type, &id)
            } else {
                StorageError::internal(format!("Failed to create resource: {e}"))
            }
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

/// Updates a resource within a transaction.
///
/// Uses SQL `jsonb_set` to inject meta without cloning the resource in Rust.
pub async fn update_with_tx(
    tx: &mut PgTransaction<'_>,
    resource: &Value,
) -> Result<StoredResource, StorageError> {
    let resource_type = resource["resourceType"]
        .as_str()
        .ok_or_else(|| StorageError::invalid_resource("Missing or invalid resourceType field"))?;

    let id = resource["id"]
        .as_str()
        .ok_or_else(|| StorageError::invalid_resource("Missing id field"))?;

    let table = SchemaManager::table_name(resource_type);
    let now = Utc::now();

    // Use CTE + SQL jsonb_set to avoid Rust-side resource.clone()
    let update_sql = format!(
        r#"WITH new_tx AS (
               INSERT INTO _transaction (status) VALUES ('committed') RETURNING txid
           )
           UPDATE "{table}" t
           SET txid = new_tx.txid,
               resource = jsonb_set(
                   jsonb_set($1::jsonb, '{{meta}}', '{{}}'::jsonb, true),
                   '{{meta}}',
                   jsonb_build_object(
                       'versionId', new_tx.txid::text,
                       'lastUpdated', to_char($2 AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"')
                   ),
                   true
               ),
               status = 'updated'
           FROM new_tx
           WHERE t.id = $3 AND t.status != 'deleted'
           RETURNING t.id, new_tx.txid, t.created_at, t.updated_at, t.resource"#
    );

    let row: Option<(String, i64, DateTime<Utc>, DateTime<Utc>, Value)> = query_as(&update_sql)
        .bind(resource)
        .bind(now)
        .bind(id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|e| StorageError::internal(format!("Failed to update resource: {e}")))?;

    match row {
        Some((_returned_id, returned_txid, created_at, updated_at, updated_resource)) => {
            let created_at_time = chrono_to_time(created_at);
            let updated_at_time = chrono_to_time(updated_at);
            Ok(StoredResource {
                id: id.to_string(),
                version_id: returned_txid.to_string(),
                resource_type: resource_type.to_string(),
                resource: updated_resource,
                last_updated: updated_at_time,
                created_at: created_at_time,
            })
        }
        None => Err(StorageError::not_found(resource_type, id)),
    }
}

/// Deletes a resource within a transaction.
///
/// Uses CTE to combine transaction creation + delete in a single query.
pub async fn delete_with_tx(
    tx: &mut PgTransaction<'_>,
    resource_type: &str,
    id: &str,
) -> Result<(), StorageError> {
    let table = SchemaManager::table_name(resource_type);

    // Single CTE query: create transaction + update atomically
    let sql = format!(
        r#"WITH new_tx AS (
               INSERT INTO _transaction (status) VALUES ('committed') RETURNING txid
           )
           UPDATE "{table}"
           SET txid = new_tx.txid, status = 'deleted'
           FROM new_tx
           WHERE id = $1 AND status != 'deleted'"#
    );

    let _result = query(&sql)
        .bind(id)
        .execute(&mut **tx)
        .await
        .map_err(|e| {
            if e.to_string().contains("does not exist") {
                return StorageError::internal(format!("Table does not exist: {e}"));
            }
            StorageError::internal(format!("Failed to delete resource: {e}"))
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
    let table = SchemaManager::table_name(resource_type);

    // Query including status to detect deleted resources
    let sql = format!(
        r#"SELECT id, txid, created_at, updated_at, resource, status::text
           FROM "{table}"
           WHERE id = $1"#
    );

    let row: Option<(String, i64, DateTime<Utc>, DateTime<Utc>, Value, String)> = query_as(&sql)
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
