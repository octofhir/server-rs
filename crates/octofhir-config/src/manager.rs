//! Configuration Manager
//!
//! Central coordinator for configuration from multiple sources.
//! Handles merging, validation, and broadcasting of configuration changes.

use crate::ConfigError;
use crate::events::{
    ConfigCategory, ConfigChangeEvent, ConfigOperation, ConfigSource as EventSource,
};
use crate::feature_flags::{FeatureContext, FeatureFlags};
use crate::merger::MergedConfig;
use crate::secrets::Secrets;
use crate::sources::{ConfigSource, WatchHandle};
use crate::storage::ConfigStorage;

use sqlx_postgres::PgPool;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{RwLock, broadcast, mpsc};
use tracing::{debug, error, info, warn};

/// Configuration manager builder
pub struct ConfigurationManagerBuilder {
    file_path: Option<PathBuf>,
    db_pool: Option<PgPool>,
    secrets: Option<Secrets>,
}

impl ConfigurationManagerBuilder {
    /// Create a new builder
    pub fn new() -> Self {
        Self {
            file_path: None,
            db_pool: None,
            secrets: None,
        }
    }

    /// Set the configuration file path
    pub fn with_file(mut self, path: impl Into<PathBuf>) -> Self {
        self.file_path = Some(path.into());
        self
    }

    /// Set the database pool for database source
    pub fn with_database(mut self, pool: PgPool) -> Self {
        self.db_pool = Some(pool);
        self
    }

    /// Set the secrets manager for encryption
    pub fn with_secrets(mut self, secrets: Secrets) -> Self {
        self.secrets = Some(secrets);
        self
    }

    /// Build the configuration manager
    pub async fn build(self) -> Result<ConfigurationManager, ConfigError> {
        let mut sources: Vec<Box<dyn ConfigSource>> = Vec::new();

        // Add file source if path provided
        if let Some(path) = self.file_path {
            use crate::sources::{FileSource, FileWatcherConfig};
            sources.push(Box::new(FileSource::new(FileWatcherConfig::new(path))));
        }

        // Add database source if pool provided
        if let Some(pool) = self.db_pool.clone() {
            use crate::sources::DatabaseSource;
            sources.push(Box::new(DatabaseSource::new(pool)));
        }

        // Create storage if database available
        let storage = self.db_pool.map(|pool| {
            if let Some(secrets) = self.secrets.clone() {
                ConfigStorage::with_secrets(pool, secrets)
            } else {
                ConfigStorage::new(pool)
            }
        });

        // Create event bus
        let (event_tx, _) = broadcast::channel(100);

        // Load initial configuration
        let mut merged = MergedConfig::defaults();
        for source in &sources {
            match source.load().await {
                Ok(partial) => {
                    let event_source = match source.name() {
                        "file" => EventSource::File,
                        "database" => EventSource::Database,
                        _ => EventSource::Default,
                    };
                    merged.merge(partial, event_source);
                }
                Err(e) => {
                    warn!("Failed to load config from {}: {e}", source.name());
                }
            }
        }

        Ok(ConfigurationManager {
            sources,
            merged: Arc::new(RwLock::new(merged)),
            event_bus: event_tx,
            storage,
            secrets: self.secrets,
            watch_handles: Arc::new(RwLock::new(Vec::new())),
        })
    }
}

impl Default for ConfigurationManagerBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Central configuration manager
pub struct ConfigurationManager {
    /// Configuration sources
    sources: Vec<Box<dyn ConfigSource>>,
    /// Merged configuration
    merged: Arc<RwLock<MergedConfig>>,
    /// Event bus for broadcasting changes
    event_bus: broadcast::Sender<ConfigChangeEvent>,
    /// Database storage (if available)
    storage: Option<ConfigStorage>,
    /// Secrets manager (if available)
    secrets: Option<Secrets>,
    /// Watch handles
    watch_handles: Arc<RwLock<Vec<WatchHandle>>>,
}

impl ConfigurationManager {
    /// Create a new builder
    pub fn builder() -> ConfigurationManagerBuilder {
        ConfigurationManagerBuilder::new()
    }

    /// Get the current merged configuration
    pub async fn config(&self) -> MergedConfig {
        self.merged.read().await.clone()
    }

