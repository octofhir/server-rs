//! SQL execution handler for DB Console.
//!
//! Provides the `$sql` system operation for executing SQL queries with
//! configurable security modes. This endpoint is designed for the DB Console UI.
//!
//! # Security
//!
//! Query execution is controlled by `DbConsoleConfig`:
//! - `readonly`: Only SELECT queries allowed (default)
//! - `readwrite`: SELECT, INSERT, UPDATE, DELETE allowed
//! - `admin`: All SQL including DDL
//!
//! # Parameterized Queries
//!
//! For security, queries can use parameterized placeholders ($1, $2, etc.)
//! with values passed in the `params` array:
//!
//! ```json
//! {
//!   "query": "SELECT * FROM patient WHERE id = $1 AND status = $2",
//!   "params": ["123", "active"]
//! }
//! ```
//!
//! # Configuration
//!
//! ```toml
//! [db_console]
//! enabled = true
//! sql_mode = "readonly"  # or "readwrite" or "admin"
//! required_role = "admin"  # optional: require specific role
//! lsp_enabled = true
//! ```

use axum::{
    Extension, Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use futures_util::TryStreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx_core::column::Column;
use sqlx_core::query::Query;
use sqlx_core::row::Row;
use sqlx_core::sql_str::AssertSqlSafe;
use sqlx_postgres::{PgArguments, Postgres};
use std::sync::Arc;
use std::time::Instant;
use tracing::{debug, info, warn};

use crate::server::AppState;
use octofhir_api::ApiError;
use octofhir_auth::middleware::AuthContext;

/// Bind a JSON value to a sqlx query.
///
/// Converts JSON values to appropriate PostgreSQL types for safe parameterized queries.
fn bind_json_value<'q>(
    query: Query<'q, Postgres, PgArguments>,
    value: &'q Value,
) -> Query<'q, Postgres, PgArguments> {
    match value {
        Value::Null => query.bind(None::<String>),
        Value::Bool(b) => query.bind(*b),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                query.bind(i)
            } else if let Some(f) = n.as_f64() {
                query.bind(f)
            } else {
                // Fallback to string representation
                query.bind(n.to_string())
            }
        }
        Value::String(s) => query.bind(s.as_str()),
        // For arrays and objects, bind as JSONB
        Value::Array(_) | Value::Object(_) => query.bind(value.clone()),
    }
}

/// Request body for SQL execution.
#[derive(Debug, Deserialize)]
pub struct SqlRequest {
    /// The SQL query to execute (supports $1, $2, etc. placeholders)
    pub query: String,
    /// Optional bind parameters for parameterized queries
    #[serde(default)]
    pub params: Vec<Value>,
}

/// Response from SQL execution.
#[derive(Debug, Serialize)]
pub struct SqlResponse {
    /// Column names in order
    pub columns: Vec<String>,
    /// Row data as array of arrays (each inner array is a row)
    pub rows: Vec<Vec<Value>>,
    /// Number of rows returned
    #[serde(rename = "rowCount")]
    pub row_count: usize,
    /// Execution time in milliseconds
    #[serde(rename = "executionTimeMs")]
    pub execution_time_ms: u64,
}

