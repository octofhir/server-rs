//! Access policy storage.
//!
//! Stores access policies that define authorization rules.
//! Policies can be attached to clients, users, or roles.

use sqlx_core::query::query;
use sqlx_core::query_as::query_as;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{PgPool, StorageError, StorageResult};

// =============================================================================
// Types
// =============================================================================

/// Policy record from database.
///
/// Follows the standard FHIR resource table structure.
#[derive(Debug, Clone)]
pub struct PolicyRow {
    /// Resource UUID
    pub id: Uuid,
    /// Transaction ID (version)
    pub txid: i64,
    /// Timestamp
    pub ts: OffsetDateTime,
    /// Full policy resource as JSONB
    pub resource: serde_json::Value,
    /// Resource status (created, updated, deleted)
    pub status: String,
}

impl PolicyRow {
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
// Policy Storage
// =============================================================================

/// Policy storage operations.
///
/// Manages access policies in PostgreSQL.
/// Uses the standard FHIR resource table pattern in the public schema.
pub struct PolicyStorage<'a> {
    pool: &'a PgPool,
}

impl<'a> PolicyStorage<'a> {
    /// Create a new policy storage with a connection pool reference.
    #[must_use]
    pub fn new(pool: &'a PgPool) -> Self {
        Self { pool }
    }

    /// Find a policy by its ID.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub async fn find_by_id(&self, id: Uuid) -> StorageResult<Option<PolicyRow>> {
        let row: Option<(Uuid, i64, OffsetDateTime, serde_json::Value, String)> = query_as(
            r#"
            SELECT id, txid, ts, resource, status
            FROM accesspolicy
            WHERE id = $1
              AND status != 'deleted'
            "#,
        )
        .bind(id)
        .fetch_optional(self.pool)
        .await?;

        Ok(row.map(PolicyRow::from_tuple))
    }

    /// Find a policy by name.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub async fn find_by_name(&self, name: &str) -> StorageResult<Option<PolicyRow>> {
        let row: Option<(Uuid, i64, OffsetDateTime, serde_json::Value, String)> = query_as(
            r#"
            SELECT id, txid, ts, resource, status
            FROM accesspolicy
            WHERE resource->>'name' = $1
              AND status != 'deleted'
            "#,
        )
        .bind(name)
        .fetch_optional(self.pool)
        .await?;

        Ok(row.map(PolicyRow::from_tuple))
    }

    /// Create a new policy.
    ///
    /// # Errors
    ///
    /// Returns an error if the database insert fails.
    pub async fn create(&self, id: Uuid, resource: serde_json::Value) -> StorageResult<PolicyRow> {
        let row: (Uuid, i64, OffsetDateTime, serde_json::Value, String) = query_as(
            r#"
            INSERT INTO accesspolicy (id, txid, ts, resource, status)
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
                return StorageError::conflict(format!(
                    "AccessPolicy with id '{}' already exists",
                    id
                ));
            }
            StorageError::from(e)
        })?;

        Ok(PolicyRow::from_tuple(row))
    }

    /// Update an existing policy.
    ///
    /// # Errors
    ///
    /// Returns an error if the policy doesn't exist or the database update fails.
    pub async fn update(&self, id: Uuid, resource: serde_json::Value) -> StorageResult<PolicyRow> {
        let row: Option<(Uuid, i64, OffsetDateTime, serde_json::Value, String)> = query_as(
            r#"
            UPDATE accesspolicy
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

        row.map(PolicyRow::from_tuple)
            .ok_or_else(|| StorageError::not_found(format!("AccessPolicy {}", id)))
    }

    /// Delete a policy (soft delete).
    ///
    /// # Errors
    ///
    /// Returns an error if the policy doesn't exist or the database update fails.
    pub async fn delete(&self, id: Uuid) -> StorageResult<()> {
        let result = query(
            r#"
            UPDATE accesspolicy
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
            return Err(StorageError::not_found(format!("AccessPolicy {}", id)));
        }

        Ok(())
    }

    /// List all active policies.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub async fn list(&self, limit: i64, offset: i64) -> StorageResult<Vec<PolicyRow>> {
        let rows: Vec<(Uuid, i64, OffsetDateTime, serde_json::Value, String)> = query_as(
            r#"
            SELECT id, txid, ts, resource, status
            FROM accesspolicy
            WHERE status != 'deleted'
            ORDER BY (resource->>'priority')::int NULLS LAST, ts DESC
            LIMIT $1 OFFSET $2
            "#,
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(self.pool)
        .await?;

        Ok(rows.into_iter().map(PolicyRow::from_tuple).collect())
    }

    /// Find policies linked to a specific client.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub async fn find_for_client(&self, client_id: &str) -> StorageResult<Vec<PolicyRow>> {
        let rows: Vec<(Uuid, i64, OffsetDateTime, serde_json::Value, String)> = query_as(
            r#"
            SELECT id, txid, ts, resource, status
            FROM accesspolicy
            WHERE status != 'deleted'
              AND (resource->>'active')::boolean = true
              AND EXISTS (
                SELECT 1 FROM jsonb_array_elements(resource->'link') AS link
                WHERE link->'client'->>'reference' LIKE '%' || $1
              )
            ORDER BY (resource->>'priority')::int NULLS LAST
            "#,
        )
        .bind(client_id)
        .fetch_all(self.pool)
        .await?;

        Ok(rows.into_iter().map(PolicyRow::from_tuple).collect())
    }

    /// Find policies linked to a specific user.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub async fn find_for_user(&self, user_id: &str) -> StorageResult<Vec<PolicyRow>> {
        let rows: Vec<(Uuid, i64, OffsetDateTime, serde_json::Value, String)> = query_as(
            r#"
            SELECT id, txid, ts, resource, status
            FROM accesspolicy
            WHERE status != 'deleted'
              AND (resource->>'active')::boolean = true
              AND EXISTS (
                SELECT 1 FROM jsonb_array_elements(resource->'link') AS link
                WHERE link->'user'->>'reference' LIKE '%' || $1
              )
            ORDER BY (resource->>'priority')::int NULLS LAST
            "#,
        )
        .bind(user_id)
        .fetch_all(self.pool)
        .await?;

        Ok(rows.into_iter().map(PolicyRow::from_tuple).collect())
    }

    /// Find policies linked to a specific role.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub async fn find_for_role(&self, role: &str) -> StorageResult<Vec<PolicyRow>> {
        let rows: Vec<(Uuid, i64, OffsetDateTime, serde_json::Value, String)> = query_as(
            r#"
            SELECT id, txid, ts, resource, status
            FROM accesspolicy
            WHERE status != 'deleted'
              AND (resource->>'active')::boolean = true
              AND EXISTS (
                SELECT 1 FROM jsonb_array_elements(resource->'link') AS link
                WHERE link->>'role' = $1
              )
            ORDER BY (resource->>'priority')::int NULLS LAST
            "#,
        )
        .bind(role)
        .fetch_all(self.pool)
        .await?;

        Ok(rows.into_iter().map(PolicyRow::from_tuple).collect())
    }
}
