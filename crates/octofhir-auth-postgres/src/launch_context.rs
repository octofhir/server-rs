//! SMART launch context storage for PostgreSQL.
//!
//! Stores temporary launch contexts during the EHR launch flow.
//! Contexts are stored with TTL and consumed during token exchange.
//!
//! # Table Structure
//!
//! ```sql
//! CREATE TABLE octofhir_auth.smart_launch_context (
//!     launch_id VARCHAR(64) PRIMARY KEY,
//!     context_data JSONB NOT NULL,
//!     expires_at TIMESTAMPTZ NOT NULL,
//!     created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
//! );
//!
//! CREATE INDEX idx_smart_launch_context_expires
//!     ON octofhir_auth.smart_launch_context (expires_at);
//! ```

use octofhir_auth::smart::launch::StoredLaunchContext;
use sqlx_core::query::query;
use sqlx_core::query_as::query_as;
use sqlx_core::query_scalar::query_scalar;
use time::OffsetDateTime;

use crate::{PgPool, StorageResult};

// =============================================================================
// Launch Context Storage
// =============================================================================

/// SMART launch context storage operations.
///
/// Manages launch contexts for SMART on FHIR EHR launches.
/// Uses a dedicated table in the octofhir_auth schema with TTL support.
pub struct LaunchContextStorage<'a> {
    pool: &'a PgPool,
}

impl<'a> LaunchContextStorage<'a> {
    /// Create a new launch context storage with a connection pool reference.
    #[must_use]
    pub fn new(pool: &'a PgPool) -> Self {
        Self { pool }
    }

    /// Store a launch context with TTL.
    ///
    /// The context is keyed by `launch_id` and will expire after `ttl_seconds`.
    /// If a context with the same launch_id already exists, it will be replaced.
    ///
    /// # Arguments
    ///
    /// * `context` - The launch context to store
    /// * `ttl_seconds` - Time-to-live in seconds
    ///
    /// # Errors
    ///
    /// Returns an error if the database insert fails.
    pub async fn store(
        &self,
        context: &StoredLaunchContext,
        ttl_seconds: u64,
    ) -> StorageResult<()> {
        let context_data = serde_json::to_value(context)?;

        query(
            r#"
            INSERT INTO octofhir_auth.smart_launch_context
                (launch_id, context_data, expires_at, created_at)
            VALUES ($1, $2, NOW() + make_interval(secs => $3), NOW())
            ON CONFLICT (launch_id) DO UPDATE SET
                context_data = EXCLUDED.context_data,
                expires_at = EXCLUDED.expires_at,
                created_at = EXCLUDED.created_at
            "#,
        )
        .bind(&context.launch_id)
        .bind(&context_data)
        .bind(ttl_seconds as f64)
        .execute(self.pool)
        .await?;

        Ok(())
    }

    /// Retrieve a launch context by launch ID without consuming it.
    ///
    /// Returns `None` if the context doesn't exist or has expired.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails or deserialization fails.
    pub async fn get(&self, launch_id: &str) -> StorageResult<Option<StoredLaunchContext>> {
        let row: Option<(serde_json::Value,)> = query_as(
            r#"
            SELECT context_data
            FROM octofhir_auth.smart_launch_context
            WHERE launch_id = $1
              AND expires_at > NOW()
            "#,
        )
        .bind(launch_id)
        .fetch_optional(self.pool)
        .await?;

        match row {
            Some((context_data,)) => {
                let context: StoredLaunchContext = serde_json::from_value(context_data)?;
                Ok(Some(context))
            }
            None => Ok(None),
        }
    }

    /// Atomically retrieve and delete a launch context.
    ///
    /// This ensures single-use semantics for launch contexts.
    /// If two concurrent requests try to consume the same context,
    /// only one will succeed.
    ///
    /// Returns `None` if the context doesn't exist, has expired,
    /// or was already consumed.
    ///
    /// # Errors
    ///
    /// Returns an error if the database operation fails or deserialization fails.
    pub async fn consume(&self, launch_id: &str) -> StorageResult<Option<StoredLaunchContext>> {
        let row: Option<(serde_json::Value,)> = query_as(
            r#"
            DELETE FROM octofhir_auth.smart_launch_context
            WHERE launch_id = $1
              AND expires_at > NOW()
            RETURNING context_data
            "#,
        )
        .bind(launch_id)
        .fetch_optional(self.pool)
        .await?;

        match row {
            Some((context_data,)) => {
                let context: StoredLaunchContext = serde_json::from_value(context_data)?;
                Ok(Some(context))
            }
            None => Ok(None),
        }
    }

    /// Delete a launch context by launch ID.
    ///
    /// This is a no-op if the context doesn't exist.
    ///
    /// # Errors
    ///
    /// Returns an error if the database delete fails.
    pub async fn delete(&self, launch_id: &str) -> StorageResult<()> {
        query(
            r#"
            DELETE FROM octofhir_auth.smart_launch_context
            WHERE launch_id = $1
            "#,
        )
        .bind(launch_id)
        .execute(self.pool)
        .await?;

        Ok(())
    }

    /// Delete expired launch context entries.
    ///
    /// Should be called periodically to clean up old entries.
    ///
    /// # Returns
    ///
    /// Returns the number of entries deleted.
    ///
    /// # Errors
    ///
    /// Returns an error if the cleanup operation fails.
    pub async fn cleanup_expired(&self) -> StorageResult<u64> {
        let result = query(
            r#"
            DELETE FROM octofhir_auth.smart_launch_context
            WHERE expires_at < NOW()
            "#,
        )
        .execute(self.pool)
        .await?;

        Ok(result.rows_affected())
    }

    /// Check if a launch context exists and is not expired.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub async fn exists(&self, launch_id: &str) -> StorageResult<bool> {
        let exists: bool = query_scalar(
            r#"
            SELECT EXISTS(
                SELECT 1 FROM octofhir_auth.smart_launch_context
                WHERE launch_id = $1
                  AND expires_at > NOW()
            )
            "#,
        )
        .bind(launch_id)
        .fetch_one(self.pool)
        .await?;

        Ok(exists)
    }

    /// Get the count of active (non-expired) launch contexts.
    ///
    /// Useful for monitoring and debugging.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub async fn count_active(&self) -> StorageResult<i64> {
        let count: i64 = query_scalar(
            r#"
            SELECT COUNT(*)
            FROM octofhir_auth.smart_launch_context
            WHERE expires_at > NOW()
            "#,
        )
        .fetch_one(self.pool)
        .await?;

        Ok(count)
    }

    /// Get expiration time for a launch context.
    ///
    /// Returns `None` if the context doesn't exist.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub async fn get_expires_at(&self, launch_id: &str) -> StorageResult<Option<OffsetDateTime>> {
        let row: Option<(OffsetDateTime,)> = query_as(
            r#"
            SELECT expires_at
            FROM octofhir_auth.smart_launch_context
            WHERE launch_id = $1
            "#,
        )
        .bind(launch_id)
        .fetch_optional(self.pool)
        .await?;

        Ok(row.map(|(expires_at,)| expires_at))
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
