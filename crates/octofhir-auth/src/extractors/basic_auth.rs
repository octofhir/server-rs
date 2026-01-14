//! Universal Basic Auth extractor for Client and App authentication.
//!
//! Provides an Axum extractor that authenticates entities (Client or App)
//! via HTTP Basic Auth.

use std::sync::Arc;

use axum::{
    Json,
    extract::{FromRef, FromRequestParts},
    http::{StatusCode, header::AUTHORIZATION, request::Parts},
    response::{IntoResponse, Response},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::Serialize;

use crate::storage::BasicAuthStorage;
use crate::types::BasicAuthEntity;

/// State container for Basic Auth.
///
/// This struct holds the storage backend needed for entity authentication.
#[derive(Clone)]
pub struct BasicAuthState {
    /// Storage backend for basic authentication.
    pub storage: Arc<dyn BasicAuthStorage>,
}

impl BasicAuthState {
    /// Creates a new BasicAuthState.
    pub fn new(storage: Arc<dyn BasicAuthStorage>) -> Self {
        Self { storage }
    }
}

/// Extracted Basic Auth info (Client or App).
///
/// This struct is returned when an entity successfully authenticates via Basic Auth.
#[derive(Debug, Clone)]
pub struct BasicAuth {
    /// The authenticated entity (Client or App)
    pub entity: BasicAuthEntity,
    /// Entity ID (client_id or app_id)
    pub entity_id: String,
}

impl BasicAuth {
    /// Check if the authenticated entity is a Client.
    pub fn is_client(&self) -> bool {
        self.entity.is_client()
    }

    /// Check if the authenticated entity is an App.
    pub fn is_app(&self) -> bool {
        self.entity.is_app()
    }
}

/// Error returned when Basic Auth fails.
#[derive(Debug, Clone, Serialize)]
pub struct BasicAuthError {
    pub error: String,
    pub error_description: String,
}

impl IntoResponse for BasicAuthError {
    fn into_response(self) -> Response {
        let status = StatusCode::UNAUTHORIZED;
        let body = Json(self);
        (status, body).into_response()
    }
}

impl<S> FromRequestParts<S> for BasicAuth
where
    S: Send + Sync,
    BasicAuthState: FromRef<S>,
{
    type Rejection = BasicAuthError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let basic_auth_state = BasicAuthState::from_ref(state);

        // Extract Authorization header
        let auth_header = parts
            .headers
            .get(AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| BasicAuthError {
                error: "invalid_request".to_string(),
                error_description: "Missing Authorization header".to_string(),
            })?;

        // Parse Basic Auth credentials
        let (entity_id, secret) = parse_basic_auth(auth_header).map_err(|e| BasicAuthError {
            error: "invalid_request".to_string(),
            error_description: e,
        })?;

        // Authenticate entity (Client or App)
        let entity = basic_auth_state
            .storage
            .authenticate(&entity_id, &secret)
            .await
            .map_err(|e| BasicAuthError {
                error: "server_error".to_string(),
                error_description: format!("Storage error: {}", e),
            })?
            .ok_or_else(|| BasicAuthError {
                error: "invalid_client".to_string(),
                error_description: "Invalid credentials".to_string(),
            })?;

        // Log successful authentication
        tracing::info!(
            entity_id = %entity_id,
            entity_type = if entity.is_client() { "client" } else { "app" },
            endpoint = %parts.uri.path(),
            method = %parts.method,
            "Entity authenticated via Basic Auth"
        );

        Ok(BasicAuth { entity, entity_id })
    }
}

/// Parse Basic Auth header.
///
/// Extracts credentials from "Basic <base64>" format.
fn parse_basic_auth(header: &str) -> Result<(String, String), String> {
    let credentials = header
        .strip_prefix("Basic ")
        .ok_or_else(|| "Authorization header must start with 'Basic '".to_string())?;

    let decoded = STANDARD
        .decode(credentials)
        .map_err(|_| "Invalid base64 encoding in Authorization header".to_string())?;

    let credentials_str = String::from_utf8(decoded)
        .map_err(|_| "Invalid UTF-8 in decoded credentials".to_string())?;

    let (entity_id, secret) = credentials_str
        .split_once(':')
        .ok_or_else(|| "Credentials must be in format 'id:secret'".to_string())?;

    Ok((entity_id.to_string(), secret.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_basic_auth_valid() {
        let credentials =
            base64::engine::general_purpose::STANDARD.encode(b"test-client:test-secret");
        let header = format!("Basic {}", credentials);

        let (id, secret) = parse_basic_auth(&header).unwrap();
        assert_eq!(id, "test-client");
        assert_eq!(secret, "test-secret");
    }

    #[test]
    fn test_parse_basic_auth_invalid_prefix() {
        let result = parse_basic_auth("Bearer token");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Basic"));
    }

    #[test]
    fn test_parse_basic_auth_invalid_base64() {
        let result = parse_basic_auth("Basic !!!invalid!!!");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_basic_auth_missing_colon() {
        let credentials = base64::engine::general_purpose::STANDARD.encode(b"no-colon-here");
        let header = format!("Basic {}", credentials);

        let result = parse_basic_auth(&header);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("id:secret"));
    }
}
