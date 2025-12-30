//! Hot Reload Configuration Watcher for Terminology Services
//!
//! This module provides file-watching capabilities to hot-reload terminology
//! configuration without requiring a server restart.
//!
//! ## Features
//!
//! - Watch configuration files for changes using `notify` crate
//! - Debounce rapid file changes to prevent unnecessary reloads
//! - Safely swap terminology providers using RwLock
//! - Clear caches on configuration reload
//! - Graceful error handling for invalid configurations
//!
//! ## Example
//!
//! ```ignore
//! use octofhir_search::config_watcher::{ConfigWatcher, WatcherConfig};
//! use octofhir_search::terminology::{HybridTerminologyProvider, TerminologyConfig};
//!
//! // Create the watcher
//! let watcher = ConfigWatcher::new(WatcherConfig::default())?;
//!
//! // Start watching a config file
//! watcher.watch_file("/etc/octofhir/terminology.toml", move |config| {
//!     // Update the terminology provider
//!     provider.update_config(config);
//! })?;
//!
//! // The watcher runs in the background until dropped
//! ```

use notify::{
    Config as NotifyConfig, Event, RecommendedWatcher, RecursiveMode, Result as NotifyResult,
    Watcher,
};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use thiserror::Error;
use tokio::sync::RwLock;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use crate::terminology::{HybridTerminologyProvider, TerminologyConfig};
use octofhir_canonical_manager::CanonicalManager;

/// Default debounce duration in milliseconds
const DEFAULT_DEBOUNCE_MS: u64 = 500;

/// Errors that can occur with configuration watching.
#[derive(Debug, Error)]
pub enum WatcherError {
    #[error("Failed to initialize file watcher: {0}")]
    InitFailed(String),

    #[error("Failed to watch path {path}: {reason}")]
    WatchFailed { path: PathBuf, reason: String },

    #[error("Failed to read config file: {0}")]
    ReadFailed(#[from] std::io::Error),

    #[error("Failed to parse config: {0}")]
    ParseFailed(String),

    #[error("Watcher channel closed")]
    ChannelClosed,

    #[error("Watcher is already running")]
    AlreadyRunning,
}

/// Configuration for the file watcher.
#[derive(Debug, Clone)]
pub struct WatcherConfig {
    /// Debounce duration to prevent rapid reloads
    pub debounce_duration: Duration,
    /// Whether to reload on file creation (not just modification)
    pub reload_on_create: bool,
    /// Maximum number of reload attempts before giving up
    pub max_retry_attempts: u32,
    /// Delay between retry attempts
    pub retry_delay: Duration,
}

impl Default for WatcherConfig {
    fn default() -> Self {
        Self {
            debounce_duration: Duration::from_millis(DEFAULT_DEBOUNCE_MS),
            reload_on_create: true,
            max_retry_attempts: 3,
            retry_delay: Duration::from_millis(100),
        }
    }
}

impl WatcherConfig {
    /// Set the debounce duration.
    pub fn with_debounce(mut self, duration: Duration) -> Self {
        self.debounce_duration = duration;
        self
    }

    /// Set whether to reload on file creation.
    pub fn with_reload_on_create(mut self, enabled: bool) -> Self {
        self.reload_on_create = enabled;
        self
    }
}

/// A handle to control the configuration watcher.
pub struct WatcherHandle {
    /// Flag to signal the watcher to stop
    stop_flag: Arc<AtomicBool>,
    /// Join handle for the watcher task
    task_handle: Option<tokio::task::JoinHandle<()>>,
}

impl WatcherHandle {
    /// Stop the watcher gracefully.
    pub async fn stop(mut self) {
        self.stop_flag.store(true, Ordering::SeqCst);
        if let Some(handle) = self.task_handle.take() {
            let _ = handle.await;
        }
    }

    /// Check if the watcher is still running.
    pub fn is_running(&self) -> bool {
        !self.stop_flag.load(Ordering::SeqCst)
    }
}

impl Drop for WatcherHandle {
    fn drop(&mut self) {
        self.stop_flag.store(true, Ordering::SeqCst);
    }
}

/// Callback type for configuration changes.
pub type ConfigCallback = Box<dyn Fn(TerminologyConfig) + Send + Sync + 'static>;

/// Configuration watcher for terminology settings.
///
/// Watches a configuration file and triggers callbacks when changes are detected.
pub struct ConfigWatcher {
    config: WatcherConfig,
    watcher: Option<RecommendedWatcher>,
    watched_paths: Vec<PathBuf>,
}

impl ConfigWatcher {
    /// Create a new configuration watcher.
    pub fn new(config: WatcherConfig) -> Result<Self, WatcherError> {
        Ok(Self {
            config,
            watcher: None,
            watched_paths: Vec::new(),
        })
    }

