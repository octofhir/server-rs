//! DB Console API endpoints for schema browsing, query history,
//! active query management, and index operations.

use axum::{
    Extension, Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use octofhir_search::SearchParameterType;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx_core::row::Row;
use std::collections::BTreeMap;
use std::sync::Arc;
use tracing::{info, warn};
use url::form_urlencoded;

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
// Index Advisor
// ============================================================================

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexAdvisorQuery {
    #[serde(default)]
    pub resource_type: Option<String>,
    #[serde(default = "default_index_advisor_limit")]
    pub limit: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyzeIndexesRequest {
    #[serde(default)]
    pub resource_type: Option<String>,
    #[serde(default)]
    pub queries: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexAdvisorResponse {
    pub resource_type: Option<String>,
    pub table: Option<String>,
    pub table_stats: Option<IndexAdvisorTableStats>,
    pub suggestions: Vec<IndexSuggestion>,
    pub observed_queries: Vec<ObservedQuery>,
    pub notes: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexAdvisorTableStats {
    pub row_estimate: i64,
    pub dead_rows: i64,
    pub table_size_bytes: i64,
    pub total_size_bytes: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexSuggestion {
    pub id: String,
    pub category: String,
    pub priority: String,
    pub resource_type: Option<String>,
    pub table: Option<String>,
    pub reason: String,
    pub tradeoff: String,
    pub create_statement: Option<String>,
    pub existing_index: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservedQuery {
    pub source: String,
    pub query: String,
    pub calls: Option<i64>,
    pub mean_time_ms: Option<f64>,
    pub total_time_ms: Option<f64>,
}

#[derive(Debug)]
struct ExistingIndex {
    name: String,
    indexdef: String,
    size_bytes: i64,
    scans: i64,
    is_primary: bool,
    is_unique: bool,
}

fn default_index_advisor_limit() -> i64 {
    20
}

fn priority_rank(priority: &str) -> u8 {
    match priority {
        "high" => 0,
        "medium" => 1,
        "low" => 2,
        _ => 3,
    }
}

fn normalize_resource_table(resource_type: &str) -> Result<String, ApiError> {
    let table = resource_type.to_ascii_lowercase();
    if !is_valid_identifier(&table) {
        return Err(ApiError::bad_request("Invalid resourceType"));
    }
    Ok(table)
}

fn quote_ident(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

async fn load_table_stats(
    pool: &sqlx_postgres::PgPool,
    table: &str,
) -> Result<Option<IndexAdvisorTableStats>, ApiError> {
    let row = sqlx_core::query::query(
        "SELECT \
             COALESCE(s.n_live_tup, 0)::bigint AS row_estimate, \
             COALESCE(s.n_dead_tup, 0)::bigint AS dead_rows, \
             pg_table_size(c.oid)::bigint AS table_size_bytes, \
             pg_total_relation_size(c.oid)::bigint AS total_size_bytes \
         FROM pg_class c \
         JOIN pg_namespace n ON n.oid = c.relnamespace \
         LEFT JOIN pg_stat_user_tables s ON s.relid = c.oid \
         WHERE n.nspname = 'public' AND c.relname = $1",
    )
    .bind(table)
    .fetch_optional(pool)
    .await
    .map_err(|e| ApiError::internal(format!("Failed to fetch table stats: {}", e)))?;

    Ok(row.map(|row| IndexAdvisorTableStats {
        row_estimate: row.try_get::<i64, _>("row_estimate").unwrap_or(0),
        dead_rows: row.try_get::<i64, _>("dead_rows").unwrap_or(0),
        table_size_bytes: row.try_get::<i64, _>("table_size_bytes").unwrap_or(0),
        total_size_bytes: row.try_get::<i64, _>("total_size_bytes").unwrap_or(0),
    }))
}

async fn load_existing_indexes(
    pool: &sqlx_postgres::PgPool,
    table: &str,
) -> Result<Vec<ExistingIndex>, ApiError> {
    let rows = sqlx_core::query::query(
        "SELECT \
             i.relname AS index_name, \
             pg_get_indexdef(i.oid) AS indexdef, \
             pg_relation_size(i.oid)::bigint AS size_bytes, \
             COALESCE(s.idx_scan, 0)::bigint AS scans, \
             ix.indisprimary AS is_primary, \
             ix.indisunique AS is_unique \
         FROM pg_index ix \
         JOIN pg_class t ON t.oid = ix.indrelid \
         JOIN pg_class i ON i.oid = ix.indexrelid \
         JOIN pg_namespace n ON n.oid = t.relnamespace \
         LEFT JOIN pg_stat_user_indexes s ON s.indexrelid = i.oid \
         WHERE n.nspname = 'public' AND t.relname = $1 \
         ORDER BY pg_relation_size(i.oid) DESC",
    )
    .bind(table)
    .fetch_all(pool)
    .await
    .map_err(|e| ApiError::internal(format!("Failed to fetch indexes: {}", e)))?;

    Ok(rows
        .iter()
        .map(|row| ExistingIndex {
            name: row.try_get::<String, _>("index_name").unwrap_or_default(),
            indexdef: row.try_get::<String, _>("indexdef").unwrap_or_default(),
            size_bytes: row.try_get::<i64, _>("size_bytes").unwrap_or(0),
            scans: row.try_get::<i64, _>("scans").unwrap_or(0),
            is_primary: row.try_get::<bool, _>("is_primary").unwrap_or(false),
            is_unique: row.try_get::<bool, _>("is_unique").unwrap_or(false),
        })
        .collect())
}

async fn pg_stat_statements_available(pool: &sqlx_postgres::PgPool) -> Result<bool, ApiError> {
    let row = sqlx_core::query::query("SELECT to_regclass('pg_stat_statements') IS NOT NULL AS ok")
        .fetch_one(pool)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to check pg_stat_statements: {}", e)))?;

    Ok(row.try_get::<bool, _>("ok").unwrap_or(false))
}

async fn load_observed_sql(
    pool: &sqlx_postgres::PgPool,
    table: Option<&str>,
    limit: i64,
) -> Result<Vec<ObservedQuery>, ApiError> {
    if !pg_stat_statements_available(pool).await? {
        return Ok(Vec::new());
    }

    let safe_limit = limit.clamp(1, 100);
    let rows = if let Some(table) = table {
        sqlx_core::query::query(
            "SELECT query, calls::bigint, mean_exec_time::float8, total_exec_time::float8 \
             FROM pg_stat_statements \
             WHERE query ILIKE '%' || $1 || '%' \
             ORDER BY total_exec_time DESC \
             LIMIT $2",
        )
        .bind(table)
        .bind(safe_limit)
        .fetch_all(pool)
        .await
    } else {
        sqlx_core::query::query(
            "SELECT query, calls::bigint, mean_exec_time::float8, total_exec_time::float8 \
             FROM pg_stat_statements \
             ORDER BY total_exec_time DESC \
             LIMIT $1",
        )
        .bind(safe_limit)
        .fetch_all(pool)
        .await
    }
    .map_err(|e| ApiError::internal(format!("Failed to fetch observed SQL: {}", e)))?;

    Ok(rows
        .iter()
        .map(|row| ObservedQuery {
            source: "pg_stat_statements".to_string(),
            query: row.try_get::<String, _>("query").unwrap_or_default(),
            calls: row.try_get::<i64, _>("calls").ok(),
            mean_time_ms: row.try_get::<f64, _>("mean_exec_time").ok(),
            total_time_ms: row.try_get::<f64, _>("total_exec_time").ok(),
        })
        .collect())
}

fn extract_fhir_query_parts(query: &str) -> (Option<String>, Option<String>) {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return (None, None);
    }

    let without_method = trimmed
        .strip_prefix("GET ")
        .or_else(|| trimmed.strip_prefix("POST "))
        .unwrap_or(trimmed)
        .trim();

    let path = without_method
        .split_whitespace()
        .next()
        .unwrap_or(without_method);
    let (path_part, query_part) = match path.split_once('?') {
        Some((p, q)) => (p, Some(q.to_string())),
        None => (path, None),
    };

    let mut segments = path_part.trim_matches('/').split('/');
    let first = segments.next();
    let resource_type = if first == Some("fhir") {
        segments.next()
    } else {
        first
    }
    .filter(|segment| !segment.is_empty() && !segment.starts_with('$') && *segment != "_search")
    .map(ToString::to_string);

    (resource_type, query_part)
}

fn base_param_name(name: &str) -> &str {
    name.split([':', '.']).next().unwrap_or(name)
}

fn analyze_fhir_queries(
    state: &AppState,
    requested_resource_type: Option<&str>,
    queries: &[String],
) -> Vec<IndexSuggestion> {
    let registry = state.search_config.config().registry.clone();
    let mut usage: BTreeMap<(String, String, String), usize> = BTreeMap::new();

    for query in queries {
        let (query_resource_type, query_string) = extract_fhir_query_parts(query);
        let resource_type = requested_resource_type
            .map(ToString::to_string)
            .or(query_resource_type);
        let Some(resource_type) = resource_type else {
            continue;
        };
        let Some(query_string) = query_string else {
            continue;
        };

        for (key, _) in form_urlencoded::parse(query_string.as_bytes()) {
            let key = key.to_string();
            if key.starts_with('_') {
                continue;
            }
            let param_name = base_param_name(&key);
            let Some(param) = registry.get(&resource_type, param_name) else {
                continue;
            };
            let kind = match param.param_type {
                SearchParameterType::String => "string",
                SearchParameterType::Token => "token",
                SearchParameterType::Date => "date",
                SearchParameterType::Reference => "reference",
                SearchParameterType::Quantity => "quantity",
                SearchParameterType::Number => "number",
                SearchParameterType::Uri => "uri",
                SearchParameterType::Composite => "composite",
                SearchParameterType::Special => "special",
            };
            *usage
                .entry((
                    resource_type.clone(),
                    param_name.to_string(),
                    kind.to_string(),
                ))
                .or_default() += 1;
        }
    }

    usage
        .into_iter()
        .map(|((resource_type, param_code, kind), count)| {
            let (priority, reason, tradeoff, create_statement) = match kind.as_str() {
                "reference" => (
                    "low",
                    format!(
                        "`{param_code}` appears in {count} observed request(s); reference search runs in place over the resource JSONB and is covered by the global GIN(resource jsonb_path_ops) index"
                    ),
                    "No extra index needed; the global resource GIN already serves reference containment.",
                    None,
                ),
                "date" => (
                    "medium",
                    format!(
                        "`{param_code}` appears in {count} observed request(s); consider a functional GiST index over fhir_extract_date_min/max(resource) for this param"
                    ),
                    "Date search reads in place; a per-param functional index removes the per-row JSONB extraction cost.",
                    None,
                ),
                "token" => (
                    "medium",
                    format!(
                        "`{param_code}` appears in {count} observed request(s); repeated token search is a candidate for a sparse token projection"
                    ),
                    "A token projection adds write amplification only for registered token params, but avoids broad JSONB GIN dependence on massive resource tables.",
                    None,
                ),
                "string" => (
                    "medium",
                    format!(
                        "`{param_code}` appears in {count} observed request(s); string search may need a workload-specific expression/trigram index"
                    ),
                    "Do not auto-create this: SearchParameter.expression must be mapped to a stable SQL expression first, otherwise the index can be wrong or too wide.",
                    None,
                ),
                "quantity" | "number" => (
                    "medium",
                    format!(
                        "`{param_code}` appears in {count} observed request(s); numeric/quantity search is a candidate for a sparse range projection"
                    ),
                    "A projection speeds range search but adds numeric extraction cost to writes; validate cardinality and query frequency first.",
                    None,
                ),
                _ => (
                    "low",
                    format!("`{param_code}` appears in {count} observed request(s)"),
                    "No automatic index recommendation for this SearchParameter type yet.",
                    None,
                ),
            };

            IndexSuggestion {
                id: format!("fhir-param-{resource_type}-{param_code}"),
                category: "fhir-search-parameter".to_string(),
                priority: priority.to_string(),
                resource_type: Some(resource_type),
                table: None,
                reason,
                tradeoff: tradeoff.to_string(),
                create_statement,
                existing_index: None,
            }
        })
        .collect()
}

fn build_table_suggestions(
    resource_type: Option<&str>,
    table: Option<&str>,
    stats: Option<&IndexAdvisorTableStats>,
    indexes: &[ExistingIndex],
) -> Vec<IndexSuggestion> {
    let mut suggestions = Vec::new();
    let Some(table) = table else {
        return suggestions;
    };

    let has_active_updated_id = indexes.iter().any(|idx| {
        let def = idx.indexdef.to_ascii_lowercase();
        def.contains("updated_at") && def.contains("id") && def.contains("status")
    });

    if !has_active_updated_id {
        suggestions.push(IndexSuggestion {
            id: format!("default-page-{table}"),
            category: "resource-table".to_string(),
            priority: "high".to_string(),
            resource_type: resource_type.map(ToString::to_string),
            table: Some(table.to_string()),
            reason: "Default search/history/export paths need stable updated_at/id keyset pagination on active rows.".to_string(),
            tradeoff: "Small extra B-tree write cost; much cheaper than deep OFFSET scans and safer than adding more JSONB expression indexes.".to_string(),
            create_statement: Some(format!(
                "CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_{table}_active_updated_id ON {} (updated_at DESC, id) WHERE status != 'deleted';",
                quote_ident(table)
            )),
            existing_index: None,
        });
    }

    for idx in indexes {
        let large_unused = idx.scans == 0 && idx.size_bytes > 10 * 1024 * 1024;
        if large_unused && !idx.is_primary && !idx.is_unique {
            suggestions.push(IndexSuggestion {
                id: format!("review-unused-{}", idx.name),
                category: "index-hygiene".to_string(),
                priority: "medium".to_string(),
                resource_type: resource_type.map(ToString::to_string),
                table: Some(table.to_string()),
                reason: format!(
                    "Index `{}` has 0 recorded scans and is larger than 10 MiB.",
                    idx.name
                ),
                tradeoff: "Do not drop automatically; pg_stat counters reset on restart and rare critical queries may still need it. Review with workload history first.".to_string(),
                create_statement: None,
                existing_index: Some(idx.name.clone()),
            });
        }
    }

    if let Some(stats) = stats
        && stats.dead_rows > 0
        && stats.row_estimate > 0
        && stats.dead_rows > stats.row_estimate / 5
    {
        suggestions.push(IndexSuggestion {
            id: format!("vacuum-pressure-{table}"),
            category: "maintenance".to_string(),
            priority: "medium".to_string(),
            resource_type: resource_type.map(ToString::to_string),
            table: Some(table.to_string()),
            reason: "Dead rows are above 20% of live row estimate; index advice may be distorted by bloat.".to_string(),
            tradeoff: "Tune autovacuum or vacuum before adding indexes, otherwise new indexes may hide the real table-bloat problem.".to_string(),
            create_statement: None,
            existing_index: None,
        });
    }

    suggestions
}

/// GET /api/db-console/index-advisor?resourceType=Observation
pub async fn get_index_advisor(
    State(state): State<AppState>,
    Extension(auth_context): Extension<Arc<AuthContext>>,
    Query(query): Query<IndexAdvisorQuery>,
) -> Result<Response, ApiError> {
    check_db_console_access(&state.config.db_console, &auth_context)?;

    let table = query
        .resource_type
        .as_deref()
        .map(normalize_resource_table)
        .transpose()?;

    let stats = if let Some(table) = &table {
        load_table_stats(state.db_pool.as_ref(), table).await?
    } else {
        None
    };
    let indexes = if let Some(table) = &table {
        load_existing_indexes(state.db_pool.as_ref(), table).await?
    } else {
        Vec::new()
    };
    let observed_queries =
        load_observed_sql(state.db_pool.as_ref(), table.as_deref(), query.limit).await?;

    let table_for_suggestions = table.as_deref().filter(|_| stats.is_some());
    let mut suggestions = build_table_suggestions(
        query.resource_type.as_deref(),
        table_for_suggestions,
        stats.as_ref(),
        &indexes,
    );

    let mut notes = Vec::new();
    if table.is_some() && stats.is_none() {
        notes.push("No matching resource table was found in the public schema.".to_string());
    }
    if observed_queries.is_empty() {
        notes.push(
            "pg_stat_statements is unavailable or has no matching SQL yet; POST real FHIR request URLs to /api/db-console/index-advisor/analyze for request-shape advice."
                .to_string(),
        );
    }
    notes.push(
        "Advisor is read-only: it returns candidate SQL, but never runs DDL automatically."
            .to_string(),
    );

    suggestions.sort_by(|a, b| {
        priority_rank(&a.priority)
            .cmp(&priority_rank(&b.priority))
            .then_with(|| a.id.cmp(&b.id))
    });

    Ok((
        StatusCode::OK,
        Json(IndexAdvisorResponse {
            resource_type: query.resource_type,
            table,
            table_stats: stats,
            suggestions,
            observed_queries,
            notes,
        }),
    )
        .into_response())
}

/// POST /api/db-console/index-advisor/analyze
pub async fn analyze_index_advisor(
    State(state): State<AppState>,
    Extension(auth_context): Extension<Arc<AuthContext>>,
    Json(req): Json<AnalyzeIndexesRequest>,
) -> Result<Response, ApiError> {
    check_db_console_access(&state.config.db_console, &auth_context)?;

    let table = req
        .resource_type
        .as_deref()
        .map(normalize_resource_table)
        .transpose()?;
    let stats = if let Some(table) = &table {
        load_table_stats(state.db_pool.as_ref(), table).await?
    } else {
        None
    };
    let indexes = if let Some(table) = &table {
        load_existing_indexes(state.db_pool.as_ref(), table).await?
    } else {
        Vec::new()
    };

    let table_for_suggestions = table.as_deref().filter(|_| stats.is_some());
    let mut suggestions = build_table_suggestions(
        req.resource_type.as_deref(),
        table_for_suggestions,
        stats.as_ref(),
        &indexes,
    );
    suggestions.extend(analyze_fhir_queries(
        &state,
        req.resource_type.as_deref(),
        &req.queries,
    ));
    suggestions.sort_by(|a, b| {
        priority_rank(&a.priority)
            .cmp(&priority_rank(&b.priority))
            .then_with(|| a.id.cmp(&b.id))
    });

    let observed_queries = req
        .queries
        .into_iter()
        .map(|query| ObservedQuery {
            source: "request".to_string(),
            query,
            calls: None,
            mean_time_ms: None,
            total_time_ms: None,
        })
        .collect();

    let mut notes = Vec::new();
    if table.is_some() && stats.is_none() {
        notes.push("No matching resource table was found in the public schema.".to_string());
    }
    notes.push(
        "FHIR request analysis is shape-based; validate every DDL candidate with EXPLAIN before applying it."
            .to_string(),
    );

    Ok((
        StatusCode::OK,
        Json(IndexAdvisorResponse {
            resource_type: req.resource_type,
            table,
            table_stats: stats,
            suggestions,
            observed_queries,
            notes,
        }),
    )
        .into_response())
}

#[cfg(test)]
mod index_advisor_tests {
    use super::{base_param_name, extract_fhir_query_parts, priority_rank};

    #[test]
    fn extracts_resource_type_and_query_from_fhir_url() {
        let (resource_type, query) =
            extract_fhir_query_parts("GET /fhir/Observation?subject=Patient/123&code=x");

        assert_eq!(resource_type.as_deref(), Some("Observation"));
        assert_eq!(query.as_deref(), Some("subject=Patient/123&code=x"));
    }

    #[test]
    fn extracts_resource_type_without_fhir_prefix() {
        let (resource_type, query) = extract_fhir_query_parts("Patient?name=smith");

        assert_eq!(resource_type.as_deref(), Some("Patient"));
        assert_eq!(query.as_deref(), Some("name=smith"));
    }

    #[test]
    fn reduces_modifier_and_chain_to_base_param() {
        assert_eq!(base_param_name("name:contains"), "name");
        assert_eq!(base_param_name("subject.name"), "subject");
    }

    #[test]
    fn priority_rank_orders_high_first() {
        assert!(priority_rank("high") < priority_rank("medium"));
        assert!(priority_rank("medium") < priority_rank("low"));
    }
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
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
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
