//! OAuth 2.0 authorization endpoint handlers.
//!
//! Implements the authorization endpoint per RFC 6749 with server-rendered
//! HTML forms for login and consent.
//!
//! # Flow
//!
//! ```text
//! GET /oauth/authorize?client_id=...&redirect_uri=...
//!     ├─► Invalid client/redirect_uri → Render error page (no redirect)
//!     ├─► No session → Render login form
//!     └─► Authenticated + has consent → Issue code immediately
//!         └─► No consent → Render consent form
//!
//! POST /oauth/authorize (form data)
//!     ├─► action=login → Authenticate user
//!     │   ├─► Success + has consent → Issue code
//!     │   ├─► Success + no consent → Render consent form
//!     │   └─► Failure → Re-render login with error
//!     └─► action=authorize/deny
//!         ├─► deny → Redirect with error=access_denied
//!         └─► authorize → Save consent → Issue code → Redirect
//! ```

use std::sync::Arc;

use axum::Form;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum_extra::extract::CookieJar;
use cookie::{Cookie, SameSite};
use serde::Deserialize;
use serde_json::json;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::config::SessionConfig;
use crate::device::{extract_ip_address, extract_user_agent, generate_device_name};
use crate::oauth::authorize::{
    AuthorizationError, AuthorizationErrorCode, AuthorizationRequest, AuthorizationResponse,
};
use crate::oauth::authorize_session::AuthorizeSession;
use crate::oauth::service::AuthorizationService;
use crate::storage::{
    AuthorizeSessionStorage, ClientStorage, ConsentStorage, SessionStorage, SsoSessionStorage,
    UserStorage,
};

use super::authorize_templates::{render_consent_form, render_error_page, render_login_form};

/// Session cookie name.
const SESSION_COOKIE_NAME: &str = "oauth_session";

/// State for the authorize handler.
#[derive(Clone)]
pub struct AuthorizeState {
    /// Authorization service for creating codes.
    pub authorization_service: Arc<AuthorizationService>,
    /// Authorize session storage.
    pub authorize_session_storage: Arc<dyn AuthorizeSessionStorage>,
    /// User storage for authentication.
    pub user_storage: Arc<dyn UserStorage>,
    /// Client storage for validation.
    pub client_storage: Arc<dyn ClientStorage>,
    /// Consent storage for checking/saving consents.
    pub consent_storage: Arc<dyn ConsentStorage>,
    /// Session storage for authorization codes.
    pub session_storage: Arc<dyn SessionStorage>,
    /// SSO session storage for persistent authentication.
    pub sso_session_storage: Arc<dyn SsoSessionStorage>,
    /// FHIR storage for accessing AuthSession resources.
    pub fhir_storage: Arc<dyn octofhir_storage::FhirStorage>,
    /// Session configuration for SSO cookie settings.
    pub session_config: SessionConfig,
    /// Whether to use secure cookies (true in production).
    pub secure_cookies: bool,
}

/// Form data for authorize POST.
#[derive(Debug, Deserialize)]
pub struct AuthorizeFormData {
    /// Action: "login", "authorize", or "deny".
    pub action: String,
    /// Session ID from hidden form field.
    pub session_id: String,
    /// Username (for login action).
    #[serde(default)]
    pub username: Option<String>,
    /// Password (for login action).
    #[serde(default)]
    pub password: Option<String>,
}

