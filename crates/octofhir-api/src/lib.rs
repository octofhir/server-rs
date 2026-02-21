use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;
use std::sync::Arc;
use thiserror::Error;

// -------------------------
// Raw JSON Type for Zero-Copy Serialization
// -------------------------

/// Raw JSON that serializes directly without re-parsing or allocation.
///
/// Stores a validated `Box<RawValue>` inside an `Arc` so cloning is cheap.
/// Validation happens once at construction; serialization is zero-copy.
///
/// Performance benefit: eliminates per-resource clone + re-validation in
/// search results (100-entry search = 100 allocations saved).
#[derive(Debug, Clone)]
pub struct RawJson(Arc<Box<RawValue>>);

impl RawJson {
    /// Create from a raw JSON string, validating once at construction.
    #[inline]
    pub fn from_string(s: impl Into<String>) -> Self {
        Self(Arc::new(
            RawValue::from_string(s.into()).expect("valid JSON"),
        ))
    }

    /// Get the raw JSON string.
    #[inline]
    pub fn as_str(&self) -> &str {
        self.0.get()
    }

    /// Parse the raw JSON into a Value.
    ///
    /// This is useful when you need to inspect or modify the JSON.
    pub fn to_value(&self) -> Result<serde_json::Value, serde_json::Error> {
        serde_json::from_str(self.0.get())
    }

    /// Extract a string field from the JSON without full parsing.
    ///
    /// For known top-level fields (`id`, `resourceType`), uses a targeted
    /// 1-field struct deserializer instead of building the full Value tree.
    /// Falls back to full parse for other fields.
    pub fn get_str_field(&self, field: &str) -> Option<String> {
        // Targeted extraction for common fields avoids full Value tree
        match field {
            "id" => {
                #[derive(Deserialize)]
                struct IdOnly {
                    id: Option<String>,
                }
                serde_json::from_str::<IdOnly>(self.0.get()).ok()?.id
            }
            "resourceType" => {
                #[derive(Deserialize)]
                struct RtOnly {
                    #[serde(rename = "resourceType")]
                    resource_type: Option<String>,
                }
                serde_json::from_str::<RtOnly>(self.0.get())
                    .ok()?
                    .resource_type
            }
            _ => {
                // Generic fallback: full parse
                let v: serde_json::Value = serde_json::from_str(self.0.get()).ok()?;
                v.get(field).and_then(|v| v.as_str()).map(String::from)
            }
        }
    }
}

impl PartialEq for RawJson {
    fn eq(&self, other: &Self) -> bool {
        self.0.get() == other.0.get()
    }
}

impl Serialize for RawJson {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        // Zero-copy: RawValue serializes directly without allocation
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for RawJson {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        // Deserialize directly into RawValue (no Value tree intermediate)
        let raw = Box::<RawValue>::deserialize(deserializer)?;
        Ok(RawJson(Arc::new(raw)))
    }
}

impl From<serde_json::Value> for RawJson {
    fn from(value: serde_json::Value) -> Self {
        let s = serde_json::to_string(&value).expect("valid JSON");
        Self(Arc::new(RawValue::from_string(s).expect("valid JSON")))
    }
}

impl From<RawJson> for serde_json::Value {
    fn from(raw: RawJson) -> Self {
        raw.to_value().expect("valid JSON")
    }
}

