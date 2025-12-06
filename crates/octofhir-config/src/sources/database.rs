//! Database-based configuration source
//!
//! Stores configuration in PostgreSQL and listens for NOTIFY events
//! when configuration changes.

use crate::ConfigError;
use crate::events::{
    ConfigCategory, ConfigChangeEvent, ConfigOperation, ConfigSource as EventSource,
};
use crate::merger::PartialConfig;
use crate::sources::{ConfigSource, WatchHandle};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sqlx_core::query::query;
use sqlx_core::query_as::query_as;
use sqlx_postgres::{PgListener, PgPool};
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

/// PostgreSQL channel for configuration changes
const CONFIG_CHANNEL: &str = "octofhir_config_changes";

/// Configuration for database source
#[derive(Debug, Clone)]
pub struct DatabaseSourceConfig {
    /// Reconnection delay on error
    pub reconnect_delay: Duration,
    /// Maximum reconnection attempts
    pub max_reconnect_attempts: u32,
}

impl Default for DatabaseSourceConfig {
    fn default() -> Self {
        Self {
            reconnect_delay: Duration::from_secs(5),
            max_reconnect_attempts: 10,
        }
    }
}

/// A configuration entry from the database
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigEntry {
    pub key: String,
    pub category: String,
    pub value: serde_json::Value,
    pub description: Option<String>,
    pub is_secret: bool,
}

/// Notification payload from PostgreSQL
#[derive(Debug, Clone, Deserialize)]
struct NotifyPayload {
    key: String,
    category: String,
    operation: String,
}

/// Database-based configuration source
pub struct DatabaseSource {
    pool: PgPool,
    config: DatabaseSourceConfig,
}

impl DatabaseSource {
    /// Create a new database source
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            config: DatabaseSourceConfig::default(),
        }
    }

    /// Create with custom config
    pub fn with_config(pool: PgPool, config: DatabaseSourceConfig) -> Self {
        Self { pool, config }
    }

    /// Load all configuration entries from database
    async fn load_entries(&self) -> Result<Vec<ConfigEntry>, ConfigError> {
        let entries: Vec<(String, String, serde_json::Value, Option<String>, bool)> = query_as(
            r#"
            SELECT key, category, value, description, is_secret
            FROM octofhir.configuration
            WHERE status = 'created' OR status = 'updated'
            ORDER BY category, key
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| ConfigError::database(format!("Failed to load config entries: {e}")))?;

        Ok(entries
            .into_iter()
            .map(
                |(key, category, value, description, is_secret)| ConfigEntry {
                    key,
                    category,
                    value,
                    description,
                    is_secret,
                },
            )
            .collect())
    }

    /// Convert entries to PartialConfig
    fn entries_to_partial(&self, entries: Vec<ConfigEntry>) -> PartialConfig {
        let mut partial = PartialConfig::new();

        // Group entries by category
        let mut by_category: std::collections::HashMap<
            String,
            serde_json::Map<String, serde_json::Value>,
        > = std::collections::HashMap::new();

        for entry in entries {
            by_category
                .entry(entry.category.clone())
                .or_default()
                .insert(entry.key, entry.value);
        }

        // Convert to partial config
        for (category, values) in by_category {
            let value = serde_json::Value::Object(values);
            partial.set_category(&category, value);
        }

        partial
    }

    /// Get a single configuration value
    pub async fn get(&self, category: &str, key: &str) -> Result<Option<ConfigEntry>, ConfigError> {
        let result: Option<(String, String, serde_json::Value, Option<String>, bool)> = query_as(
            r#"
            SELECT key, category, value, description, is_secret
            FROM octofhir.configuration
            WHERE category = $1 AND key = $2
            "#,
        )
        .bind(category)
        .bind(key)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| ConfigError::database(format!("Failed to get config entry: {e}")))?;

        Ok(result.map(
            |(key, category, value, description, is_secret)| ConfigEntry {
                key,
                category,
                value,
                description,
                is_secret,
            },
        ))
    }

    /// Set a configuration value
    pub async fn set(
        &self,
        category: &str,
        key: &str,
        value: serde_json::Value,
        description: Option<&str>,
        is_secret: bool,
    ) -> Result<(), ConfigError> {
        // First create a transaction entry
        let txid: (i64,) =
            query_as("INSERT INTO _transaction (status) VALUES ('committed') RETURNING txid")
                .fetch_one(&self.pool)
                .await
                .map_err(|e| ConfigError::database(format!("Failed to create transaction: {e}")))?;

        // Upsert the configuration
        query(
            r#"
            INSERT INTO octofhir.configuration (id, key, category, value, description, is_secret, txid)
            VALUES (gen_random_uuid(), $1, $2, $3, $4, $5, $6)
            ON CONFLICT (key) DO UPDATE SET
                value = EXCLUDED.value,
                description = EXCLUDED.description,
                is_secret = EXCLUDED.is_secret,
                txid = EXCLUDED.txid,
                ts = NOW()
            "#,
        )
        .bind(key)
        .bind(category)
        .bind(&value)
        .bind(description)
        .bind(is_secret)
        .bind(txid.0)
        .execute(&self.pool)
        .await
        .map_err(|e| ConfigError::database(format!("Failed to set config entry: {e}")))?;

        debug!("Set config {}.{}", category, key);
        Ok(())
    }

    /// Delete a configuration value
    pub async fn delete(&self, category: &str, key: &str) -> Result<bool, ConfigError> {
        let result = query(
            r#"
            DELETE FROM octofhir.configuration
            WHERE category = $1 AND key = $2
            "#,
        )
        .bind(category)
        .bind(key)
        .execute(&self.pool)
        .await
        .map_err(|e| ConfigError::database(format!("Failed to delete config entry: {e}")))?;

        Ok(result.rows_affected() > 0)
    }

    /// List all configuration entries for a category
    pub async fn list_category(&self, category: &str) -> Result<Vec<ConfigEntry>, ConfigError> {
        let entries: Vec<(String, String, serde_json::Value, Option<String>, bool)> = query_as(
            r#"
            SELECT key, category, value, description, is_secret
            FROM octofhir.configuration
            WHERE category = $1
            ORDER BY key
            "#,
        )
        .bind(category)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| ConfigError::database(format!("Failed to list config entries: {e}")))?;

        Ok(entries
            .into_iter()
            .map(
                |(key, category, value, description, is_secret)| ConfigEntry {
                    key,
                    category,
                    value,
                    description,
                    is_secret,
                },
            )
            .collect())
    }
}

