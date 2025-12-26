//! WebSocket handler for PostgreSQL LSP with authentication.
//!
//! Uses async-lsp MainLoop for reliable WebSocket-to-LSP communication.

use async_lsp::client_monitor::ClientProcessMonitorLayer;
use async_lsp::concurrency::ConcurrencyLayer;
use async_lsp::lsp_types::{notification, request};
use async_lsp::panic::CatchUnwindLayer;
use async_lsp::server::LifecycleLayer;
use async_lsp::tracing::TracingLayer;
use async_lsp::{ClientSocket, LanguageServer, MainLoop, router::Router};
use axum::{
    extract::{Query, State, WebSocketUpgrade},
    http::{HeaderMap, StatusCode, header::COOKIE},
    response::{IntoResponse, Response},
};
use futures_util::{SinkExt, StreamExt};
use octofhir_auth::token::jwt::AccessTokenClaims;
use serde::Deserialize;
use std::sync::Arc;
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};
use tower::ServiceBuilder;

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
    let token_data = auth_state
        .jwt_service
        .decode::<AccessTokenClaims>(&token)
        .map_err(|e| {
            tracing::warn!(error = %e, "LSP auth failed: invalid token");
            LspError::Unauthorized
        })?;
    let claims = token_data.claims;

    // Check db_console permission from claims
    // Allowed if user has:
    // - system/*.* scope (full system access)
    // - system/*.read scope (system read access)
    // - user/*.* scope (full user access)
    // - db_console scope (explicit db console access)
    if !has_db_console_permission(&claims) {
        tracing::warn!(
            client_id = %claims.client_id,
            scope = %claims.scope,
            "LSP auth failed: insufficient permissions for DB console"
        );
        return Err(LspError::Forbidden);
    }

    tracing::debug!(
        client_id = %claims.client_id,
        "LSP WebSocket connection authenticated - upgrading to WebSocket"
    );

    // Upgrade to WebSocket
    Ok(ws.on_upgrade(move |socket| {
        tracing::debug!("WebSocket upgrade completed, calling handle_lsp_connection");
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

/// Handle the LSP connection over WebSocket using async-lsp MainLoop.
///
/// Uses async-lsp's Router and MainLoop for reliable LSP communication.
/// The MainLoop handles JSON-RPC message routing and error handling automatically.
async fn handle_lsp_connection(
    socket: axum::extract::ws::WebSocket,
    db_pool: Arc<sqlx_postgres::PgPool>,
    octofhir_provider: Arc<crate::model_provider::OctoFhirModelProvider>,
) {
    tracing::info!("LSP WebSocket connection started");

    // Split WebSocket into read and write halves
    let (ws_write, ws_read) = socket.split();

    // Create duplex streams for async-lsp bidirectional communication
    // MainLoop reads requests from mainloop_read, writes responses to mainloop_write
    let (mainloop_read, ws_to_mainloop_write) = tokio::io::duplex(1024 * 1024); // 1MB buffer
    let (mainloop_to_ws_read, mainloop_write) = tokio::io::duplex(1024 * 1024);

    // Bridge 1: WebSocket → MainLoop
    // Read LSP requests from WebSocket, add Content-Length headers, write to MainLoop
    let ws_to_server = {
        let mut ws_read = ws_read;
        let mut mainloop_input = ws_to_mainloop_write;
        tokio::spawn(async move {
            use tokio::io::AsyncWriteExt;
            tracing::debug!("WS to MainLoop bridge task started");
            while let Some(msg) = ws_read.next().await {
                match msg {
                    Ok(axum::extract::ws::Message::Text(t)) => {
                        tracing::debug!("WS => MainLoop (raw JSON): {}", t);

                        // Add LSP Content-Length header
                        let content_len = t.len();
                        let lsp_message = format!("Content-Length: {}\r\n\r\n{}", content_len, t);

                        tracing::debug!("WS => MainLoop (with header): {}", lsp_message);

                        if let Err(e) = mainloop_input.write_all(lsp_message.as_bytes()).await {
                            tracing::debug!("Failed to write to mainloop: {}", e);
                            break;
                        }
                    }
                    Ok(axum::extract::ws::Message::Binary(b)) => {
                        // Add LSP Content-Length header for binary messages
                        let header = format!("Content-Length: {}\r\n\r\n", b.len());
                        if let Err(e) = mainloop_input.write_all(header.as_bytes()).await {
                            tracing::debug!("Failed to write header to mainloop: {}", e);
                            break;
                        }
                        if let Err(e) = mainloop_input.write_all(&b).await {
                            tracing::debug!("Failed to write binary to mainloop: {}", e);
                            break;
                        }
                    }
                    Ok(axum::extract::ws::Message::Close(_)) => {
                        tracing::debug!("WebSocket close received");
                        break;
                    }
                    Ok(axum::extract::ws::Message::Ping(_))
                    | Ok(axum::extract::ws::Message::Pong(_)) => {
                        continue;
                    }
                    Err(e) => {
                        tracing::debug!("WebSocket read error: {}", e);
                        break;
                    }
                }
            }
        })
    };

    // Bridge 2: MainLoop → WebSocket
    // Read LSP responses (with headers) from MainLoop, strip headers, send raw JSON to WebSocket
    let client_to_ws = {
        let mut ws_write = ws_write;
        let mainloop_output = mainloop_to_ws_read;
        tokio::spawn(async move {
            use tokio::io::AsyncBufReadExt;
            tracing::debug!("MainLoop to WS bridge task started");
            let mut reader = tokio::io::BufReader::new(mainloop_output);

            loop {
                // Read Content-Length header
                let mut header_line = String::new();
                match reader.read_line(&mut header_line).await {
                    Ok(0) => {
                        tracing::debug!("MainLoop to WS: EOF");
                        break;
                    }
                    Ok(_) => {
                        // Parse Content-Length
                        if !header_line.starts_with("Content-Length:") {
                            tracing::debug!("Expected Content-Length header, got: {}", header_line);
                            continue;
                        }

                        let content_length: usize = header_line
                            .trim_start_matches("Content-Length:")
                            .trim()
                            .parse()
                            .unwrap_or(0);

                        // Read empty line after header
                        let mut empty_line = String::new();
                        if let Err(e) = reader.read_line(&mut empty_line).await {
                            tracing::debug!("Failed to read empty line: {}", e);
                            break;
                        }

                        // Read the JSON content
                        let mut content = vec![0u8; content_length];
                        if let Err(e) =
                            tokio::io::AsyncReadExt::read_exact(&mut reader, &mut content).await
                        {
                            tracing::debug!("Failed to read content: {}", e);
                            break;
                        }

                        let json_text = String::from_utf8_lossy(&content).to_string();
                        tracing::debug!("MainLoop => WS (stripped headers): {}", json_text);

                        // Send raw JSON to WebSocket
                        if let Err(e) = ws_write
                            .send(axum::extract::ws::Message::Text(json_text.into()))
                            .await
                        {
                            tracing::debug!("Failed to send to WebSocket: {}", e);
                            break;
                        }
                    }
                    Err(e) => {
                        tracing::debug!("MainLoop output read error: {}", e);
                        break;
                    }
                }
            }
        })
    };

    // Create async-lsp mainloop with Router and manual method registration
    tracing::debug!("Creating LSP mainloop with Router");
    let (mainloop, _server_socket) = MainLoop::new_server(|client_socket: ClientSocket| {
        tracing::debug!("MainLoop factory called - creating PostgresLspServer");

        // Create server state with the client socket
        let server = PostgresLspServer::new(
            client_socket.clone(),
            db_pool.clone(),
            octofhir_provider.clone(),
        );

        // Create Router and manually register all LSP methods
        // This is the proper async-lsp pattern
        tracing::debug!("Creating Router and registering LSP methods");
        let mut router = Router::new(server);

        // Register all LSP methods - chaining modifies router in place
        // Return the Future directly without wrapping in async move {}
        let _ = router
            .request::<request::Initialize, _>(|st, params| st.initialize(params))
            .request::<request::Shutdown, _>(|st, params| st.shutdown(params))
            .request::<request::Completion, _>(|st, params| st.completion(params))
            .request::<request::HoverRequest, _>(|st, params| st.hover(params))
            .request::<request::Formatting, _>(|st, params| st.formatting(params))
            .notification::<notification::Initialized>(|st, params| st.initialized(params))
            .notification::<notification::DidOpenTextDocument>(|st, params| st.did_open(params))
            .notification::<notification::DidChangeTextDocument>(|st, params| st.did_change(params))
            .notification::<notification::DidCloseTextDocument>(|st, params| st.did_close(params));

        // Add middleware layers - use the original router variable
        tracing::debug!("Wrapping router with middleware layers");
        ServiceBuilder::new()
            .layer(TracingLayer::default())
            .layer(LifecycleLayer::default())
            .layer(CatchUnwindLayer::default())
            .layer(ConcurrencyLayer::default())
            .layer(ClientProcessMonitorLayer::new(client_socket))
            .service(router)
    });

    // Convert tokio duplex streams to futures-compatible streams using compat layer
    let mainloop_read_compat = mainloop_read.compat();
    let mainloop_write_compat = mainloop_write.compat_write();

    // Run the mainloop with the bridged streams
    tracing::debug!("Starting LSP mainloop");
    match mainloop
        .run_buffered(mainloop_read_compat, mainloop_write_compat)
        .await
    {
        Ok(()) => tracing::debug!("LSP mainloop completed successfully"),
        Err(e) => tracing::error!("LSP mainloop error: {:?}", e),
    }

    // Cleanup: wait for bridge tasks to finish
    tracing::debug!("Waiting for bridge tasks to finish");
    let _ = tokio::join!(ws_to_server, client_to_ws);
    tracing::info!("LSP WebSocket connection closed");
}

/// Check if the access token has permission to use the DB console.
///
/// Allowed scopes:
/// - `system/*.*` or `system/*.read` - system-level access
/// - `user/*.*` - user-level full access
/// - `db_console` - explicit DB console permission
/// - Any `admin/*` scope - admin access
fn has_db_console_permission(claims: &AccessTokenClaims) -> bool {
    let scope = &claims.scope;

    // Check for specific scope patterns that grant DB console access
    for s in scope.split_whitespace() {
        if matches!(
            s,
            "system/*.*" | "system/*.read" | "user/*.*" | "db_console" | "admin/*"
        ) {
            return true;
        }
        // Also check for admin-like patterns
        if s.starts_with("admin/") || s.starts_with("system/") {
            return true;
        }
    }

    false
}

/// LSP handler errors.
#[derive(Debug)]
pub enum LspError {
    Unauthorized,
    Forbidden,
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
            LspError::Forbidden => (
                StatusCode::FORBIDDEN,
                "Insufficient permissions for DB console access",
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