/// Minimal FHIR OperationOutcome representation for API error responses
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OperationOutcome {
    #[serde(rename = "resourceType")]
    pub resource_type: &'static str, // always "OperationOutcome"
    pub issue: Vec<OperationOutcomeIssue>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OperationOutcomeIssue {
    /// FHIR issue severity: fatal | error | warning | information
    pub severity: &'static str,
    /// FHIR issue type code (subset used): invalid | not-found | conflict | forbidden | unauthorized | not-supported | exception
    pub code: &'static str,
    /// Human-readable description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diagnostics: Option<String>,
}

impl OperationOutcome {
    pub fn single(
        severity: &'static str,
        code: &'static str,
        diagnostics: impl Into<String>,
    ) -> Self {
        Self {
            resource_type: "OperationOutcome",
            issue: vec![OperationOutcomeIssue {
                severity,
                code,
                diagnostics: Some(diagnostics.into()),
            }],
        }
    }

    /// Create an OperationOutcome with multiple warning issues.
    ///
    /// Useful for search responses where multiple parameters were ignored.
    pub fn warnings(messages: Vec<String>) -> Self {
        Self {
            resource_type: "OperationOutcome",
            issue: messages
                .into_iter()
                .map(|msg| OperationOutcomeIssue {
                    severity: "warning",
                    code: "not-supported",
                    diagnostics: Some(msg),
                })
                .collect(),
        }
    }

    /// Create a warning for unknown search parameters.
    pub fn unknown_params_warning(param_names: &[String]) -> Self {
        let messages: Vec<String> = param_names
            .iter()
            .map(|name| format!("Unknown search parameter '{}' was ignored", name))
            .collect();
        Self::warnings(messages)
    }

    /// Check if this outcome has any issues.
    pub fn has_issues(&self) -> bool {
        !self.issue.is_empty()
    }
}

/// High-level API errors to be mapped to HTTP responses and FHIR OperationOutcome
#[derive(Debug, Error)]
pub enum ApiError {
    #[error("Bad request: {0}")]
    BadRequest(String),
    #[error("Unauthorized: {0}")]
    Unauthorized(String),
    #[error("Forbidden: {0}")]
    Forbidden(String),
    #[error("Not found: {0}")]
    NotFound(String),
    #[error("Gone: {0}")]
    Gone(String),
    #[error("Conflict: {0}")]
    Conflict(String),
    #[error("Precondition failed: {0}")]
    PreconditionFailed(String),
    #[error("Unsupported media type: {0}")]
    UnsupportedMediaType(String),
    #[error("Not implemented: {0}")]
    NotImplemented(String),
    #[error("Internal server error: {0}")]
    Internal(String),
    #[error("Unprocessable entity: {message}")]
    UnprocessableEntity {
        message: String,
        operation_outcome: Option<serde_json::Value>,
    },
}

impl ApiError {
    pub fn bad_request(msg: impl Into<String>) -> Self {
        Self::BadRequest(msg.into())
    }
    pub fn unauthorized(msg: impl Into<String>) -> Self {
        Self::Unauthorized(msg.into())
    }
    pub fn forbidden(msg: impl Into<String>) -> Self {
        Self::Forbidden(msg.into())
    }
    pub fn not_found(msg: impl Into<String>) -> Self {
        Self::NotFound(msg.into())
    }
    pub fn gone(msg: impl Into<String>) -> Self {
        Self::Gone(msg.into())
    }
    pub fn conflict(msg: impl Into<String>) -> Self {
        Self::Conflict(msg.into())
    }
    pub fn precondition_failed(msg: impl Into<String>) -> Self {
        Self::PreconditionFailed(msg.into())
    }
    pub fn unsupported_media_type(msg: impl Into<String>) -> Self {
        Self::UnsupportedMediaType(msg.into())
    }
    pub fn internal(msg: impl Into<String>) -> Self {
        Self::Internal(msg.into())
    }
    pub fn not_implemented(msg: impl Into<String>) -> Self {
        Self::NotImplemented(msg.into())
    }
    pub fn unprocessable_entity(
        msg: impl Into<String>,
        outcome: Option<serde_json::Value>,
    ) -> Self {
        Self::UnprocessableEntity {
            message: msg.into(),
            operation_outcome: outcome,
        }
    }

    pub fn status_code(&self) -> StatusCode {
        match self {
            ApiError::BadRequest(_) => StatusCode::BAD_REQUEST,
            ApiError::Unauthorized(_) => StatusCode::UNAUTHORIZED,
            ApiError::Forbidden(_) => StatusCode::FORBIDDEN,
            ApiError::NotFound(_) => StatusCode::NOT_FOUND,
            ApiError::Gone(_) => StatusCode::GONE,
            ApiError::Conflict(_) => StatusCode::CONFLICT,
            ApiError::PreconditionFailed(_) => StatusCode::PRECONDITION_FAILED,
            ApiError::UnsupportedMediaType(_) => StatusCode::UNSUPPORTED_MEDIA_TYPE,
            ApiError::NotImplemented(_) => StatusCode::NOT_IMPLEMENTED,
            ApiError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
            ApiError::UnprocessableEntity { .. } => StatusCode::UNPROCESSABLE_ENTITY,
        }
    }

    pub fn to_operation_outcome(&self) -> OperationOutcome {
        match self {
            ApiError::BadRequest(msg) => OperationOutcome::single("error", "invalid", msg),
            ApiError::Unauthorized(msg) => OperationOutcome::single("error", "unauthorized", msg),
            ApiError::Forbidden(msg) => OperationOutcome::single("error", "forbidden", msg),
            ApiError::NotFound(msg) => OperationOutcome::single("error", "not-found", msg),
            ApiError::Gone(msg) => OperationOutcome::single("error", "deleted", msg),
            ApiError::Conflict(msg) => OperationOutcome::single("error", "conflict", msg),
            ApiError::PreconditionFailed(msg) => OperationOutcome::single("error", "conflict", msg),
            ApiError::UnsupportedMediaType(msg) => {
                OperationOutcome::single("error", "not-supported", msg)
            }
            ApiError::NotImplemented(msg) => {
                OperationOutcome::single("error", "not-supported", msg)
            }
            ApiError::Internal(msg) => OperationOutcome::single("fatal", "exception", msg),
            ApiError::UnprocessableEntity { message, .. } => {
                OperationOutcome::single("error", "invalid", message)
            }
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = self.status_code();

        // For UnprocessableEntity, use the provided OperationOutcome if available
        let body = if let ApiError::UnprocessableEntity {
            operation_outcome: Some(outcome),
            ..
        } = &self
        {
            match serde_json::to_vec(outcome) {
                Ok(b) => b,
                Err(_) => {
                    let fallback =
                        OperationOutcome::single("fatal", "exception", "Serialization failure");
                    serde_json::to_vec(&fallback).unwrap_or_else(|_| b"{}".to_vec())
                }
            }
        } else {
            let outcome = self.to_operation_outcome();
            match serde_json::to_vec(&outcome) {
                Ok(b) => b,
                Err(_) => {
                    // Fallback minimal body if serialization fails
                    let fallback =
                        OperationOutcome::single("fatal", "exception", "Serialization failure");
                    serde_json::to_vec(&fallback).unwrap_or_else(|_| b"{}".to_vec())
                }
            }
        };

        let mut builder = axum::http::Response::builder().status(status);
        builder = builder.header(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/fhir+json"),
        );

        builder
            .body(axum::body::Body::from(body))
            .unwrap_or_else(|_| {
                axum::http::Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .header(
                        header::CONTENT_TYPE,
                        HeaderValue::from_static("application/fhir+json"),
                    )
                    .body(axum::body::Body::from("{}"))
                    .expect("build fallback response")
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::response::IntoResponse;

    #[test]
    fn into_response_sets_status_and_content_type() {
        let resp = ApiError::bad_request("Invalid parameter").into_response();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let content_type = resp.headers().get(header::CONTENT_TYPE).unwrap();
        assert_eq!(
            content_type,
            &HeaderValue::from_static("application/fhir+json")
        );
    }

    #[test]
    fn operation_outcome_shape() {
        let outcome = ApiError::not_found("Patient/123 not found").to_operation_outcome();
        assert_eq!(outcome.resource_type, "OperationOutcome");
        assert_eq!(outcome.issue.len(), 1);
        assert_eq!(outcome.issue[0].code, "not-found");
    }

    #[test]
    fn api_error_variants_map_to_status_and_codes() {
        let cases: Vec<(ApiError, StatusCode, &str)> = vec![
            (
                ApiError::bad_request("x"),
                StatusCode::BAD_REQUEST,
                "invalid",
            ),
            (
                ApiError::unauthorized("x"),
                StatusCode::UNAUTHORIZED,
                "unauthorized",
            ),
            (ApiError::forbidden("x"), StatusCode::FORBIDDEN, "forbidden"),
            (ApiError::not_found("x"), StatusCode::NOT_FOUND, "not-found"),
            (ApiError::gone("x"), StatusCode::GONE, "deleted"),
            (ApiError::conflict("x"), StatusCode::CONFLICT, "conflict"),
            (
                ApiError::precondition_failed("x"),
                StatusCode::PRECONDITION_FAILED,
                "conflict",
            ),
            (
                ApiError::unsupported_media_type("x"),
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "not-supported",
            ),
            (
                ApiError::not_implemented("x"),
                StatusCode::NOT_IMPLEMENTED,
                "not-supported",
            ),
            (
                ApiError::internal("x"),
                StatusCode::INTERNAL_SERVER_ERROR,
                "exception",
            ),
        ];
        for (err, status, code) in cases.into_iter() {
            assert_eq!(err.status_code(), status);
            let oo = err.to_operation_outcome();
            assert_eq!(oo.issue[0].code, code);
        }
    }
}

// -------------------------
// API Response Wrapper (Task 3.4.1)
// -------------------------
use axum::http::HeaderName;

#[derive(Debug, Clone)]
pub struct ApiResponse<T> {
    pub value: T,
    pub status: StatusCode,
    pub headers: Vec<(HeaderName, HeaderValue)>,
}

impl<T> ApiResponse<T> {
    pub fn new(value: T, status: StatusCode) -> Self {
        Self {
            value,
            status,
            headers: Vec::new(),
        }
    }

    pub fn ok(value: T) -> Self {
        Self::new(value, StatusCode::OK)
    }

    pub fn with_header(mut self, name: HeaderName, value: HeaderValue) -> Self {
        self.headers.push((name, value));
        self
    }
}

impl<T: Serialize> IntoResponse for ApiResponse<T> {
    fn into_response(self) -> Response {
        let body = match serde_json::to_vec(&self.value) {
            Ok(b) => b,
            Err(_) => serde_json::to_vec(&OperationOutcome::single(
                "fatal",
                "exception",
                "Serialization failure",
            ))
            .unwrap_or_else(|_| b"{}".to_vec()),
        };
        let mut builder = axum::http::Response::builder().status(self.status);
        // Always set FHIR JSON content type
        builder = builder.header(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/fhir+json"),
        );
        // Add extra headers
        for (n, v) in self.headers.into_iter() {
            builder = builder.header(n, v);
        }
        builder
            .body(axum::body::Body::from(body))
            .unwrap_or_else(|_| {
                axum::http::Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .header(
                        header::CONTENT_TYPE,
                        HeaderValue::from_static("application/fhir+json"),
                    )
                    .body(axum::body::Body::from("{}"))
                    .expect("build fallback response")
            })
    }
}

#[cfg(test)]
mod response_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn api_response_ok_sets_status_and_content_type() {
        let payload = json!({"resourceType": "Patient"});
        let resp = ApiResponse::ok(payload).into_response();
        assert_eq!(resp.status(), StatusCode::OK);
        let content_type = resp.headers().get(header::CONTENT_TYPE).unwrap();
        assert_eq!(
            content_type,
            &HeaderValue::from_static("application/fhir+json")
        );
    }

    #[test]
    fn api_response_can_add_headers() {
        let payload = json!({"resourceType": "OperationOutcome"});
        let resp = ApiResponse::ok(payload)
            .with_header(header::ETAG, HeaderValue::from_static("W/\"1\""))
            .into_response();
        let etag = resp.headers().get(header::ETAG).unwrap();
        assert_eq!(etag, &HeaderValue::from_static("W/\"1\""));
    }

    #[test]
    fn api_response_serializes_capability_statement() {
        // Build a minimal capability statement and wrap it in ApiResponse
        let cs = CapabilityStatement::minimal_json_server();
        let resp = ApiResponse::ok(cs.clone()).into_response();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers().get(header::CONTENT_TYPE).unwrap(),
            &HeaderValue::from_static("application/fhir+json")
        );
        // Validate that the CapabilityStatement would serialize to valid JSON
        let j = serde_json::to_value(&cs).expect("capability statement should serialize");
        assert_eq!(j["resourceType"], "CapabilityStatement");
        assert_eq!(j["format"][0], "application/fhir+json");
    }
}

// -------------------------
// Content Negotiation (Task 3.4.3)
// -------------------------
/// Validate the Accept header for JSON responses per FHIR: allow application/fhir+json and application/json
pub fn validate_accept(headers: &HeaderMap) -> Result<(), ApiError> {
    if let Some(accept) = headers.get(header::ACCEPT) {
        let val = accept.to_str().unwrap_or("").to_ascii_lowercase();
        // Quick allow list: application/fhir+json or application/json or */* (common)
        let allowed = val.contains("application/fhir+json")
            || val.contains("application/json")
            || val.contains("*/*");
        if !allowed {
            return Err(ApiError::unsupported_media_type(format!(
                "Unsupported Accept: {val}. Only application/fhir+json or application/json are supported."
            )));
        }
    }
    Ok(())
}

/// Validate Content-Type for requests with bodies: require application/fhir+json or application/json
pub fn validate_content_type(headers: &HeaderMap) -> Result<(), ApiError> {
    if let Some(ct) = headers.get(header::CONTENT_TYPE) {
        let val = ct.to_str().unwrap_or("").to_ascii_lowercase();
        let allowed =
            val.starts_with("application/fhir+json") || val.starts_with("application/json");
        if !allowed {
            return Err(ApiError::unsupported_media_type(format!(
                "Unsupported Content-Type: {val}. Only application/fhir+json or application/json are supported."
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod content_negotiation_tests {
    use super::*;

    #[test]
    fn accept_allows_fhir_json_and_json() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::ACCEPT,
            HeaderValue::from_static("application/fhir+json"),
        );
        assert!(validate_accept(&headers).is_ok());
        headers.insert(header::ACCEPT, HeaderValue::from_static("application/json"));
        assert!(validate_accept(&headers).is_ok());
    }

    #[test]
    fn accept_allows_wildcard() {
        let mut headers = HeaderMap::new();
        headers.insert(header::ACCEPT, HeaderValue::from_static("*/*"));
        assert!(validate_accept(&headers).is_ok());
    }

    #[test]
    fn accept_rejects_xml() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::ACCEPT,
            HeaderValue::from_static("application/fhir+xml"),
        );
        let err = validate_accept(&headers).unwrap_err();
        assert_eq!(err.status_code(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }

    #[test]
    fn content_type_allows_json_variants() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/fhir+json; charset=UTF-8"),
        );
        assert!(validate_content_type(&headers).is_ok());
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
        assert!(validate_content_type(&headers).is_ok());
    }

    #[test]
    fn content_type_rejects_xml() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/fhir+xml"),
        );
        let err = validate_content_type(&headers).unwrap_err();
        assert_eq!(err.status_code(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }
}

// -------------------------
// FHIR Bundle Types (Tasks 3.2.1, 3.2.2)
// -------------------------
use serde_json::Value as JsonValue;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BundleLink {
    #[serde(rename = "relation")]
    pub relation: String,
    #[serde(rename = "url")]
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BundleEntry {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "fullUrl")]
    pub full_url: Option<String>,
    /// The resource content as raw JSON for efficient serialization.
    /// Use `RawJson::from(value)` to convert from `serde_json::Value`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource: Option<RawJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search: Option<BundleEntrySearch>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request: Option<BundleEntryRequest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response: Option<BundleEntryResponse>,
}

/// Search component of a Bundle entry (used in searchset bundles)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BundleEntrySearch {
    /// "match" for main search results, "include" for _include/_revinclude results
    pub mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<f64>,
}

/// Request component of a Bundle entry (used in history bundles)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BundleEntryRequest {
    pub method: String,
    pub url: String,
}

/// Response component of a Bundle entry (used in history bundles)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BundleEntryResponse {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub etag: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "lastModified")]
    pub last_modified: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Bundle {
    #[serde(rename = "resourceType")]
    pub resource_type: &'static str,
    #[serde(rename = "type")]
    pub bundle_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<u64>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub link: Vec<BundleLink>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub entry: Vec<BundleEntry>,
}

impl Bundle {
    pub fn searchset(total: u64, entries: Vec<BundleEntry>, links: Vec<BundleLink>) -> Self {
        Self {
            resource_type: "Bundle",
            bundle_type: "searchset".to_string(),
            total: Some(total),
            link: links,
            entry: entries,
        }
    }

    pub fn history(entries: Vec<BundleEntry>, links: Vec<BundleLink>) -> Self {
        Self {
            resource_type: "Bundle",
            bundle_type: "history".to_string(),
            total: None,
            link: links,
            entry: entries,
        }
    }

    pub fn collection(entries: Vec<BundleEntry>) -> Self {
        Self {
            resource_type: "Bundle",
            bundle_type: "collection".to_string(),
            total: None,
            link: Vec::new(),
            entry: entries,
        }
    }
}

#[cfg(test)]
mod bundle_tests {
    use super::*;

    #[test]
    fn serialize_searchset_bundle() {
        let entry = BundleEntry {
            full_url: Some("http://example.org/Patient/1".into()),
            resource: Some(serde_json::json!({"resourceType":"Patient","id":"1"}).into()),
            search: Some(BundleEntrySearch {
                mode: "match".into(),
                score: None,
            }),
            request: None,
            response: None,
        };
        let link_self = BundleLink {
            relation: "self".into(),
            url: "http://example.org/Patient?_count=1".into(),
        };
        let b = Bundle::searchset(1, vec![entry], vec![link_self]);
        let j = serde_json::to_value(&b).unwrap();
        assert_eq!(j["resourceType"], "Bundle");
        assert_eq!(j["type"], "searchset");
        assert_eq!(j["total"], 1);
        assert!(j["entry"].is_array());
        assert!(j["link"].is_array());
        assert_eq!(j["entry"][0]["search"]["mode"], "match");
    }

    #[test]
    fn serialize_history_bundle() {
        let entry = BundleEntry {
            full_url: Some("http://example.org/Patient/1".into()),
            resource: Some(serde_json::json!({"resourceType":"Patient","id":"1"}).into()),
            search: None,
            request: Some(BundleEntryRequest {
                method: "PUT".into(),
                url: "Patient/1".into(),
            }),
            response: Some(BundleEntryResponse {
                status: "200 OK".into(),
                etag: Some("W/\"2\"".into()),
                last_modified: Some("2023-01-15T10:30:00Z".into()),
            }),
        };
        let link_self = BundleLink {
            relation: "self".into(),
            url: "http://example.org/Patient/1/_history".into(),
        };
        let b = Bundle::history(vec![entry], vec![link_self]);
        let j = serde_json::to_value(&b).unwrap();
        assert_eq!(j["resourceType"], "Bundle");
        assert_eq!(j["type"], "history");
        assert!(j["entry"].is_array());
        assert_eq!(j["entry"][0]["request"]["method"], "PUT");
        assert_eq!(j["entry"][0]["response"]["status"], "200 OK");
    }
}

// -------------------------
// Caching headers utilities (Task 3.4.2)
// -------------------------
impl<T> ApiResponse<T> {
    pub fn with_etag_weak(mut self, version: impl Into<String>) -> Self {
        let tag = format!("W/\"{}\"", version.into());
        if let Ok(val) = HeaderValue::from_str(&tag) {
            self.headers.push((header::ETAG, val));
        }
        self
    }

    /// Provide a raw Last-Modified header value (RFC1123 dates recommended)
    pub fn with_last_modified_raw(mut self, last_modified: impl Into<String>) -> Self {
        if let Ok(val) = HeaderValue::from_str(&last_modified.into()) {
            self.headers.push((header::LAST_MODIFIED, val));
        }
        self
    }
}

/// Check If-None-Match against a weak ETag constructed from version.
/// Returns true if the request's If-None-Match matches the provided version (i.e., should return 304 Not Modified).
pub fn check_if_none_match(headers: &HeaderMap, version: impl Into<String>) -> bool {
    let needle = format!("W/\"{}\"", version.into());
    if let Some(val) = headers.get(header::IF_NONE_MATCH) {
        if let Ok(s) = val.to_str() {
            // If-None-Match may contain a list of etags separated by commas
            return s.split(',').any(|part| part.trim() == needle);
        }
    }
    false
}

#[cfg(test)]
mod caching_tests {
    use super::*;

    #[test]
    fn etag_and_last_modified_headers_added() {
        let payload = serde_json::json!({"resourceType":"Patient","id":"1"});
        let resp = ApiResponse::ok(payload)
            .with_etag_weak("7")
            .with_last_modified_raw("Wed, 21 Oct 2015 07:28:00 GMT")
            .into_response();
        assert_eq!(
            resp.headers().get(header::ETAG).unwrap(),
            &HeaderValue::from_static("W/\"7\"")
        );
        assert_eq!(
            resp.headers().get(header::LAST_MODIFIED).unwrap(),
            &HeaderValue::from_static("Wed, 21 Oct 2015 07:28:00 GMT")
        );
    }

    #[test]
    fn if_none_match_matches_weak_etag() {
        let mut headers = HeaderMap::new();
        headers.insert(header::IF_NONE_MATCH, HeaderValue::from_static("W/\"7\""));
        assert!(check_if_none_match(&headers, "7"));
        assert!(!check_if_none_match(&headers, "8"));
    }
}

// -------------------------
// Search result → Bundle generation (Task 3.2.3)
// -------------------------
fn join_url(base: &str, path: &str) -> String {
    let base = base.trim_end_matches('/');
    let path = path.trim_start_matches('/');
    format!("{base}/{path}")
}

fn build_page_url(
    base_url: &str,
    resource_type: &str,
    offset: usize,
    count: usize,
    query_suffix: Option<&str>,
) -> String {
    let mut url = format!(
        "{}/{}?_count={}&_offset={}",
        base_url.trim_end_matches('/'),
        resource_type,
        count,
        offset
    );
    if let Some(q) = query_suffix {
        if !q.is_empty() {
            // ensure starts with '&' or '?', we append as additional params
            if q.starts_with('&') {
                url.push_str(q);
            } else if q.starts_with('?') {
                url.push_str(&q.replace('?', "&"));
            } else {
                url.push('&');
                url.push_str(q);
            }
        }
    }
    url
}

/// Raw included resource entry for optimized serialization.
pub struct RawIncludedEntry {
    /// Raw JSON resource
    pub resource: RawJson,
    /// Resource type
    pub resource_type: String,
    /// Resource ID
    pub id: String,
}

/// Create a search bundle from raw JSON resources (optimized path).
///
/// Uses `RawJson` to avoid re-parsing JSON during serialization.
#[allow(clippy::too_many_arguments)]
pub fn bundle_from_search_raw(
    total: usize,
    resources: Vec<RawJson>,
    resource_ids: Vec<String>,
    included: Vec<RawIncludedEntry>,
    base_url: &str,
    resource_type: &str,
    offset: usize,
    count: usize,
    query_suffix: Option<&str>,
) -> Bundle {
    let mut entries = Vec::with_capacity(resources.len() + included.len());

    // Add main match entries
    for (res, id) in resources.into_iter().zip(resource_ids.into_iter()) {
        let full_url = Some(join_url(base_url, &format!("{resource_type}/{id}")));
        entries.push(BundleEntry {
            full_url,
            resource: Some(res),
            search: Some(BundleEntrySearch {
                mode: "match".to_string(),
                score: None,
            }),
            request: None,
            response: None,
        });
    }

    // Add included entries
    for inc in included {
        let full_url = Some(join_url(
            base_url,
            &format!("{}/{}", inc.resource_type, inc.id),
        ));
        entries.push(BundleEntry {
            full_url,
            resource: Some(inc.resource),
            search: Some(BundleEntrySearch {
                mode: "include".to_string(),
                score: None,
            }),
            request: None,
            response: None,
        });
    }

    let links = build_search_links(total, base_url, resource_type, offset, count, query_suffix);
    Bundle::searchset(total as u64, entries, links)
}

/// Create a search bundle from raw JSON resources with optional warnings.
///
/// Similar to `bundle_from_search_raw` but allows including an OperationOutcome
/// entry at the beginning of the bundle to communicate warnings (e.g., unknown
/// parameters that were ignored in lenient mode).
///
/// According to FHIR spec, the OperationOutcome is added as the first entry
/// with search mode "outcome".
#[allow(clippy::too_many_arguments)]
pub fn bundle_from_search_raw_with_warnings(
    total: usize,
    resources: Vec<RawJson>,
    resource_ids: Vec<String>,
    included: Vec<RawIncludedEntry>,
    base_url: &str,
    resource_type: &str,
    offset: usize,
    count: usize,
    query_suffix: Option<&str>,
    warnings: Option<OperationOutcome>,
) -> Bundle {
    let mut entries = Vec::with_capacity(resources.len() + included.len() + 1);

    // Add OperationOutcome as first entry if there are warnings
    if let Some(outcome) = warnings {
        if outcome.has_issues() {
            let outcome_json =
                serde_json::to_value(&outcome).expect("OperationOutcome should always serialize");
            entries.push(BundleEntry {
                full_url: None,
                resource: Some(RawJson::from(outcome_json)),
                search: Some(BundleEntrySearch {
                    mode: "outcome".to_string(),
                    score: None,
                }),
                request: None,
                response: None,
            });
        }
    }

    // Add main match entries
    for (res, id) in resources.into_iter().zip(resource_ids.into_iter()) {
        let full_url = Some(join_url(base_url, &format!("{resource_type}/{id}")));
        entries.push(BundleEntry {
            full_url,
            resource: Some(res),
            search: Some(BundleEntrySearch {
                mode: "match".to_string(),
                score: None,
            }),
            request: None,
            response: None,
        });
    }

    // Add included entries
    for inc in included {
        let full_url = Some(join_url(
            base_url,
            &format!("{}/{}", inc.resource_type, inc.id),
        ));
        entries.push(BundleEntry {
            full_url,
            resource: Some(inc.resource),
            search: Some(BundleEntrySearch {
                mode: "include".to_string(),
                score: None,
            }),
            request: None,
            response: None,
        });
    }

    let links = build_search_links(total, base_url, resource_type, offset, count, query_suffix);
    Bundle::searchset(total as u64, entries, links)
}

pub fn bundle_from_search(
    total: usize,
    resources_json: Vec<JsonValue>,
    base_url: &str,
    resource_type: &str,
    offset: usize,
    count: usize,
    query_suffix: Option<&str>,
) -> Bundle {
    let mut entries = Vec::with_capacity(resources_json.len());
    for res in resources_json.into_iter() {
        let full_url = res
            .get("id")
            .and_then(|v| v.as_str())
            .map(|id| join_url(base_url, &format!("{resource_type}/{id}")));
        entries.push(BundleEntry {
            full_url,
            resource: Some(RawJson::from(res)),
            search: Some(BundleEntrySearch {
                mode: "match".to_string(),
                score: None,
            }),
            request: None,
            response: None,
        });
    }

    // compute links
    let mut links = Vec::new();
    // self
    links.push(BundleLink {
        relation: "self".to_string(),
        url: build_page_url(base_url, resource_type, offset, count, query_suffix),
    });

    // first
    links.push(BundleLink {
        relation: "first".to_string(),
        url: build_page_url(base_url, resource_type, 0, count, query_suffix),
    });

    // last
    if count > 0 && total > 0 {
        let last_offset = ((total - 1) / count) * count;
        links.push(BundleLink {
            relation: "last".to_string(),
            url: build_page_url(base_url, resource_type, last_offset, count, query_suffix),
        });
    } else {
        links.push(BundleLink {
            relation: "last".to_string(),
            url: build_page_url(base_url, resource_type, 0, count, query_suffix),
        });
    }

    // prev
    if offset > 0 {
        let prev_offset = offset.saturating_sub(count);
        links.push(BundleLink {
            relation: "previous".to_string(),
            url: build_page_url(base_url, resource_type, prev_offset, count, query_suffix),
        });
    }

    // next
    if count > 0 && offset + count < total {
        let next_offset = offset + count;
        links.push(BundleLink {
            relation: "next".to_string(),
            url: build_page_url(base_url, resource_type, next_offset, count, query_suffix),
        });
    }

    let links = build_search_links(total, base_url, resource_type, offset, count, query_suffix);
    Bundle::searchset(total as u64, entries, links)
}

/// Represents an included resource for bundle generation.
#[derive(Debug, Clone)]
pub struct IncludedResourceEntry {
    /// The resource JSON
    pub resource: RawJson,
    /// Resource type
    pub resource_type: String,
    /// Resource ID
    pub id: String,
}

/// Create a search bundle with included resources.
///
/// Match resources have `search.mode = "match"`, included resources have `search.mode = "include"`.
#[allow(clippy::too_many_arguments)]
pub fn bundle_from_search_with_includes(
    total: usize,
    resources_json: Vec<JsonValue>,
    included_resources: Vec<IncludedResourceEntry>,
    base_url: &str,
    resource_type: &str,
    offset: usize,
    count: usize,
    query_suffix: Option<&str>,
) -> Bundle {
    let total_entries = resources_json.len() + included_resources.len();
    let mut entries = Vec::with_capacity(total_entries);

    // Add match entries
    for res in resources_json.into_iter() {
        let full_url = res
            .get("id")
            .and_then(|v| v.as_str())
            .map(|id| join_url(base_url, &format!("{resource_type}/{id}")));
        entries.push(BundleEntry {
            full_url,
            resource: Some(RawJson::from(res)),
            search: Some(BundleEntrySearch {
                mode: "match".to_string(),
                score: None,
            }),
            request: None,
            response: None,
        });
    }

    // Add include entries
    for inc in included_resources.into_iter() {
        let full_url = join_url(base_url, &format!("{}/{}", inc.resource_type, inc.id));
        entries.push(BundleEntry {
            full_url: Some(full_url),
            resource: Some(inc.resource),
            search: Some(BundleEntrySearch {
                mode: "include".to_string(),
                score: None,
            }),
            request: None,
            response: None,
        });
    }

    let links = build_search_links(total, base_url, resource_type, offset, count, query_suffix);
    // Note: total only counts match entries, not includes
    Bundle::searchset(total as u64, entries, links)
}

/// Build pagination links for search results.
pub fn build_search_links(
    total: usize,
    base_url: &str,
    resource_type: &str,
    offset: usize,
    count: usize,
    query_suffix: Option<&str>,
) -> Vec<BundleLink> {
    let mut links = Vec::new();

    // self
    links.push(BundleLink {
        relation: "self".to_string(),
        url: build_page_url(base_url, resource_type, offset, count, query_suffix),
    });

    // first
    links.push(BundleLink {
        relation: "first".to_string(),
        url: build_page_url(base_url, resource_type, 0, count, query_suffix),
    });

    // last
    if count > 0 && total > 0 {
        let last_offset = ((total - 1) / count) * count;
        links.push(BundleLink {
            relation: "last".to_string(),
            url: build_page_url(base_url, resource_type, last_offset, count, query_suffix),
        });
    } else {
        links.push(BundleLink {
            relation: "last".to_string(),
            url: build_page_url(base_url, resource_type, 0, count, query_suffix),
        });
    }

    // prev
    if offset > 0 {
        let prev_offset = offset.saturating_sub(count);
        links.push(BundleLink {
            relation: "previous".to_string(),
            url: build_page_url(base_url, resource_type, prev_offset, count, query_suffix),
        });
    }

    // next
    if count > 0 && offset + count < total {
        let next_offset = offset + count;
        links.push(BundleLink {
            relation: "next".to_string(),
            url: build_page_url(base_url, resource_type, next_offset, count, query_suffix),
        });
    }

    links
}

// -------------------------
// History Bundle Generation
// -------------------------

/// Represents a history entry for bundle generation
#[derive(Debug, Clone)]
pub struct HistoryBundleEntry {
    /// The resource JSON
    pub resource: RawJson,
    /// Resource ID
    pub id: String,
    /// Resource type
    pub resource_type: String,
    /// Version ID
    pub version_id: String,
    /// Last modified timestamp (RFC3339 format)
    pub last_modified: String,
    /// HTTP method that created this entry
    pub method: HistoryBundleMethod,
}

/// HTTP method for history entries
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryBundleMethod {
    Create,
    Update,
    Delete,
}

impl HistoryBundleMethod {
    pub fn http_method(&self) -> &'static str {
        match self {
            Self::Create => "POST",
            Self::Update => "PUT",
            Self::Delete => "DELETE",
        }
    }

    pub fn http_status(&self) -> &'static str {
        match self {
            Self::Create => "201 Created",
            Self::Update => "200 OK",
            Self::Delete => "204 No Content",
        }
    }
}

