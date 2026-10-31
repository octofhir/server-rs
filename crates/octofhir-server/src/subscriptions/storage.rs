//! Storage layer for subscription events.
//!
//! Provides database operations for the subscription event queue using PostgreSQL.

use std::sync::Arc;

use sqlx_core::query::query;
use sqlx_core::query_scalar::query_scalar;
use sqlx_core::row::Row;
use sqlx_postgres::{PgPool, PgRow};
use time::OffsetDateTime;
use tokio::sync::Notify;
use uuid::Uuid;

use super::error::{SubscriptionError, SubscriptionResult};
use super::types::{
    EventStatus, NotificationBundleBuilder, SubscriptionEvent, SubscriptionEventType,
    TriggerInteraction,
};

/// Storage for subscription events.
#[derive(Clone)]
pub struct SubscriptionEventStorage {
    pool: PgPool,
    /// Notify the delivery processor when new events are enqueued.
    event_notify: Arc<Notify>,
}

impl SubscriptionEventStorage {
    /// Create a new event storage instance.
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            event_notify: Arc::new(Notify::new()),
        }
    }

    /// Get the notify handle for waking up the delivery processor.
    pub fn event_notify(&self) -> Arc<Notify> {
        self.event_notify.clone()
    }

    /// Enqueue a new subscription event for delivery.
    pub async fn enqueue(
        &self,
        subscription_id: &str,
        topic_url: &str,
        event_type: SubscriptionEventType,
        focus_resource_type: Option<&str>,
        focus_resource_id: Option<&str>,
        focus_event: Option<TriggerInteraction>,
        notification_bundle: serde_json::Value,
    ) -> SubscriptionResult<String> {
        let id = Uuid::now_v7();
        let event_type_str = event_type.as_str();
        let focus_event_str = focus_event.map(|e| match e {
            TriggerInteraction::Create => "create",
            TriggerInteraction::Update => "update",
            TriggerInteraction::Delete => "delete",
        });

        // Get next event number using the helper function
        let event_number: i64 = query_scalar("SELECT next_subscription_event_number($1)")
            .bind(subscription_id)
            .fetch_one(&self.pool)
            .await?;

        query(
            r#"
            INSERT INTO subscription_event (
                id, subscription_id, topic_url, event_type, event_number,
                focus_resource_type, focus_resource_id, focus_event,
                notification_bundle, status, created_at, next_retry_at, expires_at
            ) VALUES (
                $1, $2, $3, $4, $5, $6, $7, $8, $9, 'pending', NOW(), NOW(),
                NOW() + INTERVAL '24 hours'
            )
            "#,
        )
        .bind(id)
        .bind(subscription_id)
        .bind(topic_url)
        .bind(event_type_str)
        .bind(event_number)
        .bind(focus_resource_type)
        .bind(focus_resource_id)
        .bind(focus_event_str)
        .bind(&notification_bundle)
        .execute(&self.pool)
        .await?;

        self.event_notify.notify_one();

        Ok(id.to_string())
    }

    /// Enqueue a handshake event for a newly activated subscription.
    pub async fn enqueue_handshake(
        &self,
        subscription_id: &str,
        topic_url: &str,
    ) -> SubscriptionResult<String> {
        let bundle = NotificationBundleBuilder::new(
            subscription_id.to_string(),
            topic_url.to_string(),
            SubscriptionEventType::Handshake,
            0, // Handshake is event 0
        )
        .build();

        self.enqueue(
            subscription_id,
            topic_url,
            SubscriptionEventType::Handshake,
            None,
            None,
            None,
            bundle,
        )
        .await
    }

    /// Enqueue a heartbeat event.
    pub async fn enqueue_heartbeat(
        &self,
        subscription_id: &str,
        topic_url: &str,
    ) -> SubscriptionResult<String> {
        // Get current event count for this subscription
        let event_count: i64 = query_scalar(
            "SELECT COALESCE(MAX(event_number), 0) FROM subscription_event WHERE subscription_id = $1"
        )
        .bind(subscription_id)
        .fetch_one(&self.pool)
        .await?;

        let bundle = NotificationBundleBuilder::new(
            subscription_id.to_string(),
            topic_url.to_string(),
            SubscriptionEventType::Heartbeat,
            event_count,
        )
        .build();

        self.enqueue(
            subscription_id,
            topic_url,
            SubscriptionEventType::Heartbeat,
            None,
            None,
            None,
            bundle,
        )
        .await
    }

    /// Claim pending events for processing.
    ///
    /// Uses `SELECT FOR UPDATE SKIP LOCKED` for distributed processing.
    pub async fn claim_events(&self, limit: i32) -> SubscriptionResult<Vec<SubscriptionEvent>> {
        let rows: Vec<PgRow> = query("SELECT * FROM claim_subscription_events($1)")
            .bind(limit)
            .fetch_all(&self.pool)
            .await?;

        let mut events = Vec::with_capacity(rows.len());
        for row in rows {
            events.push(self.row_to_event(&row)?);
        }

        Ok(events)
    }

    /// Mark an event as successfully delivered.
    pub async fn mark_delivered(&self, event_id: &str) -> SubscriptionResult<()> {
        let id =
            Uuid::parse_str(event_id).map_err(|e| SubscriptionError::ParseError(e.to_string()))?;

        query("SELECT mark_event_delivered($1)")
            .bind(id)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    /// Mark an event for retry with exponential backoff.
    pub async fn mark_retry(&self, event_id: &str, error: Option<&str>) -> SubscriptionResult<()> {
        let id =
            Uuid::parse_str(event_id).map_err(|e| SubscriptionError::ParseError(e.to_string()))?;

        query("SELECT mark_event_retry($1, $2)")
            .bind(id)
            .bind(error)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    /// Get an event by ID.
    pub async fn get_event(&self, event_id: &str) -> SubscriptionResult<Option<SubscriptionEvent>> {
        let id =
            Uuid::parse_str(event_id).map_err(|e| SubscriptionError::ParseError(e.to_string()))?;

        let row: Option<PgRow> = query("SELECT * FROM subscription_event WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;

        match row {
            Some(row) => Ok(Some(self.row_to_event(&row)?)),
            None => Ok(None),
        }
    }

    /// Get pending events for a subscription.
    pub async fn get_pending_events(
        &self,
        subscription_id: &str,
        limit: i32,
    ) -> SubscriptionResult<Vec<SubscriptionEvent>> {
        let rows: Vec<PgRow> = query(
            r#"
            SELECT * FROM subscription_event
            WHERE subscription_id = $1 AND status = 'pending'
            ORDER BY event_number ASC
            LIMIT $2
            "#,
        )
        .bind(subscription_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        let mut events = Vec::with_capacity(rows.len());
        for row in rows {
            events.push(self.row_to_event(&row)?);
        }

        Ok(events)
    }

    /// Get event statistics for a subscription (for $status operation).
    pub async fn get_subscription_stats(
        &self,
        subscription_id: &str,
    ) -> SubscriptionResult<SubscriptionStats> {
        let row: Option<PgRow> = query(
            r#"
            SELECT
                COALESCE(events_since_subscription_start, 0) as total_events,
                COALESCE(error_count, 0) as error_count,
                last_error_at,
                last_error_message,
                last_delivery_at,
                last_event_number,
                topic_url
            FROM subscription_status
            WHERE subscription_id = $1
            "#,
        )
        .bind(subscription_id)
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some(row) => Ok(SubscriptionStats {
                events_since_subscription_start: row.get("total_events"),
                error_count: row.get("error_count"),
                last_error_at: row.get("last_error_at"),
                last_error_message: row.get("last_error_message"),
                last_delivery_at: row.get("last_delivery_at"),
                last_event_number: row.get("last_event_number"),
                topic_url: row.get("topic_url"),
            }),
            None => Ok(SubscriptionStats::default()),
        }
    }

    /// Recent events of one subscription, newest first, each with its latest delivery attempt.
    pub async fn recent_events(
        &self,
        subscription_id: &str,
        limit: i64,
    ) -> SubscriptionResult<Vec<EventLogEntry>> {
        let rows = query(
            r#"
            SELECT
                e.id, e.event_type, e.event_number, e.focus_resource_type, e.focus_resource_id,
                e.focus_event, e.status, e.attempts, e.last_error, e.created_at, e.delivered_at,
                e.next_retry_at, d.http_status, d.response_time_ms, d.error_message
            FROM subscription_event e
            LEFT JOIN LATERAL (
                SELECT http_status, response_time_ms, error_message
                FROM subscription_delivery
                WHERE event_id = e.id
                ORDER BY attempt_number DESC
                LIMIT 1
            ) d ON true
            WHERE e.subscription_id = $1
            ORDER BY e.created_at DESC
            LIMIT $2
            "#,
        )
        .bind(subscription_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter()
            .map(|row| {
                Ok(EventLogEntry {
                    id: row.try_get::<Uuid, _>("id")?.to_string(),
                    event_type: row.try_get("event_type")?,
                    event_number: row.try_get("event_number")?,
                    focus_resource_type: row.try_get("focus_resource_type")?,
                    focus_resource_id: row.try_get("focus_resource_id")?,
                    focus_event: row.try_get("focus_event")?,
                    status: row.try_get("status")?,
                    attempts: row.try_get("attempts")?,
                    error: row
                        .try_get::<Option<String>, _>("error_message")?
                        .or(row.try_get("last_error")?),
                    http_status: row.try_get("http_status")?,
                    response_time_ms: row.try_get("response_time_ms")?,
                    created_at: row.try_get("created_at")?,
                    delivered_at: row.try_get("delivered_at")?,
                    next_retry_at: row.try_get("next_retry_at")?,
                })
            })
            .collect()
    }

    /// Notification events numbered within `[since, until]`, oldest first (for `$events`).
    pub async fn notification_events(
        &self,
        subscription_id: &str,
        since: Option<i64>,
        until: Option<i64>,
        limit: i64,
    ) -> SubscriptionResult<Vec<NotificationEventRow>> {
        let rows = query(
            r#"
            SELECT event_number, created_at, focus_resource_type, focus_resource_id
            FROM subscription_event
            WHERE subscription_id = $1
              AND event_type = 'event-notification'
              AND ($2::BIGINT IS NULL OR event_number >= $2)
              AND ($3::BIGINT IS NULL OR event_number <= $3)
            ORDER BY event_number ASC
            LIMIT $4
            "#,
        )
        .bind(subscription_id)
        .bind(since)
        .bind(until)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter()
            .map(|row| {
                Ok(NotificationEventRow {
                    event_number: row.try_get("event_number")?,
                    created_at: row.try_get("created_at")?,
                    focus_resource_type: row.try_get("focus_resource_type")?,
                    focus_resource_id: row.try_get("focus_resource_id")?,
                })
            })
            .collect()
    }

    /// Record a delivery attempt.
    pub async fn record_delivery_attempt(
        &self,
        event_id: &str,
        subscription_id: &str,
        attempt_number: i32,
        channel_type: &str,
        success: bool,
        http_status: Option<i32>,
        response_time_ms: i32,
        error_message: Option<&str>,
    ) -> SubscriptionResult<()> {
        let id = Uuid::now_v7();
        let event_uuid =
            Uuid::parse_str(event_id).map_err(|e| SubscriptionError::ParseError(e.to_string()))?;

        query(
            r#"
            INSERT INTO subscription_delivery (
                id, event_id, subscription_id, attempt_number, channel_type,
                started_at, completed_at, success, http_status, response_time_ms,
                error_message
            ) VALUES (
                $1, $2, $3, $4, $5, NOW(), NOW(), $6, $7, $8, $9
            )
            "#,
        )
        .bind(id)
        .bind(event_uuid)
        .bind(subscription_id)
        .bind(attempt_number)
        .bind(channel_type)
        .bind(success)
        .bind(http_status)
        .bind(response_time_ms)
        .bind(error_message)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Cleanup old events.
    pub async fn cleanup(
        &self,
        delivered_retention_hours: i32,
        failed_retention_hours: i32,
    ) -> SubscriptionResult<i32> {
        let deleted: i32 = query_scalar("SELECT cleanup_subscription_events($1, $2)")
            .bind(delivered_retention_hours)
            .bind(failed_retention_hours)
            .fetch_one(&self.pool)
            .await?;

        Ok(deleted)
    }

    /// Convert a database row to a SubscriptionEvent.
    fn row_to_event(&self, row: &PgRow) -> SubscriptionResult<SubscriptionEvent> {
        let id: Uuid = row.get("id");
        let status_str: String = row.get("status");
        let event_type_str: String = row.get("event_type");
        let focus_event_str: Option<String> = row.get("focus_event");
        let created_at: OffsetDateTime = row.get("created_at");
        let next_retry_at: Option<OffsetDateTime> = row.get("next_retry_at");

        let event_type = match event_type_str.as_str() {
            "handshake" => SubscriptionEventType::Handshake,
            "heartbeat" => SubscriptionEventType::Heartbeat,
            _ => SubscriptionEventType::EventNotification,
        };

        let focus_event = focus_event_str.map(|s| match s.as_str() {
            "create" => TriggerInteraction::Create,
            "update" => TriggerInteraction::Update,
            "delete" => TriggerInteraction::Delete,
            _ => TriggerInteraction::Update,
        });

        Ok(SubscriptionEvent {
            id: id.to_string(),
            subscription_id: row.get("subscription_id"),
            topic_url: row.get("topic_url"),
            event_type,
            event_number: row.get("event_number"),
            focus_resource_type: row.get("focus_resource_type"),
            focus_resource_id: row.get("focus_resource_id"),
            focus_event,
            notification_bundle: row.get("notification_bundle"),
            status: EventStatus::from(status_str.as_str()),
            created_at,
            attempts: row.get("attempts"),
            next_retry_at,
            last_error: row.get("last_error"),
        })
    }
}

/// Statistics for a subscription (used by $status operation).
#[derive(Debug, Clone, Default)]
pub struct SubscriptionStats {
    pub events_since_subscription_start: i64,
    pub error_count: i32,
    pub last_error_at: Option<OffsetDateTime>,
    pub last_error_message: Option<String>,
    pub last_delivery_at: Option<OffsetDateTime>,
    pub last_event_number: i64,
    pub topic_url: Option<String>,
}

/// One queued notification and how its delivery went, for the console's event log.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EventLogEntry {
    pub id: String,
    pub event_type: String,
    pub event_number: i64,
    pub focus_resource_type: Option<String>,
    pub focus_resource_id: Option<String>,
    pub focus_event: Option<String>,
    /// pending, processing, delivered, failed or expired.
    pub status: String,
    pub attempts: i32,
    /// The latest attempt's error, else the queue's last error.
    pub error: Option<String>,
    pub http_status: Option<i32>,
    pub response_time_ms: Option<i32>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    pub delivered_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub next_retry_at: Option<OffsetDateTime>,
}

/// One past notification, as `$events` reports it.
#[derive(Debug, Clone)]
pub struct NotificationEventRow {
    pub event_number: i64,
    pub created_at: OffsetDateTime,
    pub focus_resource_type: Option<String>,
    pub focus_resource_id: Option<String>,
}
