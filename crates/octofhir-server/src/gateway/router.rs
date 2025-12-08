//! Gateway router for handling dynamic API endpoints.

use std::collections::HashMap;
use std::sync::Arc;

use axum::{
    Router,
    body::Body,
    extract::{Path as AxumPath, State},
    http::Request,
    response::Response,
    routing::any,
};
use tokio::sync::RwLock;
use tracing::{debug, info, instrument};

use crate::server::AppState;
use octofhir_storage::legacy::{DynStorage, SearchQuery};

use super::error::GatewayError;
use super::types::{App, CustomOperation, RouteKey};

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
    /// 2. Loads all active CustomOperation resources
    /// 3. Builds a map of route keys (method:path) to operations
    /// 4. Updates the in-memory cache
    #[instrument(skip(self, storage))]
    pub async fn reload_routes(&self, storage: &DynStorage) -> Result<usize, GatewayError> {
        info!("Reloading gateway routes from storage");

        // Load all active Apps
        let app_query = SearchQuery::new("App".parse().unwrap());
        let apps_result = storage
            .search(&app_query)
            .await
            .map_err(|e| GatewayError::StorageError(format!("Failed to load Apps: {}", e)))?;

        let apps: Vec<App> = apps_result
            .resources
            .into_iter()
            .filter_map(|entry| {
                serde_json::to_value(&entry.data)
                    .ok()
                    .and_then(|v| serde_json::from_value(v).ok())
            })
            .collect();

        debug!(count = apps.len(), "Loaded active Apps");

        // Build a map of app ID -> App for quick lookup
        let app_map: HashMap<String, App> = apps
            .into_iter()
            .filter_map(|app| app.id.clone().map(|id| (id, app)))
            .collect();

        // Load all active CustomOperations
        let ops_query = SearchQuery::new("CustomOperation".parse().unwrap());
        let ops_result = storage.search(&ops_query).await.map_err(|e| {
            GatewayError::StorageError(format!("Failed to load CustomOperations: {}", e))
        })?;

        let operations: Vec<CustomOperation> = ops_result
            .resources
            .into_iter()
            .filter_map(|entry| {
                serde_json::to_value(&entry.data)
                    .ok()
                    .and_then(|v| serde_json::from_value(v).ok())
            })
            .collect();

        debug!(count = operations.len(), "Loaded active CustomOperations");

        // Build route map
        let mut routes = HashMap::new();

        for operation in operations {
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
            let app = app_map
                .get(app_id)
                .ok_or_else(|| GatewayError::InvalidConfig(format!("App not found: {}", app_id)))?;

            // Build full path
            let full_path = format!("{}{}", app.base_path, operation.path);

            // Create route key
            let route_key = RouteKey::new(operation.method.clone(), full_path.clone());

            debug!(
                route_key = %route_key,
                operation_type = %operation.operation_type,
                "Registered route"
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
    pub async fn get_route(&self, method: &str, path: &str) -> Option<CustomOperation> {
        let route_key = RouteKey::new(method.to_string(), path.to_string());
        let routes = self.routes.read().await;
        routes.get(&route_key.to_string()).cloned()
    }
}

impl Default for GatewayRouter {
    fn default() -> Self {
        Self::new()
    }
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

    info!(
        method = method,
        path = %full_path,
        operation_type = %operation.operation_type,
        "Routing to operation handler"
    );

    // Dispatch to the appropriate handler based on operation type
    match operation.operation_type.as_str() {
        "proxy" => super::proxy::handle_proxy(state, &operation, request).await,
        "sql" => super::sql::handle_sql(state, &operation, request).await,
        "fhirpath" => super::fhirpath::handle_fhirpath(state, &operation, request).await,
        "handler" => super::handler::handle_handler(Arc::new(state.clone()), &operation, request).await,
        unknown => Err(GatewayError::InvalidConfig(format!(
            "Unknown operation type: {}",
            unknown
        ))),
    }
}
