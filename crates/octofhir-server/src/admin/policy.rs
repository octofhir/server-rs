//! Policy management admin endpoints.
//!
//! Provides REST endpoints for managing access policy cache and hot-reload.
//!
//! # Endpoints
//!
//! - `POST /admin/policies/$reload` - Trigger policy cache reload
//! - `GET /admin/policies/status` - Get policy cache status and statistics

use std::sync::Arc;

use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use serde::Serialize;

use octofhir_api::ApiError;
use octofhir_auth::middleware::AdminAuth;
use octofhir_auth::policy::{PolicyCache, PolicyReloadService, ReloadStats};

// =============================================================================
// Types
// =============================================================================

/// State for policy management endpoints.
#[derive(Clone)]
pub struct PolicyState {
    /// Policy cache for access control.
    pub cache: Arc<PolicyCache>,

    /// Policy reload service for hot-reload.
    pub reload_service: Arc<PolicyReloadService>,
}

impl PolicyState {
    /// Create a new policy state.
    #[must_use]
    pub fn new(cache: Arc<PolicyCache>, reload_service: Arc<PolicyReloadService>) -> Self {
        Self {
            cache,
            reload_service,
        }
    }
}

/// Response for reload endpoint.
#[derive(Debug, Serialize)]
pub struct ReloadResponse {
    /// Whether the reload was triggered successfully.
    pub success: bool,
    /// Cache version before reload.
    pub version_before: u64,
    /// Cache version after reload.
    pub version_after: u64,
    /// Human-readable message.
    pub message: String,
}

/// Response for policy status endpoint.
#[derive(Debug, Serialize)]
pub struct PolicyStatusResponse {
    /// Current cache version.
    pub cache_version: u64,
    /// Number of cached policies.
    pub policy_count: usize,
    /// Last refresh timestamp (ISO 8601).
    pub last_refresh: String,
    /// Policy reload statistics.
    pub reload_stats: ReloadStatsResponse,
}

/// Serializable reload statistics.
#[derive(Debug, Serialize)]
pub struct ReloadStatsResponse {
    /// Total reload attempts.
    pub reload_attempts: u64,
    /// Successful reloads.
    pub successful_reloads: u64,
    /// Failed reloads.
    pub failed_reloads: u64,
    /// Notifications received.
    pub notifications_received: u64,
    /// Notifications debounced.
    pub notifications_debounced: u64,
}

impl From<ReloadStats> for ReloadStatsResponse {
    fn from(stats: ReloadStats) -> Self {
        Self {
            reload_attempts: stats.reload_attempts,
            successful_reloads: stats.successful_reloads,
            failed_reloads: stats.failed_reloads,
            notifications_received: stats.notifications_received,
            notifications_debounced: stats.notifications_debounced,
        }
    }
}

// =============================================================================
// Handlers
// =============================================================================

/// Trigger a policy cache reload.
///
/// POST /admin/policies/$reload
///
/// Triggers an immediate reload of the policy cache. This is useful after
/// making policy changes that need to take effect immediately.
pub async fn reload_policies(
    State(state): State<PolicyState>,
    admin: AdminAuth,
) -> Result<impl IntoResponse, ApiError> {
    tracing::info!(
        admin_user = %admin.username,
        "Triggering policy reload"
    );

    let version_before = state.cache.version().await;

    // Trigger reload
    state.reload_service.trigger_reload();

    // Wait briefly for reload to complete
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    let version_after = state.cache.version().await;

    let message = if version_after > version_before {
        format!(
            "Policies reloaded successfully (v{} -> v{})",
            version_before, version_after
        )
    } else {
        "Reload triggered, no policy changes detected".to_string()
    };

    tracing::info!(
        version_before = version_before,
        version_after = version_after,
        "Policy reload completed"
    );

    Ok((
        StatusCode::OK,
        Json(ReloadResponse {
            success: true,
            version_before,
            version_after,
            message,
        }),
    ))
}

/// Get policy cache status and statistics.
///
/// GET /admin/policies/status
///
/// Returns information about the policy cache including:
/// - Current cache version
/// - Number of cached policies
/// - Last refresh timestamp
/// - Reload statistics
pub async fn policy_status(
    State(state): State<PolicyState>,
    admin: AdminAuth,
) -> Result<impl IntoResponse, ApiError> {
    tracing::debug!(
        admin_user = %admin.username,
        "Getting policy status"
    );

    let cache_stats = state.cache.stats().await;
    let reload_stats = state.reload_service.stats();

    Ok(Json(PolicyStatusResponse {
        cache_version: cache_stats.version,
        policy_count: cache_stats.policy_count,
        last_refresh: cache_stats.last_refresh.to_string(),
        reload_stats: reload_stats.into(),
    }))
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reload_stats_response_from() {
        let stats = ReloadStats {
            reload_attempts: 10,
            successful_reloads: 8,
            failed_reloads: 2,
            notifications_received: 15,
            notifications_debounced: 5,
        };

        let response: ReloadStatsResponse = stats.into();

        assert_eq!(response.reload_attempts, 10);
        assert_eq!(response.successful_reloads, 8);
        assert_eq!(response.failed_reloads, 2);
        assert_eq!(response.notifications_received, 15);
        assert_eq!(response.notifications_debounced, 5);
    }
}
