//! WebSocket for the FML language server (`/api/lsp/fml`), behind the console's FML editor.
//!
//! The language server itself is `fml-lsp`; this carries its messages and gives it definitions:
//! the core definitions of the server's FHIR release, and the StructureDefinitions of the
//! installed packages that a map's `uses` names (logical models such as CDA, profiles), with the
//! structures those refer to. They are looked up when a map is opened or changed, once per URL
//! and connection.

use std::collections::HashSet;
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket};
use axum::extract::{Query, State, WebSocketUpgrade};
use axum::http::HeaderMap;
use axum::response::Response;
use fml_check::{CoreDefinitions, Definitions, NoDefinitions, PackageDefinitions};
use fml_lsp::Server;
use futures_util::{SinkExt, StreamExt};
use octofhir_sof::expansion::{Lookup, split_canonical};
use serde::Deserialize;
use serde_json::Value;

use super::fhirpath_handler::{FhirPathLspError, authorize};
use crate::operations::fml::api::core;
use crate::operations::fml::resolve::SharedCore;
use crate::operations::sof::term_source::packaged;
use crate::server::AppState;

/// Package structures one connection loads at most, so a model that refers to everything
/// cannot pull in without end.
const MAX_STRUCTURES: usize = 1_000;

#[derive(Debug, Deserialize)]
pub struct FmlLspQuery {
    pub token: Option<String>,
}

/// `GET /api/lsp/fml`: upgrades to the FML language server.
pub async fn fml_lsp_websocket_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Query(query): Query<FmlLspQuery>,
    headers: HeaderMap,
) -> Result<Response, FhirPathLspError> {
    authorize(&state, query.token, &headers, "FML")?;
    let core = core(&state).map(|(_, core)| core);
    Ok(ws.on_upgrade(move |socket| serve(socket, core)))
}

async fn serve(socket: WebSocket, core: Option<Arc<CoreDefinitions>>) {
    let (mut sender, mut receiver) = socket.split();
    let mut session = Session::new(core);
    while let Some(Ok(message)) = receiver.next().await {
        let text = match message {
            Message::Text(text) => text.to_string(),
            Message::Binary(bytes) => match String::from_utf8(bytes.to_vec()) {
                Ok(text) => text,
                Err(_) => continue,
            },
            Message::Close(_) => break,
            Message::Ping(_) | Message::Pong(_) => continue,
        };
        let Ok(message) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        let Some(replies) = session.handle(message).await else {
            break;
        };
        for reply in replies {
            if sender
                .send(Message::Text(reply.to_string().into()))
                .await
                .is_err()
            {
                return;
            }
        }
        if session.exited() {
            break;
        }
    }
}

/// One connection: the language server and the package structures loaded for it.
struct Session {
    /// Taken out while a message is handled on a blocking thread.
    server: Option<Server>,
    core: Option<Arc<CoreDefinitions>>,
    structures: Vec<Value>,
    looked_up: HashSet<String>,
    exited: bool,
}

impl Session {
    fn new(core: Option<Arc<CoreDefinitions>>) -> Self {
        let definitions: fml_lsp::SharedDefinitions = match &core {
            Some(core) => Arc::new(SharedCore(core.clone())),
            None => Arc::new(NoDefinitions),
        };
        Self {
            server: Some(Server::new(definitions)),
            core,
            structures: Vec::new(),
            looked_up: HashSet::new(),
            exited: false,
        }
    }

    fn exited(&self) -> bool {
        self.exited
    }

    /// Handles a message: loads the structures a changed map needs, then lets the server
    /// answer. Checking a large map takes a while, so that runs on a blocking thread. `None`
    /// when that thread failed.
    async fn handle(&mut self, message: Value) -> Option<Vec<Value>> {
        let mut replies = Vec::new();
        if let Some(text) = document_text(&message) {
            let urls = used_structures(text);
            if let Some(definitions) = self.load(urls).await {
                let mut server = self.server.take()?;
                let (server, notifications) = tokio::task::spawn_blocking(move || {
                    let notifications = server.set_definitions(definitions);
                    (server, notifications)
                })
                .await
                .ok()?;
                self.server = Some(server);
                // The document being changed is published again below.
                replies.extend(notifications);
            }
        }
        let mut server = self.server.take()?;
        let (server, answers) = tokio::task::spawn_blocking(move || {
            let answers = server.handle(&message);
            (server, answers)
        })
        .await
        .ok()?;
        self.exited = server.exited();
        self.server = Some(server);
        replies.extend(answers);
        Some(replies)
    }

    /// Looks up the structures at `urls` not looked up before, and those they refer to, in the
    /// installed packages. The new definitions when any was found.
    async fn load(&mut self, mut pending: Vec<String>) -> Option<fml_lsp::SharedDefinitions> {
        let core = self.core.clone()?;
        let before = self.structures.len();
        while let Some(canonical) = pending.pop() {
            if self.structures.len() >= MAX_STRUCTURES {
                break;
            }
            if !self.looked_up.insert(canonical.clone()) {
                continue;
            }
            let (url, version) = split_canonical(&canonical);
            // Packages also define structures in the core namespace (the SQL on FHIR
            // ViewDefinition is `http://hl7.org/fhir/StructureDefinition/ViewDefinition`), so
            // only what the core has is skipped.
            if core.structure(url).is_some() {
                continue;
            }
            match packaged("StructureDefinition", url, version).await {
                Ok(Lookup::Found(found)) => {
                    pending.extend(referenced_structures(&found.resource));
                    self.structures.push(found.resource);
                }
                Ok(_) => {}
                Err(error) => {
                    tracing::debug!(url, %error, "FML LSP: structure lookup failed");
                }
            }
        }
        if self.structures.len() == before {
            return None;
        }
        let mut definitions = PackageDefinitions::new(SharedCore(core));
        for structure in &self.structures {
            if let Err(problem) = definitions.add(structure) {
                tracing::debug!(%problem, "FML LSP: structure not usable");
            }
        }
        Some(Arc::new(definitions))
    }
}

/// The text of a document the client opened or changed.
fn document_text(message: &Value) -> Option<&str> {
    let params = message.get("params")?;
    match message.get("method")?.as_str()? {
        "textDocument/didOpen" => params["textDocument"]["text"].as_str(),
        "textDocument/didChange" => params["contentChanges"].as_array()?.last()?["text"].as_str(),
        _ => None,
    }
}

/// The structure URLs of a map's `uses`.
fn used_structures(text: &str) -> Vec<String> {
    fml_syntax::parse(text)
        .tree()
        .uses()
        .filter_map(|uses| uses.url().and_then(|url| url.value()))
        .collect()
}

/// The structures a StructureDefinition refers to: its base, and element types named by URL
/// (logical models name their types that way).
fn referenced_structures(resource: &Value) -> Vec<String> {
    let mut urls: Vec<String> = resource["baseDefinition"]
        .as_str()
        .map(str::to_owned)
        .into_iter()
        .collect();
    for view in ["snapshot", "differential"] {
        for element in resource[view]["element"].as_array().into_iter().flatten() {
            for ty in element["type"].as_array().into_iter().flatten() {
                if let Some(code) = ty["code"].as_str()
                    && code.contains("://")
                {
                    urls.push(code.to_owned());
                }
            }
        }
    }
    urls.sort();
    urls.dedup();
    urls
}
