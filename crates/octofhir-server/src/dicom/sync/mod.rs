//! Durable, source-scoped DICOM discovery and reconciliation.
//!
//! # Contents
//! Explicit backend identity, bounded full scans, ordered notifications, queue
//! coalescing and checkpointed jobs using the server's existing fenced executor.
//!
//! # Invariants
//! StudyDate is never a change cursor. Scan offsets advance atomically with enqueue.
//! Deleted/rejected state requires an ordered explicit source notification. Missing
//! responses and network failures are retries, never inferred deletion events.

mod config;
pub(crate) mod store;
mod worker;
pub use config::{DicomSyncConfig, DiscoveryMode};
pub(crate) use store::JOB;
pub(crate) use worker::execute;

use super::client::DicomWebClient;
use crate::{lifecycle::BackgroundRuntime, operations::OperationContext, server::AppState};
use sqlx_core::{connection::Connection, query::query, query_scalar::query_scalar, row::Row};
use std::sync::Arc;

pub(crate) async fn start(state: &AppState, background: &BackgroundRuntime) -> Result<(), String> {
    for (name, connection) in &state.config.dicom.connections {
        if let Some(sync) = &connection.sync {
            let context = OperationContext::for_service(state, &sync.client_id, &sync.scopes)
                .await
                .map_err(|_| "DICOM sync backend identity is unavailable or unauthorized")?;
            use octofhir_auth::smart::scopes::FhirOperation;
            for (kind, operation) in [
                ("Patient", FhirOperation::Read),
                ("Patient", FhirOperation::Search),
                ("ImagingStudy", FhirOperation::Read),
                ("ImagingStudy", FhirOperation::Create),
                ("ImagingStudy", FhirOperation::Update),
                ("Endpoint", FhirOperation::Read),
                ("Endpoint", FhirOperation::Create),
                ("Endpoint", FhirOperation::Update),
            ] {
                context.policy_context(state,operation,kind,None,&octofhir_storage::SearchParams::new())
                    .map_err(|_|"DICOM sync backend lacks required Patient/ImagingStudy/Endpoint scopes")?;
            }
            store::initialize(&state.db_pool, name, &store::fingerprint(connection)).await?;
        }
    }
    if !state
        .config
        .dicom
        .connections
        .values()
        .any(|c| c.sync.is_some())
    {
        return Ok(());
    }
    let state = state.clone();
    background.spawn(async move {
        let mut timer=tokio::time::interval(std::time::Duration::from_secs(1));
        loop {
            timer.tick().await;
            for(name,connection)in &state.config.dicom.connections {
                let Some(sync)=&connection.sync else{continue;};
                let context=match OperationContext::for_service(&state,&sync.client_id,&sync.scopes).await {
                    Ok(context)=>context,Err(_)=>{tracing::warn!(connection=%name,"DICOM service identity is unavailable");continue;}
                };
                let fingerprint=store::fingerprint(connection);
                if sync.discovery==DiscoveryMode::Scan && scan(&state,name,connection).await.is_err() {
                    let _=query("UPDATE dicom_sync_sources SET last_error='discovery_failed',scan_not_before=clock_timestamp()+make_interval(secs=>$2::double precision) WHERE connection_name=$1")
                        .bind(name).bind(sync.retry_seconds as f64).execute(state.db_pool.as_ref()).await;
                }
                if store::dispatch(&state,&context,name,&fingerprint).await.is_err() {
                    tracing::warn!(connection=%name,"Cannot schedule DICOM reconciliation");
                }
            }
        }
    });
    Ok(())
}

async fn scan(
    state: &AppState,
    name: &str,
    connection: &super::DicomConnection,
) -> Result<(), String> {
    let sync = connection
        .sync
        .as_ref()
        .ok_or("Synchronization is disabled")?;
    let due=query("SELECT 1 FROM dicom_sync_sources WHERE connection_name=$1 AND scan_not_before<=clock_timestamp()")
        .bind(name).fetch_optional(state.db_pool.as_ref()).await.map_err(|_|"Cannot read DICOM scan schedule")?.is_some();
    if !due {
        return Ok(());
    }
    let mut lock = state
        .dicom_lock_pool
        .acquire()
        .await
        .map_err(|_| "DICOM scan capacity is busy")?;
    lock.close_on_drop();
    let held: bool = query_scalar("SELECT pg_try_advisory_lock(hashtextextended($1,0))")
        .bind(format!("dicom-scan:{name}"))
        .fetch_one(&mut *lock)
        .await
        .map_err(|_| "Cannot lock DICOM scan")?;
    if !held {
        return Ok(());
    }
    let client = DicomWebClient::new(
        connection,
        Arc::new(crate::egress::EgressPolicy::from_server_config(
            &state.config.server,
        )?),
    )
    .map_err(|_| "Cannot create QIDO client")?;
    for _ in 0..sync.pages_per_run {
        // Another replica may have finished a scan before this session acquired its lock.
        let Some(row)=query("SELECT scan_offset FROM dicom_sync_sources WHERE connection_name=$1 AND scan_not_before<=clock_timestamp()")
            .bind(name).fetch_optional(&mut *lock).await.map_err(|_|"Cannot read DICOM scan checkpoint")? else{break;};
        let offset: i64 = row.get("scan_offset");
        if offset as u64 >= sync.max_studies_per_scan as u64 {
            return Err("DICOM scan capacity exceeded".into());
        }
        let records = client
            .query_studies(offset as u64, sync.page_size, None)
            .await
            .map_err(|_| "QIDO scan failed")?;
        let next = offset + records.len() as i64;
        if next > sync.max_studies_per_scan as i64 {
            return Err("DICOM scan capacity exceeded".into());
        }
        let empty = records.is_empty();
        let mut tx = lock
            .begin()
            .await
            .map_err(|_| "Cannot begin DICOM scan checkpoint")?;
        for record in records {
            store::enqueue(
                &mut tx,
                name,
                &record.uid,
                record.instances,
                None,
                store::Change::Changed,
            )
            .await?;
        }
        if empty {
            query("UPDATE dicom_sync_sources SET scan_offset=0,last_scan_at=clock_timestamp(),last_error=NULL,scan_not_before=clock_timestamp()+make_interval(secs=>$2::double precision),updated_at=clock_timestamp() WHERE connection_name=$1")
                .bind(name).bind(sync.rescan_seconds as f64).execute(&mut *tx).await.map_err(|_|"Cannot finish DICOM scan")?;
        } else {
            query("UPDATE dicom_sync_sources SET scan_offset=$2,last_error=NULL,updated_at=clock_timestamp() WHERE connection_name=$1")
                .bind(name).bind(next).execute(&mut *tx).await.map_err(|_|"Cannot checkpoint DICOM scan")?;
        }
        tx.commit()
            .await
            .map_err(|_| "Cannot commit DICOM scan checkpoint")?;
        if empty {
            break;
        }
    }
    Ok(())
}