    /// Watch a TOML configuration file and update the provider on changes.
    ///
    /// The callback is invoked whenever the file changes with the new configuration.
    pub async fn watch_toml<P, F>(
        &mut self,
        path: P,
        callback: F,
    ) -> Result<WatcherHandle, WatcherError>
    where
        P: AsRef<Path>,
        F: Fn(TerminologyConfig) + Send + Sync + 'static,
    {
        let path = path.as_ref().to_path_buf();
        let config = self.config.clone();
        let stop_flag = Arc::new(AtomicBool::new(false));
        let stop_flag_clone = stop_flag.clone();

        // Create channel for file events
        let (tx, mut rx) = mpsc::channel::<PathBuf>(100);

        // Create the file watcher
        let tx_clone = tx.clone();
        let reload_on_create = config.reload_on_create;
        let mut watcher = RecommendedWatcher::new(
            move |res: NotifyResult<Event>| {
                if let Ok(event) = res {
                    let dominated =
                        event.kind.is_modify() || (reload_on_create && event.kind.is_create());
                    if dominated {
                        for path in event.paths {
                            let _ = tx_clone.blocking_send(path);
                        }
                    }
                }
            },
            NotifyConfig::default(),
        )
        .map_err(|e| WatcherError::InitFailed(e.to_string()))?;

        // Start watching the file
        watcher
            .watch(&path, RecursiveMode::NonRecursive)
            .map_err(|e| WatcherError::WatchFailed {
                path: path.clone(),
                reason: e.to_string(),
            })?;

        self.watcher = Some(watcher);
        self.watched_paths.push(path.clone());

        info!(path = %path.display(), "Started watching configuration file");

        // Spawn the event processing task
        let debounce_duration = config.debounce_duration;
        let max_retries = config.max_retry_attempts;
        let retry_delay = config.retry_delay;

        let task_handle = tokio::spawn(async move {
            let mut last_reload = std::time::Instant::now();

            while !stop_flag_clone.load(Ordering::SeqCst) {
                tokio::select! {
                    Some(changed_path) = rx.recv() => {
                        // Debounce: ignore events that are too close together
                        if last_reload.elapsed() < debounce_duration {
                            debug!("Debouncing config change");
                            continue;
                        }

                        info!(path = %changed_path.display(), "Configuration file changed, reloading");

                        // Try to load and parse the config with retries
                        let mut attempts = 0;
                        loop {
                            attempts += 1;
                            match load_toml_config(&changed_path).await {
                                Ok(new_config) => {
                                    info!("Successfully loaded new terminology configuration");
                                    callback(new_config);
                                    last_reload = std::time::Instant::now();
                                    break;
                                }
                                Err(e) => {
                                    if attempts >= max_retries {
                                        error!(
                                            error = %e,
                                            attempts = attempts,
                                            "Failed to reload configuration after max retries"
                                        );
                                        break;
                                    }
                                    warn!(
                                        error = %e,
                                        attempt = attempts,
                                        "Failed to reload configuration, retrying"
                                    );
                                    tokio::time::sleep(retry_delay).await;
                                }
                            }
                        }
                    }
                    _ = tokio::time::sleep(Duration::from_secs(1)) => {
                        // Periodic check if we should stop
                    }
                }
            }

            info!("Configuration watcher stopped");
        });

        Ok(WatcherHandle {
            stop_flag,
            task_handle: Some(task_handle),
        })
    }

    /// Get the list of watched paths.
    pub fn watched_paths(&self) -> &[PathBuf] {
        &self.watched_paths
    }
}

/// Load terminology configuration from a TOML file.
async fn load_toml_config(path: &Path) -> Result<TerminologyConfig, WatcherError> {
    let content = tokio::fs::read_to_string(path).await?;

    // Try to parse as a full config with [terminology] section
    if let Ok(full_config) = toml::from_str::<toml::Value>(&content) {
        if let Some(term_section) = full_config.get("terminology") {
            return term_section
                .clone()
                .try_into::<TerminologyConfig>()
                .map_err(|e| WatcherError::ParseFailed(e.to_string()));
        }
    }

    // Try to parse directly as TerminologyConfig
    toml::from_str(&content).map_err(|e| WatcherError::ParseFailed(e.to_string()))
}

/// A reloadable terminology provider wrapper.
///
/// Wraps a `HybridTerminologyProvider` with hot-reload support.
pub struct ReloadableTerminologyProvider {
    /// The inner provider protected by RwLock
    inner: Arc<RwLock<Arc<HybridTerminologyProvider>>>,
    /// Current configuration
    config: Arc<RwLock<TerminologyConfig>>,
    /// Canonical manager for local lookups
    canonical_manager: Arc<CanonicalManager>,
}

impl ReloadableTerminologyProvider {
    /// Create a new reloadable provider with the given configuration.
    pub fn new(
        canonical_manager: Arc<CanonicalManager>,
        config: TerminologyConfig,
    ) -> Result<Self, crate::terminology::TerminologyError> {
        let provider = HybridTerminologyProvider::new(canonical_manager.clone(), &config)?;
        Ok(Self {
            inner: Arc::new(RwLock::new(Arc::new(provider))),
            config: Arc::new(RwLock::new(config)),
            canonical_manager,
        })
    }

