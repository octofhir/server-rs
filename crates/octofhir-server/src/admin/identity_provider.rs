//! Identity provider admin handlers.
//!
//! This module provides CRUD handlers for managing identity providers.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use octofhir_api::ApiError;
use octofhir_auth_postgres::{IdentityProviderStorage, UserStorage};
use uuid::Uuid;

use octofhir_auth::federation::resources::IdentityProviderResource;
use octofhir_auth::middleware::AdminAuth;
use octofhir_auth::{Bundle, IdpSearchParams};

use super::state::AdminState;

// =============================================================================
// Handlers
// =============================================================================

/// GET /IdentityProvider - Search/list identity providers.
///
/// Query parameters:
/// - `active`: Filter by active status
/// - `name`: Filter by name (partial match)
/// - `_count`: Maximum results to return (default: 100)
/// - `_offset`: Number of results to skip (default: 0)
pub async fn search_identity_providers(
    State(state): State<AdminState>,
    Query(params): Query<IdpSearchParams>,
    _admin: AdminAuth,
) -> Result<impl IntoResponse, ApiError> {
    let storage = IdentityProviderStorage::new(&state.pool);

    let limit = params.count.unwrap_or(100);
    let offset = params.offset.unwrap_or(0);

    let rows = storage
        .list_all(limit, offset)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    // Filter and convert to IdentityProviderResource
    let resources: Vec<IdentityProviderResource> = rows
        .into_iter()
        .filter(|row| {
            // Filter by active status
            if let Some(active_filter) = params.active {
                let active = row
                    .resource
                    .get("active")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true);
                if active != active_filter {
                    return false;
                }
            }

            // Filter by name
            if let Some(ref name_filter) = params.name {
                let name = row
                    .resource
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if !name.to_lowercase().contains(&name_filter.to_lowercase()) {
                    return false;
                }
            }

            true
        })
        .filter_map(|row| serde_json::from_value(row.resource).ok())
        .collect();

    let total = resources.len();
    let bundle = Bundle::searchset(resources, Some(total));

    Ok(Json(bundle))
}

/// GET /IdentityProvider/:id - Read a single identity provider.
pub async fn read_identity_provider(
    State(state): State<AdminState>,
    Path(id): Path<String>,
    _admin: AdminAuth,
) -> Result<impl IntoResponse, ApiError> {
    let storage = IdentityProviderStorage::new(&state.pool);

    let uuid = Uuid::parse_str(&id).map_err(|_| ApiError::bad_request("Invalid UUID format"))?;

    let row = storage
        .find_by_id(uuid)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found(format!("IdentityProvider/{}", id)))?;

    let resource: IdentityProviderResource = serde_json::from_value(row.resource)
        .map_err(|e| ApiError::internal(format!("Failed to parse resource: {}", e)))?;

    Ok(Json(resource))
}

/// POST /IdentityProvider - Create a new identity provider.
pub async fn create_identity_provider(
    State(state): State<AdminState>,
    _admin: AdminAuth,
    Json(mut provider): Json<IdentityProviderResource>,
) -> Result<impl IntoResponse, ApiError> {
    // Validate the resource
    provider
        .validate()
        .map_err(|e| ApiError::bad_request(e.to_string()))?;

    // Ensure no ID is provided (will be generated)
    if provider.id.is_some() {
        return Err(ApiError::bad_request(
            "ID must not be provided for create operation",
        ));
    }

    let storage = IdentityProviderStorage::new(&state.pool);

    // Check for duplicate issuer
    if storage
        .find_by_issuer(&provider.issuer)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .is_some()
    {
        return Err(ApiError::conflict(
            "Identity provider with this issuer already exists",
        ));
    }

    // Check for duplicate name
    if storage
        .find_by_name(&provider.name)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .is_some()
    {
        return Err(ApiError::conflict(
            "Identity provider with this name already exists",
        ));
    }

    // Generate ID and set it
    let id = Uuid::new_v4();
    provider.id = Some(id.to_string());

    // Convert to JSON for storage
    let resource = serde_json::to_value(&provider)
        .map_err(|e| ApiError::internal(format!("Failed to serialize resource: {}", e)))?;

    // Create in storage
    let _row = storage
        .create(id, resource)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    // Register with IdP service if available
    if let Some(ref service) = state.idp_auth_service
        && let Ok(config) = provider.to_config()
    {
        service.register_provider(config).await;
    }

    tracing::info!(
        id = %id,
        name = %provider.name,
        "Created identity provider"
    );

    Ok((StatusCode::CREATED, Json(provider)))
}