/// Handler for POST /api/$sql
///
/// Executes a SQL query against the database with security validation
/// based on the configured `SqlMode`.
///
/// # Authentication
///
/// This endpoint requires authentication. The `AuthContext` is injected by
/// the authentication middleware.
///
/// # Authorization
///
/// If `db_console.required_role` is configured, the authenticated user must
/// have that role to access this endpoint.
pub async fn sql_operation(
    State(state): State<AppState>,
    Extension(auth_context): Extension<Arc<AuthContext>>,
    Json(req): Json<SqlRequest>,
) -> Result<Response, ApiError> {
    let config = &state.config.db_console;

    // Check if DB console is enabled
    if !config.enabled {
        return Err(ApiError::forbidden("DB console is disabled"));
    }

    // Check required role if configured
    {
        let required_role = config.required_role.as_deref().unwrap_or("admin");
        let has_role = auth_context
            .user
            .as_ref()
            .is_some_and(|u| u.roles.iter().any(|r| r == required_role));

        if !has_role {
            warn!(
                required_role = %required_role,
                user = ?auth_context.user.as_ref().map(|u| &u.username),
                "User lacks required role for DB console"
            );
            return Err(ApiError::forbidden(format!(
                "DB console requires '{}' role",
                required_role
            )));
        }

        info!(
            user = ?auth_context.user.as_ref().map(|u| &u.username),
            role = %required_role,
            "DB console access granted"
        );
    }

    // Validate query against sql_mode
    if let Err(msg) = config.sql_mode.is_query_allowed(&req.query) {
        warn!(
            sql_mode = %config.sql_mode,
            query = %req.query,
            "Query rejected by sql_mode policy"
        );
        return Err(ApiError::forbidden(msg));
    }

    info!(
        sql_mode = %config.sql_mode,
        query_preview = %req.query.chars().take(100).collect::<String>(),
        params_count = req.params.len(),
        "Executing SQL query"
    );

    // Execute query with timing
    let start = Instant::now();

    // Build query with bind parameters
    let rows = execute_sql(&state, config.sql_mode, &req.query, &req.params).await?;

    let execution_time_ms = start.elapsed().as_millis() as u64;

    // Extract column names from first row (if any)
    let columns: Vec<String> = if let Some(first_row) = rows.first() {
        first_row
            .columns()
            .iter()
            .map(|c| c.name().to_string())
            .collect()
    } else {
        Vec::new()
    };

    // Convert rows to array of arrays for efficient JSON serialization
    let mut result_rows: Vec<Vec<Value>> = Vec::with_capacity(rows.len());

    for row in &rows {
        let mut row_data: Vec<Value> = Vec::with_capacity(columns.len());

        for idx in 0..row.columns().len() {
            // Try to extract value as different types
            let value: Value = if let Ok(val) = row.try_get::<String, _>(idx) {
                json!(val)
            } else if let Ok(val) = row.try_get::<i64, _>(idx) {
                json!(val)
            } else if let Ok(val) = row.try_get::<i32, _>(idx) {
                json!(val)
            } else if let Ok(val) = row.try_get::<f64, _>(idx) {
                json!(val)
            } else if let Ok(val) = row.try_get::<bool, _>(idx) {
                json!(val)
            } else if let Ok(val) = row.try_get::<Value, _>(idx) {
                val
            } else {
                // Check for NULL
                let col_type = row.columns()[idx].type_info();
                debug!(column_type = ?col_type, idx = idx, "Unknown column type, returning null");
                json!(null)
            };

            row_data.push(value);
        }

        result_rows.push(row_data);
    }

    let row_count = result_rows.len();

    info!(
        row_count = row_count,
        execution_time_ms = execution_time_ms,
        "SQL query executed successfully"
    );

    let response = SqlResponse {
        columns,
        rows: result_rows,
        row_count,
        execution_time_ms,
    };

    Ok((StatusCode::OK, Json(response)).into_response())
}

