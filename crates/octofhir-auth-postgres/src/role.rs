//! Role storage.
//!
//! Stores roles for the auth system.
//! Roles group permissions and can be assigned to users.

use sqlx_core::query::query;
use sqlx_core::query_as::query_as;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{PgPool, StorageError, StorageResult};

// =============================================================================
// Types
// =============================================================================

/// Role record from database.
///
/// Follows the standard FHIR resource table structure.
#[derive(Debug, Clone)]
pub struct RoleRow {
    /// Resource ID (TEXT in database, supports both UUIDs and custom IDs)
    pub id: String,
    /// Transaction ID (version)
    pub txid: i64,
    /// Created timestamp (immutable)
    pub created_at: OffsetDateTime,
    /// Last updated timestamp
    pub updated_at: OffsetDateTime,
    /// Full role resource as JSONB
    pub resource: serde_json::Value,
    /// Resource status (created, updated, deleted)
    pub status: String,
}

impl RoleRow {
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
// Role Storage
// =============================================================================

/// Role storage operations.
///
/// Manages roles in PostgreSQL.
/// Uses the standard FHIR resource table pattern in the public schema.
pub struct RoleStorage<'a> {
    pool: &'a PgPool,
}

impl<'a> RoleStorage<'a> {
    /// Create a new role storage with a connection pool reference.
    #[must_use]
    pub fn new(pool: &'a PgPool) -> Self {
        Self { pool }
    }

    /// Find a role by its ID.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub async fn find_by_id(&self, id: Uuid) -> StorageResult<Option<RoleRow>> {
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
            FROM "role"
            WHERE id = $1
              AND status != 'deleted'
            "#,
        )
        .bind(id.to_string())
        .fetch_optional(self.pool)
        .await?;

        Ok(row.map(RoleRow::from_tuple))
    }

    /// Find a role by name.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub async fn find_by_name(&self, name: &str) -> StorageResult<Option<RoleRow>> {
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
            FROM "role"
            WHERE resource->>'name' = $1
              AND status != 'deleted'
            "#,
        )
        .bind(name)
        .fetch_optional(self.pool)
        .await?;

        Ok(row.map(RoleRow::from_tuple))
    }

    /// Create a new role.
    ///
    /// # Errors
    ///
    /// Returns an error if the database insert fails.
    pub async fn create(&self, id: Uuid, resource: serde_json::Value) -> StorageResult<RoleRow> {
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
            INSERT INTO "role" (id, txid, created_at, updated_at, resource, status)
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
                return StorageError::conflict(format!("Role with id '{}' already exists", id));
            }
            StorageError::from(e)
        })?;

        Ok(RoleRow::from_tuple(row))
    }

    /// Update an existing role.
    ///
    /// # Errors
    ///
    /// Returns an error if the role doesn't exist or the database update fails.
    pub async fn update(&self, id: Uuid, resource: serde_json::Value) -> StorageResult<RoleRow> {
        let row: Option<(
            String,
            i64,
            OffsetDateTime,
            OffsetDateTime,
            serde_json::Value,
            String,
        )> = query_as(
            r#"
            UPDATE "role"
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

        row.map(RoleRow::from_tuple)
            .ok_or_else(|| StorageError::not_found(format!("Role {}", id)))
    }

    /// Delete a role (soft delete).
    ///
    /// # Errors
    ///
    /// Returns an error if the role doesn't exist or the database update fails.
    pub async fn delete(&self, id: Uuid) -> StorageResult<()> {
        let result = query(
            r#"
            UPDATE "role"
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
            return Err(StorageError::not_found(format!("Role {}", id)));
        }

        Ok(())
    }

    /// List all active roles.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub async fn list(&self, limit: i64, offset: i64) -> StorageResult<Vec<RoleRow>> {
        let rows: Vec<(
            String,
            i64,
            OffsetDateTime,
            OffsetDateTime,
            serde_json::Value,
            String,
        )> = query_as(
            r#"
            SELECT id, txid, created_at, updated_at, resource, status::text
            FROM "role"
            WHERE status != 'deleted'
            ORDER BY created_at DESC
            LIMIT $1 OFFSET $2
            "#,
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(self.pool)
        .await?;

        Ok(rows.into_iter().map(RoleRow::from_tuple).collect())
    }

    /// Count all active roles.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub async fn count(&self) -> StorageResult<i64> {
        let count: (i64,) = query_as(
            r#"
            SELECT COUNT(*)
            FROM "role"
            WHERE status != 'deleted'
            "#,
        )
        .fetch_one(self.pool)
        .await?;

        Ok(count.0)
    }

    /// Find roles that have a specific permission.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub async fn find_by_permission(&self, permission: &str) -> StorageResult<Vec<RoleRow>> {
        let rows: Vec<(
            String,
            i64,
            OffsetDateTime,
            OffsetDateTime,
            serde_json::Value,
            String,
        )> = query_as(
            r#"
            SELECT id, txid, created_at, updated_at, resource, status::text
            FROM "role"
            WHERE status != 'deleted'
              AND resource->'permissions' ? $1
            ORDER BY created_at DESC
            "#,
        )
        .bind(permission)
        .fetch_all(self.pool)
        .await?;

        Ok(rows.into_iter().map(RoleRow::from_tuple).collect())
    }

    /// Count users assigned to a specific role.
    ///
    /// This is used to check if a role can be safely deleted.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub async fn count_users_with_role(&self, role_name: &str) -> StorageResult<i64> {
        let count: (i64,) = query_as(
            r#"
            SELECT COUNT(*)
            FROM "user"
            WHERE status != 'deleted'
              AND resource->'roles' ? $1
            "#,
        )
        .bind(role_name)
        .fetch_one(self.pool)
        .await?;

        Ok(count.0)
    }
}
