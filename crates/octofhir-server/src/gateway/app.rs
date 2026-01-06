//! App handler for forwarding requests to App backends with structured context.
//!
//! This handler differs from the proxy handler in that it:
//! - Builds a structured AppOperationRequest with auth context
//! - Extracts path/query parameters
//! - Parses request body as JSON
//! - Posts to App backend expecting AppOperationResponse
//!
//! This enables App backends to receive rich context about the operation,
//! authentication, and request parameters in a standard format.

use axum::{
    body::Body,
    http::{Request, Response, StatusCode},
};
use std::collections::HashMap;
use tracing::{debug, info, instrument};

use super::error::GatewayError;
use super::types::{AppOperationRequest, AppOperationResponse, AuthInfo, CustomOperation};
use crate::server::AppState;

/// Handles type="app" operations by forwarding structured requests to App backends.
///
/// This handler:
/// 1. Extracts App endpoint configuration from operation.proxy
/// 2. Builds AppOperationRequest with auth context, path/query params, body
/// 3. POSTs JSON to App backend
/// 4. Parses AppOperationResponse or returns raw response
/// 5. Converts to HTTP response
///
/// **Note**: Policy evaluation is already done in router.rs before this handler is called.
#[instrument(skip(state, operation, request))]
pub async fn handle_app(
    state: &AppState,
    operation: &CustomOperation,
    request: Request<Body>,
) -> Result<Response<Body>, GatewayError> {
    // Get proxy config (contains App endpoint URL from reconciliation)
    let proxy_config = operation.proxy.as_ref().ok_or_else(|| {
        GatewayError::InvalidConfig(
            "App operation missing proxy configuration (App.endpoint)".to_string(),
        )
    })?;

    let endpoint_url = &proxy_config.url;
    let timeout = proxy_config.timeout.unwrap_or(30) as u64;

    info!(
        operation_id = ?operation.id,
        endpoint_url = %endpoint_url,
        timeout_secs = timeout,
        "Handling App operation"
    );

    // Extract auth context from request extensions
    let auth_context = request
        .extensions()
        .get::<std::sync::Arc<octofhir_auth::middleware::AuthContext>>()
        .cloned();

    // Build structured AppOperationRequest
    let app_request = build_app_request(operation, request, auth_context.as_deref()).await?;

    debug!(
        operation_id = %app_request.operation_id,
        method = %app_request.method,
        path = %app_request.operation_path,
        "Built AppOperationRequest"
    );

    // POST to App backend
    let response = state
        .gateway_router
        .http_client()
        .post(endpoint_url)
        .timeout(std::time::Duration::from_secs(timeout))
        .header("Content-Type", "application/json")
        .json(&app_request)
        .send()
        .await
        .map_err(|e| {
            if e.is_timeout() {
                GatewayError::ProxyError(format!(
                    "App backend request timed out after {} seconds",
                    timeout
                ))
            } else if e.is_connect() {
                GatewayError::ProxyError(format!("Failed to connect to App backend: {}", e))
            } else {
                GatewayError::ProxyError(format!("App backend request failed: {}", e))
            }
        })?;

    let status = response.status();
    info!(status = %status, "App backend responded");

    // Read response body
    let body_text = response.text().await.map_err(|e| {
        GatewayError::ProxyError(format!("Failed to read App backend response: {}", e))
    })?;

    // Try to parse as AppOperationResponse
    if let Ok(app_response) = serde_json::from_str::<AppOperationResponse>(&body_text) {
        debug!("Parsed AppOperationResponse from App backend");
        return convert_app_response(app_response);
    }

    // Otherwise return raw response
    debug!("App backend returned non-structured response, passing through");
    Response::builder()
        .status(status)
        .header("Content-Type", "application/json")
        .body(Body::from(body_text))
        .map_err(|e| GatewayError::InternalError(format!("Failed to build response: {}", e)))
}

