use axum::{
    body::Body,
    http::{HeaderName, HeaderValue, Request, StatusCode},
    middleware::Next,
    response::Response,
    Json,
};
use axum::response::IntoResponse;
use serde_json::{json, Value};
use uuid::Uuid;

// Middleware that ensures each request has an X-Request-Id and mirrors it on the response
pub async fn request_id(mut req: Request<Body>, next: Next) -> Response {
    let header_name = HeaderName::from_static("x-request-id");

    // If the incoming request already has a request-id, preserve it; otherwise generate one
    let req_id_value = req
        .headers()
        .get(&header_name)
        .cloned()
        .unwrap_or_else(|| HeaderValue::from_str(&Uuid::new_v4().to_string()).unwrap());

    // Add to request extensions for downstream usage (e.g., logging)
    req.extensions_mut().insert(req_id_value.clone());

    let mut res = next.run(req).await;

    // Add/propagate the request id header to response
    res.headers_mut().insert(header_name.clone(), req_id_value);

    res
}

// Content negotiation middleware: accept FHIR JSON and plain JSON for Accept,
// and require one of them for POST/PUT Content-Type.
pub async fn content_negotiation(req: Request<Body>, next: Next) -> Response {
    let accepts_hdr = req.headers().get("accept").and_then(|v| v.to_str().ok());
    let accept_ok = accepts_hdr.map(|v| {
        let v = v.to_ascii_lowercase();
        v.contains("application/fhir+json") || v.contains("application/json") || v.contains("*/*")
    }).unwrap_or(true); // if missing, treat as ok per HTTP defaults

    if !accept_ok {
        return error_response(StatusCode::UNSUPPORTED_MEDIA_TYPE, "Only JSON is supported (application/fhir+json or application/json) in Accept");
    }

    let method = req.method().clone();
    let needs_body_type = method == axum::http::Method::POST || method == axum::http::Method::PUT;

    if needs_body_type {
        let content_type = req
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_ascii_lowercase());
        let content_ok = content_type.as_deref().map(|s| {
            s.starts_with("application/fhir+json") || s.starts_with("application/json")
        }).unwrap_or(false);
        if !content_ok {
            return error_response(StatusCode::UNSUPPORTED_MEDIA_TYPE, "Content-Type must be application/fhir+json or application/json");
        }
    }

    next.run(req).await
}

fn error_response(status: StatusCode, msg: &str) -> Response {
    let body: Value = json!({
        "resourceType": "OperationOutcome",
        "issue": [{
            "severity": "error",
            "code": "invalid",
            "diagnostics": msg,
        }]
    });
    (status, Json(body)).into_response()
}
