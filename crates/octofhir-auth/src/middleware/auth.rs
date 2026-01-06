//! Bearer token authentication extractor.
//!
//! This module provides Axum extractors for validating Bearer tokens
//! and extracting authentication context from requests.
//!
//! # Example
//!
//! ```ignore
//! use axum::{Router, routing::get};
//! use octofhir_auth::middleware::{AuthState, BearerAuth};
//!
//! async fn protected_handler(BearerAuth(auth): BearerAuth) -> String {
//!     format!("Hello, {}!", auth.subject())
//! }
//!
//! let app = Router::new()
//!     .route("/protected", get(protected_handler))
//!     .with_state(auth_state);
//! ```

use std::sync::Arc;

use axum::{
    extract::{FromRef, FromRequestParts},
    http::{header::AUTHORIZATION, header::COOKIE, request::Parts},
};
use crate::config::CookieConfig;
use crate::error::AuthError;
use crate::storage::{ClientStorage, RevokedTokenStorage, UserStorage};
use crate::token::jwt::{AccessTokenClaims, JwtService};

use super::types::{AuthContext, UserContext};

// =============================================================================
// Auth State
// =============================================================================

/// State required for bearer token authentication.
///
/// This struct should be included in your application state and made
/// available to the `BearerAuth` extractor via `FromRef`.
///
/// # Example
///
/// ```ignore
/// #[derive(Clone)]
/// struct AppState {
///     auth: AuthState,
///     // ... other state
/// }
///
/// impl FromRef<AppState> for AuthState {
///     fn from_ref(state: &AppState) -> Self {
///         state.auth.clone()
///     }
/// }
/// ```
#[derive(Clone)]
pub struct AuthState {
    /// JWT service for token validation.
    pub jwt_service: Arc<JwtService>,

    /// Client storage for looking up OAuth clients.
    pub client_storage: Arc<dyn ClientStorage>,

    /// Revoked token storage for checking token revocation.
    pub revoked_token_storage: Arc<dyn RevokedTokenStorage>,

    /// User storage for loading user context.
    pub user_storage: Arc<dyn UserStorage>,

    /// Cookie configuration for browser-based auth.
    pub cookie_config: CookieConfig,
}

impl AuthState {
    /// Creates a new auth state.
    pub fn new(
        jwt_service: Arc<JwtService>,
        client_storage: Arc<dyn ClientStorage>,
        revoked_token_storage: Arc<dyn RevokedTokenStorage>,
        user_storage: Arc<dyn UserStorage>,
    ) -> Self {
        Self {
            jwt_service,
            client_storage,
            revoked_token_storage,
            user_storage,
            cookie_config: CookieConfig::default(),
        }
    }

    /// Sets cookie configuration for browser-based authentication.
    #[must_use]
    pub fn with_cookie_config(mut self, cookie_config: CookieConfig) -> Self {
        self.cookie_config = cookie_config;
        self
    }
}

// =============================================================================
// Bearer Auth Extractor
// =============================================================================

/// Axum extractor that validates Bearer tokens and extracts auth context.
///
/// This extractor:
/// 1. Extracts the `Authorization: Bearer <token>` header
/// 2. Decodes and validates the JWT
/// 3. Checks token expiration
/// 4. Checks token revocation
/// 5. Loads the OAuth client (and verifies it's active)
/// 6. Optionally loads user context
///
/// # Errors
///
/// Returns `AuthError` (which implements `IntoResponse`) if:
/// - Authorization header is missing or malformed
/// - Token is invalid, expired, or revoked
/// - Client is unknown or inactive
///
/// # Example
///
/// ```ignore
/// async fn handler(BearerAuth(auth): BearerAuth) -> impl IntoResponse {
///     if auth.has_scope("patient/Patient.read") {
///         // Allow access
///     }
/// }
/// ```
pub struct BearerAuth(pub AuthContext);

