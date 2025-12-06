//! Revoked access token storage for PostgreSQL.
//!
//! Tracks revoked access token JTIs to enable token revocation validation.
//! JTIs are stored with their expiration time for cleanup.
//!
//! Uses the standard FHIR resource table pattern with JSONB storage.
//! The `revokedtoken` table is auto-created from the RevokedToken StructureDefinition.

use sqlx_core::query::query;
use sqlx_core::query_scalar::query_scalar;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{PgPool, StorageResult};

// =============================================================================
// Revoked Token Storage
// =============================================================================

/// Revoked access token JTI storage operations.
///
/// Manages revoked access token JTIs in PostgreSQL.
/// Uses the standard FHIR resource table `revokedtoken` in the public schema.
pub struct RevokedTokenStorage<'a> {
    pool: &'a PgPool,
}

impl<'a> RevokedTokenStorage<'a> {
    /// Create a new revoked token storage with a connection pool reference.
    #[must_use]
    pub fn new(pool: &'a PgPool) -> Self {
        Self { pool }
    }

    /// Mark a JTI as revoked.
    ///
    /// The `expires_at` parameter is used for cleanup - once the token would have
    /// naturally expired, the revocation record can be deleted.
    ///
    /// This operation is idempotent - revoking an already-revoked JTI succeeds.
    ///
    /// # Errors
    ///
    /// Returns an error if the database insert fails.
    pub async fn revoke(&self, jti: &str, expires_at: OffsetDateTime) -> StorageResult<()> {
        let id = Uuid::new_v4();
        let now = OffsetDateTime::now_utc();
        let resource = serde_json::json!({
            "resourceType": "RevokedToken",
            "id": id.to_string(),
            "jti": jti,
            "expiresAt": expires_at.format(&time::format_description::well_known::Rfc3339).unwrap_or_default(),
            "revokedAt": now.format(&time::format_description::well_known::Rfc3339).unwrap_or_default()
        });

        query(
            r#"
            INSERT INTO revokedtoken (id, txid, ts, resource, status)
            SELECT $1, txid, NOW(), $2, 'created'
            FROM _transaction
            ORDER BY txid DESC
            LIMIT 1
            ON CONFLICT (id) DO NOTHING
            "#,
        )
        .bind(id)
        .bind(&resource)
        .execute(self.pool)
        .await?;

        Ok(())
    }

    /// Check if a JTI has been revoked.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub async fn is_revoked(&self, jti: &str) -> StorageResult<bool> {
        let exists: bool = query_scalar(
            r#"
            SELECT EXISTS(
                SELECT 1 FROM revokedtoken WHERE resource->>'jti' = $1
            )
            "#,
        )
        .bind(jti)
        .fetch_one(self.pool)
        .await?;

        Ok(exists)
    }

    /// Delete expired revocation records.
    ///
    /// Removes records where `expires_at` is in the past. These tokens would have
    /// expired anyway, so tracking their revocation is no longer necessary.
    ///
    /// # Returns
    ///
    /// Returns the number of records deleted.
    ///
    /// # Errors
    ///
    /// Returns an error if the database delete fails.
    pub async fn cleanup_expired(&self) -> StorageResult<u64> {
        let result = query(
            r#"
            DELETE FROM revokedtoken
            WHERE (resource->>'expiresAt')::timestamptz < NOW()
            "#,
        )
        .execute(self.pool)
        .await?;

        Ok(result.rows_affected())
    }

    /// Get the count of revoked tokens.
    ///
    /// Useful for monitoring and debugging.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub async fn count(&self) -> StorageResult<i64> {
        let count: i64 = query_scalar("SELECT COUNT(*) FROM revokedtoken")
            .fetch_one(self.pool)
            .await?;

        Ok(count)
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    #[test]
    fn test_storage_creation() {
        // This is a compile-time test to ensure the storage can be created
        // Actual database tests would require a test database connection
    }
}