/// GET /oauth/authorize handler.
///
/// Validates the authorization request and renders the appropriate form.
pub async fn authorize_get(
    State(state): State<AuthorizeState>,
    Query(params): Query<AuthorizationRequest>,
    jar: CookieJar,
) -> Response {
    // Validate client and redirect_uri
    let client = match state
        .client_storage
        .find_by_client_id(&params.client_id)
        .await
    {
        Ok(Some(c)) => c,
        Ok(None) => {
            return (
                StatusCode::BAD_REQUEST,
                Html(render_error_page(
                    "invalid_client",
                    &format!("Unknown client: {}", params.client_id),
                )),
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!("Failed to lookup client: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Html(render_error_page(
                    "server_error",
                    "Failed to validate client",
                )),
            )
                .into_response();
        }
    };

    // Validate redirect_uri
    if !client.redirect_uris.contains(&params.redirect_uri) {
        return (
            StatusCode::BAD_REQUEST,
            Html(render_error_page(
                "invalid_redirect_uri",
                "The redirect_uri does not match any registered for this client",
            )),
        )
            .into_response();
    }

    // Validate response_type
    if params.response_type != "code" {
        return redirect_with_error(
            &params.redirect_uri,
            AuthorizationErrorCode::UnsupportedResponseType,
            "Only response_type=code is supported",
            &params.state,
        );
    }

    // Validate PKCE based on client type (RFC 8252, RFC 9207)
    if !client.confidential {
        // Public client: PKCE is REQUIRED (RFC 8252)
        if params.code_challenge.is_none() || params.code_challenge_method.is_none() {
            return redirect_with_error(
                &params.redirect_uri,
                AuthorizationErrorCode::InvalidRequest,
                "PKCE (code_challenge and code_challenge_method) is required for public clients",
                &params.state,
            );
        }
    } else {
        // Confidential client: PKCE is RECOMMENDED but optional (RFC 9207)
        if params.code_challenge.is_none() && params.code_challenge_method.is_none() {
            tracing::warn!(
                client_id = %params.client_id,
                "Confidential client is not using PKCE (recommended per RFC 9207)"
            );
        }
    }

    // If either PKCE parameter is provided, both must be provided
    if params.code_challenge.is_some() != params.code_challenge_method.is_some() {
        return redirect_with_error(
            &params.redirect_uri,
            AuthorizationErrorCode::InvalidRequest,
            "Both code_challenge and code_challenge_method must be provided together",
            &params.state,
        );
    }

    // Validate PKCE method if provided
    if let Some(method) = &params.code_challenge_method
        && method != "S256"
    {
        return redirect_with_error(
            &params.redirect_uri,
            AuthorizationErrorCode::InvalidRequest,
            "Only S256 code_challenge_method is supported",
            &params.state,
        );
    }

    // Check for SSO session first (persistent login across OAuth flows)
    if let Some(sso_cookie) = jar.get(&state.session_config.cookie_name)
        && let Ok(Some(sso_resource_id)) = state
            .sso_session_storage
            .find_session_by_token(sso_cookie.value())
            .await
        && let Ok(Some(sso_session)) = state
            .fhir_storage
            .read("AuthSession", &sso_resource_id)
            .await
    {
        let session_data = sso_session.resource;

        // Check if session is still active (not revoked/expired)
        // Extract user_id from subject reference
        if session_data.get("status").and_then(|v| v.as_str()) == Some("active")
            && let Some(subject) = session_data
                .get("subject")
                .and_then(|s| s.get("reference"))
                .and_then(|r| r.as_str())
            && let Some(user_id) = subject.strip_prefix("User/")
        {
            // Update lastActivityAt to extend session
            let mut updated_session = session_data.clone();
            updated_session["lastActivityAt"] = json!(
                OffsetDateTime::now_utc()
                    .format(&time::format_description::well_known::Rfc3339)
                    .unwrap()
            );

            // Calculate new expiry based on idle timeout
            let idle_timeout_duration =
                time::Duration::seconds(state.session_config.idle_timeout.as_secs() as i64);
            let new_expiry = OffsetDateTime::now_utc() + idle_timeout_duration;
            updated_session["expiresAt"] = json!(
                new_expiry
                    .format(&time::format_description::well_known::Rfc3339)
                    .unwrap()
            );

            // Update the session (fire and forget - don't block on this)
            let _ = state.fhir_storage.update(&updated_session, None).await;

            // User is authenticated via SSO - check consent
            let scopes: Vec<&str> = params.scope.split_whitespace().collect();

            if let Ok(true) = state
                .consent_storage
                .has_consent(user_id, &params.client_id, &scopes)
                .await
            {
                // Has consent - issue code immediately without showing any UI
                let temp_session = AuthorizeSession::new(params.clone());
                return issue_authorization_code(&state, &temp_session, user_id).await;
            }

            // No consent - create temporary session and show consent form
            let temp_session = AuthorizeSession::new(params.clone());
            let session_id = temp_session.id;

            if let Err(e) = state.authorize_session_storage.create(&temp_session).await {
                tracing::error!("Failed to create authorize session: {}", e);
                return redirect_with_error(
                    &params.redirect_uri,
                    AuthorizationErrorCode::ServerError,
                    "Failed to create session",
                    &params.state,
                );
            }

            // Update session with user_id
            if let Err(e) = state
                .authorize_session_storage
                .update_user(session_id, user_id)
                .await
            {
                tracing::error!("Failed to update session with user: {}", e);
            }

            // Create oauth session cookie and show consent form
            let cookie = create_session_cookie(session_id, state.secure_cookies);
            let jar = jar.add(cookie);

            return (
                StatusCode::OK,
                jar,
                Html(render_consent_form(
                    &client.name,
                    &params.redirect_uri,
                    &scopes,
                    &session_id.to_string(),
                )),
            )
                .into_response();
        }
    }

    // Check for existing session cookie
    if let Some(cookie) = jar.get(SESSION_COOKIE_NAME)
        && let Ok(session_id) = Uuid::parse_str(cookie.value())
        && let Ok(Some(session)) = state.authorize_session_storage.find_by_id(session_id).await
        && let Some(ref user_id) = session.user_id
    {
        let scopes: Vec<&str> = session.scopes();

        // Check if user has prior consent
        if let Ok(true) = state
            .consent_storage
            .has_consent(user_id, session.client_id(), &scopes)
            .await
        {
            // Issue code immediately
            return issue_authorization_code(&state, &session, user_id).await;
        }

        // Show consent form
        return (
            StatusCode::OK,
            Html(render_consent_form(
                &client.name,
                &params.redirect_uri,
                &scopes,
                &session_id.to_string(),
            )),
        )
            .into_response();
    }

    // Create new session
    let session = AuthorizeSession::new(params.clone());
    let session_id = session.id;

    if let Err(e) = state.authorize_session_storage.create(&session).await {
        tracing::error!("Failed to create authorize session: {}", e);
        return redirect_with_error(
            &params.redirect_uri,
            AuthorizationErrorCode::ServerError,
            "Failed to create session",
            &params.state,
        );
    }

    // Create session cookie
    let cookie = create_session_cookie(session_id, state.secure_cookies);
    let jar = jar.add(cookie);

    // Render login form
    let html = render_login_form(&client.name, &session_id.to_string(), None);

    (jar, Html(html)).into_response()
}

