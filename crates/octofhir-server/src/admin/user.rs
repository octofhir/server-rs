//! User admin handlers.
//!
//! This module provides CRUD handlers for managing users and their external identities.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use octofhir_api::ApiError;
use octofhir_auth_postgres::{IdentityProviderStorage, UserStorage};
use time::OffsetDateTime;
use uuid::Uuid;

use octofhir_auth::federation::resources::{Reference, UserIdentityElement, UserResource};
use octofhir_auth::middleware::AdminAuth;
use octofhir_auth::{Bundle, LinkIdentityRequest, UnlinkIdentityRequest, UserSearchParams};

use super::state::AdminState;

// =============================================================================
// Handlers
// =============================================================================

/// GET /User - Search/list users.
///
/// Query parameters:
/// - `email`: Filter by email address
/// - `username`: Filter by username (partial match)
/// - `active`: Filter by active status
/// - `identity-provider`: Filter by linked identity provider ID
/// - `_count`: Maximum results to return (default: 100)
/// - `_offset`: Number of results to skip (default: 0)
pub async fn search_users(
    State(state): State<AdminState>,
    Query(params): Query<UserSearchParams>,
    _admin: AdminAuth,
) -> Result<impl IntoResponse, ApiError> {
    let storage = UserStorage::new(&state.pool);

    let limit = params.count.unwrap_or(100);
    let offset = params.offset.unwrap_or(0);

    let rows = storage
        .list(limit, offset)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    // Filter and convert to UserResource
    let resources: Vec<UserResource> = rows
        .into_iter()
        .filter(|row| {
            // Filter by email
            if let Some(ref email_filter) = params.email {
                let email = row
                    .resource
                    .get("email")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if !email.eq_ignore_ascii_case(email_filter) {
                    return false;
                }
            }

            // Filter by username
            if let Some(ref username_filter) = params.username {
                let username = row
                    .resource
                    .get("username")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if !username
                    .to_lowercase()
                    .contains(&username_filter.to_lowercase())
                {
                    return false;
                }
            }

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

            // Filter by identity provider
            if let Some(ref provider_filter) = params.identity_provider {
                let provider_ref = format!("IdentityProvider/{}", provider_filter);
                let has_provider = row
                    .resource
                    .get("identity")
                    .and_then(|v| v.as_array())
                    .is_some_and(|identities| {
                        identities.iter().any(|i| {
                            i.get("provider")
                                .and_then(|p| p.get("reference"))
                                .and_then(|r| r.as_str())
                                .is_some_and(|r| r == provider_ref)
                        })
                    });
                if !has_provider {
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

/// GET /User/:id - Read a single user.
pub async fn read_user(
    State(state): State<AdminState>,
    Path(id): Path<String>,
    _admin: AdminAuth,
) -> Result<impl IntoResponse, ApiError> {
    let storage = UserStorage::new(&state.pool);

    let uuid = Uuid::parse_str(&id).map_err(|_| ApiError::bad_request("Invalid UUID format"))?;

    let row = storage
        .find_by_id(uuid)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found(format!("User/{}", id)))?;

    let resource: UserResource = serde_json::from_value(row.resource)
        .map_err(|e| ApiError::internal(format!("Failed to parse resource: {}", e)))?;

    Ok(Json(resource))
}

/// POST /User - Create a new user.
pub async fn create_user(
    State(state): State<AdminState>,
    _admin: AdminAuth,
    Json(mut user): Json<UserResource>,
) -> Result<impl IntoResponse, ApiError> {
    // Validate the resource
    user.validate()
        .map_err(|e| ApiError::bad_request(e.to_string()))?;

    // Ensure no ID is provided (will be generated)
    if user.id.is_some() {
        return Err(ApiError::bad_request(
            "ID must not be provided for create operation",
        ));
    }

    let storage = UserStorage::new(&state.pool);

    // Check for duplicate username
    if storage
        .find_by_username(&user.username)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .is_some()
    {
        return Err(ApiError::conflict(
            "User with this username already exists",
        ));
    }

    // Check for duplicate email (if provided)
    if let Some(ref email) = user.email
        && storage
            .find_by_email(email)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?
            .is_some()
    {
        return Err(ApiError::conflict("User with this email already exists"));
    }

    // Generate ID and set it
    let id = Uuid::new_v4();
    user.id = Some(id.to_string());

    // Convert to JSON for storage
    let resource = serde_json::to_value(&user)
        .map_err(|e| ApiError::internal(format!("Failed to serialize resource: {}", e)))?;

    // Create in storage
    let _row = storage
        .create(id, resource)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    tracing::info!(
        id = %id,
        username = %user.username,
        "Created user"
    );

    Ok((StatusCode::CREATED, Json(user)))
}

/// PUT /User/:id - Update an existing user.
pub async fn update_user(
    State(state): State<AdminState>,
    Path(id): Path<String>,
    _admin: AdminAuth,
    Json(mut user): Json<UserResource>,
) -> Result<impl IntoResponse, ApiError> {
    // Validate the resource
    user.validate()
        .map_err(|e| ApiError::bad_request(e.to_string()))?;

    let uuid = Uuid::parse_str(&id).map_err(|_| ApiError::bad_request("Invalid UUID format"))?;

    let storage = UserStorage::new(&state.pool);

    // Ensure the user exists
    let existing = storage
        .find_by_id(uuid)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found(format!("User/{}", id)))?;

    // Check for duplicate username (if changed)
    let existing_username = existing
        .resource
        .get("username")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if user.username != existing_username
        && storage
            .find_by_username(&user.username)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?
            .is_some()
    {
        return Err(ApiError::conflict(
            "User with this username already exists",
        ));
    }

    // Check for duplicate email (if changed)
    if let Some(ref email) = user.email {
        let existing_email = existing
            .resource
            .get("email")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if email != existing_email
            && storage
                .find_by_email(email)
                .await
                .map_err(|e| ApiError::internal(e.to_string()))?
                .is_some()
        {
            return Err(ApiError::conflict("User with this email already exists"));
        }
    }

    // Set the ID from the path
    user.id = Some(id.clone());

    // Convert to JSON for storage
    let resource = serde_json::to_value(&user)
        .map_err(|e| ApiError::internal(format!("Failed to serialize resource: {}", e)))?;

    // Update in storage
    let _row = storage
        .update(uuid, resource)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    tracing::info!(
        id = %id,
        username = %user.username,
        "Updated user"
    );

    Ok(Json(user))
}

/// DELETE /User/:id - Delete a user (soft delete).
pub async fn delete_user(
    State(state): State<AdminState>,
    Path(id): Path<String>,
    _admin: AdminAuth,
) -> Result<impl IntoResponse, ApiError> {
    let uuid = Uuid::parse_str(&id).map_err(|_| ApiError::bad_request("Invalid UUID format"))?;

    let storage = UserStorage::new(&state.pool);

    // Ensure the user exists
    let _existing = storage
        .find_by_id(uuid)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found(format!("User/{}", id)))?;

    // Delete from storage
    storage
        .delete(uuid)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    tracing::info!(id = %id, "Deleted user");

    Ok(StatusCode::NO_CONTENT)
}

/// POST /User/:id/$link-identity - Link an external identity to a user.
pub async fn link_identity(
    State(state): State<AdminState>,
    Path(id): Path<String>,
    _admin: AdminAuth,
    Json(request): Json<LinkIdentityRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let uuid = Uuid::parse_str(&id).map_err(|_| ApiError::bad_request("Invalid UUID format"))?;

    let user_storage = UserStorage::new(&state.pool);
    let idp_storage = IdentityProviderStorage::new(&state.pool);

    // Load the user
    let row = user_storage
        .find_by_id(uuid)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found(format!("User/{}", id)))?;

    let mut user: UserResource = serde_json::from_value(row.resource)
        .map_err(|e| ApiError::internal(format!("Failed to parse resource: {}", e)))?;

    // Validate that the identity provider exists
    let provider_uuid = Uuid::parse_str(&request.provider_id)
        .map_err(|_| ApiError::bad_request("Invalid provider ID format"))?;

    let _provider = idp_storage
        .find_by_id(provider_uuid)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| {
            ApiError::bad_request(format!(
                "Identity provider not found: {}",
                request.provider_id
            ))
        })?;

    // Check if this identity is already linked to another user
    let existing_user = user_storage
        .find_by_external_identity(&request.provider_id, &request.external_id)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    if let Some(existing) = existing_user
        && existing.id != uuid
    {
        return Err(ApiError::conflict(
            "This identity is already linked to another user",
        ));
    }

    // Remove existing identity for this provider (if any)
    let provider_ref = format!("IdentityProvider/{}", request.provider_id);
    user.identity
        .retain(|i| i.provider.reference.as_deref() != Some(&provider_ref));

    // Add the new identity
    let now = OffsetDateTime::now_utc();
    let linked_at = now
        .format(&time::format_description::well_known::Rfc3339)
        .ok();

    user.identity.push(UserIdentityElement {
        provider: Reference {
            reference: Some(provider_ref),
            display: None,
        },
        external_id: request.external_id.clone(),
        email: request.email.clone(),
        linked_at,
    });

    // Save the updated user
    let resource = serde_json::to_value(&user)
        .map_err(|e| ApiError::internal(format!("Failed to serialize resource: {}", e)))?;

    let _row = user_storage
        .update(uuid, resource)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    tracing::info!(
        user_id = %id,
        provider_id = %request.provider_id,
        external_id = %request.external_id,
        "Linked identity to user"
    );

    Ok(Json(user))
}

