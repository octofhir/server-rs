//! Proxy handler for forwarding requests to external services.

use axum::{
    body::Body,
    http::{HeaderMap, HeaderName, HeaderValue, Request},
    response::Response,
};
use tracing::{debug, info, instrument, warn};

use super::error::GatewayError;
use super::types::CustomOperation;
use crate::server::AppState;

/// Handles proxy operations by forwarding requests to external services.
///
/// This handler:
/// 1. Extracts proxy configuration from the operation
/// 2. Forwards the request to the target URL
/// 3. Filters and transforms headers
/// 4. Applies timeouts
/// 5. Maps errors to FHIR OperationOutcome
#[instrument(skip(state, operation, request))]
pub async fn handle_proxy(
    state: &AppState,
    operation: &CustomOperation,
    request: Request<Body>,
) -> Result<Response, GatewayError> {
    let proxy_config = operation.proxy.as_ref().ok_or_else(|| {
        GatewayError::InvalidConfig("Proxy operation missing proxy configuration".to_string())
    })?;

    let target_url = &proxy_config.url;
    let timeout = proxy_config.timeout.unwrap_or(30) as u64;

    info!(
        target_url = %target_url,
        timeout_secs = timeout,
        "Proxying request"
    );

    // Extract method and headers from the incoming request
    let method = request.method().clone();
    let incoming_headers = request.headers();

    // Filter and transform headers
    let mut headers = HeaderMap::new();

    // Copy headers, filtering out hop-by-hop headers
    for (name, value) in incoming_headers.iter() {
        // Skip hop-by-hop headers as defined in RFC 2616
        if is_hop_by_hop_header(name.as_str()) {
            debug!(header = %name, "Skipping hop-by-hop header");
            continue;
        }

        // Skip authentication headers unless explicitly allowed
        if !proxy_config.forward_auth.unwrap_or(false) && is_auth_header(name.as_str()) {
            debug!(header = %name, "Skipping authentication header");
            continue;
        }

        headers.insert(name.clone(), value.clone());
    }

    // Add any custom headers from config
    if let Some(custom_headers) = &proxy_config.headers {
        for (name, value) in custom_headers {
            if let (Ok(header_name), Ok(header_value)) = (
                HeaderName::try_from(name.as_str()),
                HeaderValue::try_from(value.as_str()),
            ) {
                headers.insert(header_name, header_value);
                debug!(header = %name, value = %value, "Added custom header");
            } else {
                warn!(header = %name, "Invalid header name or value");
            }
        }
    }

    // Read request body
    let body_bytes = axum::body::to_bytes(request.into_body(), 10_000_000)
        .await
        .map_err(|e| GatewayError::ProxyError(format!("Failed to read request body: {}", e)))?;

    // Build the proxy request
    let proxy_request = state
        .gateway_router
        .http_client()
        .request(method.clone(), target_url)
        .headers(headers)
        .body(body_bytes.to_vec())
        .timeout(std::time::Duration::from_secs(timeout))
        .build()
        .map_err(|e| GatewayError::ProxyError(format!("Failed to build proxy request: {}", e)))?;

    // Execute the proxy request
    let proxy_response = state
        .gateway_router
        .http_client()
        .execute(proxy_request)
        .await
        .map_err(|e| {
            if e.is_timeout() {
                GatewayError::ProxyError(format!(
                    "Proxy request timed out after {} seconds",
                    timeout
                ))
            } else if e.is_connect() {
                GatewayError::ProxyError(format!("Failed to connect to target: {}", e))
            } else {
                GatewayError::ProxyError(format!("Proxy request failed: {}", e))
            }
        })?;

    let status = proxy_response.status();
    info!(status = %status, "Proxy request completed");

    // Build response
    let mut response_builder = Response::builder().status(status);

    // Copy response headers (filtering hop-by-hop)
    for (name, value) in proxy_response.headers().iter() {
        if !is_hop_by_hop_header(name.as_str()) {
            response_builder = response_builder.header(name, value);
        }
    }

    // Read response body
    let response_body = proxy_response
        .bytes()
        .await
        .map_err(|e| GatewayError::ProxyError(format!("Failed to read response body: {}", e)))?;

    response_builder
        .body(Body::from(response_body.to_vec()))
        .map_err(|e| GatewayError::InternalError(format!("Failed to build response: {}", e)))
}

/// Checks if a header is a hop-by-hop header that should not be forwarded.
///
/// Hop-by-hop headers are defined in RFC 2616 Section 13.5.1.
fn is_hop_by_hop_header(name: &str) -> bool {
    matches!(
        name.to_lowercase().as_str(),
        "connection"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailers"
            | "transfer-encoding"
            | "upgrade"
            | "host" // Host should be set to target, not forwarded
    )
}

/// Checks if a header is an authentication header.
fn is_auth_header(name: &str) -> bool {
    matches!(
        name.to_lowercase().as_str(),
        "authorization" | "cookie" | "set-cookie"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_hop_by_hop_header() {
        assert!(is_hop_by_hop_header("Connection"));
        assert!(is_hop_by_hop_header("Transfer-Encoding"));
        assert!(is_hop_by_hop_header("host"));
        assert!(!is_hop_by_hop_header("Content-Type"));
        assert!(!is_hop_by_hop_header("Authorization"));
    }

    #[test]
    fn test_is_auth_header() {
        assert!(is_auth_header("Authorization"));
        assert!(is_auth_header("Cookie"));
        assert!(is_auth_header("Set-Cookie"));
        assert!(!is_auth_header("Content-Type"));
    }
}