/// Build a history bundle from history entries
pub fn bundle_from_history(
    entries: Vec<HistoryBundleEntry>,
    base_url: &str,
    resource_type: &str,
    id: Option<&str>,
    offset: usize,
    count: usize,
    total: Option<u32>,
) -> Bundle {
    let bundle_entries: Vec<BundleEntry> = entries
        .into_iter()
        .map(|entry| {
            let full_url = format!("{}/{}/{}", base_url, entry.resource_type, entry.id);

            let request_url = match entry.method {
                HistoryBundleMethod::Create => entry.resource_type.clone(),
                HistoryBundleMethod::Update | HistoryBundleMethod::Delete => {
                    format!("{}/{}", entry.resource_type, entry.id)
                }
            };

            // For DELETE entries, don't include the resource
            let resource = if entry.method == HistoryBundleMethod::Delete {
                None
            } else {
                Some(entry.resource)
            };

            BundleEntry {
                full_url: Some(full_url),
                resource,
                search: None, // History entries don't have search mode
                request: Some(BundleEntryRequest {
                    method: entry.method.http_method().to_string(),
                    url: request_url,
                }),
                response: Some(BundleEntryResponse {
                    status: entry.method.http_status().to_string(),
                    etag: Some(format!("W/\"{}\"", entry.version_id)),
                    last_modified: Some(entry.last_modified),
                }),
            }
        })
        .collect();

    // Build links
    let mut links = Vec::new();

    // Build self link URL
    let self_url = build_history_link(base_url, resource_type, id, offset, count);
    links.push(BundleLink {
        relation: "self".to_string(),
        url: self_url,
    });

    // Pagination links
    if let Some(total_count) = total {
        let total_usize = total_count as usize;

        // first
        links.push(BundleLink {
            relation: "first".to_string(),
            url: build_history_link(base_url, resource_type, id, 0, count),
        });

        // prev
        if offset > 0 {
            let prev_offset = offset.saturating_sub(count);
            links.push(BundleLink {
                relation: "previous".to_string(),
                url: build_history_link(base_url, resource_type, id, prev_offset, count),
            });
        }

        // next
        if count > 0 && offset + count < total_usize {
            links.push(BundleLink {
                relation: "next".to_string(),
                url: build_history_link(base_url, resource_type, id, offset + count, count),
            });
        }

        // last
        if count > 0 && total_usize > 0 {
            let last_offset = ((total_usize - 1) / count) * count;
            links.push(BundleLink {
                relation: "last".to_string(),
                url: build_history_link(base_url, resource_type, id, last_offset, count),
            });
        }
    }

    Bundle::history(bundle_entries, links)
}

