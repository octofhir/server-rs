//! OAuth 2.0 endpoint routes and handlers.
//!
//! This module sets up the OAuth 2.0 routes for the FHIR server:
//!
//! - `/auth/token` - Token endpoint (RFC 6749)
//! - `/auth/authorize` - Authorization endpoint
//! - `/auth/revoke` - Token revocation (RFC 7009)
//! - `/auth/introspect` - Token introspection (RFC 7662)
//! - `/.well-known/smart-configuration` - SMART on FHIR discovery
//! - `/auth/jwks` - JWKS endpoint (RFC 7517)
//! - `/auth/userinfo` - OpenID Connect userinfo

use std::sync::Arc;

use axum::{Router, routing::{get, post}};
use octofhir_auth::{
    TokenState, token_handler,
    jwks_handler, JwksState,
    smart_configuration_handler, SmartConfigState,
};
use octofhir_auth::token::service::TokenConfig;
use octofhir_auth::token::jwt::JwtService;
use octofhir_auth_postgres::{
    ArcClientStorage, ArcRevokedTokenStorage, ArcRefreshTokenStorage, ArcSessionStorage,
    ArcUserStorage,
};
use time::Duration;
use url::Url;

use crate::server::AppState;

/// OAuth state containing all auth-related storage.
#[derive(Clone)]
pub struct OAuthState {
    pub token_state: TokenState,
    pub jwks_state: JwksState,
    pub smart_config_state: SmartConfigState,
}

impl OAuthState {
    /// Creates OAuth state from application state.
    pub fn from_app_state(
        app_state: &AppState,
        jwt_service: Arc<JwtService>,
    ) -> Option<Self> {
        // Get config
        let config = &app_state.config;

        if !config.auth.enabled {
            return None;
        }

        let db_pool = app_state.db_pool.clone();

        // Create storage adapters
        let client_storage = Arc::new(ArcClientStorage::new(db_pool.clone()));
        let session_storage = Arc::new(ArcSessionStorage::new(db_pool.clone()));
        let refresh_storage = Arc::new(ArcRefreshTokenStorage::new(db_pool.clone()));
        let revoked_storage = Arc::new(ArcRevokedTokenStorage::new(db_pool.clone()));

        // Convert std::time::Duration to time::Duration for token lifetimes
        let access_token_secs = config.auth.oauth.access_token_lifetime.as_secs() as i64;
        let refresh_token_secs = config.auth.oauth.refresh_token_lifetime.as_secs() as i64;

        // Create token config
        let token_config = TokenConfig::new(
            config.auth.issuer.clone(),
            config.base_url(),
        )
        .with_access_token_lifetime(Duration::seconds(access_token_secs))
        .with_refresh_token_lifetime(Duration::seconds(refresh_token_secs));

        // Create user storage for password grant support
        let user_storage = Arc::new(ArcUserStorage::new(db_pool.clone()));

        // Create TokenState with user storage for password grant
        let token_state = TokenState::new(
            jwt_service.clone(),
            session_storage,
            refresh_storage,
            revoked_storage,
            client_storage.clone(),
            token_config,
        ).with_user_storage(user_storage);

        // Create JwksState
        let jwks_state = JwksState::new(jwt_service.clone());

        // Create SmartConfigState - parse base URL
        let base_url = Url::parse(&config.base_url()).ok()?;
        let smart_config_state = SmartConfigState::new(
            config.auth.clone(),
            base_url,
        );

        Some(Self {
            token_state,
            jwks_state,
            smart_config_state,
        })
    }
}

/// Creates OAuth routes that are NOT subject to FHIR content negotiation.
///
/// These routes use application/x-www-form-urlencoded for token requests
/// and application/json for responses.
pub fn oauth_routes(state: OAuthState) -> Router {
    Router::new()
        // Token endpoint - accepts x-www-form-urlencoded, returns JSON
        .route("/auth/token", post(token_handler))
        .with_state(state.token_state)
}

/// Creates JWKS route.
pub fn jwks_route(state: JwksState) -> Router {
    Router::new()
        .route("/auth/jwks", get(jwks_handler))
        .with_state(state)
}

/// Creates SMART configuration discovery route.
pub fn smart_config_route(state: SmartConfigState) -> Router {
    Router::new()
        .route("/.well-known/smart-configuration", get(smart_configuration_handler))
        .with_state(state)
}
