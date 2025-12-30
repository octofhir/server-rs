use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::State;
use axum::response::IntoResponse;
use axum::{
    Json,
    body::Body,
    http::{
        HeaderName, HeaderValue, Request, StatusCode,
        header::{AUTHORIZATION, COOKIE},
    },
    middleware::Next,
    response::Response,
};
use serde_json::{Value, json};
use uuid::Uuid;

use octofhir_auth::config::CookieConfig;
use octofhir_auth::middleware::{AuthContext, AuthState};
use octofhir_auth::policy::{
    AccessDecision, DenyReason, PolicyContext, PolicyContextBuilder, PolicyEvaluator,
};
use octofhir_auth::token::jwt::AccessTokenClaims;

use crate::cache::{AuthContextCache, JwtVerificationCache};
use crate::operation_registry::OperationRegistryService;

// =============================================================================
// Extended Auth State
// =============================================================================

/// Combined authentication state that includes operation registry and auth context cache.
#[derive(Clone)]
pub struct ExtendedAuthState {
    /// Core authentication state.
    pub auth_state: AuthState,
    /// Operation registry for public path lookups (source of truth).
    pub operation_registry: Arc<OperationRegistryService>,
    /// Cache for authenticated contexts (reduces DB queries per request).
    pub auth_cache: Arc<dyn AuthContextCache>,
    /// Cache for JWT verification (reduces signature verification overhead).
    pub jwt_cache: Arc<JwtVerificationCache>,
}

impl ExtendedAuthState {
    /// Creates a new extended auth state.
    pub fn new(
        auth_state: AuthState,
        operation_registry: Arc<OperationRegistryService>,
        auth_cache: Arc<dyn AuthContextCache>,
        jwt_cache: Arc<JwtVerificationCache>,
    ) -> Self {
        Self {
            auth_state,
            operation_registry,
            auth_cache,
            jwt_cache,
        }
    }
}

// =============================================================================
// Authentication Middleware
// =============================================================================

