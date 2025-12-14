//! Access policy storage.
//!
//! Stores access policies that define authorization rules.
//! Policies can be attached to clients, users, or roles.

use std::sync::Arc;

use async_trait::async_trait;
use sqlx_core::query::query;
use sqlx_core::query_as::query_as;
use time::OffsetDateTime;
use uuid::Uuid;

use octofhir_auth::AuthResult;
use octofhir_auth::policy::resources::AccessPolicy;
use octofhir_auth::smart::scopes::FhirOperation;
use octofhir_auth::storage::{PolicySearchParams, PolicyStorage as PolicyStorageTrait};

use crate::{PgPool, StorageError, StorageResult};

// =============================================================================
// Types
// =============================================================================

/// Policy record from database.
///
/// Follows the standard FHIR resource table structure.
#[derive(Debug, Clone)]
pub struct PolicyRow {
    /// Resource ID (TEXT in database, supports both UUIDs and custom IDs)
    pub id: String,
    /// Transaction ID (version)
    pub txid: i64,
    /// Timestamp
    pub ts: OffsetDateTime,
    /// Full policy resource as JSONB
    pub resource: serde_json::Value,
    /// Resource status (created, updated, deleted)
    pub status: String,
}

impl PolicyRow {
    /// Create from database tuple.
    fn from_tuple(row: (String, i64, OffsetDateTime, serde_json::Value, String)) -> Self {
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
// Policy Storage
// =============================================================================

/// Policy storage operations.
///
/// Manages access policies in PostgreSQL.
/// Uses the standard FHIR resource table pattern in the public schema.
pub struct PolicyStorage<'a> {
    pool: &'a PgPool,
}

impl<'a> PolicyStorage<'a> {
    /// Create a new policy storage with a connection pool reference.
    #[must_use]
    pub fn new(pool: &'a PgPool) -> Self {
        Self { pool }
    }

    /// Find a policy by its ID.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub async fn find_by_id(&self, id: Uuid) -> StorageResult<Option<PolicyRow>> {
        let row: Option<(String, i64, OffsetDateTime, serde_json::Value, String)> = query_as(
            r#"
            SELECT id, txid, ts, resource, status::text
            FROM accesspolicy
            WHERE id = $1
              AND status != 'deleted'
            "#,
        )
        .bind(id.to_string())
        .fetch_optional(self.pool)
        .await?;

        Ok(row.map(PolicyRow::from_tuple))
    }

    /// Find a policy by name.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub async fn find_by_name(&self, name: &str) -> StorageResult<Option<PolicyRow>> {
        let row: Option<(String, i64, OffsetDateTime, serde_json::Value, String)> = query_as(
            r#"
            SELECT id, txid, ts, resource, status::text
            FROM accesspolicy
            WHERE resource->>'name' = $1
              AND status != 'deleted'
            "#,
        )
        .bind(name)
        .fetch_optional(self.pool)
        .await?;

