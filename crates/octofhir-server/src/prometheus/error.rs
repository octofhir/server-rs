//! Errors of the metrics and dashboards API, as `{"error": {"kind", "message"}}` JSON.

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;

#[derive(Debug)]
pub enum MetricsError {
    /// `[metrics.prometheus]` is not configured.
    NotConfigured,
    /// The request itself is malformed.
    BadRequest(String),
    /// The query does not parse or cannot be scoped.
    Query(String),
    /// Prometheus parsed the query but failed to run it.
    Execution(String),
    /// The query ran past the configured timeout.
    Timeout,
    /// Prometheus (or Grafana) could not be reached or refused the request.
    Unavailable(String),
}

impl MetricsError {
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::BadRequest(message.into())
    }

    fn parts(&self) -> (StatusCode, &'static str, String) {
        match self {
            Self::NotConfigured => (
                StatusCode::NOT_FOUND,
                "not_configured",
                "metrics.prometheus is not configured on this server".into(),
            ),
            Self::BadRequest(message) => (StatusCode::BAD_REQUEST, "bad_request", message.clone()),
            Self::Query(message) => (StatusCode::BAD_REQUEST, "bad_query", message.clone()),
            Self::Execution(message) => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "execution",
                message.clone(),
            ),
            Self::Timeout => (
                StatusCode::GATEWAY_TIMEOUT,
                "timeout",
                "the query ran past the configured timeout".into(),
            ),
            Self::Unavailable(message) => (StatusCode::BAD_GATEWAY, "unavailable", message.clone()),
        }
    }
}

impl std::fmt::Display for MetricsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.parts().2)
    }
}

impl IntoResponse for MetricsError {
    fn into_response(self) -> Response {
        let (status, kind, message) = self.parts();
        (
            status,
            Json(json!({ "error": { "kind": kind, "message": message } })),
        )
            .into_response()
    }
}
