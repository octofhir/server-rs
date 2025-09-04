use axum::{
    extract::{Path, Query},
    http::{header, HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashMap;

#[derive(Serialize)]
pub struct HealthResponse<'a> {
    status: &'a str,
}

pub async fn root() -> impl IntoResponse {
    let body = json!({
        "service": "OctoFHIR Server",
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
    });
    (StatusCode::OK, Json(body))
}

pub async fn healthz() -> impl IntoResponse {
    (StatusCode::OK, Json(HealthResponse { status: "ok" }))
}

pub async fn readyz() -> impl IntoResponse {
    // In future, perform checks for DB/connectivity etc.
    (StatusCode::OK, Json(HealthResponse { status: "ready" }))
}

pub async fn metadata() -> impl IntoResponse {
    let body = json!({
        "resourceType": "CapabilityStatement",
        "status": "draft",
        "kind": "instance",
        "software": { "name": "OctoFHIR Server", "version": env!("CARGO_PKG_VERSION") },
        "format": ["application/fhir+json", "application/json"],
    });
    (StatusCode::OK, Json(body))
}

// ---- CRUD & Search placeholders ----

pub async fn create_resource(
    Path(resource_type): Path<String>,
    Json(_payload): Json<Value>,
) -> impl IntoResponse {
    let span = tracing::info_span!("fhir.create", resource_type = %resource_type);
    let _g = span.enter();
    let msg = format!("Create for resource type '{}' not yet implemented", resource_type);
    tracing::error!(error.kind = "not-supported", message = %msg);
    let outcome = operation_outcome(
        "not-supported",
        msg,
    );
    (StatusCode::NOT_IMPLEMENTED, Json(outcome))
}

pub async fn read_resource(Path((resource_type, id)): Path<(String, String)>) -> impl IntoResponse {
    let span = tracing::info_span!("fhir.read", resource_type = %resource_type, id = %id);
    let _g = span.enter();
    let msg = format!(
        "Read for resource type '{}' and id '{}' not yet implemented",
        resource_type, id
    );
    tracing::error!(error.kind = "not-supported", message = %msg);
    let outcome = operation_outcome(
        "not-supported",
        msg,
    );
    (StatusCode::NOT_IMPLEMENTED, Json(outcome))
}

pub async fn update_resource(
    Path((resource_type, id)): Path<(String, String)>,
    Json(_payload): Json<Value>,
) -> impl IntoResponse {
    let span = tracing::info_span!("fhir.update", resource_type = %resource_type, id = %id);
    let _g = span.enter();
    let msg = format!(
        "Update for resource type '{}' and id '{}' not yet implemented",
        resource_type, id
    );
    tracing::error!(error.kind = "not-supported", message = %msg);
    let outcome = operation_outcome(
        "not-supported",
        msg,
    );
    (StatusCode::NOT_IMPLEMENTED, Json(outcome))
}

pub async fn delete_resource(Path((resource_type, id)): Path<(String, String)>) -> impl IntoResponse {
    let span = tracing::info_span!("fhir.delete", resource_type = %resource_type, id = %id);
    let _g = span.enter();
    let outcome = operation_outcome(
        "not-supported",
        format!(
            "Delete for resource type '{}' and id '{}' not yet implemented",
            resource_type, id
        ),
    );
    (StatusCode::NOT_IMPLEMENTED, Json(outcome))
}

pub async fn search_resource(
    Path(resource_type): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let span = tracing::info_span!("fhir.search", resource_type = %resource_type);
    let _g = span.enter();
    // Read pagination settings from shared config (hot-reloadable)
    let (default_count, max_count) = crate::config::shared::with_config(|c| {
        (c.search.default_count, c.search.max_count)
    }).unwrap_or((10, 100));

    let requested_count = params.get("_count").and_then(|v| v.parse::<usize>().ok());
    let applied_count = match requested_count {
        Some(v) if v > 0 => v.min(max_count),
        _ => default_count,
    };

    tracing::info!(
        resource_type = %resource_type,
        requested_count = ?requested_count,
        default_count = default_count,
        max_count = max_count,
        applied_count = applied_count,
        "search pagination applied"
    );

    tracing::info!(result.count = applied_count, params = ?params, "search request processed");
    let outcome = operation_outcome(
        "not-supported",
        format!(
            "Search for resource type '{}' not yet implemented (applied _count={}; default={}; max={})",
            resource_type, applied_count, default_count, max_count
        ),
    );
    (StatusCode::NOT_IMPLEMENTED, Json(outcome))
}

fn operation_outcome(code: &str, diagnostics: String) -> Value {
    json!({
        "resourceType": "OperationOutcome",
        "issue": [
            {
                "severity": "warning",
                "code": code,
                "diagnostics": diagnostics,
            }
        ]
    })
}

// Special handler for browsers requesting /favicon.ico
pub async fn favicon() -> impl IntoResponse {
    // Minimal, fast response to avoid 404 noise in logs. Browsers accept 204 with no body.
    // Also set a caching header to reduce repeated requests.
    let mut headers = HeaderMap::new();
    headers.insert(header::CACHE_CONTROL, header::HeaderValue::from_static("public, max-age=86400"));
    (StatusCode::NO_CONTENT, headers)
}
