//! WebSocket handler for PostgreSQL LSP with authentication.
//!
//! Uses direct Tower service calls instead of duplex stream bridging
//! for more reliable WebSocket-to-LSP communication.

use axum::{
    extract::{Query, State, WebSocketUpgrade},
    http::{HeaderMap, StatusCode, header::COOKIE},
    response::{IntoResponse, Response},
};
use futures_util::{SinkExt, StreamExt};
use octofhir_auth::token::jwt::AccessTokenClaims;
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::Mutex;
use tower::{Service, ServiceExt};
use tower_lsp::LspService;
use tower_lsp::jsonrpc::Request;

use crate::server::AppState;

use super::PostgresLspServer;

/// Query parameters for LSP WebSocket connection.
#[derive(Debug, Deserialize)]
pub struct LspQueryParams {
    /// Authentication token (required)
    pub token: Option<String>,
}

/// WebSocket handler for PostgreSQL LSP.
///
/// Allows authentication via the `token` query parameter or the HttpOnly auth cookie.
pub async fn lsp_websocket_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Query(params): Query<LspQueryParams>,
    headers: HeaderMap,
) -> Result<Response, LspError> {
    let auth_state = state
        .auth_state
        .as_ref()
        .ok_or(LspError::AuthNotConfigured)?;

    // Prefer explicit query token, but fall back to the auth cookie for UI sessions.
    let token = params
        .token
        .or_else(|| {
            if !auth_state.cookie_config.enabled {
                return None;
            }

            extract_cookie_token(&headers, auth_state.cookie_config.name.as_str())
        })
        .ok_or(LspError::Unauthorized)?;

    // Decode and validate JWT
    let _claims = auth_state
        .jwt_service
        .decode::<AccessTokenClaims>(&token)
        .map_err(|e| {
            tracing::warn!(error = %e, "LSP auth failed: invalid token");
            LspError::Unauthorized
        })?;

    // TODO: Check db_console:access permission from claims/policies
    // For now, any valid token grants access

    tracing::info!("LSP WebSocket connection authenticated");

    // Upgrade to WebSocket
    Ok(ws.on_upgrade(move |socket| {
        handle_lsp_connection(socket, state.db_pool, state.model_provider.clone())
    }))
}

fn extract_cookie_token(headers: &HeaderMap, cookie_name: &str) -> Option<String> {
    let cookie_header = headers.get(COOKIE)?.to_str().ok()?;

    for cookie in cookie_header.split(';') {
        let cookie = cookie.trim();
        if let Some((name, value)) = cookie.split_once('=') {
            if name.trim() == cookie_name {
                let value = value.trim();
                if !value.is_empty() {
                    return Some(value.to_string());
                }
            }
        }
    }

    None
}