/// Build a history endpoint URL with pagination
fn build_history_link(
    base_url: &str,
    resource_type: &str,
    id: Option<&str>,
    offset: usize,
    count: usize,
) -> String {
    let base_url = base_url.trim_end_matches('/');
    let path = match id {
        Some(id) => format!("{base_url}/{resource_type}/{id}/_history"),
        None => format!("{base_url}/{resource_type}/_history"),
    };

    format!("{path}?_count={count}&__offset={offset}")
}

/// Build a system-level history endpoint URL with pagination
fn build_system_history_link(base_url: &str, offset: usize, count: usize) -> String {
    let base_url = base_url.trim_end_matches('/');
    format!("{base_url}/_history?_count={count}&__offset={offset}")
}

/// Build a system-level history bundle from history entries
///
/// This creates a Bundle with type=history containing entries from all resource types
/// in the system, ordered by transaction ID (most recent first).
pub fn bundle_from_system_history(
    entries: Vec<HistoryBundleEntry>,
    base_url: &str,
    offset: usize,
    count: usize,
    total: Option<u32>,
) -> Bundle {
    let bundle_entries: Vec<BundleEntry> = entries
        .into_iter()
        .map(|entry| {
            let full_url = format!("{}/{}/{}", base_url, entry.resource_type, entry.id);

            let request_url = match entry.method {
                HistoryBundleMethod::Create => entry.resource_type.clone(),
                HistoryBundleMethod::Update | HistoryBundleMethod::Delete => {
                    format!("{}/{}", entry.resource_type, entry.id)
                }
            };

            // For DELETE entries, don't include the resource
            let resource = if entry.method == HistoryBundleMethod::Delete {
                None
            } else {
                Some(entry.resource)
            };

            BundleEntry {
                full_url: Some(full_url),
                resource,
                search: None, // History entries don't have search mode
                request: Some(BundleEntryRequest {
                    method: entry.method.http_method().to_string(),
                    url: request_url,
                }),
                response: Some(BundleEntryResponse {
                    status: entry.method.http_status().to_string(),
                    etag: Some(format!("W/\"{}\"", entry.version_id)),
                    last_modified: Some(entry.last_modified),
                }),
            }
        })
        .collect();

    // Build links
    let mut links = Vec::new();

    // Build self link URL
    let self_url = build_system_history_link(base_url, offset, count);
    links.push(BundleLink {
        relation: "self".to_string(),
        url: self_url,
    });

    // Pagination links
    if let Some(total_count) = total {
        let total_usize = total_count as usize;

        // first
        links.push(BundleLink {
            relation: "first".to_string(),
            url: build_system_history_link(base_url, 0, count),
        });

        // prev
        if offset > 0 {
            let prev_offset = offset.saturating_sub(count);
            links.push(BundleLink {
                relation: "previous".to_string(),
                url: build_system_history_link(base_url, prev_offset, count),
            });
        }

        // next
        if count > 0 && offset + count < total_usize {
            links.push(BundleLink {
                relation: "next".to_string(),
                url: build_system_history_link(base_url, offset + count, count),
            });
        }

        // last
        if count > 0 && total_usize > 0 {
            let last_offset = ((total_usize - 1) / count) * count;
            links.push(BundleLink {
                relation: "last".to_string(),
                url: build_system_history_link(base_url, last_offset, count),
            });
        }
    }

    Bundle::history(bundle_entries, links)
}

