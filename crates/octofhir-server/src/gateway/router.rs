//! Gateway router for handling dynamic API endpoints.

use std::collections::HashMap;
use std::sync::Arc;

use axum::{
    Router,
    body::Body,
    extract::{FromRequestParts, Path as AxumPath, State, ws::WebSocketUpgrade},
    http::Request,
    response::Response,
    routing::any,
};
use tokio::sync::RwLock;
use tracing::{debug, info, instrument};

use crate::server::AppState;
use octofhir_storage::{DynStorage, SearchParams};

use super::error::GatewayError;
use super::types::{App, CustomOperation, InlineOperation, ProxyConfig, Reference, RouteKey};

/// Gateway router that dynamically routes requests based on App and CustomOperation resources.
#[derive(Clone)]
pub struct GatewayRouter {
    /// In-memory cache of routes (method:path -> CustomOperation).
    routes: Arc<RwLock<HashMap<String, CustomOperation>>>,

    /// HTTP client for proxy requests.
    http_client: reqwest::Client,
}

impl GatewayRouter {
    /// Creates a new GatewayRouter.
    pub fn new() -> Self {
        Self {
            routes: Arc::new(RwLock::new(HashMap::new())),
            http_client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("Failed to create HTTP client"),
        }
    }

    /// Returns the HTTP client for making proxy requests.
    pub fn http_client(&self) -> &reqwest::Client {
        &self.http_client
    }

    /// Reloads routes from storage by loading all active Apps and CustomOperations.
    ///
    /// This function:
    /// 1. Loads all active App resources
    /// 2. Processes inline operations from App.operations[]
    /// 3. Loads all active standalone CustomOperation resources
    /// 4. Builds a map of route keys (method:path) to operations
    /// 5. Updates the in-memory cache
    #[instrument(skip(self, storage))]
    pub async fn reload_routes(&self, storage: &DynStorage) -> Result<usize, GatewayError> {
        info!("Reloading gateway routes from storage");

        // Load all Apps
        let search_params = SearchParams::new().with_count(1000);
        let apps_result = storage
            .search("App", &search_params)
            .await
            .map_err(|e| GatewayError::StorageError(format!("Failed to load Apps: {}", e)))?;

        let apps: Vec<App> = apps_result
            .entries
            .into_iter()
            .filter_map(|stored| serde_json::from_value(stored.resource).ok())
            .collect();

        debug!(count = apps.len(), "Loaded Apps");

        // Build a map of app ID -> App for quick lookup
        let app_map: HashMap<String, App> = apps
            .into_iter()
            .filter_map(|app| app.id.clone().map(|id| (id, app)))
            .collect();

        // Build route map
        let mut routes = HashMap::new();

        // Process inline operations from active Apps
        for (app_id, app) in &app_map {
            // Skip inactive apps
            if !app.is_active() {
                debug!(app_id = %app_id, "Skipping inactive app");
                continue;
            }

            // Register inline operations
            for inline_op in &app.operations {
                let custom_op = self.inline_to_custom_operation(app, inline_op);
                let full_path = inline_op.path_string();
                let route_key = RouteKey::new(inline_op.method.to_string(), full_path.clone());

                debug!(
                    route_key = %route_key,
                    app_id = %app_id,
                    operation_id = %inline_op.id,
                    "Registered inline operation"
                );

                routes.insert(route_key.to_string(), custom_op);
            }
        }

        // Load all active standalone CustomOperations
        let ops_result = storage
            .search("CustomOperation", &search_params)
            .await
            .map_err(|e| {
                GatewayError::StorageError(format!("Failed to load CustomOperations: {}", e))
            })?;

        let operations: Vec<CustomOperation> = ops_result
            .entries
            .into_iter()
            .filter_map(|stored| serde_json::from_value(stored.resource).ok())
            .collect();

        debug!(
            count = operations.len(),
            "Loaded standalone CustomOperations"
        );

        // Process standalone CustomOperations
        for operation in operations {
            // Skip inactive operations
            if !operation.active {
                continue;
            }

            // Extract app reference
            let app_ref = operation.app.reference.as_ref().ok_or_else(|| {
                GatewayError::InvalidConfig(format!(
                    "CustomOperation {} has no app reference",
                    operation.id.as_deref().unwrap_or("unknown")
                ))
            })?;

            // Extract app ID from reference (e.g., "App/123" -> "123")
            let app_id = app_ref.split('/').next_back().ok_or_else(|| {
                GatewayError::InvalidConfig(format!("Invalid app reference: {}", app_ref))
            })?;

            // Find the app
            let app = app_map.get(app_id);

            // Build full path (support both new and deprecated formats)
            let full_path = if let Some(app) = app {
                if let Some(base_path) = &app.base_path {
                    // Deprecated: use App.base_path
                    format!("{}{}", base_path, operation.path)
                } else {
                    // New: path is already absolute
                    operation.path.clone()
                }
            } else {
                // App not found, use path as-is
                operation.path.clone()
            };

            // Create route key
            let route_key = RouteKey::new(operation.method.clone(), full_path.clone());

            debug!(
                route_key = %route_key,
                operation_type = %operation.operation_type,
                "Registered standalone operation"
            );

            routes.insert(route_key.to_string(), operation);
        }

        let count = routes.len();

        // Update routes atomically
        let mut current_routes = self.routes.write().await;
        *current_routes = routes;

        info!(count = count, "Gateway routes reloaded");

        Ok(count)
    }

