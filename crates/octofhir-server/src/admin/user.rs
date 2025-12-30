//! User admin handlers.
//!
//! This module provides CRUD handlers for managing users, including
//! session management and password reset operations.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use octofhir_api::ApiError;
use octofhir_auth_postgres::{TokenStorage, UserStorage};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use octofhir_auth::middleware::AdminAuth;
use octofhir_auth::Bundle;

use super::state::AdminState;

// =============================================================================
// Request/Response Types
// =============================================================================

/// User resource for API responses.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserResource {
    /// Resource type (always "User")
    pub resource_type: String,

    /// User ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Username
    pub username: String,

    /// Email address
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,

    /// User's display name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// FHIR user reference
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fhir_user: Option<String>,

    /// User roles
    #[serde(default)]
    pub roles: Vec<String>,

    /// Whether the user is active
    #[serde(default = "default_active")]
    pub active: bool,

    /// Last login timestamp
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_login: Option<String>,

    /// MFA enabled status
    #[serde(default)]
    pub mfa_enabled: bool,

    /// External identities linked to this user
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identity: Option<Vec<LinkedIdentity>>,

    /// When the user was created
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,

    /// When the user was last updated
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

fn default_active() -> bool {
    true
}

/// External identity linked to a user.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkedIdentity {
    /// Identity provider reference
    pub provider: ProviderReference,

    /// External subject identifier
    pub external_id: String,

    /// Display name from the provider
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
}

/// Reference to an identity provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderReference {
    /// Reference string (e.g., "IdentityProvider/abc")
    pub reference: String,

    /// Display name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display: Option<String>,
}

impl UserResource {
    /// Validate the user resource.
    pub fn validate(&self) -> Result<(), String> {
        if self.username.is_empty() {
            return Err("Username is required".to_string());
        }
        if self.username.len() > 100 {
            return Err("Username must be 100 characters or less".to_string());
        }
        if let Some(ref email) = self.email {
            if !email.contains('@') {
                return Err("Invalid email format".to_string());
            }
        }
        Ok(())
    }
}

/// Search parameters for users.
#[derive(Debug, Deserialize)]
pub struct UserSearchParams {
    /// Filter by active status
    pub active: Option<bool>,

    /// Filter by username (partial match)
    pub username: Option<String>,

    /// Filter by email (partial match)
    pub email: Option<String>,

    /// Maximum results to return
    #[serde(rename = "_count")]
    pub count: Option<i64>,

    /// Number of results to skip
    #[serde(rename = "_offset")]
    pub offset: Option<i64>,
}

/// User session for API responses.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserSession {
    /// Session ID
    pub id: String,

    /// User ID
    pub user_id: String,

    /// Client ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,

    /// Client name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_name: Option<String>,

    /// IP address
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ip_address: Option<String>,

    /// User agent
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_agent: Option<String>,

    /// When the session was created
    pub created_at: String,

    /// When the session expires
    pub expires_at: String,

    /// Last activity timestamp
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_activity: Option<String>,
}

/// Password reset request.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PasswordResetRequest {
    /// New password
    pub new_password: String,

    /// Require password change on next login
    #[serde(default)]
    pub require_change: bool,
}

/// Bulk update request.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BulkUpdateRequest {
    /// User IDs to update
    pub ids: Vec<String>,

    /// Action to perform
    pub action: BulkAction,
}

/// Bulk action types.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BulkAction {
    /// Activate users
    Activate,
    /// Deactivate users
    Deactivate,
    /// Revoke all sessions
    RevokeSessions,
}

// =============================================================================
// Handlers
// =============================================================================

