//! CRUD (Create, Read, Update, Delete) query implementations.
//!
//! This module contains the SQL queries for basic resource operations.

use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx_core::query::query;
use sqlx_core::query_as::query_as;
use sqlx_core::query_scalar::query_scalar;
use sqlx_postgres::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

use octofhir_storage::{StorageError, StoredResource};

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
pub async fn create(
    pool: &PgPool,
    schema: &SchemaManager,
    resource: &Value,
) -> Result<StoredResource, StorageError> {
    let resource_type = resource["resourceType"]
        .as_str()
        .ok_or_else(|| StorageError::invalid_resource("Missing or invalid resourceType field"))?;

    // Ensure table exists
    schema
        .ensure_table(resource_type)
        .await
        .map_err(|e| StorageError::internal(format!("Schema error: {e}")))?;

    // Generate ID if not provided
    let id = resource["id"]
        .as_str()
        .map(String::from)
        .unwrap_or_else(|| Uuid::new_v4().to_string());

    // Parse ID as UUID
    let id_uuid = Uuid::parse_str(&id)
        .map_err(|e| StorageError::invalid_resource(format!("Invalid UUID format: {e}")))?;

    // Create transaction for this operation
    let txid = create_transaction(pool).await?;
    let now = Utc::now();

    // Build resource with id and meta fields
    let mut resource = resource.clone();
    resource["id"] = serde_json::json!(id);
    resource["meta"] = serde_json::json!({
        "versionId": txid.to_string(),
        "lastUpdated": now.to_rfc3339()
    });

    // Insert into table
    let table = SchemaManager::table_name(resource_type);
    let sql = format!(
        r#"INSERT INTO "{table}" (id, txid, ts, resource, status)
           VALUES ($1, $2, $3, $4, 'created')
           RETURNING id, txid, ts"#
    );

    let row: (Uuid, i64, DateTime<Utc>) = query_as(&sql)
        .bind(id_uuid)
        .bind(txid)
        .bind(now)
        .bind(&resource)
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

    let now_time = chrono_to_time(row.2);

    Ok(StoredResource {
        id,
        version_id: row.1.to_string(),
        resource_type: resource_type.to_string(),
        resource,
        last_updated: now_time,
        created_at: now_time,
    })
}