/// Authentication middleware that validates Bearer tokens and injects AuthContext.
///
/// This middleware:
/// 1. Checks if the path should skip authentication (public endpoints from registry + hardcoded)
/// 2. Extracts and validates Bearer tokens
/// 3. Stores the `AuthContext` in request extensions for downstream use
///
/// If authentication fails, returns 401 Unauthorized with FHIR OperationOutcome.
/// If no token is present for protected routes, returns 401.
///
/// # Requirements
///
/// - `ExtendedAuthState` must be available via `FromRef<S>`
pub async fn authentication_middleware(
    State(state): State<ExtendedAuthState>,
    mut req: Request<Body>,
    next: Next,
) -> Response {
    // Skip authentication for public endpoints (from registry + hardcoded)
    if should_skip_authentication(&req, &state.operation_registry) {
        return next.run(req).await;
    }

    // Get the core auth state for token validation
    let auth_state = &state.auth_state;

    // 1. Try Authorization header first
    let token = if let Some(auth_header) = req
        .headers()
        .get(AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
    {
        // Parse Bearer token from header
        match auth_header.strip_prefix("Bearer ") {
            Some(t) if !t.is_empty() => Some(t.to_string()),
            _ => {
                return unauthorized_response("Invalid Authorization header format");
            }
        }
    } else {
        None
    };

    // 2. If no Authorization header, try cookie (if enabled)
    let token = match token {
        Some(t) => t,
        None => {
            // Try to extract from cookie
            match extract_token_from_cookie(&req, &auth_state.cookie_config) {
                Some(t) => t,
                None => {
                    tracing::debug!(path = %req.uri().path(), "No Authorization header or cookie");
                    return unauthorized_response("Authentication required");
                }
            }
        }
    };

    // Validate token and build auth context (with caching)
    match validate_token(auth_state, &state.auth_cache, &state.jwt_cache, &token).await {
        Ok(auth_context) => {
            tracing::debug!(
                client_id = %auth_context.client_id(),
                subject = %auth_context.subject(),
                "Token validated successfully"
            );
            // Store auth context in request extensions
            req.extensions_mut().insert(auth_context);
            next.run(req).await
        }
        Err(e) => {
            tracing::debug!(error = %e, "Token validation failed");
            match e {
                octofhir_auth::AuthError::TokenExpired => unauthorized_response("Token expired"),
                octofhir_auth::AuthError::TokenRevoked => unauthorized_response("Token revoked"),
                _ => unauthorized_response(&e.to_string()),
            }
        }
    }
}

/// Validates a Bearer token and returns the AuthContext.
///
/// Uses a two-level caching strategy:
/// 1. **JWT Cache**: Check if token signature was recently verified (avoids crypto)
/// 2. **Auth Cache**: Check if auth context is cached by JTI (avoids DB queries)
///
/// Cache lookup flow:
/// 1. Check JWT cache by token hash -> get verified claims if cached
/// 2. If not cached, decode JWT (signature verification)
/// 3. Check auth cache by JTI -> return cached context if found
/// 4. Check revocation and load client/user from DB
/// 5. Cache the result for future requests
async fn validate_token(
    state: &AuthState,
    auth_cache: &Arc<dyn AuthContextCache>,
    jwt_cache: &Arc<JwtVerificationCache>,
    token: &str,
) -> Result<Arc<AuthContext>, octofhir_auth::AuthError> {
    // Try JWT verification cache first (avoids expensive signature verification)
    let claims = if let Some(cached_claims) = jwt_cache.get(token) {
        tracing::trace!(jti = %cached_claims.jti, "JWT verification cache hit");
        cached_claims
    } else {
        // Cache miss - perform full JWT decode with signature verification
        // Use spawn_blocking to avoid blocking the tokio runtime with CPU-intensive
        // cryptographic operations (RS256/ES384 signature verification takes 1-3ms)
        let decoded_claims = {
            let jwt_service = Arc::clone(&state.jwt_service);
            let token_string = token.to_string();
            tokio::task::spawn_blocking(move || {
                jwt_service.decode::<AccessTokenClaims>(&token_string)
            })
            .await
            .map_err(|_| octofhir_auth::AuthError::internal("JWT decode task panicked"))?
            .map_err(|e| octofhir_auth::AuthError::invalid_token(e.to_string()))?
        };
        let decoded_claims = Arc::new(decoded_claims.claims);

        // Cache the verified claims for future requests
        jwt_cache.insert(token, decoded_claims.clone());
        tracing::trace!(jti = %decoded_claims.jti, "JWT verification cache miss - cached");

        decoded_claims
    };

    // Check expiration
    let now = time::OffsetDateTime::now_utc().unix_timestamp();
    if claims.exp < now {
        return Err(octofhir_auth::AuthError::TokenExpired);
    }

    // Check auth context cache (avoids DB queries)
    if let Some(cached_ctx) = auth_cache.get(&claims.jti).await {
        tracing::trace!(jti = %claims.jti, "Auth context cache hit");
        return Ok(cached_ctx);
    }

    // Cache miss - perform DB lookups
    tracing::trace!(jti = %claims.jti, "Auth context cache miss");

    // Check revocation
    if state.revoked_token_storage.is_revoked(&claims.jti).await? {
        return Err(octofhir_auth::AuthError::TokenRevoked);
    }

    // Load client
    let client = state
        .client_storage
        .find_by_client_id(&claims.client_id)
        .await?
        .ok_or_else(|| octofhir_auth::AuthError::invalid_token("Unknown client"))?;

    if !client.active {
        return Err(octofhir_auth::AuthError::invalid_token(
            "Client is inactive",
        ));
    }

    // Load user context if subject is a UUID
    let user = if let Ok(user_id) = uuid::Uuid::parse_str(&claims.sub) {
        match state.user_storage.find_by_id(user_id).await? {
            Some(user) if user.active => Some(octofhir_auth::middleware::UserContext {
                id: user.id,
                username: user.username,
                fhir_user: user.fhir_user.or_else(|| claims.fhir_user.clone()),
                roles: user.roles,
                attributes: user.attributes,
            }),
            Some(_) => {
                return Err(octofhir_auth::AuthError::invalid_token("User is inactive"));
            }
            None => None,
        }
    } else {
        None
    };

    // Build auth context - Arc::clone is cheap (just reference counting)
    let jti = claims.jti.clone(); // Clone jti before moving claims
    let auth_context = AuthContext {
        patient: claims.patient.clone(),
        encounter: claims.encounter.clone(),
        token_claims: claims, // Move Arc, no clone of AccessTokenClaims
        client,
        user,
    };

    // Cache the result and get Arc back (avoids extra allocation)
    let arc_context = auth_cache.insert(jti, auth_context).await;

    Ok(arc_context)
}

/// Extract token from cookie if cookie auth is enabled.
fn extract_token_from_cookie(req: &Request<Body>, cookie_config: &CookieConfig) -> Option<String> {
    // Only try cookie auth if enabled
    if !cookie_config.enabled {
        return None;
    }

    // Get Cookie header
    let cookie_header = req.headers().get(COOKIE)?.to_str().ok()?;

    // Parse cookies (simple key=value; key=value format)
    let cookie_name = &cookie_config.name;
    for cookie in cookie_header.split(';') {
        let cookie = cookie.trim();
        if let Some((name, value)) = cookie.split_once('=') {
            if name.trim() == cookie_name {
                let value = value.trim();
                if !value.is_empty() {
                    tracing::debug!(cookie_name = %cookie_name, "Token extracted from cookie");
                    return Some(value.to_string());
                }
            }
        }
    }

    None
}

/// Check if a request should skip authentication.
///
/// Uses the operation registry's public paths cache as the primary source of truth.
/// Static file paths (/ui) are handled separately as they're not in the registry.
fn should_skip_authentication(req: &Request<Body>, registry: &OperationRegistryService) -> bool {
    let path = req.uri().path();

    // Check the operation registry for public endpoints (O(1) in-memory lookup)
    // This includes /metadata, /healthz, /readyz, /metrics, /.well-known/*, /auth/*, etc.
    if registry.is_path_public(path) {
        tracing::debug!(path = %path, "Skipping authentication: public operation from registry");
        return true;
    }

    // UI paths are always public (not in operation registry as they're static files)
    if path.starts_with("/ui") {
        tracing::debug!(path = %path, "Skipping authentication: static UI path");
        return true;
    }

    false
}

// =============================================================================
// Other Middleware
// =============================================================================

/// Prometheus metrics middleware.
///
/// Records HTTP request metrics including:
/// - Request count by method, path, and status
/// - Request duration histogram
/// - Active connection gauge
///
/// Excludes noisy endpoints (health checks, metrics, favicon) from recording.
pub async fn metrics_middleware(req: Request<Body>, next: Next) -> Response {
    use crate::metrics;
    use std::time::Instant;

    let path = req.uri().path();

    // Skip metrics recording for noisy endpoints
    if should_skip_metrics(path) {
        return next.run(req).await;
    }

    let method = req.method().to_string();
    let path = path.to_string();
    let start = Instant::now();

    // Track active connections
    metrics::increment_active_connections();

    // Execute the request
    let response = next.run(req).await;

    // Record metrics
    let status = response.status().as_u16();
    let duration = start.elapsed();

    metrics::record_http_request(&method, &path, status, duration);
    metrics::decrement_active_connections();

    response
}

/// Check if metrics should be skipped for this path.
///
/// Excludes high-frequency infrastructure endpoints that would create noise:
/// - Health checks (/healthz, /readyz, /livez, /api/health)
/// - Metrics endpoint (/metrics) - avoid self-referential metrics
/// - Favicon (/favicon.ico) - browser noise
#[inline]
fn should_skip_metrics(path: &str) -> bool {
    matches!(
        path,
        "/healthz"
            | "/readyz"
            | "/livez"
            | "/metrics"
            | "/favicon.ico"
            | "/api/health"
    )
}

// Middleware that ensures each request has an X-Request-Id and mirrors it on the response
pub async fn request_id(mut req: Request<Body>, next: Next) -> Response {
    let header_name = HeaderName::from_static("x-request-id");

    // If the incoming request already has a request-id, preserve it; otherwise generate one
    let req_id_value = req
        .headers()
        .get(&header_name)
        .cloned()
        .unwrap_or_else(|| HeaderValue::from_str(&Uuid::new_v4().to_string()).unwrap());

    // Add to request extensions for downstream usage (e.g., logging)
    req.extensions_mut().insert(req_id_value.clone());

    let mut res = next.run(req).await;

    // Add/propagate the request id header to response
    res.headers_mut().insert(header_name.clone(), req_id_value);

    res
}

// Content negotiation middleware: accept FHIR JSON, plain JSON, and SSE for Accept,
// and require JSON for POST/PUT Content-Type.
//
// OAuth endpoints are excluded as they use application/x-www-form-urlencoded per RFC 6749.
pub async fn content_negotiation(req: Request<Body>, next: Next) -> Response {
    let path = req.uri().path();

    // Skip content negotiation for OAuth endpoints (they use form-urlencoded per RFC 6749)
    if path.starts_with("/oauth/") || path.starts_with("/auth/") {
        return next.run(req).await;
    }

    let accepts_hdr = req.headers().get("accept").and_then(|v| v.to_str().ok());
    let accept_ok = accepts_hdr
        .map(|v| {
            let v = v.to_ascii_lowercase();
            v.contains("application/fhir+json")
                || v.contains("application/json")
                || v.contains("text/event-stream") // For SSE endpoints
                || v.contains("*/*")
        })
        .unwrap_or(true); // if missing, treat as ok per HTTP defaults

    if !accept_ok {
        return error_response(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "Only JSON is supported (application/fhir+json or application/json) in Accept",
        );
    }

    let method = req.method().clone();
    let needs_body_type = method == axum::http::Method::POST || method == axum::http::Method::PUT;

    if needs_body_type {
        let content_type = req
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_ascii_lowercase());
        let content_ok = content_type
            .as_deref()
            .map(|s| s.starts_with("application/fhir+json") || s.starts_with("application/json"))
            .unwrap_or(false);
        if !content_ok {
            return error_response(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "Content-Type must be application/fhir+json or application/json",
            );
        }
    }

    next.run(req).await
}

