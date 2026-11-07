//! Subscription endpoints beyond CRUD: the R5 `$events` history and the console's delivery log.
//!
//! - `GET /fhir/Subscription/{id}/$events` (no WebSocket upgrade) - past notifications, R5 style
//! - `GET /api/subscriptions/{id}/events` - recent notifications with their delivery outcome
//! - `POST /api/subscriptions/{id}/ping` - queue a heartbeat to check the channel end to end

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use octofhir_api::ApiError;
use serde::Deserialize;
use serde_json::json;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use super::types::{ActiveSubscription, SubscriptionStatus};
use crate::server::AppState;

const DEFAULT_EVENTS: i64 = 50;
const MAX_EVENTS: i64 = 500;
/// Notifications returned by one `$events` call; ask again from the last number for more.
const HISTORY_PAGE: i64 = 100;

#[derive(Debug, Default, Deserialize)]
pub struct EventLogQuery {
    limit: Option<i64>,
}

/// `$events` input parameters.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventHistoryQuery {
    events_since_number: Option<i64>,
    events_until_number: Option<i64>,
}

fn rfc3339(at: Option<OffsetDateTime>) -> Option<String> {
    at.and_then(|at| at.format(&Rfc3339).ok())
}

async fn load(state: &AppState, id: &str) -> Result<ActiveSubscription, ApiError> {
    state
        .subscription_state
        .subscription_manager
        .get_subscription(id)
        .await
        .map_err(|e| ApiError::BadRequest(format!("Subscription/{id} can't be read: {e}")))?
        .ok_or_else(|| ApiError::NotFound(format!("Subscription/{id} not found")))
}

/// GET /api/subscriptions/{id}/events
///
/// Recent notifications for the subscription, newest first, with the latest delivery attempt
/// of each, plus the counters `$status` reports.
pub async fn subscription_event_log(
    Path(id): Path<String>,
    State(state): State<AppState>,
    Query(query): Query<EventLogQuery>,
) -> Result<impl IntoResponse, ApiError> {
    load(&state, &id).await?;
    let storage = &state.subscription_state.event_storage;
    let limit = query.limit.unwrap_or(DEFAULT_EVENTS).clamp(1, MAX_EVENTS);
    let events = storage
        .recent_events(&id, limit)
        .await
        .map_err(|e| ApiError::Internal(format!("Failed to read subscription events: {e}")))?;
    let stats = storage
        .get_subscription_stats(&id)
        .await
        .map_err(|e| ApiError::Internal(format!("Failed to read subscription status: {e}")))?;

    Ok(Json(json!({
        "events": events,
        "eventsSinceSubscriptionStart": stats.events_since_subscription_start,
        "errorCount": stats.error_count,
        "lastErrorMessage": stats.last_error_message,
        "lastErrorAt": rfc3339(stats.last_error_at),
        "lastDeliveryAt": rfc3339(stats.last_delivery_at),
    })))
}

/// POST /api/subscriptions/{id}/ping
///
/// Queues a heartbeat notification so the channel can be checked without changing data.
pub async fn subscription_ping(
    Path(id): Path<String>,
    State(state): State<AppState>,
) -> Result<impl IntoResponse, ApiError> {
    let subscription = load(&state, &id).await?;
    if subscription.status != SubscriptionStatus::Active {
        return Err(ApiError::Conflict(format!(
            "Subscription/{id} is {}; only active subscriptions are notified",
            subscription.status.as_str()
        )));
    }
    let event_id = state
        .subscription_state
        .event_storage
        .enqueue_heartbeat(&id, &subscription.topic_url)
        .await
        .map_err(|e| ApiError::Internal(format!("Failed to queue the notification: {e}")))?;
    Ok((StatusCode::ACCEPTED, Json(json!({ "eventId": event_id }))))
}

/// `GET /fhir/Subscription/{id}/$events` without a WebSocket upgrade.
///
/// Returns a `history` Bundle whose SubscriptionStatus (type `query-event`) lists past
/// notifications with their number, time and focus, so a client that missed deliveries can
/// tell which changes to re-read. At most `HISTORY_PAGE` events per call, oldest first.
pub async fn subscription_event_history(
    state: &AppState,
    id: &str,
    query: &EventHistoryQuery,
) -> Result<Response, ApiError> {
    let subscription = load(state, id).await?;
    let storage = &state.subscription_state.event_storage;
    let events = storage
        .notification_events(
            id,
            query.events_since_number,
            query.events_until_number,
            HISTORY_PAGE,
        )
        .await
        .map_err(|e| ApiError::Internal(format!("Failed to read subscription events: {e}")))?;
    let stats = storage
        .get_subscription_stats(id)
        .await
        .map_err(|e| ApiError::Internal(format!("Failed to read subscription status: {e}")))?;

    let notification_events: Vec<_> = events
        .iter()
        .map(|event| {
            let mut entry = json!({
                // integer64 is a JSON string in FHIR R5.
                "eventNumber": event.event_number.to_string(),
                "timestamp": rfc3339(Some(event.created_at)),
            });
            if let (Some(kind), Some(focus_id)) =
                (&event.focus_resource_type, &event.focus_resource_id)
            {
                entry["focus"] = json!({ "reference": format!("{kind}/{focus_id}") });
            }
            entry
        })
        .collect();

    let status = json!({
        "resourceType": "SubscriptionStatus",
        "status": subscription.status.as_str(),
        "type": "query-event",
        "eventsSinceSubscriptionStart": stats.events_since_subscription_start.to_string(),
        "subscription": { "reference": format!("Subscription/{id}") },
        "topic": subscription.topic_url,
        "notificationEvent": notification_events,
    });
    let bundle = json!({
        "resourceType": "Bundle",
        "type": "history",
        "timestamp": rfc3339(Some(OffsetDateTime::now_utc())),
        "entry": [{
            "fullUrl": format!("urn:uuid:{}", uuid::Uuid::new_v4()),
            "resource": crate::subscriptions::backport::status(status),
            "request": { "method": "GET", "url": format!("Subscription/{id}/$events") },
            "response": { "status": "200" }
        }]
    });
    Ok((
        [(axum::http::header::CONTENT_TYPE, "application/fhir+json")],
        Json(bundle),
    )
        .into_response())
}
