//! WebSocket handler for PostgreSQL LSP with authentication.

use axum::{
    extract::{Query, State, WebSocketUpgrade},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use futures_util::{SinkExt, StreamExt};
use octofhir_auth::token::jwt::AccessTokenClaims;
use serde::Deserialize;
use std::sync::Arc;
use tower_lsp::{LspService, Server};

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
/// Requires authentication via query parameter: `/api/pg-lsp?token=xxx`
pub async fn lsp_websocket_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Query(params): Query<LspQueryParams>,
) -> Result<Response, LspError> {
    // Validate authentication token
    let token = params.token.ok_or(LspError::Unauthorized)?;

    // Validate the token using auth state
    let auth_state = state.auth_state.as_ref().ok_or(LspError::AuthNotConfigured)?;

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
    Ok(ws.on_upgrade(move |socket| handle_lsp_connection(socket, state.db_pool)))
}

/// Handle the LSP connection over WebSocket.
async fn handle_lsp_connection(
    socket: axum::extract::ws::WebSocket,
    db_pool: Arc<sqlx_postgres::PgPool>,
) {
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};

    let (mut ws_write, mut ws_read) = socket.split();

    // Create LSP service
    let (service, socket_handle) = LspService::new(|client| {
        PostgresLspServer::new(client, db_pool)
    });

    // Create duplex streams for LSP server
    let (lsp_read, lsp_write) = tokio::io::duplex(4096);
    let (mut lsp_read_out, lsp_write_out) = tokio::io::duplex(4096);

    // Spawn LSP server
    let server_handle = tokio::spawn(async move {
        Server::new(lsp_read, lsp_write_out, socket_handle)
            .serve(service)
            .await;
    });

    // Bridge: WebSocket -> LSP stdin
    let ws_to_lsp = tokio::spawn(async move {
        let mut lsp_write = lsp_write;
        while let Some(msg) = ws_read.next().await {
            match msg {
                Ok(axum::extract::ws::Message::Text(text)) => {
                    // Write LSP message with Content-Length header
                    let header = format!("Content-Length: {}\r\n\r\n", text.len());
                    if lsp_write.write_all(header.as_bytes()).await.is_err() {
                        break;
                    }
                    if lsp_write.write_all(text.as_bytes()).await.is_err() {
                        break;
                    }
                    if lsp_write.flush().await.is_err() {
                        break;
                    }
                }
                Ok(axum::extract::ws::Message::Close(_)) => break,
                Err(e) => {
                    tracing::debug!(error = %e, "WebSocket read error");
                    break;
                }
                _ => {}
            }
        }
    });

    // Bridge: LSP stdout -> WebSocket
    let lsp_to_ws = tokio::spawn(async move {
        let mut reader = tokio::io::BufReader::new(&mut lsp_read_out);

        loop {
            // Read headers
            let mut content_length: usize = 0;
            loop {
                let mut line = String::new();
                match reader.read_line(&mut line).await {
                    Ok(0) => return, // EOF
                    Ok(_) => {
                        let line = line.trim();
                        if line.is_empty() {
                            break;
                        }
                        if let Some(len_str) = line.strip_prefix("Content-Length: ") {
                            content_length = len_str.parse().unwrap_or(0);
                        }
                    }
                    Err(_) => return,
                }
            }

            if content_length == 0 {
                continue;
            }

            // Read content
            let mut content = vec![0u8; content_length];
            if reader.read_exact(&mut content).await.is_err() {
                return;
            }

            // Send as WebSocket message
            let text = String::from_utf8_lossy(&content).to_string();
            if ws_write.send(axum::extract::ws::Message::Text(text.into())).await.is_err() {
                return;
            }
        }
    });

    // Wait for any task to complete
    tokio::select! {
        _ = ws_to_lsp => {
            tracing::debug!("WebSocket to LSP bridge closed");
        }
        _ = lsp_to_ws => {
            tracing::debug!("LSP to WebSocket bridge closed");
        }
        _ = server_handle => {
            tracing::debug!("LSP server closed");
        }
    }

    tracing::info!("LSP WebSocket connection closed");
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
            LspError::Unauthorized => {
                (StatusCode::UNAUTHORIZED, "Invalid or missing authentication token").into_response()
            }
            LspError::AuthNotConfigured => {
                (StatusCode::SERVICE_UNAVAILABLE, "Authentication not configured").into_response()
            }
        }
    }
}
