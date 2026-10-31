//! Console endpoints for subscriptions: the delivery log and a test notification.
//!
//! - `GET /api/subscriptions/{id}/events` - recent notifications with their delivery outcome
//! - `POST /api/subscriptions/{id}/ping` - queue a heartbeat to check the channel end to end

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use octofhir_api::ApiError;
use serde::Deserialize;
use serde_json::json;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use super::types::{ActiveSubscription, SubscriptionStatus};
use crate::server::AppState;

const DEFAULT_EVENTS: i64 = 50;
const MAX_EVENTS: i64 = 500;

#[derive(Debug, Default, Deserialize)]
pub struct EventLogQuery {
    limit: Option<i64>,
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
