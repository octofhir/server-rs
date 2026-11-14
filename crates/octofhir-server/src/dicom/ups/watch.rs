//! Fenced UPS notifications as wakeups, with query-based recovery and deletion locks.
//!
//! # Contents
//! Egress-checked WebSocket transport, one source leader, filtered subscriptions,
//! bounded frames, leased polling fallback and terminal subscription release.
//!
//! # Invariants
//! Attempt the notification connection before subscribing. Only the persistent source
//! label namespace is subscribed, never the entire archive. Event bodies never
//! become clinical state or durable replay cursors. A fresh GET drives every write.
//! Deletion locks are released only after the terminal FHIR checkpoint is applied.
//! Poll-only status never bypasses the live subscription readiness lease.
use super::{config::DicomUpsConfig, protocol::Client};
use crate::{operations::OperationContext, server::AppState};
use futures_util::{SinkExt, StreamExt};
use sqlx_core::{query::query, query_scalar::query_scalar, row::Row};
use std::{sync::Arc, time::Duration};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, client_async_tls_with_config,
    tungstenite::{
        client::IntoClientRequest,
        protocol::{Message, WebSocketConfig},
    },
};

pub(super) async fn run(state: AppState, connection: String) {
    let retry = state.config.dicom.connections[&connection]
        .ups
        .as_ref()
        .expect("UPS configuration")
        .retry_seconds;
    loop {
        let execution = uuid::Uuid::new_v4();
        if session(&state, &connection, execution).await.is_err() {
            let _=query("UPDATE dicom_ups_sources SET watch_ready_until=NULL,watch_error='notification_connection_failed',updated_at=clock_timestamp() WHERE connection_name=$1 AND watch_execution=$2").bind(&connection).bind(execution).execute(state.db_pool.as_ref()).await;
        }
        tokio::time::sleep(Duration::from_secs(retry)).await;
    }
}
async fn session(state: &AppState, name: &str, execution: uuid::Uuid) -> Result<(), String> {
    let connection = &state.config.dicom.connections[name];
    let config = connection
        .ups
        .as_ref()
        .ok_or("UPS notifications are disabled")?;
    OperationContext::for_service(state, &config.client_id, &config.scopes)
        .await
        .map_err(|_| "UPS service identity is unavailable")?;
    let mut lock = state
        .dicom_watch_pool
        .acquire()
        .await
        .map_err(|_| "UPS watcher capacity is busy")?;
    lock.close_on_drop();
    let held: bool = query_scalar("SELECT pg_try_advisory_lock(hashtextextended($1,0))")
        .bind(format!("dicom-ups-watch:{name}"))
        .fetch_one(&mut *lock)
        .await
        .map_err(|_| "Cannot lock UPS watcher")?;
    if !held {
        return Ok(());
    }
    query("UPDATE dicom_ups_sources SET watch_execution=$2,watch_ready_until=NULL,poll_only=false WHERE connection_name=$1").bind(name).bind(execution).execute(&mut *lock).await.map_err(|_|"Cannot fence UPS watcher")?;
    let prefix: String =
        query_scalar("SELECT subscription_prefix FROM dicom_ups_sources WHERE connection_name=$1")
            .bind(name)
            .fetch_one(&mut *lock)
            .await
            .map_err(|_| "Cannot read UPS notification namespace")?;
    let policy = Arc::new(crate::egress::EgressPolicy::from_server_config(
        &state.config.server,
    )?);
    let mut socket = connect(state, name, policy.clone()).await.ok();
    // Never infer subscription durability from an open WebSocket. Establish the
    // filtered deletion lock, then recover every known active item by fresh queries.
    let client = Client::new(connection, policy.clone())?;
    client.subscribe(&prefix).await?;
    if socket.is_none() {
        tracing::warn!(
            connection = name,
            "UPS notifications unavailable; using polling with a filtered deletion lock"
        );
    }
    wake(state, name).await?;
    heartbeat(state, name, execution, config, socket.is_none(), &mut lock).await?;
    let mut timer = tokio::time::interval(Duration::from_secs(5));
    let mut last_seen = tokio::time::Instant::now();
    let mut last_wake = last_seen - Duration::from_secs(1);
    // Keep renewing readiness and releasing completed locks while attempts to
    // restore the notification channel run, including slow TCP/TLS handshakes.
    let reconnect = async {
        loop {
            tokio::time::sleep(Duration::from_secs(config.retry_seconds.max(1))).await;
            if let Ok(socket) = connect(state, name, policy.clone()).await {
                return socket;
            }
        }
    };
    tokio::pin!(reconnect);
    loop {
        tokio::select! {
            _ = timer.tick() => {
                if socket.is_some() && last_seen.elapsed() > Duration::from_secs(30) {
                    return Err("UPS notification heartbeat expired".into());
                }
                heartbeat(state, name, execution, config, socket.is_none(), &mut lock).await?;
                release_completed(state, name, &client, execution).await?;
                if let Some(socket) = socket.as_mut() {
                    tokio::time::timeout(
                        Duration::from_secs(5),
                        socket.send(Message::Ping(Vec::new().into())),
                    )
                        .await
                        .map_err(|_| "UPS ping timed out")?
                        .map_err(|_| "UPS ping failed")?;
                }
            }
            restored = &mut reconnect, if socket.is_none() => {
                // A new channel is not proof of subscription durability. Renew
                // the filtered subscription and revisit terminal releases, since
                // subscribing again may reacquire their deletion locks.
                client.subscribe(&prefix).await?;
                query(
                    "UPDATE dicom_ups_tasks SET subscription_release_execution=NULL
                     WHERE connection_name=$1 AND subscription_release_execution=$2
                       AND EXISTS(SELECT 1 FROM dicom_ups_sources
                                  WHERE connection_name=$1 AND watch_execution=$2)",
                )
                .bind(name)
                .bind(execution)
                .execute(state.db_pool.as_ref())
                .await
                .map_err(|_| "Cannot reset UPS subscription releases")?;
                wake(state, name).await?;
                socket = Some(restored);
                last_seen = tokio::time::Instant::now();
                heartbeat(state, name, execution, config, false, &mut lock).await?;
            }
            message = async {
                socket.as_mut().expect("active notification channel").next().await
            }, if socket.is_some() => {
                let message = message.ok_or("UPS notification connection closed")?
                    .map_err(|_| "UPS notification transport failed")?;
                last_seen = tokio::time::Instant::now();
                match message {
                    Message::Text(text) => {
                        let dataset: serde_json::Value = serde_json::from_str(&text)
                            .map_err(|_| "Invalid UPS event JSON")?;
                        if !(dataset.is_object() || dataset.as_array().is_some_and(|rows| {
                            rows.len() == 1 && rows[0].is_object()
                        })) {
                            return Err("Invalid UPS event dataset".into());
                        }
                        if last_wake.elapsed() >= Duration::from_secs(1) {
                            wake(state, name).await?;
                            last_wake = tokio::time::Instant::now();
                        }
                    }
                    Message::Ping(_) | Message::Pong(_) => {}
                    Message::Close(_) => return Err("UPS notification connection closed".into()),
                    _ => return Err("UPS event reports require text frames".into()),
                }
            }
        }
    }
}