fn error_response(status: StatusCode, msg: &str) -> Response {
    let body: Value = json!({
        "resourceType": "OperationOutcome",
        "issue": [{
            "severity": "error",
            "code": "invalid",
            "diagnostics": msg,
        }]
    });
    (status, Json(body)).into_response()
}

/// Helper function to check if the request has the `Prefer: respond-async` header.
///
/// This is used to detect when a client wants to use the FHIR asynchronous request pattern
/// for long-running operations.
///
/// ## FHIR Specification
/// Per FHIR spec, the client sends:
/// ```
/// Prefer: respond-async
/// ```
///
/// The server should then:
/// 1. Return 202 Accepted
/// 2. Include `Content-Location` header pointing to the status endpoint
/// 3. Process the request asynchronously
pub fn has_prefer_async_header(req: &Request<Body>) -> bool {
    if let Some(prefer) = req.headers().get("prefer")
        && let Ok(prefer_str) = prefer.to_str()
    {
        return prefer_str
            .split(',')
            .any(|v| v.trim().eq_ignore_ascii_case("respond-async"));
    }
    false
}

// =============================================================================
// Authorization Middleware
// =============================================================================

/// State required for authorization middleware.
#[derive(Clone)]
pub struct AuthorizationState {
    /// Policy evaluator for access control.
    pub policy_evaluator: Arc<PolicyEvaluator>,
    /// Operation registry for public path lookups.
    pub operation_registry: Arc<OperationRegistryService>,
}

