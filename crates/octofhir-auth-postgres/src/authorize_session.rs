//! PostgreSQL storage for OAuth authorize flow sessions.
//!
//! Stores temporary sessions during the login/consent UI flow.
//! Sessions are stored in `octofhir_auth.authorize_sessions` table.

use sqlx_core::query::query;
use sqlx_core::query_as::query_as;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{PgPool, StorageResult};

// =============================================================================
// Authorize Session Storage
// =============================================================================

/// Authorize session storage operations.
///
/// Manages authorize sessions during the OAuth login/consent UI flow.
/// Uses a dedicated table in the octofhir_auth schema.
pub struct AuthorizeSessionStorage<'a> {
    pool: &'a PgPool,
}

impl<'a> AuthorizeSessionStorage<'a> {
    /// Create a new authorize session storage with a connection pool reference.
    #[must_use]
    pub fn new(pool: &'a PgPool) -> Self {
        Self { pool }
    }

    /// Store a new authorize session.
    pub async fn create(
        &self,
        id: Uuid,
        authorization_request: serde_json::Value,
        expires_at: OffsetDateTime,
    ) -> StorageResult<()> {
        query(
            r#"
            INSERT INTO octofhir_auth.authorize_sessions
                (id, authorization_request, expires_at)
            VALUES ($1, $2, $3)
            "#,
        )
        .bind(id)
        .bind(&authorization_request)
        .bind(expires_at)
        .execute(self.pool)
        .await?;

        Ok(())
    }

    /// Find a session by ID.
    ///
    /// Returns None if not found or expired.
    pub async fn find_by_id(
        &self,
        id: Uuid,
    ) -> StorageResult<Option<AuthorizeSessionRow>> {
        let row: Option<(
            Uuid,
            Option<String>,
            serde_json::Value,
            OffsetDateTime,
            OffsetDateTime,
        )> = query_as(
            r#"
            SELECT id, user_id, authorization_request, created_at, expires_at
            FROM octofhir_auth.authorize_sessions
            WHERE id = $1 AND expires_at > NOW()
            "#,
        )
        .bind(id)
        .fetch_optional(self.pool)
        .await?;

        Ok(row.map(|r| AuthorizeSessionRow {
            id: r.0,
            user_id: r.1,
            authorization_request: r.2,
            created_at: r.3,
            expires_at: r.4,
        }))
    }

    /// Update session with user ID after authentication.
    pub async fn update_user(&self, id: Uuid, user_id: &str) -> StorageResult<()> {
        let rows_affected = query(
            r#"
            UPDATE octofhir_auth.authorize_sessions
            SET user_id = $2
            WHERE id = $1 AND expires_at > NOW()
            "#,
        )
        .bind(id)
        .bind(user_id)
        .execute(self.pool)
        .await?
        .rows_affected();

        if rows_affected == 0 {
            return Err(crate::StorageError::not_found("Authorize session"));
        }

        Ok(())
    }

    /// Delete a session by ID.
    pub async fn delete(&self, id: Uuid) -> StorageResult<()> {
        query(
            r#"
            DELETE FROM octofhir_auth.authorize_sessions
            WHERE id = $1
            "#,
        )
        .bind(id)
        .execute(self.pool)
        .await?;

        Ok(())
    }

    /// Delete expired sessions.
    ///
    /// Returns the number of sessions deleted.
    pub async fn cleanup_expired(&self) -> StorageResult<u64> {
        let result = query(
            r#"
            DELETE FROM octofhir_auth.authorize_sessions
            WHERE expires_at <= NOW()
            "#,
        )
        .execute(self.pool)
        .await?;

        Ok(result.rows_affected())
    }
}

/// Row from authorize_sessions table.
#[derive(Debug, Clone)]
pub struct AuthorizeSessionRow {
    /// Session ID.
    pub id: Uuid,
    /// User ID (set after authentication).
    pub user_id: Option<String>,
    /// Authorization request parameters (JSONB).
    pub authorization_request: serde_json::Value,
    /// When the session was created.
    pub created_at: OffsetDateTime,
    /// When the session expires.
    pub expires_at: OffsetDateTime,
}
