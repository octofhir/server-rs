//! OAuth 2.0 endpoint routes and handlers.
//!
//! This module sets up the OAuth 2.0 routes for the FHIR server:
//!
//! - `/auth/token` - Token endpoint (RFC 6749)
//! - `/auth/logout` - Logout endpoint (revokes token, clears cookie)
//! - `/auth/authorize` - Authorization endpoint
//! - `/auth/revoke` - Token revocation (RFC 7009)
//! - `/auth/introspect` - Token introspection (RFC 7662)
//! - `/.well-known/smart-configuration` - SMART on FHIR discovery
//! - `/auth/jwks` - JWKS endpoint (RFC 7517)
//! - `/auth/userinfo` - OpenID Connect userinfo

use std::sync::Arc;

use axum::{
    Form, Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::Response,
    routing::{get, post},
};
use octofhir_auth::oauth::service::{AuthorizationConfig, AuthorizationService};
use octofhir_auth::oauth::token::TokenRequest;
use octofhir_auth::token::jwt::JwtService;
use octofhir_auth::token::service::TokenConfig;
use octofhir_auth::{
    AuthState, AuthorizeState, JwksState, LogoutState, SmartConfigState, TokenState,
    authorize_get, authorize_post, jwks_handler, logout_handler, oidc_logout_handler,
    smart_configuration_handler, token_handler, userinfo_handler,
};
use octofhir_auth_postgres::{
    ArcAuthorizeSessionStorage, ArcClientStorage, ArcConsentStorage, ArcRefreshTokenStorage,
    ArcRevokedTokenStorage, ArcSessionStorage, ArcUserStorage,
    PostgresSsoSessionStorage,
};
use time::Duration;
use url::Url;

use crate::audit::AuditService;
use crate::server::AppState;

/// OAuth state containing all auth-related storage.
#[derive(Clone)]
pub struct OAuthState {
    pub token_state: TokenState,
    pub logout_state: LogoutState,
    pub jwks_state: JwksState,
    pub smart_config_state: SmartConfigState,
    pub authorize_state: AuthorizeState,
    pub audit_service: Arc<AuditService>,
}

/// Combined state for auditing token handler.
#[derive(Clone)]
pub struct AuditingTokenState {
    pub token_state: TokenState,
    pub audit_service: Arc<AuditService>,
}

impl OAuthState {
    /// Creates OAuth state from application state.
    pub fn from_app_state(app_state: &AppState, jwt_service: Arc<JwtService>) -> Option<Self> {
        // Get config
        let config = &app_state.config;
        let db_pool = app_state.db_pool.clone();

        // Create storage adapters
        let client_storage = Arc::new(ArcClientStorage::new(db_pool.clone()));
        let session_storage = Arc::new(ArcSessionStorage::new(db_pool.clone()));
        let refresh_storage = Arc::new(ArcRefreshTokenStorage::new(db_pool.clone()));
        let revoked_storage = Arc::new(ArcRevokedTokenStorage::new(db_pool.clone()));
        let authorize_session_storage = Arc::new(ArcAuthorizeSessionStorage::new(db_pool.clone()));
        let consent_storage = Arc::new(ArcConsentStorage::new(db_pool.clone()));

        // Convert std::time::Duration to time::Duration for token lifetimes
        let access_token_secs = config.auth.oauth.access_token_lifetime.as_secs() as i64;
        let refresh_token_secs = config.auth.oauth.refresh_token_lifetime.as_secs() as i64;

        // Create token config
        let token_config = TokenConfig::new(config.auth.issuer.clone(), config.base_url())
            .with_access_token_lifetime(Duration::seconds(access_token_secs))
            .with_refresh_token_lifetime(Duration::seconds(refresh_token_secs));

        // Create user storage for password grant support
        let user_storage = Arc::new(ArcUserStorage::new(db_pool.clone()));

        // Create TokenState with user storage for password grant and cookie config
        let token_state = TokenState::new(
            jwt_service.clone(),
            session_storage.clone(),
            refresh_storage,
            revoked_storage.clone(),
            client_storage.clone(),
            token_config,
        )
        .with_user_storage(user_storage.clone())
        .with_cookie_config(config.auth.cookie.clone())
        .with_fhir_storage(app_state.storage.clone());

        // Create PostgresSsoSessionStorage for SSO logout support
        // Uses FHIR storage for AuthSession resources
        let sso_session_storage = Arc::new(PostgresSsoSessionStorage::new(
            app_state.storage.clone(),
        ));

        // Create LogoutState for browser-based logout
        // Includes client_storage for OIDC RP-Initiated Logout validation
        let logout_state = LogoutState::new(
            jwt_service.clone(),
            revoked_storage,
            config.auth.cookie.clone(),
            sso_session_storage.clone(),
            config.auth.session.clone(),
            client_storage.clone(),
        );

        // Create JwksState
        let jwks_state = JwksState::new(jwt_service.clone());

        // Create SmartConfigState - parse base URL
        let base_url = Url::parse(&config.base_url()).ok()?;
        let smart_config_state = SmartConfigState::new(config.auth.clone(), base_url);

        // Create AuthorizationService for authorize endpoint
        let authorization_service = Arc::new(AuthorizationService::new(
            client_storage.clone(),
            session_storage.clone(),
            AuthorizationConfig::default(),
        ));

        // Create AuthorizeState for the authorize endpoint
        let secure_cookies = config.auth.cookie.secure;
        let authorize_state = AuthorizeState {
            authorization_service,
            authorize_session_storage,
            user_storage,
            client_storage,
            consent_storage,
            session_storage,
            secure_cookies,
            sso_session_storage,
            session_config: config.auth.session.clone(),
            fhir_storage: app_state.storage.clone(),
        };

        Some(Self {
            token_state,
            logout_state,
            jwks_state,
            smart_config_state,
            authorize_state,
            audit_service: app_state.audit_service.clone(),
        })
    }
}