#[async_trait]
impl ConfigSource for DatabaseSource {
    fn name(&self) -> &str {
        "database"
    }

    fn priority(&self) -> i32 {
        20 // Database has higher priority than file
    }

    async fn load(&self) -> Result<PartialConfig, ConfigError> {
        let entries = self.load_entries().await?;
        Ok(self.entries_to_partial(entries))
    }

    async fn watch(&self, tx: mpsc::Sender<ConfigChangeEvent>) -> Result<WatchHandle, ConfigError> {
        let pool = self.pool.clone();
        let reconnect_delay = self.config.reconnect_delay;
        let max_attempts = self.config.max_reconnect_attempts;

        let (shutdown_tx, mut shutdown_rx) = tokio::sync::oneshot::channel();

        let handle = tokio::spawn(async move {
            let mut attempts = 0;

            loop {
                // Check for shutdown
                if shutdown_rx.try_recv().is_ok() {
                    info!("Database config listener shutting down");
                    break;
                }

                // Connect listener
                let mut listener = match PgListener::connect_with(&pool).await {
                    Ok(l) => {
                        attempts = 0; // Reset on successful connect
                        l
                    }
                    Err(e) => {
                        error!("Failed to connect database listener: {e}");
                        attempts += 1;
                        if attempts >= max_attempts {
                            error!("Max reconnection attempts reached, stopping listener");
                            break;
                        }
                        tokio::time::sleep(reconnect_delay).await;
                        continue;
                    }
                };

                // Subscribe to channel
                if let Err(e) = listener.listen(CONFIG_CHANNEL).await {
                    error!("Failed to listen on {CONFIG_CHANNEL}: {e}");
                    tokio::time::sleep(reconnect_delay).await;
                    continue;
                }

                info!("Database config listener started on channel: {CONFIG_CHANNEL}");

                // Process notifications
                loop {
                    tokio::select! {
                        _ = &mut shutdown_rx => {
                            info!("Database config listener shutting down");
                            return;
                        }
                        notification = listener.recv() => {
                            match notification {
                                Ok(notif) => {
                                    debug!("Received config notification: {:?}", notif.payload());

                                    // Parse payload
                                    match serde_json::from_str::<NotifyPayload>(notif.payload()) {
                                        Ok(payload) => {
                                            let category = ConfigCategory::from_str(&payload.category)
                                                .unwrap_or(ConfigCategory::Server);

                                            let operation = match payload.operation.as_str() {
                                                "INSERT" => ConfigOperation::Set,
                                                "UPDATE" => ConfigOperation::Update,
                                                "DELETE" => ConfigOperation::Delete,
                                                _ => ConfigOperation::Update,
                                            };

                                            let event = ConfigChangeEvent::with_key(
                                                EventSource::Database,
                                                category,
                                                &payload.key,
                                                operation,
                                            );

                                            if tx.send(event).await.is_err() {
                                                warn!("Config change receiver dropped");
                                                return;
                                            }
                                        }
                                        Err(e) => {
                                            warn!("Failed to parse config notification: {e}");
                                        }
                                    }
                                }
                                Err(e) => {
                                    error!("Database listener error: {e}");
                                    break; // Reconnect
                                }
                            }
                        }
                    }
                }

                // Reconnect delay
                tokio::time::sleep(reconnect_delay).await;
            }
        });

        Ok(WatchHandle::new(handle, shutdown_tx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: These tests require a running PostgreSQL database
    // They are marked as ignored by default

    #[tokio::test]
    #[ignore]
    async fn test_database_source_load() {
        let pool = PgPool::connect("postgres://postgres@localhost/octofhir_test")
            .await
            .unwrap();

        let source = DatabaseSource::new(pool);
        let config = source.load().await.unwrap();

        // Should not error even if no config entries exist
        assert!(config.server.is_none() || config.server.is_some());
    }

    #[test]
    fn test_notify_payload_parsing() {
        let json = r#"{"key":"port","category":"server","operation":"UPDATE"}"#;
        let payload: NotifyPayload = serde_json::from_str(json).unwrap();

        assert_eq!(payload.key, "port");
        assert_eq!(payload.category, "server");
        assert_eq!(payload.operation, "UPDATE");
    }
}
