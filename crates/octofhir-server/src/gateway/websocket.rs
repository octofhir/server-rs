//! WebSocket proxy handler for real-time connections to backend services.

use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    response::Response,
};
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::{connect_async, tungstenite};
use tracing::{debug, error, info, instrument};

use super::error::GatewayError;
use super::types::{AuthInfo, CustomOperation};
use crate::server::AppState;

/// Handles WebSocket upgrade and proxies connection to backend.
///
/// This handler:
/// 1. Extracts WebSocket configuration from the operation
/// 2. Upgrades the HTTP connection to WebSocket
/// 3. Connects to the backend WebSocket
/// 4. Bidirectionally proxies messages between client and backend
#[instrument(skip(_state, operation, ws_upgrade, auth_info, original_query))]
pub async fn handle_websocket(
    _state: &AppState,
    operation: &CustomOperation,
    ws_upgrade: WebSocketUpgrade,
    auth_info: &AuthInfo,
    original_query: Option<&str>,
) -> Result<Response, GatewayError> {
    // Get WebSocket config from proxy.websocket
    let ws_config = operation
        .proxy
        .as_ref()
        .and_then(|p| p.websocket.as_ref())
        .ok_or_else(|| {
            GatewayError::InvalidConfig(
                "WebSocket operation missing websocket configuration".to_string(),
            )
        })?;

    let mut backend_url = ws_config.url.clone();

    // Forward original query parameters from client request
    if let Some(query) = original_query {
        if !query.is_empty() {
            if backend_url.contains('?') {
                backend_url = format!("{}&{}", backend_url, query);
            } else {
                backend_url = format!("{}?{}", backend_url, query);
            }
            debug!(original_query = %query, "Forwarded original query params");
        }
    }

    // Forward auth info as query parameters if configured
    if ws_config.forward_auth_in_query {
        debug!(
            fhir_user = ?auth_info.fhir_user,
            user_id = ?auth_info.user_id,
            authenticated = auth_info.authenticated,
            "Appending auth to WebSocket URL"
        );
        backend_url = append_auth_query(&backend_url, auth_info);
    }

    info!(
        backend_url = %backend_url,
        operation_id = ?operation.id,
        forward_auth = ws_config.forward_auth_in_query,
        "Upgrading to WebSocket and proxying to backend"
    );

    // Clone for async move
    let backend_url_clone = backend_url.clone();

    // Upgrade HTTP to WebSocket and spawn proxy task
    Ok(ws_upgrade.on_upgrade(move |client_ws| async move {
        if let Err(e) = proxy_websocket(client_ws, &backend_url_clone).await {
            error!(error = %e, "WebSocket proxy error");
        }
    }))
}

/// Proxies WebSocket messages bidirectionally between client and backend.
async fn proxy_websocket(
    client_ws: WebSocket,
    backend_url: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Connect to backend WebSocket
    let (backend_ws, response) = connect_async(backend_url).await.map_err(|e| {
        error!(error = %e, url = %backend_url, "Failed to connect to backend WebSocket");
        e
    })?;

    info!(
        status = ?response.status(),
        "Connected to backend WebSocket"
    );

    // Split both connections into sink/stream pairs
    let (mut backend_sink, mut backend_stream) = backend_ws.split();
    let (mut client_sink, mut client_stream) = client_ws.split();

    // Bidirectional message forwarding
    loop {
        tokio::select! {
            // Client → Backend
            client_msg = client_stream.next() => {
                match client_msg {
                    Some(Ok(msg)) => {
                        match convert_client_to_backend(msg) {
                            Some(backend_msg) => {
                                if let Err(e) = backend_sink.send(backend_msg).await {
                                    debug!(error = %e, "Failed to send to backend, closing");
                                    break;
                                }
                            }
                            None => {
                                // Close message received
                                debug!("Client sent close, shutting down");
                                let _ = backend_sink.close().await;
                                break;
                            }
                        }
                    }
                    Some(Err(e)) => {
                        debug!(error = %e, "Client WebSocket error");
                        break;
                    }
                    None => {
                        debug!("Client stream ended");
                        break;
                    }
                }
            }

            // Backend → Client
            backend_msg = backend_stream.next() => {
                match backend_msg {
                    Some(Ok(msg)) => {
                        match convert_backend_to_client(msg) {
                            Some(client_msg) => {
                                if let Err(e) = client_sink.send(client_msg).await {
                                    debug!(error = %e, "Failed to send to client, closing");
                                    break;
                                }
                            }
                            None => {
                                // Close message received
                                debug!("Backend sent close, shutting down");
                                let _ = client_sink.close().await;
                                break;
                            }
                        }
                    }
                    Some(Err(e)) => {
                        debug!(error = %e, "Backend WebSocket error");
                        break;
                    }
                    None => {
                        debug!("Backend stream ended");
                        break;
                    }
                }
            }
        }
    }

    info!("WebSocket proxy connection closed");
    Ok(())
}