#[cfg(test)]
mod bundle_generation_tests {
    use super::*;

    fn make_pat(id: &str) -> JsonValue {
        serde_json::json!({"resourceType":"Patient","id": id})
    }

    #[test]
    fn middle_page_links_and_entries() {
        let total = 25usize;
        let count = 10usize;
        let offset = 10usize;
        let resources = vec![make_pat("11"), make_pat("12")];
        let b = bundle_from_search(
            total,
            resources,
            "http://example.org",
            "Patient",
            offset,
            count,
            Some("name=John"),
        );
        assert_eq!(b.resource_type, "Bundle");
        assert_eq!(b.bundle_type, "searchset");
        assert_eq!(b.total, Some(total as u64));
        assert_eq!(b.entry.len(), 2);
        // links
        let rels: std::collections::HashMap<_, _> = b
            .link
            .iter()
            .map(|l| (l.relation.clone(), l.url.clone()))
            .collect();
        assert!(rels.get("self").unwrap().contains("_offset=10"));
        assert!(rels.get("previous").unwrap().contains("_offset=0"));
        assert!(rels.get("next").unwrap().contains("_offset=20"));
        assert!(rels.get("last").unwrap().contains("_offset=20"));
        assert!(rels.get("first").unwrap().contains("_offset=0"));
        // entry fullUrl
        let fu = &b.entry[0].full_url.as_ref().unwrap();
        assert!(fu.ends_with("/Patient/11"));
    }

