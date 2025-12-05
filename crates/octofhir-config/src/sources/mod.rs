//! Configuration sources
//!
//! This module provides different sources for configuration:
//! - File: Watch configuration files for changes
//! - Database: Listen for PostgreSQL NOTIFY events
//! - API: Runtime configuration updates via HTTP

mod file;
mod database;

pub use file::{FileSource, FileWatcherConfig};
pub use database::{DatabaseSource, DatabaseSourceConfig};

use crate::events::ConfigChangeEvent;
use crate::merger::PartialConfig;
use crate::ConfigError;

use async_trait::async_trait;
use tokio::sync::mpsc;

/// Trait for configuration sources
#[async_trait]
pub trait ConfigSource: Send + Sync {
    /// Name of this source (for logging and debugging)
    fn name(&self) -> &str;

    /// Priority of this source (higher wins in merges)
    fn priority(&self) -> i32;

    /// Load current configuration from this source
    async fn load(&self) -> Result<PartialConfig, ConfigError>;

    /// Start watching for changes
    ///
    /// The source should send events through the provided channel when
    /// configuration changes are detected.
    async fn watch(&self, tx: mpsc::Sender<ConfigChangeEvent>) -> Result<WatchHandle, ConfigError>;
}

/// Handle for a running watcher
pub struct WatchHandle {
    /// Task handle for the watcher
    handle: tokio::task::JoinHandle<()>,
    /// Shutdown signal
    shutdown: tokio::sync::oneshot::Sender<()>,
}

impl WatchHandle {
    /// Create a new watch handle
    pub fn new(
        handle: tokio::task::JoinHandle<()>,
        shutdown: tokio::sync::oneshot::Sender<()>,
    ) -> Self {
        Self { handle, shutdown }
    }

    /// Stop the watcher
    pub async fn stop(self) {
        let _ = self.shutdown.send(());
        let _ = self.handle.await;
    }
}