    /// Get a specific category configuration
    pub async fn get_category(&self, category: ConfigCategory) -> Option<serde_json::Value> {
        let config = self.merged.read().await;
        config.get_category(&category.to_string()).cloned()
    }

    /// Get the feature flags
    pub async fn feature_flags(&self) -> FeatureFlags {
        self.merged.read().await.feature_flags().clone()
    }

    /// Check if a feature is enabled
    pub async fn is_feature_enabled(&self, name: &str, context: &FeatureContext) -> bool {
        self.merged
            .read()
            .await
            .feature_flags()
            .is_enabled(name, context)
    }

    /// Subscribe to configuration changes
    pub fn subscribe(&self) -> broadcast::Receiver<ConfigChangeEvent> {
        self.event_bus.subscribe()
    }

    /// Start watching all sources for changes
    pub async fn start_watching(&self) -> Result<(), ConfigError> {
        let (tx, mut rx) = mpsc::channel::<ConfigChangeEvent>(100);

        // Start watchers for all sources
        let mut handles = self.watch_handles.write().await;
        for source in &self.sources {
            match source.watch(tx.clone()).await {
                Ok(handle) => {
                    info!("Started watching {} source", source.name());
                    handles.push(handle);
                }
                Err(e) => {
                    error!("Failed to start watching {} source: {e}", source.name());
                }
            }
        }

        // Spawn task to process events
        let _merged = Arc::clone(&self.merged);
        let sources = self
            .sources
            .iter()
            .map(|s| (s.name().to_string(), s.priority()))
            .collect::<Vec<_>>();
        let event_bus = self.event_bus.clone();

        tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                debug!("Received config change event: {:?}", event);

                // TODO: Reload configuration from all sources
                // In a full implementation, we would:
                // 1. Reload from the source that changed
                // 2. Re-merge all configurations
                // 3. Validate the new configuration
                // 4. Update the merged config
                // 5. Broadcast the change

                for (name, _priority) in &sources {
                    debug!("Source available for reload: {}", name);
                }

                if event_bus.send(event.clone()).is_err() {
                    warn!("No subscribers for config change event");
                }
            }
        });

        Ok(())
    }

    /// Stop all watchers
    pub async fn stop_watching(&self) {
        let mut handles = self.watch_handles.write().await;
        for handle in handles.drain(..) {
            handle.stop().await;
        }
    }

    /// Reload configuration from all sources
    pub async fn reload(&self) -> Result<(), ConfigError> {
        let mut new_merged = MergedConfig::defaults();

        for source in &self.sources {
            match source.load().await {
                Ok(partial) => {
                    let event_source = match source.name() {
                        "file" => EventSource::File,
                        "database" => EventSource::Database,
                        _ => EventSource::Default,
                    };
                    new_merged.merge(partial, event_source);
                }
                Err(e) => {
                    warn!("Failed to reload config from {}: {e}", source.name());
                }
            }
        }

        // Validate before applying
        new_merged.validate()?;

        // Update merged config
        {
            let mut merged = self.merged.write().await;
            *merged = new_merged;
        }

        // Broadcast reload event
        let event = ConfigChangeEvent::new(
            EventSource::File,
            ConfigCategory::Server,
            ConfigOperation::Reload,
        );
        let _ = self.event_bus.send(event);

        info!("Configuration reloaded");
        Ok(())
    }

    /// Set a configuration value via API
    pub async fn set_config(
        &self,
        category: ConfigCategory,
        key: &str,
        value: serde_json::Value,
        description: Option<&str>,
        is_secret: bool,
        updated_by: Option<&str>,
    ) -> Result<(), ConfigError> {
        // Store in database if available
        if let Some(storage) = &self.storage {
            storage
                .set(
                    &category.to_string(),
                    key,
                    value.clone(),
                    description,
                    is_secret,
                    updated_by,
                )
                .await?;
        }

        // Update merged config
        {
            let mut merged = self.merged.write().await;
            if let Some(category_value) = merged.config_mut().as_object_mut()
                && let Some(cat_obj) = category_value
                    .get_mut(&category.to_string())
                    .and_then(|v| v.as_object_mut())
            {
                cat_obj.insert(key.to_string(), value.clone());
            }
        }

        // Broadcast change event
        let event =
            ConfigChangeEvent::with_key(EventSource::Api, category, key, ConfigOperation::Set)
                .with_value(if is_secret {
                    serde_json::json!("<secret>")
                } else {
                    value
                });

        let _ = self.event_bus.send(event);

        info!("Configuration set: {}.{}", category, key);
        Ok(())
    }

    /// Delete a configuration value
    pub async fn delete_config(
        &self,
        category: ConfigCategory,
        key: &str,
    ) -> Result<bool, ConfigError> {
        // Delete from database if available
        let deleted = if let Some(storage) = &self.storage {
            storage.delete(&category.to_string(), key).await?
        } else {
            false
        };

        // Broadcast delete event
        let event =
            ConfigChangeEvent::with_key(EventSource::Api, category, key, ConfigOperation::Delete);
        let _ = self.event_bus.send(event);

        Ok(deleted)
    }

    /// Get stored configuration (from database)
    pub async fn get_stored_config(
        &self,
        category: ConfigCategory,
        key: &str,
    ) -> Result<Option<serde_json::Value>, ConfigError> {
        if let Some(storage) = &self.storage {
            storage.get_decrypted(&category.to_string(), key).await
        } else {
            // Fall back to merged config
            let config = self.merged.read().await;
            Ok(config
                .get_category(&category.to_string())
                .and_then(|v| v.get(key))
                .cloned())
        }
    }

    /// Toggle a feature flag
    pub async fn toggle_feature(&self, name: &str, enabled: bool) -> Result<(), ConfigError> {
        use crate::feature_flags::FeatureFlag;

        // Update in merged config
        {
            let mut merged = self.merged.write().await;
            let flags = merged.feature_flags_mut();
            if let Some(existing) = flags.get(name) {
                let mut updated = existing.clone();
                updated.enabled = enabled;
                flags.set(updated);
            } else {
                flags.set(FeatureFlag::boolean(name, enabled));
            }
        }

        // Store in database if available
        if let Some(storage) = &self.storage {
            storage
                .set(
                    "features",
                    name,
                    serde_json::json!({ "enabled": enabled }),
                    None,
                    false,
                    None,
                )
                .await?;
        }

        // Broadcast change
        let event = ConfigChangeEvent::with_key(
            EventSource::Api,
            ConfigCategory::Features,
            name,
            ConfigOperation::Update,
        );
        let _ = self.event_bus.send(event);

        info!("Feature flag {} set to {}", name, enabled);
        Ok(())
    }

    /// Get the secrets manager
    pub fn secrets(&self) -> Option<&Secrets> {
        self.secrets.as_ref()
    }
}