/// Converts axum WebSocket message to tungstenite message.
fn convert_client_to_backend(msg: Message) -> Option<tungstenite::Message> {
    match msg {
        Message::Text(text) => Some(tungstenite::Message::Text(text.to_string())),
        Message::Binary(data) => Some(tungstenite::Message::Binary(data.to_vec())),
        Message::Ping(data) => Some(tungstenite::Message::Ping(data.to_vec())),
        Message::Pong(data) => Some(tungstenite::Message::Pong(data.to_vec())),
        Message::Close(_) => None,
    }
}

/// Converts tungstenite message to axum WebSocket message.
fn convert_backend_to_client(msg: tungstenite::Message) -> Option<Message> {
    match msg {
        tungstenite::Message::Text(text) => Some(Message::Text(text.into())),
        tungstenite::Message::Binary(data) => Some(Message::Binary(data.into())),
        tungstenite::Message::Ping(data) => Some(Message::Ping(data.into())),
        tungstenite::Message::Pong(data) => Some(Message::Pong(data.into())),
        tungstenite::Message::Close(_) => None,
        tungstenite::Message::Frame(_) => None,
    }
}

/// Appends authentication info as query parameters to the URL.
fn append_auth_query(url: &str, auth_info: &AuthInfo) -> String {
    let mut params = Vec::new();

    if let Some(ref fhir_user) = auth_info.fhir_user {
        params.push(format!("fhirUser={}", urlencoding::encode(fhir_user)));
    }

    if let Some(ref user_id) = auth_info.user_id {
        params.push(format!("userId={}", urlencoding::encode(user_id)));
    }

    if let Some(ref username) = auth_info.username {
        params.push(format!("username={}", urlencoding::encode(username)));
    }

    if let Some(ref client_id) = auth_info.client_id {
        params.push(format!("clientId={}", urlencoding::encode(client_id)));
    }

    if !auth_info.roles.is_empty() {
        params.push(format!("roles={}", urlencoding::encode(&auth_info.roles.join(","))));
    }

    if params.is_empty() {
        return url.to_string();
    }

    let query_string = params.join("&");

    if url.contains('?') {
        format!("{}&{}", url, query_string)
    } else {
        format!("{}?{}", url, query_string)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_append_auth_query_empty() {
        let auth = AuthInfo::default();
        let result = append_auth_query("ws://localhost:3000/ws", &auth);
        assert_eq!(result, "ws://localhost:3000/ws");
    }

    #[test]
    fn test_append_auth_query_with_fhir_user() {
        let auth = AuthInfo {
            fhir_user: Some("Patient/123".to_string()),
            ..Default::default()
        };
        let result = append_auth_query("ws://localhost:3000/ws", &auth);
        assert_eq!(result, "ws://localhost:3000/ws?fhirUser=Patient%2F123");
    }

    #[test]
    fn test_append_auth_query_with_existing_params() {
        let auth = AuthInfo {
            fhir_user: Some("Patient/123".to_string()),
            user_id: Some("user-456".to_string()),
            ..Default::default()
        };
        let result = append_auth_query("ws://localhost:3000/ws?token=abc", &auth);
        assert!(result.starts_with("ws://localhost:3000/ws?token=abc&"));
        assert!(result.contains("fhirUser=Patient%2F123"));
        assert!(result.contains("userId=user-456"));
    }

    #[test]
    fn test_append_auth_query_with_roles() {
        let auth = AuthInfo {
            roles: vec!["admin".to_string(), "user".to_string()],
            ..Default::default()
        };
        let result = append_auth_query("ws://localhost:3000/ws", &auth);
        assert_eq!(result, "ws://localhost:3000/ws?roles=admin%2Cuser");
    }
}