    #[test]
    fn first_page_has_no_prev() {
        let b = bundle_from_search(
            25,
            vec![make_pat("1")],
            "http://example.org",
            "Patient",
            0,
            10,
            None,
        );
        let rels: std::collections::HashMap<_, _> = b
            .link
            .iter()
            .map(|l| (l.relation.clone(), l.url.clone()))
            .collect();
        assert!(!rels.contains_key("previous"));
        assert!(rels.contains_key("next"));
    }

    #[test]
    fn last_page_has_no_next() {
        let b = bundle_from_search(
            25,
            vec![make_pat("21")],
            "http://example.org",
            "Patient",
            20,
            10,
            None,
        );
        let rels: std::collections::HashMap<_, _> = b
            .link
            .iter()
            .map(|l| (l.relation.clone(), l.url.clone()))
            .collect();
        assert!(!rels.contains_key("next"));
        assert!(rels.contains_key("previous"));
        assert!(rels.get("last").unwrap().contains("_offset=20"));
    }

    #[test]
    fn empty_results_still_have_links() {
        let b = bundle_from_search(0, Vec::new(), "http://example.org", "Patient", 0, 10, None);
        assert_eq!(b.entry.len(), 0);
        let rels: std::collections::HashMap<_, _> = b
            .link
            .iter()
            .map(|l| (l.relation.clone(), l.url.clone()))
            .collect();
        assert!(rels.contains_key("self"));
        assert!(rels.contains_key("first"));
        assert!(rels.contains_key("last"));
        assert!(!rels.contains_key("next"));
        assert!(!rels.contains_key("previous"));
    }

    #[test]
    fn query_suffix_is_preserved_in_links() {
        let b = bundle_from_search(
            25,
            vec![make_pat("1")],
            "http://example.org",
            "Patient",
            0,
            10,
            Some("name=John&identifier=urn%3Asys%7C123"),
        );
        let rels: std::collections::HashMap<_, _> = b
            .link
            .iter()
            .map(|l| (l.relation.clone(), l.url.clone()))
            .collect();
        for key in ["self", "first", "last", "next"].iter() {
            if let Some(u) = rels.get(*key) {
                assert!(u.contains("name=John"), "{key} missing name param: {u}");
                assert!(
                    u.contains("identifier=urn%3Asys%7C123"),
                    "{key} missing identifier param: {u}"
                );
            }
        }
    }