impl std::fmt::Debug for ConfigurationManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConfigurationManager")
            .field("sources", &self.sources.len())
            .field("has_storage", &self.storage.is_some())
            .field("has_secrets", &self.secrets.is_some())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_builder_file_only() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("test.toml");

        std::fs::write(
            &config_path,
            r#"
[server]
port = 9090
"#,
        )
        .unwrap();

        let manager = ConfigurationManager::builder()
            .with_file(&config_path)
            .build()
            .await
            .unwrap();

        let config = manager.config().await;
        let server = config.get_category("server").unwrap();
        assert_eq!(server.get("port").unwrap().as_u64(), Some(9090));
    }

    #[tokio::test]
    async fn test_feature_flags() {
        let manager = ConfigurationManager::builder().build().await.unwrap();

        // Default flags should be present
        let flags = manager.feature_flags().await;
        assert!(flags.get("search.optimization.enabled").is_some());
    }

    #[tokio::test]
    async fn test_is_feature_enabled() {
        let manager = ConfigurationManager::builder().build().await.unwrap();

        // search.optimization.enabled should be true by default
        let enabled = manager
            .is_feature_enabled("search.optimization.enabled", &FeatureContext::new())
            .await;
        assert!(enabled);

        // Unknown flag should be disabled
        let unknown = manager
            .is_feature_enabled("unknown.flag", &FeatureContext::new())
            .await;
        assert!(!unknown);
    }

    #[tokio::test]
    async fn test_subscribe() {
        let manager = ConfigurationManager::builder().build().await.unwrap();

        let mut rx = manager.subscribe();

        // Toggle a feature to generate an event
        manager.toggle_feature("test.feature", true).await.unwrap();

        // Should receive the event
        let event = rx.recv().await.unwrap();
        assert_eq!(event.category, ConfigCategory::Features);
    }
}
