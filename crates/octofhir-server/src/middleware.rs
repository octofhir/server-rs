use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::State;
use axum::response::IntoResponse;
use axum::{
    Json,
    body::Body,
    http::{HeaderName, HeaderValue, Request, StatusCode, header::AUTHORIZATION},
    middleware::Next,
    response::Response,
};
use serde_json::{Value, json};
use uuid::Uuid;

use octofhir_auth::middleware::{AuthContext, AuthState};
use octofhir_auth::policy::{
    AccessDecision, DenyReason, PolicyContext, PolicyContextBuilder, PolicyEvaluator,
};
use octofhir_auth::token::jwt::AccessTokenClaims;

// =============================================================================
// Authentication Middleware
// =============================================================================

/// Authentication middleware that validates Bearer tokens and injects AuthContext.
///
/// This middleware:
/// 1. Checks if the path should skip authentication (public endpoints)
/// 2. Extracts and validates Bearer tokens
/// 3. Stores the `AuthContext` in request extensions for downstream use
///
/// If authentication fails, returns 401 Unauthorized with FHIR OperationOutcome.
/// If no token is present for protected routes, returns 401.
///
/// # Requirements
///
/// - `AuthState` must be available via `FromRef<S>`
pub async fn authentication_middleware(
    State(state): State<AuthState>,
    mut req: Request<Body>,
    next: Next,
) -> Response {
    // Skip authentication for public endpoints
    if should_skip_authentication(&req) {
        return next.run(req).await;
    }

    // Extract Authorization header
    let auth_header = match req.headers().get(AUTHORIZATION).and_then(|h| h.to_str().ok()) {
        Some(header) => header,
        None => {
            // No auth header - check if this route requires auth
            tracing::debug!(path = %req.uri().path(), "No Authorization header");
            return unauthorized_response("Authentication required");
        }
    };

    // Parse Bearer token
    let token = match auth_header.strip_prefix("Bearer ") {
        Some(t) if !t.is_empty() => t,
        _ => {
            return unauthorized_response("Invalid Authorization header format");
        }
    };

    // Validate token and build auth context
    match validate_token(&state, token).await {
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
                octofhir_auth::AuthError::TokenExpired => {
                    unauthorized_response("Token expired")
                }
                octofhir_auth::AuthError::TokenRevoked => {
                    unauthorized_response("Token revoked")
                }
                _ => unauthorized_response(&e.to_string()),
            }
        }
    }
}

/// Validates a Bearer token and returns the AuthContext.
async fn validate_token(
    state: &AuthState,
    token: &str,
) -> Result<AuthContext, octofhir_auth::AuthError> {
    // Decode and validate JWT
    let claims = state
        .jwt_service
        .decode::<AccessTokenClaims>(token)
        .map_err(|e| octofhir_auth::AuthError::invalid_token(e.to_string()))?
        .claims;

    // Check expiration
    let now = time::OffsetDateTime::now_utc().unix_timestamp();
    if claims.exp < now {
        return Err(octofhir_auth::AuthError::TokenExpired);
    }

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
        return Err(octofhir_auth::AuthError::invalid_token("Client is inactive"));
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

    Ok(AuthContext {
        patient: claims.patient.clone(),
        encounter: claims.encounter.clone(),
        token_claims: claims,
        client,
        user,
    })
}

/// Check if a request should skip authentication.
fn should_skip_authentication(req: &Request<Body>) -> bool {
    let path = req.uri().path();

    // Public endpoints that don't require authentication
    let public_paths = [
        "/metadata",
        "/healthz",
        "/readyz",
        "/health",
        "/ready",
        "/",
        "/favicon.ico",
    ];

    // Check exact matches
    if public_paths.contains(&path) {
        return true;
    }

    // Check prefix matches
    let public_prefixes = [
        "/.well-known/",
        "/auth/",
        "/api/health",
        "/api/build-info",
        "/ui",
    ];

    public_prefixes.iter().any(|prefix| path.starts_with(prefix))
}

// =============================================================================
// Other Middleware
// =============================================================================

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

// Content negotiation middleware: accept FHIR JSON and plain JSON for Accept,
// and require one of them for POST/PUT Content-Type.
pub async fn content_negotiation(req: Request<Body>, next: Next) -> Response {
    let accepts_hdr = req.headers().get("accept").and_then(|v| v.to_str().ok());
    let accept_ok = accepts_hdr
        .map(|v| {
            let v = v.to_ascii_lowercase();
            v.contains("application/fhir+json")
                || v.contains("application/json")
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
}

impl AuthorizationState {
    /// Creates a new authorization state.
    #[must_use]
    pub fn new(policy_evaluator: Arc<PolicyEvaluator>) -> Self {
        Self { policy_evaluator }
    }
}

/// Authorization middleware that enforces policy-based access control.
///
/// This middleware:
/// 1. Checks if the path should skip authorization (public endpoints)
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
    // Skip authorization for public endpoints
    if should_skip_authorization(&req) {
        return next.run(req).await;
    }

    // Get auth context from request extensions (set by BearerAuth)
    let auth_context = match req.extensions().get::<AuthContext>() {
        Some(ctx) => ctx.clone(),
        None => {
            // No auth context means not authenticated - return 401
            return unauthorized_response("Authentication required");
        }
    };

    // Build policy context from request
    let policy_context = match build_policy_context(&req, &auth_context) {
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
fn should_skip_authorization(req: &Request<Body>) -> bool {
    let path = req.uri().path();

    // Public endpoints that don't require authorization
    let public_paths = [
        "/metadata",
        "/healthz",
        "/readyz",
        "/health",
        "/ready",
        "/",
        "/favicon.ico",
    ];

    // Check exact matches
    if public_paths.contains(&path) {
        return true;
    }

    // Check prefix matches
    let public_prefixes = ["/.well-known/", "/auth/", "/api/health", "/ui"];

    public_prefixes
        .iter()
        .any(|prefix| path.starts_with(prefix))
}

/// Build a PolicyContext from the request and auth context.
fn build_policy_context(
    req: &Request<Body>,
    auth: &AuthContext,
) -> Result<PolicyContext, octofhir_auth::policy::ContextError> {
    let method = req.method().as_str();
    let path = req.uri().path();

    // Parse query parameters
    let query_params: HashMap<String, String> = req
        .uri()
        .query()
        .map(|q| {
            url::form_urlencoded::parse(q.as_bytes())
                .map(|(k, v)| (k.to_string(), v.to_string()))
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
