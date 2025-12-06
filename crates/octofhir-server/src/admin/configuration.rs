//! Configuration management admin endpoints.
//!
//! Provides REST endpoints for managing runtime configuration and feature flags.
//!
//! # Endpoints
//!
//! ## Configuration
//!
//! - `GET /admin/config` - List all configuration entries
//! - `GET /admin/config/:category` - List configuration for a category
//! - `GET /admin/config/:category/:key` - Get a specific configuration value
//! - `PUT /admin/config/:category/:key` - Set a configuration value
//! - `DELETE /admin/config/:category/:key` - Delete (reset) a configuration value
//!
//! ## Feature Flags
//!
//! - `GET /admin/features` - List all feature flags
//! - `GET /admin/features/:name` - Get a specific feature flag
//! - `PUT /admin/features/:name` - Toggle a feature flag
//! - `POST /admin/features/:name/evaluate` - Evaluate a feature flag for a context

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use octofhir_api::ApiError;
use octofhir_auth::middleware::AdminAuth;
use octofhir_config::{ConfigCategory, ConfigurationManager, FeatureContext};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

// =============================================================================
// Types
// =============================================================================

/// Configuration state for admin endpoints
#[derive(Clone)]
pub struct ConfigState {
    /// Configuration manager
    pub config_manager: Arc<ConfigurationManager>,
}

impl ConfigState {
    /// Create a new config state
    pub fn new(config_manager: Arc<ConfigurationManager>) -> Self {
        Self { config_manager }
    }
}

/// Query parameters for listing configuration
#[derive(Debug, Deserialize)]
pub struct ConfigListParams {
    /// Filter by category
    pub category: Option<String>,
    /// Include secret values (masked)
    #[serde(default)]
    pub include_secrets: bool,
}

/// Request body for setting configuration
#[derive(Debug, Deserialize)]
pub struct SetConfigRequest {
    /// Configuration value
    pub value: serde_json::Value,
    /// Description of the configuration
    pub description: Option<String>,
    /// Whether this is a secret value
    #[serde(default)]
    pub is_secret: bool,
}

/// Request body for toggling feature flags
#[derive(Debug, Deserialize)]
pub struct ToggleFeatureRequest {
    /// Whether to enable the feature
    pub enabled: bool,
}

/// Request body for evaluating feature flags
#[derive(Debug, Deserialize)]
pub struct EvaluateFeatureRequest {
    /// Tenant ID for evaluation
    pub tenant_id: Option<String>,
    /// User ID for evaluation
    pub user_id: Option<String>,
    /// Request ID for evaluation
    pub request_id: Option<String>,
}

/// Response for configuration entries
#[derive(Debug, Serialize)]
pub struct ConfigEntryResponse {
    pub key: String,
    pub category: String,
    pub value: serde_json::Value,
    pub description: Option<String>,
    pub is_secret: bool,
}

/// Response for feature flags
#[derive(Debug, Serialize)]
pub struct FeatureFlagResponse {
    pub name: String,
    pub enabled: bool,
    pub flag_type: String,
    pub description: Option<String>,
}

/// Response for feature flag evaluation
#[derive(Debug, Serialize)]
pub struct FeatureEvaluationResponse {
    pub name: String,
    pub enabled: bool,
    pub context: serde_json::Value,
}

// =============================================================================
// Configuration Handlers
// =============================================================================

/// List all configuration entries
pub async fn list_config(
    State(state): State<ConfigState>,
    Query(params): Query<ConfigListParams>,
    admin: AdminAuth,
) -> Result<impl IntoResponse, ApiError> {
    tracing::info!(
        admin_user = %admin.username,
        category = ?params.category,
        "Listing configuration"
    );

    let config = state.config_manager.config().await;
    let mut entries = Vec::new();

    // Get categories to list
    let categories: Vec<ConfigCategory> = if let Some(cat_str) = params.category {
        match ConfigCategory::from_str(&cat_str) {
            Some(cat) => vec![cat],
            None => {
                return Err(ApiError::bad_request(format!(
                    "Unknown category: {}",
                    cat_str
                )));
            }
        }
    } else {
        ConfigCategory::all().to_vec()
    };

    for category in categories {
        if let Some(cat_value) = config.get_category(&category.to_string()) {
            if let Some(obj) = cat_value.as_object() {
                for (key, value) in obj {
                    entries.push(ConfigEntryResponse {
                        key: key.clone(),
                        category: category.to_string(),
                        value: if octofhir_config::secrets::is_secret_key(key)
                            && !params.include_secrets
                        {
                            serde_json::json!("<secret>")
                        } else {
                            value.clone()
                        },
                        description: None,
                        is_secret: octofhir_config::secrets::is_secret_key(key),
                    });
                }
            }
        }
    }

    Ok(Json(entries))
}

