//! App authentication for Gateway operations.

use axum::http::{HeaderMap, Request};
use axum::body::Body;

use crate::server::AppState;
use super::error::GatewayError;
use super::types::{App, CustomOperation};

/// Header name for App secret authentication.
pub const X_APP_SECRET_HEADER: &str = "x-app-secret";

/// Authenticate an App operation using the X-App-Secret header.
///
/// This function:
/// 1. Extracts the X-App-Secret header from the request
/// 2. Parses the App reference from the CustomOperation
/// 3. Loads the App resource from storage
/// 4. Verifies the provided secret against the stored hash
///
/// # Arguments
///
/// * `operation` - The CustomOperation being executed
/// * `headers` - Request headers containing X-App-Secret
/// * `state` - Application state for storage access
///
/// # Returns
///
/// `Ok(())` if authentication succeeds, `Err(GatewayError::Unauthorized)` if it fails.
///
/// # Example
///
/// ```ignore
/// authenticate_app_operation(&operation, request.headers(), &state).await?;
/// ```
pub async fn authenticate_app_operation(
    operation: &CustomOperation,
    headers: &HeaderMap,
    state: &AppState,
) -> Result<(), GatewayError> {
    // Extract X-App-Secret header
    let secret = headers
        .get(X_APP_SECRET_HEADER)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| {
            GatewayError::Unauthorized(
                "Missing X-App-Secret header for App operation".to_string()
            )
        })?;

    // Extract App ID from operation.app.reference
    let app_ref = operation.app.reference.as_ref().ok_or_else(|| {
        GatewayError::InternalError(format!(
            "CustomOperation {} has no app reference",
            operation.id.as_deref().unwrap_or("unknown")
        ))
    })?;

    // Parse App ID from reference (e.g., "App/123" -> "123")
    let app_id = app_ref
        .strip_prefix("App/")
        .ok_or_else(|| {
            GatewayError::InternalError(format!("Invalid app reference format: {}", app_ref))
        })?;

    // Load App resource from storage
    let app_json = state
        .storage
        .read("App", app_id)
        .await
        .map_err(|e| {
            GatewayError::InternalError(format!("Failed to load App {}: {}", app_id, e))
        })?
        .ok_or_else(|| {
            GatewayError::InternalError(format!("App {} not found", app_id))
        })?;

    let app: App = serde_json::from_value(app_json.resource).map_err(|e| {
        GatewayError::InternalError(format!("Failed to parse App {}: {}", app_id, e))
    })?;

    // Verify secret against stored hash
    let is_valid = octofhir_auth::verify_app_secret(secret, &app.secret).map_err(|e| {
        GatewayError::InternalError(format!("Failed to verify app secret: {}", e))
    })?;

    if !is_valid {
        return Err(GatewayError::Unauthorized(format!(
            "Invalid app secret for App {}",
            app_id
        )));
    }

    tracing::debug!(app_id = %app_id, "App authentication successful");
    Ok(())
}

/// Extract headers from a request for authentication.
///
/// This is a helper function to get headers from a request without consuming it.
pub fn extract_headers(request: &Request<Body>) -> &HeaderMap {
    request.headers()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_app_id_from_reference() {
        // Valid reference
        let app_ref = "App/test-app-123";
        let app_id = app_ref.strip_prefix("App/").unwrap();
        assert_eq!(app_id, "test-app-123");

        // Invalid reference (no prefix)
        let invalid_ref = "test-app-123";
        assert!(invalid_ref.strip_prefix("App/").is_none());

        // Invalid reference (wrong prefix)
        let invalid_ref = "Application/test-app-123";
        assert!(invalid_ref.strip_prefix("App/").is_none());
    }

    #[test]
    fn test_header_name_constant() {
        assert_eq!(X_APP_SECRET_HEADER, "x-app-secret");
    }
}
