//! IdentityProvider storage.
//!
//! Provides CRUD operations for external identity provider configurations.
//! Providers are stored as FHIR resources in the `identityprovider` table.

use sqlx_core::query::query;
use sqlx_core::query_as::query_as;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{PgPool, StorageError, StorageResult};

// =============================================================================
// Types
// =============================================================================

/// IdentityProvider record from database.
///
/// Follows the standard FHIR resource table structure.
#[derive(Debug, Clone)]
pub struct IdentityProviderRow {
    /// Resource ID (TEXT in database, supports both UUIDs and custom IDs)
    pub id: String,
    /// Transaction ID (version).
    pub txid: i64,
    /// Created timestamp.
    pub created_at: OffsetDateTime,
    /// Updated timestamp.
    pub updated_at: OffsetDateTime,
    /// Full resource as JSONB.
    pub resource: serde_json::Value,
    /// Resource status (created, updated, deleted).
    pub status: String,
}

impl IdentityProviderRow {
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

    /// Returns the provider name from the resource.
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        self.resource.get("name").and_then(|v| v.as_str())
    }

    /// Returns the issuer URL from the resource.
    #[must_use]
    pub fn issuer(&self) -> Option<&str> {
        self.resource.get("issuer").and_then(|v| v.as_str())
    }

    /// Returns whether the provider is active.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.resource
            .get("active")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    }
}

// =============================================================================
// IdentityProvider Storage
// =============================================================================

/// IdentityProvider storage operations.
///
/// Provides methods for managing external identity provider configurations
/// in PostgreSQL. Uses the standard FHIR resource table pattern.
pub struct IdentityProviderStorage<'a> {
    pool: &'a PgPool,
}

impl<'a> IdentityProviderStorage<'a> {
    /// Create a new identity provider storage with a connection pool reference.
    #[must_use]
    pub fn new(pool: &'a PgPool) -> Self {
        Self { pool }
    }

    /// Find a provider by its resource ID.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub async fn find_by_id(&self, id: Uuid) -> StorageResult<Option<IdentityProviderRow>> {
        let row: Option<(String, i64, OffsetDateTime, OffsetDateTime, serde_json::Value, String)> = query_as(
            r#"
            SELECT id, txid, created_at, updated_at, resource, status::text
            FROM identityprovider
            WHERE id = $1
              AND status != 'deleted'
            "#,
        )
        .bind(id.to_string())
        .fetch_optional(self.pool)
        .await?;

        Ok(row.map(IdentityProviderRow::from_tuple))
    }

    /// Find a provider by its name.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub async fn find_by_name(&self, name: &str) -> StorageResult<Option<IdentityProviderRow>> {
        let row: Option<(String, i64, OffsetDateTime, OffsetDateTime, serde_json::Value, String)> = query_as(
            r#"
            SELECT id, txid, created_at, updated_at, resource, status::text
            FROM identityprovider
            WHERE resource->>'name' = $1
              AND status != 'deleted'
            "#,
        )
        .bind(name)
        .fetch_optional(self.pool)
        .await?;

        Ok(row.map(IdentityProviderRow::from_tuple))
    }

    /// Find a provider by its issuer URL.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub async fn find_by_issuer(&self, issuer: &str) -> StorageResult<Option<IdentityProviderRow>> {
        let row: Option<(String, i64, OffsetDateTime, OffsetDateTime, serde_json::Value, String)> = query_as(
            r#"
            SELECT id, txid, created_at, updated_at, resource, status::text
            FROM identityprovider
            WHERE resource->>'issuer' = $1
              AND status != 'deleted'
            "#,
        )
        .bind(issuer)
        .fetch_optional(self.pool)
        .await?;

        Ok(row.map(IdentityProviderRow::from_tuple))
    }

    /// List all active identity providers.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub async fn list_active(&self) -> StorageResult<Vec<IdentityProviderRow>> {
        let rows: Vec<(String, i64, OffsetDateTime, OffsetDateTime, serde_json::Value, String)> = query_as(
            r#"
            SELECT id, txid, created_at, updated_at, resource, status::text
            FROM identityprovider
            WHERE resource->>'active' = 'true'
              AND status != 'deleted'
            ORDER BY resource->>'name'
            "#,
        )
        .fetch_all(self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(IdentityProviderRow::from_tuple)
            .collect())
    }

    /// List all identity providers with pagination.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub async fn list_all(
        &self,
        limit: i64,
        offset: i64,
    ) -> StorageResult<Vec<IdentityProviderRow>> {
        let rows: Vec<(String, i64, OffsetDateTime, OffsetDateTime, serde_json::Value, String)> = query_as(
            r#"
            SELECT id, txid, created_at, updated_at, resource, status::text
            FROM identityprovider
            WHERE status != 'deleted'
            ORDER BY resource->>'name'
            LIMIT $1 OFFSET $2
            "#,
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(IdentityProviderRow::from_tuple)
            .collect())
    }

    /// Create a new identity provider.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - A provider with the same ID already exists
    /// - The database insert fails
    pub async fn create(
        &self,
        id: Uuid,
        resource: serde_json::Value,
    ) -> StorageResult<IdentityProviderRow> {
        let id_str = id.to_string();
        let row: (String, i64, OffsetDateTime, OffsetDateTime, serde_json::Value, String) = query_as(
            r#"
            INSERT INTO identityprovider (id, txid, created_at, updated_at, resource, status)
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
                    "IdentityProvider with id '{}' already exists",
                    id
                ));
            }
            StorageError::from(e)
        })?;

        Ok(IdentityProviderRow::from_tuple(row))
    }

    /// Update an existing identity provider.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The provider doesn't exist
    /// - The database update fails
    pub async fn update(
        &self,
        id: Uuid,
        resource: serde_json::Value,
    ) -> StorageResult<IdentityProviderRow> {
        let row: Option<(String, i64, OffsetDateTime, OffsetDateTime, serde_json::Value, String)> = query_as(
            r#"
            UPDATE identityprovider
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

        row.map(IdentityProviderRow::from_tuple)
            .ok_or_else(|| StorageError::not_found(format!("IdentityProvider {}", id)))
    }

    /// Delete an identity provider (soft delete).
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The provider doesn't exist
    /// - The database update fails
    pub async fn delete(&self, id: Uuid) -> StorageResult<()> {
        let result = query(
            r#"
            UPDATE identityprovider
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
            return Err(StorageError::not_found(format!("IdentityProvider {}", id)));
        }

        Ok(())
    }

    /// Count all active identity providers.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub async fn count_active(&self) -> StorageResult<i64> {
        let count: (i64,) = query_as(
            r#"
            SELECT COUNT(*)
            FROM identityprovider
            WHERE resource->>'active' = 'true'
              AND status != 'deleted'
            "#,
        )
        .fetch_one(self.pool)
        .await?;

        Ok(count.0)
    }

    /// Count all identity providers.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub async fn count(&self) -> StorageResult<i64> {
        let count: (i64,) = query_as(
            r#"
            SELECT COUNT(*)
            FROM identityprovider
            WHERE status != 'deleted'
            "#,
        )
        .fetch_one(self.pool)
        .await?;

        Ok(count.0)
    }
}
