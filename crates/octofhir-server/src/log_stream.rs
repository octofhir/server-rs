//! Real-time log streaming via WebSocket.
//!
//! Provides a WebSocket endpoint at `/api/logs/stream` that streams server logs
//! to connected clients in real-time. Uses a custom tracing layer to capture
//! log events and broadcast them to all connected WebSocket clients.

use axum::{
    extract::{
        Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, StatusCode, header::COOKIE},
    response::{IntoResponse, Response},
};
use futures_util::{SinkExt, StreamExt};
use octofhir_auth::token::jwt::AccessTokenClaims;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;
use tokio::sync::broadcast;
use tracing_subscriber::Layer;

use crate::server::AppState;

/// Maximum number of log entries to buffer in the broadcast channel.
const LOG_BUFFER_SIZE: usize = 1000;

/// Global broadcast sender for log events.
static LOG_SENDER: OnceLock<broadcast::Sender<LogEntry>> = OnceLock::new();

/// Log level enumeration matching tracing levels.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl From<&tracing::Level> for LogLevel {
    fn from(level: &tracing::Level) -> Self {
        match *level {
            tracing::Level::TRACE => LogLevel::Trace,
            tracing::Level::DEBUG => LogLevel::Debug,
            tracing::Level::INFO => LogLevel::Info,
            tracing::Level::WARN => LogLevel::Warn,
            tracing::Level::ERROR => LogLevel::Error,
        }
    }
}

/// A single log entry sent to WebSocket clients.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub id: String,
    pub timestamp: String,
    pub level: LogLevel,
    pub target: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fields: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub span: Option<SpanInfo>,
}

/// Information about the span context of a log entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpanInfo {
    pub name: String,
    pub target: String,
}

/// Query parameters for log stream WebSocket connection.
#[derive(Debug, Deserialize)]
pub struct LogStreamParams {
    /// Authentication token (optional if using cookie auth)
    pub token: Option<String>,
    /// Filter by minimum log level (default: info)
    pub level: Option<String>,
}

/// Initialize the log broadcast channel and return the sender.
/// Should be called once during server startup.
pub fn init_log_broadcast() -> broadcast::Sender<LogEntry> {
    let (sender, _) = broadcast::channel(LOG_BUFFER_SIZE);
    let _ = LOG_SENDER.set(sender.clone());
    sender
}

/// Get a reference to the global log sender.
pub fn get_log_sender() -> Option<&'static broadcast::Sender<LogEntry>> {
    LOG_SENDER.get()
}

/// Custom tracing layer that broadcasts log events to WebSocket clients.
pub struct LogBroadcastLayer {
    sender: broadcast::Sender<LogEntry>,
    min_level: tracing::Level,
}

impl LogBroadcastLayer {
    /// Create a new log broadcast layer.
    pub fn new(sender: broadcast::Sender<LogEntry>) -> Self {
        Self {
            sender,
            min_level: tracing::Level::INFO,
        }
    }

    /// Set the minimum log level to broadcast.
    pub fn with_min_level(mut self, level: tracing::Level) -> Self {
        self.min_level = level;
        self
    }
}

impl<S> Layer<S> for LogBroadcastLayer
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
{
    fn on_event(&self, event: &tracing::Event<'_>, ctx: tracing_subscriber::layer::Context<'_, S>) {
        // Check level filter
        if event.metadata().level() > &self.min_level {
            return;
        }

        // Don't broadcast if no receivers
        if self.sender.receiver_count() == 0 {
            return;
        }

        // Extract message and fields from the event
        let mut visitor = LogVisitor::default();
        event.record(&mut visitor);

        // Get span context if available
        let span_info = ctx.event_span(event).map(|span| {
            let meta = span.metadata();
            SpanInfo {
                name: meta.name().to_string(),
                target: meta.target().to_string(),
            }
        });

        let entry = LogEntry {
            id: generate_id(),
            timestamp: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            level: event.metadata().level().into(),
            target: event.metadata().target().to_string(),
            message: visitor.message.unwrap_or_default(),
            fields: if visitor.fields.is_empty() {
                None
            } else {
                Some(serde_json::Value::Object(visitor.fields))
            },
            span: span_info,
        };

        // Broadcast (ignore errors - receivers may have disconnected)
        let _ = self.sender.send(entry);
    }
}