/// Handle the LSP connection over WebSocket.
///
/// Uses direct Tower service calls instead of duplex stream bridging.
/// This approach is more reliable and follows the same pattern Supabase
/// uses in their postgres-language-server tests.
///
/// Includes periodic ping to keep connection alive and reduce log verbosity.
async fn handle_lsp_connection(
    socket: axum::extract::ws::WebSocket,
    db_pool: Arc<sqlx_postgres::PgPool>,
    octofhir_provider: Arc<crate::model_provider::OctoFhirModelProvider>,
) {
    tracing::debug!("=== LSP WebSocket connection START ===");
    let (ws_write, mut ws_read) = socket.split();

    // Create LSP service - use direct service calls, not Server::new with duplex streams
    tracing::debug!("Creating LSP service...");
    let (service, _client_socket) = LspService::new(|client| {
        PostgresLspServer::new(client, db_pool, octofhir_provider.clone())
    });

    // Wrap service in Arc<Mutex> for shared access
    let service = Arc::new(Mutex::new(service));
    tracing::debug!("LSP service created, ready for requests");

    // Spawn a task to send periodic pings to keep connection alive
    let ping_ws_write = Arc::new(Mutex::new(ws_write));
    let ping_handle = {
        let ws_write = ping_ws_write.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(30));
            loop {
                interval.tick().await;
                let mut write = ws_write.lock().await;
                if write
                    .send(axum::extract::ws::Message::Ping(vec![].into()))
                    .await
                    .is_err()
                {
                    break;
                }
                tracing::trace!("Sent WebSocket ping");
            }
        })
    };

    // Process WebSocket messages directly
    while let Some(msg) = ws_read.next().await {
        let text = match msg {
            Ok(axum::extract::ws::Message::Text(t)) => t.to_string(),
            Ok(axum::extract::ws::Message::Binary(b)) => match String::from_utf8(b.to_vec()) {
                Ok(s) => s,
                Err(e) => {
                    tracing::debug!("Invalid UTF-8 in binary message: {}", e);
                    continue;
                }
            },
            Ok(axum::extract::ws::Message::Close(_)) => {
                tracing::debug!("WebSocket close received");
                break;
            }
            Ok(axum::extract::ws::Message::Ping(_)) => {
                // Respond to ping with pong
                let mut write = ping_ws_write.lock().await;
                let _ = write
                    .send(axum::extract::ws::Message::Pong(vec![].into()))
                    .await;
                tracing::trace!("Received ping, sent pong");
                continue;
            }
            Ok(axum::extract::ws::Message::Pong(_)) => {
                tracing::trace!("Received pong");
                continue;
            }
            Err(e) => {
                tracing::debug!("WebSocket read error: {}", e);
                break;
            }
        };

        // Skip empty messages
        if text.trim().is_empty() {
            continue;
        }

        tracing::trace!("LSP <= WS: {}", text);

        // Parse JSON-RPC request
        let request: Request = match serde_json::from_str(&text) {
            Ok(r) => r,
            Err(e) => {
                tracing::debug!("Invalid JSON-RPC request: {} - input: {}", e, text);
                // Send error response for malformed requests
                let error_response = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": null,
                    "error": {
                        "code": -32700,
                        "message": "Parse error"
                    }
                });
                let mut write = ping_ws_write.lock().await;
                let _ = write
                    .send(axum::extract::ws::Message::Text(
                        error_response.to_string().into(),
                    ))
                    .await;
                continue;
            }
        };

        let method = request.method().to_string();
        tracing::debug!("Processing LSP request: method={}", method);

        // Call service directly using Tower's ServiceExt
        let mut svc = service.lock().await;
        match svc.ready().await {
            Ok(ready_svc) => {
                match ready_svc.call(request).await {
                    Ok(Some(response)) => {
                        let json = match serde_json::to_string(&response) {
                            Ok(j) => j,
                            Err(e) => {
                                tracing::error!("Failed to serialize LSP response: {}", e);
                                continue;
                            }
                        };

                        tracing::trace!("LSP => WS: {}", json);

                        let mut write = ping_ws_write.lock().await;
                        if write
                            .send(axum::extract::ws::Message::Text(json.into()))
                            .await
                            .is_err()
                        {
                            tracing::debug!("Failed to send LSP response to WebSocket");
                            break;
                        }
                    }
                    Ok(None) => {
                        // Notification - no response expected (e.g., initialized, didOpen)
                        tracing::trace!("LSP notification processed (no response): {}", method);
                    }
                    Err(e) => {
                        tracing::error!("LSP service call failed: {:?}", e);
                        // Send error response
                        let error_response = serde_json::json!({
                            "jsonrpc": "2.0",
                            "id": null,
                            "error": {
                                "code": -32603,
                                "message": format!("Internal error: {:?}", e)
                            }
                        });
                        let mut write = ping_ws_write.lock().await;
                        let _ = write
                            .send(axum::extract::ws::Message::Text(
                                error_response.to_string().into(),
                            ))
                            .await;
                    }
                }
            }
            Err(e) => {
                tracing::error!("LSP service not ready: {:?}", e);
            }
        }
    }

    // Cancel ping task
    ping_handle.abort();
    tracing::debug!("LSP WebSocket connection closed");
}

/// LSP handler errors.
#[derive(Debug)]
pub enum LspError {
    Unauthorized,
    AuthNotConfigured,
}

impl IntoResponse for LspError {
    fn into_response(self) -> Response {
        match self {
            LspError::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                "Invalid or missing authentication token",
            )
                .into_response(),
            LspError::AuthNotConfigured => (
                StatusCode::SERVICE_UNAVAILABLE,
                "Authentication not configured",
            )
                .into_response(),
        }
    }
}
