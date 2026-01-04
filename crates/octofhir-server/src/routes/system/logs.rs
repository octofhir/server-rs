//! System logs endpoint for receiving logs from Apps.
//!
//! Apps can submit their logs to `/api/system/logs` for centralized logging.
//! Logs are emitted to the server's tracing infrastructure with app context.

use axum::{http::StatusCode, Json};
use octofhir_auth::extractors::BasicAuth;
use serde::{Deserialize, Serialize};

/// Log level enumeration.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

/// Error information in log entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogError {
    /// Error type (e.g., "ValidationError", "NetworkError")
    #[serde(rename = "type")]
    pub error_type: String,

    /// Error message
    pub message: String,

    /// Optional stack trace
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stack: Option<String>,
}

/// Single log entry from an App.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppLogEntry {
    /// Log level
    pub level: LogLevel,

    /// Log message
    pub message: String,

    /// Optional timestamp from app (ISO 8601)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,

    /// Operation ID (for correlating with app operations)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<String>,

    /// Request ID (for correlating with HTTP requests)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,

    /// User ID (from app's perspective)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,

    /// FHIR user reference (e.g., "Patient/123")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fhir_user: Option<String>,

    /// Additional structured data
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,

    /// Error information (for error-level logs)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<LogError>,
}

/// Batch wrapper for multiple log entries.
#[derive(Debug, Deserialize)]
pub struct LogBatch {
    pub entries: Vec<AppLogEntry>,
}

/// POST /api/system/logs
///
/// Accept array of log entries from Apps.
///
/// # Authentication
///
/// Requires BasicAuth (HTTP Basic Auth with app_id:secret or client_id:secret)
/// Only Apps are allowed to submit logs (not Clients).
///
/// # Example
///
/// ```json
/// [
///   {
///     "level": "info",
///     "message": "Appointment booked successfully",
///     "operationId": "book-appointment",
///     "userId": "user-456",
///     "data": { "appointmentId": "apt-001" }
///   }
/// ]
/// ```
pub async fn handle_app_logs(
    auth: BasicAuth,
    Json(entries): Json<Vec<AppLogEntry>>,
) -> Result<StatusCode, StatusCode> {
    // Only Apps can submit logs
    if !auth.is_app() {
        tracing::warn!(
            entity_id = %auth.entity_id,
            entity_type = "client",
            "Client attempted to submit app logs (forbidden)"
        );
        return Err(StatusCode::FORBIDDEN);
    }

    // Extract app info
    let app = match &auth.entity {
        octofhir_auth::types::BasicAuthEntity::App(app) => app,
        _ => unreachable!("Already checked is_app()"),
    };

    tracing::info!(
        app_id = %app.id,
        app_version = ?app.version,
        entries_count = entries.len(),
        "App logs received"
    );

    for entry in entries {
        emit_log(&app.id, app.version.as_deref(), &entry);
    }

    Ok(StatusCode::ACCEPTED)
}

/// POST /api/system/logs/batch
///
/// Accept batch object with multiple log entries.
///
/// # Example
///
/// ```json
/// {
///   "entries": [
///     {"level": "info", "message": "..."},
///     {"level": "warn", "message": "..."}
///   ]
/// }
/// ```
pub async fn handle_app_logs_batch(
    auth: BasicAuth,
    Json(batch): Json<LogBatch>,
) -> Result<StatusCode, StatusCode> {
    // Only Apps can submit logs
    if !auth.is_app() {
        tracing::warn!(
            entity_id = %auth.entity_id,
            entity_type = "client",
            "Client attempted to submit app logs batch (forbidden)"
        );
        return Err(StatusCode::FORBIDDEN);
    }

    // Extract app info
    let app = match &auth.entity {
        octofhir_auth::types::BasicAuthEntity::App(app) => app,
        _ => unreachable!("Already checked is_app()"),
    };

    tracing::info!(
        app_id = %app.id,
        app_version = ?app.version,
        entries_count = batch.entries.len(),
        "App logs batch received"
    );

    for entry in batch.entries {
        emit_log(&app.id, app.version.as_deref(), &entry);
    }

    Ok(StatusCode::ACCEPTED)
}

/// Emit a log entry to the server's tracing infrastructure.
///
/// Logs are emitted with `target: "app_log"` and include app context fields.
fn emit_log(app_id: &str, app_version: Option<&str>, entry: &AppLogEntry) {
    let version = app_version.unwrap_or("unknown");

    match entry.level {
        LogLevel::Debug => {
            tracing::debug!(
                target: "app_log",
                app_id = %app_id,
                app_version = %version,
                operation_id = ?entry.operation_id,
                request_id = ?entry.request_id,
                user_id = ?entry.user_id,
                fhir_user = ?entry.fhir_user,
                data = ?entry.data,
                "{}",
                entry.message
            );
        }
        LogLevel::Info => {
            tracing::info!(
                target: "app_log",
                app_id = %app_id,
                app_version = %version,
                operation_id = ?entry.operation_id,
                request_id = ?entry.request_id,
                user_id = ?entry.user_id,
                fhir_user = ?entry.fhir_user,
                "{}",
                entry.message
            );
        }
        LogLevel::Warn => {
            tracing::warn!(
                target: "app_log",
                app_id = %app_id,
                app_version = %version,
                operation_id = ?entry.operation_id,
                request_id = ?entry.request_id,
                "{}",
                entry.message
            );
        }
        LogLevel::Error => {
            tracing::error!(
                target: "app_log",
                app_id = %app_id,
                app_version = %version,
                operation_id = ?entry.operation_id,
                request_id = ?entry.request_id,
                error_type = ?entry.error.as_ref().map(|e| &e.error_type),
                error_message = ?entry.error.as_ref().map(|e| &e.message),
                error_stack = ?entry.error.as_ref().and_then(|e| e.stack.as_ref()),
                "{}",
                entry.message
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_entry_deserialization() {
        let json = r#"{
            "level": "info",
            "message": "Test message",
            "operationId": "test-op",
            "userId": "user-123"
        }"#;

        let entry: AppLogEntry = serde_json::from_str(json).unwrap();
        assert_eq!(entry.message, "Test message");
        assert_eq!(entry.operation_id, Some("test-op".to_string()));
        assert_eq!(entry.user_id, Some("user-123".to_string()));
    }

    #[test]
    fn test_log_entry_with_error() {
        let json = r#"{
            "level": "error",
            "message": "Failed to process",
            "error": {
                "type": "ValidationError",
                "message": "Invalid input",
                "stack": "at line 42"
            }
        }"#;

        let entry: AppLogEntry = serde_json::from_str(json).unwrap();
        assert!(entry.error.is_some());
        let error = entry.error.unwrap();
        assert_eq!(error.error_type, "ValidationError");
        assert_eq!(error.message, "Invalid input");
        assert_eq!(error.stack, Some("at line 42".to_string()));
    }

    #[test]
    fn test_log_batch_deserialization() {
        let json = r#"{
            "entries": [
                {"level": "info", "message": "First"},
                {"level": "warn", "message": "Second"}
            ]
        }"#;

        let batch: LogBatch = serde_json::from_str(json).unwrap();
        assert_eq!(batch.entries.len(), 2);
        assert_eq!(batch.entries[0].message, "First");
        assert_eq!(batch.entries[1].message, "Second");
    }
}
