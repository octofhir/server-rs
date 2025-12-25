//! Authorization session storage.
//!
//! Stores OAuth authorization sessions during the authorization code flow.
//! Sessions track PKCE challenges, scopes, and other flow parameters.

use sqlx_core::query::query;
use sqlx_core::query_as::query_as;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{PgPool, StorageError, StorageResult};

// =============================================================================
// Types
// =============================================================================

/// Session record from database.
///
/// Follows the standard FHIR resource table structure.
#[derive(Debug, Clone)]
pub struct SessionRow {
    /// Resource ID (TEXT in database, supports both UUIDs and custom IDs)
    pub id: String,
    /// Transaction ID (version)
    pub txid: i64,
    /// Created timestamp
    pub created_at: OffsetDateTime,
    /// Updated timestamp
    pub updated_at: OffsetDateTime,
    /// Full session resource as JSONB
    pub resource: serde_json::Value,
    /// Resource status (created, updated, deleted)
    pub status: String,
}

impl SessionRow {
    /// Create from database tuple.
    fn from_tuple(
        row: (
            String,
            i64,
            OffsetDateTime,
            OffsetDateTime,
            serde_json::Value,
            String,
        ),
    ) -> Self {
        Self {
            id: row.0,
            txid: row.1,
            created_at: row.2,
            updated_at: row.3,
            resource: row.4,
            status: row.5,
        }
    }
}

// =============================================================================
// Session Storage
// =============================================================================

/// Session storage operations.
///
/// Manages authorization sessions during OAuth flows.
/// Uses the standard FHIR resource table pattern in the public schema.
pub struct SessionStorage<'a> {
    pool: &'a PgPool,
}

impl<'a> SessionStorage<'a> {
    /// Create a new session storage with a connection pool reference.
    #[must_use]
    pub fn new(pool: &'a PgPool) -> Self {
        Self { pool }
    }

    /// Find a session by authorization code.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub async fn find_by_code(&self, code: &str) -> StorageResult<Option<SessionRow>> {
        let row: Option<(
            String,
            i64,
            OffsetDateTime,
            OffsetDateTime,
            serde_json::Value,
            String,
        )> = query_as(
            r#"
            SELECT id, txid, created_at, updated_at, resource, status::text
            FROM session
            WHERE resource->>'code' = $1
              AND status != 'deleted'
            "#,
        )
        .bind(code)
        .fetch_optional(self.pool)
        .await?;

