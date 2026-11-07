pub mod adapters;
pub mod error;
pub mod provider;
pub mod queue;
#[cfg(any(feature = "smtp", feature = "http"))]
pub mod scheduler;
pub mod service;
pub mod subscription;
pub mod templates;
pub mod types;

#[cfg(any(feature = "smtp", feature = "http"))]
pub use adapters::EmailAdapter;
pub use adapters::{NotificationAdapter, RenderedContent, SendResult};
#[cfg(feature = "http")]
pub use adapters::{TelegramAdapter, WebhookAdapter};
pub use error::NotificationError;
pub use provider::{NotificationProvider, ProviderConfig, ProviderType, WebhookHeader};
pub use queue::{
    NotificationProviderStorage, NotificationQueueStorage, NotificationTemplateStorage,
};
#[cfg(any(feature = "smtp", feature = "http"))]
pub use scheduler::{DecryptFn, NotificationProcessor};
pub use service::NotificationService;
pub use subscription::parse_iso_duration;
pub use templates::{Template, TemplateRenderer, render_template};
pub use types::*;