/// Builds AppOperationRequest from incoming request and operation metadata.
///
/// Extracts:
/// - operation_id, method, path from operation
/// - auth context from request extensions
/// - path parameters from URL matching operation.path pattern
/// - query parameters from URL
/// - body as parsed JSON
/// - selected headers
async fn build_app_request(
    operation: &CustomOperation,
    request: Request<Body>,
    auth_context: Option<&octofhir_auth::middleware::AuthContext>,
) -> Result<AppOperationRequest, GatewayError> {
    let (parts, body) = request.into_parts();

    // Extract path params by matching request path against operation.path pattern
    let request_path = parts.uri.path();
    let path_params = extract_path_params(request_path, &operation.path)?;

    // Extract query params
    let query_params = extract_query_params(&parts.uri);

    // Read body bytes (up to 10MB)
    let body_bytes = axum::body::to_bytes(body, 10_000_000)
        .await
        .map_err(|e| GatewayError::ProxyError(format!("Failed to read request body: {}", e)))?;

    // Parse body as JSON (if not empty)
    let body_json = if body_bytes.is_empty() {
        None
    } else {
        Some(serde_json::from_slice(&body_bytes).map_err(|e| {
            GatewayError::ProxyError(format!("Failed to parse request body as JSON: {}", e))
        })?)
    };

    // Optionally include raw body bytes (base64-encoded)
    let raw_body = if operation.include_raw_body.unwrap_or(false) && !body_bytes.is_empty() {
        use base64::{engine::general_purpose::STANDARD, Engine};
        Some(STANDARD.encode(&body_bytes))
    } else {
        None
    };

    // Extract headers to forward (all except hop-by-hop headers)
    let hop_by_hop = [
        "connection",
        "keep-alive",
        "proxy-authenticate",
        "proxy-authorization",
        "te",
        "trailer",
        "transfer-encoding",
        "upgrade",
        "host",
        "content-length",
        "content-type",
    ];
    let mut headers = HashMap::new();
    for (key, value) in &parts.headers {
        let key_str = key.as_str().to_lowercase();
        if !hop_by_hop.contains(&key_str.as_str()) {
            if let Ok(v) = value.to_str() {
                headers.insert(key_str, v.to_string());
            }
        }
    }

    Ok(AppOperationRequest {
        operation_id: operation
            .id
            .clone()
            .unwrap_or_else(|| "unknown".to_string()),
        operation_path: request_path.to_string(),
        method: parts.method.to_string(),
        auth: AuthInfo::from_auth_context(auth_context),
        path_params,
        query_params,
        body: body_json,
        raw_body,
        headers,
    })
}

/// Extracts path parameters by matching request path against operation path pattern.
///
/// Pattern syntax:
/// - `:param` - matches a single path segment
/// - Example: `/users/:id/posts/:postId` matches `/users/123/posts/456`
///   -> {id: "123", postId: "456"}
///
/// Returns empty map if no parameters defined or if paths don't align.
fn extract_path_params(
    request_path: &str,
    pattern: &str,
) -> Result<HashMap<String, String>, GatewayError> {
    let mut params = HashMap::new();

    // Split paths into segments, filtering out empty strings
    let path_segments: Vec<&str> = request_path
        .split('/')
        .filter(|s| !s.is_empty())
        .collect();
    let pattern_segments: Vec<&str> = pattern.split('/').filter(|s| !s.is_empty()).collect();

    // Match segments
    for (i, pattern_segment) in pattern_segments.iter().enumerate() {
        if let Some(param_name) = pattern_segment.strip_prefix(':') {
            // Extract parameter name (remove ':' prefix)
            // Get corresponding value from request path
            if let Some(value) = path_segments.get(i) {
                params.insert(param_name.to_string(), value.to_string());
            }
        }
    }

    Ok(params)
}

/// Extracts query parameters from URI.
///
/// Example: `?status=active&limit=10` -> {status: "active", limit: "10"}
fn extract_query_params(uri: &axum::http::Uri) -> HashMap<String, String> {
    uri.query()
        .map(|q| {
            url::form_urlencoded::parse(q.as_bytes())
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect()
        })
        .unwrap_or_default()
}

