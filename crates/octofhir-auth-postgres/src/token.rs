//! Refresh token storage.
//!
//! Stores OAuth refresh tokens for token refresh flow.
//! Tokens are hashed before storage for security.

use sqlx_core::query::query;
use sqlx_core::query_as::query_as;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{PgPool, StorageError, StorageResult};

// =============================================================================
// Types
// =============================================================================

/// Refresh token record from database.
///
/// Follows the standard FHIR resource table structure.
#[derive(Debug, Clone)]
pub struct TokenRow {
    /// Resource ID (TEXT in database, supports both UUIDs and custom IDs)
    pub id: String,
    /// Transaction ID (version)
    pub txid: i64,
    /// Created timestamp
    pub created_at: OffsetDateTime,
    /// Updated timestamp
    pub updated_at: OffsetDateTime,
    /// Full token resource as JSONB
    pub resource: serde_json::Value,
    /// Resource status (created, updated, deleted)
    pub status: String,
}

impl TokenRow {
    /// Create from database tuple.
    fn from_tuple(row: (String, i64, OffsetDateTime, OffsetDateTime, serde_json::Value, String)) -> Self {
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
// Token Storage
// =============================================================================

/// Refresh token storage operations.
///
/// Manages OAuth refresh tokens in PostgreSQL.
/// Uses the standard FHIR resource table pattern in the public schema.
pub struct TokenStorage<'a> {
    pool: &'a PgPool,
}

impl<'a> TokenStorage<'a> {
    /// Create a new token storage with a connection pool reference.
    #[must_use]
    pub fn new(pool: &'a PgPool) -> Self {
        Self { pool }
    }

    /// Find a token by its hash.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub async fn find_by_token_hash(&self, token_hash: &str) -> StorageResult<Option<TokenRow>> {
        let row: Option<(String, i64, OffsetDateTime, OffsetDateTime, serde_json::Value, String)> = query_as(
            r#"
            SELECT id, txid, created_at, updated_at, resource, status::text
            FROM refreshtoken
            WHERE resource->>'tokenHash' = $1
              AND status != 'deleted'
            "#,
        )
        .bind(token_hash)
        .fetch_optional(self.pool)
        .await?;

        Ok(row.map(TokenRow::from_tuple))
    }

    /// Find a token by its ID.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub async fn find_by_id(&self, id: Uuid) -> StorageResult<Option<TokenRow>> {
        let row: Option<(String, i64, OffsetDateTime, OffsetDateTime, serde_json::Value, String)> = query_as(
            r#"
            SELECT id, txid, created_at, updated_at, resource, status::text
            FROM refreshtoken
            WHERE id = $1
              AND status != 'deleted'
            "#,
        )
        .bind(id.to_string())
        .fetch_optional(self.pool)
        .await?;

