use sqlx_core::query::query;
use sqlx_core::query_scalar::query_scalar;
use sqlx_postgres::PgPool;
use time::OffsetDateTime;
use tracing::{info, instrument};
use uuid::Uuid;

/// Lightweight index for fast cookie → AuthSession resource lookup.
/// This avoids expensive JSONB queries on the main resources table.
///
/// The index is maintained via database triggers or application-level hooks
/// that fire when AuthSession resources are created, updated, or deleted.
pub struct SessionTokenIndex<'a> {
    pool: &'a PgPool,
}

impl<'a> SessionTokenIndex<'a> {
    pub fn new(pool: &'a PgPool) -> Self {
        Self { pool }
    }

    /// Find AuthSession resource ID by session token (for cookie validation).
    /// Returns None if token doesn't exist or session is expired.
    #[instrument(skip(self))]
    pub async fn find_by_token(
        &self,
        token: &str,
    ) -> Result<Option<String>, sqlx_core::error::Error> {
        let result = query_scalar::<_, String>(
            r#"
            SELECT resource_id
            FROM auth_session_tokens
            WHERE session_token = $1 AND expires_at > NOW()
            "#,
        )
        .bind(token)
        .fetch_optional(self.pool)
        .await?;

        Ok(result)
    }

    /// Insert new session token index entry.
    /// Called when AuthSession resource is created.
    #[instrument(skip(self))]
    pub async fn insert(
        &self,
        resource_id: &str,
        session_token: &str,
        user_id: Uuid,
        expires_at: OffsetDateTime,
    ) -> Result<(), sqlx_core::error::Error> {
        query(
            r#"
            INSERT INTO auth_session_tokens (resource_id, session_token, user_id, expires_at)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (session_token) DO UPDATE SET
                resource_id = EXCLUDED.resource_id,
                user_id = EXCLUDED.user_id,
                expires_at = EXCLUDED.expires_at
            "#,
        )
        .bind(resource_id)
        .bind(session_token)
        .bind(user_id)
        .bind(expires_at)
        .execute(self.pool)
        .await?;

        info!(
            resource_id = %resource_id,
            session_token_prefix = &session_token[..8.min(session_token.len())],
            "Inserted session token index"
        );

        Ok(())
    }

    /// Update session expiry in index.
    /// Called when AuthSession resource is updated (lastActivityAt change).
    #[instrument(skip(self))]
    pub async fn update_expiry(
        &self,
        resource_id: &str,
        expires_at: OffsetDateTime,
    ) -> Result<(), sqlx_core::error::Error> {
        let rows_affected = query(
            r#"
            UPDATE auth_session_tokens
            SET expires_at = $1
            WHERE resource_id = $2
            "#,
        )
        .bind(expires_at)
        .bind(resource_id)
        .execute(self.pool)
        .await?
        .rows_affected();

        if rows_affected > 0 {
            info!(
                resource_id = %resource_id,
                "Updated session token expiry"
            );
        }

        Ok(())
    }

    /// Delete session token index entry.
    /// Called when AuthSession resource is deleted or revoked.
    #[instrument(skip(self))]
    pub async fn delete(&self, resource_id: &str) -> Result<(), sqlx_core::error::Error> {
        let rows_affected = query(
            r#"
            DELETE FROM auth_session_tokens
            WHERE resource_id = $1
            "#,
        )
        .bind(resource_id)
        .execute(self.pool)
        .await?
        .rows_affected();

        if rows_affected > 0 {
            info!(
                resource_id = %resource_id,
                "Deleted session token index"
            );
        }

        Ok(())
    }

    /// Delete all session tokens for a user.
    /// Used for "logout all devices" functionality.
    #[instrument(skip(self))]
    pub async fn delete_all_for_user(&self, user_id: Uuid) -> Result<u64, sqlx_core::error::Error> {
        let rows_affected = query(
            r#"
            DELETE FROM auth_session_tokens
            WHERE user_id = $1
            "#,
        )
        .bind(user_id)
        .execute(self.pool)
        .await?
        .rows_affected();

        if rows_affected > 0 {
            info!(
                user_id = %user_id,
                count = rows_affected,
                "Deleted all session tokens for user"
            );
        }

        Ok(rows_affected)
    }

    /// Delete expired session tokens (cleanup).
    /// Should be called periodically by a background job.
    #[instrument(skip(self))]
    pub async fn cleanup_expired(&self) -> Result<u64, sqlx_core::error::Error> {
        let rows_affected = query(
            r#"
            DELETE FROM auth_session_tokens
            WHERE expires_at <= NOW()
            "#,
        )
        .execute(self.pool)
        .await?
        .rows_affected();

        if rows_affected > 0 {
            info!(count = rows_affected, "Cleaned up expired session tokens");
        }

        Ok(rows_affected)
    }

    /// Count active sessions for a user.
    /// Used to enforce concurrent session limits.
    #[instrument(skip(self))]
    pub async fn count_active_for_user(
        &self,
        user_id: Uuid,
    ) -> Result<i64, sqlx_core::error::Error> {
        let count = query_scalar::<_, i64>(
            r#"
            SELECT COUNT(*)
            FROM auth_session_tokens
            WHERE user_id = $1 AND expires_at > NOW()
            "#,
        )
        .bind(user_id)
        .fetch_one(self.pool)
        .await?;

        Ok(count)
    }

    /// Create the session token index table.
    /// Should be called during server bootstrap.
    #[instrument(skip(self))]
    pub async fn create_table_if_not_exists(&self) -> Result<(), sqlx_core::error::Error> {
        query(
            r#"
            CREATE TABLE IF NOT EXISTS auth_session_tokens (
                resource_id TEXT PRIMARY KEY,
                session_token TEXT UNIQUE NOT NULL,
                user_id UUID NOT NULL,
                expires_at TIMESTAMPTZ NOT NULL,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )
            "#,
        )
        .execute(self.pool)
        .await?;

        // Create index on session_token for fast lookups
        query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_auth_session_tokens_token
            ON auth_session_tokens(session_token)
            "#,
        )
        .execute(self.pool)
        .await?;

        // Create index on user_id for "list my sessions" and "logout all"
        query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_auth_session_tokens_user_id
            ON auth_session_tokens(user_id)
            "#,
        )
        .execute(self.pool)
        .await?;

        // Create index on expires_at for cleanup
        query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_auth_session_tokens_expires_at
            ON auth_session_tokens(expires_at)
            "#,
        )
        .execute(self.pool)
        .await?;

        info!("Session token index table created");

        Ok(())
    }
}

// TODO: Re-add integration tests using testcontainers (previously used non-existent sqlx_postgres::test macro)
