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
use octofhir_auth::federation::resources::IdentityProviderResource;
use octofhir_auth::federation::saml::ServiceProvider;
use octofhir_auth::federation::{
    ClientJwksCache, DiscoveryCache, DiscoveryCacheConfig, IdpAuthService, IdpAuthServiceConfig,
    JwksCacheConfig, ProviderJwksCache, ProviderJwksCacheConfig, ProvisioningConfig,
};
use octofhir_auth::http::federated::{
    FederatedState, federated_callback, federated_login, federated_providers, saml_acs,
    saml_metadata,
};
use octofhir_auth::http::introspect::{IntrospectionState, introspect_handler};
use octofhir_auth::http::revoke::{RevocationState, revoke_handler};
use octofhir_auth::http::udap::{UdapState, udap_metadata_handler, udap_register_handler};
use octofhir_auth::lockout::Lockout;
use octofhir_auth::oauth::service::{AuthorizationConfig, AuthorizationService};
use octofhir_auth::oauth::token::TokenRequest;
use octofhir_auth::token::jwt::JwtService;
use octofhir_auth::token::service::TokenConfig;
use octofhir_auth::udap::metadata::Endpoints;
use octofhir_auth::udap::tiered::TieredOAuth;
use octofhir_auth::udap::{Trust, UdapMetadata};
use octofhir_auth::{
    AuthState, AuthorizeState, JwksState, LaunchState, LogoutState, SmartConfigState, TokenState,
    authorize_get, authorize_post, create_launch_handler, jwks_handler, logout_handler,
    oidc_logout_handler, openid_configuration_handler, smart_configuration_handler, token_handler,
    userinfo_handler,
};
use octofhir_auth_postgres::{
    ArcAuthorizeSessionStorage, ArcClientStorage, ArcConsentStorage, ArcJtiStorage,
    ArcLaunchContextStorage, ArcRefreshTokenStorage, ArcRevokedTokenStorage, ArcSessionStorage,
    ArcUserStorage, PostgresSsoSessionStorage,
};
use time::Duration;
use url::Url;

use crate::audit::AuditService;
use crate::server::AppState;

/// OAuth state containing all auth-related storage.
#[derive(Clone)]
pub struct OAuthState {
    pub token_state: TokenState,
    pub revocation_state: RevocationState,
    pub introspection_state: IntrospectionState,
    pub logout_state: LogoutState,
    pub jwks_state: JwksState,
    pub smart_config_state: SmartConfigState,
    pub authorize_state: AuthorizeState,
    pub launch_state: LaunchState,
    pub audit_service: Arc<AuditService>,
    /// Set when UDAP is enabled and its files could be read.
    pub udap_state: Option<UdapState>,
    /// Set when sign-in through external identity providers is allowed.
    pub federated_state: Option<FederatedState>,
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
        let resource_audiences = vec![
            config.base_url(),
            format!("{}/fhir", config.base_url().trim_end_matches('/')),
        ];
        let token_config = TokenConfig::new(config.auth.issuer.clone(), config.base_url())
            .with_audiences(resource_audiences.clone())
            .with_access_token_lifetime(Duration::seconds(access_token_secs))
            .with_refresh_token_lifetime(Duration::seconds(refresh_token_secs));

        // Create user storage for password grant support
        let user_storage = Arc::new(ArcUserStorage::new(db_pool.clone()));

        // Create TokenState with user storage for password grant and cookie config
        let lockout = Arc::new(Lockout::new(config.auth.lockout.clone()));
        let token_state = TokenState::new(
            jwt_service.clone(),
            session_storage.clone(),
            refresh_storage,
            revoked_storage.clone(),
            client_storage.clone(),
            token_config,
        )
        .with_user_storage(user_storage.clone())
        .with_lockout(lockout.clone())
        .with_cookie_config(config.auth.cookie.clone())
        .with_fhir_storage(app_state.storage.clone())
        // private_key_jwt, the client authentication of SMART Backend Services.
        .with_token_endpoint_url(format!(
            "{}/auth/token",
            config.base_url().trim_end_matches('/')
        ))
        .with_jwks_cache(Arc::new(ClientJwksCache::new(JwksCacheConfig::default())))
        .with_jti_storage(Arc::new(ArcJtiStorage::new(db_pool.clone())));

