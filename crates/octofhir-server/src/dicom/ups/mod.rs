//! Built-in UPS-RS Requestor/Watcher integration on durable async jobs.
//!
//! # Contents
//! Explicit configuration, typed work requests, guarded HTTP/WebSocket transport,
//! immutable Task bindings and source-fenced reconciliation state.
//!
//! # Invariants
//! Performers remain external. DICOM outputs stay in PACS; only references and
//! explicitly mapped clinical FHIR projections belong in PostgreSQL.
mod config;
mod model;
mod protocol;
mod store;
mod watch;
pub use config::{DicomUpsConfig, ResultKind, ResultMapping};

mod operation;
mod publication;
mod worker;
pub(crate) use operation::operate;
pub(crate) use store::JOB;
pub(crate) use worker::execute;

use crate::{lifecycle::BackgroundRuntime, operations::OperationContext, server::AppState};
use std::time::Duration;
/// Validate identities before serving, then attach work to the embedding runtime's
/// cancellation boundary. No detached global worker or second executor exists.
pub(crate) async fn start(state: &AppState, background: &BackgroundRuntime) -> Result<(), String> {
    for (name, source) in &state.config.dicom.connections {
        let Some(config) = &source.ups else {
            continue;
        };
        let context = OperationContext::for_service(state, &config.client_id, &config.scopes)
            .await
            .map_err(|_| "UPS backend identity is unavailable")?;
        use octofhir_auth::smart::scopes::FhirOperation::{Create, Read, Update};
        let mut required = vec![
            ("Task", Read),
            ("Task", Update),
            ("Patient", Read),
            ("ImagingStudy", Read),
            ("ImagingStudy", Create),
            ("ImagingStudy", Update),
            ("Endpoint", Read),
            ("Endpoint", Create),
            ("Endpoint", Update),
            ("Provenance", Read),
            ("Provenance", Create),
            ("Provenance", Update),
        ];
        for mapping in config.result_mappings.values() {
            let kind = match mapping.kind {
                ResultKind::DocumentReference => "DocumentReference",
                ResultKind::DiagnosticReport => "DiagnosticReport",
            };
            required.extend([(kind, Read), (kind, Create), (kind, Update)]);
        }
        for (kind, operation) in required {
            context
                .policy_context(
                    state,
                    operation,
                    kind,
                    None,
                    &octofhir_storage::SearchParams::new(),
                )
                .map_err(|_| "UPS backend lacks required explicit resource scopes")?;
        }
        store::initialize(&state.db_pool, name, &store::fingerprint(source)).await?;
        if config.notifications {
            let state = state.clone();
            let name = name.clone();
            background.spawn(watch::run(state, name));
        }
    }
    if !state
        .config
        .dicom
        .connections
        .values()
        .any(|c| c.ups.is_some())
    {
        return Ok(());
    }
    let state = state.clone();
    background.spawn(async move {
        let mut timer = tokio::time::interval(Duration::from_secs(1));
        loop {
            timer.tick().await;
            for (name, source) in &state.config.dicom.connections {
                let Some(config) = &source.ups else {
                    continue;
                };
                let Ok(context) =
                    OperationContext::for_service(&state, &config.client_id, &config.scopes).await
                else {
                    continue;
                };
                if store::dispatch(&state, &context, name, &store::fingerprint(source))
                    .await
                    .is_err()
                {
                    tracing::warn!(connection=%name,"Cannot schedule UPS reconciliation");
                }
            }
        }
    });
    Ok(())
}
