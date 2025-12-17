//! History query implementations.
//!
//! This module contains the SQL queries for FHIR history operations:
//! - Instance history: All versions of a specific resource
//! - Type history: All versions of all resources of a type
//! - System history: All versions across all resource types

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

/// Row type for history queries (id, txid, created_at, updated_at, resource, status).
type HistoryRow = (Uuid, i64, DateTime<Utc>, DateTime<Utc>, Value, String);

/// Row type for system history queries (id, txid, created_at, updated_at, resource, status, resource_type).
type SystemHistoryRow = (Uuid, i64, DateTime<Utc>, DateTime<Utc>, Value, String, Option<String>);

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

/// Converts time::OffsetDateTime to chrono::DateTime for sqlx binding.
fn time_to_chrono(dt: OffsetDateTime) -> DateTime<Utc> {
    DateTime::from_timestamp(dt.unix_timestamp(), dt.nanosecond()).unwrap_or(DateTime::UNIX_EPOCH)
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

    // Parse optional UUID
    let id_uuid: Option<Uuid> = match id {
        Some(id_str) => Some(
            Uuid::parse_str(id_str)
                .map_err(|e| StorageError::invalid_resource(format!("Invalid UUID: {e}")))?,
        ),
        None => None,
    };

    // Default pagination
    let limit = params.count.unwrap_or(100) as i64;
    let offset = params.offset.unwrap_or(0) as i64;

    // Convert time parameters to chrono
    let since_chrono = params.since.map(time_to_chrono);
    let at_chrono = params.at.map(time_to_chrono);

    // Build UNION ALL query for better performance
    let (sql, has_id, has_since, has_at) = build_union_query(
        &table,
        &history_table,
        id_uuid.is_some(),
        &since_chrono,
        &at_chrono,
    );

    // Add ordering and pagination
    let full_sql = format!(
        r#"WITH all_versions AS ({sql})
           SELECT id, txid, created_at, updated_at, resource, status
           FROM all_versions
           ORDER BY txid DESC
           LIMIT {limit} OFFSET {offset}"#
    );

    // Execute query with appropriate bindings
    let rows = execute_history_query(
        &full_sql,
        pool,
        id_uuid,
        since_chrono,
        at_chrono,
        has_id,
        has_since,
        has_at,
    )
    .await?;

    // Convert rows to history entries
    let entries: Vec<HistoryEntry> = rows
        .into_iter()
        .map(|(row_id, txid, created_at, updated_at, resource, status)| {
            let created_at_time = chrono_to_time(created_at);
            let updated_at_time = chrono_to_time(updated_at);
            HistoryEntry::new(
                StoredResource {
                    id: row_id.to_string(),
                    version_id: txid.to_string(),
                    resource_type: resource_type.to_string(),
                    resource,
                    last_updated: updated_at_time,
                    created_at: created_at_time,
                },
                status_to_method(&status),
            )
        })
        .collect();

    let total = entries.len() as u32;

    Ok(HistoryResult {
        entries,
        total: Some(total),
    })
}

/// Retrieves history across all resource types (system-level history).
///
/// # Arguments
///
/// * `pool` - Database connection pool
/// * `schema` - Schema manager to get list of tables
/// * `params` - History query parameters (since, at, count, offset)
pub async fn get_system_history(
    pool: &PgPool,
    schema: &SchemaManager,
    params: &HistoryParams,
) -> Result<HistoryResult, StorageError> {
    // Get all resource tables
    let tables = schema
        .list_tables()
        .await
        .map_err(|e| StorageError::internal(format!("Failed to list tables: {e}")))?;

    if tables.is_empty() {
        return Ok(HistoryResult {
            entries: vec![],
            total: Some(0),
        });
    }

    // Default pagination
    let limit = params.count.unwrap_or(100) as i64;
    let offset = params.offset.unwrap_or(0) as i64;

    // Convert time parameters to chrono
    let since_chrono = params.since.map(time_to_chrono);
    let at_chrono = params.at.map(time_to_chrono);

    // Build WHERE clause for time filters
    let mut where_conditions = Vec::new();
    let mut param_idx = 1;

    if since_chrono.is_some() {
        where_conditions.push(format!("updated_at > ${param_idx}"));
        param_idx += 1;
    }

    if at_chrono.is_some() {
        where_conditions.push(format!("updated_at <= ${param_idx}"));
    }

    let where_clause = if where_conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", where_conditions.join(" AND "))
    };

    // Build UNION ALL query across all tables
    let mut unions = Vec::new();
    for table in &tables {
        let history_table = format!("{}_history", table);
        // Include resource_type from the JSONB resource field
        unions.push(format!(
            r#"SELECT id, txid, created_at, updated_at, resource, status::text, resource->>'resourceType' as resource_type
               FROM "{table}" {where_clause}"#
        ));
        unions.push(format!(
            r#"SELECT id, txid, created_at, updated_at, resource, status::text, resource->>'resourceType' as resource_type
               FROM "{history_table}" {where_clause}"#
        ));
    }

    let sql = format!(
        r#"WITH all_versions AS ({})
           SELECT id, txid, created_at, updated_at, resource, status, resource_type
           FROM all_versions
           ORDER BY txid DESC
           LIMIT {limit} OFFSET {offset}"#,
        unions.join(" UNION ALL ")
    );

    // Execute with appropriate bindings
    let rows: Vec<SystemHistoryRow> = match (since_chrono, at_chrono) {
        (Some(since), Some(at)) => query_as(&sql).bind(since).bind(at).fetch_all(pool).await,
        (Some(since), None) => query_as(&sql).bind(since).fetch_all(pool).await,
        (None, Some(at)) => query_as(&sql).bind(at).fetch_all(pool).await,
        (None, None) => query_as(&sql).fetch_all(pool).await,
    }
    .map_err(|e| StorageError::internal(format!("Failed to query system history: {e}")))?;

    // Convert rows to history entries
    let entries: Vec<HistoryEntry> = rows
        .into_iter()
        .map(|(row_id, txid, created_at, updated_at, resource, status, resource_type)| {
            let created_at_time = chrono_to_time(created_at);
            let updated_at_time = chrono_to_time(updated_at);
            let rt = resource_type.unwrap_or_else(|| "Unknown".to_string());
            HistoryEntry::new(
                StoredResource {
                    id: row_id.to_string(),
                    version_id: txid.to_string(),
                    resource_type: rt,
                    resource,
                    last_updated: updated_at_time,
                    created_at: created_at_time,
                },
                status_to_method(&status),
            )
        })
        .collect();

    let total = entries.len() as u32;

    Ok(HistoryResult {
        entries,
        total: Some(total),
    })
}