    /// Converts an inline operation from App.operations[] to a CustomOperation.
    fn inline_to_custom_operation(&self, app: &App, op: &InlineOperation) -> CustomOperation {
        // Build proxy config based on operation type
        let proxy = if op.operation_type == "websocket" {
            // WebSocket operations use websocket config
            Some(ProxyConfig {
                url: String::new(), // Not used for websocket
                timeout: None,
                forward_auth: None,
                headers: None,
                websocket: op.websocket.clone(),
            })
        } else {
            // Regular app operations use endpoint config
            app.endpoint.as_ref().map(|ep| ProxyConfig {
                url: ep.url.clone(),
                timeout: ep.timeout,
                forward_auth: Some(true),
                headers: None,
                websocket: None,
            })
        };

        CustomOperation {
            id: Some(format!(
                "{}-{}",
                app.id.as_deref().unwrap_or("unknown"),
                op.id
            )),
            resource_type: "CustomOperation".to_string(),
            app: Reference {
                reference: Some(format!("App/{}", app.id.as_deref().unwrap_or(""))),
                display: Some(app.name.clone()),
            },
            path: op.path_string(),
            method: op.method.to_string(),
            operation_type: op.operation_type.clone(),
            active: true,
            public: op.public,
            policy: op.policy.clone(),
            proxy,
            sql: None,
            fhirpath: None,
            handler: None,
            include_raw_body: op.include_raw_body,
        }
    }

    /// Creates an Axum router for the gateway.
    ///
    /// The router uses a catch-all `/{*path}` route as a fallback for any path
    /// not matched by explicit routes. This allows users to define custom operations
    /// on any path (not just `/api/*`).
    ///
    /// **Important**: This router should be merged LAST in the router chain so that
    /// explicit routes are matched first.
    pub fn create_router() -> Router<AppState> {
        Router::new().route("/{*path}", any(gateway_handler))
    }

    /// Looks up a route by method and path.
    ///
    /// Supports path parameters in the format `:param` (e.g., `/users/:id/profile`).
    /// First tries exact match, then falls back to pattern matching.
    pub async fn get_route(&self, method: &str, path: &str) -> Option<CustomOperation> {
        let routes = self.routes.read().await;

        // First try exact match (faster for static routes)
        let exact_key = RouteKey::new(method.to_string(), path.to_string());
        if let Some(op) = routes.get(&exact_key.to_string()) {
            return Some(op.clone());
        }

        // Fall back to pattern matching for routes with parameters
        let request_segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();

        for (key, operation) in routes.iter() {
            // Parse route key "METHOD:/path/to/resource"
            if let Some((route_method, route_path)) = key.split_once(':') {
                // Method must match exactly
                if route_method != method {
                    continue;
                }

                // Check path pattern match
                if path_matches_pattern(route_path, &request_segments) {
                    return Some(operation.clone());
                }
            }
        }

        None
    }
}

impl Default for GatewayRouter {
    fn default() -> Self {
        Self::new()
    }
}

/// Checks if a request path matches a route pattern with parameters.
///
/// Route patterns use `:param` syntax for dynamic segments.
/// E.g., `/users/:id/profile` matches `/users/123/profile`.
fn path_matches_pattern(route_path: &str, request_segments: &[&str]) -> bool {
    let route_segments: Vec<&str> = route_path.split('/').filter(|s| !s.is_empty()).collect();

    // Must have same number of segments
    if route_segments.len() != request_segments.len() {
        return false;
    }

    // Check each segment
    for (route_seg, req_seg) in route_segments.iter().zip(request_segments.iter()) {
        // Path parameters start with ':'
        if route_seg.starts_with(':') {
            // Parameter matches any non-empty value
            continue;
        }

        // Static segments must match exactly
        if route_seg != req_seg {
            return false;
        }
    }

    true
}

