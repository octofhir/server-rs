//! OAuth client storage.
//!
//! Provides CRUD operations for OAuth 2.0 client registrations.
//! Clients are stored as standard FHIR resources in the `client` table.
//!
//! This module provides two layers:
//! - [`ClientStorage`] - Low-level CRUD operations on `ClientRow`
//! - [`PostgresClientStorage`] - Implements the `ClientStorage` trait from `octofhir-auth`

use async_trait::async_trait;
use bcrypt::verify;
use sqlx_core::query::query;
use sqlx_core::query_as::query_as;
use time::OffsetDateTime;
use uuid::Uuid;

use octofhir_auth::storage::ClientStorage as ClientStorageTrait;
use octofhir_auth::types::Client;
use octofhir_auth::{AuthError, AuthResult};

use crate::{PgPool, StorageError, StorageResult};

// =============================================================================
// Types
// =============================================================================

/// Client record from database.
///
/// Follows the standard FHIR resource table structure.
#[derive(Debug, Clone)]
pub struct ClientRow {
    /// Resource UUID (from resource.id)
    pub id: Uuid,
    /// Transaction ID (version)
    pub txid: i64,
    /// Timestamp
    pub ts: OffsetDateTime,
    /// Full client resource as JSONB
    pub resource: serde_json::Value,
    /// Resource status (created, updated, deleted)
    pub status: String,
}

impl ClientRow {
    /// Create from database tuple.
    fn from_tuple(row: (Uuid, i64, OffsetDateTime, serde_json::Value, String)) -> Self {
        Self {
            id: row.0,
            txid: row.1,
            ts: row.2,
            resource: row.3,
            status: row.4,
        }
    }
}

// =============================================================================
// Client Storage
// =============================================================================

/// Client storage operations.
///
/// Provides methods for managing OAuth 2.0 client registrations in PostgreSQL.
/// Uses the standard FHIR resource table pattern in the public schema.
pub struct ClientStorage<'a> {
    pool: &'a PgPool,
}

impl<'a> ClientStorage<'a> {
    /// Create a new client storage with a connection pool reference.
    #[must_use]
    pub fn new(pool: &'a PgPool) -> Self {
        Self { pool }
    }

    /// Find a client by its OAuth client_id.
    ///
    /// Returns `None` if the client doesn't exist or is deleted.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub async fn find_by_client_id(&self, client_id: &str) -> StorageResult<Option<ClientRow>> {
        let row: Option<(Uuid, i64, OffsetDateTime, serde_json::Value, String)> = query_as(
            r#"
            SELECT id, txid, ts, resource, status
            FROM client
            WHERE resource->>'clientId' = $1
              AND status != 'deleted'
            "#,
        )
        .bind(client_id)
        .fetch_optional(self.pool)
        .await?;

        Ok(row.map(ClientRow::from_tuple))
    }

    /// Find a client by its resource ID.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub async fn find_by_id(&self, id: Uuid) -> StorageResult<Option<ClientRow>> {
        let row: Option<(Uuid, i64, OffsetDateTime, serde_json::Value, String)> = query_as(
            r#"
            SELECT id, txid, ts, resource, status
            FROM client
            WHERE id = $1
              AND status != 'deleted'
            "#,
        )
        .bind(id)
        .fetch_optional(self.pool)
        .await?;

        Ok(row.map(ClientRow::from_tuple))
    }

    /// Create a new client.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The database insert fails
    /// - A client with the same ID already exists
    pub async fn create(&self, id: Uuid, resource: serde_json::Value) -> StorageResult<ClientRow> {
        let row: (Uuid, i64, OffsetDateTime, serde_json::Value, String) = query_as(
            r#"
            INSERT INTO client (id, txid, ts, resource, status)
            VALUES ($1, 1, NOW(), $2, 'created')
            RETURNING id, txid, ts, resource, status
            "#,
        )
        .bind(id)
        .bind(&resource)
        .fetch_one(self.pool)
        .await
        .map_err(|e| {
            if let sqlx_core::Error::Database(ref db_err) = e
                && db_err.is_unique_violation()
            {
                return StorageError::conflict(format!("Client with id '{}' already exists", id));
            }
            StorageError::from(e)
        })?;

        Ok(ClientRow::from_tuple(row))
    }

    /// Update an existing client.
    ///
    /// Increments the transaction ID (version) on each update.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The client doesn't exist
    /// - The database update fails
    pub async fn update(&self, id: Uuid, resource: serde_json::Value) -> StorageResult<ClientRow> {
        let row: Option<(Uuid, i64, OffsetDateTime, serde_json::Value, String)> = query_as(
            r#"
            UPDATE client
            SET resource = $2,
                txid = txid + 1,
                ts = NOW(),
                status = 'updated'
            WHERE id = $1
              AND status != 'deleted'
            RETURNING id, txid, ts, resource, status
            "#,
        )
        .bind(id)
        .bind(&resource)
        .fetch_optional(self.pool)
        .await?;

        row.map(ClientRow::from_tuple)
            .ok_or_else(|| StorageError::not_found(format!("Client {}", id)))
    }

    /// Delete a client (soft delete).
    ///
    /// Sets the status to 'deleted' rather than removing the row.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The client doesn't exist
    /// - The database update fails
    pub async fn delete(&self, id: Uuid) -> StorageResult<()> {
        let result = query(
            r#"
            UPDATE client
            SET status = 'deleted',
                txid = txid + 1,
                ts = NOW()
            WHERE id = $1
              AND status != 'deleted'
            "#,
        )
        .bind(id)
        .execute(self.pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(StorageError::not_found(format!("Client {}", id)));
        }

        Ok(())
    }

    /// List all active clients.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub async fn list(&self, limit: i64, offset: i64) -> StorageResult<Vec<ClientRow>> {
        let rows: Vec<(Uuid, i64, OffsetDateTime, serde_json::Value, String)> = query_as(
            r#"
            SELECT id, txid, ts, resource, status
            FROM client
            WHERE status != 'deleted'
            ORDER BY ts DESC
            LIMIT $1 OFFSET $2
            "#,
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(self.pool)
        .await?;

        Ok(rows.into_iter().map(ClientRow::from_tuple).collect())
    }

    /// Count all active clients.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub async fn count(&self) -> StorageResult<i64> {
        let count: (i64,) = query_as(
            r#"
            SELECT COUNT(*)
            FROM client
            WHERE status != 'deleted'
            "#,
        )
        .fetch_one(self.pool)
        .await?;

        Ok(count.0)
    }
}