/// POST /oauth/authorize handler.
///
/// Processes login or consent form submissions.
pub async fn authorize_post(
    State(state): State<AuthorizeState>,
    headers: HeaderMap,
    jar: CookieJar,
    Form(form): Form<AuthorizeFormData>,
) -> Response {
    // Parse session ID
    let session_id = match Uuid::parse_str(&form.session_id) {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Html(render_error_page("invalid_request", "Invalid session")),
            )
                .into_response();
        }
    };

    // Load session
    let session = match state.authorize_session_storage.find_by_id(session_id).await {
        Ok(Some(s)) => s,
        Ok(None) => {
            return (
                StatusCode::BAD_REQUEST,
                Html(render_error_page(
                    "invalid_request",
                    "Session expired or not found",
                )),
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!("Failed to load session: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Html(render_error_page("server_error", "Failed to load session")),
            )
                .into_response();
        }
    };

    // Load client for display
    let client = match state
        .client_storage
        .find_by_client_id(session.client_id())
        .await
    {
        Ok(Some(c)) => c,
        _ => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Html(render_error_page("server_error", "Client not found")),
            )
                .into_response();
        }
    };

    match form.action.as_str() {
        "login" => handle_login(&state, &session, &form, &client.name, jar, &headers).await,
        "authorize" => handle_authorize(&state, &session, jar).await,
        "deny" => {
            // Clean up session
            let _ = state.authorize_session_storage.delete(session_id).await;
            let _jar = jar.remove(Cookie::from(SESSION_COOKIE_NAME));

            // Return redirect with error
            redirect_with_error(
                session.redirect_uri(),
                AuthorizationErrorCode::AccessDenied,
                "The user denied the authorization request",
                session.state(),
            )
        }
        _ => (
            StatusCode::BAD_REQUEST,
            Html(render_error_page("invalid_request", "Invalid action")),
        )
            .into_response(),
    }
}

