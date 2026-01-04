//! PostgreSQL storage implementation for notifications.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use dashmap::DashSet;
use sqlx_postgres::PgPool;
use time::OffsetDateTime;
use tracing::{debug, info, instrument};

use octofhir_notifications::{
    Notification, NotificationChannel, NotificationError, NotificationProvider,
    NotificationProviderStorage, NotificationQueueStorage, NotificationRecipient, NotificationStats,
    NotificationStatus,
};

/// PostgreSQL implementation of notification storage.
#[derive(Clone)]
pub struct PostgresNotificationStorage {
    pool: PgPool,
    tables_created: Arc<DashSet<String>>,
}

impl PostgresNotificationStorage {
    /// Create a new PostgreSQL notification storage.
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            tables_created: Arc::new(DashSet::new()),
        }
    }

    /// Ensure the notification tables exist.
    #[instrument(skip(self))]
    async fn ensure_tables(&self) -> Result<(), NotificationError> {
        if self.tables_created.contains("notifications") {
            return Ok(());
        }

        // Create notifications table
        sqlx_core::query::query(
            r#"
            CREATE TABLE IF NOT EXISTS _notifications (
                id TEXT PRIMARY KEY,
                channel TEXT NOT NULL,
                provider_id TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'pending',
                recipient JSONB NOT NULL,
                template_id TEXT NOT NULL,
                template_data JSONB NOT NULL DEFAULT '{}',
                scheduled_at TIMESTAMPTZ,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                sent_at TIMESTAMPTZ,
                error TEXT,
                retry_count INTEGER NOT NULL DEFAULT 0,
                next_retry_at TIMESTAMPTZ
            )
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| NotificationError::Internal(e.to_string()))?;

        // Create indexes
        sqlx_core::query::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_notifications_status ON _notifications(status);
            CREATE INDEX IF NOT EXISTS idx_notifications_scheduled ON _notifications(scheduled_at) WHERE scheduled_at IS NOT NULL;
            CREATE INDEX IF NOT EXISTS idx_notifications_next_retry ON _notifications(next_retry_at) WHERE next_retry_at IS NOT NULL;
            CREATE INDEX IF NOT EXISTS idx_notifications_provider ON _notifications(provider_id);
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| NotificationError::Internal(e.to_string()))?;

        info!("Created notifications queue table");
        self.tables_created.insert("notifications".to_string());
        Ok(())
    }

    fn parse_channel(s: &str) -> NotificationChannel {
        match s {
            "email" => NotificationChannel::Email,
            "telegram" => NotificationChannel::Telegram,
            "webhook" => NotificationChannel::Webhook,
            _ => NotificationChannel::Email,
        }
    }

    fn parse_status(s: &str) -> NotificationStatus {
        match s {
            "pending" => NotificationStatus::Pending,
            "sending" => NotificationStatus::Sending,
            "sent" => NotificationStatus::Sent,
            "failed" => NotificationStatus::Failed,
            "cancelled" => NotificationStatus::Cancelled,
            _ => NotificationStatus::Pending,
        }
    }

    fn status_to_str(status: NotificationStatus) -> &'static str {
        match status {
            NotificationStatus::Pending => "pending",
            NotificationStatus::Sending => "sending",
            NotificationStatus::Sent => "sent",
            NotificationStatus::Failed => "failed",
            NotificationStatus::Cancelled => "cancelled",
        }
    }

    fn channel_to_str(channel: NotificationChannel) -> &'static str {
        match channel {
            NotificationChannel::Email => "email",
            NotificationChannel::Telegram => "telegram",
            NotificationChannel::Webhook => "webhook",
        }
    }

    fn time_to_chrono(t: OffsetDateTime) -> DateTime<Utc> {
        DateTime::from_timestamp(t.unix_timestamp(), t.nanosecond())
            .unwrap_or_else(Utc::now)
    }

    fn time_to_chrono_opt(t: Option<OffsetDateTime>) -> Option<DateTime<Utc>> {
        t.map(Self::time_to_chrono)
    }

    fn chrono_to_time(t: DateTime<Utc>) -> OffsetDateTime {
        OffsetDateTime::from_unix_timestamp(t.timestamp())
            .unwrap_or_else(|_| OffsetDateTime::now_utc())
    }

    fn chrono_to_time_opt(t: Option<DateTime<Utc>>) -> Option<OffsetDateTime> {
        t.map(Self::chrono_to_time)
    }

    fn default_recipient() -> NotificationRecipient {
        NotificationRecipient {
            reference: None,
            email: None,
            telegram_chat_id: None,
            webhook_url: None,
        }
    }
}

