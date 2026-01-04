pub mod adapters;
pub mod error;
pub mod provider;
pub mod queue;
pub mod scheduler;
pub mod service;
pub mod subscription;
pub mod templates;
pub mod types;

pub use adapters::{
    EmailAdapter, NotificationAdapter, RenderedContent, SendResult, TelegramAdapter,
    WebhookAdapter,
};
pub use error::NotificationError;
pub use provider::{NotificationProvider, ProviderConfig, ProviderType};
pub use queue::{NotificationProviderStorage, NotificationQueueStorage};
pub use scheduler::{DecryptFn, NotificationProcessor};
pub use service::NotificationService;
pub use subscription::parse_iso_duration;
pub use templates::{Template, TemplateRenderer};
pub use types::*;