/// Creates OAuth routes that are NOT subject to FHIR content negotiation.
///
/// These routes use application/x-www-form-urlencoded for token requests
/// and application/json for responses.
pub fn oauth_routes(state: OAuthState) -> Router {
    let auditing_state = AuditingTokenState {
        token_state: state.token_state.clone(),
        audit_service: state.audit_service.clone(),
    };

    Router::new()
        // Token endpoint with audit logging - accepts x-www-form-urlencoded, returns JSON
        .route("/auth/token", post(auditing_token_handler))
        .with_state(auditing_state)
        .merge(logout_route(state.logout_state))
}

/// Token handler with audit logging.
///
/// Wraps the standard token handler and logs authentication events as FHIR AuditEvents.
async fn auditing_token_handler(
    State(state): State<AuditingTokenState>,
    headers: HeaderMap,
    form: Form<TokenRequest>,
) -> Response {
    use crate::audit::{AuditAction, AuditOutcome, AuditSource};
    use std::net::IpAddr;

    // Extract info for audit logging before consuming the form
    let grant_type = form.grant_type.clone();
    let client_id = form.client_id.clone();
    let username = form.username.clone();

    // Extract source IP from headers
    let source_ip: Option<IpAddr> = headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.split(',').next().unwrap_or(s).trim().parse().ok())
        .or_else(|| {
            headers
                .get("x-real-ip")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse().ok())
        });

    let user_agent = headers
        .get("user-agent")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    // Extract request ID from headers
    let request_id = headers
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    // Call the actual token handler
    let response = token_handler(State(state.token_state.clone()), headers, form).await;

    // Check response status for audit outcome
    let status = response.status();
    let (audit_outcome, outcome_desc) = if status.is_success() {
        (AuditOutcome::Success, "Authentication successful")
    } else if status == StatusCode::UNAUTHORIZED {
        (AuditOutcome::SeriousFailure, "Authentication failed: invalid credentials")
    } else if status == StatusCode::BAD_REQUEST {
        (AuditOutcome::MinorFailure, "Authentication failed: invalid request")
    } else {
        (AuditOutcome::SeriousFailure, "Authentication failed")
    };

    // Determine the action based on grant type and outcome
    let action = match (grant_type.as_str(), &audit_outcome) {
        ("password", AuditOutcome::Success) => AuditAction::UserLogin,
        ("password", _) => AuditAction::UserLoginFailed,
        ("client_credentials", _) => AuditAction::ClientAuth,
        _ => AuditAction::ClientAuth, // authorization_code, refresh_token, etc.
    };

    // Build audit source
    let source = AuditSource {
        ip_address: source_ip,
        user_agent,
        site: Some("OctoFHIR".to_string()),
    };

    // Log the audit event asynchronously (fire and forget)
    let audit_service = state.audit_service.clone();
    let username_clone = username.clone();
    let client_id_clone = client_id.clone();

    tokio::spawn(async move {
        if let Err(e) = audit_service
            .log_auth_event(
                action,
                audit_outcome,
                Some(outcome_desc),
                None, // user_id (we don't have it from the token request)
                username_clone.as_deref(),
                client_id_clone.as_deref(),
                &source,
                request_id.as_deref(),
                None, // session_id (not available at token creation time)
            )
            .await
        {
            tracing::warn!(error = %e, "Failed to create auth audit event");
        } else {
            tracing::debug!(
                grant_type = %grant_type,
                client_id = ?client_id,
                ?action,
                "Auth audit event created"
            );
        }
    });

    response
}

/// Creates logout routes for browser-based authentication.
///
/// Supports both:
/// - POST: API-style logout with JSON response
/// - GET: OIDC RP-Initiated Logout 1.0 with redirect support
///
/// Per OpenID Connect RP-Initiated Logout 1.0, the logout endpoint
/// MUST support both GET and POST methods.
pub fn logout_route(state: LogoutState) -> Router {
    Router::new()
        .route("/auth/logout", get(oidc_logout_handler).post(logout_handler))
        .with_state(state)
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
        .route(
            "/.well-known/smart-configuration",
            get(smart_configuration_handler),
        )
        .with_state(state)
}

/// Creates userinfo route for OpenID Connect.
pub fn userinfo_route(state: AuthState) -> Router {
    Router::new()
        .route("/auth/userinfo", get(userinfo_handler))
        .with_state(state)
}

/// Creates authorization endpoint route for OAuth 2.0 authorization code flow.
///
/// This endpoint handles the interactive authorization flow where users
/// authenticate and consent to grant access to clients.
pub fn authorize_route(state: AuthorizeState) -> Router {
    Router::new()
        .route("/auth/authorize", get(authorize_get).post(authorize_post))
        .with_state(state)
}