/// GET /User - Search/list users.
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

            // Filter by email
            if let Some(ref email_filter) = params.email {
                let email = row
                    .resource
                    .get("email")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if !email.to_lowercase().contains(&email_filter.to_lowercase()) {
                    return false;
                }
            }

            true
        })
        .filter_map(|row| {
            // Convert resource to UserResource, stripping sensitive fields
            let mut resource = row.resource;
            // Remove password hash from response
            if let Some(obj) = resource.as_object_mut() {
                obj.remove("password_hash");
                obj.remove("passwordHash");
            }
            serde_json::from_value(resource).ok()
        })
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

    // Remove sensitive fields
    let mut resource = row.resource;
    if let Some(obj) = resource.as_object_mut() {
        obj.remove("password_hash");
        obj.remove("passwordHash");
    }

    let user: UserResource = serde_json::from_value(resource)
        .map_err(|e| ApiError::internal(format!("Failed to parse resource: {}", e)))?;

    Ok(Json(user))
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
        return Err(ApiError::conflict("User with this username already exists"));
    }

    // Check for duplicate email
    if let Some(ref email) = user.email {
        if storage
            .find_by_email(email)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?
            .is_some()
        {
            return Err(ApiError::conflict("User with this email already exists"));
        }
    }

    // Generate ID and set it
    let id = Uuid::new_v4();
    user.id = Some(id.to_string());
    user.resource_type = "User".to_string();

    // Set timestamps
    let now = time::OffsetDateTime::now_utc();
    user.created_at = Some(
        now.format(&time::format_description::well_known::Rfc3339)
            .unwrap(),
    );
    user.updated_at = Some(
        now.format(&time::format_description::well_known::Rfc3339)
            .unwrap(),
    );

    // Convert to JSON for storage
    let resource = serde_json::to_value(&user)
        .map_err(|e| ApiError::internal(format!("Failed to serialize resource: {}", e)))?;

    // Create in storage
    let _row = storage
        .create(id, resource)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    tracing::info!(id = %id, username = %user.username, "Created user");

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
        return Err(ApiError::conflict("User with this username already exists"));
    }

    // Check for duplicate email (if changed)
    let existing_email = existing
        .resource
        .get("email")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if let Some(ref email) = user.email {
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
    user.resource_type = "User".to_string();

    // Preserve created_at, update updated_at
    user.created_at = existing
        .resource
        .get("createdAt")
        .and_then(|v| v.as_str())
        .map(String::from);
    let now = time::OffsetDateTime::now_utc();
    user.updated_at = Some(
        now.format(&time::format_description::well_known::Rfc3339)
            .unwrap(),
    );

    // Convert to JSON for storage
    let resource = serde_json::to_value(&user)
        .map_err(|e| ApiError::internal(format!("Failed to serialize resource: {}", e)))?;

    // Update in storage
    let _row = storage
        .update(uuid, resource)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    tracing::info!(id = %id, username = %user.username, "Updated user");

    Ok(Json(user))
}

/// DELETE /User/:id - Delete a user.
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

    // Revoke all user sessions before deletion
    let token_storage = TokenStorage::new(&state.pool);
    let _revoked = token_storage
        .revoke_by_user(uuid)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    // Delete from storage
    storage
        .delete(uuid)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    tracing::info!(id = %id, "Deleted user");

    Ok(StatusCode::NO_CONTENT)
}

/// GET /User/:id/sessions - Get user sessions.
pub async fn get_user_sessions(
    State(state): State<AdminState>,
    Path(id): Path<String>,
    _admin: AdminAuth,
) -> Result<impl IntoResponse, ApiError> {
    let uuid = Uuid::parse_str(&id).map_err(|_| ApiError::bad_request("Invalid UUID format"))?;

    // Verify user exists
    let user_storage = UserStorage::new(&state.pool);
    let _user = user_storage
        .find_by_id(uuid)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found(format!("User/{}", id)))?;

    // Get user's active tokens (which represent sessions)
    let token_storage = TokenStorage::new(&state.pool);
    let tokens = token_storage
        .list_by_user(uuid)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    // Convert to UserSession format
    let sessions: Vec<UserSession> = tokens
        .into_iter()
        .filter_map(|token| {
            let resource = &token.resource;
            Some(UserSession {
                id: token.id,
                user_id: resource.get("userId")?.as_str()?.to_string(),
                client_id: resource.get("clientId").and_then(|v| v.as_str()).map(String::from),
                client_name: resource.get("clientName").and_then(|v| v.as_str()).map(String::from),
                ip_address: resource.get("ipAddress").and_then(|v| v.as_str()).map(String::from),
                user_agent: resource.get("userAgent").and_then(|v| v.as_str()).map(String::from),
                created_at: token.created_at.format(&time::format_description::well_known::Rfc3339).ok()?,
                expires_at: resource.get("expiresAt")?.as_str()?.to_string(),
                last_activity: resource.get("lastUsedAt").and_then(|v| v.as_str()).map(String::from),
            })
        })
        .collect();

    Ok(Json(serde_json::json!({
        "sessions": sessions,
        "total": sessions.len()
    })))
}

