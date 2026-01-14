//! Role admin handlers.
//!
//! This module provides CRUD handlers for managing roles.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use octofhir_api::ApiError;
use octofhir_auth::storage::{Permission, default_permissions};
use octofhir_auth_postgres::RoleStorage;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use octofhir_auth::Bundle;
use octofhir_auth::middleware::AdminAuth;

use super::state::AdminState;

// =============================================================================
// Request/Response Types
// =============================================================================

/// Role resource for API responses.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoleResource {
    /// Resource type (always "Role")
    pub resource_type: String,

    /// Role ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Role name
    pub name: String,

    /// Role description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Permissions assigned to this role
    #[serde(default)]
    pub permissions: Vec<String>,

    /// Whether this is a system role
    #[serde(default)]
    pub is_system: bool,

    /// Whether the role is active
    #[serde(default = "default_active")]
    pub active: bool,

    /// When the role was created
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,

    /// When the role was last updated
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

fn default_active() -> bool {
    true
}

impl RoleResource {
    /// Validate the role resource.
    pub fn validate(&self) -> Result<(), String> {
        if self.name.is_empty() {
            return Err("Role name is required".to_string());
        }
        if self.name.len() > 100 {
            return Err("Role name must be 100 characters or less".to_string());
        }
        Ok(())
    }
}

/// Search parameters for roles.
#[derive(Debug, Deserialize)]
pub struct RoleSearchParams {
    /// Filter by active status
    pub active: Option<bool>,

    /// Filter by name (partial match)
    pub name: Option<String>,

    /// Maximum results to return
    #[serde(rename = "_count")]
    pub count: Option<i64>,

    /// Number of results to skip
    #[serde(rename = "_offset")]
    pub offset: Option<i64>,
}

// =============================================================================
// Handlers
// =============================================================================

/// GET /Role - Search/list roles.
///
/// Query parameters:
/// - `active`: Filter by active status
/// - `name`: Filter by name (partial match)
/// - `_count`: Maximum results to return (default: 100)
/// - `_offset`: Number of results to skip (default: 0)
pub async fn search_roles(
    State(state): State<AdminState>,
    Query(params): Query<RoleSearchParams>,
    _admin: AdminAuth,
) -> Result<impl IntoResponse, ApiError> {
    let storage = RoleStorage::new(&state.pool);

    let limit = params.count.unwrap_or(100);
    let offset = params.offset.unwrap_or(0);

    let rows = storage
        .list(limit, offset)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    // Filter and convert to RoleResource
    let resources: Vec<RoleResource> = rows
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

/// GET /Role/:id - Read a single role.
pub async fn read_role(
    State(state): State<AdminState>,
    Path(id): Path<String>,
    _admin: AdminAuth,
) -> Result<impl IntoResponse, ApiError> {
    let storage = RoleStorage::new(&state.pool);

    let uuid = Uuid::parse_str(&id).map_err(|_| ApiError::bad_request("Invalid UUID format"))?;

    let row = storage
        .find_by_id(uuid)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found(format!("Role/{}", id)))?;

    let resource: RoleResource = serde_json::from_value(row.resource)
        .map_err(|e| ApiError::internal(format!("Failed to parse resource: {}", e)))?;

    Ok(Json(resource))
}

/// POST /Role - Create a new role.
pub async fn create_role(
    State(state): State<AdminState>,
    _admin: AdminAuth,
    Json(mut role): Json<RoleResource>,
) -> Result<impl IntoResponse, ApiError> {
    // Validate the resource
    role.validate()
        .map_err(|e| ApiError::bad_request(e.to_string()))?;

    // Ensure no ID is provided (will be generated)
    if role.id.is_some() {
        return Err(ApiError::bad_request(
            "ID must not be provided for create operation",
        ));
    }

    // System roles cannot be created via API
    if role.is_system {
        return Err(ApiError::bad_request("Cannot create system roles via API"));
    }

    let storage = RoleStorage::new(&state.pool);

    // Check for duplicate name
    if storage
        .find_by_name(&role.name)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .is_some()
    {
        return Err(ApiError::conflict("Role with this name already exists"));
    }

    // Generate ID and set it
    let id = Uuid::new_v4();
    role.id = Some(id.to_string());
    role.resource_type = "Role".to_string();

    // Set timestamps
    let now = time::OffsetDateTime::now_utc();
    role.created_at = Some(
        now.format(&time::format_description::well_known::Rfc3339)
            .unwrap(),
    );
    role.updated_at = Some(
        now.format(&time::format_description::well_known::Rfc3339)
            .unwrap(),
    );

    // Convert to JSON for storage
    let resource = serde_json::to_value(&role)
        .map_err(|e| ApiError::internal(format!("Failed to serialize resource: {}", e)))?;

    // Create in storage
    let _row = storage
        .create(id, resource)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    tracing::info!(id = %id, name = %role.name, "Created role");

    Ok((StatusCode::CREATED, Json(role)))
}

