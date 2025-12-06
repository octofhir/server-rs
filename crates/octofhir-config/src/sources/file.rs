//! File-based configuration source
//!
//! Watches configuration files for changes and reloads when modified.

use crate::ConfigError;
use crate::events::{
    ConfigCategory, ConfigChangeEvent, ConfigOperation, ConfigSource as EventSource,
};
use crate::merger::PartialConfig;
use crate::sources::{ConfigSource, WatchHandle};

use async_trait::async_trait;
use notify::RecursiveMode;
use notify_debouncer_mini::{DebouncedEventKind, new_debouncer};
use std::path::PathBuf;
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

/// Configuration for file watcher
#[derive(Debug, Clone)]
pub struct FileWatcherConfig {
    /// Path to the configuration file
    pub path: PathBuf,
    /// Debounce duration for rapid changes
    pub debounce: Duration,
    /// Whether to reload on file creation
    pub reload_on_create: bool,
}

impl Default for FileWatcherConfig {
    fn default() -> Self {
        Self {
            path: PathBuf::from("octofhir.toml"),
            debounce: Duration::from_millis(500),
            reload_on_create: true,
        }
    }
}

impl FileWatcherConfig {
    /// Create config for a specific path
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            ..Default::default()
        }
    }

    /// Set debounce duration
    pub fn with_debounce(mut self, debounce: Duration) -> Self {
        self.debounce = debounce;
        self
    }
}

/// File-based configuration source
pub struct FileSource {
    config: FileWatcherConfig,
}

impl FileSource {
    /// Create a new file source
    pub fn new(config: FileWatcherConfig) -> Self {
        Self { config }
    }

    /// Create with default config for given path
    pub fn from_path(path: impl Into<PathBuf>) -> Self {
        Self::new(FileWatcherConfig::new(path))
    }

    /// Read and parse the configuration file
    fn read_config(&self) -> Result<PartialConfig, ConfigError> {
        let path = &self.config.path;

        if !path.exists() {
            debug!("Config file does not exist: {:?}", path);
            return Ok(PartialConfig::new());
        }

        let content = std::fs::read_to_string(path).map_err(|e| ConfigError::Io(e))?;

        PartialConfig::from_toml(&content)
    }
}

#[async_trait]
impl ConfigSource for FileSource {
    fn name(&self) -> &str {
        "file"
    }

    fn priority(&self) -> i32 {
        10 // File has low priority, overridden by DB and API
    }

    async fn load(&self) -> Result<PartialConfig, ConfigError> {
        self.read_config()
    }

    async fn watch(&self, tx: mpsc::Sender<ConfigChangeEvent>) -> Result<WatchHandle, ConfigError> {
        let path = self.config.path.clone();
        let debounce = self.config.debounce;
        let reload_on_create = self.config.reload_on_create;

        // Determine watch path (parent directory for file, or directory itself)
        let watch_path = if path.is_file() {
            path.parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| PathBuf::from("."))
        } else {
            path.clone()
        };

        let (shutdown_tx, mut shutdown_rx) = tokio::sync::oneshot::channel();

        let handle = tokio::spawn(async move {
            // Create channel for debounced events
            let (notify_tx, notify_rx) = std::sync::mpsc::channel();

            // Create debounced watcher
            let mut debouncer = match new_debouncer(debounce, notify_tx) {
                Ok(d) => d,
                Err(e) => {
                    error!("Failed to create file watcher: {e}");
                    return;
                }
            };

            // Start watching
            if let Err(e) = debouncer
                .watcher()
                .watch(&watch_path, RecursiveMode::NonRecursive)
            {
                error!("Failed to watch path {:?}: {e}", watch_path);
                return;
            }

            info!("Started watching config file: {:?}", path);

            loop {
                tokio::select! {
                    _ = &mut shutdown_rx => {
                        info!("File watcher shutting down");
                        break;
                    }
                    _ = tokio::time::sleep(Duration::from_millis(100)) => {
                        // Check for file events
                        while let Ok(events) = notify_rx.try_recv() {
                            match events {
                                Ok(events) => {
                                    for event in events {
                                        // Check if this event is for our config file
                                        let is_config_file = event.path.file_name() == path.file_name();

                                        if !is_config_file {
                                            continue;
                                        }

                                        let is_create = matches!(event.kind, DebouncedEventKind::Any);
                                        let should_reload = !is_create || reload_on_create;

                                        if should_reload {
                                            info!("Config file changed: {:?}", event.path);

                                            // Send reload event
                                            let change_event = ConfigChangeEvent::new(
                                                EventSource::File,
                                                ConfigCategory::Server, // Represents full reload
                                                ConfigOperation::Reload,
                                            );

                                            if tx.send(change_event).await.is_err() {
                                                warn!("Config change receiver dropped");
                                                return;
                                            }
                                        }
                                    }
                                }
                                Err(e) => {
                                    error!("File watcher error: {:?}", e);
                                }
                            }
                        }
                    }
                }
            }
        });

        Ok(WatchHandle::new(handle, shutdown_tx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use tokio::time::timeout;

    #[tokio::test]
    async fn test_load_config() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("test.toml");

        std::fs::write(
            &config_path,
            r#"
[server]
port = 9090

[search]
default_count = 25
"#,
        )
        .unwrap();

        let source = FileSource::from_path(&config_path);
        let config = source.load().await.unwrap();

        assert!(config.server.is_some());
        assert!(config.search.is_some());
    }

    #[tokio::test]
    async fn test_load_nonexistent_config() {
        let source = FileSource::from_path("/nonexistent/path.toml");
        let config = source.load().await.unwrap();

        // Should return empty config, not error
        assert!(config.server.is_none());
    }

    #[tokio::test]
    async fn test_file_watcher() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("watch-test.toml");

        // Create initial file
        std::fs::write(
            &config_path,
            r#"
[server]
port = 8080
"#,
        )
        .unwrap();

        let source = FileSource::new(FileWatcherConfig {
            path: config_path.clone(),
            debounce: Duration::from_millis(100),
            reload_on_create: true,
        });

        let (tx, mut rx) = mpsc::channel(10);
        let handle = source.watch(tx).await.unwrap();

        // Give watcher time to start
        tokio::time::sleep(Duration::from_millis(200)).await;

        // Modify the file
        std::fs::write(
            &config_path,
            r#"
[server]
port = 9090
"#,
        )
        .unwrap();

        // Wait for event with timeout
        let result = timeout(Duration::from_secs(2), rx.recv()).await;

        match result {
            Ok(Some(event)) => {
                assert_eq!(event.source, EventSource::File);
                assert_eq!(event.operation, ConfigOperation::Reload);
            }
            Ok(None) => panic!("Channel closed unexpectedly"),
            Err(_) => panic!("Timeout waiting for file change event"),
        }

        handle.stop().await;
    }
}
