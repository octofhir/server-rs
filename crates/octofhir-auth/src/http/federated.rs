//! Sign-in through an external OpenID Connect provider.
//!
//! - `GET /auth/idp/providers` lists the active providers for a sign-in page.
//! - `GET /auth/idp/{provider}/login?return_to=` sends the browser to the provider.
//! - `GET /auth/idp/callback` takes it back, finds or creates the user and starts a session.
//!
//! `state` is single use, lives ten minutes and is tied to the browser with a cookie, so a code
//! obtained elsewhere cannot be planted in someone's browser. `return_to` is a local path. An
//! interrupted `/auth/authorize` carries on from there, as the SSO session now answers it;
//! anywhere else is the console, which gets its cookie here.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::header::SET_COOKIE;
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum_extra::extract::CookieJar;
use cookie::{Cookie, SameSite};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::AuthError;
use crate::federation::{
    IdpAuthResult, IdpAuthService, IdpError, ProvisioningConfig, add_identity,
    create_identity_from_auth_result, create_user_from_auth_result,
};
use crate::http::authorize::{AuthorizeState, create_sso_session};
use crate::http::authorize_templates::render_error_page;
use crate::http::token::TokenState;
use crate::oauth::pkce::PkceVerifier;
use crate::storage::User;

/// Ties a sign-in to the browser that started it.
const STATE_COOKIE: &str = "octofhir_idp_state";
const STATE_COOKIE_PATH: &str = "/auth/idp";

/// How long someone has at the provider.
const PENDING_LIFETIME: Duration = Duration::from_secs(600);

/// Starting a sign-in needs no authentication, so what it can hold is bounded.
const MAX_PENDING: usize = 10_000;

/// Where a sign-in without a destination ends.
const CONSOLE_PATH: &str = "/ui/";

/// What the console asks for at sign-in. No `offline_access`: the cookie is the session.
const CONSOLE_SCOPE: &str = "openid user/*.cruds system/*.cruds";

/// A sign-in the provider has not answered yet.
struct Pending {
    provider_id: String,
    nonce: String,
    verifier: PkceVerifier,
    return_to: String,
    expires: Instant,
}

/// State of the federated sign-in endpoints.
#[derive(Clone)]
pub struct FederatedState {
    service: Arc<IdpAuthService>,
    authorize: AuthorizeState,
    token: TokenState,
    provisioning: ProvisioningConfig,
    console_client: String,
    pending: Arc<Mutex<HashMap<String, Pending>>>,
}

impl FederatedState {
    pub fn new(
        service: Arc<IdpAuthService>,
        authorize: AuthorizeState,
        token: TokenState,
        provisioning: ProvisioningConfig,
        console_client: impl Into<String>,
    ) -> Self {
        Self {
            service,
            authorize,
            token,
            provisioning,
            console_client: console_client.into(),
            pending: Arc::default(),
        }
    }

    /// Remember a sign-in; `false` when too many are under way.
    fn remember(&self, state: String, pending: Pending) -> bool {
        let mut all = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        all.retain(|_, pending| pending.expires > now);
        if all.len() >= MAX_PENDING {
            return false;
        }
        all.insert(state, pending);
        true
    }

    /// A sign-in can be answered once.
    fn take(&self, state: &str) -> Option<Pending> {
        let mut all = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        all.remove(state)
            .filter(|pending| pending.expires > Instant::now())
    }

    /// The user behind what a provider said: already linked, linked now, or created.
    async fn user_of(&self, result: &IdpAuthResult) -> Result<User, Refusal> {
        let users = &self.authorize.user_storage;
        let linked = users
            .find_by_external_identity(&result.provider_id, &result.external_subject)
            .await?;
        if let Some(user) = linked {
            return active(user);
        }
        if let Some(user) = self.link_by_email(result).await? {
            return active(user);
        }
        if !self.provisioning.auto_provision {
            return Err(Refusal::NoAccount);
        }
        let mut user = create_user_from_auth_result(result, &self.provisioning);
        // A name someone else has is not a way into their account.
        if users.find_by_username(&user.username).await?.is_some() {
            user.username = format!("{}_{}", result.provider_id, result.external_subject);
        }
        users.create(&user).await?;
        tracing::info!(
            user_id = %user.id,
            provider = %result.provider_id,
            "Created a user at federated sign-in"
        );
        Ok(user)
    }