        Ok(row.map(PolicyRow::from_tuple))
    }

    /// Create a new policy.
    ///
    /// # Errors
    ///
    /// Returns an error if the database insert fails.
    pub async fn create(&self, id: Uuid, resource: serde_json::Value) -> StorageResult<PolicyRow> {
        let id_str = id.to_string();
        let row: (String, i64, OffsetDateTime, serde_json::Value, String) = query_as(
            r#"
            INSERT INTO accesspolicy (id, txid, ts, resource, status)
            VALUES ($1, 1, NOW(), $2, 'created')
            RETURNING id, txid, ts, resource, status::text
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
                    "AccessPolicy with id '{}' already exists",
                    id
                ));
            }
            StorageError::from(e)
        })?;

        Ok(PolicyRow::from_tuple(row))
    }

    /// Update an existing policy.
    ///
    /// # Errors
    ///
    /// Returns an error if the policy doesn't exist or the database update fails.
    pub async fn update(&self, id: Uuid, resource: serde_json::Value) -> StorageResult<PolicyRow> {
        let row: Option<(String, i64, OffsetDateTime, serde_json::Value, String)> = query_as(
            r#"
            UPDATE accesspolicy
            SET resource = $2,
                txid = txid + 1,
                ts = NOW(),
                status = 'updated'
            WHERE id = $1
              AND status != 'deleted'
            RETURNING id, txid, ts, resource, status::text
            "#,
        )
        .bind(id.to_string())
        .bind(&resource)
        .fetch_optional(self.pool)
        .await?;

        row.map(PolicyRow::from_tuple)
            .ok_or_else(|| StorageError::not_found(format!("AccessPolicy {}", id)))
    }

    /// Delete a policy (soft delete).
    ///
    /// # Errors
    ///
    /// Returns an error if the policy doesn't exist or the database update fails.
    pub async fn delete(&self, id: Uuid) -> StorageResult<()> {
        let result = query(
            r#"
            UPDATE accesspolicy
            SET status = 'deleted',
                txid = txid + 1,
                ts = NOW()
            WHERE id = $1
              AND status != 'deleted'
            "#,
        )
        .bind(id.to_string())
        .execute(self.pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(StorageError::not_found(format!("AccessPolicy {}", id)));
        }

        Ok(())
    }

    /// List all active policies.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub async fn list(&self, limit: i64, offset: i64) -> StorageResult<Vec<PolicyRow>> {
        let rows: Vec<(String, i64, OffsetDateTime, serde_json::Value, String)> = query_as(
            r#"
            SELECT id, txid, ts, resource, status::text
            FROM accesspolicy
            WHERE status != 'deleted'
            ORDER BY (resource->>'priority')::int NULLS LAST, ts DESC
            LIMIT $1 OFFSET $2
            "#,
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(self.pool)
        .await?;

        Ok(rows.into_iter().map(PolicyRow::from_tuple).collect())
    }

    /// Find policies linked to a specific client.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub async fn find_for_client(&self, client_id: &str) -> StorageResult<Vec<PolicyRow>> {
        let rows: Vec<(String, i64, OffsetDateTime, serde_json::Value, String)> = query_as(
            r#"
            SELECT id, txid, ts, resource, status::text
            FROM accesspolicy
            WHERE status != 'deleted'
              AND (resource->>'active')::boolean = true
              AND EXISTS (
                SELECT 1 FROM jsonb_array_elements(resource->'link') AS link
                WHERE link->'client'->>'reference' LIKE '%' || $1
              )
            ORDER BY (resource->>'priority')::int NULLS LAST
            "#,
        )
        .bind(client_id)
        .fetch_all(self.pool)
        .await?;

        Ok(rows.into_iter().map(PolicyRow::from_tuple).collect())
    }

    /// Find policies linked to a specific user.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub async fn find_for_user(&self, user_id: &str) -> StorageResult<Vec<PolicyRow>> {
        let rows: Vec<(String, i64, OffsetDateTime, serde_json::Value, String)> = query_as(
            r#"
            SELECT id, txid, ts, resource, status::text
            FROM accesspolicy
            WHERE status != 'deleted'
              AND (resource->>'active')::boolean = true
              AND EXISTS (
                SELECT 1 FROM jsonb_array_elements(resource->'link') AS link
                WHERE link->'user'->>'reference' LIKE '%' || $1
              )
            ORDER BY (resource->>'priority')::int NULLS LAST
            "#,
        )
        .bind(user_id)
        .fetch_all(self.pool)
        .await?;

        Ok(rows.into_iter().map(PolicyRow::from_tuple).collect())
    }

    /// Find policies linked to a specific role.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub async fn find_for_role(&self, role: &str) -> StorageResult<Vec<PolicyRow>> {
        let rows: Vec<(String, i64, OffsetDateTime, serde_json::Value, String)> = query_as(
            r#"
            SELECT id, txid, ts, resource, status::text
            FROM accesspolicy
            WHERE status != 'deleted'
              AND (resource->>'active')::boolean = true
              AND EXISTS (
                SELECT 1 FROM jsonb_array_elements(resource->'link') AS link
                WHERE link->>'role' = $1
              )
            ORDER BY (resource->>'priority')::int NULLS LAST
            "#,
        )
        .bind(role)
        .fetch_all(self.pool)
        .await?;

        Ok(rows.into_iter().map(PolicyRow::from_tuple).collect())
    }
}