        let udap_state = udap_state(
            config,
            client_storage.clone(),
            Arc::new(ArcJtiStorage::new(db_pool.clone())),
            access_token_secs,
        );
        let token_state = match &udap_state {
            Some(udap) => token_state.with_udap(udap.trust.clone()),
            None => token_state,
        };

        let revocation_state =
            RevocationState::new(token_state.token_service(), client_storage.clone());
        let introspection_state =
            IntrospectionState::new(token_state.token_service(), client_storage.clone());

        // Create PostgresSsoSessionStorage for SSO logout support
        // Uses FHIR storage for AuthSession resources
        let sso_session_storage =
            Arc::new(PostgresSsoSessionStorage::new(app_state.storage.clone()));

        // Create LogoutState for browser-based logout
        // Includes client_storage for OIDC RP-Initiated Logout validation
        // Includes JWT cache invalidation callback to immediately reject revoked tokens
        let jwt_cache = app_state.jwt_cache.clone();
        let logout_state = LogoutState::new(
            jwt_service.clone(),
            revoked_storage,
            config.auth.cookie.clone(),
            sso_session_storage.clone(),
            config.auth.session.clone(),
            client_storage.clone(),
        )
        .with_token_revoked_callback(Arc::new(move |jti: &str| {
            jwt_cache.invalidate_by_jti(jti);
        }));

        // Create JwksState
        let jwks_state = JwksState::new(jwt_service.clone());

        // Create SmartConfigState - parse base URL
        let base_url = Url::parse(&config.base_url()).ok()?;
        let smart_config_state = SmartConfigState::new(config.auth.clone(), base_url);

        // Create LaunchState for EHR launch context
        let launch_storage: Arc<ArcLaunchContextStorage> =
            Arc::new(ArcLaunchContextStorage::new(db_pool));
        let launch_state = LaunchState::new(launch_storage.clone());

        // Create AuthorizationService for authorize endpoint
        let authorization_service = Arc::new(
            AuthorizationService::new(
                client_storage.clone(),
                session_storage.clone(),
                AuthorizationConfig::default().with_audiences(resource_audiences),
            )
            .with_launch_storage(launch_storage.clone()),
        );

        // Create AuthorizeState for the authorize endpoint
        let secure_cookies = config.auth.cookie.secure;

        let authorize_state = AuthorizeState {
            authorization_service,
            authorize_session_storage,
            user_storage,
            client_storage,
            consent_storage,
            session_storage,
            lockout,
            secure_cookies,
            sso_session_storage,
            session_config: config.auth.session.clone(),
            fhir_storage: app_state.storage.clone(),
            identity_providers: app_state.identity_providers.clone(),
            tiered_oauth: tiered_oauth(
                config,
                udap_state.as_ref(),
                app_state.identity_providers.as_ref(),
                app_state.storage.clone(),
            ),
        };

        let federation = &config.auth.federation;
        let federated_state = app_state.identity_providers.clone().map(|service| {
            let provisioning = ProvisioningConfig::new()
                .with_auto_provision(federation.auto_provision_users)
                .with_link_by_email(federation.link_by_email)
                .with_default_roles(federation.default_roles.clone());
            let federated = FederatedState::new(
                service,
                authorize_state.clone(),
                token_state.clone(),
                provisioning,
                crate::bootstrap::DEFAULT_UI_CLIENT_ID,
            );
            match saml_service_provider(config) {
                Some(saml) => federated.with_saml(saml),
                None => federated,
            }
        });

        Some(Self {
            token_state,
            revocation_state,
            introspection_state,
            logout_state,
            jwks_state,
            smart_config_state,
            authorize_state,
            launch_state,
            audit_service: app_state.audit_service.clone(),
            udap_state,
            federated_state,
        })
    }
}

