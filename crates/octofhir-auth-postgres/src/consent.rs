//! PostgreSQL storage for user consent records.
//!
//! Stores persistent user consent records for OAuth clients.
//! Records are stored in `octofhir_auth.user_consents` table.

use sqlx_core::query::query;
use sqlx_core::query_as::query_as;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{PgPool, StorageResult};

// =============================================================================
// Consent Storage
// =============================================================================

/// Consent storage operations.
///
/// Manages user consent records for OAuth clients.
/// Uses a dedicated table in the octofhir_auth schema.
pub struct ConsentStorage<'a> {
    pool: &'a PgPool,
}

impl<'a> ConsentStorage<'a> {
    /// Create a new consent storage with a connection pool reference.
    #[must_use]
    pub fn new(pool: &'a PgPool) -> Self {
        Self { pool }
    }

    /// Check if user has granted consent for client with all requested scopes.
    ///
    /// Returns true only if the user has an existing consent record that
    /// includes ALL the requested scopes.
    pub async fn has_consent(
        &self,
        user_id: &str,
        client_id: &str,
        scopes: &[&str],
    ) -> StorageResult<bool> {
        // Query to check if all requested scopes are contained in stored scopes
        let result: Option<(bool,)> = query_as(
            r#"
            SELECT scopes @> $3::text[]
            FROM octofhir_auth.user_consents
            WHERE user_id = $1 AND client_id = $2
            "#,
        )
        .bind(user_id)
        .bind(client_id)
        .bind(scopes)
        .fetch_optional(self.pool)
        .await?;

        Ok(result.map(|r| r.0).unwrap_or(false))
    }

    /// Save or update consent (upsert).
    ///
    /// If consent already exists for this user+client, the scopes are updated.
    pub async fn save_consent(
        &self,
        user_id: &str,
        client_id: &str,
        scopes: &[String],
    ) -> StorageResult<()> {
        query(
            r#"
            INSERT INTO octofhir_auth.user_consents
                (user_id, client_id, scopes)
            VALUES ($1, $2, $3)
            ON CONFLICT (user_id, client_id)
            DO UPDATE SET
                scopes = $3,
                updated_at = NOW()
            "#,
        )
        .bind(user_id)
        .bind(client_id)
        .bind(scopes)
        .execute(self.pool)
        .await?;

        Ok(())
    }

    /// Revoke consent for a client.
    pub async fn revoke_consent(&self, user_id: &str, client_id: &str) -> StorageResult<()> {
        query(
            r#"
            DELETE FROM octofhir_auth.user_consents
            WHERE user_id = $1 AND client_id = $2
            "#,
        )
        .bind(user_id)
        .bind(client_id)
        .execute(self.pool)
        .await?;

        Ok(())
    }

    /// List all consents for a user.
    pub async fn list_consents(&self, user_id: &str) -> StorageResult<Vec<ConsentRow>> {
        let rows: Vec<(
            Uuid,
            String,
            String,
            Vec<String>,
            OffsetDateTime,
            OffsetDateTime,
        )> = query_as(
            r#"
            SELECT id, user_id, client_id, scopes, created_at, updated_at
            FROM octofhir_auth.user_consents
            WHERE user_id = $1
            ORDER BY updated_at DESC
            "#,
        )
        .bind(user_id)
        .fetch_all(self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| ConsentRow {
                id: r.0,
                user_id: r.1,
                client_id: r.2,
                scopes: r.3,
                created_at: r.4,
                updated_at: r.5,
            })
            .collect())
    }
}

/// Row from user_consents table.
#[derive(Debug, Clone)]
pub struct ConsentRow {
    /// Consent record ID.
    pub id: Uuid,
    /// User ID who granted consent.
    pub user_id: String,
    /// Client ID that received consent.
    pub client_id: String,
    /// Granted scopes.
    pub scopes: Vec<String>,
    /// When the consent was first granted.
    pub created_at: OffsetDateTime,
    /// When the consent was last updated.
    pub updated_at: OffsetDateTime,
}
