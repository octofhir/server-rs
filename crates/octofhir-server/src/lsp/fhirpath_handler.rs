//! WebSocket handler for FHIRPath LSP with authentication.
//!
//! Provides FHIRPath expression completion, diagnostics, and context-aware
//! suggestions for ViewDefinition and SQL on FHIR expressions.

use axum::{
    extract::{Query, State, WebSocketUpgrade},
    http::{HeaderMap, StatusCode, header::COOKIE},
    response::{IntoResponse, Response},
};
use futures_util::{SinkExt, StreamExt};
use octofhir_auth::token::jwt::AccessTokenClaims;
use octofhir_fhirpath::evaluator::create_function_registry;
use octofhir_fhirpath::lsp::{CompletionProvider, LspHandlers, SetContextParams};
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc};

use crate::server::AppState;

/// Query parameters for FHIRPath LSP WebSocket connection.
#[derive(Debug, Deserialize)]
pub struct FhirPathLspQueryParams {
    /// Authentication token (required)
    pub token: Option<String>,
}

/// WebSocket handler for FHIRPath LSP.
///
/// Uses lower permission check than pg-lsp: any authenticated user
/// with user/*, system/*, or admin/* scope can connect.
pub async fn fhirpath_lsp_websocket_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Query(params): Query<FhirPathLspQueryParams>,
    headers: HeaderMap,
) -> Result<Response, FhirPathLspError> {
    let auth_state = &state.auth_state;

    // Prefer explicit query token, but fall back to the auth cookie for UI sessions.
    let token = params
        .token
        .or_else(|| {
            if !auth_state.cookie_config.enabled {
                return None;
            }
            extract_cookie_token(&headers, auth_state.cookie_config.name.as_str())
        })
        .ok_or(FhirPathLspError::Unauthorized)?;

    // Decode and validate JWT
    let token_data = auth_state
        .jwt_service
        .decode::<AccessTokenClaims>(&token)
        .map_err(|e| {
            tracing::warn!(error = %e, "FHIRPath LSP auth failed: invalid token");
            FhirPathLspError::Unauthorized
        })?;
    let claims = token_data.claims;

    // Lower permission check: any authenticated user with user/*, system/*, or admin/* scope
    if !has_fhirpath_permission(&claims) {
        tracing::warn!(
            client_id = %claims.client_id,
            scope = %claims.scope,
            "FHIRPath LSP auth failed: insufficient permissions"
        );
        return Err(FhirPathLspError::Forbidden);
    }

    tracing::debug!(
        client_id = %claims.client_id,
        "FHIRPath LSP WebSocket connection authenticated - upgrading to WebSocket"
    );

    // Upgrade to WebSocket
    Ok(ws.on_upgrade(move |socket| {
        tracing::debug!("FHIRPath WebSocket upgrade completed");
        handle_fhirpath_lsp_connection(socket, state.model_provider.clone())
    }))
}