/// Converts AppOperationResponse to HTTP Response.
///
/// - Maps response.status to HTTP status code
/// - Adds custom headers from response.headers
/// - Sets Content-Type to application/json
/// - Serializes response.body to JSON
fn convert_app_response(
    app_response: AppOperationResponse,
) -> Result<Response<Body>, GatewayError> {
    let mut builder = Response::builder().status(
        StatusCode::from_u16(app_response.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
    );

    // Add custom headers
    if let Some(headers) = &app_response.headers {
        for (name, value) in headers {
            builder = builder.header(name.as_str(), value.as_str());
        }
    }

    // Always set Content-Type to application/json
    builder = builder.header("Content-Type", "application/json");

    // Serialize body
    let body_string = serde_json::to_string(&app_response.body)
        .map_err(|e| GatewayError::InternalError(format!("Failed to serialize response: {}", e)))?;

    builder
        .body(Body::from(body_string))
        .map_err(|e| GatewayError::InternalError(format!("Failed to build response: {}", e)))
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_path_params_simple() {
        let result = extract_path_params("/users/123", "/users/:id").unwrap();
        assert_eq!(result.get("id"), Some(&"123".to_string()));
    }

    #[test]
    fn test_extract_path_params_multiple() {
        let result = extract_path_params("/users/123/posts/456", "/users/:userId/posts/:postId")
            .unwrap();
        assert_eq!(result.get("userId"), Some(&"123".to_string()));
        assert_eq!(result.get("postId"), Some(&"456".to_string()));
    }

    #[test]
    fn test_extract_path_params_no_params() {
        let result = extract_path_params("/users/list", "/users/list").unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_extract_path_params_trailing_slash() {
        let result = extract_path_params("/users/123/", "/users/:id").unwrap();
        assert_eq!(result.get("id"), Some(&"123".to_string()));
    }

    #[test]
    fn test_extract_query_params() {
        let uri: axum::http::Uri = "http://example.com/api?status=active&limit=10"
            .parse()
            .unwrap();
        let result = extract_query_params(&uri);
        assert_eq!(result.get("status"), Some(&"active".to_string()));
        assert_eq!(result.get("limit"), Some(&"10".to_string()));
    }

    #[test]
    fn test_extract_query_params_empty() {
        let uri: axum::http::Uri = "http://example.com/api".parse().unwrap();
        let result = extract_query_params(&uri);
        assert!(result.is_empty());
    }

    #[test]
    fn test_extract_query_params_encoded() {
        let uri: axum::http::Uri = "http://example.com/api?name=John%20Doe"
            .parse()
            .unwrap();
        let result = extract_query_params(&uri);
        assert_eq!(result.get("name"), Some(&"John Doe".to_string()));
    }

    #[test]
    fn test_convert_app_response_success() {
        let app_response = AppOperationResponse {
            status: 200,
            body: serde_json::json!({"id": "123", "status": "ok"}),
            fhir_wrapper: None,
            headers: None,
        };

        let response = convert_app_response(app_response).unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[test]
    fn test_convert_app_response_with_headers() {
        let mut headers = HashMap::new();
        headers.insert("X-Custom-Header".to_string(), "value".to_string());

        let app_response = AppOperationResponse {
            status: 201,
            body: serde_json::json!({"id": "456"}),
            fhir_wrapper: None,
            headers: Some(headers),
        };

        let response = convert_app_response(app_response).unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        assert_eq!(
            response.headers().get("X-Custom-Header").unwrap(),
            "value"
        );
    }

    #[test]
    fn test_convert_app_response_invalid_status() {
        let app_response = AppOperationResponse {
            status: 999, // Valid but uncommon status code
            body: serde_json::json!({"error": "test"}),
            fhir_wrapper: None,
            headers: None,
        };

        let response = convert_app_response(app_response).unwrap();
        // Status 999 is actually valid in HTTP, so it should be preserved
        assert_eq!(response.status().as_u16(), 999);
    }
}