#[async_trait]
impl NotificationQueueStorage for PostgresNotificationStorage {
    async fn enqueue(&self, notification: &Notification) -> Result<(), NotificationError> {
        self.ensure_tables().await?;

        let recipient_json = serde_json::to_value(&notification.recipient)
            .map_err(|e| NotificationError::Internal(e.to_string()))?;
        let template_data_json = serde_json::to_value(&notification.template_data)
            .map_err(|e| NotificationError::Internal(e.to_string()))?;

        sqlx_core::query::query(
            r#"
            INSERT INTO _notifications (
                id, channel, provider_id, status, recipient, template_id,
                template_data, scheduled_at, created_at, retry_count
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            "#,
        )
        .bind(&notification.id)
        .bind(Self::channel_to_str(notification.channel))
        .bind(&notification.provider_id)
        .bind(Self::status_to_str(notification.status))
        .bind(recipient_json)
        .bind(&notification.template_id)
        .bind(template_data_json)
        .bind(Self::time_to_chrono_opt(notification.scheduled_at))
        .bind(Self::time_to_chrono(notification.created_at))
        .bind(notification.retry_count as i32)
        .execute(&self.pool)
        .await
        .map_err(|e| NotificationError::Internal(e.to_string()))?;

        debug!(id = %notification.id, "Enqueued notification");
        Ok(())
    }

    async fn get(&self, id: &str) -> Result<Option<Notification>, NotificationError> {
        self.ensure_tables().await?;

        let row: Option<(
            String,
            String,
            String,
            String,
            serde_json::Value,
            String,
            serde_json::Value,
            Option<DateTime<Utc>>,
            DateTime<Utc>,
            Option<DateTime<Utc>>,
            Option<String>,
            i32,
        )> = sqlx_core::query_as::query_as(
            r#"
            SELECT id, channel, provider_id, status, recipient, template_id,
                   template_data, scheduled_at, created_at, sent_at, error, retry_count
            FROM _notifications WHERE id = $1
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| NotificationError::Internal(e.to_string()))?;

        Ok(row.map(
            |(
                id,
                channel,
                provider_id,
                status,
                recipient,
                template_id,
                template_data,
                scheduled_at,
                created_at,
                sent_at,
                error,
                retry_count,
            )| {
                Notification {
                    id,
                    channel: Self::parse_channel(&channel),
                    provider_id,
                    status: Self::parse_status(&status),
                    recipient: serde_json::from_value(recipient).unwrap_or_else(|_| Self::default_recipient()),
                    template_id,
                    template_data: serde_json::from_value(template_data).unwrap_or_default(),
                    scheduled_at: Self::chrono_to_time_opt(scheduled_at),
                    created_at: Self::chrono_to_time(created_at),
                    sent_at: Self::chrono_to_time_opt(sent_at),
                    error,
                    retry_count: retry_count as u32,
                }
            },
        ))
    }

    async fn update_status(
        &self,
        id: &str,
        status: NotificationStatus,
        error: Option<&str>,
    ) -> Result<(), NotificationError> {
        self.ensure_tables().await?;

        let sent_at: Option<DateTime<Utc>> = if status == NotificationStatus::Sent {
            Some(Utc::now())
        } else {
            None
        };

        sqlx_core::query::query(
            r#"
            UPDATE _notifications
            SET status = $2, error = $3, sent_at = COALESCE($4, sent_at)
            WHERE id = $1
            "#,
        )
        .bind(id)
        .bind(Self::status_to_str(status))
        .bind(error)
        .bind(sent_at)
        .execute(&self.pool)
        .await
        .map_err(|e| NotificationError::Internal(e.to_string()))?;

        Ok(())
    }

    async fn mark_sent(&self, id: &str) -> Result<(), NotificationError> {
        self.update_status(id, NotificationStatus::Sent, None)
            .await
    }

    async fn schedule_retry(
        &self,
        id: &str,
        next_retry: OffsetDateTime,
        error: &str,
    ) -> Result<(), NotificationError> {
        self.ensure_tables().await?;

        sqlx_core::query::query(
            r#"
            UPDATE _notifications
            SET status = 'pending',
                next_retry_at = $2,
                error = $3,
                retry_count = retry_count + 1
            WHERE id = $1
            "#,
        )
        .bind(id)
        .bind(Self::time_to_chrono(next_retry))
        .bind(error)
        .execute(&self.pool)
        .await
        .map_err(|e| NotificationError::Internal(e.to_string()))?;

        Ok(())
    }

    async fn fetch_pending(&self, limit: i32) -> Result<Vec<Notification>, NotificationError> {
        self.ensure_tables().await?;

        let rows: Vec<(
            String,
            String,
            String,
            String,
            serde_json::Value,
            String,
            serde_json::Value,
            Option<DateTime<Utc>>,
            DateTime<Utc>,
            Option<DateTime<Utc>>,
            Option<String>,
            i32,
        )> = sqlx_core::query_as::query_as(
            r#"
            SELECT id, channel, provider_id, status, recipient, template_id,
                   template_data, scheduled_at, created_at, sent_at, error, retry_count
            FROM _notifications
            WHERE status = 'pending'
              AND (scheduled_at IS NULL OR scheduled_at <= NOW())
              AND (next_retry_at IS NULL OR next_retry_at <= NOW())
            ORDER BY created_at ASC
            LIMIT $1
            FOR UPDATE SKIP LOCKED
            "#,
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| NotificationError::Internal(e.to_string()))?;

        Ok(rows
            .into_iter()
            .map(
                |(
                    id,
                    channel,
                    provider_id,
                    status,
                    recipient,
                    template_id,
                    template_data,
                    scheduled_at,
                    created_at,
                    sent_at,
                    error,
                    retry_count,
                )| {
                    Notification {
                        id,
                        channel: Self::parse_channel(&channel),
                        provider_id,
                        status: Self::parse_status(&status),
                        recipient: serde_json::from_value(recipient).unwrap_or_else(|_| Self::default_recipient()),
                        template_id,
                        template_data: serde_json::from_value(template_data).unwrap_or_default(),
                        scheduled_at: Self::chrono_to_time_opt(scheduled_at),
                        created_at: Self::chrono_to_time(created_at),
                        sent_at: Self::chrono_to_time_opt(sent_at),
                        error,
                        retry_count: retry_count as u32,
                    }
                },
            )
            .collect())
    }

    async fn cancel(&self, id: &str) -> Result<bool, NotificationError> {
        self.ensure_tables().await?;

        let result = sqlx_core::query::query(
            r#"
            UPDATE _notifications
            SET status = 'cancelled'
            WHERE id = $1 AND status = 'pending'
            "#,
        )
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(|e| NotificationError::Internal(e.to_string()))?;

        Ok(result.rows_affected() > 0)
    }

    async fn restart(&self, id: &str) -> Result<bool, NotificationError> {
        self.ensure_tables().await?;

        let result = sqlx_core::query::query(
            r#"
            UPDATE _notifications
            SET status = 'pending',
                error = NULL,
                retry_count = 0,
                next_retry_at = NULL
            WHERE id = $1 AND status = 'failed'
            "#,
        )
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(|e| NotificationError::Internal(e.to_string()))?;

        Ok(result.rows_affected() > 0)
    }

    async fn restart_all_failed(&self) -> Result<u32, NotificationError> {
        self.ensure_tables().await?;

        let result = sqlx_core::query::query(
            r#"
            UPDATE _notifications
            SET status = 'pending',
                error = NULL,
                retry_count = 0,
                next_retry_at = NULL
            WHERE status = 'failed'
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| NotificationError::Internal(e.to_string()))?;

        Ok(result.rows_affected() as u32)
    }

    async fn list_by_status(
        &self,
        status: NotificationStatus,
        limit: i32,
        offset: i32,
    ) -> Result<Vec<Notification>, NotificationError> {
        self.ensure_tables().await?;

        let rows: Vec<(
            String,
            String,
            String,
            String,
            serde_json::Value,
            String,
            serde_json::Value,
            Option<DateTime<Utc>>,
            DateTime<Utc>,
            Option<DateTime<Utc>>,
            Option<String>,
            i32,
        )> = sqlx_core::query_as::query_as(
            r#"
            SELECT id, channel, provider_id, status, recipient, template_id,
                   template_data, scheduled_at, created_at, sent_at, error, retry_count
            FROM _notifications
            WHERE status = $1
            ORDER BY created_at DESC
            LIMIT $2 OFFSET $3
            "#,
        )
        .bind(Self::status_to_str(status))
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| NotificationError::Internal(e.to_string()))?;

        Ok(rows
            .into_iter()
            .map(
                |(
                    id,
                    channel,
                    provider_id,
                    status,
                    recipient,
                    template_id,
                    template_data,
                    scheduled_at,
                    created_at,
                    sent_at,
                    error,
                    retry_count,
                )| {
                    Notification {
                        id,
                        channel: Self::parse_channel(&channel),
                        provider_id,
                        status: Self::parse_status(&status),
                        recipient: serde_json::from_value(recipient).unwrap_or_else(|_| Self::default_recipient()),
                        template_id,
                        template_data: serde_json::from_value(template_data).unwrap_or_default(),
                        scheduled_at: Self::chrono_to_time_opt(scheduled_at),
                        created_at: Self::chrono_to_time(created_at),
                        sent_at: Self::chrono_to_time_opt(sent_at),
                        error,
                        retry_count: retry_count as u32,
                    }
                },
            )
            .collect())
    }