/// The identity providers people can sign in through: the active `IdentityProvider` resources.
/// `None` when the configuration does not allow it.
pub async fn identity_providers(
    config: &crate::config::AppConfig,
    pool: &sqlx_postgres::PgPool,
) -> Option<Arc<IdpAuthService>> {
    let federation = &config.auth.federation;
    if !federation.allow_external_idp {
        return None;
    }
    let base = config.base_url();
    let callback = format!("{}/auth/idp/callback", base.trim_end_matches('/'));
    let callback = match url::Url::parse(&callback) {
        Ok(callback) => callback,
        Err(error) => {
            tracing::error!(%error, "External identity providers stay off: bad base URL");
            return None;
        }
    };
    let settings = IdpAuthServiceConfig::new(callback).with_allow_http(federation.allow_http);
    let discovery = DiscoveryCacheConfig::default()
        .with_request_timeout(settings.request_timeout)
        .with_allow_http(federation.allow_http);
    let keys = ProviderJwksCacheConfig::default()
        .with_default_ttl(federation.jwks_cache_ttl)
        .with_request_timeout(settings.request_timeout)
        .with_allow_http(federation.allow_http);
    let service = IdpAuthService::with_caches(
        settings,
        Arc::new(DiscoveryCache::new(discovery)),
        Arc::new(ProviderJwksCache::new(keys)),
    );
    let rows = octofhir_auth_postgres::IdentityProviderStorage::new(pool)
        .list_active()
        .await;
    match rows {
        Ok(rows) => {
            for row in rows {
                let provider = serde_json::from_value::<IdentityProviderResource>(row.resource)
                    .map_err(|error| error.to_string())
                    .and_then(|resource| resource.to_config().map_err(|error| error.to_string()));
                match provider {
                    Ok(provider) => service.register_provider(provider).await,
                    // One bad resource does not take the others down.
                    Err(error) => {
                        tracing::error!(id = %row.id, %error, "Skipping identity provider")
                    }
                }
            }
        }
        Err(error) => tracing::error!(%error, "Cannot read the identity providers"),
    }
    Some(Arc::new(service))
}

/// UDAP Tiered OAuth, when `[auth.udap.tiered_oauth]` enables it: it needs UDAP itself (this
/// server's certificate signs its registrations and client assertions) and external identity
/// providers (`[auth.federation]`), since the sign-in goes through them.
fn tiered_oauth(
    config: &crate::config::AppConfig,
    udap: Option<&UdapState>,
    providers: Option<&Arc<IdpAuthService>>,
    storage: Arc<dyn octofhir_storage::FhirStorage>,
) -> Option<Arc<TieredOAuth>> {
    let tiered = &config.auth.udap.tiered_oauth;
    if !tiered.enabled {
        return None;
    }
    let (Some(udap), Some(providers)) = (udap, providers) else {
        tracing::error!(
            "UDAP Tiered OAuth stays off: it needs auth.udap and auth.federation enabled"
        );
        return None;
    };
    providers.set_client_assertion(udap.metadata.clone());
    let callback = format!(
        "{}/auth/idp/callback",
        config.base_url().trim_end_matches('/')
    );
    Some(Arc::new(
        TieredOAuth::new(
            udap.metadata.clone(),
            udap.trust.clone(),
            providers.clone(),
            storage,
            callback,
            tiered.clone(),
        )
        .with_allow_http(config.auth.federation.allow_http),
    ))
}

/// The UDAP endpoints. A misconfiguration leaves them off and says why: nothing here may
/// weaken the rest of the server.
fn udap_state(
    config: &crate::config::AppConfig,
    clients: Arc<ArcClientStorage>,
    jti: Arc<ArcJtiStorage>,
    access_token_lifetime: i64,
) -> Option<UdapState> {
    let mut udap = config.auth.udap.clone();
    if !udap.enabled {
        return None;
    }
    // A data holder that supports Tiered OAuth lists `udap` in scopes_supported, and client
    // apps that use it register with it.
    if udap.tiered_oauth.enabled && !udap.scopes.iter().any(|scope| scope == "udap") {
        udap.scopes.push("udap".to_owned());
    }
    let udap = &udap;
    let base = config.base_url();
    let base = base.trim_end_matches('/');
    let registration_url = format!("{base}/auth/udap/register");
    let built = Trust::from_config(udap).and_then(|trust| {
        let endpoints = Endpoints {
            base_url: format!("{base}/fhir"),
            authorization: format!("{base}/auth/authorize"),
            token: format!("{base}/auth/token"),
            registration: registration_url.clone(),
        };
        Ok((trust, UdapMetadata::from_config(udap, endpoints)?))
    });
    match built {
        Ok((trust, metadata)) => Some(UdapState {
            trust: Arc::new(trust),
            metadata: Arc::new(metadata),
            clients,
            jti,
            registration_url,
            scopes: udap.scopes.clone().into(),
            access_token_lifetime,
        }),
        Err(error) => {
            tracing::error!(%error, "UDAP stays off");
            None
        }
    }
}