// =============================================================================
// PostgreSQL Client Storage Trait Implementation
// =============================================================================

/// PostgreSQL implementation of the `ClientStorage` trait from `octofhir-auth`.
///
/// This struct wraps the low-level [`ClientStorage`] and implements the trait
/// interface, handling serialization/deserialization between domain types and JSON.
///
/// # Example
///
/// ```ignore
/// use octofhir_auth_postgres::PostgresClientStorage;
/// use octofhir_auth::storage::ClientStorage;
///
/// let storage = PostgresClientStorage::new(&pool);
/// let client = storage.find_by_client_id("my-app").await?;
/// ```
pub struct PostgresClientStorage<'a> {
    pool: &'a PgPool,
}

impl<'a> PostgresClientStorage<'a> {
    /// Create a new PostgreSQL client storage.
    #[must_use]
    pub fn new(pool: &'a PgPool) -> Self {
        Self { pool }
    }

    /// Get the low-level storage operations.
    fn storage(&self) -> ClientStorage<'_> {
        ClientStorage::new(self.pool)
    }

    /// Convert storage error to auth error.
    fn map_storage_error(err: StorageError) -> AuthError {
        if err.is_not_found() || err.is_conflict() {
            AuthError::invalid_client(err.to_string())
        } else {
            AuthError::storage(err.to_string())
        }
    }
}

#[async_trait]
impl ClientStorageTrait for PostgresClientStorage<'_> {
    async fn find_by_client_id(&self, client_id: &str) -> AuthResult<Option<Client>> {
        let row = self
            .storage()
            .find_by_client_id(client_id)
            .await
            .map_err(Self::map_storage_error)?;

        match row {
            Some(r) => {
                let client: Client = serde_json::from_value(r.resource).map_err(|e| {
                    AuthError::storage(format!("Failed to deserialize client: {}", e))
                })?;
                Ok(Some(client))
            }
            None => Ok(None),
        }
    }

    async fn create(&self, client: &Client) -> AuthResult<Client> {
        // Validate client before creation
        client
            .validate()
            .map_err(|e| AuthError::invalid_client(e.to_string()))?;

        let id = Uuid::new_v4();
        let resource = serde_json::to_value(client)
            .map_err(|e| AuthError::storage(format!("Failed to serialize client: {}", e)))?;

        let row = self
            .storage()
            .create(id, resource)
            .await
            .map_err(Self::map_storage_error)?;

        serde_json::from_value(row.resource)
            .map_err(|e| AuthError::storage(format!("Failed to deserialize client: {}", e)))
    }

    async fn update(&self, client_id: &str, client: &Client) -> AuthResult<Client> {
        // Validate client before update
        client
            .validate()
            .map_err(|e| AuthError::invalid_client(e.to_string()))?;

        // Find existing client to get its UUID
        let existing = self
            .storage()
            .find_by_client_id(client_id)
            .await
            .map_err(Self::map_storage_error)?
            .ok_or_else(|| {
                AuthError::invalid_client(format!("Client '{}' not found", client_id))
            })?;

        let resource = serde_json::to_value(client)
            .map_err(|e| AuthError::storage(format!("Failed to serialize client: {}", e)))?;

        let row = self
            .storage()
            .update(existing.id, resource)
            .await
            .map_err(Self::map_storage_error)?;

        serde_json::from_value(row.resource)
            .map_err(|e| AuthError::storage(format!("Failed to deserialize client: {}", e)))
    }

    async fn delete(&self, client_id: &str) -> AuthResult<()> {
        // Find existing client to get its UUID
        let existing = self
            .storage()
            .find_by_client_id(client_id)
            .await
            .map_err(Self::map_storage_error)?
            .ok_or_else(|| {
                AuthError::invalid_client(format!("Client '{}' not found", client_id))
            })?;

        self.storage()
            .delete(existing.id)
            .await
            .map_err(Self::map_storage_error)
    }

    async fn list(&self, limit: i64, offset: i64) -> AuthResult<Vec<Client>> {
        let rows = self
            .storage()
            .list(limit, offset)
            .await
            .map_err(Self::map_storage_error)?;

        rows.into_iter()
            .map(|r| {
                serde_json::from_value(r.resource)
                    .map_err(|e| AuthError::storage(format!("Failed to deserialize client: {}", e)))
            })
            .collect()
    }

    async fn verify_secret(&self, client_id: &str, secret: &str) -> AuthResult<bool> {
        let client = self.find_by_client_id(client_id).await?.ok_or_else(|| {
            AuthError::invalid_client(format!("Client '{}' not found", client_id))
        })?;

        match &client.client_secret {
            Some(hash) => {
                // BCrypt verify returns Result<bool, BcryptError>
                Ok(verify(secret, hash).unwrap_or(false))
            }
            None => Ok(false),
        }
    }
}