impl AuthorizationState {
    /// Creates a new authorization state.
    #[must_use]
    pub fn new(
        policy_evaluator: Arc<PolicyEvaluator>,
        operation_registry: Arc<OperationRegistryService>,
    ) -> Self {
        Self {
            policy_evaluator,
            operation_registry,
        }
    }
}

/// Authorization middleware that enforces policy-based access control.
///
/// This middleware:
/// 1. Checks if the path should skip authorization (public endpoints from registry)
/// 2. Extracts the `AuthContext` from request extensions
/// 3. Builds a `PolicyContext` from the request
/// 4. Evaluates policies using the `PolicyEvaluator`
/// 5. Returns 403 Forbidden if access is denied
///
/// # Requirements
///
/// - `AuthContext` must be present in request extensions (set by `BearerAuth` extractor)
/// - `AuthorizationState` must be available via `FromRef<S>`
pub async fn authorization_middleware(
    State(state): State<AuthorizationState>,
    req: Request<Body>,
    next: Next,
) -> Response {
    // Skip authorization for public endpoints (from operation registry)
    if should_skip_authorization(&req, &state.operation_registry) {
        return next.run(req).await;
    }

    // Get auth context from request extensions (set by authentication_middleware)
    // Now stored as Arc<AuthContext> for cheap cloning
    let auth_context = match req.extensions().get::<Arc<AuthContext>>() {
        Some(ctx) => Arc::clone(ctx),
        None => {
            // No auth context means not authenticated - return 401
            return unauthorized_response("Authentication required");
        }
    };

    // Build policy context from request
    // Deref Arc to get &AuthContext
    let policy_context = match build_policy_context(&req, &*auth_context) {
        Ok(ctx) => ctx,
        Err(e) => {
            tracing::error!(error = %e, "Failed to build policy context");
            return internal_error_response("Policy evaluation failed");
        }
    };

    // Evaluate policies
    let decision = state.policy_evaluator.evaluate(&policy_context).await;

    match decision {
        AccessDecision::Allow => {
            // Store policy context for use by handlers (e.g., search filtering)
            let mut req = req;
            req.extensions_mut().insert(policy_context);
            next.run(req).await
        }
        AccessDecision::Deny(reason) => {
            tracing::info!(
                reason = %reason.message,
                code = %reason.code,
                user = ?auth_context.user.as_ref().map(|u| &u.username),
                path = %req.uri().path(),
                "Access denied"
            );
            forbidden_response(&reason)
        }
        AccessDecision::Abstain => {
            // Default deny if all policies abstain
            tracing::info!(
                user = ?auth_context.user.as_ref().map(|u| &u.username),
                path = %req.uri().path(),
                "Access denied: no matching policy"
            );
            forbidden_response(&DenyReason {
                code: "no-matching-policy".to_string(),
                message: "No policy matched this request".to_string(),
                details: None,
                policy_id: None,
            })
        }
    }
}