/// Handle login form submission.
async fn handle_login(
    state: &AuthorizeState,
    session: &AuthorizeSession,
    form: &AuthorizeFormData,
    client_name: &str,
    jar: CookieJar,
    headers: &HeaderMap,
) -> Response {
    let username = form.username.as_deref().unwrap_or("");
    let password = form.password.as_deref().unwrap_or("");

    if username.is_empty() || password.is_empty() {
        return (
            StatusCode::OK,
            Html(render_login_form(
                client_name,
                &session.id.to_string(),
                Some("Username and password are required"),
            )),
        )
            .into_response();
    }

    // Find user by username
    tracing::debug!(username = %username, "Looking up user by username");
    let user = match state.user_storage.find_by_username(username).await {
        Ok(Some(u)) => {
            tracing::debug!(
                user_id = %u.id,
                username = %u.username,
                has_password_hash = u.password_hash.is_some(),
                password_hash_prefix = ?u.password_hash.as_ref().map(|h| h.chars().take(20).collect::<String>()),
                active = u.active,
                "User found"
            );
            u
        }
        Ok(None) => {
            tracing::warn!(username = %username, "User not found");
            return (
                StatusCode::OK,
                Html(render_login_form(
                    client_name,
                    &session.id.to_string(),
                    Some("Invalid username or password"),
                )),
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!(username = %username, error = %e, "Failed to find user");
            return (
                StatusCode::OK,
                Html(render_login_form(
                    client_name,
                    &session.id.to_string(),
                    Some("Authentication failed"),
                )),
            )
                .into_response();
        }
    };

    // Verify password
    tracing::debug!(user_id = %user.id, "Verifying password");
    match state.user_storage.verify_password(&user.id, password).await {
        Ok(true) => {
            tracing::info!(user_id = %user.id, username = %user.username, "Password verified successfully");
            // Password verified, update session with user_id
            if let Err(e) = state
                .authorize_session_storage
                .update_user(session.id, &user.id)
                .await
            {
                tracing::error!("Failed to update session: {}", e);
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Html(render_error_page(
                        "server_error",
                        "Failed to update session",
                    )),
                )
                    .into_response();
            }

            // Create SSO session for persistent login
            let jar = create_sso_session(state, &user.id, headers, jar).await;

            let scopes: Vec<&str> = session.scopes();

            // Check if user has prior consent
            if let Ok(true) = state
                .consent_storage
                .has_consent(&user.id, session.client_id(), &scopes)
                .await
            {
                // Issue code immediately
                return issue_authorization_code(state, session, &user.id).await;
            }

            // Show consent form
            (
                StatusCode::OK,
                jar,
                Html(render_consent_form(
                    client_name,
                    session.redirect_uri(),
                    &scopes,
                    &session.id.to_string(),
                )),
            )
                .into_response()
        }
        Ok(false) => {
            tracing::warn!(
                user_id = %user.id,
                username = %user.username,
                has_password_hash = user.password_hash.is_some(),
                "Password verification failed - hash mismatch"
            );
            (
                StatusCode::OK,
                Html(render_login_form(
                    client_name,
                    &session.id.to_string(),
                    Some("Invalid username or password"),
                )),
            )
                .into_response()
        }
        Err(e) => {
            tracing::error!(user_id = %user.id, error = %e, "Failed to verify password");
            (
                StatusCode::OK,
                Html(render_login_form(
                    client_name,
                    &session.id.to_string(),
                    Some("Authentication failed"),
                )),
            )
                .into_response()
        }
    }
}

/// Handle authorize consent form submission.
async fn handle_authorize(
    state: &AuthorizeState,
    session: &AuthorizeSession,
    _jar: CookieJar,
) -> Response {
    let user_id = match &session.user_id {
        Some(id) => id,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Html(render_error_page(
                    "invalid_request",
                    "User not authenticated",
                )),
            )
                .into_response();
        }
    };

    // Save consent
    let scopes: Vec<String> = session.scopes().iter().map(|s| s.to_string()).collect();
    if let Err(e) = state
        .consent_storage
        .save_consent(user_id, session.client_id(), &scopes)
        .await
    {
        tracing::error!("Failed to save consent: {}", e);
        // Continue anyway - consent not saved but we can still issue the code
    }

    // Issue authorization code
    issue_authorization_code(state, session, user_id).await
}

