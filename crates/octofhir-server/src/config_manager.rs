//! Unified Configuration Management for OctoFHIR Server.
//!
//! Provides multi-source configuration with hot-reload capabilities:
//! - File watching with debounce
//! - Database storage with PostgreSQL NOTIFY
//! - Feature flags with context-aware evaluation
//! - Secret encryption at rest (AES-256-GCM)
//!
//! # Usage
//!
//! ```ignore
//! use octofhir_server::config_manager::ServerConfigManager;
//!
//! // Initialize with file and database sources
//! let manager = ServerConfigManager::builder()
//!     .with_file("octofhir.toml")
//!     .with_database(pool)
//!     .build()
//!     .await?;
//!
//! // Start watching for changes
//! manager.start_watching(shared_config).await;
//!
//! // Check feature flags
//! if manager.is_feature_enabled("experimental.new_search", &context).await {
//!     // Use new search implementation
//! }
//! ```

use std::path::PathBuf;
use std::sync::Arc;

use octofhir_config::{
    ConfigCategory, ConfigChangeEvent, ConfigurationManager, ConfigurationManagerBuilder,
    FeatureContext, FeatureFlags,
};
use sqlx_postgres::PgPool;
use tokio::sync::{RwLock, broadcast};
use tracing::{debug, error, info, warn};

use crate::config::AppConfig;

/// Server configuration manager.
///
/// Wraps `ConfigurationManager` and provides server-specific integration
/// for hot-reloading configuration, applying changes, and managing feature flags.
#[derive(Clone)]
pub struct ServerConfigManager {
    /// Inner configuration manager
    inner: Arc<ConfigurationManager>,
    /// Path to the config file (for reloading AppConfig)
    config_path: Option<PathBuf>,
}

/// Builder for ServerConfigManager.
pub struct ServerConfigManagerBuilder {
    file_path: Option<PathBuf>,
    db_pool: Option<PgPool>,
}

impl ServerConfigManagerBuilder {
    /// Create a new builder.
    pub fn new() -> Self {
        Self {
            file_path: None,
            db_pool: None,
        }
    }

    /// Add a file source.
    pub fn with_file(mut self, path: impl Into<PathBuf>) -> Self {
        self.file_path = Some(path.into());
        self
    }

    /// Add a database source.
    pub fn with_database(mut self, pool: PgPool) -> Self {
        self.db_pool = Some(pool);
        self
    }

    /// Build the configuration manager.
    pub async fn build(self) -> Result<ServerConfigManager, octofhir_config::ConfigError> {
        let mut builder = ConfigurationManagerBuilder::new();

        if let Some(ref path) = self.file_path {
            builder = builder.with_file(path.clone());
            info!(path = ?path, "Config source: file");
        }

        if let Some(ref pool) = self.db_pool {
            builder = builder.with_database(pool.clone());
            info!("Config source: database");
        }

        let manager = builder.build().await?;

        Ok(ServerConfigManager {
            inner: Arc::new(manager),
            config_path: self.file_path,
        })
    }
}

impl Default for ServerConfigManagerBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl ServerConfigManager {
    /// Create a new builder.
    pub fn builder() -> ServerConfigManagerBuilder {
        ServerConfigManagerBuilder::new()
    }

    /// Get the underlying configuration manager.
    pub fn inner(&self) -> &ConfigurationManager {
        &self.inner
    }

    /// Get the underlying configuration manager as an Arc.
    pub fn inner_arc(&self) -> Arc<ConfigurationManager> {
        self.inner.clone()
    }

    /// Subscribe to configuration change events.
    pub fn subscribe(&self) -> broadcast::Receiver<ConfigChangeEvent> {
        self.inner.subscribe()
    }

    /// Start watching for configuration changes.
    ///
    /// Spawns a background task that:
    /// - Listens for config change events
    /// - Applies hot-reloadable settings (logging, OTEL)
    /// - Updates the shared AppConfig
    /// - Rebuilds canonical registry when packages change
    pub async fn start_watching(&self, shared_config: Arc<RwLock<AppConfig>>) {
        let mut rx = self.subscribe();
        let config_path = self.config_path.clone();
        let manager = self.inner.clone();

        tokio::spawn(async move {
            info!("Configuration watcher started");

            while let Ok(event) = rx.recv().await {
                debug!(
                    source = ?event.source,
                    category = ?event.category,
                    key = ?event.key,
                    "Config change event received"
                );

                Self::handle_config_change(&event, &config_path, &shared_config, &manager).await;
            }

            warn!("Configuration watcher stopped");
        });
    }

