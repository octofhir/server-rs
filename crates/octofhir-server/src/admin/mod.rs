//! Admin API endpoints.
//!
//! This module provides administrative REST endpoints for managing
//! IdentityProvider, User, Role, and Configuration resources.
//!
//! # Endpoints
//!
//! ## IdentityProvider
//!
//! - `GET /IdentityProvider` - Search/list identity providers
//! - `GET /IdentityProvider/:id` - Read a single provider
//! - `POST /IdentityProvider` - Create a new provider
//! - `PUT /IdentityProvider/:id` - Update a provider
//! - `DELETE /IdentityProvider/:id` - Delete a provider
//!
//! ## User
//!
//! - `GET /User` - Search/list users
//! - `GET /User/:id` - Read a single user
//! - `POST /User` - Create a new user
//! - `PUT /User/:id` - Update a user
//! - `DELETE /User/:id` - Delete a user
//! - `GET /User/:id/sessions` - Get user sessions
//! - `DELETE /User/:id/sessions` - Revoke all user sessions
//! - `DELETE /User/:id/sessions/:sessionId` - Revoke a specific session
//! - `POST /User/:id/$reset-password` - Reset user password
//! - `POST /User/$bulk` - Bulk update users
//!
//! ## Role
//!
//! - `GET /Role` - Search/list roles
//! - `GET /Role/:id` - Read a single role
//! - `POST /Role` - Create a new role
//! - `PUT /Role/:id` - Update a role
//! - `DELETE /Role/:id` - Delete a role
//! - `GET /Role/$permissions` - Get available permissions
//!
//! ## Configuration
//!
//! - `GET /config` - List all configuration
//! - `GET /config/:category` - Get configuration for a category
//! - `GET /config/:category/:key` - Get a specific configuration value
//! - `PUT /config/:category/:key` - Set a configuration value
//! - `DELETE /config/:category/:key` - Delete a configuration value
//! - `POST /config/$reload` - Reload configuration from all sources
//!
//! ## Feature Flags
//!
//! - `GET /features` - List all feature flags
//! - `GET /features/:name` - Get a specific feature flag
//! - `PUT /features/:name` - Toggle a feature flag
//! - `POST /features/:name/$evaluate` - Evaluate a feature flag
//!
//! ## Policies
//!
//! - `POST /policies/$reload` - Trigger policy cache reload
//! - `GET /policies/status` - Get policy cache status and statistics
//!
//! ## OAuth Clients
//!
//! - `POST /clients/:id/regenerate-secret` - Regenerate a client's secret
//!
//! ## Audit Analytics
//!
//! - `GET /audit/$analytics` - Get audit event analytics and aggregations

pub mod audit;
pub mod client;
pub mod configuration;
pub mod identity_provider;
pub mod policy;
pub mod role;
pub mod state;
pub mod user;

pub use audit::get_audit_analytics;
pub use client::regenerate_client_secret;
pub use configuration::{
    ConfigState, delete_config_value, evaluate_feature, get_category_config, get_config_value,
    get_feature, list_config, list_features, reload_config, set_config_value, toggle_feature,
};
pub use identity_provider::{
    create_identity_provider, delete_identity_provider, read_identity_provider,
    search_identity_providers, update_identity_provider,
};
pub use policy::{PolicyState, policy_status, reload_policies};
pub use role::{create_role, delete_role, list_permissions, read_role, search_roles, update_role};
pub use state::{AdminState, CombinedAdminState};
pub use user::{
    bulk_update_users, create_user, delete_user, get_user_sessions, read_user, reset_user_password,
    revoke_all_user_sessions, revoke_user_session, search_users, update_user,
};

use axum::Router;
use axum::extract::FromRef;
use axum::routing::{delete, get, post};

use crate::server::AppState;
use octofhir_auth::middleware::AuthState;

// =============================================================================
// Routes
// =============================================================================

/// Creates the admin routes for IdentityProvider, User, and Role management.
///
/// All routes require admin authentication via the `AdminAuth` extractor.
///
/// # Type Parameters
///
/// - `S`: Application state that provides `AuthState` and `AdminState` via `FromRef`.
pub fn admin_routes<S>() -> Router<S>
where
    S: Clone + Send + Sync + 'static,
    AuthState: FromRef<S>,
    AdminState: FromRef<S>,
{
    Router::new()
        // IdentityProvider endpoints
        .route(
            "/IdentityProvider",
            get(search_identity_providers).post(create_identity_provider),
        )
        .route(
            "/IdentityProvider/{id}",
            get(read_identity_provider)
                .put(update_identity_provider)
                .delete(delete_identity_provider),
        )
        // User endpoints
        .route("/User", get(search_users).post(create_user))
        .route(
            "/User/{id}",
            get(read_user).put(update_user).delete(delete_user),
        )
        .route(
            "/User/{id}/sessions",
            get(get_user_sessions).delete(revoke_all_user_sessions),
        )
        .route(
            "/User/{user_id}/sessions/{session_id}",
            delete(revoke_user_session),
        )
        .route("/User/{id}/$reset-password", post(reset_user_password))
        .route("/User/$bulk", post(bulk_update_users))
        // Role endpoints
        .route("/Role", get(search_roles).post(create_role))
        .route(
            "/Role/{id}",
            get(read_role).put(update_role).delete(delete_role),
        )
        .route("/Role/$permissions", get(list_permissions))
        // OAuth Client endpoints
        .route(
            "/clients/{id}/regenerate-secret",
            post(regenerate_client_secret),
        )
}

/// Creates the audit analytics routes.
///
/// These routes require admin authentication.
///
/// # Type Parameters
///
/// - `S`: Application state that provides `AuthState` and `AppState` via `FromRef`.
pub fn audit_routes<S>() -> Router<S>
where
    S: Clone + Send + Sync + 'static,
    AuthState: FromRef<S>,
    AppState: FromRef<S>,
{
    Router::new().route("/audit/$analytics", get(get_audit_analytics))
}

/// Creates the configuration management routes.
///
/// These routes require admin authentication and ConfigState via `FromRef`.
///
/// # Type Parameters
///
/// - `S`: Application state that provides `AuthState` and `ConfigState` via `FromRef`.
pub fn config_routes<S>() -> Router<S>
where
    S: Clone + Send + Sync + 'static,
    AuthState: FromRef<S>,
    ConfigState: FromRef<S>,
{
    Router::new()
        // Configuration endpoints
        .route("/config", get(list_config))
        .route("/config/$reload", post(reload_config))
        .route("/config/{category}", get(get_category_config))
        .route(
            "/config/{category}/{key}",
            get(get_config_value)
                .put(set_config_value)
                .delete(delete_config_value),
        )
        // Feature flag endpoints
        .route("/features", get(list_features))
        .route("/features/{name}", get(get_feature).put(toggle_feature))
        .route("/features/{name}/$evaluate", post(evaluate_feature))
}

/// Creates the policy management routes.
///
/// These routes require admin authentication and PolicyState via `FromRef`.
///
/// # Type Parameters
///
/// - `S`: Application state that provides `AuthState` and `PolicyState` via `FromRef`.
pub fn policy_routes<S>() -> Router<S>
where
    S: Clone + Send + Sync + 'static,
    AuthState: FromRef<S>,
    PolicyState: FromRef<S>,
{
    Router::new()
        .route("/policies/$reload", post(reload_policies))
        .route("/policies/status", get(policy_status))
}