/// Issue an authorization code and redirect.
async fn issue_authorization_code(
    state: &AuthorizeState,
    session: &AuthorizeSession,
    user_id: &str,
) -> Response {
    // Use the authorization service to create the authorization session
    match state
        .authorization_service
        .authorize(&session.authorization_request)
        .await
    {
        Ok(auth_session) => {
            // Update the session with the user ID
            if let Err(e) = state
                .session_storage
                .update_user(auth_session.id, user_id)
                .await
            {
                tracing::error!("Failed to update authorization session with user: {}", e);
                return redirect_with_error(
                    session.redirect_uri(),
                    AuthorizationErrorCode::ServerError,
                    "Failed to create authorization code",
                    session.state(),
                );
            }

            // Clean up authorize session
            let _ = state.authorize_session_storage.delete(session.id).await;

            // Build redirect URL
            let response =
                AuthorizationResponse::new(auth_session.code.clone(), session.state().to_string());

            match response.to_redirect_url(session.redirect_uri()) {
                Ok(url) => Redirect::to(&url).into_response(),
                Err(e) => {
                    tracing::error!("Failed to build redirect URL: {}", e);
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Html(render_error_page(
                            "server_error",
                            "Failed to build redirect URL",
                        )),
                    )
                        .into_response()
                }
            }
        }
        Err(e) => {
            tracing::error!("Failed to create authorization code: {}", e);
            redirect_with_error(
                session.redirect_uri(),
                AuthorizationErrorCode::ServerError,
                "Failed to create authorization code",
                session.state(),
            )
        }
    }
}

/// Create a redirect response with an OAuth error.
fn redirect_with_error(
    redirect_uri: &str,
    error_code: AuthorizationErrorCode,
    description: &str,
    state: &str,
) -> Response {
    let error = AuthorizationError::with_description(error_code, description, state.to_string());

    match error.to_redirect_url(redirect_uri) {
        Ok(url) => Redirect::to(&url).into_response(),
        Err(_) => (
            StatusCode::BAD_REQUEST,
            Html(render_error_page(error_code.as_str(), description)),
        )
            .into_response(),
    }
}

/// Create a session cookie.
fn create_session_cookie(session_id: Uuid, secure: bool) -> Cookie<'static> {
    Cookie::build((SESSION_COOKIE_NAME, session_id.to_string()))
        .http_only(true)
        .secure(secure)
        .same_site(SameSite::Lax)
        .path("/auth")
        .max_age(Duration::minutes(10))
        .build()
}

/// Create an SSO session and return the updated cookie jar.
async fn create_sso_session(
    state: &AuthorizeState,
    user_id: &str,
    headers: &HeaderMap,
    jar: CookieJar,
) -> CookieJar {
    // Extract device information
    let user_agent = extract_user_agent(headers);
    let device_name = generate_device_name(user_agent.as_deref());
    let ip_address = extract_ip_address(headers);

    // Generate session token
    let session_token = Uuid::new_v4().to_string();

    // Calculate expiry times
    let now = OffsetDateTime::now_utc();
    let idle_timeout = time::Duration::seconds(state.session_config.idle_timeout.as_secs() as i64);
    let absolute_timeout =
        time::Duration::seconds(state.session_config.absolute_timeout.as_secs() as i64);
    let idle_expiry = now + idle_timeout;
    let absolute_expiry = now + absolute_timeout;

    // Use the earlier of the two expiry times
    let expires_at = if idle_expiry < absolute_expiry {
        idle_expiry
    } else {
        absolute_expiry
    };

    // Create AuthSession resource
    let auth_session = json!({
        "resourceType": "AuthSession",
        "status": "active",
        "sessionToken": session_token,
        "subject": {
            "reference": format!("User/{}", user_id)
        },
        "deviceName": device_name,
        "userAgent": user_agent.unwrap_or_default(),
        "ipAddress": ip_address.unwrap_or_default(),
        "createdAt": now.format(&time::format_description::well_known::Rfc3339).unwrap(),
        "lastActivityAt": now.format(&time::format_description::well_known::Rfc3339).unwrap(),
        "expiresAt": expires_at.format(&time::format_description::well_known::Rfc3339).unwrap(),
    });

    // Create the resource via FHIR API
    match state.fhir_storage.create(&auth_session).await {
        Ok(stored) => {
            tracing::info!(
                user_id = %user_id,
                session_id = %stored.id,
                "Created SSO session"
            );

            // Create SSO cookie
            let sso_cookie =
                create_sso_cookie(&state.session_config, &session_token, state.secure_cookies);
            jar.add(sso_cookie)
        }
        Err(e) => {
            tracing::error!("Failed to create SSO session: {}", e);
            // Return jar unchanged - authentication still works, just no SSO
            jar
        }
    }
}

/// Create an SSO session cookie.
fn create_sso_cookie(config: &SessionConfig, token: &str, secure: bool) -> Cookie<'static> {
    let max_age = Duration::seconds(config.idle_timeout.as_secs() as i64);

    Cookie::build((config.cookie_name.clone(), token.to_string()))
        .http_only(true)
        .secure(secure)
        .same_site(SameSite::Lax)
        .path("/")
        .max_age(max_age)
        .build()
}