        Ok(row.map(TokenRow::from_tuple))
    }

    /// Create a new refresh token.
    ///
    /// # Errors
    ///
    /// Returns an error if the database insert fails.
    pub async fn create(&self, id: Uuid, resource: serde_json::Value) -> StorageResult<TokenRow> {
        let id_str = id.to_string();
        let row: (String, i64, OffsetDateTime, OffsetDateTime, serde_json::Value, String) = query_as(
            r#"
            INSERT INTO refreshtoken (id, txid, created_at, updated_at, resource, status)
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
                return StorageError::conflict(format!(
                    "RefreshToken with id '{}' already exists",
                    id
                ));
            }
            StorageError::from(e)
        })?;

        Ok(TokenRow::from_tuple(row))
    }

    /// Update the last used timestamp.
    ///
    /// # Errors
    ///
    /// Returns an error if the token doesn't exist or the database update fails.
    pub async fn update_last_used(&self, id: Uuid) -> StorageResult<TokenRow> {
        let row: Option<(String, i64, OffsetDateTime, OffsetDateTime, serde_json::Value, String)> = query_as(
            r#"
            UPDATE refreshtoken
            SET resource = jsonb_set(resource, '{lastUsedAt}', to_jsonb(NOW()::text)),
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

        row.map(TokenRow::from_tuple)
            .ok_or_else(|| StorageError::not_found(format!("RefreshToken {}", id)))
    }

    /// Revoke a refresh token.
    ///
    /// # Errors
    ///
    /// Returns an error if the token doesn't exist or the database update fails.
    pub async fn revoke(&self, id: Uuid) -> StorageResult<TokenRow> {
        let row: Option<(String, i64, OffsetDateTime, OffsetDateTime, serde_json::Value, String)> = query_as(
            r#"
            UPDATE refreshtoken
            SET resource = resource || jsonb_build_object('revoked', true, 'revokedAt', NOW()::text),
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

        row.map(TokenRow::from_tuple)
            .ok_or_else(|| StorageError::not_found(format!("RefreshToken {}", id)))
    }

    /// Delete a token (soft delete).
    ///
    /// # Errors
    ///
    /// Returns an error if the token doesn't exist or the database update fails.
    pub async fn delete(&self, id: Uuid) -> StorageResult<()> {
        let result = query(
            r#"
            UPDATE refreshtoken
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
            return Err(StorageError::not_found(format!("RefreshToken {}", id)));
        }

        Ok(())
    }

    /// Delete expired tokens.
    ///
    /// # Errors
    ///
    /// Returns an error if the database update fails.
    pub async fn delete_expired(&self) -> StorageResult<u64> {
        let result = query(
            r#"
            UPDATE refreshtoken
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

    /// Revoke all tokens for a user.
    ///
    /// # Errors
    ///
    /// Returns an error if the database update fails.
    pub async fn revoke_all_for_user(&self, user_id: &str) -> StorageResult<u64> {
        let result = query(
            r#"
            UPDATE refreshtoken
            SET resource = resource || jsonb_build_object('revoked', true, 'revokedAt', NOW()::text),
                txid = txid + 1,
                updated_at = NOW(),
                status = 'updated'
            WHERE resource->>'userId' = $1
              AND status != 'deleted'
              AND (resource->>'revoked')::boolean IS NOT TRUE
            "#,
        )
        .bind(user_id)
        .execute(self.pool)
        .await?;

        Ok(result.rows_affected())
    }

    /// Find a token by its hash (alias for `find_by_token_hash`).
    pub async fn find_by_hash(&self, token_hash: &str) -> StorageResult<Option<TokenRow>> {
        self.find_by_token_hash(token_hash).await
    }

    /// Revoke a token by its hash.
    pub async fn revoke_by_hash(&self, token_hash: &str) -> StorageResult<()> {
        let result = query(
            r#"
            UPDATE refreshtoken
            SET resource = resource || jsonb_build_object('revoked', true, 'revokedAt', NOW()::text),
                txid = txid + 1,
                updated_at = NOW(),
                status = 'updated'
            WHERE resource->>'tokenHash' = $1
              AND status != 'deleted'
              AND (resource->>'revoked')::boolean IS NOT TRUE
            "#,
        )
        .bind(token_hash)
        .execute(self.pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(StorageError::not_found("RefreshToken with given hash"));
        }

        Ok(())
    }

    /// Revoke all tokens for a client.
    pub async fn revoke_by_client(&self, client_id: &str) -> StorageResult<u64> {
        let result = query(
            r#"
            UPDATE refreshtoken
            SET resource = resource || jsonb_build_object('revoked', true, 'revokedAt', NOW()::text),
                txid = txid + 1,
                updated_at = NOW(),
                status = 'updated'
            WHERE resource->>'clientId' = $1
              AND status != 'deleted'
              AND (resource->>'revoked')::boolean IS NOT TRUE
            "#,
        )
        .bind(client_id)
        .execute(self.pool)
        .await?;

        Ok(result.rows_affected())
    }

    /// Revoke all tokens for a user by UUID.
    pub async fn revoke_by_user(&self, user_id: Uuid) -> StorageResult<u64> {
        let result = query(
            r#"
            UPDATE refreshtoken
            SET resource = resource || jsonb_build_object('revoked', true, 'revokedAt', NOW()::text),
                txid = txid + 1,
                updated_at = NOW(),
                status = 'updated'
            WHERE resource->>'userId' = $1
              AND status != 'deleted'
              AND (resource->>'revoked')::boolean IS NOT TRUE
            "#,
        )
        .bind(user_id.to_string())
        .execute(self.pool)
        .await?;

        Ok(result.rows_affected())
    }

    /// Cleanup expired tokens (alias for `delete_expired`).
    pub async fn cleanup_expired(&self) -> StorageResult<u64> {
        self.delete_expired().await
    }

    /// List all active (non-revoked, non-expired) tokens for a user.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub async fn list_by_user(&self, user_id: Uuid) -> StorageResult<Vec<TokenRow>> {
        let rows: Vec<(String, i64, OffsetDateTime, OffsetDateTime, serde_json::Value, String)> = query_as(
            r#"
            SELECT id, txid, created_at, updated_at, resource, status::text
            FROM refreshtoken
            WHERE resource->>'userId' = $1
              AND status != 'deleted'
              AND (resource->>'revoked')::boolean IS NOT TRUE
              AND (resource->>'expiresAt')::timestamptz > NOW()
            ORDER BY created_at DESC
            "#,
        )
        .bind(user_id.to_string())
        .fetch_all(self.pool)
        .await?;

        Ok(rows.into_iter().map(TokenRow::from_tuple).collect())
    }
}