        Ok(row.map(SessionRow::from_tuple))
    }

    /// Find a session by its ID.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub async fn find_by_id(&self, id: Uuid) -> StorageResult<Option<SessionRow>> {
        let row: Option<(
            String,
            i64,
            OffsetDateTime,
            OffsetDateTime,
            serde_json::Value,
            String,
        )> = query_as(
            r#"
            SELECT id, txid, created_at, updated_at, resource, status::text
            FROM session
            WHERE id = $1
              AND status != 'deleted'
            "#,
        )
        .bind(id.to_string())
        .fetch_optional(self.pool)
        .await?;

        Ok(row.map(SessionRow::from_tuple))
    }

    /// Create a new session.
    ///
    /// # Errors
    ///
    /// Returns an error if the database insert fails.
    pub async fn create(&self, id: Uuid, resource: serde_json::Value) -> StorageResult<SessionRow> {
        let id_str = id.to_string();
        let row: (
            String,
            i64,
            OffsetDateTime,
            OffsetDateTime,
            serde_json::Value,
            String,
        ) = query_as(
            r#"
            INSERT INTO session (id, txid, created_at, updated_at, resource, status)
            VALUES ($1, 1, NOW(), NOW(), $2, 'created')
            RETURNING id, txid, created_at, updated_at, resource, status::text
            "#,
        )
        .bind(&id_str)
        .bind(&resource)
        .fetch_one(self.pool)
        .await
        .map_err(|e| {
            if let sqlx_core::Error::Database(ref db_err) = e
                && db_err.is_unique_violation()
            {
                return StorageError::conflict(format!("Session with id '{}' already exists", id));
            }
            StorageError::from(e)
        })?;

        Ok(SessionRow::from_tuple(row))
    }

    /// Mark a session's authorization code as used.
    ///
    /// # Errors
    ///
    /// Returns an error if the session doesn't exist or the database update fails.
    pub async fn mark_used(&self, id: Uuid) -> StorageResult<SessionRow> {
        let row: Option<(
            String,
            i64,
            OffsetDateTime,
            OffsetDateTime,
            serde_json::Value,
            String,
        )> = query_as(
            r#"
            UPDATE session
            SET resource = jsonb_set(resource, '{used}', 'true'),
                txid = txid + 1,
                updated_at = NOW(),
                status = 'updated'
            WHERE id = $1
              AND status != 'deleted'
            RETURNING id, txid, created_at, updated_at, resource, status::text
            "#,
        )
        .bind(id.to_string())
        .fetch_optional(self.pool)
        .await?;

        row.map(SessionRow::from_tuple)
            .ok_or_else(|| StorageError::not_found(format!("Session {}", id)))
    }

    /// Delete a session (soft delete).
    ///
    /// # Errors
    ///
    /// Returns an error if the session doesn't exist or the database update fails.
    pub async fn delete(&self, id: Uuid) -> StorageResult<()> {
        let result = query(
            r#"
            UPDATE session
            SET status = 'deleted',
                txid = txid + 1,
                updated_at = NOW()
            WHERE id = $1
              AND status != 'deleted'
            "#,
        )
        .bind(id.to_string())
        .execute(self.pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(StorageError::not_found(format!("Session {}", id)));
        }

        Ok(())
    }

    /// Delete expired sessions.
    ///
    /// # Errors
    ///
    /// Returns an error if the database update fails.
    pub async fn delete_expired(&self) -> StorageResult<u64> {
        let result = query(
            r#"
            UPDATE session
            SET status = 'deleted',
                txid = txid + 1,
                updated_at = NOW()
            WHERE status != 'deleted'
              AND (resource->>'expiresAt')::timestamptz < NOW()
            "#,
        )
        .execute(self.pool)
        .await?;

        Ok(result.rows_affected())
    }

    /// Cleanup expired sessions (alias for `delete_expired`).
    pub async fn cleanup_expired(&self) -> StorageResult<u64> {
        self.delete_expired().await
    }

    /// Delete all sessions for a specific client.
    ///
    /// # Errors
    ///
    /// Returns an error if the database update fails.
    pub async fn delete_by_client(&self, client_id: &str) -> StorageResult<u64> {
        let result = query(
            r#"
            UPDATE session
            SET status = 'deleted',
                txid = txid + 1,
                updated_at = NOW()
            WHERE resource->>'clientId' = $1
              AND status != 'deleted'
            "#,
        )
        .bind(client_id)
        .execute(self.pool)
        .await?;

        Ok(result.rows_affected())
    }

    /// Update a session.
    ///
    /// # Errors
    ///
    /// Returns an error if the session doesn't exist or the database update fails.
    pub async fn update(&self, id: Uuid, resource: serde_json::Value) -> StorageResult<SessionRow> {
        let row: Option<(
            String,
            i64,
            OffsetDateTime,
            OffsetDateTime,
            serde_json::Value,
            String,
        )> = query_as(
            r#"
            UPDATE session
            SET resource = $2,
                txid = txid + 1,
                updated_at = NOW(),
                status = 'updated'
            WHERE id = $1
              AND status != 'deleted'
            RETURNING id, txid, created_at, updated_at, resource, status::text
            "#,
        )
        .bind(id.to_string())
        .bind(&resource)
        .fetch_optional(self.pool)
        .await?;

        row.map(SessionRow::from_tuple)
            .ok_or_else(|| StorageError::not_found(format!("Session {}", id)))
    }
}