    async fn link_by_email(&self, result: &IdpAuthResult) -> Result<Option<User>, AuthError> {
        let mapped = &result.mapped_user;
        let email = mapped
            .email
            .as_deref()
            .filter(|_| self.provisioning.link_by_email && mapped.email_verified == Some(true));
        let Some(email) = email else {
            return Ok(None);
        };
        let users = &self.authorize.user_storage;
        let Some(mut user) = users.find_by_email(email).await? else {
            return Ok(None);
        };
        add_identity(&mut user, create_identity_from_auth_result(result));
        users.update(&user).await?;
        tracing::info!(
            user_id = %user.id,
            provider = %result.provider_id,
            "Linked an external identity by email"
        );
        Ok(Some(user))
    }
}

/// Why someone the provider vouched for does not get in.
enum Refusal {
    NoAccount,
    Inactive,
    Storage(AuthError),
}

impl From<AuthError> for Refusal {
    fn from(error: AuthError) -> Self {
        Self::Storage(error)
    }
}

impl IntoResponse for Refusal {
    fn into_response(self) -> Response {
        match self {
            Self::NoAccount => error_page(
                StatusCode::FORBIDDEN,
                "access_denied",
                "This account is not known here. Ask an administrator for access.",
            ),
            Self::Inactive => error_page(
                StatusCode::FORBIDDEN,
                "access_denied",
                "This account is disabled.",
            ),
            Self::Storage(error) => {
                tracing::error!(%error, "Federated sign-in failed");
                error_page(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "server_error",
                    "Sign-in failed.",
                )
            }
        }
    }
}

fn active(user: User) -> Result<User, Refusal> {
    if user.active {
        Ok(user)
    } else {
        Err(Refusal::Inactive)
    }
}

fn error_page(status: StatusCode, code: &str, description: &str) -> Response {
    (status, Html(render_error_page(code, description))).into_response()
}

/// A path on this server, and nothing a browser would read as another host.
fn is_local(path: &str) -> bool {
    path.starts_with('/')
        && !path.starts_with("//")
        && !path.contains('\\')
        && !path.chars().any(char::is_control)
}

/// The link that signs in through a provider and comes back to `return_to`.
pub(crate) fn federated_login_url(provider_id: &str, return_to: &str) -> String {
    let query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("return_to", return_to)
        .finish();
    format!("/auth/idp/{provider_id}/login?{query}")
}

/// A provider on a sign-in page.
#[derive(Debug, Serialize)]
pub struct ProviderLink {
    pub id: String,
    pub name: String,
}

/// `GET /auth/idp/providers`
pub async fn federated_providers(State(state): State<FederatedState>) -> Json<Vec<ProviderLink>> {
    let mut providers: Vec<ProviderLink> = state
        .service
        .list_enabled_providers()
        .await
        .into_iter()
        .map(|provider| ProviderLink {
            id: provider.id,
            name: provider.name,
        })
        .collect();
    providers.sort_by(|a, b| a.name.cmp(&b.name));
    Json(providers)
}

/// Query of the login endpoint.
#[derive(Debug, Deserialize)]
pub struct LoginQuery {
    /// A local path to come back to.
    pub return_to: Option<String>,
}

/// `GET /auth/idp/{provider}/login`
pub async fn federated_login(
    State(state): State<FederatedState>,
    Path(provider_id): Path<String>,
    Query(query): Query<LoginQuery>,
    jar: CookieJar,
) -> Response {
    let return_to = query.return_to.unwrap_or_else(|| CONSOLE_PATH.to_owned());
    if !is_local(&return_to) {
        return error_page(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "return_to must be a path on this server.",
        );
    }
    let token = Uuid::new_v4().to_string();
    let nonce = Uuid::new_v4().to_string();
    let request = match state.service.start_auth(&provider_id, &token, &nonce).await {
        Ok(request) => request,
        Err(IdpError::ProviderNotFound(_) | IdpError::ProviderDisabled(_)) => {
            return error_page(
                StatusCode::NOT_FOUND,
                "invalid_request",
                "This identity provider is not available.",
            );
        }
        Err(error) => {
            tracing::error!(provider = %provider_id, %error, "Cannot reach the identity provider");
            return error_page(
                StatusCode::BAD_GATEWAY,
                "temporarily_unavailable",
                "The identity provider cannot be reached.",
            );
        }
    };
    let pending = Pending {
        provider_id,
        nonce,
        verifier: request.pkce_verifier,
        return_to,
        expires: Instant::now() + PENDING_LIFETIME,
    };
    if !state.remember(token.clone(), pending) {
        return error_page(
            StatusCode::SERVICE_UNAVAILABLE,
            "temporarily_unavailable",
            "Too many sign-ins are under way, try again later.",
        );
    }
    let cookie = Cookie::build((STATE_COOKIE, token))
        .http_only(true)
        .secure(state.authorize.secure_cookies)
        // The provider sends the browser back with a top-level GET.
        .same_site(SameSite::Lax)
        .path(STATE_COOKIE_PATH)
        .max_age(time::Duration::seconds(PENDING_LIFETIME.as_secs() as i64))
        .build();
    (
        jar.add(cookie),
        Redirect::to(request.authorization_url.as_str()),
    )
        .into_response()
}

