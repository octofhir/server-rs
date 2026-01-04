use async_trait::async_trait;
use time::OffsetDateTime;

use crate::error::NotificationError;
use crate::types::{Notification, NotificationStats, NotificationStatus};

/// Storage trait for notification queue
#[async_trait]
pub trait NotificationQueueStorage: Send + Sync {
    /// Add a notification to the queue
    async fn enqueue(&self, notification: &Notification) -> Result<(), NotificationError>;

    /// Get a notification by ID
    async fn get(&self, id: &str) -> Result<Option<Notification>, NotificationError>;

    /// Update notification status
    async fn update_status(
        &self,
        id: &str,
        status: NotificationStatus,
        error: Option<&str>,
    ) -> Result<(), NotificationError>;

    /// Mark notification as sent
    async fn mark_sent(&self, id: &str) -> Result<(), NotificationError>;

    /// Schedule a retry with exponential backoff
    async fn schedule_retry(
        &self,
        id: &str,
        next_retry: OffsetDateTime,
        error: &str,
    ) -> Result<(), NotificationError>;

    /// Fetch pending notifications ready to be sent
    /// Returns notifications that are:
    /// - Pending with no scheduled_at (immediate)
    /// - Pending with scheduled_at <= now
    /// - Pending with next_retry_at <= now (retries)
    async fn fetch_pending(&self, limit: i32) -> Result<Vec<Notification>, NotificationError>;

    /// Cancel a pending notification
    async fn cancel(&self, id: &str) -> Result<bool, NotificationError>;

    /// Restart a failed notification (reset status to pending, clear error, reset retry count)
    async fn restart(&self, id: &str) -> Result<bool, NotificationError>;

    /// Restart all failed notifications
    async fn restart_all_failed(&self) -> Result<u32, NotificationError>;

    /// Get notifications by status for monitoring
    async fn list_by_status(
        &self,
        status: NotificationStatus,
        limit: i32,
        offset: i32,
    ) -> Result<Vec<Notification>, NotificationError>;

    /// Get notification statistics (counts by status)
    async fn get_stats(&self) -> Result<NotificationStats, NotificationError>;
}

/// Storage trait for notification providers
#[async_trait]
pub trait NotificationProviderStorage: Send + Sync {
    /// Get provider by ID
    async fn get(
        &self,
        id: &str,
    ) -> Result<Option<crate::provider::NotificationProvider>, NotificationError>;

    /// List all providers
    async fn list(&self) -> Result<Vec<crate::provider::NotificationProvider>, NotificationError>;
}
