use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use octofhir_api::ApiError;
use octofhir_auth_postgres::PostgresAuthStorage;
use octofhir_storage::{FhirStorage, SearchParams};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use tracing::{error, info, instrument};
use uuid::Uuid;

/// State for AuthSession operations
#[derive(Clone)]
pub struct AuthSessionOperationState {
    pub storage: Arc<dyn FhirStorage>,
    pub auth_storage: Arc<PostgresAuthStorage>,
}

/// Input parameters for $revoke operation on a specific session
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RevokeSessionInput {
    /// Optional reason for revocation
    pub reason: Option<String>,
}

/// Input parameters for $revoke-all operation (type-level)
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RevokeAllSessionsInput {
    /// User ID to revoke sessions for
    pub subject: String, // Format: "User/{id}"

    /// Optional: session ID to exclude from revocation (keep current session alive)
    pub exclude_session: Option<String>,

    /// Optional reason for revocation
    pub reason: Option<String>,
}

/// Response for revoke operations
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RevokeResponse {
    pub success: bool,
    pub revoked_count: usize,
    pub message: String,
}

/// POST /AuthSession/:id/$revoke - Revoke a specific session
#[instrument(skip(state))]
pub async fn revoke_session(
    State(state): State<AuthSessionOperationState>,
    Path(session_id): Path<String>,
    Json(input): Json<RevokeSessionInput>,
) -> Result<Response, ApiError> {
    info!(session_id = %session_id, "Revoking auth session");

    // 1. Load the AuthSession resource
    let stored = state
        .storage
        .read("AuthSession", &session_id)
        .await
        .map_err(|e| {
            error!("Failed to read AuthSession {}: {}", session_id, e);
            ApiError::internal(format!("Failed to read session: {}", e))
        })?;

    let mut stored = stored.ok_or_else(|| {
        error!("AuthSession {} not found", session_id);
        ApiError::not_found(format!("AuthSession {} not found", session_id))
    })?;

    // 2. Update session status to "revoked"
    stored.resource["status"] = json!("revoked");

    // Add revocation metadata
    if let Some(reason) = &input.reason
        && let Some(ext) = stored.resource.get_mut("extension").and_then(|e: &mut serde_json::Value| e.as_array_mut()) {
            ext.push(json!({
                "url": "http://octofhir.io/fhir/StructureDefinition/revocation-reason",
                "valueString": reason
            }));
        }

    // 3. Update the resource in storage
    state
        .storage
        .update(&stored.resource, None)
        .await
        .map_err(|e| {
            error!("Failed to update AuthSession {}: {}", session_id, e);
            ApiError::internal(format!("Failed to update session: {}", e))
        })?;

    // 4. Delete from token index
    let token_index = state.auth_storage.session_token_index();
    let resource_id = format!("AuthSession/{}", session_id);

    if let Err(e) = token_index.delete(&resource_id).await {
        error!(
            "Failed to delete session token index for {}: {}",
            resource_id, e
        );
        // Don't fail the operation if index deletion fails
    }

    info!(session_id = %session_id, "Session revoked successfully");

    let response = RevokeResponse {
        success: true,
        revoked_count: 1,
        message: format!("Session {} revoked successfully", session_id),
    };

    Ok((StatusCode::OK, Json(response)).into_response())
}

/// POST /AuthSession/$revoke-all - Revoke all sessions for a user
#[instrument(skip(state))]
pub async fn revoke_all_sessions(
    State(state): State<AuthSessionOperationState>,
    Json(input): Json<RevokeAllSessionsInput>,
) -> Result<Response, ApiError> {
    info!(subject = %input.subject, "Revoking all sessions for user");

    // 1. Parse user ID from subject reference
    let user_id = input
        .subject
        .strip_prefix("User/")
        .ok_or_else(|| {
            error!("Invalid subject format: {}", input.subject);
            ApiError::bad_request("Subject must be in format 'User/{id}'")
        })?;

    let user_uuid = Uuid::parse_str(user_id).map_err(|_| {
        error!("Invalid user UUID: {}", user_id);
        ApiError::bad_request("Invalid user ID format")
    })?;

    // 2. Search for all active sessions for this user
    let search_params = SearchParams::new()
        .with_param("subject", &input.subject)
        .with_param("status", "active")
        .with_count(1000); // Get all sessions

    let result = state
        .storage
        .search("AuthSession", &search_params)
        .await
        .map_err(|e| {
            error!("Failed to search sessions for user {}: {}", user_id, e);
            ApiError::internal(format!("Failed to search sessions: {}", e))
        })?;

    // 3. Extract session IDs from SearchResult
    let sessions: Vec<String> = result
        .entries
        .iter()
        .map(|stored| stored.id.clone())
        .collect();

    // 4. Filter out excluded session if specified
    let sessions_to_revoke: Vec<String> = sessions
        .into_iter()
        .filter(|id| {
            if let Some(ref exclude) = input.exclude_session {
                id != exclude
            } else {
                true
            }
        })
        .collect();

    let mut revoked_count = 0;

    // 5. Revoke each session
    for session_id in &sessions_to_revoke {
        // Load and update session
        if let Ok(Some(mut stored)) = state.storage.read("AuthSession", session_id).await {
            stored.resource["status"] = json!("revoked");

            // Add revocation metadata
            if let Some(reason) = &input.reason
                && let Some(ext) = stored.resource.get_mut("extension").and_then(|e: &mut serde_json::Value| e.as_array_mut()) {
                    ext.push(json!({
                        "url": "http://octofhir.io/fhir/StructureDefinition/revocation-reason",
                        "valueString": reason
                    }));
                }

            if state
                .storage
                .update(&stored.resource, None)
                .await
                .is_ok()
            {
                revoked_count += 1;

                // Delete from token index
                let token_index = state.auth_storage.session_token_index();
                let resource_id = format!("AuthSession/{}", session_id);
                let _ = token_index.delete(&resource_id).await;
            } else {
                error!("Failed to update session {}", session_id);
            }
        }
    }

    // 6. Also delete from token index by user_id (cleanup any orphaned entries)
    let token_index = state.auth_storage.session_token_index();
    if let Ok(deleted) = token_index.delete_all_for_user(user_uuid).await {
        info!(
            user_id = %user_id,
            deleted_from_index = deleted,
            "Cleaned up session token index"
        );
    }

    info!(
        user_id = %user_id,
        revoked_count,
        "Successfully revoked sessions"
    );

    let response = RevokeResponse {
        success: true,
        revoked_count,
        message: format!(
            "Revoked {} session(s) for user {}",
            revoked_count, user_id
        ),
    };

    Ok((StatusCode::OK, Json(response)).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_user_subject() {
        let subject = "User/123e4567-e89b-12d3-a456-426614174000";
        let user_id = subject.strip_prefix("User/").unwrap();
        assert!(Uuid::parse_str(user_id).is_ok());
    }

    #[test]
    fn test_invalid_subject_format() {
        let subject = "Patient/123";
        assert!(subject.strip_prefix("User/").is_none());
    }
}