/// Reads a FHIR resource by type and ID.
///
/// Returns `None` if the resource doesn't exist or has been deleted.
pub async fn read(
    pool: &PgPool,
    resource_type: &str,
    id: &str,
) -> Result<Option<StoredResource>, StorageError> {
    let table = SchemaManager::table_name(resource_type);

    // Parse ID as UUID
    let id_uuid = Uuid::parse_str(id)
        .map_err(|e| StorageError::invalid_resource(format!("Invalid UUID format: {e}")))?;

    let sql = format!(
        r#"SELECT id, txid, ts, resource
           FROM "{table}"
           WHERE id = $1 AND status != 'deleted'"#
    );

    let row: Option<(Uuid, i64, DateTime<Utc>, Value)> = query_as(&sql)
        .bind(id_uuid)
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
        Some((row_id, txid, ts, resource)) => {
            let ts_time = chrono_to_time(ts);
            Ok(Some(StoredResource {
                id: row_id.to_string(),
                version_id: txid.to_string(),
                resource_type: resource_type.to_string(),
                resource,
                last_updated: ts_time,
                created_at: ts_time, // Note: Would need separate column for true created_at
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
    schema: &SchemaManager,
    resource: &Value,
    if_match: Option<&str>,
) -> Result<StoredResource, StorageError> {
    let resource_type = resource["resourceType"]
        .as_str()
        .ok_or_else(|| StorageError::invalid_resource("Missing or invalid resourceType field"))?;

    let id = resource["id"]
        .as_str()
        .ok_or_else(|| StorageError::invalid_resource("Missing id field"))?;

    // Ensure table exists
    schema
        .ensure_table(resource_type)
        .await
        .map_err(|e| StorageError::internal(format!("Schema error: {e}")))?;

    let table = SchemaManager::table_name(resource_type);

    // Parse ID as UUID
    let id_uuid = Uuid::parse_str(id)
        .map_err(|e| StorageError::invalid_resource(format!("Invalid UUID format: {e}")))?;

    // Check version if If-Match provided
    if let Some(expected_version) = if_match {
        let version_sql =
            format!(r#"SELECT txid FROM "{table}" WHERE id = $1 AND status != 'deleted'"#);

        let current_version: Option<i64> = query_scalar(&version_sql)
            .bind(id_uuid)
            .fetch_optional(pool)
            .await
            .map_err(|e| StorageError::internal(format!("Failed to check version: {e}")))?;

        match current_version {
            Some(v) if v.to_string() != expected_version => {
                return Err(StorageError::version_conflict(
                    expected_version,
                    v.to_string(),
                ));
            }
            None => {
                return Err(StorageError::not_found(resource_type, id));
            }
            _ => {}
        }
    }

    // Create new transaction for this version
    let txid = create_transaction(pool).await?;
    let now = Utc::now();

    // Build updated resource with new meta
    let mut resource = resource.clone();
    resource["meta"] = serde_json::json!({
        "versionId": txid.to_string(),
        "lastUpdated": now.to_rfc3339()
    });

    // Update resource (trigger will archive old version to history)
    let update_sql = format!(
        r#"UPDATE "{table}"
           SET txid = $1, ts = $2, resource = $3, status = 'updated'
           WHERE id = $4 AND status != 'deleted'
           RETURNING id, txid, ts"#
    );

    let row: Option<(Uuid, i64, DateTime<Utc>)> = query_as(&update_sql)
        .bind(txid)
        .bind(now)
        .bind(&resource)
        .bind(id_uuid)
        .fetch_optional(pool)
        .await
        .map_err(|e| StorageError::internal(format!("Failed to update resource: {e}")))?;

    match row {
        Some((_, returned_txid, ts)) => {
            let ts_time = chrono_to_time(ts);
            Ok(StoredResource {
                id: id.to_string(),
                version_id: returned_txid.to_string(),
                resource_type: resource_type.to_string(),
                resource,
                last_updated: ts_time,
                created_at: ts_time, // Note: Would need separate column for true created_at
            })
        }
        None => Err(StorageError::not_found(resource_type, id)),
    }
}

/// Soft deletes a FHIR resource.
///
/// The resource is marked as deleted but not physically removed,
/// preserving history and allowing for potential recovery.
pub async fn delete(pool: &PgPool, resource_type: &str, id: &str) -> Result<(), StorageError> {
    let table = SchemaManager::table_name(resource_type);

    // Parse ID as UUID
    let id_uuid = Uuid::parse_str(id)
        .map_err(|e| StorageError::invalid_resource(format!("Invalid UUID format: {e}")))?;

    // Create transaction for the delete operation
    let txid = create_transaction(pool).await?;

    let sql = format!(
        r#"UPDATE "{table}"
           SET txid = $1, ts = NOW(), status = 'deleted'
           WHERE id = $2 AND status != 'deleted'"#
    );

    let result = query(&sql)
        .bind(txid)
        .bind(id_uuid)
        .execute(pool)
        .await
        .map_err(|e| {
            if e.to_string().contains("does not exist") {
                StorageError::not_found(resource_type, id)
            } else {
                StorageError::internal(format!("Failed to delete resource: {e}"))
            }
        })?;

    if result.rows_affected() == 0 {
        return Err(StorageError::not_found(resource_type, id));
    }

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

    // Parse ID as UUID
    let id_uuid = Uuid::parse_str(id)
        .map_err(|e| StorageError::invalid_resource(format!("Invalid UUID format: {e}")))?;

    // Parse version as i64
    let version_id: i64 = version
        .parse()
        .map_err(|e| StorageError::invalid_resource(format!("Invalid version ID: {e}")))?;

    // First check current table
    let current_sql = format!(
        r#"SELECT id, txid, ts, resource
           FROM "{table}"
           WHERE id = $1 AND txid = $2"#
    );

    let row: Option<(Uuid, i64, DateTime<Utc>, Value)> = query_as(&current_sql)
        .bind(id_uuid)
        .bind(version_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| {
            if e.to_string().contains("does not exist") {
                return StorageError::internal(format!("Table does not exist: {e}"));
            }
            StorageError::internal(format!("Failed to read version: {e}"))
        })?;

    if let Some((row_id, txid, ts, resource)) = row {
        let ts_time = chrono_to_time(ts);
        return Ok(Some(StoredResource {
            id: row_id.to_string(),
            version_id: txid.to_string(),
            resource_type: resource_type.to_string(),
            resource,
            last_updated: ts_time,
            created_at: ts_time,
        }));
    }

    // Check history table
    let history_sql = format!(
        r#"SELECT id, txid, ts, resource
           FROM "{history_table}"
           WHERE id = $1 AND txid = $2"#
    );

    let row: Option<(Uuid, i64, DateTime<Utc>, Value)> = query_as(&history_sql)
        .bind(id_uuid)
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
        Some((row_id, txid, ts, resource)) => {
            let ts_time = chrono_to_time(ts);
            Ok(Some(StoredResource {
                id: row_id.to_string(),
                version_id: txid.to_string(),
                resource_type: resource_type.to_string(),
                resource,
                last_updated: ts_time,
                created_at: ts_time,
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
