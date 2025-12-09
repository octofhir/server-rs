//! PostgreSQL implementation of ConformanceStorage for internal OctoFHIR resources.
//!
//! This module provides storage for conformance resources (StructureDefinitions,
//! ValueSets, CodeSystems, SearchParameters) in the public schema using standard
//! FHIR resource tables.

use async_trait::async_trait;
use serde_json::Value;
use sqlx_core::query::query;
use sqlx_core::query_as::query_as;
use sqlx_postgres::PgPool;
use tracing::{debug, info, instrument};
use uuid::Uuid;

use octofhir_storage::{ConformanceStorage, StorageError};

use crate::error::PostgresError;
use crate::schema::SchemaManager;

/// PostgreSQL implementation of the ConformanceStorage trait.
///
/// This implementation stores conformance resources in the public schema
/// using standard FHIR resource tables with CRUD support and change notifications
/// for hot-reload.
#[derive(Debug, Clone)]
pub struct PostgresConformanceStorage {
    pool: PgPool,
}

impl PostgresConformanceStorage {
    /// Creates a new PostgresConformanceStorage with the given connection pool.
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Returns a reference to the connection pool.
    #[must_use]
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Creates a new transaction ID for conformance operations.
    async fn create_txid(&self) -> Result<i64, StorageError> {
        let row: (i64,) =
            query_as("INSERT INTO _transaction (status) VALUES ('committed') RETURNING txid")
                .fetch_one(&self.pool)
                .await
                .map_err(PostgresError::from)?;

        Ok(row.0)
    }

    /// Ensures the table exists for a resource type.
    async fn ensure_table(&self, resource_type: &str) -> Result<(), StorageError> {
        let schema_manager = SchemaManager::new(self.pool.clone());
        schema_manager
            .ensure_table(resource_type)
            .await
            .map_err(|e| StorageError::internal(format!("Failed to ensure table: {}", e)))
    }
}

#[async_trait]
impl ConformanceStorage for PostgresConformanceStorage {
    // ==================== StructureDefinition ====================