    async fn get_stats(&self) -> Result<NotificationStats, NotificationError> {
        self.ensure_tables().await?;

        let row: (i64, i64, i64, i64, i64) = sqlx_core::query_as::query_as(
            r#"
            SELECT
                COUNT(*) FILTER (WHERE status = 'pending') as pending,
                COUNT(*) FILTER (WHERE status = 'sending') as sending,
                COUNT(*) FILTER (WHERE status = 'sent') as sent,
                COUNT(*) FILTER (WHERE status = 'failed') as failed,
                COUNT(*) FILTER (WHERE status = 'cancelled') as cancelled
            FROM _notifications
            "#,
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|e| NotificationError::Internal(e.to_string()))?;

        Ok(NotificationStats {
            pending: row.0 as u32,
            sending: row.1 as u32,
            sent: row.2 as u32,
            failed: row.3 as u32,
            cancelled: row.4 as u32,
        })
    }
}

#[async_trait]
impl NotificationProviderStorage for PostgresNotificationStorage {
    async fn get(&self, id: &str) -> Result<Option<NotificationProvider>, NotificationError> {
        // Query from FHIR resource table (created by schema manager)
        // Table might not exist if no NotificationProvider resources have been created
        let row: Option<(serde_json::Value,)> = sqlx_core::query_as::query_as(
            r#"
            SELECT resource
            FROM notificationprovider
            WHERE id = $1
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| {
            // Table doesn't exist - no providers configured
            if e.to_string().contains("does not exist") {
                return NotificationError::ProviderNotFound(format!(
                    "NotificationProvider table not found: {}",
                    id
                ));
            }
            NotificationError::Internal(e.to_string())
        })?;

        match row {
            Some((resource,)) => {
                let provider: NotificationProvider = serde_json::from_value(resource)
                    .map_err(|e| NotificationError::Internal(format!("Failed to parse NotificationProvider: {}", e)))?;
                Ok(Some(provider))
            }
            None => Ok(None),
        }
    }

    async fn list(&self) -> Result<Vec<NotificationProvider>, NotificationError> {
        // Query from FHIR resource table (created by schema manager)
        let rows: Vec<(serde_json::Value,)> = sqlx_core::query_as::query_as(
            r#"
            SELECT resource
            FROM notificationprovider
            ORDER BY resource->>'name'
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| {
            // Table doesn't exist - no providers configured
            if e.to_string().contains("does not exist") {
                return NotificationError::Internal("NotificationProvider table not found".to_string());
            }
            NotificationError::Internal(e.to_string())
        })
        .unwrap_or_default();

        let mut providers = Vec::new();
        for (resource,) in rows {
            match serde_json::from_value::<NotificationProvider>(resource) {
                Ok(provider) => providers.push(provider),
                Err(e) => {
                    tracing::warn!("Failed to parse NotificationProvider: {}", e);
                }
            }
        }
        Ok(providers)
    }
}