/// Shared execution boundary for console and gateway SQL. Restricted modes must
/// never borrow a role capable of escaping PostgreSQL into server files/programs.
pub(crate) async fn execute_sql(
    state: &AppState,
    mode: crate::config::SqlMode,
    sql: &str,
    params: &[Value],
) -> Result<Vec<sqlx_postgres::PgRow>, ApiError> {
    use crate::config::SqlMode;
    mode.is_query_allowed(sql).map_err(ApiError::forbidden)?;
    let mut connection = state
        .db_pool
        .acquire()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    // Committed SQL can change session settings. Never return that session to
    // FHIR traffic; read-only requests roll back their settings with the query.
    if mode != SqlMode::Readonly {
        connection.close_on_drop();
    }
    let mut tx = sqlx_core::connection::Connection::begin(&mut *connection)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let db_error = |error: sqlx_core::error::Error| {
        ApiError::bad_request(format!("Query execution failed: {error}"))
    };
    if mode == SqlMode::Readonly {
        sqlx_core::query::query("SET TRANSACTION READ ONLY")
            .execute(&mut *tx)
            .await
            .map_err(db_error)?;
    }
    {
        let dangerous: bool = sqlx_core::query_scalar::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM pg_roles r \
             WHERE (r.rolsuper OR r.rolcreaterole OR r.rolname IN \
             ('pg_execute_server_program', 'pg_read_server_files', 'pg_write_server_files')) \
             AND pg_has_role(current_user, r.oid, 'MEMBER'))",
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(db_error)?;
        if dangerous {
            return Err(ApiError::forbidden(
                "SQL console requires a PostgreSQL role without superuser, CREATEROLE, or server file/program privileges",
            ));
        }
    }
    let timeout = state
        .config
        .read_timeout()
        .min(std::time::Duration::from_secs(30));
    sqlx_core::query::query("SELECT set_config('statement_timeout', $1, true), set_config('lock_timeout', '5000', true)")
        .bind(timeout.as_millis().max(1).to_string())
        .execute(&mut *tx).await.map_err(db_error)?;
    let mut query = sqlx_core::query::query(AssertSqlSafe(sql.to_owned()));
    for param in params {
        query = bind_json_value(query, param);
    }
    let mut rows = Vec::new();
    let mut bytes = 0usize;
    {
        let mut stream = query.fetch(&mut *tx);
        while let Some(row) = stream.try_next().await.map_err(db_error)? {
            for index in 0..row.columns().len() {
                if let Ok(value) = row.try_get_raw(index) {
                    bytes = bytes.saturating_add(value.as_bytes().map_or(0, |v| v.len()));
                }
            }
            if rows.len() >= 10_000 || bytes > 16 * 1024 * 1024 {
                return Err(ApiError::bad_request(
                    "SQL result exceeds 10000 rows or 16 MiB; use LIMIT",
                ));
            }
            rows.push(row);
        }
    }
    if mode == SqlMode::Readonly {
        tx.rollback().await.map_err(db_error)?;
    } else {
        tx.commit().await.map_err(db_error)?;
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::SqlMode;

    #[test]
    fn test_sql_request_deserialize() {
        let json = r#"{"query": "SELECT * FROM patient"}"#;
        let req: SqlRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.query, "SELECT * FROM patient");
    }

    #[test]
    fn test_sql_response_serialize() {
        let response = SqlResponse {
            columns: vec!["id".to_string(), "name".to_string()],
            rows: vec![vec![json!(1), json!("John")]],
            row_count: 1,
            execution_time_ms: 10,
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"rowCount\":1"));
        assert!(json.contains("\"executionTimeMs\":10"));
    }

    #[test]
    fn test_sql_mode_readonly_allows_select() {
        let mode = SqlMode::Readonly;
        assert!(mode.is_query_allowed("SELECT * FROM users").is_ok());
        assert!(mode.is_query_allowed("select id from patient").is_ok());
        assert!(
            mode.is_query_allowed("WITH cte AS (SELECT 1) SELECT * FROM cte")
                .is_ok()
        );
    }

    #[test]
    fn test_sql_mode_readonly_blocks_mutations() {
        let mode = SqlMode::Readonly;
        assert!(
            mode.is_query_allowed("INSERT INTO users VALUES (1)")
                .is_err()
        );
        assert!(
            mode.is_query_allowed("UPDATE users SET name = 'x'")
                .is_err()
        );
        assert!(mode.is_query_allowed("DELETE FROM users").is_err());
        assert!(mode.is_query_allowed("DROP TABLE users").is_err());
    }

    #[test]
    fn test_sql_mode_readwrite_allows_dml() {
        let mode = SqlMode::Readwrite;
        assert!(mode.is_query_allowed("SELECT * FROM users").is_ok());
        assert!(
            mode.is_query_allowed("INSERT INTO users VALUES (1)")
                .is_ok()
        );
        assert!(mode.is_query_allowed("UPDATE users SET name = 'x'").is_ok());
        assert!(mode.is_query_allowed("DELETE FROM users").is_ok());
    }

    #[test]
    fn test_sql_mode_readwrite_blocks_ddl() {
        let mode = SqlMode::Readwrite;
        assert!(mode.is_query_allowed("DROP TABLE users").is_err());
        assert!(mode.is_query_allowed("CREATE TABLE test (id INT)").is_err());
        assert!(
            mode.is_query_allowed("ALTER TABLE users ADD col INT")
                .is_err()
        );
        assert!(mode.is_query_allowed("TRUNCATE TABLE users").is_err());
    }

    #[test]
    fn test_sql_mode_admin_allows_all() {
        let mode = SqlMode::Admin;
        assert!(mode.is_query_allowed("SELECT * FROM users").is_ok());
        assert!(mode.is_query_allowed("DROP TABLE users").is_ok());
        assert!(mode.is_query_allowed("CREATE TABLE test (id INT)").is_ok());
        assert!(mode.is_query_allowed("TRUNCATE TABLE users").is_ok());
    }

    #[test]
    fn test_sql_mode_readonly_allows_explain_select() {
        let mode = SqlMode::Readonly;
        assert!(mode.is_query_allowed("EXPLAIN SELECT * FROM users").is_ok());
        assert!(
            mode.is_query_allowed("EXPLAIN WITH cte AS (SELECT 1) SELECT * FROM cte")
                .is_ok()
        );
    }

    #[test]
    fn test_sql_mode_readonly_allows_explain_analyze_select() {
        let mode = SqlMode::Readonly;
        assert!(
            mode.is_query_allowed("EXPLAIN ANALYZE SELECT * FROM users")
                .is_ok()
        );
        assert!(
            mode.is_query_allowed("explain analyze select * from patient")
                .is_ok()
        );
    }

    #[test]
    fn test_sql_mode_readonly_blocks_explain_insert() {
        let mode = SqlMode::Readonly;
        assert!(
            mode.is_query_allowed("EXPLAIN INSERT INTO users VALUES (1)")
                .is_err()
        );
    }

    #[test]
    fn test_sql_mode_readonly_blocks_explain_analyze_delete() {
        let mode = SqlMode::Readonly;
        assert!(
            mode.is_query_allowed("EXPLAIN ANALYZE DELETE FROM users")
                .is_err()
        );
    }
}