// =============================================================================
// PostgreSQL Policy Storage Adapter
// =============================================================================

/// PostgreSQL implementation of the [`PolicyStorageTrait`].
///
/// This adapter wraps the low-level [`PolicyStorage`] and implements the
/// trait from `octofhir-auth`, enabling integration with the policy cache
/// and evaluation engine.
#[derive(Debug, Clone)]
pub struct PostgresPolicyStorageAdapter {
    pool: Arc<PgPool>,
}

impl PostgresPolicyStorageAdapter {
    /// Create a new adapter with the given connection pool.
    #[must_use]
    pub fn new(pool: Arc<PgPool>) -> Self {
        Self { pool }
    }

    /// Get a reference to the connection pool.
    #[must_use]
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Convert a PolicyRow to AccessPolicy.
    fn row_to_policy(row: PolicyRow) -> AuthResult<AccessPolicy> {
        serde_json::from_value(row.resource).map_err(|e| {
            octofhir_auth::AuthError::internal(format!("Failed to deserialize policy: {}", e))
        })
    }

    /// Convert StorageError to AuthError.
    fn map_storage_error(e: StorageError) -> octofhir_auth::AuthError {
        match e {
            StorageError::NotFound(msg) => octofhir_auth::AuthError::storage(msg),
            StorageError::Conflict(msg) => octofhir_auth::AuthError::storage(msg),
            StorageError::InvalidInput(msg) => octofhir_auth::AuthError::invalid_request(msg),
            StorageError::Database(e) => {
                octofhir_auth::AuthError::storage(format!("Database error: {}", e))
            }
            StorageError::Serialization(e) => {
                octofhir_auth::AuthError::storage(format!("Serialization error: {}", e))
            }
        }
    }
}

#[async_trait]
impl PolicyStorageTrait for PostgresPolicyStorageAdapter {
    async fn get(&self, id: &str) -> AuthResult<Option<AccessPolicy>> {
        let uuid = Uuid::parse_str(id).map_err(|_| {
            octofhir_auth::AuthError::invalid_request(format!("Invalid policy ID: {}", id))
        })?;

        let storage = PolicyStorage::new(&self.pool);
        match storage.find_by_id(uuid).await {
            Ok(Some(row)) => Ok(Some(Self::row_to_policy(row)?)),
            Ok(None) => Ok(None),
            Err(e) => Err(Self::map_storage_error(e)),
        }
    }

    async fn list_active(&self) -> AuthResult<Vec<AccessPolicy>> {
        let rows: Vec<(String, i64, OffsetDateTime, serde_json::Value, String)> = query_as(
            r#"
            SELECT id, txid, ts, resource, status::text
            FROM accesspolicy
            WHERE status != 'deleted'
              AND (resource->>'active')::boolean = true
            ORDER BY (resource->>'priority')::int NULLS LAST, ts DESC
            "#,
        )
        .fetch_all(self.pool.as_ref())
        .await
        .map_err(|e| octofhir_auth::AuthError::internal(format!("Database error: {}", e)))?;

        rows.into_iter()
            .map(PolicyRow::from_tuple)
            .map(Self::row_to_policy)
            .collect()
    }

    async fn list_all(&self) -> AuthResult<Vec<AccessPolicy>> {
        let storage = PolicyStorage::new(&self.pool);
        let rows = storage
            .list(1000, 0)
            .await
            .map_err(Self::map_storage_error)?;

        rows.into_iter().map(Self::row_to_policy).collect()
    }

