use std::sync::Arc;
use std::time::Duration;

use time::OffsetDateTime;
use tokio::time::interval;
use tracing::{error, info, warn};

use crate::adapters::{EmailAdapter, NotificationAdapter, TelegramAdapter, WebhookAdapter};
use crate::error::NotificationError;
use crate::provider::{NotificationProvider, ProviderConfig};
use crate::queue::{NotificationProviderStorage, NotificationQueueStorage};
use crate::templates::TemplateRenderer;
use crate::types::{Notification, NotificationChannel, NotificationStatus};

/// Decryption function type
pub type DecryptFn =
    Arc<dyn Fn(&str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> + Send + Sync>;

/// Notification processor that handles sending queued notifications
pub struct NotificationProcessor<Q, P>
where
    Q: NotificationQueueStorage,
    P: NotificationProviderStorage,
{
    queue: Arc<Q>,
    providers: Arc<P>,
    templates: Arc<TemplateRenderer>,
    email_adapter: EmailAdapter,
    telegram_adapter: TelegramAdapter,
    webhook_adapter: WebhookAdapter,
    decrypt_fn: DecryptFn,
}

impl<Q, P> NotificationProcessor<Q, P>
where
    Q: NotificationQueueStorage + 'static,
    P: NotificationProviderStorage + 'static,
{
    pub fn new(
        queue: Arc<Q>,
        providers: Arc<P>,
        templates: Arc<TemplateRenderer>,
        decrypt_fn: DecryptFn,
    ) -> Self {
        Self {
            queue,
            providers,
            templates,
            email_adapter: EmailAdapter::new(),
            telegram_adapter: TelegramAdapter::new(),
            webhook_adapter: WebhookAdapter::new(),
            decrypt_fn,
        }
    }

    /// Start processing loop
    pub async fn run(&self, poll_interval: Duration) {
        let mut ticker = interval(poll_interval);

        info!("Notification processor started");

        loop {
            ticker.tick().await;

            match self.process_batch(10).await {
                Ok(processed) => {
                    if processed > 0 {
                        info!(count = processed, "Processed notifications");
                    }
                }
                Err(e) => {
                    error!(error = %e, "Error processing notifications");
                }
            }
        }
    }

    /// Process a batch of pending notifications
    pub async fn process_batch(&self, limit: i32) -> Result<u32, NotificationError> {
        let pending = self.queue.fetch_pending(limit).await?;
        let mut processed = 0;

        for notification in pending {
            // Mark as sending before attempting
            self.queue
                .update_status(&notification.id, NotificationStatus::Sending, None)
                .await?;

            match self.process_one(&notification).await {
                Ok(()) => {
                    self.queue.mark_sent(&notification.id).await?;
                    processed += 1;
                }
                Err(e) => {
                    let error_msg = e.to_string();
                    warn!(
                        notification_id = %notification.id,
                        error = %error_msg,
                        retry_count = notification.retry_count,
                        "Notification send failed"
                    );

                    // Check if we should retry (max 5 retries)
                    const MAX_RETRIES: u32 = 5;
                    if notification.retry_count >= MAX_RETRIES {
                        // Mark as permanently failed
                        self.queue
                            .update_status(
                                &notification.id,
                                NotificationStatus::Failed,
                                Some(&error_msg),
                            )
                            .await?;
                        warn!(
                            notification_id = %notification.id,
                            "Notification permanently failed after {} retries",
                            MAX_RETRIES
                        );
                    } else {
                        // Schedule retry with exponential backoff
                        // 60s, 120s, 240s, 480s, 960s
                        let backoff_secs = 60 * (2_i64.pow(notification.retry_count));
                        let next_retry =
                            OffsetDateTime::now_utc() + time::Duration::seconds(backoff_secs);
                        self.queue
                            .schedule_retry(&notification.id, next_retry, &error_msg)
                            .await?;
                    }
                }
            }
        }

        Ok(processed)
    }

    async fn process_one(&self, notification: &Notification) -> Result<(), NotificationError> {
        // 1. Get provider
        let provider = self
            .providers
            .get(&notification.provider_id)
            .await?
            .ok_or_else(|| {
                NotificationError::ProviderNotFound(notification.provider_id.clone())
            })?;

        if !provider.active {
            return Err(NotificationError::ProviderDisabled(
                notification.provider_id.clone(),
            ));
        }

        // 2. Decrypt credentials
        let decrypted_config = self.decrypt_config(&provider)?;

        // 3. Render template
        let content = self
            .templates
            .render(&notification.template_id, &notification.template_data)?;

        // 4. Send via appropriate adapter
        let result = match notification.channel {
            NotificationChannel::Email => {
                self.email_adapter
                    .send(&decrypted_config, notification, &content)
                    .await?
            }
            NotificationChannel::Telegram => {
                self.telegram_adapter
                    .send(&decrypted_config, notification, &content)
                    .await?
            }
            NotificationChannel::Webhook => {
                self.webhook_adapter
                    .send(&decrypted_config, notification, &content)
                    .await?
            }
        };

        if result.success {
            info!(
                notification_id = %notification.id,
                channel = ?notification.channel,
                external_id = ?result.external_id,
                "Notification sent successfully"
            );
            Ok(())
        } else {
            Err(NotificationError::SendFailed(
                result.error.unwrap_or_else(|| "Unknown error".to_string()),
            ))
        }
    }

    fn decrypt_config(&self, provider: &NotificationProvider) -> Result<ProviderConfig, NotificationError> {
        let mut config = provider.config.clone();

        if let Some(ref encrypted) = config.api_key
            && encrypted.starts_with("$encrypted$")
        {
            config.api_key = Some(
                (self.decrypt_fn)(encrypted)
                    .map_err(|e| NotificationError::Internal(e.to_string()))?,
            );
        }
        if let Some(ref encrypted) = config.smtp_password
            && encrypted.starts_with("$encrypted$")
        {
            config.smtp_password = Some(
                (self.decrypt_fn)(encrypted)
                    .map_err(|e| NotificationError::Internal(e.to_string()))?,
            );
        }
        if let Some(ref encrypted) = config.bot_token
            && encrypted.starts_with("$encrypted$")
        {
            config.bot_token = Some(
                (self.decrypt_fn)(encrypted)
                    .map_err(|e| NotificationError::Internal(e.to_string()))?,
            );
        }
        if let Some(ref encrypted) = config.webhook_secret
            && encrypted.starts_with("$encrypted$")
        {
            config.webhook_secret = Some(
                (self.decrypt_fn)(encrypted)
                    .map_err(|e| NotificationError::Internal(e.to_string()))?,
            );
        }

        Ok(config)
    }
}
