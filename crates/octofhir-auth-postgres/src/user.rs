//! User storage.
//!
//! Stores user accounts for the auth system.
//! Users may be linked to FHIR Practitioner, Patient, or Person resources.

use sqlx_core::query::query;
use sqlx_core::query_as::query_as;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{PgPool, StorageError, StorageResult};

// =============================================================================
// Types
// =============================================================================

/// User record from database.
///
/// Follows the standard FHIR resource table structure.
#[derive(Debug, Clone)]
pub struct UserRow {
    /// Resource UUID
    pub id: Uuid,
    /// Transaction ID (version)
    pub txid: i64,
    /// Timestamp
    pub ts: OffsetDateTime,
    /// Full user resource as JSONB
    pub resource: serde_json::Value,
    /// Resource status (created, updated, deleted)
    pub status: String,
}

impl UserRow {
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
// User Storage
// =============================================================================

/// User storage operations.
///
/// Manages user accounts in PostgreSQL.
/// Uses the standard FHIR resource table pattern in the public schema.
pub struct UserStorage<'a> {
    pool: &'a PgPool,
}

impl<'a> UserStorage<'a> {
    /// Create a new user storage with a connection pool reference.
    #[must_use]
    pub fn new(pool: &'a PgPool) -> Self {
        Self { pool }
    }

    /// Find a user by its ID.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub async fn find_by_id(&self, id: Uuid) -> StorageResult<Option<UserRow>> {
        let row: Option<(Uuid, i64, OffsetDateTime, serde_json::Value, String)> = query_as(
            r#"
            SELECT id, txid, ts, resource, status
            FROM "user"
            WHERE id = $1
              AND status != 'deleted'
            "#,
        )
        .bind(id)
        .fetch_optional(self.pool)
        .await?;