/// Query of the callback.
#[derive(Debug, Deserialize)]
pub struct CallbackQuery {
    pub code: Option<String>,
    pub state: Option<String>,
    pub error: Option<String>,
    pub error_description: Option<String>,
}

/// `GET /auth/idp/callback`
pub async fn federated_callback(
    State(state): State<FederatedState>,
    Query(query): Query<CallbackQuery>,
    jar: CookieJar,
    headers: HeaderMap,
) -> Response {
    let expired = || {
        error_page(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "This sign-in has expired or was started elsewhere. Please start again.",
        )
    };
    let Some(token) = query.state.as_deref() else {
        return expired();
    };
    if jar.get(STATE_COOKIE).map(Cookie::value) != Some(token) {
        return expired();
    }
    let Some(pending) = state.take(token) else {
        return expired();
    };
    let jar = jar.remove(Cookie::build(STATE_COOKIE).path(STATE_COOKIE_PATH).build());
    if let Some(error) = query.error {
        let description = query.error_description.unwrap_or_default();
        tracing::warn!(provider = %pending.provider_id, %error, %description, "Provider refused");
        return error_page(
            StatusCode::FORBIDDEN,
            "access_denied",
            "Sign-in was refused.",
        );
    }
    let Some(code) = query.code else {
        return expired();
    };
    let result = state
        .service
        .handle_callback(
            &pending.provider_id,
            &code,
            &pending.verifier,
            &pending.nonce,
        )
        .await;
    let result = match result {
        Ok(result) => result,
        Err(error) => {
            tracing::warn!(provider = %pending.provider_id, %error, "Federated sign-in failed");
            return error_page(
                StatusCode::UNAUTHORIZED,
                "access_denied",
                "The identity provider's answer could not be verified.",
            );
        }
    };
    let user = match state.user_of(&result).await {
        Ok(user) => user,
        Err(refusal) => return refusal.into_response(),
    };
    if let Err(error) = state
        .authorize
        .user_storage
        .update_last_login(&user.id)
        .await
    {
        tracing::warn!(user_id = %user.id, %error, "Failed to update last login timestamp");
    }
    let jar = create_sso_session(&state.authorize, &user.id, &headers, jar).await;
    let mut response = (jar, Redirect::to(&pending.return_to)).into_response();
    if pending.return_to.starts_with("/auth/authorize") {
        return response;
    }
    let cookie = state
        .token
        .login_cookie(&user, &state.console_client, CONSOLE_SCOPE)
        .await;
    match cookie.map(|cookie| cookie.map(|c| HeaderValue::from_str(&c))) {
        Ok(Some(Ok(cookie))) => {
            response.headers_mut().append(SET_COOKIE, cookie);
            response
        }
        Ok(_) => error_page(
            StatusCode::INTERNAL_SERVER_ERROR,
            "server_error",
            "Cookie sign-in is switched off on this server.",
        ),
        Err(error) => {
            tracing::error!(%error, "Cannot sign in to the console");
            error_page(
                StatusCode::INTERNAL_SERVER_ERROR,
                "server_error",
                "Sign-in failed.",
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn return_to_stays_on_this_server() {
        assert!(is_local("/ui/"));
        assert!(is_local(
            "/auth/authorize?client_id=a&redirect_uri=https%3A%2F%2Fapp"
        ));
        for foreign in [
            "https://evil.example",
            "//evil.example",
            "/\\evil.example",
            "\\\\evil.example",
            "/ui\r\nSet-Cookie: a=b",
            "",
        ] {
            assert!(!is_local(foreign), "{foreign}");
        }
    }

    #[test]
    fn the_login_link_carries_the_way_back() {
        assert_eq!(
            federated_login_url("okta", "/auth/authorize?a=1&b=2"),
            "/auth/idp/okta/login?return_to=%2Fauth%2Fauthorize%3Fa%3D1%26b%3D2"
        );
    }
}