/// Visitor to extract message and fields from tracing events.
#[derive(Default)]
struct LogVisitor {
    message: Option<String>,
    fields: serde_json::Map<String, serde_json::Value>,
}

impl tracing::field::Visit for LogVisitor {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.message = Some(value.to_string());
        } else {
            self.fields.insert(
                field.name().to_string(),
                serde_json::Value::String(value.to_string()),
            );
        }
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        let formatted = format!("{:?}", value);
        if field.name() == "message" {
            self.message = Some(formatted);
        } else {
            self.fields.insert(
                field.name().to_string(),
                serde_json::Value::String(formatted),
            );
        }
    }

    fn record_i64(&mut self, field: &tracing::field::Field, value: i64) {
        self.fields.insert(
            field.name().to_string(),
            serde_json::Value::Number(value.into()),
        );
    }

    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        self.fields.insert(
            field.name().to_string(),
            serde_json::Value::Number(value.into()),
        );
    }

    fn record_bool(&mut self, field: &tracing::field::Field, value: bool) {
        self.fields
            .insert(field.name().to_string(), serde_json::Value::Bool(value));
    }

    fn record_f64(&mut self, field: &tracing::field::Field, value: f64) {
        if let Some(n) = serde_json::Number::from_f64(value) {
            self.fields
                .insert(field.name().to_string(), serde_json::Value::Number(n));
        }
    }
}

/// Generate a unique ID for log entries.
fn generate_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let count = COUNTER.fetch_add(1, Ordering::Relaxed);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("{}-{}", now, count)
}

/// WebSocket handler for log streaming.
///
/// Authenticates via the `token` query parameter or HttpOnly auth cookie,
/// then streams log events to the client.
pub async fn log_stream_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Query(params): Query<LogStreamParams>,
    headers: HeaderMap,
) -> Result<Response, LogStreamError> {
    // Check if log broadcast is initialized
    let sender = get_log_sender().ok_or(LogStreamError::NotConfigured)?;

    // Authenticate (auth is mandatory)
    let auth_state = &state.auth_state;
    let token = params
        .token
        .or_else(|| {
            if !auth_state.cookie_config.enabled {
                return None;
            }
            extract_cookie_token(&headers, auth_state.cookie_config.name.as_str())
        })
        .ok_or(LogStreamError::Unauthorized)?;

    // Decode and validate JWT
    let token_data = auth_state
        .jwt_service
        .decode::<AccessTokenClaims>(&token)
        .map_err(|e| {
            tracing::warn!(error = %e, "Log stream auth failed: invalid token");
            LogStreamError::Unauthorized
        })?;

    // Check for admin/system scope for log access
    if !has_log_access_permission(&token_data.claims) {
        tracing::warn!(
            client_id = %token_data.claims.client_id,
            scope = %token_data.claims.scope,
            "Log stream auth failed: insufficient permissions"
        );
        return Err(LogStreamError::Forbidden);
    }

    tracing::debug!(
        client_id = %token_data.claims.client_id,
        "Log stream WebSocket authenticated"
    );

    // Parse minimum level filter
    let min_level = params
        .level
        .as_deref()
        .map(|s| match s.to_lowercase().as_str() {
            "trace" => LogLevel::Trace,
            "debug" => LogLevel::Debug,
            "info" => LogLevel::Info,
            "warn" | "warning" => LogLevel::Warn,
            "error" => LogLevel::Error,
            _ => LogLevel::Info,
        })
        .unwrap_or(LogLevel::Info);

    let receiver = sender.subscribe();

    Ok(ws.on_upgrade(move |socket| handle_log_stream(socket, receiver, min_level)))
}