/// Get configuration for a specific category
pub async fn get_category_config(
    State(state): State<ConfigState>,
    Path(category): Path<String>,
    admin: AdminAuth,
) -> Result<impl IntoResponse, ApiError> {
    tracing::info!(
        admin_user = %admin.username,
        category = %category,
        "Getting category configuration"
    );

    let cat = ConfigCategory::from_str(&category)
        .ok_or_else(|| ApiError::bad_request(format!("Unknown category: {}", category)))?;

    let config = state.config_manager.config().await;

    match config.get_category(&cat.to_string()) {
        Some(value) => Ok(Json(value.clone())),
        None => Err(ApiError::not_found(format!(
            "Configuration category not found: {}",
            category
        ))),
    }
}

/// Get a specific configuration value
pub async fn get_config_value(
    State(state): State<ConfigState>,
    Path((category, key)): Path<(String, String)>,
    admin: AdminAuth,
) -> Result<impl IntoResponse, ApiError> {
    tracing::info!(
        admin_user = %admin.username,
        category = %category,
        key = %key,
        "Getting configuration value"
    );

    let cat = ConfigCategory::from_str(&category)
        .ok_or_else(|| ApiError::bad_request(format!("Unknown category: {}", category)))?;

    match state.config_manager.get_stored_config(cat, &key).await {
        Ok(Some(value)) => Ok(Json(ConfigEntryResponse {
            key: key.clone(),
            category,
            value,
            description: None,
            is_secret: octofhir_config::secrets::is_secret_key(&key),
        })),
        Ok(None) => Err(ApiError::not_found(format!(
            "Configuration not found: {}.{}",
            category, key
        ))),
        Err(e) => Err(ApiError::internal(format!(
            "Failed to get configuration: {}",
            e
        ))),
    }
}

/// Set a configuration value
pub async fn set_config_value(
    State(state): State<ConfigState>,
    Path((category, key)): Path<(String, String)>,
    admin: AdminAuth,
    Json(request): Json<SetConfigRequest>,
) -> Result<impl IntoResponse, ApiError> {
    tracing::info!(
        admin_user = %admin.username,
        category = %category,
        key = %key,
        "Setting configuration value"
    );

    let cat = ConfigCategory::from_str(&category)
        .ok_or_else(|| ApiError::bad_request(format!("Unknown category: {}", category)))?;

    state
        .config_manager
        .set_config(
            cat,
            &key,
            request.value.clone(),
            request.description.as_deref(),
            request.is_secret,
            Some(&admin.username),
        )
        .await
        .map_err(|e| ApiError::internal(format!("Failed to set configuration: {}", e)))?;

    Ok((
        StatusCode::OK,
        Json(ConfigEntryResponse {
            key,
            category,
            value: if request.is_secret {
                serde_json::json!("<secret>")
            } else {
                request.value
            },
            description: request.description,
            is_secret: request.is_secret,
        }),
    ))
}

/// Delete (reset) a configuration value
pub async fn delete_config_value(
    State(state): State<ConfigState>,
    Path((category, key)): Path<(String, String)>,
    admin: AdminAuth,
) -> Result<impl IntoResponse, ApiError> {
    tracing::info!(
        admin_user = %admin.username,
        category = %category,
        key = %key,
        "Deleting configuration value"
    );

    let cat = ConfigCategory::from_str(&category)
        .ok_or_else(|| ApiError::bad_request(format!("Unknown category: {}", category)))?;

    let deleted = state
        .config_manager
        .delete_config(cat, &key)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to delete configuration: {}", e)))?;

    if deleted {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::not_found(format!(
            "Configuration not found: {}.{}",
            category, key
        )))
    }
}

