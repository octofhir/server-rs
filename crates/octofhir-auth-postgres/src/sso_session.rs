//! PostgreSQL implementation of SSO session storage.
//!
//! This module provides storage operations for SSO authentication sessions
//! (AuthSession resources) using FHIR storage and search.

use async_trait::async_trait;
use octofhir_auth::storage::sso_session::{SsoSessionStorage, StorageError};
use octofhir_storage::{FhirStorage, SearchParams};
use serde_json::json;
use std::sync::Arc;
use time::OffsetDateTime;

/// PostgreSQL implementation of SSO session storage.
///
/// Uses FHIR storage for AuthSession resources with search parameters
/// for efficient token and user lookups.
pub struct PostgresSsoSessionStorage {
    fhir_storage: Arc<dyn FhirStorage>,
}

impl PostgresSsoSessionStorage {
    /// Create a new PostgreSQL SSO session storage.
    pub fn new(fhir_storage: Arc<dyn FhirStorage>) -> Self {
        Self { fhir_storage }
    }
}

#[async_trait]
impl SsoSessionStorage for PostgresSsoSessionStorage {
    async fn find_session_by_token(&self, token: &str) -> Result<Option<String>, StorageError> {
        // Search for active session by token
        let params = SearchParams::new()
            .with_param("session-token", token)
            .with_param("status", "active");

        let result = self
            .fhir_storage
            .search("AuthSession", &params)
            .await
            .map_err(|e| StorageError::Database(e.to_string()))?;

        // Return the first matching session ID if not expired
        if let Some(first) = result.entries.first() {
            let resource = &first.resource;
            let is_valid = resource
                .get("expiresAt")
                .and_then(|v| v.as_str())
                .and_then(|expires_at| {
                    OffsetDateTime::parse(
                        expires_at,
                        &time::format_description::well_known::Rfc3339,
                    )
                    .ok()
                })
                .is_some_and(|expiry| expiry > OffsetDateTime::now_utc());

            if is_valid && let Some(id) = resource.get("id").and_then(|v| v.as_str()) {
                return Ok(Some(id.to_string()));
            }
        }

        Ok(None)
    }

    async fn revoke_session(&self, session_id: &str) -> Result<(), StorageError> {
        // Load the AuthSession resource
        let stored = self
            .fhir_storage
            .read("AuthSession", session_id)
            .await
            .map_err(|e| match e {
                octofhir_storage::StorageError::NotFound { .. } => StorageError::NotFound,
                _ => StorageError::Database(e.to_string()),
            })?
            .ok_or(StorageError::NotFound)?;

        // Update status to "revoked"
        let mut resource = stored.resource;
        resource["status"] = json!("revoked");

        // Update the resource
        self.fhir_storage
            .update(&resource, None)
            .await
            .map_err(|e| StorageError::Database(e.to_string()))?;

        Ok(())
    }

    async fn count_active_sessions(&self, user_id: &str) -> Result<u32, StorageError> {
        // Search for active sessions for user
        let now = OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .map_err(|e| StorageError::Internal(e.to_string()))?;

        let params = SearchParams::new()
            .with_param("subject", format!("User/{}", user_id))
            .with_param("status", "active")
            .with_param("expires-at", format!("gt{}", now))
            .with_param("_summary", "count");

        let result = self
            .fhir_storage
            .search("AuthSession", &params)
            .await
            .map_err(|e| StorageError::Database(e.to_string()))?;

        // Use total if available, otherwise count entries
        let count = result.total.unwrap_or(result.entries.len() as u32);
        Ok(count)
    }

    async fn cleanup_expired_sessions(&self) -> Result<u64, StorageError> {
        // Search for expired sessions
        let now = OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .map_err(|e| StorageError::Internal(e.to_string()))?;

        let params = SearchParams::new()
            .with_param("expires-at", format!("lt{}", now))
            .with_param("_count", "100"); // Process in batches

        let result = self
            .fhir_storage
            .search("AuthSession", &params)
            .await
            .map_err(|e| StorageError::Database(e.to_string()))?;

        let mut deleted = 0u64;

        // Delete each expired session
        for entry in &result.entries {
            if let Some(id) = entry.resource.get("id").and_then(|v| v.as_str())
                && self.fhir_storage.delete("AuthSession", id).await.is_ok()
            {
                deleted += 1;
            }
        }

        Ok(deleted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // TODO: Add integration tests when we have a test database setup
}