    async fn create(&self, policy: &AccessPolicy) -> AuthResult<AccessPolicy> {
        let id = policy
            .id
            .as_ref()
            .and_then(|s| Uuid::parse_str(s).ok())
            .unwrap_or_else(Uuid::new_v4);

        let resource = serde_json::to_value(policy).map_err(|e| {
            octofhir_auth::AuthError::internal(format!("Failed to serialize policy: {}", e))
        })?;

        let storage = PolicyStorage::new(&self.pool);
        let row = storage
            .create(id, resource)
            .await
            .map_err(Self::map_storage_error)?;

        Self::row_to_policy(row)
    }

    async fn update(&self, id: &str, policy: &AccessPolicy) -> AuthResult<AccessPolicy> {
        let uuid = Uuid::parse_str(id).map_err(|_| {
            octofhir_auth::AuthError::invalid_request(format!("Invalid policy ID: {}", id))
        })?;

        let resource = serde_json::to_value(policy).map_err(|e| {
            octofhir_auth::AuthError::internal(format!("Failed to serialize policy: {}", e))
        })?;

        let storage = PolicyStorage::new(&self.pool);
        let row = storage
            .update(uuid, resource)
            .await
            .map_err(Self::map_storage_error)?;

        Self::row_to_policy(row)
    }

    async fn delete(&self, id: &str) -> AuthResult<()> {
        let uuid = Uuid::parse_str(id).map_err(|_| {
            octofhir_auth::AuthError::invalid_request(format!("Invalid policy ID: {}", id))
        })?;

        let storage = PolicyStorage::new(&self.pool);
        storage.delete(uuid).await.map_err(Self::map_storage_error)
    }

    async fn find_applicable(
        &self,
        resource_type: &str,
        _operation: FhirOperation,
    ) -> AuthResult<Vec<AccessPolicy>> {
        // Find policies that match the resource type or have no resource type filter
        let rows: Vec<(String, i64, OffsetDateTime, serde_json::Value, String)> = query_as(
            r#"
            SELECT id, txid, ts, resource, status::text
            FROM accesspolicy
            WHERE status != 'deleted'
              AND (resource->>'active')::boolean = true
              AND (
                resource->'matcher'->'resourceTypes' IS NULL
                OR resource->'matcher'->'resourceTypes' = '[]'::jsonb
                OR resource->'matcher'->'resourceTypes' @> to_jsonb($1::text)
              )
            ORDER BY (resource->>'priority')::int NULLS LAST
            "#,
        )
        .bind(resource_type)
        .fetch_all(self.pool.as_ref())
        .await
        .map_err(|e| octofhir_auth::AuthError::internal(format!("Database error: {}", e)))?;

        rows.into_iter()
            .map(PolicyRow::from_tuple)
            .map(Self::row_to_policy)
            .collect()
    }