/// PUT /IdentityProvider/:id - Update an existing identity provider.
pub async fn update_identity_provider(
    State(state): State<AdminState>,
    Path(id): Path<String>,
    _admin: AdminAuth,
    Json(mut provider): Json<IdentityProviderResource>,
) -> Result<impl IntoResponse, ApiError> {
    // Validate the resource
    provider
        .validate()
        .map_err(|e| ApiError::bad_request(e.to_string()))?;

    let uuid = Uuid::parse_str(&id).map_err(|_| ApiError::bad_request("Invalid UUID format"))?;

    let storage = IdentityProviderStorage::new(&state.pool);

    // Ensure the provider exists
    let existing = storage
        .find_by_id(uuid)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found(format!("IdentityProvider/{}", id)))?;

    // Check for duplicate issuer (if changed)
    let existing_issuer = existing
        .resource
        .get("issuer")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if provider.issuer != existing_issuer
        && storage
            .find_by_issuer(&provider.issuer)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?
            .is_some()
    {
        return Err(ApiError::conflict(
            "Identity provider with this issuer already exists",
        ));
    }

    // Check for duplicate name (if changed)
    let existing_name = existing
        .resource
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if provider.name != existing_name
        && storage
            .find_by_name(&provider.name)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?
            .is_some()
    {
        return Err(ApiError::conflict(
            "Identity provider with this name already exists",
        ));
    }

    // Set the ID from the path
    provider.id = Some(id.clone());

    // Convert to JSON for storage
    let resource = serde_json::to_value(&provider)
        .map_err(|e| ApiError::internal(format!("Failed to serialize resource: {}", e)))?;

    // Update in storage
    let _row = storage
        .update(uuid, resource)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    // Re-register with IdP service if available
    if let Some(ref service) = state.idp_auth_service
        && let Ok(config) = provider.to_config()
    {
        service.register_provider(config).await;
    }

    tracing::info!(
        id = %id,
        name = %provider.name,
        "Updated identity provider"
    );

    Ok(Json(provider))
}

/// DELETE /IdentityProvider/:id - Delete an identity provider.
pub async fn delete_identity_provider(
    State(state): State<AdminState>,
    Path(id): Path<String>,
    _admin: AdminAuth,
) -> Result<impl IntoResponse, ApiError> {
    let uuid = Uuid::parse_str(&id).map_err(|_| ApiError::bad_request("Invalid UUID format"))?;

    let idp_storage = IdentityProviderStorage::new(&state.pool);
    let user_storage = UserStorage::new(&state.pool);

    // Ensure the provider exists
    let _existing = idp_storage
        .find_by_id(uuid)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found(format!("IdentityProvider/{}", id)))?;

    // Check if any users are linked to this provider
    let linked_users = user_storage
        .count_by_identity_provider(&id)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    if linked_users > 0 {
        return Err(ApiError::conflict(format!(
            "Cannot delete: {} user(s) are linked to this identity provider",
            linked_users
        )));
    }

    // Delete from storage
    idp_storage
        .delete(uuid)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    // Note: IdP service cache will need to be invalidated separately
    // since IdpAuthService doesn't have an unregister method
    let _ = &state.idp_auth_service; // Suppress unused field warning

    tracing::info!(id = %id, "Deleted identity provider");

    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_uuid_parsing() {
        let valid = "550e8400-e29b-41d4-a716-446655440000";
        assert!(Uuid::parse_str(valid).is_ok());

        let invalid = "not-a-uuid";
        assert!(Uuid::parse_str(invalid).is_err());
    }
}