/// Handle the log stream WebSocket connection.
async fn handle_log_stream(
    socket: WebSocket,
    mut receiver: broadcast::Receiver<LogEntry>,
    min_level: LogLevel,
) {
    let (mut ws_write, mut ws_read) = socket.split();

    tracing::info!("Log stream WebSocket connected");

    // Spawn a task to handle incoming messages (for ping/pong and close)
    let mut read_task = tokio::spawn(async move {
        while let Some(msg) = ws_read.next().await {
            match msg {
                Ok(Message::Close(_)) => break,
                Ok(Message::Ping(data)) => {
                    // Ping handling is automatic in axum, but we can log it
                    tracing::trace!("Log stream ping received: {:?}", data);
                }
                Err(e) => {
                    tracing::debug!("Log stream read error: {}", e);
                    break;
                }
                _ => {}
            }
        }
    });

    // Stream log entries to the client
    loop {
        tokio::select! {
            result = receiver.recv() => {
                match result {
                    Ok(entry) => {
                        // Apply level filter
                        if should_send_level(entry.level, min_level) {
                            match serde_json::to_string(&entry) {
                                Ok(json) => {
                                    if let Err(e) = ws_write.send(Message::Text(json.into())).await {
                                        tracing::debug!("Log stream write error: {}", e);
                                        break;
                                    }
                                }
                                Err(e) => {
                                    tracing::warn!("Failed to serialize log entry: {}", e);
                                }
                            }
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!("Log stream lagged by {} messages", n);
                        // Continue receiving
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        tracing::debug!("Log broadcast channel closed");
                        break;
                    }
                }
            }
            _ = &mut read_task => {
                tracing::debug!("Log stream client disconnected");
                break;
            }
        }
    }

    tracing::info!("Log stream WebSocket disconnected");
}

/// Check if a log level should be sent based on the minimum level filter.
fn should_send_level(level: LogLevel, min_level: LogLevel) -> bool {
    let level_ord = match level {
        LogLevel::Error => 0,
        LogLevel::Warn => 1,
        LogLevel::Info => 2,
        LogLevel::Debug => 3,
        LogLevel::Trace => 4,
    };
    let min_ord = match min_level {
        LogLevel::Error => 0,
        LogLevel::Warn => 1,
        LogLevel::Info => 2,
        LogLevel::Debug => 3,
        LogLevel::Trace => 4,
    };
    level_ord <= min_ord
}

/// Extract authentication token from cookie header.
fn extract_cookie_token(headers: &HeaderMap, cookie_name: &str) -> Option<String> {
    let cookie_header = headers.get(COOKIE)?.to_str().ok()?;

    for cookie in cookie_header.split(';') {
        let cookie = cookie.trim();
        if let Some((name, value)) = cookie.split_once('=')
            && name.trim() == cookie_name
        {
            let value = value.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }

    None
}

/// Check if the access token has permission to view logs.
fn has_log_access_permission(claims: &AccessTokenClaims) -> bool {
    let scope = &claims.scope;

    for s in scope.split_whitespace() {
        // Exact matches for common permission patterns
        if matches!(
            s,
            "system/*.*"
                | "system/*.read"
                | "system/*.cruds"
                | "admin/*"
                | "admin/*.*"
                | "user/*.*"
                | "user/*.cruds"
                | "logs"
                | "logs.read"
        ) {
            return true;
        }
        // Prefix matches for admin/system scopes
        if s.starts_with("admin/") || s.starts_with("system/") {
            return true;
        }
    }

    false
}

/// Log stream handler errors.
#[derive(Debug)]
pub enum LogStreamError {
    Unauthorized,
    Forbidden,
    NotConfigured,
}

impl IntoResponse for LogStreamError {
    fn into_response(self) -> Response {
        match self {
            LogStreamError::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                "Invalid or missing authentication token",
            )
                .into_response(),
            LogStreamError::Forbidden => (
                StatusCode::FORBIDDEN,
                "Insufficient permissions for log access",
            )
                .into_response(),
            LogStreamError::NotConfigured => (
                StatusCode::SERVICE_UNAVAILABLE,
                "Log streaming not configured",
            )
                .into_response(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_level_filter() {
        assert!(should_send_level(LogLevel::Error, LogLevel::Info));
        assert!(should_send_level(LogLevel::Warn, LogLevel::Info));
        assert!(should_send_level(LogLevel::Info, LogLevel::Info));
        assert!(!should_send_level(LogLevel::Debug, LogLevel::Info));
        assert!(!should_send_level(LogLevel::Trace, LogLevel::Info));

        assert!(should_send_level(LogLevel::Trace, LogLevel::Trace));
        assert!(should_send_level(LogLevel::Error, LogLevel::Trace));
    }
}