impl<S> FromRequestParts<S> for BearerAuth
where
    S: Send + Sync,
    AuthState: FromRef<S>,
{
    type Rejection = AuthError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let auth_state = AuthState::from_ref(state);

        // 1. Try Authorization header first
        let token = if let Some(auth_header) = parts
            .headers
            .get(AUTHORIZATION)
            .and_then(|h| h.to_str().ok())
        {
            // Parse Bearer token from header
            auth_header
                .strip_prefix("Bearer ")
                .filter(|t| !t.is_empty())
                .map(ToString::to_string)
        } else {
            None
        };

        // 2. If no Authorization header, try cookie (if enabled)
        let token = match token {
            Some(t) => t,
            None => {
                // Try to extract from cookie
                if let Some(t) = extract_token_from_cookie(parts, &auth_state.cookie_config) {
                    t
                } else if let Some(t) = extract_token_from_query(parts) {
                    // 3. Try query parameter (for WebSocket connections)
                    t
                } else {
                    return Err(AuthError::unauthorized("Missing Authorization header"));
                }
            }
        };

        if token.is_empty() {
            return Err(AuthError::unauthorized("Empty Bearer token"));
        }

        // 3. Decode and validate JWT (using spawn_blocking to avoid blocking async runtime)
        let claims = auth_state
            .jwt_service
            .decode_async::<AccessTokenClaims>(token)
            .await
            .map_err(|e| {
                tracing::debug!(error = %e, "Failed to decode token");
                AuthError::invalid_token(e.to_string())
            })?
            .claims;

        // 4. Check expiration (already done by JWT library, but explicit check)
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        if claims.exp < now {
            tracing::debug!(jti = %claims.jti, "Token expired");
            return Err(AuthError::TokenExpired);
        }

        // 5. Check revocation
        if auth_state
            .revoked_token_storage
            .is_revoked(&claims.jti)
            .await?
        {
            tracing::debug!(jti = %claims.jti, "Token revoked");
            return Err(AuthError::TokenRevoked);
        }

        // 6. Load client
        let client = auth_state
            .client_storage
            .find_by_client_id(&claims.client_id)
            .await?
            .ok_or_else(|| {
                tracing::warn!(client_id = %claims.client_id, "Unknown client in token");
                AuthError::invalid_token("Unknown client")
            })?;

        // 7. Verify client is active
        if !client.active {
            tracing::warn!(client_id = %claims.client_id, "Inactive client");
            return Err(AuthError::invalid_token("Client is inactive"));
        }

        // 8. Load user context if subject is a valid UUID
        let user = load_user_context(&auth_state, &claims).await?;

        // 9. Build auth context (wrap claims in Arc for cheap cloning)
        let auth_context = AuthContext {
            patient: claims.patient.clone(),
            encounter: claims.encounter.clone(),
            token_claims: Arc::new(claims),
            client,
            user,
        };

        tracing::debug!(
            client_id = %auth_context.client_id(),
            subject = %auth_context.subject(),
            has_user = auth_context.is_user_authenticated(),
            "Token validated successfully"
        );

        Ok(BearerAuth(auth_context))
    }
}

/// Loads user context from the token's subject claim.
async fn load_user_context(
    state: &AuthState,
    claims: &AccessTokenClaims,
) -> Result<Option<UserContext>, AuthError> {
    // Subject claim contains the user ID
    let user_id = &claims.sub;

    // If subject is empty or looks like a client ID, skip user lookup
    if user_id.is_empty() {
        return Ok(None);
    }

    // Load user from storage
    match state.user_storage.find_by_id(user_id).await? {
        Some(user) => {
            if !user.active {
                tracing::warn!(user_id = %user_id, "Inactive user");
                return Err(AuthError::invalid_token("User is inactive"));
            }

            Ok(Some(UserContext {
                id: user.id,
                username: user.username,
                name: user.name,
                email: user.email,
                fhir_user: user.fhir_user.or_else(|| claims.fhir_user.clone()),
                roles: user.roles,
                attributes: user.attributes,
            }))
        }
        None => {
            // User not found - treat as valid token but no user context
            // This can happen if user was deleted after token was issued
            tracing::debug!(user_id = %user_id, "User not found in storage");
            Ok(None)
        }
    }
}

// =============================================================================
// Optional Bearer Auth Extractor
// =============================================================================

/// Axum extractor that optionally validates Bearer tokens.
///
/// Unlike `BearerAuth`, this extractor does not fail if no Authorization
/// header is present. It returns `None` in that case.
///
/// Useful for endpoints that work differently for authenticated vs
/// unauthenticated requests.
///
/// # Example
///
/// ```ignore
/// async fn handler(OptionalBearerAuth(auth): OptionalBearerAuth) -> impl IntoResponse {
///     match auth {
///         Some(ctx) => format!("Hello, {}!", ctx.subject()),
///         None => "Hello, anonymous!".to_string(),
///     }
/// }
/// ```
pub struct OptionalBearerAuth(pub Option<AuthContext>);

