//! Revoked access token storage for PostgreSQL.
//!
//! Tracks revoked access token JTIs to enable token revocation validation.
//! JTIs are stored with their expiration time for cleanup.

use sqlx_core::query::query;
use sqlx_core::query_scalar::query_scalar;
use time::OffsetDateTime;

use crate::{PgPool, StorageResult};

// =============================================================================
// Revoked Token Storage
// =============================================================================

/// Revoked access token JTI storage operations.
///
/// Manages revoked access token JTIs in PostgreSQL.
/// Uses a dedicated table in the octofhir_auth schema.
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
        query(
            r#"
            INSERT INTO octofhir_auth.revoked_token (jti, expires_at, revoked_at)
            VALUES ($1, $2, NOW())
            ON CONFLICT (jti) DO NOTHING
            "#,
        )
        .bind(jti)
        .bind(expires_at)
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
                SELECT 1 FROM octofhir_auth.revoked_token WHERE jti = $1
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
            DELETE FROM octofhir_auth.revoked_token
            WHERE expires_at < NOW()
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
        let count: i64 = query_scalar("SELECT COUNT(*) FROM octofhir_auth.revoked_token")
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
    use super::*;

    #[test]
    fn test_storage_creation() {
        // This is a compile-time test to ensure the storage can be created
        // Actual database tests would require a test database connection
    }
}