/// Gateway handler for routed requests (used with `/{*path}` route).
#[axum::debug_handler]
async fn gateway_handler(
    State(state): State<AppState>,
    AxumPath(path): AxumPath<String>,
    request: Request<Body>,
) -> Result<Response, GatewayError> {
    let method = request.method().as_str();
    let full_path = format!("/{}", path);

    debug!(method = method, path = %full_path, "Gateway routed request");
    gateway_dispatch(&state, &full_path, request).await
}

/// Gateway fallback handler for unmatched requests.
///
/// This is used as a `.fallback()` handler when no explicit route matches.
/// It looks up custom operations defined by users. If no matching custom
/// operation is found, it returns 404.
#[axum::debug_handler]
pub async fn gateway_fallback_handler(
    State(state): State<AppState>,
    request: Request<Body>,
) -> Result<Response, GatewayError> {
    let full_path = request.uri().path().to_string();
    debug!(path = %full_path, "Gateway fallback request");
    gateway_dispatch(&state, &full_path, request).await
}

/// Internal dispatch function used by both routed and fallback handlers.
async fn gateway_dispatch(
    state: &AppState,
    full_path: &str,
    request: Request<Body>,
) -> Result<Response, GatewayError> {
    let method = request.method().as_str();

    debug!(method = method, path = %full_path, "Gateway dispatch");

    // Look up the route
    let operation = state
        .gateway_router
        .get_route(method, full_path)
        .await
        .ok_or_else(|| GatewayError::RouteNotFound {
            method: method.to_string(),
            path: full_path.to_string(),
        })?;

    // Authenticate based on policy.authType
    // - authType="app": validate X-App-Secret header
    // - authType="forward" (default): OAuth token handled by middleware
    if let Some(policy) = &operation.policy {
        use crate::app_platform::AuthType;
        let auth_type = policy.auth_type.unwrap_or(AuthType::Forward);
        if auth_type == AuthType::App {
            super::auth::authenticate_app_operation(&operation, request.headers(), state).await?;
        }
    }

    // Extract auth context from request extensions
    let auth_context = request
        .extensions()
        .get::<std::sync::Arc<octofhir_auth::middleware::AuthContext>>()
        .cloned();

    // Evaluate policy BEFORE dispatching to handler
    super::policy::evaluate_operation_policy(&operation, auth_context.as_ref(), state).await?;

    info!(
        method = method,
        path = %full_path,
        operation_type = %operation.operation_type,
        "Routing to operation handler"
    );

    // Dispatch to the appropriate handler based on operation type
    match operation.operation_type.as_str() {
        "proxy" => super::proxy::handle_proxy(state, &operation, request).await,
        "app" => super::app::handle_app(state, &operation, request).await,
        "sql" => super::sql::handle_sql(state, &operation, request).await,
        "fhirpath" => super::fhirpath::handle_fhirpath(state, &operation, request).await,
        "handler" => super::handler::handle_handler(state.clone(), &operation, request).await,
        "websocket" => {
            // WebSocket operations require a WebSocket upgrade request.
            // Extract WebSocketUpgrade from request parts.
            let (mut parts, _body) = request.into_parts();

            // Extract query string from original request to forward to backend
            let original_query = parts.uri.query().map(String::from);

            let ws = WebSocketUpgrade::from_request_parts(&mut parts, state)
                .await
                .map_err(|_| {
                    GatewayError::BadRequest(
                        "WebSocket operation requires WebSocket upgrade request".to_string(),
                    )
                })?;

            // Build auth info for the backend
            let auth_info = super::types::AuthInfo::from_auth_context(
                auth_context.as_ref().map(|arc| arc.as_ref()),
            );

            debug!(
                fhir_user = ?auth_info.fhir_user,
                user_id = ?auth_info.user_id,
                authenticated = auth_info.authenticated,
                original_query = ?original_query,
                "WebSocket auth info"
            );

            super::websocket::handle_websocket(
                state,
                &operation,
                ws,
                &auth_info,
                original_query.as_deref(),
            )
            .await
        }
        unknown => Err(GatewayError::InvalidConfig(format!(
            "Unknown operation type: {}",
            unknown
        ))),
    }
}
