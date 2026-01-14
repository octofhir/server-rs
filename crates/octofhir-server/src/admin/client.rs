//! OAuth client admin handlers.
//!
//! This module provides administrative handlers for OAuth client management,
//! including secret regeneration.

use axum::Json;
use axum::extract::{Path, State};
use axum::response::IntoResponse;
use octofhir_api::ApiError;
use octofhir_auth::middleware::AdminAuth;
use octofhir_auth::storage::ClientStorage;
use octofhir_auth_postgres::PostgresClientStorage;
use serde::Serialize;

use super::state::AdminState;

// =============================================================================
// Types
// =============================================================================

/// Response for secret regeneration.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RegenerateSecretResponse {
    /// The OAuth client_id.
    pub client_id: String,
    /// The newly generated plaintext secret.
    /// This is shown once and never stored.
    pub client_secret: String,
}

// =============================================================================
// Handlers
// =============================================================================

/// POST /clients/:id/regenerate-secret - Regenerate a client's secret.
///
/// Generates a new random secret for a confidential client.
/// The new secret is returned in plaintext and should be displayed to the user
/// exactly once. The old secret is invalidated immediately.
///
/// # Path Parameters
///
/// - `id`: The OAuth client_id (not the resource UUID)
///
/// # Errors
///
/// - 400 Bad Request: Client is not confidential (public clients have no secrets)
/// - 404 Not Found: Client does not exist
/// - 500 Internal Server Error: Storage or hashing failure
pub async fn regenerate_client_secret(
    State(state): State<AdminState>,
    Path(client_id): Path<String>,
    _admin: AdminAuth,
) -> Result<impl IntoResponse, ApiError> {
    let storage = PostgresClientStorage::new(&state.pool);

    let (client, plain_secret) = storage.regenerate_secret(&client_id).await.map_err(|e| {
        if e.to_string().contains("not found") {
            ApiError::not_found(format!("Client/{}", client_id))
        } else if e.to_string().contains("public clients") {
            ApiError::bad_request("Cannot regenerate secret for public clients")
        } else {
            ApiError::internal(e.to_string())
        }
    })?;

    Ok(Json(RegenerateSecretResponse {
        client_id: client.client_id,
        client_secret: plain_secret,
    }))
}