    /// Handle a configuration change event.
    async fn handle_config_change(
        event: &ConfigChangeEvent,
        config_path: &Option<PathBuf>,
        shared_config: &Arc<RwLock<AppConfig>>,
        _manager: &ConfigurationManager,
    ) {
        match event.category {
            ConfigCategory::Logging => {
                info!("Applying logging configuration changes");
                if let Some(new_cfg) = Self::reload_app_config(config_path).await {
                    crate::observability::apply_logging_level(&new_cfg.logging.level);
                }
            }

            ConfigCategory::Otel => {
                info!("Applying OTEL configuration changes");
                if let Some(new_cfg) = Self::reload_app_config(config_path).await {
                    crate::observability::apply_otel_config(&new_cfg.otel);
                }
            }

            ConfigCategory::Packages => {
                info!("Rebuilding canonical registry");
                if let Some(new_cfg) = Self::reload_app_config(config_path).await {
                    let cfg_clone = new_cfg.clone();
                    tokio::spawn(async move {
                        if let Err(e) =
                            crate::canonical::rebuild_from_config_async(&cfg_clone).await
                        {
                            error!(error = %e, "Failed to rebuild canonical registry");
                        } else {
                            info!("Canonical registry rebuilt successfully");
                        }
                    });
                }
            }

            ConfigCategory::Search => {
                info!("Search configuration changed");
                // Handled by ReloadableSearchConfig subscribers
            }

            ConfigCategory::Terminology => {
                info!("Terminology configuration changed");
                // Handled by ReloadableTerminologyProvider subscribers
            }

            ConfigCategory::Features => {
                info!("Feature flags changed");
                // Feature flag evaluation uses ConfigurationManager directly
            }

            _ => {
                // Full config reload for other categories
                if let Some(new_cfg) = Self::reload_app_config(config_path).await {
                    // Apply all hot-reloadable settings
                    crate::observability::apply_logging_level(&new_cfg.logging.level);
                    crate::observability::apply_otel_config(&new_cfg.otel);

                    // Update shared config
                    {
                        let mut guard = shared_config.write().await;
                        *guard = new_cfg;
                    }

                    info!("Configuration reloaded");
                }
            }
        }
    }

    /// Reload AppConfig from file.
    async fn reload_app_config(config_path: &Option<PathBuf>) -> Option<AppConfig> {
        let path = config_path.as_ref()?;
        match crate::config::loader::load_config(path.to_str()) {
            Ok(cfg) => Some(cfg),
            Err(e) => {
                error!(error = %e, "Failed to reload configuration");
                None
            }
        }
    }

    /// Check if a feature flag is enabled.
    pub async fn is_feature_enabled(&self, name: &str, context: &FeatureContext) -> bool {
        self.inner.is_feature_enabled(name, context).await
    }

    /// Get all feature flags.
    pub async fn feature_flags(&self) -> FeatureFlags {
        self.inner.feature_flags().await
    }

    /// Reload configuration from all sources.
    pub async fn reload(&self) -> Result<(), octofhir_config::ConfigError> {
        self.inner.reload().await
    }

    /// Get the config file path.
    pub fn config_path(&self) -> Option<&PathBuf> {
        self.config_path.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_builder_without_sources() {
        let manager = ServerConfigManager::builder().build().await;
        assert!(manager.is_ok());
    }

    #[tokio::test]
    async fn test_subscribe() {
        let manager = ServerConfigManager::builder().build().await.unwrap();
        let _rx = manager.subscribe();
    }

    #[tokio::test]
    async fn test_feature_flags_default() {
        let manager = ServerConfigManager::builder().build().await.unwrap();
        let context = FeatureContext::new();

        // Unknown feature should be disabled by default
        let enabled = manager
            .is_feature_enabled("unknown.feature", &context)
            .await;
        assert!(!enabled);
    }
}