type Socket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

async fn connect(
    state: &AppState,
    name: &str,
    policy: Arc<crate::egress::EgressPolicy>,
) -> Result<Socket, String> {
    let connection = &state.config.dicom.connections[name];
    let config = connection
        .ups
        .as_ref()
        .ok_or("UPS notifications are disabled")?;
    let url = config.notification_url(connection)?;
    let mut transport = url.clone();
    let tls = transport.scheme() == "wss";
    transport
        .set_scheme(if tls { "https" } else { "http" })
        .map_err(|_| "Invalid UPS notification scheme")?;
    let socket = tokio::time::timeout(
        Duration::from_secs(connection.timeout_seconds),
        policy.connect_tcp(&transport),
    )
    .await
    .map_err(|_| "UPS notification connection timed out")??;
    let mut request = url
        .as_str()
        .into_client_request()
        .map_err(|_| "Invalid UPS WebSocket request")?;
    let headers = request.headers_mut();
    headers.insert(
        "content-type",
        "application/dicom+json".parse().expect("constant header"),
    );
    headers.insert(
        "origin",
        url::Url::parse(&state.base_url)
            .map_err(|_| "Invalid UPS Origin")?
            .origin()
            .ascii_serialization()
            .parse()
            .map_err(|_| "Invalid UPS Origin")?,
    );
    if let Some(variable) = &config.bearer_token_env {
        let token =
            std::env::var(variable).map_err(|_| "UPS notification credential is unavailable")?;
        if token.trim().is_empty() {
            return Err("UPS notification credential is empty".into());
        }
        let mut credential = format!("Bearer {token}")
            .parse::<tokio_tungstenite::tungstenite::http::HeaderValue>()
            .map_err(|_| "Invalid UPS notification credential")?;
        credential.set_sensitive(true);
        headers.insert("authorization", credential);
    }
    let limits = WebSocketConfig::default()
        .max_message_size(Some(64 * 1024))
        .max_frame_size(Some(64 * 1024))
        .write_buffer_size(16 * 1024)
        .max_write_buffer_size(64 * 1024);
    let (socket, _) = tokio::time::timeout(
        Duration::from_secs(connection.timeout_seconds),
        client_async_tls_with_config(request, socket, Some(limits), None),
    )
    .await
    .map_err(|_| "UPS notification handshake timed out")?
    .map_err(|_| "UPS notification handshake failed")?;
    Ok(socket)
}