/// DELETE /User/:id/sessions/:sessionId - Revoke a specific session.
pub async fn revoke_user_session(
    State(state): State<AdminState>,
    Path((user_id, session_id)): Path<(String, String)>,
    _admin: AdminAuth,
) -> Result<impl IntoResponse, ApiError> {
    let user_uuid =
        Uuid::parse_str(&user_id).map_err(|_| ApiError::bad_request("Invalid user UUID format"))?;
    let session_uuid = Uuid::parse_str(&session_id)
        .map_err(|_| ApiError::bad_request("Invalid session UUID format"))?;

    // Verify user exists
    let user_storage = UserStorage::new(&state.pool);
    let _user = user_storage
        .find_by_id(user_uuid)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found(format!("User/{}", user_id)))?;

    // Verify session belongs to user
    let token_storage = TokenStorage::new(&state.pool);
    let token = token_storage
        .find_by_id(session_uuid)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found(format!("Session/{}", session_id)))?;

    let token_user_id = token
        .resource
        .get("userId")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if token_user_id != user_uuid.to_string() {
        return Err(ApiError::not_found(format!("Session/{}", session_id)));
    }

    // Revoke the session
    token_storage
        .revoke(session_uuid)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    tracing::info!(user_id = %user_id, session_id = %session_id, "Revoked user session");

    Ok(StatusCode::NO_CONTENT)
}

/// DELETE /User/:id/sessions - Revoke all user sessions.
pub async fn revoke_all_user_sessions(
    State(state): State<AdminState>,
    Path(id): Path<String>,
    _admin: AdminAuth,
) -> Result<impl IntoResponse, ApiError> {
    let uuid = Uuid::parse_str(&id).map_err(|_| ApiError::bad_request("Invalid UUID format"))?;

    // Verify user exists
    let user_storage = UserStorage::new(&state.pool);
    let _user = user_storage
        .find_by_id(uuid)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found(format!("User/{}", id)))?;

    // Revoke all sessions
    let token_storage = TokenStorage::new(&state.pool);
    let revoked = token_storage
        .revoke_by_user(uuid)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    tracing::info!(user_id = %id, sessions_revoked = %revoked, "Revoked all user sessions");

    Ok(Json(serde_json::json!({
        "sessionsRevoked": revoked
    })))
}

/// POST /User/:id/$reset-password - Reset user password.
pub async fn reset_user_password(
    State(state): State<AdminState>,
    Path(id): Path<String>,
    _admin: AdminAuth,
    Json(request): Json<PasswordResetRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let uuid = Uuid::parse_str(&id).map_err(|_| ApiError::bad_request("Invalid UUID format"))?;

    // Validate password
    if request.new_password.len() < 8 {
        return Err(ApiError::bad_request(
            "Password must be at least 8 characters",
        ));
    }

    let storage = UserStorage::new(&state.pool);

    // Ensure the user exists
    let existing = storage
        .find_by_id(uuid)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found(format!("User/{}", id)))?;

    // Hash the new password
    let password_hash = bcrypt::hash(&request.new_password, bcrypt::DEFAULT_COST)
        .map_err(|e| ApiError::internal(format!("Failed to hash password: {}", e)))?;

    // Update the user with new password hash
    let mut resource = existing.resource;
    if let Some(obj) = resource.as_object_mut() {
        obj.insert("passwordHash".to_string(), serde_json::json!(password_hash));
        if request.require_change {
            obj.insert("requirePasswordChange".to_string(), serde_json::json!(true));
        }
        let now = time::OffsetDateTime::now_utc();
        obj.insert(
            "updatedAt".to_string(),
            serde_json::json!(now
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap()),
        );
    }

    // Update in storage
    storage
        .update(uuid, resource)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    // Optionally revoke all existing sessions
    let token_storage = TokenStorage::new(&state.pool);
    let revoked = token_storage
        .revoke_by_user(uuid)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    tracing::info!(user_id = %id, sessions_revoked = %revoked, "Reset user password");

    Ok(Json(serde_json::json!({
        "success": true,
        "sessionsRevoked": revoked
    })))
}