// =============================================================================
// Feature Flag Handlers
// =============================================================================

/// List all feature flags
pub async fn list_features(
    State(state): State<ConfigState>,
    admin: AdminAuth,
) -> Result<impl IntoResponse, ApiError> {
    tracing::info!(
        admin_user = %admin.username,
        "Listing feature flags"
    );

    let flags = state.config_manager.feature_flags().await;

    let response: Vec<FeatureFlagResponse> = flags
        .list()
        .map(|flag| FeatureFlagResponse {
            name: flag.name.clone(),
            enabled: flag.enabled,
            flag_type: format!("{:?}", flag.flag_type),
            description: flag.description.clone(),
        })
        .collect();

    Ok(Json(response))
}

/// Get a specific feature flag
pub async fn get_feature(
    State(state): State<ConfigState>,
    Path(name): Path<String>,
    admin: AdminAuth,
) -> Result<impl IntoResponse, ApiError> {
    tracing::info!(
        admin_user = %admin.username,
        feature = %name,
        "Getting feature flag"
    );

    let flags = state.config_manager.feature_flags().await;

    match flags.get(&name) {
        Some(flag) => Ok(Json(FeatureFlagResponse {
            name: flag.name.clone(),
            enabled: flag.enabled,
            flag_type: format!("{:?}", flag.flag_type),
            description: flag.description.clone(),
        })),
        None => Err(ApiError::not_found(format!(
            "Feature flag not found: {}",
            name
        ))),
    }
}

/// Toggle a feature flag
pub async fn toggle_feature(
    State(state): State<ConfigState>,
    Path(name): Path<String>,
    admin: AdminAuth,
    Json(request): Json<ToggleFeatureRequest>,
) -> Result<impl IntoResponse, ApiError> {
    tracing::info!(
        admin_user = %admin.username,
        feature = %name,
        enabled = request.enabled,
        "Toggling feature flag"
    );

    state
        .config_manager
        .toggle_feature(&name, request.enabled)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to toggle feature: {}", e)))?;

    // Get the updated flag
    let flags = state.config_manager.feature_flags().await;
    match flags.get(&name) {
        Some(flag) => Ok(Json(FeatureFlagResponse {
            name: flag.name.clone(),
            enabled: flag.enabled,
            flag_type: format!("{:?}", flag.flag_type),
            description: flag.description.clone(),
        })),
        None => {
            // Flag was just created
            Ok(Json(FeatureFlagResponse {
                name,
                enabled: request.enabled,
                flag_type: "Boolean".to_string(),
                description: None,
            }))
        }
    }
}

/// Evaluate a feature flag for a given context
pub async fn evaluate_feature(
    State(state): State<ConfigState>,
    Path(name): Path<String>,
    admin: AdminAuth,
    Json(request): Json<EvaluateFeatureRequest>,
) -> Result<impl IntoResponse, ApiError> {
    tracing::info!(
        admin_user = %admin.username,
        feature = %name,
        "Evaluating feature flag"
    );

    let mut context = FeatureContext::new();
    if let Some(tenant_id) = &request.tenant_id {
        context = context.attribute("tenant_id", tenant_id.clone());
    }
    if let Some(user_id) = &request.user_id {
        context = context.user(user_id.clone());
    }
    if let Some(request_id) = &request.request_id {
        context = context.request(request_id.clone());
    }

    let enabled = state
        .config_manager
        .is_feature_enabled(&name, &context)
        .await;

    Ok(Json(FeatureEvaluationResponse {
        name,
        enabled,
        context: serde_json::json!({
            "tenant_id": request.tenant_id,
            "user_id": request.user_id,
            "request_id": request.request_id,
        }),
    }))
}

/// Reload configuration from all sources
pub async fn reload_config(
    State(state): State<ConfigState>,
    admin: AdminAuth,
) -> Result<impl IntoResponse, ApiError> {
    tracing::info!(
        admin_user = %admin.username,
        "Reloading configuration"
    );

    state
        .config_manager
        .reload()
        .await
        .map_err(|e| ApiError::internal(format!("Failed to reload configuration: {}", e)))?;

    Ok((
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "reloaded",
            "message": "Configuration reloaded from all sources"
        })),
    ))
}