async fn heartbeat(
    state: &AppState,
    name: &str,
    execution: uuid::Uuid,
    config: &DicomUpsConfig,
    poll_only: bool,
    lock: &mut sqlx_postgres::PgConnection,
) -> Result<(), String> {
    OperationContext::for_service(state, &config.client_id, &config.scopes)
        .await
        .map_err(|_| "UPS service identity is unavailable")?;
    // Renew through the connection that holds the advisory lock. A lost lock
    // connection must stop this leader from advertising subscription readiness.
    let updated=query("UPDATE dicom_ups_sources SET watch_ready_until=clock_timestamp()+interval '15 seconds',watch_error=CASE WHEN $3 THEN 'notification_connection_unavailable' ELSE NULL END,poll_only=$3,updated_at=clock_timestamp() WHERE connection_name=$1 AND watch_execution=$2").bind(name).bind(execution).bind(poll_only).execute(lock).await.map_err(|_|"Cannot renew UPS watcher")?;
    if updated.rows_affected() != 1 {
        return Err("UPS watcher lost its source fence".into());
    }
    Ok(())
}
async fn wake(state: &AppState, name: &str) -> Result<(), String> {
    query("UPDATE dicom_ups_tasks SET desired_revision=CASE WHEN pending_job IS NULL THEN GREATEST(desired_revision,applied_revision+1) ELSE desired_revision+1 END,next_attempt_at=clock_timestamp() WHERE connection_name=$1 AND NOT cancelled_before_submission AND (remote_state IS NULL OR remote_state NOT IN ('COMPLETED','CANCELED'))").bind(name).execute(state.db_pool.as_ref()).await.map_err(|_|"Cannot schedule UPS event reconciliation")?;
    Ok(())
}
pub(super) async fn release_completed(
    state: &AppState,
    name: &str,
    client: &Client,
    execution: uuid::Uuid,
) -> Result<(), String> {
    let rows=query("SELECT task_id,workitem_uid FROM dicom_ups_tasks WHERE connection_name=$1 AND remote_state IN ('COMPLETED','CANCELED') AND applied_revision>=desired_revision AND pending_job IS NULL AND subscription_release_execution IS DISTINCT FROM $2 ORDER BY task_id LIMIT 16").bind(name).bind(execution).fetch_all(state.db_pool.as_ref()).await.map_err(|_|"Cannot select completed UPS subscriptions")?;
    for row in rows {
        let uid: String = row.get("workitem_uid");
        client.unsubscribe(&uid).await?;
        query("UPDATE dicom_ups_tasks SET subscription_release_execution=$2 WHERE task_id=$1 AND remote_state IN ('COMPLETED','CANCELED') AND applied_revision>=desired_revision AND pending_job IS NULL AND EXISTS(SELECT 1 FROM dicom_ups_sources WHERE connection_name=$3 AND watch_execution=$2)").bind(row.get::<String,_>("task_id")).bind(execution).bind(name).execute(state.db_pool.as_ref()).await.map_err(|_|"Cannot acknowledge UPS subscription release")?;
    }
    Ok(())
}