/// PUT /Role/:id - Update an existing role.
pub async fn update_role(
    State(state): State<AdminState>,
    Path(id): Path<String>,
    _admin: AdminAuth,
    Json(mut role): Json<RoleResource>,
) -> Result<impl IntoResponse, ApiError> {
    // Validate the resource
    role.validate()
        .map_err(|e| ApiError::bad_request(e.to_string()))?;

    let uuid = Uuid::parse_str(&id).map_err(|_| ApiError::bad_request("Invalid UUID format"))?;

    let storage = RoleStorage::new(&state.pool);

    // Ensure the role exists
    let existing = storage
        .find_by_id(uuid)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found(format!("Role/{}", id)))?;

    // Check if it's a system role
    let is_system = existing
        .resource
        .get("isSystem")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    if is_system {
        // Can only update permissions on system roles
        let existing_name = existing
            .resource
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if role.name != existing_name {
            return Err(ApiError::bad_request("Cannot modify name of system roles"));
        }
    }

    // Check for duplicate name (if changed)
    let existing_name = existing
        .resource
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if role.name != existing_name
        && storage
            .find_by_name(&role.name)
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?
            .is_some()
    {
        return Err(ApiError::conflict("Role with this name already exists"));
    }

    // Set the ID from the path
    role.id = Some(id.clone());
    role.resource_type = "Role".to_string();
    role.is_system = is_system; // Preserve system flag

    // Preserve created_at, update updated_at
    role.created_at = existing
        .resource
        .get("createdAt")
        .and_then(|v| v.as_str())
        .map(String::from);
    let now = time::OffsetDateTime::now_utc();
    role.updated_at = Some(
        now.format(&time::format_description::well_known::Rfc3339)
            .unwrap(),
    );

    // Convert to JSON for storage
    let resource = serde_json::to_value(&role)
        .map_err(|e| ApiError::internal(format!("Failed to serialize resource: {}", e)))?;

    // Update in storage
    let _row = storage
        .update(uuid, resource)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    tracing::info!(id = %id, name = %role.name, "Updated role");

    Ok(Json(role))
}

/// DELETE /Role/:id - Delete a role.
pub async fn delete_role(
    State(state): State<AdminState>,
    Path(id): Path<String>,
    _admin: AdminAuth,
) -> Result<impl IntoResponse, ApiError> {
    let uuid = Uuid::parse_str(&id).map_err(|_| ApiError::bad_request("Invalid UUID format"))?;

    let storage = RoleStorage::new(&state.pool);

    // Ensure the role exists
    let existing = storage
        .find_by_id(uuid)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .ok_or_else(|| ApiError::not_found(format!("Role/{}", id)))?;

    // Check if it's a system role
    let is_system = existing
        .resource
        .get("isSystem")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    if is_system {
        return Err(ApiError::bad_request("Cannot delete system roles"));
    }

    // Get the role name to check for users
    let role_name = existing
        .resource
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    // Check if any users are assigned to this role
    let users_with_role = storage
        .count_users_with_role(role_name)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    if users_with_role > 0 {
        return Err(ApiError::conflict(format!(
            "Cannot delete: {} user(s) are assigned to this role",
            users_with_role
        )));
    }

    // Delete from storage
    storage
        .delete(uuid)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    tracing::info!(id = %id, "Deleted role");

    Ok(StatusCode::NO_CONTENT)
}

/// GET /Role/$permissions - Get available permissions.
pub async fn list_permissions(_admin: AdminAuth) -> Result<impl IntoResponse, ApiError> {
    let permissions = default_permissions();

    // Group permissions by category
    let mut grouped: std::collections::HashMap<String, Vec<&Permission>> =
        std::collections::HashMap::new();
    for perm in &permissions {
        let category = perm.category.as_deref().unwrap_or("Other");
        grouped.entry(category.to_string()).or_default().push(perm);
    }

    Ok(Json(serde_json::json!({
        "permissions": permissions,
        "categories": grouped.keys().collect::<Vec<_>>()
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_role_resource_validation() {
        let role = RoleResource {
            resource_type: "Role".to_string(),
            id: None,
            name: "admin".to_string(),
            description: Some("Administrator role".to_string()),
            permissions: vec!["system:admin".to_string()],
            is_system: false,
            active: true,
            created_at: None,
            updated_at: None,
        };
        assert!(role.validate().is_ok());

        let empty_name = RoleResource {
            resource_type: "Role".to_string(),
            id: None,
            name: String::new(),
            description: None,
            permissions: vec![],
            is_system: false,
            active: true,
            created_at: None,
            updated_at: None,
        };
        assert!(empty_name.validate().is_err());
    }
}
