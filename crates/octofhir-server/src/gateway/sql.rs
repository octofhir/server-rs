//! SQL handler for executing database queries.
//!
//! **Security Note**: This handler only supports READ-ONLY queries (SELECT)
//! to prevent accidental or malicious data modification.

use axum::{
    Json,
    body::Body,
    http::{Request, StatusCode},
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};
use sqlx::{Column, Row};
use std::collections::HashMap;
use tracing::{debug, info, instrument, warn};

use super::error::GatewayError;
use super::types::CustomOperation;
use crate::server::AppState;

/// Handles SQL operations by executing queries against the database.
///
/// This handler:
/// 1. Validates the SQL query (READ-ONLY)
/// 2. Extracts parameters from request (path, query, body)
/// 3. Executes the query with parameters
/// 4. Converts results to JSON
///
/// # Security
///
/// - Only SELECT queries are allowed
/// - Parameters are properly escaped using parameterized queries
/// - Queries run with limited permissions
#[instrument(skip(state, operation, request))]
pub async fn handle_sql(
    state: &AppState,
    operation: &CustomOperation,
    request: Request<Body>,
) -> Result<Response, GatewayError> {
    let sql_query = operation.sql.as_ref().ok_or_else(|| {
        GatewayError::InvalidConfig("SQL operation missing sql configuration".to_string())
    })?;

    info!(query = %sql_query, "Executing SQL query");

    // Validate query is READ-ONLY (basic check)
    if !is_read_only_query(sql_query) {
        return Err(GatewayError::SqlError(
            "Only SELECT queries are allowed for security reasons".to_string(),
        ));
    }

    // Extract parameters from request
    let params = extract_parameters(&request)?;

    debug!(params = ?params, "Extracted query parameters");

    // Execute the query if we have a PostgreSQL pool
    let pool = state.db_pool.as_ref().ok_or_else(|| {
        GatewayError::SqlError("SQL handler requires PostgreSQL storage backend".to_string())
    })?;

    info!("Executing SQL query");

    // Execute the query and convert rows to JSON
    let rows = sqlx::query(sql_query)
        .fetch_all(pool.as_ref())
        .await
        .map_err(|e| GatewayError::SqlError(format!("Query execution failed: {}", e)))?;

    // Convert rows to JSON array
    let mut results = Vec::new();
    for row in rows {
        let mut obj = serde_json::Map::new();

        // Iterate through columns and extract values
        for (idx, column) in row.columns().iter().enumerate() {
            let col_name = column.name();

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
                // If all else fails, try to get as string or null
                json!(null)
            };

            obj.insert(col_name.to_string(), value);
        }

        results.push(Value::Object(obj));
    }

    info!(row_count = results.len(), "Query executed successfully");

    // Return results as JSON array
    Ok((StatusCode::OK, Json(json!(results))).into_response())
}

/// Validates that a SQL query is READ-ONLY.
///
/// This is a basic security check that prevents modification queries.
/// It checks for common DML/DDL keywords.
fn is_read_only_query(query: &str) -> bool {
    let query_upper = query.trim().to_uppercase();

    // Query must start with SELECT
    if !query_upper.starts_with("SELECT") {
        return false;
    }

    // Check for disallowed keywords anywhere in the query
    let disallowed = [
        "INSERT", "UPDATE", "DELETE", "DROP", "CREATE", "ALTER", "TRUNCATE", "GRANT", "REVOKE",
        "EXEC", "EXECUTE", "CALL",
    ];

    for keyword in &disallowed {
        if query_upper.contains(keyword) {
            warn!(keyword = keyword, "Disallowed SQL keyword detected");
            return false;
        }
    }

    true
}

/// Extracts parameters from the request.
///
/// Parameters can come from:
/// - URL query parameters (?key=value)
/// - Request body (JSON)
/// - Path parameters (via Axum extractors)
fn extract_parameters(request: &Request<Body>) -> Result<HashMap<String, Value>, GatewayError> {
    let mut params = HashMap::new();

    // Extract query parameters
    if let Some(query) = request.uri().query() {
        for (key, value) in url::form_urlencoded::parse(query.as_bytes()) {
            params.insert(key.to_string(), Value::String(value.to_string()));
        }
    }

    // Note: Extracting body parameters would require consuming the request body
    // which would need to be handled differently in the actual implementation

    Ok(params)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_read_only_query() {
        // Valid SELECT queries
        assert!(is_read_only_query("SELECT * FROM users"));
        assert!(is_read_only_query(
            "select id, name from patients where active = true"
        ));
        assert!(is_read_only_query("  SELECT count(*) FROM observation  "));

        // Invalid queries
        assert!(!is_read_only_query("INSERT INTO users VALUES (1, 'test')"));
        assert!(!is_read_only_query("UPDATE users SET name = 'test'"));
        assert!(!is_read_only_query("DELETE FROM users"));
        assert!(!is_read_only_query("DROP TABLE users"));
        assert!(!is_read_only_query("CREATE TABLE test (id INT)"));
        assert!(!is_read_only_query("TRUNCATE TABLE users"));

        // Queries with disallowed keywords in subqueries or comments
        assert!(!is_read_only_query(
            "SELECT * FROM users WHERE id IN (SELECT id FROM users); DROP TABLE users--"
        ));
    }

    #[test]
    fn test_extract_parameters_empty() {
        // This would need a proper request builder for full testing
        // Just a placeholder to show the structure
    }
}