/// Builds a UNION ALL query for combining current and history tables.
fn build_union_query(
    table: &str,
    history_table: &str,
    has_id: bool,
    since: &Option<DateTime<Utc>>,
    at: &Option<DateTime<Utc>>,
) -> (String, bool, bool, bool) {
    let mut conditions = Vec::new();
    let mut param_idx = 1;

    if has_id {
        conditions.push(format!("id = ${param_idx}"));
        param_idx += 1;
    }

    let has_since = since.is_some();
    if has_since {
        conditions.push(format!("updated_at > ${param_idx}"));
        param_idx += 1;
    }

    let has_at = at.is_some();
    if has_at {
        conditions.push(format!("updated_at <= ${param_idx}"));
    }

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };

    let sql = format!(
        r#"SELECT id, txid, created_at, updated_at, resource, status::text FROM "{table}" {where_clause}
           UNION ALL
           SELECT id, txid, created_at, updated_at, resource, status::text FROM "{history_table}" {where_clause}"#
    );

    (sql, has_id, has_since, has_at)
}

/// Executes a history query with dynamic parameter bindings.
#[allow(clippy::too_many_arguments)]
async fn execute_history_query(
    sql: &str,
    pool: &PgPool,
    id_uuid: Option<Uuid>,
    since: Option<DateTime<Utc>>,
    at: Option<DateTime<Utc>>,
    has_id: bool,
    has_since: bool,
    has_at: bool,
) -> Result<Vec<HistoryRow>, StorageError> {
    // Build query dynamically based on which parameters are present
    let result = match (has_id, has_since, has_at) {
        (true, true, true) => {
            query_as(sql)
                .bind(id_uuid.unwrap())
                .bind(since.unwrap())
                .bind(at.unwrap())
                .fetch_all(pool)
                .await
        }
        (true, true, false) => {
            query_as(sql)
                .bind(id_uuid.unwrap())
                .bind(since.unwrap())
                .fetch_all(pool)
                .await
        }
        (true, false, true) => {
            query_as(sql)
                .bind(id_uuid.unwrap())
                .bind(at.unwrap())
                .fetch_all(pool)
                .await
        }
        (true, false, false) => query_as(sql).bind(id_uuid.unwrap()).fetch_all(pool).await,
        (false, true, true) => {
            query_as(sql)
                .bind(since.unwrap())
                .bind(at.unwrap())
                .fetch_all(pool)
                .await
        }
        (false, true, false) => query_as(sql).bind(since.unwrap()).fetch_all(pool).await,
        (false, false, true) => query_as(sql).bind(at.unwrap()).fetch_all(pool).await,
        (false, false, false) => query_as(sql).fetch_all(pool).await,
    }
    .map_err(|e| {
        if e.to_string().contains("does not exist") {
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

    #[test]
    fn test_build_union_query_no_filters() {
        let (sql, has_id, has_since, has_at) =
            build_union_query("patient", "patient_history", false, &None, &None);
        assert!(!has_id);
        assert!(!has_since);
        assert!(!has_at);
        assert!(sql.contains("UNION ALL"));
        assert!(sql.contains("patient"));
        assert!(sql.contains("patient_history"));
    }

    #[test]
    fn test_build_union_query_with_id() {
        let (sql, has_id, has_since, has_at) =
            build_union_query("patient", "patient_history", true, &None, &None);
        assert!(has_id);
        assert!(!has_since);
        assert!(!has_at);
        assert!(sql.contains("id = $1"));
    }
}
