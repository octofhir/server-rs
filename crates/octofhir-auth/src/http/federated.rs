//! Sign-in through an external OpenID Connect provider.
//!
//! - `GET /auth/idp/providers` lists the active providers for a sign-in page.
//! - `GET /auth/idp/{provider}/login?return_to=` sends the browser to the provider.
//! - `GET /auth/idp/callback` takes it back, finds or creates the user and starts a session.
//! - `POST /auth/idp/saml/acs` takes a SAML response, which then goes on to the callback.
//! - `GET /auth/idp/saml/metadata` describes this server to SAML providers.
//!
//! `state` is single use, lives ten minutes and is tied to the browser with a cookie, so a code
//! obtained elsewhere cannot be planted in someone's browser. A SAML response arrives by a
//! cross-site POST, which carries no `SameSite=Lax` cookie, so it is verified there and finished
//! at the callback, where the cookie is back. `return_to` is a local path. An
//! interrupted `/auth/authorize` carries on from there, as the SSO session now answers it;
//! anywhere else is the console, which gets its cookie here.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::Json;
use axum::extract::{Form, Path, Query, State};
use axum::http::header::{CONTENT_TYPE, SET_COOKIE};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum_extra::extract::CookieJar;
use cookie::{Cookie, SameSite};
use saml_rs::PendingAuthnRequest;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::error::AuthError;
use crate::federation::saml::ServiceProvider;
use crate::federation::{
    IdpAuthResult, IdpAuthService, IdpError, ProvisioningConfig, add_identity,
    create_identity_from_auth_result, create_user_from_auth_result, result_from_claims,
};
use crate::http::authorize::{AuthorizeState, create_sso_session, redirect_with_error};
use crate::http::authorize_templates::render_error_page;
use crate::http::token::TokenState;
use crate::oauth::authorize::AuthorizationErrorCode;
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
    step: Step,
    return_to: String,
    expires: Instant,
}

/// Where a sign-in is.
enum Step {
    /// Waiting for an authorization code.
    Code {
        nonce: String,
        verifier: PkceVerifier,
    },
    /// Waiting for a SAML response.
    Saml(Box<PendingAuthnRequest>),
    /// The provider's answer is verified; the browser has to come back for it.
    Verified(Box<IdpAuthResult>),
}

