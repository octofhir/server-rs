//! PostgreSQL provider persistence, create-only bootstrap and on-demand credentials.
//!
//! # Contents
//! Versioned administrative mutations, redacted audit history and provider snapshots.
//!
//! # Invariants
//! Metadata and encrypted credentials commit atomically. A per-id transaction lock
//! serializes creation/deletion and prevents revision reuse. Secrets are encrypted
//! before entering SQL and never appear in public resources or history.
use super::{ProviderCredentials, ProviderMeta, StorageProvider};
use crate::egress::EgressPolicy;
use aws_credential_types::Credentials;
use octofhir_config::{SecretValue, Secrets};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx_core::{query::query, query_scalar::query_scalar, row::Row};
use sqlx_postgres::{PgPool, PgRow};
#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("{0}")]
    Invalid(String),
    #[error("Storage provider not found")]
    NotFound,
    #[error("{0}")]
    Conflict(String),
    #[error("Provider was deliberately removed; bootstrap seed skipped")]
    RetiredSeed,
    #[error("Storage provider database operation failed")]
    Database,
}
pub(crate) enum WriteMode {
    Create,
    Update(i64),
    Seed,
}
pub(crate) struct ResolvedProvider {
    pub provider: StorageProvider,
    encrypted: Option<SecretValue>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct KeyMaterial {
    access_key_id: String,
    secret_access_key: String,
    session_token: Option<String>,
}
fn parse(row: &PgRow) -> Result<StorageProvider, ProviderError> {
    let mut provider: StorageProvider =
        serde_json::from_value(row.get("resource")).map_err(|_| ProviderError::Database)?;
    let time: chrono::DateTime<chrono::Utc> = row.get("updated_at");
    provider.meta = Some(ProviderMeta {
        version_id: row.get::<i64, _>("version").to_string(),
        last_updated: time.to_rfc3339(),
    });
    Ok(provider)
}
pub(crate) async fn read(pool: &PgPool, id: &str) -> Result<StorageProvider, ProviderError> {
    let row = query("SELECT resource,version,updated_at FROM _storage_providers WHERE id=$1")
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(|_| ProviderError::Database)?
        .ok_or(ProviderError::NotFound)?;
    parse(&row)
}
pub(crate) async fn list(
    pool: &PgPool,
    limit: i64,
    offset: i64,
) -> Result<Vec<StorageProvider>, ProviderError> {
    let rows = query(
        "SELECT resource,version,updated_at FROM _storage_providers ORDER BY id LIMIT $1 OFFSET $2",
    )
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await
    .map_err(|_| ProviderError::Database)?;
    rows.iter().map(parse).collect()
}
pub(crate) async fn save(
    pool: &PgPool,
    mut provider: StorageProvider,
    actor: &str,
    mode: WriteMode,
    policy: &EgressPolicy,
) -> Result<StorageProvider, ProviderError> {
    super::model::validate_id(&provider.id).map_err(ProviderError::Invalid)?;
    let mut tx = pool.begin().await.map_err(|_| ProviderError::Database)?;
    query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(format!("storage-provider:{}", provider.id))
        .execute(&mut *tx)
        .await
        .map_err(|_| ProviderError::Database)?;
    let existing=query("SELECT resource,version,updated_at,encrypted_credentials FROM _storage_providers WHERE id=$1 FOR UPDATE").bind(&provider.id).fetch_optional(&mut *tx).await.map_err(|_|ProviderError::Database)?;
    match (&mode, &existing) {
        (WriteMode::Create, Some(_)) => {
            return Err(ProviderError::Conflict("Provider id already exists".into()));
        }
        (WriteMode::Update(_), None) => return Err(ProviderError::NotFound),
        (WriteMode::Update(expected), Some(row)) if row.get::<i64, _>("version") != *expected => {
            return Err(ProviderError::Conflict(
                "Provider changed; reload before saving".into(),
            ));
        }
        (WriteMode::Seed, Some(row)) => return parse(row),
        _ => {}
    }
    if matches!(mode, WriteMode::Seed) && existing.is_none() {
        let retired: bool = query_scalar(
            "SELECT EXISTS(SELECT 1 FROM _storage_provider_history WHERE provider_id=$1)",
        )
        .bind(&provider.id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| ProviderError::Database)?;
        if retired {
            return Err(ProviderError::RetiredSeed);
        }
    }
    provider.validate(policy).map_err(ProviderError::Invalid)?;
    let encrypted = match &provider.credentials {
        ProviderCredentials::Environment { .. } => None,
        ProviderCredentials::Stored {
            access_key_id: Some(access),
            secret_access_key: Some(secret),
            session_token,
        } => {
            let manager = Secrets::from_env()
                .map_err(|_| ProviderError::Invalid("Invalid OCTOFHIR_CONFIG_KEY".into()))?
                .ok_or_else(|| {
                    ProviderError::Invalid("Stored credentials require OCTOFHIR_CONFIG_KEY".into())
                })?;
            let material = KeyMaterial {
                access_key_id: access.clone(),
                secret_access_key: secret.clone(),
                session_token: session_token.clone(),
            };
            let plaintext =
                serde_json::to_string(&material).map_err(|_| ProviderError::Database)?;
            let value = manager
                .encrypt(&plaintext)
                .await
                .map_err(|_| ProviderError::Invalid("Cannot encrypt storage credentials".into()))?;
            Some(serde_json::to_value(value).map_err(|_| ProviderError::Database)?)
        }
        ProviderCredentials::Stored { .. } => existing
            .as_ref()
            .and_then(|row| row.get::<Option<Value>, _>("encrypted_credentials"))
            .ok_or_else(|| {
                ProviderError::Invalid(
                    "New stored credentials require accessKeyId and secretAccessKey".into(),
                )
            })
            .map(Some)?,
    };
    provider.meta = None;
    let resource = serde_json::to_value(&provider).map_err(|_| ProviderError::Database)?;
    let version: i64 = query_scalar(
        "SELECT COALESCE(MAX(version),0)+1 FROM _storage_provider_history WHERE provider_id=$1",
    )
    .bind(&provider.id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| ProviderError::Database)?;
    let row=query("INSERT INTO _storage_providers(id,version,resource,encrypted_credentials,updated_by) VALUES($1,$2,$3,$4,$5) ON CONFLICT(id) DO UPDATE SET version=EXCLUDED.version,resource=EXCLUDED.resource,encrypted_credentials=EXCLUDED.encrypted_credentials,updated_by=EXCLUDED.updated_by,updated_at=clock_timestamp() RETURNING resource,version,updated_at").bind(&provider.id).bind(version).bind(&resource).bind(encrypted).bind(actor).fetch_one(&mut *tx).await.map_err(|_|ProviderError::Database)?;
    query("INSERT INTO _storage_provider_history(provider_id,version,action,resource,actor) VALUES($1,$2,$3,$4,$5)").bind(&provider.id).bind(version).bind(if existing.is_some(){"update"}else{"create"}).bind(resource).bind(actor).execute(&mut *tx).await.map_err(|_|ProviderError::Database)?;
    tx.commit().await.map_err(|_| ProviderError::Database)?;
    parse(&row)
}
pub(crate) async fn remove(
    pool: &PgPool,
    id: &str,
    version: i64,
    actor: &str,
) -> Result<(), ProviderError> {
    let mut tx = pool.begin().await.map_err(|_| ProviderError::Database)?;
    query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(format!("storage-provider:{id}"))
        .execute(&mut *tx)
        .await
        .map_err(|_| ProviderError::Database)?;
    let row = query("SELECT version,resource FROM _storage_providers WHERE id=$1 FOR UPDATE")
        .bind(id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|_| ProviderError::Database)?
        .ok_or(ProviderError::NotFound)?;
    if row.get::<i64, _>("version") != version {
        return Err(ProviderError::Conflict(
            "Provider changed; reload before deleting".into(),
        ));
    }
    query("INSERT INTO _storage_provider_history(provider_id,version,action,resource,actor) VALUES($1,$2,'delete',$3,$4)").bind(id).bind(version+1).bind(row.get::<Value,_>("resource")).bind(actor).execute(&mut *tx).await.map_err(|_|ProviderError::Database)?;
    query("DELETE FROM _storage_providers WHERE id=$1")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(|_| ProviderError::Database)?;
    tx.commit().await.map_err(|_| ProviderError::Database)?;
    Ok(())
}
pub(crate) async fn bootstrap(
    pool: &PgPool,
    providers: &[StorageProvider],
    policy: &EgressPolicy,
) -> Result<(), String> {
    for provider in providers {
        match save(pool, provider.clone(), "bootstrap", WriteMode::Seed, policy).await {
            Ok(_) | Err(ProviderError::RetiredSeed) => {}
            Err(error) => return Err(format!("StorageProvider/{}: {error}", provider.id)),
        }
    }
    Ok(())
}
pub(crate) async fn resolve(
    pool: &PgPool,
    id: &str,
    policy: &EgressPolicy,
) -> Result<ResolvedProvider, String> {
    let row=query("SELECT resource,version,updated_at,encrypted_credentials FROM _storage_providers WHERE id=$1").bind(id).fetch_optional(pool).await.map_err(|_|"Cannot read storage provider")?.ok_or_else(||format!("StorageProvider/{id} is unavailable"))?;
    let provider = parse(&row).map_err(|e| e.to_string())?;
    provider.validate(policy)?;
    if !provider.enabled {
        return Err(format!("StorageProvider/{id} is disabled"));
    }
    let encrypted = row
        .get::<Option<Value>, _>("encrypted_credentials")
        .map(serde_json::from_value)
        .transpose()
        .map_err(|_| "Invalid encrypted storage credentials")?;
    Ok(ResolvedProvider {
        provider,
        encrypted,
    })
}
impl ResolvedProvider {
    pub(crate) async fn credentials(&self) -> Result<Credentials, String> {
        match &self.provider.credentials {
            ProviderCredentials::Environment {
                access_key_id_env,
                secret_access_key_env,
                session_token_env,
            } => {
                let required = |name: &str| {
                    std::env::var(name)
                        .ok()
                        .filter(|v| !v.is_empty())
                        .ok_or_else(|| {
                            format!(
                                "Storage credential environment variable {name} is missing or empty"
                            )
                        })
                };
                let access = required(access_key_id_env)?;
                let secret = required(secret_access_key_env)?;
                let token = match session_token_env {
                    Some(name) => Some(required(name)?),
                    None if access_key_id_env == "AWS_ACCESS_KEY_ID"
                        && secret_access_key_env == "AWS_SECRET_ACCESS_KEY" =>
                    {
                        std::env::var("AWS_SESSION_TOKEN")
                            .ok()
                            .filter(|v| !v.is_empty())
                    }
                    None => None,
                };
                Ok(Credentials::new(
                    access,
                    secret,
                    token,
                    None,
                    "octofhir-storage-provider",
                ))
            }
            ProviderCredentials::Stored { .. } => {
                let encrypted = self
                    .encrypted
                    .as_ref()
                    .ok_or("Storage provider has no stored credentials")?;
                let manager = Secrets::from_env()
                    .map_err(|_| "Invalid OCTOFHIR_CONFIG_KEY")?
                    .ok_or("Stored credentials require OCTOFHIR_CONFIG_KEY")?;
                let plaintext = manager
                    .decrypt(encrypted)
                    .await
                    .map_err(|_| "Cannot decrypt storage credentials")?;
                let material: KeyMaterial = serde_json::from_str(&plaintext)
                    .map_err(|_| "Invalid stored credential format")?;
                Ok(Credentials::new(
                    material.access_key_id,
                    material.secret_access_key,
                    material.session_token,
                    None,
                    "octofhir-storage-provider",
                ))
            }
        }
    }
}
