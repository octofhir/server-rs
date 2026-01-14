//! HTTP API implementation
//!
//! Provides `http.fetch(url, options)` for making HTTP requests from automations.
//! Uses reqwest under the hood with tokio::runtime::Handle::block_on for sync interface.

use crate::apis::{get_arg_as_json, get_arg_as_string, json_to_js_value, make_exception};
use crate::bindings::*;
use crate::error::JscResult;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::ffi::CString;
use std::ptr;
use std::str::FromStr;
use std::time::Duration;
use tracing::{debug, warn};

/// HTTP fetch options matching JavaScript fetch API
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
struct FetchOptions {
    method: Option<String>,
    headers: Option<HashMap<String, String>>,
    body: Option<serde_json::Value>,
    timeout: Option<u64>, // milliseconds
}

/// HTTP response structure
#[derive(Debug, Clone, Serialize)]
struct FetchResponse {
    ok: bool,
    status: u16,
    status_text: String,
    headers: HashMap<String, String>,
    body: serde_json::Value,
    url: String,
}

/// Register the http API on the global object
pub fn register_http_api(ctx: JSContextRef) -> JscResult<()> {
    unsafe {
        // Create http object
        let http_obj = JSObjectMake(ctx, ptr::null_mut(), ptr::null_mut());

        // Register http.fetch
        let fetch_name = CString::new("fetch").unwrap();
        let fetch_name_ref = JSStringCreateWithUTF8CString(fetch_name.as_ptr());
        let fetch_func = JSObjectMakeFunctionWithCallback(ctx, fetch_name_ref, Some(js_http_fetch));

        let mut exception: JSValueRef = ptr::null_mut();
        JSObjectSetProperty(
            ctx,
            http_obj,
            fetch_name_ref,
            fetch_func as JSValueRef,
            K_JS_PROPERTY_ATTRIBUTE_NONE,
            &mut exception,
        );
        JSStringRelease(fetch_name_ref);

        // Set http on global object
        let http_name = CString::new("http").unwrap();
        let http_name_ref = JSStringCreateWithUTF8CString(http_name.as_ptr());
        let global = JSContextGetGlobalObject(ctx);

        JSObjectSetProperty(
            ctx,
            global,
            http_name_ref,
            http_obj as JSValueRef,
            K_JS_PROPERTY_ATTRIBUTE_NONE,
            &mut exception,
        );

        JSStringRelease(http_name_ref);
    }

    Ok(())
}

/// http.fetch(url, options) implementation
///
/// ```javascript
/// const response = http.fetch("https://api.example.com/data", {
///     method: "POST",
///     headers: { "Content-Type": "application/json" },
///     body: { key: "value" },
///     timeout: 5000
/// });
/// console.log(response.status, response.body);
/// ```
unsafe extern "C" fn js_http_fetch(
    ctx: JSContextRef,
    _function: JSObjectRef,
    _this_object: JSObjectRef,
    argument_count: usize,
    arguments: *const JSValueRef,
    exception: *mut JSValueRef,
) -> JSValueRef {
    // Get URL (required first argument)
    let url = match get_arg_as_string(ctx, arguments, 0, argument_count) {
        Some(u) => u,
        None => {
            *exception = make_exception(ctx, "http.fetch requires a URL as first argument");
            return JSValueMakeUndefined(ctx);
        }
    };

    // Get options (optional second argument)
    let options: FetchOptions = get_arg_as_json(ctx, arguments, 1, argument_count)
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();

    debug!(
        url = %url,
        method = ?options.method,
        "http.fetch called"
    );

    // Execute the request synchronously using block_on
    match execute_fetch(&url, options) {
        Ok(response) => {
            let response_json = serde_json::to_value(&response).unwrap_or(json!(null));
            json_to_js_value(ctx, &response_json)
        }
        Err(e) => {
            warn!(error = %e, url = %url, "http.fetch failed");
            *exception = make_exception(ctx, &e);
            JSValueMakeUndefined(ctx)
        }
    }
}

/// Execute HTTP fetch using reqwest (blocking)
fn execute_fetch(url: &str, options: FetchOptions) -> Result<FetchResponse, String> {
    // Try to get existing tokio runtime handle, or create blocking client
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_millis(options.timeout.unwrap_or(30000)))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    let method = options.method.as_deref().unwrap_or("GET").to_uppercase();

    let mut request = match method.as_str() {
        "GET" => client.get(url),
        "POST" => client.post(url),
        "PUT" => client.put(url),
        "DELETE" => client.delete(url),
        "PATCH" => client.patch(url),
        "HEAD" => client.head(url),
        _ => return Err(format!("Unsupported HTTP method: {}", method)),
    };

    // Add headers
    if let Some(headers) = options.headers {
        let mut header_map = HeaderMap::new();
        for (key, value) in headers {
            if let (Ok(name), Ok(val)) = (HeaderName::from_str(&key), HeaderValue::from_str(&value))
            {
                header_map.insert(name, val);
            }
        }
        request = request.headers(header_map);
    }

    // Add body for methods that support it
    if let Some(body) = options.body {
        match body {
            serde_json::Value::String(s) => {
                request = request.body(s);
            }
            _ => {
                // Serialize to JSON
                let json_str = serde_json::to_string(&body)
                    .map_err(|e| format!("Failed to serialize body: {}", e))?;
                request = request
                    .header("Content-Type", "application/json")
                    .body(json_str);
            }
        }
    }

    // Execute request
    let response = request
        .send()
        .map_err(|e| format!("HTTP request failed: {}", e))?;

    let status = response.status();
    let status_code = status.as_u16();
    let status_text = status.canonical_reason().unwrap_or("Unknown").to_string();
    let url = response.url().to_string();

    // Collect headers
    let mut headers = HashMap::new();
    for (name, value) in response.headers() {
        if let Ok(v) = value.to_str() {
            headers.insert(name.to_string(), v.to_string());
        }
    }

    // Parse body
    let body_text = response
        .text()
        .map_err(|e| format!("Failed to read response body: {}", e))?;

    // Try to parse as JSON, fallback to string
    let body: serde_json::Value =
        serde_json::from_str(&body_text).unwrap_or_else(|_| serde_json::Value::String(body_text));

    Ok(FetchResponse {
        ok: status_code >= 200 && status_code < 300,
        status: status_code,
        status_text,
        headers,
        body,
        url,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fetch_options_default() {
        let options: FetchOptions = serde_json::from_str("{}").unwrap();
        assert!(options.method.is_none());
        assert!(options.headers.is_none());
        assert!(options.body.is_none());
    }

    #[test]
    fn test_fetch_options_full() {
        let json = r#"{
            "method": "POST",
            "headers": {"Content-Type": "application/json"},
            "body": {"key": "value"},
            "timeout": 5000
        }"#;
        let options: FetchOptions = serde_json::from_str(json).unwrap();
        assert_eq!(options.method, Some("POST".to_string()));
        assert!(options.headers.is_some());
        assert!(options.body.is_some());
        assert_eq!(options.timeout, Some(5000));
    }
}