/// State of the federated sign-in endpoints.
#[derive(Clone)]
pub struct FederatedState {
    service: Arc<IdpAuthService>,
    authorize: AuthorizeState,
    token: TokenState,
    provisioning: ProvisioningConfig,
    console_client: String,
    saml: Option<Arc<ServiceProvider>>,
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
            saml: None,
            pending: Arc::default(),
        }
    }

    /// Take SAML providers too, as this service provider.
    pub fn with_saml(mut self, saml: Arc<ServiceProvider>) -> Self {
        self.saml = Some(saml);
        self
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
        // When the provider decides roles, having none means having no business here.
        if result.manages_roles && result.mapped_user.roles.is_empty() {
            return Err(Refusal::NoRole);
        }
        let users = &self.authorize.user_storage;
        let linked = users
            .find_by_external_identity(&result.provider_id, &result.external_subject)
            .await?;
        if let Some(user) = linked {
            return self.with_roles_of(active(user)?, result).await;
        }
        if let Some(user) = self.link_by_email(result).await? {
            return self.with_roles_of(active(user)?, result).await;
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

    /// A user's roles follow the provider that manages them, so leaving a group at the provider
    /// takes the role away at the next sign-in.
    async fn with_roles_of(&self, mut user: User, result: &IdpAuthResult) -> Result<User, Refusal> {
        if !result.manages_roles {
            return Ok(user);
        }
        let mut roles = self.provisioning.default_roles.clone();
        for role in &result.mapped_user.roles {
            if !roles.contains(role) {
                roles.push(role.clone());
            }
        }
        let same = roles.len() == user.roles.len() && roles.iter().all(|r| user.roles.contains(r));
        if !same {
            tracing::info!(
                user_id = %user.id,
                provider = %result.provider_id,
                ?roles,
                "Roles changed at federated sign-in"
            );
            self.store(&user, "roles", json!(roles)).await?;
            user.roles = roles;
        }
        Ok(user)
    }

    /// Change an element of a user through the resource store, so that what reads it, the cache
    /// of resources among them, hears of it.
    async fn store(&self, user: &User, element: &str, value: Value) -> Result<(), AuthError> {
        let storage = &self.authorize.fhir_storage;
        let stored = storage
            .read("User", &user.id)
            .await
            .map_err(|error| AuthError::internal(error.to_string()))?
            .ok_or_else(|| AuthError::internal(format!("User {} is gone", user.id)))?;
        let mut resource = stored.resource;
        // Users are kept without these, and the store goes by them.
        resource["resourceType"] = json!("User");
        resource["id"] = json!(user.id);
        resource[element] = value;
        storage
            .update(&resource, None)
            .await
            .map_err(|error| AuthError::internal(error.to_string()))?;
        Ok(())
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
        self.store(&user, "attributes", json!(user.attributes))
            .await?;
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
    NoRole,
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
            Self::NoRole => error_page(
                StatusCode::FORBIDDEN,
                "access_denied",
                "This account has no role here. Ask an administrator for access.",
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logo: Option<String>,
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
            logo: provider.logo_url,
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
    let unavailable = || {
        error_page(
            StatusCode::NOT_FOUND,
            "invalid_request",
            "This identity provider is not available.",
        )
    };
    let provider = match state.service.get_provider(&provider_id).await {
        Some(provider) if provider.enabled => provider,
        _ => return unavailable(),
    };
    let (step, redirect) = if let Some(saml) = &provider.saml {
        let Some(sp) = &state.saml else {
            return unavailable();
        };
        let started = ServiceProvider::provider(&saml.entity_id, &saml.metadata)
            .and_then(|descriptor| sp.start(&descriptor, &token));
        match started {
            Ok(started) => (Step::Saml(Box::new(started.pending)), started.redirect),
            Err(error) => {
                tracing::error!(provider = %provider_id, %error, "Cannot start a SAML sign-in");
                return error_page(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "server_error",
                    "This identity provider cannot be used yet; the server log says why.",
                );
            }
        }
    } else {
        let nonce = Uuid::new_v4().to_string();
        match state.service.start_auth(&provider_id, &token, &nonce).await {
            Ok(request) => (
                Step::Code {
                    nonce,
                    verifier: request.pkce_verifier,
                },
                request.authorization_url.to_string(),
            ),
            Err(IdpError::ProviderNotFound(_) | IdpError::ProviderDisabled(_)) => {
                return unavailable();
            }
            Err(error) => {
                tracing::error!(provider = %provider_id, %error, "Cannot reach the identity provider");
                return error_page(
                    StatusCode::BAD_GATEWAY,
                    "temporarily_unavailable",
                    "The identity provider cannot be reached.",
                );
            }
        }
    };
    let pending = Pending {
        provider_id,
        step,
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
    (jar.add(cookie), Redirect::to(&redirect)).into_response()
}

/// When the sign-in was for an authorization request, the client app hears of a failure at its
/// redirect URI, as UDAP Tiered OAuth requires (`access_denied`, `invalid_idp`). The redirect
/// URI is checked against the client's own first: `return_to` came through the browser.
async fn refuse_to_client(
    state: &FederatedState,
    return_to: &str,
    code: AuthorizationErrorCode,
    description: &str,
) -> Option<Response> {
    let query = return_to.strip_prefix("/auth/authorize?")?;
    let fields: HashMap<String, String> = url::form_urlencoded::parse(query.as_bytes())
        .into_owned()
        .collect();
    let (client_id, redirect_uri) = (fields.get("client_id")?, fields.get("redirect_uri")?);
    let client = state
        .authorize
        .client_storage
        .find_by_client_id(client_id)
        .await
        .ok()??;
    if !client.redirect_uris.contains(redirect_uri) {
        return None;
    }
    let client_state = fields.get("state").map_or("", String::as_str);
    Some(redirect_with_error(
        redirect_uri,
        code,
        description,
        client_state,
    ))
}

fn unverified() -> Response {
    error_page(
        StatusCode::UNAUTHORIZED,
        "access_denied",
        "The identity provider's answer could not be verified.",
    )
}

/// What a SAML provider posts.
#[derive(Debug, Deserialize)]
pub struct AcsForm {
    #[serde(rename = "SAMLResponse")]
    pub saml_response: String,
    #[serde(rename = "RelayState")]
    pub relay_state: Option<String>,
}

/// `POST /auth/idp/saml/acs`
pub async fn saml_acs(State(state): State<FederatedState>, Form(form): Form<AcsForm>) -> Response {
    let expired = || {
        error_page(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "This sign-in has expired or was not started here. Please start again.",
        )
    };
    // Only answers to requests made here: RelayState names the sign-in.
    let Some(token) = form.relay_state else {
        return expired();
    };
    let Some(mut pending) = state.take(&token) else {
        return expired();
    };
    let Step::Saml(request) = &pending.step else {
        return expired();
    };
    let (Some(sp), Some(provider)) = (
        &state.saml,
        state.service.get_provider(&pending.provider_id).await,
    ) else {
        return expired();
    };
    let Some(saml) = &provider.saml else {
        return expired();
    };
    let verified = ServiceProvider::provider(&saml.entity_id, &saml.metadata)
        .and_then(|descriptor| sp.finish(&descriptor, request, &form.saml_response, &token))
        .map_err(|error| error.to_string())
        .and_then(|claims| result_from_claims(&provider, &claims).map_err(|e| e.to_string()));
    let result = match verified {
        Ok(result) => result,
        Err(error) => {
            tracing::warn!(provider = %pending.provider_id, %error, "SAML response refused");
            return unverified();
        }
    };
    pending.step = Step::Verified(Box::new(result));
    if !state.remember(token.clone(), pending) {
        return expired();
    }
    let query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("state", &token)
        .finish();
    Redirect::to(&format!("/auth/idp/callback?{query}")).into_response()
}

/// `GET /auth/idp/saml/metadata`
pub async fn saml_metadata(State(state): State<FederatedState>) -> Response {
    match &state.saml {
        Some(sp) => (
            [(CONTENT_TYPE, "application/samlmetadata+xml")],
            sp.metadata().to_owned(),
        )
            .into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
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
    let result = match pending.step {
        Step::Verified(result) => *result,
        Step::Saml(_) => return expired(),
        Step::Code { nonce, verifier } => {
            if let Some(error) = query.error {
                let description = query.error_description.unwrap_or_default();
                tracing::warn!(provider = %pending.provider_id, %error, %description, "Provider refused");
                if let Some(response) = refuse_to_client(
                    &state,
                    &pending.return_to,
                    AuthorizationErrorCode::AccessDenied,
                    "The identity provider refused the sign-in",
                )
                .await
                {
                    return response;
                }
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
                .handle_callback(&pending.provider_id, &code, &verifier, &nonce)
                .await;
            match result {
                Ok(result) => result,
                Err(error) => {
                    tracing::warn!(provider = %pending.provider_id, %error, "Federated sign-in failed");
                    if let Some(response) = refuse_to_client(
                        &state,
                        &pending.return_to,
                        AuthorizationErrorCode::InvalidIdp,
                        "The identity provider's answer could not be verified",
                    )
                    .await
                    {
                        return response;
                    }
                    return unverified();
                }
            }
        }
    };
    let user = match state.user_of(&result).await {
        Ok(user) => user,
        Err(refusal) => {
            if let Some(response) = refuse_to_client(
                &state,
                &pending.return_to,
                AuthorizationErrorCode::AccessDenied,
                "The user could not be matched to an account here",
            )
            .await
            {
                return response;
            }
            return refusal.into_response();
        }
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
