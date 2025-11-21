//! History query implementations.
//!
//! This module contains the SQL queries for FHIR history operations.

use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx_core::query_as::query_as;
use sqlx_postgres::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

use octofhir_storage::{
    HistoryEntry, HistoryMethod, HistoryParams, HistoryResult, StorageError, StoredResource,
};

use crate::schema::SchemaManager;

/// Converts chrono DateTime to time OffsetDateTime.
fn chrono_to_time(dt: DateTime<Utc>) -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp(dt.timestamp()).unwrap_or(OffsetDateTime::UNIX_EPOCH)
        + time::Duration::nanoseconds(dt.timestamp_subsec_nanos() as i64)
}

/// Converts database status string to HistoryMethod.
fn status_to_method(status: &str) -> HistoryMethod {
    match status {
        "created" => HistoryMethod::Create,
        "updated" => HistoryMethod::Update,
        "deleted" => HistoryMethod::Delete,
        _ => HistoryMethod::Update, // Default fallback
    }
}

/// Retrieves the history of a specific resource or all resources of a type.
///
/// # Arguments
///
/// * `pool` - Database connection pool
/// * `resource_type` - The FHIR resource type
/// * `id` - Optional resource ID. If None, returns history for all resources of the type.
/// * `params` - History query parameters (since, at, count, offset)
pub async fn get_history(
    pool: &PgPool,
    resource_type: &str,
    id: Option<&str>,
    params: &HistoryParams,
) -> Result<HistoryResult, StorageError> {
    let table = SchemaManager::table_name(resource_type);
    let history_table = format!("{}_history", table);

    let mut entries = Vec::new();

    // Parse optional UUID
    let id_uuid: Option<Uuid> = match id {
        Some(id_str) => Some(
            Uuid::parse_str(id_str)
                .map_err(|e| StorageError::invalid_resource(format!("Invalid UUID: {e}")))?,
        ),
        None => None,
    };

    // Build dynamic WHERE clause
    let mut conditions = Vec::new();
    let mut current_param_idx = 1;

    if id_uuid.is_some() {
        conditions.push(format!("id = ${current_param_idx}"));
        current_param_idx += 1;
    }

    if params.since.is_some() {
        conditions.push(format!("ts > ${current_param_idx}"));
        current_param_idx += 1;
    }

    if params.at.is_some() {
        conditions.push(format!("ts <= ${current_param_idx}"));
        // No need to increment - this is the last potential parameter
    }

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };

    // Default pagination
    let limit = params.count.unwrap_or(100) as i64;
    let offset = params.offset.unwrap_or(0) as i64;

    // Query current table first (only current versions)
    let current_sql = format!(
        r#"SELECT id, txid, ts, resource, status::text
           FROM "{table}"
           {where_clause}
           ORDER BY ts DESC
           LIMIT {limit} OFFSET {offset}"#
    );

    // Build query with bindings based on conditions
    let current_rows: Vec<(Uuid, i64, DateTime<Utc>, Value, String)> =
        build_history_query(&current_sql, pool, id_uuid, params).await?;

    for (row_id, txid, ts, resource, status) in current_rows {
        let ts_time = chrono_to_time(ts);
        entries.push(HistoryEntry::new(
            StoredResource {
                id: row_id.to_string(),
                version_id: txid.to_string(),
                resource_type: resource_type.to_string(),
                resource,
                last_updated: ts_time,
                created_at: ts_time,
            },
            status_to_method(&status),
        ));
    }

    // Query history table for older versions
    let history_sql = format!(
        r#"SELECT id, txid, ts, resource, status::text
           FROM "{history_table}"
           {where_clause}
           ORDER BY ts DESC
           LIMIT {limit} OFFSET {offset}"#
    );

    let history_rows: Vec<(Uuid, i64, DateTime<Utc>, Value, String)> =
        build_history_query(&history_sql, pool, id_uuid, params).await?;

    for (row_id, txid, ts, resource, status) in history_rows {
        let ts_time = chrono_to_time(ts);
        entries.push(HistoryEntry::new(
            StoredResource {
                id: row_id.to_string(),
                version_id: txid.to_string(),
                resource_type: resource_type.to_string(),
                resource,
                last_updated: ts_time,
                created_at: ts_time,
            },
            status_to_method(&status),
        ));
    }

    // Sort all entries by timestamp descending
    entries.sort_by(|a, b| b.resource.last_updated.cmp(&a.resource.last_updated));

    // Apply final limit after merging
    let total = entries.len() as u32;
    entries.truncate(limit as usize);

    Ok(HistoryResult {
        entries,
        total: Some(total),
    })
}

/// Helper function to build and execute history queries with dynamic bindings.
async fn build_history_query(
    sql: &str,
    pool: &PgPool,
    id_uuid: Option<Uuid>,
    params: &HistoryParams,
) -> Result<Vec<(Uuid, i64, DateTime<Utc>, Value, String)>, StorageError> {
    // Convert time::OffsetDateTime to chrono::DateTime for sqlx binding
    let since_chrono: Option<DateTime<Utc>> = params.since.map(|dt| {
        DateTime::from_timestamp(dt.unix_timestamp(), dt.nanosecond())
            .unwrap_or(DateTime::UNIX_EPOCH)
    });

    let at_chrono: Option<DateTime<Utc>> = params.at.map(|dt| {
        DateTime::from_timestamp(dt.unix_timestamp(), dt.nanosecond())
            .unwrap_or(DateTime::UNIX_EPOCH)
    });

    // Build query dynamically based on which parameters are present
    let result: Vec<(Uuid, i64, DateTime<Utc>, Value, String)> =
        match (id_uuid, since_chrono, at_chrono) {
            (Some(id), Some(since), Some(at)) => {
                query_as(sql)
                    .bind(id)
                    .bind(since)
                    .bind(at)
                    .fetch_all(pool)
                    .await
            }
            (Some(id), Some(since), None) => {
                query_as(sql).bind(id).bind(since).fetch_all(pool).await
            }
            (Some(id), None, Some(at)) => query_as(sql).bind(id).bind(at).fetch_all(pool).await,
            (Some(id), None, None) => query_as(sql).bind(id).fetch_all(pool).await,
            (None, Some(since), Some(at)) => {
                query_as(sql).bind(since).bind(at).fetch_all(pool).await
            }
            (None, Some(since), None) => query_as(sql).bind(since).fetch_all(pool).await,
            (None, None, Some(at)) => query_as(sql).bind(at).fetch_all(pool).await,
            (None, None, None) => query_as(sql).fetch_all(pool).await,
        }
        .map_err(|e| {
            if e.to_string().contains("does not exist") {
                // Table doesn't exist yet, return empty result
                return StorageError::internal(format!("Table does not exist: {e}"));
            }
            StorageError::internal(format!("Failed to query history: {e}"))
        })?;

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_status_to_method() {
        assert_eq!(status_to_method("created"), HistoryMethod::Create);
        assert_eq!(status_to_method("updated"), HistoryMethod::Update);
        assert_eq!(status_to_method("deleted"), HistoryMethod::Delete);
        assert_eq!(status_to_method("unknown"), HistoryMethod::Update);
    }
}