    #[test]
    fn single_page_has_no_next_or_prev_and_last_offset_zero() {
        let total = 7usize;
        let count = 10usize; // single page since total <= count
        let b = bundle_from_search(
            total,
            vec![make_pat("1"), make_pat("2")],
            "http://example.org",
            "Patient",
            0,
            count,
            Some("name=Jane"),
        );
        let rels: std::collections::HashMap<_, _> = b
            .link
            .iter()
            .map(|l| (l.relation.clone(), l.url.clone()))
            .collect();
        assert!(!rels.contains_key("previous"));
        assert!(!rels.contains_key("next"));
        assert!(rels.get("last").unwrap().contains("_offset=0"));
        assert!(rels.get("self").unwrap().contains("name=Jane"));
    }
}

// -------------------------
// CapabilityStatement Types (Task 3.3.1)
// -------------------------
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CapabilityStatement {
    #[serde(rename = "resourceType")]
    pub resource_type: &'static str, // always "CapabilityStatement"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>, // e.g., "active"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<String>, // ISO 8601 date-time string
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>, // e.g., "instance" or "capability"
    #[serde(rename = "fhirVersion")]
    pub fhir_version: String, // e.g., "4.3.0" (R4B)
    pub format: Vec<String>, // e.g., ["application/fhir+json"]
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub rest: Vec<CapabilityStatementRest>,
}