        Ok(row.map(UserRow::from_tuple))
    }

    /// Find a user by username.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub async fn find_by_username(&self, username: &str) -> StorageResult<Option<UserRow>> {
        let row: Option<(Uuid, i64, OffsetDateTime, serde_json::Value, String)> = query_as(
            r#"
            SELECT id, txid, ts, resource, status
            FROM "user"
            WHERE resource->>'username' = $1
              AND status != 'deleted'
            "#,
        )
        .bind(username)
        .fetch_optional(self.pool)
        .await?;

        Ok(row.map(UserRow::from_tuple))
    }

    /// Find a user by email.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub async fn find_by_email(&self, email: &str) -> StorageResult<Option<UserRow>> {
        let row: Option<(Uuid, i64, OffsetDateTime, serde_json::Value, String)> = query_as(
            r#"
            SELECT id, txid, ts, resource, status
            FROM "user"
            WHERE resource->>'email' = $1
              AND status != 'deleted'
            "#,
        )
        .bind(email)
        .fetch_optional(self.pool)
        .await?;

        Ok(row.map(UserRow::from_tuple))
    }

    /// Find a user by FHIR resource reference.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub async fn find_by_fhir_user(&self, fhir_user: &str) -> StorageResult<Option<UserRow>> {
        let row: Option<(Uuid, i64, OffsetDateTime, serde_json::Value, String)> = query_as(
            r#"
            SELECT id, txid, ts, resource, status
            FROM "user"
            WHERE resource->'fhirUser'->>'reference' = $1
              AND status != 'deleted'
            "#,
        )
        .bind(fhir_user)
        .fetch_optional(self.pool)
        .await?;

        Ok(row.map(UserRow::from_tuple))
    }

    /// Find a user by external identity provider link.
    ///
    /// Searches for a user that has a linked identity from the specified
    /// provider with the given external subject identifier.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub async fn find_by_external_identity(
        &self,
        provider_id: &str,
        external_subject: &str,
    ) -> StorageResult<Option<UserRow>> {
        // Build the JSON object to match against the identities array
        let identity_match = serde_json::json!([{
            "provider_id": provider_id,
            "external_subject": external_subject
        }]);

        let row: Option<(Uuid, i64, OffsetDateTime, serde_json::Value, String)> = query_as(
            r#"
            SELECT id, txid, ts, resource, status
            FROM "user"
            WHERE resource->'attributes'->'identities' @> $1::jsonb
              AND status != 'deleted'
            "#,
        )
        .bind(&identity_match)
        .fetch_optional(self.pool)
        .await?;

        Ok(row.map(UserRow::from_tuple))
    }

    /// Create a new user.
    ///
    /// # Errors
    ///
    /// Returns an error if the database insert fails.
    pub async fn create(&self, id: Uuid, resource: serde_json::Value) -> StorageResult<UserRow> {
        let row: (Uuid, i64, OffsetDateTime, serde_json::Value, String) = query_as(
            r#"
            INSERT INTO "user" (id, txid, ts, resource, status)
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
                return StorageError::conflict(format!("User with id '{}' already exists", id));
            }
            StorageError::from(e)
        })?;

        Ok(UserRow::from_tuple(row))
    }

    /// Update an existing user.
    ///
    /// # Errors
    ///
    /// Returns an error if the user doesn't exist or the database update fails.
    pub async fn update(&self, id: Uuid, resource: serde_json::Value) -> StorageResult<UserRow> {
        let row: Option<(Uuid, i64, OffsetDateTime, serde_json::Value, String)> = query_as(
            r#"
            UPDATE "user"
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

        row.map(UserRow::from_tuple)
            .ok_or_else(|| StorageError::not_found(format!("User {}", id)))
    }

    /// Update last login timestamp.
    ///
    /// # Errors
    ///
    /// Returns an error if the user doesn't exist or the database update fails.
    pub async fn update_last_login(&self, id: Uuid) -> StorageResult<UserRow> {
        let row: Option<(Uuid, i64, OffsetDateTime, serde_json::Value, String)> = query_as(
            r#"
            UPDATE "user"
            SET resource = jsonb_set(resource, '{lastLogin}', to_jsonb(NOW()::text)),
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

        row.map(UserRow::from_tuple)
            .ok_or_else(|| StorageError::not_found(format!("User {}", id)))
    }

    /// Delete a user (soft delete).
    ///
    /// # Errors
    ///
    /// Returns an error if the user doesn't exist or the database update fails.
    pub async fn delete(&self, id: Uuid) -> StorageResult<()> {
        let result = query(
            r#"
            UPDATE "user"
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
            return Err(StorageError::not_found(format!("User {}", id)));
        }

        Ok(())
    }

    /// List all active users.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub async fn list(&self, limit: i64, offset: i64) -> StorageResult<Vec<UserRow>> {
        let rows: Vec<(Uuid, i64, OffsetDateTime, serde_json::Value, String)> = query_as(
            r#"
            SELECT id, txid, ts, resource, status
            FROM "user"
            WHERE status != 'deleted'
            ORDER BY ts DESC
            LIMIT $1 OFFSET $2
            "#,
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(self.pool)
        .await?;

        Ok(rows.into_iter().map(UserRow::from_tuple).collect())
    }

    /// Count all active users.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub async fn count(&self) -> StorageResult<i64> {
        let count: (i64,) = query_as(
            r#"
            SELECT COUNT(*)
            FROM "user"
            WHERE status != 'deleted'
            "#,
        )
        .fetch_one(self.pool)
        .await?;

        Ok(count.0)
    }

    /// Count users linked to a specific identity provider.
    ///
    /// This is used to check if an identity provider can be safely deleted.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub async fn count_by_identity_provider(&self, provider_id: &str) -> StorageResult<i64> {
        // Search for users with identities that have a provider reference containing the provider_id
        let provider_ref = format!("IdentityProvider/{}", provider_id);

        let count: (i64,) = query_as(
            r#"
            SELECT COUNT(*)
            FROM "user"
            WHERE status != 'deleted'
              AND resource->'identity' @> $1::jsonb
            "#,
        )
        .bind(serde_json::json!([{ "provider": { "reference": provider_ref } }]))
        .fetch_one(self.pool)
        .await?;

        Ok(count.0)
    }
}