/// Check if a request should skip authorization.
///
/// Uses the operation registry's public paths cache to determine
/// if the endpoint is marked as public.
fn should_skip_authorization(req: &Request<Body>, registry: &OperationRegistryService) -> bool {
    let path = req.uri().path();

    // Check the operation registry for public endpoints (O(1) in-memory lookup)
    if registry.is_path_public(path) {
        tracing::debug!(path = %path, "Skipping authorization: public operation from registry");
        return true;
    }

    // UI paths are always public (not in operation registry as they're static files)
    if path.starts_with("/ui") {
        return true;
    }

    false
}

/// Build a PolicyContext from the request and auth context.
fn build_policy_context(
    req: &Request<Body>,
    auth: &AuthContext,
) -> Result<PolicyContext, octofhir_auth::policy::ContextError> {
    let method = req.method().as_str();
    let path = req.uri().path();

    // Parse query parameters using into_owned() to avoid allocation when Cow is already Owned
    let query_params: HashMap<String, String> = req
        .uri()
        .query()
        .map(|q| {
            url::form_urlencoded::parse(q.as_bytes())
                .map(|(k, v)| (k.into_owned(), v.into_owned()))
                .collect()
        })
        .unwrap_or_default();

    // Get request ID from headers if available
    let request_id = req
        .headers()
        .get("x-request-id")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("unknown")
        .to_string();

    // Build the policy context
    PolicyContextBuilder::new()
        .with_auth_context(auth)
        .with_request(method, path, query_params, None)
        .with_environment(request_id, None)
        .build()
}

/// Create an unauthorized (401) response with FHIR OperationOutcome.
fn unauthorized_response(message: &str) -> Response {
    let body = json!({
        "resourceType": "OperationOutcome",
        "issue": [{
            "severity": "error",
            "code": "login",
            "diagnostics": message
        }]
    });

    (
        StatusCode::UNAUTHORIZED,
        [("WWW-Authenticate", "Bearer")],
        Json(body),
    )
        .into_response()
}

/// Create a forbidden (403) response with FHIR OperationOutcome.
fn forbidden_response(reason: &DenyReason) -> Response {
    let body = json!({
        "resourceType": "OperationOutcome",
        "issue": [{
            "severity": "error",
            "code": "forbidden",
            "diagnostics": reason.message,
            "details": {
                "coding": [{
                    "system": "http://octofhir.io/CodeSystem/access-denied-reason",
                    "code": reason.code
                }]
            }
        }]
    });

    (StatusCode::FORBIDDEN, Json(body)).into_response()
}

/// Create an internal server error (500) response with FHIR OperationOutcome.
fn internal_error_response(message: &str) -> Response {
    let body = json!({
        "resourceType": "OperationOutcome",
        "issue": [{
            "severity": "error",
            "code": "exception",
            "diagnostics": message
        }]
    });

    (StatusCode::INTERNAL_SERVER_ERROR, Json(body)).into_response()
}

// =============================================================================
// Audit Middleware
// =============================================================================

