//! Admin API endpoints.
//!
//! This module provides administrative REST endpoints for managing
//! IdentityProvider, User, and Configuration resources.
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
//! - `POST /User/:id/$link-identity` - Link an external identity
//! - `POST /User/:id/$unlink-identity` - Unlink an external identity
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

pub mod configuration;
pub mod identity_provider;
pub mod state;
pub mod user;

pub use configuration::{
    delete_config_value, evaluate_feature, get_category_config, get_config_value, get_feature,
    list_config, list_features, reload_config, set_config_value, toggle_feature, ConfigState,
};
pub use identity_provider::{
    create_identity_provider, delete_identity_provider, read_identity_provider,
    search_identity_providers, update_identity_provider,
};
pub use state::{AdminState, CombinedAdminState};
pub use user::{
    create_user, delete_user, link_identity, read_user, search_users, unlink_identity, update_user,
};

use axum::Router;
use axum::extract::FromRef;
use axum::routing::{get, post};

use octofhir_auth::middleware::AuthState;

// =============================================================================
// Routes
// =============================================================================

/// Creates the admin routes for IdentityProvider and User management.
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
        .route("/User/{id}/$link-identity", post(link_identity))
        .route("/User/{id}/$unlink-identity", post(unlink_identity))
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
        .route(
            "/features/{name}",
            get(get_feature).put(toggle_feature),
        )
        .route("/features/{name}/$evaluate", post(evaluate_feature))
}
