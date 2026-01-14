//! App storage for authentication.
//!
//! Provides operations for retrieving App records with secrets for authentication.
//! Apps are stored as FHIR resources in the `app` table.

use sqlx_core::query_as::query_as;
use time::OffsetDateTime;

use octofhir_auth::AuthResult;
use octofhir_auth::types::AppRecord;

use crate::{PgPool, StorageError, StorageResult};

// =============================================================================
// Types
// =============================================================================

/// App record from database.
///
/// Follows the standard FHIR resource table structure.
#[derive(Debug, Clone)]
pub struct AppRow {
    /// Resource ID
    pub id: String,
    /// Transaction ID (version)
    pub txid: i64,
    /// Created timestamp
    pub created_at: OffsetDateTime,
    /// Updated timestamp
    pub updated_at: OffsetDateTime,
    /// Full app resource as JSONB
    pub resource: serde_json::Value,
    /// Resource status (created, updated, deleted)
    pub status: String,
}

impl AppRow {
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

    /// Convert to AppRecord.
    ///
    /// Extracts fields from the JSONB resource.
    pub fn to_app_record(&self) -> AuthResult<AppRecord> {
        let name = self
            .resource
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or(&self.id)
            .to_string();

        let version = self
            .resource
            .get("apiVersion")
            .and_then(|v| v.as_u64())
            .map(|v| v.to_string());

        let active = self
            .resource
            .get("status")
            .and_then(|v| v.as_str())
            .map(|s| s == "active")
            .unwrap_or(true); // Default to active if not specified

        Ok(AppRecord {
            id: self.id.clone(),
            name,
            version,
            active,
        })
    }

    /// Extract secret from the JSONB resource.
    pub fn secret(&self) -> Option<String> {
        self.resource
            .get("secret")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    }
}

// =============================================================================
// App Storage
// =============================================================================

/// App storage operations.
///
/// Provides methods for retrieving Apps from PostgreSQL for authentication.
/// Uses the standard FHIR resource table pattern.
pub struct AppStorage<'a> {
    pool: &'a PgPool,
}

impl<'a> AppStorage<'a> {
    /// Create a new app storage with a connection pool reference.
    #[must_use]
    pub fn new(pool: &'a PgPool) -> Self {
        Self { pool }
    }

    /// Find an app by its ID with secret.
    ///
    /// Returns `None` if the app doesn't exist or is deleted.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub async fn find_by_id_with_secret(
        &self,
        app_id: &str,
    ) -> StorageResult<Option<(AppRecord, Option<String>)>> {
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
            FROM app
            WHERE id = $1
              AND status != 'deleted'
            "#,
        )
        .bind(app_id)
        .fetch_optional(self.pool)
        .await?;

        match row {
            Some(tuple) => {
                let app_row = AppRow::from_tuple(tuple);
                let app_record = app_row.to_app_record().map_err(|e| {
                    StorageError::Internal(format!("Failed to parse app record: {}", e))
                })?;
                let secret = app_row.secret();
                Ok(Some((app_record, secret)))
            }
            None => Ok(None),
        }
    }
}