/// POST /User/:id/$unlink-identity - Unlink an external identity from a user.
pub async fn unlink_identity(
    State(state): State<AdminState>,
    Path(id): Path<String>,
    _admin: AdminAuth,
    Json(request): Json<UnlinkIdentityRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let uuid = Uuid::parse_str(&id).map_err(|_| ApiError::bad_request("Invalid UUID format"))?;

    let storage = UserStorage::new(&state.pool);

    // Load the user
    let row = storage
        .find_by_id(uuid)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found(format!("User/{}", id)))?;

    let mut user: UserResource = serde_json::from_value(row.resource)
        .map_err(|e| ApiError::internal(format!("Failed to parse resource: {}", e)))?;

    // Check that the user has this identity
    let provider_ref = format!("IdentityProvider/{}", request.provider_id);
    let has_identity = user
        .identity
        .iter()
        .any(|i| i.provider.reference.as_deref() == Some(&provider_ref));

    if !has_identity {
        return Err(ApiError::not_found(format!(
            "User does not have an identity linked to provider: {}",
            request.provider_id
        )));
    }

    // Check that this isn't the last identity (if user has no password)
    let has_password = user.password_hash.is_some();
    if !has_password && user.identity.len() <= 1 {
        return Err(ApiError::bad_request(
            "Cannot remove the last identity from a user without a password",
        ));
    }

    // Remove the identity
    user.identity
        .retain(|i| i.provider.reference.as_deref() != Some(&provider_ref));

    // Save the updated user
    let resource = serde_json::to_value(&user)
        .map_err(|e| ApiError::internal(format!("Failed to serialize resource: {}", e)))?;

    let _row = storage
        .update(uuid, resource)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    tracing::info!(
        user_id = %id,
        provider_id = %request.provider_id,
        "Unlinked identity from user"
    );

    Ok(Json(user))
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