/// POST /User/$bulk - Bulk update users.
pub async fn bulk_update_users(
    State(state): State<AdminState>,
    _admin: AdminAuth,
    Json(request): Json<BulkUpdateRequest>,
) -> Result<impl IntoResponse, ApiError> {
    if request.ids.is_empty() {
        return Err(ApiError::bad_request("No user IDs provided"));
    }

    let user_storage = UserStorage::new(&state.pool);
    let token_storage = TokenStorage::new(&state.pool);

    let mut success_count = 0;
    let mut error_count = 0;
    let mut errors: Vec<String> = Vec::new();

    for id_str in &request.ids {
        let uuid = match Uuid::parse_str(id_str) {
            Ok(u) => u,
            Err(_) => {
                error_count += 1;
                errors.push(format!("Invalid UUID: {}", id_str));
                continue;
            }
        };

        // Get existing user
        let existing = match user_storage.find_by_id(uuid).await {
            Ok(Some(row)) => row,
            Ok(None) => {
                error_count += 1;
                errors.push(format!("User not found: {}", id_str));
                continue;
            }
            Err(e) => {
                error_count += 1;
                errors.push(format!("Error finding user {}: {}", id_str, e));
                continue;
            }
        };

        let mut resource = existing.resource;

        match request.action {
            BulkAction::Activate => {
                if let Some(obj) = resource.as_object_mut() {
                    obj.insert("active".to_string(), serde_json::json!(true));
                }
            }
            BulkAction::Deactivate => {
                if let Some(obj) = resource.as_object_mut() {
                    obj.insert("active".to_string(), serde_json::json!(false));
                }
                // Also revoke sessions for deactivated users
                let _ = token_storage.revoke_by_user(uuid).await;
            }
            BulkAction::RevokeSessions => {
                if let Err(e) = token_storage.revoke_by_user(uuid).await {
                    error_count += 1;
                    errors.push(format!("Error revoking sessions for {}: {}", id_str, e));
                    continue;
                }
                success_count += 1;
                continue; // No need to update user resource
            }
        }

        // Update timestamp
        if let Some(obj) = resource.as_object_mut() {
            let now = time::OffsetDateTime::now_utc();
            obj.insert(
                "updatedAt".to_string(),
                serde_json::json!(now
                    .format(&time::format_description::well_known::Rfc3339)
                    .unwrap()),
            );
        }

        // Save updated user
        match user_storage.update(uuid, resource).await {
            Ok(_) => success_count += 1,
            Err(e) => {
                error_count += 1;
                errors.push(format!("Error updating user {}: {}", id_str, e));
            }
        }
    }

    tracing::info!(
        action = ?request.action,
        success = success_count,
        errors = error_count,
        "Bulk user update completed"
    );

    Ok(Json(serde_json::json!({
        "success": success_count,
        "errors": error_count,
        "errorDetails": errors
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_user_resource_validation() {
        let user = UserResource {
            resource_type: "User".to_string(),
            id: None,
            username: "testuser".to_string(),
            email: Some("test@example.com".to_string()),
            name: Some("Test User".to_string()),
            fhir_user: None,
            roles: vec!["user".to_string()],
            active: true,
            last_login: None,
            mfa_enabled: false,
            identity: None,
            created_at: None,
            updated_at: None,
        };
        assert!(user.validate().is_ok());

        let empty_username = UserResource {
            resource_type: "User".to_string(),
            id: None,
            username: String::new(),
            email: None,
            name: None,
            fhir_user: None,
            roles: vec![],
            active: true,
            last_login: None,
            mfa_enabled: false,
            identity: None,
            created_at: None,
            updated_at: None,
        };
        assert!(empty_username.validate().is_err());

        let invalid_email = UserResource {
            resource_type: "User".to_string(),
            id: None,
            username: "testuser".to_string(),
            email: Some("invalid-email".to_string()),
            name: None,
            fhir_user: None,
            roles: vec![],
            active: true,
            last_login: None,
            mfa_enabled: false,
            identity: None,
            created_at: None,
            updated_at: None,
        };
        assert!(invalid_email.validate().is_err());
    }
}