    /// Get a reference to the current provider.
    pub async fn provider(&self) -> Arc<HybridTerminologyProvider> {
        self.inner.read().await.clone()
    }

    /// Update the configuration and reload the provider.
    pub async fn reload(
        &self,
        new_config: TerminologyConfig,
    ) -> Result<(), crate::terminology::TerminologyError> {
        info!(
            server_url = %new_config.server_url,
            cache_ttl = new_config.cache_ttl_secs,
            "Reloading terminology provider with new configuration"
        );

        // Create new provider with new config
        let new_provider =
            HybridTerminologyProvider::new(self.canonical_manager.clone(), &new_config)?;

        // Update the inner provider
        {
            let mut inner = self.inner.write().await;
            *inner = Arc::new(new_provider);
        }

        // Update the stored config
        {
            let mut config = self.config.write().await;
            *config = new_config;
        }

        info!("Terminology provider reloaded successfully");
        Ok(())
    }

    /// Get the current configuration.
    pub async fn current_config(&self) -> TerminologyConfig {
        self.config.read().await.clone()
    }

    /// Clear all caches in the provider.
    pub async fn clear_caches(&self) {
        let provider = self.inner.read().await;
        provider.clear_cache();
        info!("Terminology provider caches cleared");
    }
}

impl Clone for ReloadableTerminologyProvider {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            config: self.config.clone(),
            canonical_manager: self.canonical_manager.clone(),
        }
    }
}

/// Start watching a configuration file and automatically reload the provider.
///
/// This is a convenience function that sets up both the watcher and the provider.
pub async fn watch_and_reload<P>(
    config_path: P,
    canonical_manager: Arc<CanonicalManager>,
    initial_config: TerminologyConfig,
    watcher_config: WatcherConfig,
) -> Result<(ReloadableTerminologyProvider, WatcherHandle), WatcherError>
where
    P: AsRef<Path>,
{
    let provider =
        ReloadableTerminologyProvider::new(canonical_manager, initial_config).map_err(|e| {
            WatcherError::InitFailed(format!("Failed to create terminology provider: {e}"))
        })?;
    let provider_clone = provider.clone();

    let mut watcher = ConfigWatcher::new(watcher_config)?;
    let handle = watcher
        .watch_toml(config_path, move |new_config| {
            let provider = provider_clone.clone();
            tokio::spawn(async move {
                if let Err(e) = provider.reload(new_config).await {
                    error!("Failed to reload terminology provider: {e}");
                }
            });
        })
        .await?;

    Ok((provider, handle))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_watcher_config_default() {
        let config = WatcherConfig::default();
        assert_eq!(
            config.debounce_duration,
            Duration::from_millis(DEFAULT_DEBOUNCE_MS)
        );
        assert!(config.reload_on_create);
        assert_eq!(config.max_retry_attempts, 3);
    }

    #[test]
    fn test_watcher_config_builder() {
        let config = WatcherConfig::default()
            .with_debounce(Duration::from_secs(1))
            .with_reload_on_create(false);

        assert_eq!(config.debounce_duration, Duration::from_secs(1));
        assert!(!config.reload_on_create);
    }

    #[tokio::test]
    async fn test_load_toml_config_direct() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(
            file,
            r#"
server_url = "https://test.example.com/r4"
cache_ttl_secs = 7200
"#
        )
        .unwrap();

        let config = load_toml_config(file.path()).await.unwrap();
        assert_eq!(config.server_url, "https://test.example.com/r4");
        assert_eq!(config.cache_ttl_secs, 7200);
    }

    #[tokio::test]
    async fn test_load_toml_config_with_section() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(
            file,
            r#"
[terminology]
server_url = "https://other.example.com/r4"
cache_ttl_secs = 1800
"#
        )
        .unwrap();

        let config = load_toml_config(file.path()).await.unwrap();
        assert_eq!(config.server_url, "https://other.example.com/r4");
        assert_eq!(config.cache_ttl_secs, 1800);
    }

    #[tokio::test]
    async fn test_load_toml_config_invalid() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "this is not valid toml {{{{").unwrap();

        let result = load_toml_config(file.path()).await;
        assert!(result.is_err());
    }

    #[test]
    fn test_watcher_error_display() {
        let err = WatcherError::WatchFailed {
            path: PathBuf::from("/test/path"),
            reason: "permission denied".to_string(),
        };
        assert!(err.to_string().contains("/test/path"));
        assert!(err.to_string().contains("permission denied"));
    }

    // Note: ReloadableTerminologyProvider tests require a CanonicalManager setup.
    // These are tested via integration tests in octofhir-server/tests/terminology_integration.rs

    #[test]
    fn test_watcher_handle_is_running() {
        let stop_flag = Arc::new(AtomicBool::new(false));
        let handle = WatcherHandle {
            stop_flag: stop_flag.clone(),
            task_handle: None,
        };

        assert!(handle.is_running());

        stop_flag.store(true, Ordering::SeqCst);
        assert!(!handle.is_running());
    }
}
