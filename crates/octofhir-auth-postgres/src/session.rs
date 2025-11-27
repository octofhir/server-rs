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
    /// Resource UUID
    pub id: Uuid,
    /// Transaction ID (version)
    pub txid: i64,
    /// Timestamp
    pub ts: OffsetDateTime,
    /// Full session resource as JSONB
    pub resource: serde_json::Value,
    /// Resource status (created, updated, deleted)
    pub status: String,
}

impl SessionRow {
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
        let row: Option<(Uuid, i64, OffsetDateTime, serde_json::Value, String)> = query_as(
            r#"
            SELECT id, txid, ts, resource, status
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
        let row: Option<(Uuid, i64, OffsetDateTime, serde_json::Value, String)> = query_as(
            r#"
            SELECT id, txid, ts, resource, status
            FROM session
            WHERE id = $1
              AND status != 'deleted'
            "#,
        )
        .bind(id)
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
        let row: (Uuid, i64, OffsetDateTime, serde_json::Value, String) = query_as(
            r#"
            INSERT INTO session (id, txid, ts, resource, status)
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
        let row: Option<(Uuid, i64, OffsetDateTime, serde_json::Value, String)> = query_as(
            r#"
            UPDATE session
            SET resource = jsonb_set(resource, '{used}', 'true'),
                txid = txid + 1,
                ts = NOW(),
                status = 'updated'
            WHERE id = $1
              AND status != 'deleted'
            RETURNING id, txid, ts, resource, status
            "#,
        )
        .bind(id)
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
                ts = NOW()
            WHERE id = $1
              AND status != 'deleted'
            "#,
        )
        .bind(id)
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
                ts = NOW()
            WHERE status != 'deleted'
              AND (resource->>'expiresAt')::timestamptz < NOW()
            "#,
        )
        .execute(self.pool)
        .await?;

        Ok(result.rows_affected())
    }
}