    async fn get_by_ids(&self, ids: &[String]) -> AuthResult<Vec<AccessPolicy>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }

        let uuids: Vec<Uuid> = ids
            .iter()
            .filter_map(|id| Uuid::parse_str(id).ok())
            .collect();

        if uuids.is_empty() {
            return Ok(Vec::new());
        }

        // Convert UUIDs to strings for the query
        let uuid_strs: Vec<String> = uuids.iter().map(|u| u.to_string()).collect();

        let rows: Vec<(String, i64, OffsetDateTime, serde_json::Value, String)> = query_as(
            r#"
            SELECT id, txid, ts, resource, status::text
            FROM accesspolicy
            WHERE status != 'deleted'
              AND id = ANY($1)
            ORDER BY (resource->>'priority')::int NULLS LAST
            "#,
        )
        .bind(&uuid_strs)
        .fetch_all(self.pool.as_ref())
        .await
        .map_err(|e| octofhir_auth::AuthError::internal(format!("Database error: {}", e)))?;

        rows.into_iter()
            .map(PolicyRow::from_tuple)
            .map(Self::row_to_policy)
            .collect()
    }

    async fn search(&self, params: &PolicySearchParams) -> AuthResult<Vec<AccessPolicy>> {
        // Build dynamic query based on search parameters
        let limit = params.count.unwrap_or(100) as i64;
        let offset = params.offset.unwrap_or(0) as i64;

        let rows: Vec<(String, i64, OffsetDateTime, serde_json::Value, String)> =
            if let Some(ref name) = params.name {
                query_as(
                    r#"
                SELECT id, txid, ts, resource, status::text
                FROM accesspolicy
                WHERE status != 'deleted'
                  AND resource->>'name' ILIKE '%' || $1 || '%'
                ORDER BY (resource->>'priority')::int NULLS LAST
                LIMIT $2 OFFSET $3
                "#,
                )
                .bind(name)
                .bind(limit)
                .bind(offset)
                .fetch_all(self.pool.as_ref())
                .await
                .map_err(|e| octofhir_auth::AuthError::internal(format!("Database error: {}", e)))?
            } else {
                query_as(
                    r#"
                SELECT id, txid, ts, resource, status::text
                FROM accesspolicy
                WHERE status != 'deleted'
                ORDER BY (resource->>'priority')::int NULLS LAST
                LIMIT $1 OFFSET $2
                "#,
                )
                .bind(limit)
                .bind(offset)
                .fetch_all(self.pool.as_ref())
                .await
                .map_err(|e| octofhir_auth::AuthError::internal(format!("Database error: {}", e)))?
            };

        rows.into_iter()
            .map(PolicyRow::from_tuple)
            .map(Self::row_to_policy)
            .collect()
    }

    async fn find_for_client(&self, client_id: &str) -> AuthResult<Vec<AccessPolicy>> {
        let storage = PolicyStorage::new(&self.pool);
        let rows = storage
            .find_for_client(client_id)
            .await
            .map_err(Self::map_storage_error)?;

        rows.into_iter().map(Self::row_to_policy).collect()
    }

    async fn find_for_user(&self, user_id: &str) -> AuthResult<Vec<AccessPolicy>> {
        let storage = PolicyStorage::new(&self.pool);
        let rows = storage
            .find_for_user(user_id)
            .await
            .map_err(Self::map_storage_error)?;

        rows.into_iter().map(Self::row_to_policy).collect()
    }

    async fn find_for_role(&self, role: &str) -> AuthResult<Vec<AccessPolicy>> {
        let storage = PolicyStorage::new(&self.pool);
        let rows = storage
            .find_for_role(role)
            .await
            .map_err(Self::map_storage_error)?;

        rows.into_iter().map(Self::row_to_policy).collect()
    }

    async fn upsert(&self, policy: &AccessPolicy) -> AuthResult<AccessPolicy> {
        let id = policy
            .id
            .as_ref()
            .and_then(|s| Uuid::parse_str(s).ok())
            .unwrap_or_else(Uuid::new_v4);

        let resource = serde_json::to_value(policy).map_err(|e| {
            octofhir_auth::AuthError::internal(format!("Failed to serialize policy: {}", e))
        })?;

        // Use INSERT ... ON CONFLICT DO UPDATE for upsert
        let id_str = id.to_string();
        let row: (String, i64, OffsetDateTime, serde_json::Value, String) = query_as(
            r#"
            INSERT INTO accesspolicy (id, txid, ts, resource, status)
            VALUES ($1, 1, NOW(), $2, 'created')
            ON CONFLICT (id) DO UPDATE SET
                resource = EXCLUDED.resource,
                txid = accesspolicy.txid + 1,
                ts = NOW(),
                status = 'updated'
            RETURNING id, txid, ts, resource, status::text
            "#,
        )
        .bind(&id_str)
        .bind(&resource)
        .fetch_one(self.pool.as_ref())
        .await
        .map_err(|e| octofhir_auth::AuthError::internal(format!("Database error: {}", e)))?;

        Self::row_to_policy(PolicyRow::from_tuple(row))
    }
}