impl<S> FromRequestParts<S> for OptionalBearerAuth
where
    S: Send + Sync,
    AuthState: FromRef<S>,
{
    type Rejection = AuthError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let auth_state = AuthState::from_ref(state);

        // Check if Authorization header is present, cookie auth is available, or token in query
        let has_auth_header = parts.headers.get(AUTHORIZATION).is_some();
        let has_cookie_token =
            auth_state.cookie_config.enabled && parts.headers.get(COOKIE).is_some();
        let has_query_token = parts.uri.query().is_some_and(|q| q.contains("token="));

        if !has_auth_header && !has_cookie_token && !has_query_token {
            return Ok(OptionalBearerAuth(None));
        }

        // Try to extract, but convert missing credentials to None
        match BearerAuth::from_request_parts(parts, state).await {
            Ok(BearerAuth(ctx)) => Ok(OptionalBearerAuth(Some(ctx))),
            Err(AuthError::Unauthorized { .. }) => Ok(OptionalBearerAuth(None)),
            Err(e) => Err(e), // Propagate other errors (invalid token, etc.)
        }
    }
}

// =============================================================================
// Cookie Helpers
// =============================================================================

/// Extract token from query parameter.
///
/// Useful for WebSocket connections where headers can't be set.
/// Looks for `token` query parameter in the request URI.
fn extract_token_from_query(parts: &Parts) -> Option<String> {
    let query = parts.uri.query()?;

    // Parse query string (simple key=value&key=value format)
    for pair in query.split('&') {
        if let Some((key, value)) = pair.split_once('=') {
            if key == "token" {
                let value = value.trim();
                if !value.is_empty() {
                    tracing::debug!("Token extracted from query parameter");
                    return Some(value.to_string());
                }
            }
        }
    }

    None
}

/// Extract token from cookie if cookie auth is enabled.
///
/// Parses the Cookie header and looks for the configured cookie name.
fn extract_token_from_cookie(parts: &Parts, cookie_config: &CookieConfig) -> Option<String> {
    // Only try cookie auth if enabled
    if !cookie_config.enabled {
        return None;
    }

    // Get Cookie header
    let cookie_header = parts.headers.get(COOKIE)?.to_str().ok()?;

    // Parse cookies (simple key=value; key=value format)
    let cookie_name = &cookie_config.name;
    for cookie in cookie_header.split(';') {
        let cookie = cookie.trim();
        if let Some((name, value)) = cookie.split_once('=')
            && name.trim() == cookie_name
        {
            let value = value.trim();
            if !value.is_empty() {
                tracing::debug!(cookie_name = %cookie_name, "Token extracted from cookie");
                return Some(value.to_string());
            }
        }
    }

    None
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auth_state_creation() {
        // This is a compile-time test to ensure AuthState can be created
        // Actual tests would require mock storage implementations
    }

    #[test]
    fn test_cookie_config_build_cookie() {
        let config = CookieConfig {
            enabled: true,
            name: "test_token".to_string(),
            secure: true,
            http_only: true,
            same_site: "strict".to_string(),
            path: "/".to_string(),
            domain: None,
        };

        let cookie = config.build_cookie("my_token_value", 3600);
        assert!(cookie.is_some());
        let cookie = cookie.unwrap();
        assert!(cookie.contains("test_token=my_token_value"));
        assert!(cookie.contains("Max-Age=3600"));
        assert!(cookie.contains("HttpOnly"));
        assert!(cookie.contains("Secure"));
        assert!(cookie.contains("SameSite=Strict"));
    }

    #[test]
    fn test_cookie_config_disabled() {
        // Explicitly create a disabled config
        let config = CookieConfig {
            enabled: false,
            ..CookieConfig::default()
        };
        assert!(!config.enabled);

        let cookie = config.build_cookie("my_token_value", 3600);
        assert!(cookie.is_none());
    }

    #[test]
    fn test_cookie_config_default_enabled() {
        // Cookie config is enabled by default for UI support
        let config = CookieConfig::default();
        assert!(config.enabled);
    }
}