fn extract_cookie_token(headers: &HeaderMap, cookie_name: &str) -> Option<String> {
    let cookie_header = headers.get(COOKIE)?.to_str().ok()?;

    for cookie in cookie_header.split(';') {
        let cookie = cookie.trim();
        if let Some((name, value)) = cookie.split_once('=')
            && name.trim() == cookie_name
        {
            let value = value.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }

    None
}

/// Handle the FHIRPath LSP connection over WebSocket.
///
/// Uses a simple JSON-RPC message processing approach since the FHIRPath LSP
/// from octofhir-fhirpath provides handlers that work directly with JSON-RPC.
async fn handle_fhirpath_lsp_connection(
    socket: axum::extract::ws::WebSocket,
    model_provider: Arc<crate::model_provider::OctoFhirModelProvider>,
) {
    tracing::info!("FHIRPath LSP WebSocket connection started");

    let (mut ws_sender, mut ws_receiver) = socket.split();

    // Create channels for LSP communication
    let (response_tx, mut response_rx) = mpsc::unbounded_channel::<String>();

    // Create FHIRPath LSP handlers
    let function_registry = Arc::new(create_function_registry());
    let completion_provider = CompletionProvider::new(model_provider, function_registry);
    let handlers = Arc::new(Mutex::new(LspHandlers::new(completion_provider)));

    // Spawn task to forward responses to WebSocket
    let send_task = tokio::spawn(async move {
        while let Some(msg) = response_rx.recv().await {
            if ws_sender
                .send(axum::extract::ws::Message::Text(msg.into()))
                .await
                .is_err()
            {
                break;
            }
        }
    });

    // Process incoming WebSocket messages
    while let Some(msg) = ws_receiver.next().await {
        match msg {
            Ok(axum::extract::ws::Message::Text(text)) => {
                tracing::debug!("FHIRPath LSP received: {}", text);
                if let Some(response) = process_lsp_message(&text, &handlers, &response_tx).await
                    && response_tx.send(response).is_err()
                {
                    break;
                }
            }
            Ok(axum::extract::ws::Message::Binary(data)) => {
                if let Ok(text) = String::from_utf8(data.to_vec())
                    && let Some(response) =
                        process_lsp_message(&text, &handlers, &response_tx).await
                    && response_tx.send(response).is_err()
                {
                    break;
                }
            }
            Ok(axum::extract::ws::Message::Close(_)) => {
                tracing::debug!("FHIRPath LSP WebSocket close received");
                break;
            }
            Ok(axum::extract::ws::Message::Ping(_)) | Ok(axum::extract::ws::Message::Pong(_)) => {
                continue;
            }
            Err(e) => {
                tracing::debug!("FHIRPath LSP WebSocket read error: {}", e);
                break;
            }
        }
    }

    // Clean up
    drop(response_tx);
    let _ = send_task.await;
    tracing::info!("FHIRPath LSP WebSocket connection closed");
}

/// Process a single LSP message and return response if any
async fn process_lsp_message(
    message: &str,
    handlers: &Arc<Mutex<LspHandlers>>,
    response_tx: &mpsc::UnboundedSender<String>,
) -> Option<String> {
    use async_lsp::lsp_types::{
        CompletionParams, DidChangeTextDocumentParams, DidCloseTextDocumentParams,
        DidOpenTextDocumentParams, InitializeParams,
    };

    // Parse the LSP message (JSON-RPC)
    let json: serde_json::Value = match serde_json::from_str(message) {
        Ok(v) => v,
        Err(e) => {
            tracing::error!("Failed to parse FHIRPath LSP message: {}", e);
            return None;
        }
    };

    let method = json.get("method")?.as_str()?;
    let id = json.get("id");
    let params = json.get("params");

    match method {
        "initialize" => {
            let params: InitializeParams = serde_json::from_value(params?.clone()).ok()?;
            let handlers = handlers.lock().await;
            let result = handlers.initialize(params);
            let response = serde_json::json!({
                "jsonrpc": "2.0",
                "id": id?,
                "result": result
            });
            Some(serde_json::to_string(&response).ok()?)
        }
        "shutdown" => {
            let response = serde_json::json!({
                "jsonrpc": "2.0",
                "id": id?,
                "result": null
            });
            Some(serde_json::to_string(&response).ok()?)
        }
        "textDocument/completion" => {
            let params: CompletionParams = serde_json::from_value(params?.clone()).ok()?;
            let handlers = handlers.lock().await;
            let result = handlers.completion(params).await;
            let response = serde_json::json!({
                "jsonrpc": "2.0",
                "id": id?,
                "result": result
            });
            Some(serde_json::to_string(&response).ok()?)
        }
        "textDocument/didOpen" => {
            let params: DidOpenTextDocumentParams = serde_json::from_value(params?.clone()).ok()?;
            let uri = params.text_document.uri.clone();
            let mut handlers = handlers.lock().await;
            let diagnostics = handlers.did_open(params);
            // Send diagnostics notification
            let notification = serde_json::json!({
                "jsonrpc": "2.0",
                "method": "textDocument/publishDiagnostics",
                "params": {
                    "uri": uri.to_string(),
                    "diagnostics": diagnostics
                }
            });
            let _ = response_tx.send(serde_json::to_string(&notification).ok()?);
            None
        }
        "textDocument/didChange" => {
            let params: DidChangeTextDocumentParams =
                serde_json::from_value(params?.clone()).ok()?;
            let uri = params.text_document.uri.clone();
            let mut handlers = handlers.lock().await;
            let diagnostics = handlers.did_change(params);
            // Send diagnostics notification
            let notification = serde_json::json!({
                "jsonrpc": "2.0",
                "method": "textDocument/publishDiagnostics",
                "params": {
                    "uri": uri.to_string(),
                    "diagnostics": diagnostics
                }
            });
            let _ = response_tx.send(serde_json::to_string(&notification).ok()?);
            None
        }
        "textDocument/didClose" => {
            let params: DidCloseTextDocumentParams =
                serde_json::from_value(params?.clone()).ok()?;
            let mut handlers = handlers.lock().await;
            handlers.did_close(params);
            None
        }
        "fhirpath/setContext" => {
            let params: SetContextParams = serde_json::from_value(params?.clone()).ok()?;
            let mut handlers = handlers.lock().await;
            handlers.set_context(params);
            tracing::debug!("FHIRPath context updated");
            None
        }
        "fhirpath/clearContext" => {
            let mut handlers = handlers.lock().await;
            handlers.clear_context();
            tracing::debug!("FHIRPath context cleared");
            None
        }
        "initialized" => {
            tracing::info!("FHIRPath LSP server initialized");
            None
        }
        _ => {
            tracing::warn!("Unknown FHIRPath LSP method: {}", method);
            if id.is_some() {
                // Send error response for unknown requests
                let response = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": id?,
                    "error": {
                        "code": -32601,
                        "message": format!("Method not found: {}", method)
                    }
                });
                Some(serde_json::to_string(&response).ok()?)
            } else {
                None
            }
        }
    }
}

/// Check if the access token has permission to use FHIRPath LSP.
///
/// Lower permission than DB console - accepts any authenticated user with:
/// - Any `user/*` scope
/// - Any `system/*` scope
/// - Any `admin/*` scope
fn has_fhirpath_permission(claims: &AccessTokenClaims) -> bool {
    let scope = &claims.scope;

    for s in scope.split_whitespace() {
        if s.starts_with("user/") || s.starts_with("system/") || s.starts_with("admin/") {
            return true;
        }
    }

    false
}

/// FHIRPath LSP handler errors.
#[derive(Debug)]
pub enum FhirPathLspError {
    Unauthorized,
    Forbidden,
}

impl IntoResponse for FhirPathLspError {
    fn into_response(self) -> Response {
        match self {
            FhirPathLspError::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                "Invalid or missing authentication token",
            )
                .into_response(),
            FhirPathLspError::Forbidden => (
                StatusCode::FORBIDDEN,
                "Insufficient permissions for FHIRPath LSP access",
            )
                .into_response(),
        }
    }
}
