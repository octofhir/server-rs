use async_trait::async_trait;

use crate::error::NotificationError;
use crate::types::{Notification, SendNotificationRequest};

#[async_trait]
pub trait NotificationService: Send + Sync {
    /// Queue a notification for sending
    async fn send(&self, request: SendNotificationRequest)
        -> Result<Notification, NotificationError>;

    /// Get notification by ID
    async fn get(&self, id: &str) -> Result<Option<Notification>, NotificationError>;

    /// Cancel a pending notification
    async fn cancel(&self, id: &str) -> Result<(), NotificationError>;

    /// Process pending notifications (called by scheduler)
    async fn process_pending(&self) -> Result<u32, NotificationError>;
}
