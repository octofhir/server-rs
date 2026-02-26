//! DB Console API endpoints for schema browsing, query history,
//! active query management, and index operations.

use axum::{
    Extension, Json,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx_core::row::Row;
use std::sync::Arc;
use tracing::{info, warn};

use crate::server::AppState;
use octofhir_api::ApiError;
use octofhir_auth::middleware::AuthContext;

/// Shared auth check for all db-console API endpoints.
fn check_db_console_access(
    config: &crate::config::DbConsoleConfig,
    auth_context: &AuthContext,
) -> Result<(), ApiError> {
    if !config.enabled {
        return Err(ApiError::forbidden("DB console is disabled"));
    }

    if let Some(ref required_role) = config.required_role {
        let has_role = auth_context
            .user
            .as_ref()
            .is_some_and(|u| u.roles.iter().any(|r| r == required_role));

        if !has_role {
            return Err(ApiError::forbidden(format!(
                "DB console requires '{}' role",
                required_role
            )));
        }
    }

    Ok(())
}

// ============================================================================
// Query History
// ============================================================================

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryHistoryEntry {
    pub id: String,
    pub user_id: String,
    pub query: String,
    pub execution_time_ms: Option<i64>,
    pub row_count: Option<i32>,
    pub is_error: bool,
    pub error_message: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveHistoryRequest {
    pub query: String,
    #[serde(default)]
    pub execution_time_ms: Option<i64>,
    #[serde(default)]
    pub row_count: Option<i32>,
    #[serde(default)]
    pub is_error: bool,
    #[serde(default)]
    pub error_message: Option<String>,
}

/// GET /api/db-console/history
pub async fn list_history(
    State(state): State<AppState>,
    Extension(auth_context): Extension<Arc<AuthContext>>,
) -> Result<Response, ApiError> {
    check_db_console_access(&state.config.db_console, &auth_context)?;

    let user_id = auth_context.subject();

    let rows = sqlx_core::query::query(
        "SELECT id::text, user_id, query, execution_time_ms, row_count, is_error, error_message, \
         created_at::text \
         FROM db_console_history \
         WHERE user_id = $1 \
         ORDER BY created_at DESC \
         LIMIT 50",
    )
    .bind(user_id)
    .fetch_all(state.db_pool.as_ref())
    .await
    .map_err(|e| ApiError::internal(format!("Failed to fetch history: {}", e)))?;

    let entries: Vec<QueryHistoryEntry> = rows
        .iter()
        .map(|row| QueryHistoryEntry {
            id: row.try_get::<String, _>("id").unwrap_or_default(),
            user_id: row.try_get::<String, _>("user_id").unwrap_or_default(),
            query: row.try_get::<String, _>("query").unwrap_or_default(),
            execution_time_ms: row.try_get::<i64, _>("execution_time_ms").ok(),
            row_count: row.try_get::<i32, _>("row_count").ok(),
            is_error: row.try_get::<bool, _>("is_error").unwrap_or(false),
            error_message: row.try_get::<String, _>("error_message").ok(),
            created_at: row.try_get::<String, _>("created_at").unwrap_or_default(),
        })
        .collect();

    Ok((StatusCode::OK, Json(json!({ "entries": entries }))).into_response())
}

/// POST /api/db-console/history
pub async fn save_history(
    State(state): State<AppState>,
    Extension(auth_context): Extension<Arc<AuthContext>>,
    Json(req): Json<SaveHistoryRequest>,
) -> Result<Response, ApiError> {
    check_db_console_access(&state.config.db_console, &auth_context)?;

    let user_id = auth_context.subject();

    sqlx_core::query::query(
        "INSERT INTO db_console_history (user_id, query, execution_time_ms, row_count, is_error, error_message) \
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(user_id)
    .bind(&req.query)
    .bind(req.execution_time_ms)
    .bind(req.row_count)
    .bind(req.is_error)
    .bind(&req.error_message)
    .execute(state.db_pool.as_ref())
    .await
    .map_err(|e| ApiError::internal(format!("Failed to save history: {}", e)))?;

    Ok((StatusCode::CREATED, Json(json!({ "success": true }))).into_response())
}

/// DELETE /api/db-console/history
pub async fn clear_history(
    State(state): State<AppState>,
    Extension(auth_context): Extension<Arc<AuthContext>>,
) -> Result<Response, ApiError> {
    check_db_console_access(&state.config.db_console, &auth_context)?;

    let user_id = auth_context.subject();

    sqlx_core::query::query("DELETE FROM db_console_history WHERE user_id = $1")
        .bind(user_id)
        .execute(state.db_pool.as_ref())
        .await
        .map_err(|e| ApiError::internal(format!("Failed to clear history: {}", e)))?;

    Ok((StatusCode::OK, Json(json!({ "success": true }))).into_response())
}

// ============================================================================
// Tables
// ============================================================================

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DbTableInfo {
    pub schema: String,
    pub name: String,
    pub table_type: String,
    pub row_estimate: Option<i64>,
}

/// GET /api/db-console/tables
pub async fn list_tables(
    State(state): State<AppState>,
    Extension(auth_context): Extension<Arc<AuthContext>>,
) -> Result<Response, ApiError> {
    check_db_console_access(&state.config.db_console, &auth_context)?;

    let rows = sqlx_core::query::query(
        "SELECT t.table_schema, t.table_name, t.table_type, \
                COALESCE(s.n_live_tup, 0)::bigint AS row_estimate \
         FROM information_schema.tables t \
         LEFT JOIN pg_stat_user_tables s \
           ON s.schemaname = t.table_schema AND s.relname = t.table_name \
         WHERE t.table_schema NOT IN ('pg_catalog', 'information_schema') \
         ORDER BY t.table_schema, t.table_name",
    )
    .fetch_all(state.db_pool.as_ref())
    .await
    .map_err(|e| ApiError::internal(format!("Failed to fetch tables: {}", e)))?;

    let tables: Vec<DbTableInfo> = rows
        .iter()
        .map(|row| DbTableInfo {
            schema: row.try_get::<String, _>("table_schema").unwrap_or_default(),
            name: row.try_get::<String, _>("table_name").unwrap_or_default(),
            table_type: row.try_get::<String, _>("table_type").unwrap_or_default(),
            row_estimate: row.try_get::<i64, _>("row_estimate").ok(),
        })
        .collect();

    Ok((StatusCode::OK, Json(json!({ "tables": tables }))).into_response())
}

// ============================================================================
// Table Detail (columns + indexes)
// ============================================================================

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DbColumnInfo {
    pub name: String,
    pub data_type: String,
    pub is_nullable: bool,
    pub default_value: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DbIndexInfo {
    pub name: String,
    pub columns: Vec<String>,
    pub is_unique: bool,
    pub is_primary: bool,
    pub index_type: String,
    pub size_bytes: Option<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TableDetailResponse {
    pub schema: String,
    pub name: String,
    pub columns: Vec<DbColumnInfo>,
    pub indexes: Vec<DbIndexInfo>,
}

/// GET /api/db-console/tables/{schema}/{table}
pub async fn get_table_detail(
    State(state): State<AppState>,
    Extension(auth_context): Extension<Arc<AuthContext>>,
    Path((schema, table)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    check_db_console_access(&state.config.db_console, &auth_context)?;

    // Fetch columns
    let col_rows = sqlx_core::query::query(
        "SELECT column_name, data_type, is_nullable, column_default \
         FROM information_schema.columns \
         WHERE table_schema = $1 AND table_name = $2 \
         ORDER BY ordinal_position",
    )
    .bind(&schema)
    .bind(&table)
    .fetch_all(state.db_pool.as_ref())
    .await
    .map_err(|e| ApiError::internal(format!("Failed to fetch columns: {}", e)))?;

    let columns: Vec<DbColumnInfo> = col_rows
        .iter()
        .map(|row| DbColumnInfo {
            name: row.try_get::<String, _>("column_name").unwrap_or_default(),
            data_type: row.try_get::<String, _>("data_type").unwrap_or_default(),
            is_nullable: row
                .try_get::<String, _>("is_nullable")
                .map(|v| v == "YES")
                .unwrap_or(true),
            default_value: row.try_get::<String, _>("column_default").ok(),
        })
        .collect();

    // Fetch indexes
    let idx_rows = sqlx_core::query::query(
        "SELECT \
             i.relname AS index_name, \
             am.amname AS index_type, \
             ix.indisunique AS is_unique, \
             ix.indisprimary AS is_primary, \
             pg_relation_size(i.oid) AS size_bytes, \
             array_agg(a.attname ORDER BY array_position(ix.indkey, a.attnum))::text[] AS columns \
         FROM pg_index ix \
         JOIN pg_class t ON t.oid = ix.indrelid \
         JOIN pg_class i ON i.oid = ix.indexrelid \
         JOIN pg_namespace n ON n.oid = t.relnamespace \
         JOIN pg_am am ON am.oid = i.relam \
         JOIN pg_attribute a ON a.attrelid = t.oid AND a.attnum = ANY(ix.indkey) \
         WHERE n.nspname = $1 AND t.relname = $2 \
         GROUP BY i.relname, am.amname, ix.indisunique, ix.indisprimary, i.oid \
         ORDER BY i.relname",
    )
    .bind(&schema)
    .bind(&table)
    .fetch_all(state.db_pool.as_ref())
    .await
    .map_err(|e| ApiError::internal(format!("Failed to fetch indexes: {}", e)))?;

    let indexes: Vec<DbIndexInfo> = idx_rows
        .iter()
        .map(|row| DbIndexInfo {
            name: row.try_get::<String, _>("index_name").unwrap_or_default(),
            columns: row.try_get::<Vec<String>, _>("columns").unwrap_or_default(),
            is_unique: row.try_get::<bool, _>("is_unique").unwrap_or(false),
            is_primary: row.try_get::<bool, _>("is_primary").unwrap_or(false),
            index_type: row.try_get::<String, _>("index_type").unwrap_or_default(),
            size_bytes: row.try_get::<i64, _>("size_bytes").ok(),
        })
        .collect();

    let response = TableDetailResponse {
        schema,
        name: table,
        columns,
        indexes,
    };

    Ok((StatusCode::OK, Json(response)).into_response())
}

// ============================================================================
// Active Queries
// ============================================================================

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveQuery {
    pub pid: i32,
    pub username: Option<String>,
    pub database: Option<String>,
    pub query: Option<String>,
    pub state: Option<String>,
    pub query_start: Option<String>,
    pub duration_ms: Option<i64>,
    pub wait_event_type: Option<String>,
    pub wait_event: Option<String>,
}

/// GET /api/db-console/active-queries
pub async fn list_active_queries(
    State(state): State<AppState>,
    Extension(auth_context): Extension<Arc<AuthContext>>,
) -> Result<Response, ApiError> {
    check_db_console_access(&state.config.db_console, &auth_context)?;

    let rows = sqlx_core::query::query(
        "SELECT \
             pid, \
             usename, \
             datname, \
             query, \
             state, \
             query_start::text, \
             (EXTRACT(EPOCH FROM (now() - query_start)) * 1000)::bigint AS duration_ms, \
             wait_event_type, \
             wait_event \
         FROM pg_stat_activity \
         WHERE datname = current_database() \
           AND pid != pg_backend_pid() \
           AND state IS NOT NULL \
         ORDER BY query_start DESC NULLS LAST",
    )
    .fetch_all(state.db_pool.as_ref())
    .await
    .map_err(|e| ApiError::internal(format!("Failed to fetch active queries: {}", e)))?;

    let queries: Vec<ActiveQuery> = rows
        .iter()
        .map(|row| ActiveQuery {
            pid: row.try_get::<i32, _>("pid").unwrap_or(0),
            username: row.try_get::<String, _>("usename").ok(),
            database: row.try_get::<String, _>("datname").ok(),
            query: row.try_get::<String, _>("query").ok(),
            state: row.try_get::<String, _>("state").ok(),
            query_start: row.try_get::<String, _>("query_start").ok(),
            duration_ms: row.try_get::<i64, _>("duration_ms").ok(),
            wait_event_type: row.try_get::<String, _>("wait_event_type").ok(),
            wait_event: row.try_get::<String, _>("wait_event").ok(),
        })
        .collect();

    Ok((StatusCode::OK, Json(json!({ "queries": queries }))).into_response())
}

// ============================================================================
// Terminate Query
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct TerminateQueryRequest {
    pub pid: i32,
    #[serde(default)]
    pub force: bool,
}

/// POST /api/db-console/terminate-query
pub async fn terminate_query(
    State(state): State<AppState>,
    Extension(auth_context): Extension<Arc<AuthContext>>,
    Json(req): Json<TerminateQueryRequest>,
) -> Result<Response, ApiError> {
    check_db_console_access(&state.config.db_console, &auth_context)?;

    // Require at least readwrite mode for query termination
    if matches!(
        state.config.db_console.sql_mode,
        crate::config::SqlMode::Readonly
    ) {
        return Err(ApiError::forbidden(
            "Query termination requires readwrite or admin mode",
        ));
    }

    let sql = if req.force {
        "SELECT pg_terminate_backend($1)"
    } else {
        "SELECT pg_cancel_backend($1)"
    };

    warn!(
        pid = req.pid,
        force = req.force,
        user = ?auth_context.user.as_ref().map(|u| &u.username),
        "Terminating query"
    );

    let row = sqlx_core::query::query(sql)
        .bind(req.pid)
        .fetch_one(state.db_pool.as_ref())
        .await
        .map_err(|e| ApiError::internal(format!("Failed to terminate query: {}", e)))?;

    let success: bool = row.try_get::<bool, _>(0).unwrap_or(false);

    Ok((
        StatusCode::OK,
        Json(json!({ "success": true, "terminated": success })),
    )
        .into_response())
}

// ============================================================================
// Drop Index
// ============================================================================

/// Validate that an identifier is safe (letters, digits, underscores).
fn is_valid_identifier(s: &str) -> bool {
    !s.is_empty()
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && s.chars()
            .next()
            .map_or(false, |c| c.is_ascii_alphabetic() || c == '_')
}

/// DELETE /api/db-console/indexes/{schema}/{index_name}
pub async fn drop_index(
    State(state): State<AppState>,
    Extension(auth_context): Extension<Arc<AuthContext>>,
    Path((schema, index_name)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    check_db_console_access(&state.config.db_console, &auth_context)?;

    // Require admin mode for DDL operations
    if !matches!(
        state.config.db_console.sql_mode,
        crate::config::SqlMode::Admin
    ) {
        return Err(ApiError::forbidden("Index management requires admin mode"));
    }

    // Validate identifiers to prevent SQL injection
    if !is_valid_identifier(&schema) || !is_valid_identifier(&index_name) {
        return Err(ApiError::bad_request("Invalid schema or index name"));
    }

    warn!(
        schema = %schema,
        index_name = %index_name,
        user = ?auth_context.user.as_ref().map(|u| &u.username),
        "Dropping index"
    );

    // Use quoted identifiers for safety
    let sql = format!("DROP INDEX IF EXISTS \"{}\".\"{}\"", schema, index_name);

    sqlx_core::query::query(&sql)
        .execute(state.db_pool.as_ref())
        .await
        .map_err(|e| ApiError::internal(format!("Failed to drop index: {}", e)))?;

    info!(schema = %schema, index_name = %index_name, "Index dropped successfully");

    Ok((
        StatusCode::OK,
        Json(json!({
            "success": true,
            "message": format!("Index {}.{} dropped", schema, index_name)
        })),
    )
        .into_response())
}