/// This server as a SAML service provider, signing its requests when it has a certificate.
fn saml_service_provider(config: &crate::config::AppConfig) -> Option<Arc<ServiceProvider>> {
    let federation = &config.auth.federation;
    let base = config.base_url();
    let base = base.trim_end_matches('/');
    let signing = match (&federation.saml_certificate, &federation.saml_private_key) {
        (Some(certificate), Some(key)) => {
            match (
                std::fs::read_to_string(certificate),
                std::fs::read_to_string(key),
            ) {
                (Ok(certificate), Ok(key)) => Some((certificate, key)),
                (Err(error), _) | (_, Err(error)) => {
                    tracing::error!(%error, "SAML stays off: cannot read its certificate or key");
                    return None;
                }
            }
        }
        (None, None) => None,
        _ => {
            tracing::error!("SAML stays off: a certificate needs its key and the other way round");
            return None;
        }
    };
    let built = ServiceProvider::new(
        &format!("{base}/auth/idp/saml/metadata"),
        &format!("{base}/auth/idp/saml/acs"),
        signing,
    );
    match built {
        Ok(sp) => Some(Arc::new(sp)),
        Err(error) => {
            tracing::error!(%error, "SAML stays off");
            None
        }
    }
}

/// Sign-in through external identity providers.
pub fn federated_route(state: FederatedState) -> Router {
    Router::new()
        .route("/auth/idp/providers", get(federated_providers))
        .route("/auth/idp/callback", get(federated_callback))
        .route("/auth/idp/{provider}/login", get(federated_login))
        .route("/auth/idp/saml/acs", post(saml_acs))
        .route("/auth/idp/saml/metadata", get(saml_metadata))
        .with_state(state)
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
        .merge(
            Router::new()
                .route("/auth/revoke", post(revoke_handler))
                .with_state(state.revocation_state),
        )
        .merge(
            Router::new()
                .route("/auth/introspect", post(introspect_handler))
                .with_state(state.introspection_state),
        )
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
        (
            AuditOutcome::SeriousFailure,
            "Authentication failed: invalid credentials",
        )
    } else if status == StatusCode::BAD_REQUEST {
        (
            AuditOutcome::MinorFailure,
            "Authentication failed: invalid request",
        )
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
        .route(
            "/auth/logout",
            get(oidc_logout_handler).post(logout_handler),
        )
        .with_state(state)
}

/// Creates JWKS route.
pub fn jwks_route(state: JwksState) -> Router {
    Router::new()
        .route("/auth/jwks", get(jwks_handler))
        .route("/.well-known/jwks.json", get(jwks_handler))
        .with_state(state)
}

/// Creates SMART configuration discovery route.
pub fn smart_config_route(state: SmartConfigState) -> Router {
    Router::new()
        .route(
            "/.well-known/smart-configuration",
            get(smart_configuration_handler),
        )
        .route(
            "/.well-known/openid-configuration",
            get(openid_configuration_handler),
        )
        .with_state(state)
}

/// Creates the UDAP discovery document and registration endpoint.
pub fn udap_route(state: UdapState) -> Router {
    Router::new()
        .route("/fhir/.well-known/udap", get(udap_metadata_handler))
        .route("/auth/udap/register", post(udap_register_handler))
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

/// Creates SMART EHR launch route.
///
/// Allows EHR systems to create a launch context before redirecting
/// to the authorization endpoint with a `launch` parameter.
pub fn launch_route(state: LaunchState) -> Router {
    Router::new()
        .route("/auth/launch", post(create_launch_handler))
        .with_state(state)
}