impl CapabilityStatement {
    pub fn minimal_json_server() -> Self {
        Self {
            resource_type: "CapabilityStatement",
            status: Some("active".to_string()),
            date: None,
            kind: Some("instance".to_string()),
            fhir_version: "4.3.0".to_string(),
            format: vec!["application/fhir+json".to_string()],
            rest: vec![CapabilityStatementRest::server_minimal()],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CapabilityStatementRest {
    pub mode: String, // "server" or "client"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource: Option<Vec<CapabilityStatementRestResource>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation: Option<Vec<CapabilityStatementRestOperation>>,
}

impl CapabilityStatementRest {
    pub fn server_minimal() -> Self {
        Self {
            mode: "server".to_string(),
            resource: Some(vec![]),
            operation: None,
        }
    }
}

/// Operation definition in CapabilityStatement
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CapabilityStatementRestOperation {
    /// Name of the operation (e.g., "$graphql", "$validate")
    pub name: String,
    /// Canonical URL or URN for the operation definition
    pub definition: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CapabilityStatementRestResource {
    #[serde(rename = "type")]
    pub type_: String, // Resource type name, e.g., "Patient"
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub interaction: Vec<ResourceInteraction>,
    #[serde(rename = "searchParam", skip_serializing_if = "Vec::is_empty", default)]
    pub search_param: Vec<SearchParam>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    #[serde(
        rename = "supportedProfile",
        skip_serializing_if = "Vec::is_empty",
        default
    )]
    pub supported_profile: Vec<String>,
}

impl CapabilityStatementRestResource {
    pub fn new(type_name: impl Into<String>) -> Self {
        Self {
            type_: type_name.into(),
            interaction: Vec::new(),
            search_param: Vec::new(),
            profile: None,
            supported_profile: Vec::new(),
        }
    }

    pub fn with_interactions(mut self, codes: &[&str]) -> Self {
        self.interaction = codes
            .iter()
            .map(|c| ResourceInteraction {
                code: c.to_string(),
            })
            .collect();
        self
    }

    pub fn with_search_params(mut self, params: Vec<SearchParam>) -> Self {
        self.search_param = params;
        self
    }

    pub fn with_profile(mut self, profile: Option<String>) -> Self {
        self.profile = profile;
        self
    }

    pub fn with_supported_profiles(mut self, profiles: Vec<String>) -> Self {
        self.supported_profile = profiles;
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResourceInteraction {
    pub code: String, // e.g., "read", "search-type", "create", etc.
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SearchParam {
    pub name: String,
    #[serde(rename = "type")]
    pub type_: String, // FHIR search parameter type, e.g., "token", "string"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub documentation: Option<String>,
}

#[cfg(test)]
mod capability_statement_tests {
    use super::*;

    #[test]
    fn serialize_minimal_capability_statement() {
        let mut cs = CapabilityStatement::minimal_json_server();
        // Add one resource capability (Patient with read + search)
        let res = CapabilityStatementRestResource::new("Patient")
            .with_interactions(&["read", "search-type"])
            .with_search_params(vec![SearchParam {
                name: "_id".to_string(),
                type_: "token".to_string(),
                documentation: None,
            }]);
        if let Some(ref mut resources) = cs.rest.get_mut(0).unwrap().resource {
            resources.push(res);
        }

        let j = serde_json::to_value(&cs).unwrap();
        assert_eq!(j["resourceType"], "CapabilityStatement");
        assert_eq!(j["fhirVersion"], "4.3.0");
        // format contains application/fhir+json
        assert!(j["format"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "application/fhir+json"));
        // rest[0].mode == server
        assert_eq!(j["rest"][0]["mode"], "server");
        // resource type and interactions
        assert_eq!(j["rest"][0]["resource"][0]["type"], "Patient");
        let interactions = j["rest"][0]["resource"][0]["interaction"]
            .as_array()
            .unwrap();
        assert!(interactions.iter().any(|v| v["code"] == "read"));
        assert!(interactions.iter().any(|v| v["code"] == "search-type"));
        // searchParam exists with name _id
        assert_eq!(j["rest"][0]["resource"][0]["searchParam"][0]["name"], "_id");
    }
}

// -------------------------
// CapabilityStatement Builder (Task 3.3.2)
// -------------------------
#[derive(Debug, Default, Clone)]
pub struct CapabilityStatementBuilder {
    status: Option<String>,
    kind: Option<String>,
    date: Option<String>,
    fhir_version: String,
    formats: Vec<String>,
    resources: Vec<CapabilityStatementRestResource>,
    operations: Vec<CapabilityStatementRestOperation>,
}

impl CapabilityStatementBuilder {
    pub fn new_json_r4b() -> Self {
        Self {
            status: Some("active".to_string()),
            kind: Some("instance".to_string()),
            date: None,
            fhir_version: "4.3.0".to_string(),
            formats: vec!["application/fhir+json".to_string()],
            resources: Vec::new(),
            operations: Vec::new(),
        }
    }

    pub fn status(mut self, status: impl Into<String>) -> Self {
        self.status = Some(status.into());
        self
    }
    pub fn kind(mut self, kind: impl Into<String>) -> Self {
        self.kind = Some(kind.into());
        self
    }
    pub fn date(mut self, date: impl Into<String>) -> Self {
        self.date = Some(date.into());
        self
    }
    pub fn fhir_version(mut self, version: impl Into<String>) -> Self {
        self.fhir_version = version.into();
        self
    }
    pub fn add_format(mut self, format: impl Into<String>) -> Self {
        self.formats.push(format.into());
        self
    }

    pub fn add_resource(
        mut self,
        type_name: impl Into<String>,
        interactions: &[&str],
        search_params: Vec<SearchParam>,
    ) -> Self {
        let r = CapabilityStatementRestResource::new(type_name)
            .with_interactions(interactions)
            .with_search_params(search_params);
        self.resources.push(r);
        self
    }

    pub fn add_resource_struct(mut self, resource: CapabilityStatementRestResource) -> Self {
        self.resources.push(resource);
        self
    }

    /// Add an operation to the capability statement
    pub fn add_operation(mut self, name: impl Into<String>, definition: impl Into<String>) -> Self {
        self.operations.push(CapabilityStatementRestOperation {
            name: name.into(),
            definition: definition.into(),
        });
        self
    }

    pub fn build(self) -> CapabilityStatement {
        let operations = if self.operations.is_empty() {
            None
        } else {
            Some(self.operations)
        };

        let mut cs = CapabilityStatement {
            resource_type: "CapabilityStatement",
            status: self.status,
            date: self.date,
            kind: self.kind,
            fhir_version: self.fhir_version,
            format: self.formats,
            rest: vec![CapabilityStatementRest {
                mode: "server".to_string(),
                resource: Some(self.resources),
                operation: operations,
            }],
        };
        // Ensure formats has at least application/fhir+json
        if cs.format.is_empty() {
            cs.format.push("application/fhir+json".to_string());
        }
        cs
    }
}

#[cfg(test)]
mod capability_statement_builder_tests {
    use super::*;

    #[test]
    fn build_capability_statement_with_resources() {
        let builder = CapabilityStatementBuilder::new_json_r4b()
            .status("active")
            .kind("instance")
            .add_resource(
                "Patient",
                &["read", "search-type"],
                vec![
                    SearchParam {
                        name: "_id".to_string(),
                        type_: "token".to_string(),
                        documentation: Some("FHIR logical id".to_string()),
                    },
                    SearchParam {
                        name: "name".to_string(),
                        type_: "string".to_string(),
                        documentation: None,
                    },
                ],
            )
            .add_resource(
                "Observation",
                &["read"],
                vec![SearchParam {
                    name: "code".to_string(),
                    type_: "token".to_string(),
                    documentation: None,
                }],
            );

        let cs = builder.build();
        assert_eq!(cs.resource_type, "CapabilityStatement");
        assert_eq!(cs.fhir_version, "4.3.0");
        assert!(cs.format.iter().any(|f| f == "application/fhir+json"));
        assert_eq!(cs.rest.len(), 1);
        let rest = &cs.rest[0];
        assert_eq!(rest.mode, "server");
        let resources = rest.resource.as_ref().unwrap();
        assert_eq!(resources.len(), 2);
        assert_eq!(resources[0].type_, "Patient");
        assert!(resources[0].interaction.iter().any(|i| i.code == "read"));
        assert!(resources[0]
            .interaction
            .iter()
            .any(|i| i.code == "search-type"));
        assert!(resources[0].search_param.iter().any(|p| p.name == "_id"));
        assert_eq!(resources[1].type_, "Observation");
        assert!(resources[1].interaction.iter().any(|i| i.code == "read"));
    }
}

// -------------------------
// Search parameter documentation helpers (Task 3.3.3)
// -------------------------
/// Common search parameters applicable to most resources with documentation text.
pub fn common_search_params() -> Vec<SearchParam> {
    vec![
        SearchParam {
            name: "_id".to_string(),
            type_: "token".to_string(),
            documentation: Some("Logical id of the resource (Resource.id)".to_string()),
        },
        SearchParam {
            name: "_lastUpdated".to_string(),
            type_: "date".to_string(),
            documentation: Some("When the resource version last changed".to_string()),
        },
    ]
}

/// Check if a resource type name is a standard FHIR resource type.
///
/// Non-standard types (AccessPolicy, App, Client, etc.) should be excluded
/// from the CapabilityStatement rest.resource to pass FHIR validation.
pub fn is_standard_fhir_resource(name: &str) -> bool {
    // Standard FHIR R4/R4B resource types (from the ResourceType value set)
    matches!(
        name,
        "Account"
            | "ActivityDefinition"
            | "AdministrableProductDefinition"
            | "AdverseEvent"
            | "AllergyIntolerance"
            | "Appointment"
            | "AppointmentResponse"
            | "AuditEvent"
            | "Basic"
            | "Binary"
            | "BiologicallyDerivedProduct"
            | "BodyStructure"
            | "Bundle"
            | "CapabilityStatement"
            | "CarePlan"
            | "CareTeam"
            | "CatalogEntry"
            | "ChargeItem"
            | "ChargeItemDefinition"
            | "Citation"
            | "Claim"
            | "ClaimResponse"
            | "ClinicalImpression"
            | "ClinicalUseDefinition"
            | "CodeSystem"
            | "Communication"
            | "CommunicationRequest"
            | "CompartmentDefinition"
            | "Composition"
            | "ConceptMap"
            | "Condition"
            | "Consent"
            | "Contract"
            | "Coverage"
            | "CoverageEligibilityRequest"
            | "CoverageEligibilityResponse"
            | "DetectedIssue"
            | "Device"
            | "DeviceDefinition"
            | "DeviceMetric"
            | "DeviceRequest"
            | "DeviceUseStatement"
            | "DiagnosticReport"
            | "DocumentManifest"
            | "DocumentReference"
            | "Encounter"
            | "Endpoint"
            | "EnrollmentRequest"
            | "EnrollmentResponse"
            | "EpisodeOfCare"
            | "EventDefinition"
            | "Evidence"
            | "EvidenceReport"
            | "EvidenceVariable"
            | "ExampleScenario"
            | "ExplanationOfBenefit"
            | "FamilyMemberHistory"
            | "Flag"
            | "Goal"
            | "GraphDefinition"
            | "Group"
            | "GuidanceResponse"
            | "HealthcareService"
            | "ImagingStudy"
            | "Immunization"
            | "ImmunizationEvaluation"
            | "ImmunizationRecommendation"
            | "ImplementationGuide"
            | "Ingredient"
            | "InsurancePlan"
            | "Invoice"
            | "Library"
            | "Linkage"
            | "List"
            | "Location"
            | "ManufacturedItemDefinition"
            | "Measure"
            | "MeasureReport"
            | "Media"
            | "Medication"
            | "MedicationAdministration"
            | "MedicationDispense"
            | "MedicationKnowledge"
            | "MedicationRequest"
            | "MedicationStatement"
            | "MedicinalProductDefinition"
            | "MessageDefinition"
            | "MessageHeader"
            | "MolecularSequence"
            | "NamingSystem"
            | "NutritionOrder"
            | "NutritionProduct"
            | "Observation"
            | "ObservationDefinition"
            | "OperationDefinition"
            | "OperationOutcome"
            | "Organization"
            | "OrganizationAffiliation"
            | "PackagedProductDefinition"
            | "Parameters"
            | "Patient"
            | "PaymentNotice"
            | "PaymentReconciliation"
            | "Person"
            | "PlanDefinition"
            | "Practitioner"
            | "PractitionerRole"
            | "Procedure"
            | "Provenance"
            | "Questionnaire"
            | "QuestionnaireResponse"
            | "RegulatedAuthorization"
            | "RelatedPerson"
            | "RequestGroup"
            | "ResearchDefinition"
            | "ResearchElementDefinition"
            | "ResearchStudy"
            | "ResearchSubject"
            | "RiskAssessment"
            | "Schedule"
            | "SearchParameter"
            | "ServiceRequest"
            | "Slot"
            | "Specimen"
            | "SpecimenDefinition"
            | "StructureDefinition"
            | "StructureMap"
            | "Subscription"
            | "SubscriptionStatus"
            | "SubscriptionTopic"
            | "Substance"
            | "SubstanceDefinition"
            | "SupplyDelivery"
            | "SupplyRequest"
            | "Task"
            | "TerminologyCapabilities"
            | "TestReport"
            | "TestScript"
            | "ValueSet"
            | "VerificationResult"
            | "VisionPrescription"
    )
}

#[cfg(test)]
mod search_param_docs_tests {
    use super::*;

    #[test]
    fn common_params_include_documentation() {
        let params = common_search_params();
        assert!(params.iter().any(|p| p.name == "_id"
            && p.documentation.as_deref() == Some("Logical id of the resource (Resource.id)")));
        assert!(params
            .iter()
            .any(|p| p.name == "_lastUpdated" && p.type_ == "date"));
    }

    #[test]
    fn builder_can_use_common_params() {
        let cs = CapabilityStatementBuilder::new_json_r4b()
            .add_resource("Patient", &["read", "search-type"], common_search_params())
            .build();
        let sp = &cs.rest[0].resource.as_ref().unwrap()[0].search_param;
        // Ensure documentation is present for _id
        let id = sp.iter().find(|p| p.name == "_id").unwrap();
        assert!(id.documentation.is_some());
    }
}