/// Audit middleware that logs FHIR operations to the audit trail.
///
/// This middleware:
/// 1. Captures request information (method, path, auth context)
/// 2. Passes the request to the next handler
/// 3. Logs an audit event based on the response status
///
/// Audit events are logged asynchronously to avoid blocking the response.
pub async fn audit_middleware(
    State(state): State<crate::server::AppState>,
    req: Request<Body>,
    next: Next,
) -> Response {
    use crate::audit::{
        action_from_request, actor_from_auth_context, outcome_from_status, parse_fhir_path,
        AuditSource,
    };

    // Extract request information before passing to handler
    let method = req.method().clone();
    let path = req.uri().path().to_string();

    // Extract only needed headers for audit (avoid cloning entire HeaderMap ~500-1000 bytes)
    let ip_address: Option<std::net::IpAddr> = req
        .headers()
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.split(',').next())
        .or_else(|| {
            req.headers()
                .get("x-real-ip")
                .and_then(|v| v.to_str().ok())
        })
        .and_then(|s| s.trim().parse().ok());
    let user_agent = req
        .headers()
        .get(axum::http::header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .map(String::from);
    let request_id = req
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .map(String::from);

    // Extract auth context if present
    // Auth context is stored as Arc<AuthContext> for cheap cloning
    let auth_context = req.extensions().get::<Arc<AuthContext>>().cloned();

    // Determine if this is an auditable FHIR operation
    let audit_action = action_from_request(&method, &path);

    // Call the next handler
    let response = next.run(req).await;

    // Log audit event if this is an auditable action
    if let Some(action) = audit_action {
        let status = response.status();
        let outcome = outcome_from_status(status);
        let (resource_type, resource_id) = parse_fhir_path(&path);

        // Check if this is an auth action
        let is_auth_action = matches!(
            action,
            crate::audit::AuditAction::UserLogin
                | crate::audit::AuditAction::UserLogout
                | crate::audit::AuditAction::UserLoginFailed
                | crate::audit::AuditAction::ClientAuth
        );

        // Check if we should log this action
        let should_log = if is_auth_action {
            state.audit_service.should_log(&action, None)
        } else if let Some(rt) = resource_type.as_deref() {
            state.audit_service.should_log(&action, Some(rt))
        } else {
            false
        };

        if should_log {
            let audit_service = state.audit_service.clone();
            // Build AuditSource from pre-extracted headers (avoids HeaderMap clone)
            let source = AuditSource {
                ip_address,
                user_agent,
                site: None,
            };
            // Deref Arc to get &AuthContext for actor_from_auth_context
            let actor = auth_context.as_ref().map(|ctx| actor_from_auth_context(&**ctx));
            let session_id = auth_context
                .as_ref()
                .and_then(|ctx| ctx.token_claims.sid.clone());

            if is_auth_action {
                // Log auth event
                tokio::spawn(async move {
                    // Determine auth outcome - 4xx = login failure
                    let auth_outcome = if status.is_client_error() {
                        crate::audit::AuditOutcome::MinorFailure
                    } else {
                        outcome
                    };
                    let action_type = if status.is_client_error()
                        && matches!(action, crate::audit::AuditAction::UserLogin)
                    {
                        crate::audit::AuditAction::UserLoginFailed
                    } else {
                        action
                    };

                    if let Err(e) = audit_service
                        .log_auth_event(
                            action_type,
                            auth_outcome,
                            None,
                            actor.as_ref().and_then(|a| match a {
                                crate::audit::ActorType::User { id, .. } => Some(*id),
                                _ => None,
                            }),
                            actor.as_ref().and_then(|a| match a {
                                crate::audit::ActorType::User { name, .. } => {
                                    name.as_ref().map(|s| s.as_str())
                                }
                                _ => None,
                            }),
                            actor.as_ref().and_then(|a| match a {
                                crate::audit::ActorType::Client { id, .. } => Some(id.as_str()),
                                _ => None,
                            }),
                            &source,
                            request_id.as_deref(),
                            session_id.as_deref(),
                        )
                        .await
                    {
                        tracing::debug!(
                            error = %e,
                            "Failed to log auth audit event"
                        );
                    }
                });
            } else {
                // Log FHIR operation
                let resource_type_owned = resource_type.unwrap_or_default();
                let resource_id_owned = resource_id.clone();
                tokio::spawn(async move {
                    if let Err(e) = audit_service
                        .log_fhir_operation(
                            action,
                            outcome,
                            &resource_type_owned,
                            resource_id_owned.as_deref(),
                            None,
                            actor.as_ref(),
                            &source,
                            request_id.as_deref(),
                            session_id.as_deref(),
                        )
                        .await
                    {
                        tracing::debug!(
                            error = %e,
                            resource_type = %resource_type_owned,
                            "Failed to log FHIR audit event"
                        );
                    }
                });
            }
        }
    }

    response
}
