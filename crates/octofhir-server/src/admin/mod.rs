//! Admin API endpoints.
//!
//! This module provides administrative REST endpoints for managing
//! IdentityProvider and User resources.
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

pub mod identity_provider;
pub mod state;
pub mod user;

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