    #[instrument(skip(self))]
    async fn list_structure_definitions(&self) -> Result<Vec<Value>, StorageError> {
        // Ensure table exists
        self.ensure_table("StructureDefinition").await?;

        let rows: Vec<(Value,)> = query_as(
            "SELECT resource FROM structuredefinition WHERE status != 'deleted' ORDER BY resource->>'name'",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(PostgresError::from)?;

        Ok(rows.into_iter().map(|(r,)| r).collect())
    }

    #[instrument(skip(self))]
    async fn get_structure_definition_by_url(
        &self,
        url: &str,
        version: Option<&str>,
    ) -> Result<Option<Value>, StorageError> {
        self.ensure_table("StructureDefinition").await?;

        let row: Option<(Value,)> = if let Some(ver) = version {
            query_as(
                "SELECT resource FROM structuredefinition WHERE resource->>'url' = $1 AND resource->>'version' = $2 AND status != 'deleted'",
            )
            .bind(url)
            .bind(ver)
            .fetch_optional(&self.pool)
            .await
            .map_err(PostgresError::from)?
        } else {
            query_as(
                "SELECT resource FROM structuredefinition WHERE resource->>'url' = $1 AND status != 'deleted' ORDER BY resource->>'version' DESC NULLS LAST LIMIT 1",
            )
            .bind(url)
            .fetch_optional(&self.pool)
            .await
            .map_err(PostgresError::from)?
        };

        Ok(row.map(|(r,)| r))
    }

    #[instrument(skip(self))]
    async fn get_structure_definition(&self, id: &str) -> Result<Option<Value>, StorageError> {
        self.ensure_table("StructureDefinition").await?;

        let uuid = Uuid::parse_str(id)
            .map_err(|_| StorageError::invalid_resource("Invalid UUID format"))?;

        let row: Option<(Value,)> = query_as(
            "SELECT resource FROM structuredefinition WHERE id = $1 AND status != 'deleted'",
        )
        .bind(uuid)
        .fetch_optional(&self.pool)
        .await
        .map_err(PostgresError::from)?;

        Ok(row.map(|(r,)| r))
    }

    #[instrument(skip(self, resource))]
    async fn create_structure_definition(&self, resource: &Value) -> Result<Value, StorageError> {
        self.ensure_table("StructureDefinition").await?;

        let txid = self.create_txid().await?;
        let id = Uuid::new_v4();

        // Add id to resource
        let mut result = resource.clone();
        result["id"] = Value::String(id.to_string());

        let row: (Value,) = query_as(
            r#"
            INSERT INTO structuredefinition (id, txid, resource, status)
            VALUES ($1, $2, $3, 'created')
            RETURNING resource
            "#,
        )
        .bind(id)
        .bind(txid)
        .bind(&result)
        .fetch_one(&self.pool)
        .await
        .map_err(PostgresError::from)?;

        // If this is a resource or logical type, create the corresponding table in public schema
        let kind = resource.get("kind").and_then(Value::as_str);
        let type_field = resource.get("type").and_then(Value::as_str);

        if let (Some(kind), Some(resource_type)) = (kind, type_field) {
            if kind == "resource" || kind == "logical" {
                let schema_manager = SchemaManager::new(self.pool.clone());
                match schema_manager.ensure_table(resource_type).await {
                    Ok(()) => {
                        info!(
                            resource_type = resource_type,
                            kind = kind,
                            "Created table for StructureDefinition"
                        );
                    }
                    Err(e) => {
                        // Log but don't fail - table creation is best-effort during bootstrap
                        tracing::warn!(
                            error = %e,
                            resource_type = resource_type,
                            kind = kind,
                            "Failed to create table for StructureDefinition"
                        );
                    }
                }
            }
        }

        let name = resource.get("name").and_then(Value::as_str).unwrap_or("?");
        let url = resource.get("url").and_then(Value::as_str).unwrap_or("?");
        debug!("Created StructureDefinition: {} ({})", name, url);
        Ok(row.0)
    }

    #[instrument(skip(self, resource))]
    async fn update_structure_definition(
        &self,
        id: &str,
        resource: &Value,
    ) -> Result<Value, StorageError> {
        self.ensure_table("StructureDefinition").await?;

        let uuid = Uuid::parse_str(id)
            .map_err(|_| StorageError::invalid_resource("Invalid UUID format"))?;

        let txid = self.create_txid().await?;

        // Ensure id is in resource
        let mut result = resource.clone();
        result["id"] = Value::String(id.to_string());

        let row: Option<(Value,)> = query_as(
            r#"
            UPDATE structuredefinition
            SET txid = $2, ts = NOW(), resource = $3, status = 'updated'
            WHERE id = $1 AND status != 'deleted'
            RETURNING resource
            "#,
        )
        .bind(uuid)
        .bind(txid)
        .bind(&result)
        .fetch_optional(&self.pool)
        .await
        .map_err(PostgresError::from)?;

        row.map(|(r,)| r)
            .ok_or_else(|| StorageError::not_found("StructureDefinition", id))
    }

    #[instrument(skip(self))]
    async fn delete_structure_definition(&self, id: &str) -> Result<(), StorageError> {
        self.ensure_table("StructureDefinition").await?;

        let uuid = Uuid::parse_str(id)
            .map_err(|_| StorageError::invalid_resource("Invalid UUID format"))?;

        let txid = self.create_txid().await?;

        // Soft delete by updating status
        let result = query(
            "UPDATE structuredefinition SET status = 'deleted', txid = $2, ts = NOW() WHERE id = $1 AND status != 'deleted'",
        )
        .bind(uuid)
        .bind(txid)
        .execute(&self.pool)
        .await
        .map_err(PostgresError::from)?;

        if result.rows_affected() == 0 {
            return Err(StorageError::not_found("StructureDefinition", id));
        }

        debug!("Deleted StructureDefinition: {}", id);
        Ok(())
    }

    // ==================== ValueSet ====================

    #[instrument(skip(self))]
    async fn list_value_sets(&self) -> Result<Vec<Value>, StorageError> {
        self.ensure_table("ValueSet").await?;

        let rows: Vec<(Value,)> = query_as(
            "SELECT resource FROM valueset WHERE status != 'deleted' ORDER BY resource->>'name'",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(PostgresError::from)?;

        Ok(rows.into_iter().map(|(r,)| r).collect())
    }

    #[instrument(skip(self))]
    async fn get_value_set_by_url(
        &self,
        url: &str,
        version: Option<&str>,
    ) -> Result<Option<Value>, StorageError> {
        self.ensure_table("ValueSet").await?;

        let row: Option<(Value,)> = if let Some(ver) = version {
            query_as("SELECT resource FROM valueset WHERE resource->>'url' = $1 AND resource->>'version' = $2 AND status != 'deleted'")
                .bind(url)
                .bind(ver)
                .fetch_optional(&self.pool)
                .await
                .map_err(PostgresError::from)?
        } else {
            query_as(
                "SELECT resource FROM valueset WHERE resource->>'url' = $1 AND status != 'deleted' ORDER BY resource->>'version' DESC NULLS LAST LIMIT 1",
            )
            .bind(url)
            .fetch_optional(&self.pool)
            .await
            .map_err(PostgresError::from)?
        };

        Ok(row.map(|(r,)| r))
    }

    #[instrument(skip(self))]
    async fn get_value_set(&self, id: &str) -> Result<Option<Value>, StorageError> {
        self.ensure_table("ValueSet").await?;

        let uuid = Uuid::parse_str(id)
            .map_err(|_| StorageError::invalid_resource("Invalid UUID format"))?;

        let row: Option<(Value,)> =
            query_as("SELECT resource FROM valueset WHERE id = $1 AND status != 'deleted'")
                .bind(uuid)
                .fetch_optional(&self.pool)
                .await
                .map_err(PostgresError::from)?;

        Ok(row.map(|(r,)| r))
    }

    #[instrument(skip(self, resource))]
    async fn create_value_set(&self, resource: &Value) -> Result<Value, StorageError> {
        self.ensure_table("ValueSet").await?;

        let txid = self.create_txid().await?;
        let id = Uuid::new_v4();

        let mut result = resource.clone();
        result["id"] = Value::String(id.to_string());

        let row: (Value,) = query_as(
            r#"
            INSERT INTO valueset (id, txid, resource, status)
            VALUES ($1, $2, $3, 'created')
            RETURNING resource
            "#,
        )
        .bind(id)
        .bind(txid)
        .bind(&result)
        .fetch_one(&self.pool)
        .await
        .map_err(PostgresError::from)?;

        let name = resource.get("name").and_then(Value::as_str).unwrap_or("?");
        let url = resource.get("url").and_then(Value::as_str).unwrap_or("?");
        debug!("Created ValueSet: {} ({})", name, url);
        Ok(row.0)
    }

    #[instrument(skip(self, resource))]
    async fn update_value_set(&self, id: &str, resource: &Value) -> Result<Value, StorageError> {
        self.ensure_table("ValueSet").await?;

        let uuid = Uuid::parse_str(id)
            .map_err(|_| StorageError::invalid_resource("Invalid UUID format"))?;

        let txid = self.create_txid().await?;

        let mut result = resource.clone();
        result["id"] = Value::String(id.to_string());

        let row: Option<(Value,)> = query_as(
            r#"
            UPDATE valueset
            SET txid = $2, ts = NOW(), resource = $3, status = 'updated'
            WHERE id = $1 AND status != 'deleted'
            RETURNING resource
            "#,
        )
        .bind(uuid)
        .bind(txid)
        .bind(&result)
        .fetch_optional(&self.pool)
        .await
        .map_err(PostgresError::from)?;

        row.map(|(r,)| r)
            .ok_or_else(|| StorageError::not_found("ValueSet", id))
    }

    #[instrument(skip(self))]
    async fn delete_value_set(&self, id: &str) -> Result<(), StorageError> {
        self.ensure_table("ValueSet").await?;

        let uuid = Uuid::parse_str(id)
            .map_err(|_| StorageError::invalid_resource("Invalid UUID format"))?;

        let txid = self.create_txid().await?;

        let result = query(
            "UPDATE valueset SET status = 'deleted', txid = $2, ts = NOW() WHERE id = $1 AND status != 'deleted'",
        )
        .bind(uuid)
        .bind(txid)
        .execute(&self.pool)
        .await
        .map_err(PostgresError::from)?;

        if result.rows_affected() == 0 {
            return Err(StorageError::not_found("ValueSet", id));
        }

        debug!("Deleted ValueSet: {}", id);
        Ok(())
    }

    // ==================== CodeSystem ====================

    #[instrument(skip(self))]
    async fn list_code_systems(&self) -> Result<Vec<Value>, StorageError> {
        self.ensure_table("CodeSystem").await?;

        let rows: Vec<(Value,)> = query_as(
            "SELECT resource FROM codesystem WHERE status != 'deleted' ORDER BY resource->>'name'",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(PostgresError::from)?;

        Ok(rows.into_iter().map(|(r,)| r).collect())
    }

    #[instrument(skip(self))]
    async fn get_code_system_by_url(
        &self,
        url: &str,
        version: Option<&str>,
    ) -> Result<Option<Value>, StorageError> {
        self.ensure_table("CodeSystem").await?;

        let row: Option<(Value,)> = if let Some(ver) = version {
            query_as("SELECT resource FROM codesystem WHERE resource->>'url' = $1 AND resource->>'version' = $2 AND status != 'deleted'")
                .bind(url)
                .bind(ver)
                .fetch_optional(&self.pool)
                .await
                .map_err(PostgresError::from)?
        } else {
            query_as(
                "SELECT resource FROM codesystem WHERE resource->>'url' = $1 AND status != 'deleted' ORDER BY resource->>'version' DESC NULLS LAST LIMIT 1",
            )
            .bind(url)
            .fetch_optional(&self.pool)
            .await
            .map_err(PostgresError::from)?
        };

        Ok(row.map(|(r,)| r))
    }

    #[instrument(skip(self))]
    async fn get_code_system(&self, id: &str) -> Result<Option<Value>, StorageError> {
        self.ensure_table("CodeSystem").await?;

        let uuid = Uuid::parse_str(id)
            .map_err(|_| StorageError::invalid_resource("Invalid UUID format"))?;

        let row: Option<(Value,)> =
            query_as("SELECT resource FROM codesystem WHERE id = $1 AND status != 'deleted'")
                .bind(uuid)
                .fetch_optional(&self.pool)
                .await
                .map_err(PostgresError::from)?;

        Ok(row.map(|(r,)| r))
    }

    #[instrument(skip(self, resource))]
    async fn create_code_system(&self, resource: &Value) -> Result<Value, StorageError> {
        self.ensure_table("CodeSystem").await?;

        let txid = self.create_txid().await?;
        let id = Uuid::new_v4();

        let mut result = resource.clone();
        result["id"] = Value::String(id.to_string());

        let row: (Value,) = query_as(
            r#"
            INSERT INTO codesystem (id, txid, resource, status)
            VALUES ($1, $2, $3, 'created')
            RETURNING resource
            "#,
        )
        .bind(id)
        .bind(txid)
        .bind(&result)
        .fetch_one(&self.pool)
        .await
        .map_err(PostgresError::from)?;

        let name = resource.get("name").and_then(Value::as_str).unwrap_or("?");
        let url = resource.get("url").and_then(Value::as_str).unwrap_or("?");
        debug!("Created CodeSystem: {} ({})", name, url);
        Ok(row.0)
    }

    #[instrument(skip(self, resource))]
    async fn update_code_system(&self, id: &str, resource: &Value) -> Result<Value, StorageError> {
        self.ensure_table("CodeSystem").await?;

        let uuid = Uuid::parse_str(id)
            .map_err(|_| StorageError::invalid_resource("Invalid UUID format"))?;

        let txid = self.create_txid().await?;

        let mut result = resource.clone();
        result["id"] = Value::String(id.to_string());

        let row: Option<(Value,)> = query_as(
            r#"
            UPDATE codesystem
            SET txid = $2, ts = NOW(), resource = $3, status = 'updated'
            WHERE id = $1 AND status != 'deleted'
            RETURNING resource
            "#,
        )
        .bind(uuid)
        .bind(txid)
        .bind(&result)
        .fetch_optional(&self.pool)
        .await
        .map_err(PostgresError::from)?;

        row.map(|(r,)| r)
            .ok_or_else(|| StorageError::not_found("CodeSystem", id))
    }

    #[instrument(skip(self))]
    async fn delete_code_system(&self, id: &str) -> Result<(), StorageError> {
        self.ensure_table("CodeSystem").await?;

        let uuid = Uuid::parse_str(id)
            .map_err(|_| StorageError::invalid_resource("Invalid UUID format"))?;

        let txid = self.create_txid().await?;

        let result = query(
            "UPDATE codesystem SET status = 'deleted', txid = $2, ts = NOW() WHERE id = $1 AND status != 'deleted'",
        )
        .bind(uuid)
        .bind(txid)
        .execute(&self.pool)
        .await
        .map_err(PostgresError::from)?;

        if result.rows_affected() == 0 {
            return Err(StorageError::not_found("CodeSystem", id));
        }

        debug!("Deleted CodeSystem: {}", id);
        Ok(())
    }

    // ==================== SearchParameter ====================

    #[instrument(skip(self))]
    async fn list_search_parameters(&self) -> Result<Vec<Value>, StorageError> {
        self.ensure_table("SearchParameter").await?;

        let rows: Vec<(Value,)> =
            query_as("SELECT resource FROM searchparameter WHERE status != 'deleted' ORDER BY resource->>'name'")
                .fetch_all(&self.pool)
                .await
                .map_err(PostgresError::from)?;

        Ok(rows.into_iter().map(|(r,)| r).collect())
    }

    #[instrument(skip(self))]
    async fn get_search_parameter_by_url(
        &self,
        url: &str,
        version: Option<&str>,
    ) -> Result<Option<Value>, StorageError> {
        self.ensure_table("SearchParameter").await?;

        let row: Option<(Value,)> = if let Some(ver) = version {
            query_as(
                "SELECT resource FROM searchparameter WHERE resource->>'url' = $1 AND resource->>'version' = $2 AND status != 'deleted'",
            )
            .bind(url)
            .bind(ver)
            .fetch_optional(&self.pool)
            .await
            .map_err(PostgresError::from)?
        } else {
            query_as(
                "SELECT resource FROM searchparameter WHERE resource->>'url' = $1 AND status != 'deleted' ORDER BY resource->>'version' DESC NULLS LAST LIMIT 1",
            )
            .bind(url)
            .fetch_optional(&self.pool)
            .await
            .map_err(PostgresError::from)?
        };

        Ok(row.map(|(r,)| r))
    }

    #[instrument(skip(self))]
    async fn get_search_parameters_for_resource(
        &self,
        resource_type: &str,
    ) -> Result<Vec<Value>, StorageError> {
        self.ensure_table("SearchParameter").await?;

        // Use JSONB containment to check if resource_type is in the base array
        let rows: Vec<(Value,)> = query_as(
            "SELECT resource FROM searchparameter WHERE resource->'base' ? $1 AND status != 'deleted' ORDER BY resource->>'name'",
        )
        .bind(resource_type)
        .fetch_all(&self.pool)
        .await
        .map_err(PostgresError::from)?;

        Ok(rows.into_iter().map(|(r,)| r).collect())
    }

    #[instrument(skip(self))]
    async fn get_search_parameter(&self, id: &str) -> Result<Option<Value>, StorageError> {
        self.ensure_table("SearchParameter").await?;

        let uuid = Uuid::parse_str(id)
            .map_err(|_| StorageError::invalid_resource("Invalid UUID format"))?;

        let row: Option<(Value,)> =
            query_as("SELECT resource FROM searchparameter WHERE id = $1 AND status != 'deleted'")
                .bind(uuid)
                .fetch_optional(&self.pool)
                .await
                .map_err(PostgresError::from)?;

        Ok(row.map(|(r,)| r))
    }

    #[instrument(skip(self, resource))]
    async fn create_search_parameter(&self, resource: &Value) -> Result<Value, StorageError> {
        self.ensure_table("SearchParameter").await?;

        let txid = self.create_txid().await?;
        let id = Uuid::new_v4();

        let mut result = resource.clone();
        result["id"] = Value::String(id.to_string());

        let row: (Value,) = query_as(
            r#"
            INSERT INTO searchparameter (id, txid, resource, status)
            VALUES ($1, $2, $3, 'created')
            RETURNING resource
            "#,
        )
        .bind(id)
        .bind(txid)
        .bind(&result)
        .fetch_one(&self.pool)
        .await
        .map_err(PostgresError::from)?;

        let name = resource.get("name").and_then(Value::as_str).unwrap_or("?");
        let url = resource.get("url").and_then(Value::as_str).unwrap_or("?");
        debug!("Created SearchParameter: {} ({})", name, url);
        Ok(row.0)
    }

    #[instrument(skip(self, resource))]
    async fn update_search_parameter(
        &self,
        id: &str,
        resource: &Value,
    ) -> Result<Value, StorageError> {
        self.ensure_table("SearchParameter").await?;

        let uuid = Uuid::parse_str(id)
            .map_err(|_| StorageError::invalid_resource("Invalid UUID format"))?;

        let txid = self.create_txid().await?;

        let mut result = resource.clone();
        result["id"] = Value::String(id.to_string());

        let row: Option<(Value,)> = query_as(
            r#"
            UPDATE searchparameter
            SET txid = $2, ts = NOW(), resource = $3, status = 'updated'
            WHERE id = $1 AND status != 'deleted'
            RETURNING resource
            "#,
        )
        .bind(uuid)
        .bind(txid)
        .bind(&result)
        .fetch_optional(&self.pool)
        .await
        .map_err(PostgresError::from)?;

        row.map(|(r,)| r)
            .ok_or_else(|| StorageError::not_found("SearchParameter", id))
    }

    #[instrument(skip(self))]
    async fn delete_search_parameter(&self, id: &str) -> Result<(), StorageError> {
        self.ensure_table("SearchParameter").await?;

        let uuid = Uuid::parse_str(id)
            .map_err(|_| StorageError::invalid_resource("Invalid UUID format"))?;

        let txid = self.create_txid().await?;

        let result = query(
            "UPDATE searchparameter SET status = 'deleted', txid = $2, ts = NOW() WHERE id = $1 AND status != 'deleted'",
        )
        .bind(uuid)
        .bind(txid)
        .execute(&self.pool)
        .await
        .map_err(PostgresError::from)?;

        if result.rows_affected() == 0 {
            return Err(StorageError::not_found("SearchParameter", id));
        }

        debug!("Deleted SearchParameter: {}", id);
        Ok(())
    }

    // ==================== Bulk Operations ====================

    #[instrument(skip(self))]
    async fn load_all_conformance(
        &self,
    ) -> Result<(Vec<Value>, Vec<Value>, Vec<Value>, Vec<Value>), StorageError> {
        let structure_definitions = self.list_structure_definitions().await?;
        let value_sets = self.list_value_sets().await?;
        let code_systems = self.list_code_systems().await?;
        let search_parameters = self.list_search_parameters().await?;

        debug!(
            "Loaded conformance resources: {} SDs, {} VSs, {} CSs, {} SPs",
            structure_definitions.len(),
            value_sets.len(),
            code_systems.len(),
            search_parameters.len()
        );

        Ok((
            structure_definitions,
            value_sets,
            code_systems,
            search_parameters,
        ))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_conformance_storage_creation() {
        // Basic test - actual functionality requires database connection
    }
}
