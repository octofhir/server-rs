//! Database storage operations for configuration
//!
//! Provides CRUD operations for configuration stored in PostgreSQL.

use crate::ConfigError;
use crate::secrets::{SecretValue, Secrets};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx_core::query::query;
use sqlx_core::query_as::query_as;
use sqlx_postgres::PgPool;
use uuid::Uuid;

/// A stored configuration entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredConfig {
    pub id: Uuid,
    pub key: String,
    pub category: String,
    pub value: serde_json::Value,
    pub description: Option<String>,
    pub is_secret: bool,
    pub txid: i64,
    pub ts: DateTime<Utc>,
    pub updated_by: Option<String>,
}

/// Configuration storage operations
pub struct ConfigStorage {
    pool: PgPool,
    secrets: Option<Secrets>,
}

impl ConfigStorage {
    /// Create a new config storage
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            secrets: None,
        }
    }

    /// Create with secrets manager for encryption
    pub fn with_secrets(pool: PgPool, secrets: Secrets) -> Self {
        Self {
            pool,
            secrets: Some(secrets),
        }
    }

    /// Get a configuration value
    pub async fn get(
        &self,
        category: &str,
        key: &str,
    ) -> Result<Option<StoredConfig>, ConfigError> {
        let result: Option<(
            Uuid,
            String,
            String,
            serde_json::Value,
            Option<String>,
            bool,
            i64,
            DateTime<Utc>,
            Option<String>,
        )> = query_as(
            r#"
                SELECT id, key, category, value, description, is_secret, txid, ts, updated_by
                FROM octofhir.configuration
                WHERE category = $1 AND key = $2
                "#,
        )
        .bind(category)
        .bind(key)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| ConfigError::database(format!("Failed to get config: {e}")))?;

        Ok(result.map(
            |(id, key, category, value, description, is_secret, txid, ts, updated_by)| {
                StoredConfig {
                    id,
                    key,
                    category,
                    value,
                    description,
                    is_secret,
                    txid,
                    ts,
                    updated_by,
                }
            },
        ))
    }

    /// Get a decrypted configuration value
    pub async fn get_decrypted(
        &self,
        category: &str,
        key: &str,
    ) -> Result<Option<serde_json::Value>, ConfigError> {
        let config = self.get(category, key).await?;

        match config {
            Some(c) if c.is_secret => {
                // Decrypt the value
                let secret: SecretValue = serde_json::from_value(c.value)
                    .map_err(|e| ConfigError::encryption(format!("Invalid secret format: {e}")))?;

                let secrets = self.secrets.as_ref().ok_or_else(|| {
                    ConfigError::encryption("Secrets manager required to decrypt value")
                })?;

                let plaintext = secrets.decrypt(&secret).await?;
                Ok(Some(serde_json::Value::String(plaintext)))
            }
            Some(c) => Ok(Some(c.value)),
            None => Ok(None),
        }
    }

    /// Set a configuration value
    pub async fn set(
        &self,
        category: &str,
        key: &str,
        value: serde_json::Value,
        description: Option<&str>,
        is_secret: bool,
        updated_by: Option<&str>,
    ) -> Result<StoredConfig, ConfigError> {
        // Encrypt if secret and secrets manager available
        let stored_value = if is_secret {
            if let Some(secrets) = &self.secrets {
                let plaintext = match &value {
                    serde_json::Value::String(s) => s.clone(),
                    v => v.to_string(),
                };
                let encrypted = secrets.encrypt(&plaintext).await?;
                serde_json::to_value(encrypted).map_err(|e| {
                    ConfigError::encryption(format!("Failed to serialize secret: {e}"))
                })?
            } else {
                value
            }
        } else {
            value
        };

        // Create transaction
        let txid: (i64,) =
            query_as("INSERT INTO _transaction (status) VALUES ('committed') RETURNING txid")
                .fetch_one(&self.pool)
                .await
                .map_err(|e| ConfigError::database(format!("Failed to create transaction: {e}")))?;

        // Upsert configuration
        let result: (Uuid, DateTime<Utc>) = query_as(
            r#"
            INSERT INTO octofhir.configuration (id, key, category, value, description, is_secret, txid, updated_by)
            VALUES (gen_random_uuid(), $1, $2, $3, $4, $5, $6, $7)
            ON CONFLICT (key) DO UPDATE SET
                value = EXCLUDED.value,
                description = EXCLUDED.description,
                is_secret = EXCLUDED.is_secret,
                txid = EXCLUDED.txid,
                updated_by = EXCLUDED.updated_by,
                ts = NOW()
            RETURNING id, ts
            "#,
        )
        .bind(key)
        .bind(category)
        .bind(&stored_value)
        .bind(description)
        .bind(is_secret)
        .bind(txid.0)
        .bind(updated_by)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| ConfigError::database(format!("Failed to set config: {e}")))?;

        Ok(StoredConfig {
            id: result.0,
            key: key.to_string(),
            category: category.to_string(),
            value: stored_value,
            description: description.map(String::from),
            is_secret,
            txid: txid.0,
            ts: result.1,
            updated_by: updated_by.map(String::from),
        })
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
        .map_err(|e| ConfigError::database(format!("Failed to delete config: {e}")))?;

        Ok(result.rows_affected() > 0)
    }

    /// List all configuration for a category
    pub async fn list_category(&self, category: &str) -> Result<Vec<StoredConfig>, ConfigError> {
        let rows: Vec<(
            Uuid,
            String,
            String,
            serde_json::Value,
            Option<String>,
            bool,
            i64,
            DateTime<Utc>,
            Option<String>,
        )> = query_as(
            r#"
                SELECT id, key, category, value, description, is_secret, txid, ts, updated_by
                FROM octofhir.configuration
                WHERE category = $1
                ORDER BY key
                "#,
        )
        .bind(category)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| ConfigError::database(format!("Failed to list config: {e}")))?;

        Ok(rows
            .into_iter()
            .map(
                |(id, key, category, value, description, is_secret, txid, ts, updated_by)| {
                    StoredConfig {
                        id,
                        key,
                        category,
                        value,
                        description,
                        is_secret,
                        txid,
                        ts,
                        updated_by,
                    }
                },
            )
            .collect())
    }

    /// List all configuration
    pub async fn list_all(&self) -> Result<Vec<StoredConfig>, ConfigError> {
        let rows: Vec<(
            Uuid,
            String,
            String,
            serde_json::Value,
            Option<String>,
            bool,
            i64,
            DateTime<Utc>,
            Option<String>,
        )> = query_as(
            r#"
                SELECT id, key, category, value, description, is_secret, txid, ts, updated_by
                FROM octofhir.configuration
                ORDER BY category, key
                "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| ConfigError::database(format!("Failed to list config: {e}")))?;

        Ok(rows
            .into_iter()
            .map(
                |(id, key, category, value, description, is_secret, txid, ts, updated_by)| {
                    StoredConfig {
                        id,
                        key,
                        category,
                        value,
                        description,
                        is_secret,
                        txid,
                        ts,
                        updated_by,
                    }
                },
            )
            .collect())
    }

    /// Get configuration history for a key
    pub async fn get_history(
        &self,
        category: &str,
        key: &str,
        limit: i64,
    ) -> Result<Vec<StoredConfig>, ConfigError> {
        let rows: Vec<(
            Uuid,
            String,
            String,
            serde_json::Value,
            Option<String>,
            bool,
            i64,
            DateTime<Utc>,
        )> = query_as(
            r#"
                SELECT id, key, category, value, description, is_secret, txid, ts
                FROM octofhir.configuration_history
                WHERE category = $1 AND key = $2
                ORDER BY txid DESC
                LIMIT $3
                "#,
        )
        .bind(category)
        .bind(key)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| ConfigError::database(format!("Failed to get config history: {e}")))?;

        Ok(rows
            .into_iter()
            .map(
                |(id, key, category, value, description, is_secret, txid, ts)| StoredConfig {
                    id,
                    key,
                    category,
                    value,
                    description,
                    is_secret,
                    txid,
                    ts,
                    updated_by: None,
                },
            )
            .collect())
    }
}

impl std::fmt::Debug for ConfigStorage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConfigStorage")
            .field("pool", &"<PgPool>")
            .field("secrets", &self.secrets.is_some())
            .finish()
    }
}
