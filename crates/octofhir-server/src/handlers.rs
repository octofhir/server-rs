use axum::{
    extract::{Path, Query},
    http::StatusCode,
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
        "format": ["application/fhir+json"],
    });
    (StatusCode::OK, Json(body))
}

// ---- CRUD & Search placeholders ----

pub async fn create_resource(
    Path(resource_type): Path<String>,
    Json(_payload): Json<Value>,
) -> impl IntoResponse {
    let outcome = operation_outcome(
        "not-supported",
        format!("Create for resource type '{}' not yet implemented", resource_type),
    );
    (StatusCode::NOT_IMPLEMENTED, Json(outcome))
}

pub async fn read_resource(Path((resource_type, id)): Path<(String, String)>) -> impl IntoResponse {
    let outcome = operation_outcome(
        "not-supported",
        format!(
            "Read for resource type '{}' and id '{}' not yet implemented",
            resource_type, id
        ),
    );
    (StatusCode::NOT_IMPLEMENTED, Json(outcome))
}

pub async fn update_resource(
    Path((resource_type, id)): Path<(String, String)>,
    Json(_payload): Json<Value>,
) -> impl IntoResponse {
    let outcome = operation_outcome(
        "not-supported",
        format!(
            "Update for resource type '{}' and id '{}' not yet implemented",
            resource_type, id
        ),
    );
    (StatusCode::NOT_IMPLEMENTED, Json(outcome))
}

pub async fn delete_resource(Path((resource_type, id)): Path<(String, String)>) -> impl IntoResponse {
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
    Query(_params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let outcome = operation_outcome(
        "not-supported",
        format!(
            "Search for resource type '{}' not yet implemented",
            resource_type
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
