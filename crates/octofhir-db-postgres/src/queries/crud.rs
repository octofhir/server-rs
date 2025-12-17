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
        r#"INSERT INTO "{table}" (id, txid, created_at, updated_at, resource, status)
           VALUES ($1, $2, $3, $3, $4, 'created')
           RETURNING id, txid, created_at, updated_at"#
    );

    let row: (String, i64, DateTime<Utc>, DateTime<Utc>) = query_as(&sql)
        .bind(&id)
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
                    meta_obj.insert("lastUpdated".to_string(), serde_json::json!(updated_at.to_rfc3339()));
                    meta_obj.insert("createdAt".to_string(), serde_json::json!(created_at.to_rfc3339()));
                }
            } else {
                resource["meta"] = serde_json::json!({
                    "versionId": txid.to_string(),
                    "lastUpdated": updated_at.to_rfc3339(),
                    "createdAt": created_at.to_rfc3339()
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
    // Note: updated_at is automatically updated by the update_updated_at_column() trigger
    let update_sql = format!(
        r#"UPDATE "{table}"
           SET txid = $1, resource = $2, status = 'updated'
           WHERE id = $3 AND status != 'deleted'
           RETURNING id, txid, created_at, updated_at"#
    );

    let row: Option<(Uuid, i64, DateTime<Utc>, DateTime<Utc>)> = query_as(&update_sql)
        .bind(txid)
        .bind(&resource)
        .bind(id_uuid)
        .fetch_optional(pool)
        .await
        .map_err(|e| StorageError::internal(format!("Failed to update resource: {e}")))?;

    match row {
        Some((_, returned_txid, created_at, updated_at)) => {
            let created_at_time = chrono_to_time(created_at);
            let updated_at_time = chrono_to_time(updated_at);
            Ok(StoredResource {
                id: id.to_string(),
                version_id: returned_txid.to_string(),
                resource_type: resource_type.to_string(),
                resource,
                last_updated: updated_at_time,
                created_at: created_at_time,
            })
        }
        None => Err(StorageError::not_found(resource_type, id)),
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

    // Parse ID as UUID - if invalid format, delete is idempotent (nothing to delete)
    let id_uuid = match Uuid::parse_str(id) {
        Ok(u) => u,
        Err(_) => return Ok(()), // Invalid ID format means nothing to delete
    };

    // Create transaction for the delete operation
    let txid = create_transaction(pool).await?;

    // Note: updated_at is automatically updated by the update_updated_at_column() trigger
    let sql = format!(
        r#"UPDATE "{table}"
           SET txid = $1, status = 'deleted'
           WHERE id = $2 AND status != 'deleted'"#
    );

    let _result = query(&sql)
        .bind(txid)
        .bind(id_uuid)
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

    // Parse ID as UUID - if invalid format, resource doesn't exist
    let id_uuid = match Uuid::parse_str(id) {
        Ok(u) => u,
        Err(_) => return Ok(None), // Invalid ID format means resource not found
    };

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

    let row: Option<(Uuid, i64, DateTime<Utc>, DateTime<Utc>, Value)> = query_as(&current_sql)
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

    if let Some((row_id, txid, created_at, updated_at, resource)) = row {
        let created_at_time = chrono_to_time(created_at);
        let updated_at_time = chrono_to_time(updated_at);
        return Ok(Some(StoredResource {
            id: row_id.to_string(),
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

    let row: Option<(Uuid, i64, DateTime<Utc>, DateTime<Utc>, Value)> = query_as(&history_sql)
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
        Some((row_id, txid, created_at, updated_at, resource)) => {
            let created_at_time = chrono_to_time(created_at);
            let updated_at_time = chrono_to_time(updated_at);
            Ok(Some(StoredResource {
                id: row_id.to_string(),
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

// ============================================================================
// Transaction-aware CRUD operations
// ============================================================================

/// Creates a new resource within a transaction.
///
/// This variant uses an existing transaction instead of the pool,
/// allowing multiple operations to be grouped atomically.
pub async fn create_with_tx(
    tx: &mut PgTransaction<'_>,
    schema: &SchemaManager,
    resource: &Value,
) -> Result<StoredResource, StorageError> {
    let resource_type = resource["resourceType"]
        .as_str()
        .ok_or_else(|| StorageError::invalid_resource("Missing or invalid resourceType field"))?;

    // Ensure table exists (uses the pool from the transaction's connection)
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

    // Create transaction for this operation (within the outer transaction)
    let txid: i64 =
        query_scalar("INSERT INTO _transaction (status) VALUES ('committed') RETURNING txid")
            .fetch_one(&mut **tx)
            .await
            .map_err(|e| StorageError::internal(format!("Failed to create transaction: {e}")))?;

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
        r#"INSERT INTO "{table}" (id, txid, created_at, updated_at, resource, status)
           VALUES ($1, $2, $3, $3, $4, 'created')
           RETURNING id, txid, created_at, updated_at"#
    );

    let row: (Uuid, i64, DateTime<Utc>, DateTime<Utc>) = query_as(&sql)
        .bind(id_uuid)
        .bind(txid)
        .bind(now)
        .bind(&resource)
        .fetch_one(&mut **tx)
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
pub async fn update_with_tx(
    tx: &mut PgTransaction<'_>,
    schema: &SchemaManager,
    resource: &Value,
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

    // Create new transaction for this version (within the outer transaction)
    let txid: i64 =
        query_scalar("INSERT INTO _transaction (status) VALUES ('committed') RETURNING txid")
            .fetch_one(&mut **tx)
            .await
            .map_err(|e| StorageError::internal(format!("Failed to create transaction: {e}")))?;

    let now = Utc::now();

    // Build updated resource with new meta
    let mut resource = resource.clone();
    resource["meta"] = serde_json::json!({
        "versionId": txid.to_string(),
        "lastUpdated": now.to_rfc3339()
    });

    // Update resource (trigger will archive old version to history)
    // Note: updated_at is automatically updated by the update_updated_at_column() trigger
    let update_sql = format!(
        r#"UPDATE "{table}"
           SET txid = $1, resource = $2, status = 'updated'
           WHERE id = $3 AND status != 'deleted'
           RETURNING id, txid, created_at, updated_at"#
    );

    let row: Option<(Uuid, i64, DateTime<Utc>, DateTime<Utc>)> = query_as(&update_sql)
        .bind(txid)
        .bind(&resource)
        .bind(id_uuid)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|e| StorageError::internal(format!("Failed to update resource: {e}")))?;

    match row {
        Some((_, returned_txid, created_at, updated_at)) => {
            let created_at_time = chrono_to_time(created_at);
            let updated_at_time = chrono_to_time(updated_at);
            Ok(StoredResource {
                id: id.to_string(),
                version_id: returned_txid.to_string(),
                resource_type: resource_type.to_string(),
                resource,
                last_updated: updated_at_time,
                created_at: created_at_time,
            })
        }
        None => Err(StorageError::not_found(resource_type, id)),
    }
}

/// Deletes a resource within a transaction.
pub async fn delete_with_tx(
    tx: &mut PgTransaction<'_>,
    _schema: &SchemaManager,
    resource_type: &str,
    id: &str,
) -> Result<(), StorageError> {
    let table = SchemaManager::table_name(resource_type);

    // Parse ID as UUID - if invalid format, delete is idempotent (nothing to delete)
    let id_uuid = match Uuid::parse_str(id) {
        Ok(u) => u,
        Err(_) => return Ok(()), // Invalid ID format means nothing to delete
    };

    // Create transaction for the delete operation (within the outer transaction)
    let txid: i64 =
        query_scalar("INSERT INTO _transaction (status) VALUES ('committed') RETURNING txid")
            .fetch_one(&mut **tx)
            .await
            .map_err(|e| StorageError::internal(format!("Failed to create transaction: {e}")))?;

    // Note: updated_at is automatically updated by the update_updated_at_column() trigger
    let sql = format!(
        r#"UPDATE "{table}"
           SET txid = $1, status = 'deleted'
           WHERE id = $2 AND status != 'deleted'"#
    );

    let _result = query(&sql)
        .bind(txid)
        .bind(id_uuid)
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
    _schema: &SchemaManager,
    resource_type: &str,
    id: &str,
) -> Result<Option<StoredResource>, StorageError> {
    let table = SchemaManager::table_name(resource_type);

    // Parse ID as UUID - if invalid format, resource doesn't exist
    let id_uuid = match Uuid::parse_str(id) {
        Ok(u) => u,
        Err(_) => return Ok(None),
    };

    // Query including status to detect deleted resources
    let sql = format!(
        r#"SELECT id, txid, created_at, updated_at, resource, status::text
           FROM "{table}"
           WHERE id = $1"#
    );

    let row: Option<(Uuid, i64, DateTime<Utc>, DateTime<Utc>, Value, String)> = query_as(&sql)
        .bind(id_uuid)
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
                id: row_id.to_string(),
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
