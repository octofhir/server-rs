//! PostgreSQL implementation of ConformanceStorage for internal OctoFHIR resources.
//!
//! This module provides storage for conformance resources (StructureDefinitions,
//! ValueSets, CodeSystems, SearchParameters) in the `octofhir` schema.

use async_trait::async_trait;
use serde_json::Value;
use sqlx_core::query::query;
use sqlx_core::query_as::query_as;
use sqlx_postgres::PgPool;
use tracing::{debug, instrument};
use uuid::Uuid;

use octofhir_storage::{ConformanceStorage, StorageError};

use crate::error::PostgresError;

/// PostgreSQL implementation of the ConformanceStorage trait.
///
/// This implementation stores conformance resources in the `octofhir` schema
/// with full CRUD support and change notifications for hot-reload.
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
        let row: (i64,) = query_as(
            "INSERT INTO public._transaction (status) VALUES ('committed') RETURNING txid",
        )
        .fetch_one(&self.pool)
        .await
        .map_err(PostgresError::from)?;

        Ok(row.0)
    }

    /// Extracts common fields from a conformance resource.
    fn extract_common_fields(
        resource: &Value,
    ) -> Result<(String, Option<String>, String), StorageError> {
        let url = resource
            .get("url")
            .and_then(Value::as_str)
            .ok_or_else(|| StorageError::invalid_resource("Missing required field: url"))?
            .to_string();

        let version = resource
            .get("version")
            .and_then(Value::as_str)
            .map(String::from);

        let name = resource
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| StorageError::invalid_resource("Missing required field: name"))?
            .to_string();

        Ok((url, version, name))
    }
}

#[async_trait]
impl ConformanceStorage for PostgresConformanceStorage {
    // ==================== StructureDefinition ====================

    #[instrument(skip(self))]
    async fn list_structure_definitions(&self) -> Result<Vec<Value>, StorageError> {
        let rows: Vec<(Value,)> =
            query_as("SELECT resource FROM octofhir.structuredefinition ORDER BY name")
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
        let row: Option<(Value,)> = if let Some(ver) = version {
            query_as(
                "SELECT resource FROM octofhir.structuredefinition WHERE url = $1 AND version = $2",
            )
            .bind(url)
            .bind(ver)
            .fetch_optional(&self.pool)
            .await
            .map_err(PostgresError::from)?
        } else {
            query_as(
                "SELECT resource FROM octofhir.structuredefinition WHERE url = $1 ORDER BY version DESC NULLS LAST LIMIT 1",
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
        let uuid = Uuid::parse_str(id)
            .map_err(|_| StorageError::invalid_resource("Invalid UUID format"))?;

        let row: Option<(Value,)> =
            query_as("SELECT resource FROM octofhir.structuredefinition WHERE id = $1")
                .bind(uuid)
                .fetch_optional(&self.pool)
                .await
                .map_err(PostgresError::from)?;

        Ok(row.map(|(r,)| r))
    }

    #[instrument(skip(self, resource))]
    async fn create_structure_definition(&self, resource: &Value) -> Result<Value, StorageError> {
        let (url, version, name) = Self::extract_common_fields(resource)?;

        let status = resource
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("draft");

        let kind = resource
            .get("kind")
            .and_then(Value::as_str)
            .ok_or_else(|| StorageError::invalid_resource("Missing required field: kind"))?;

        let type_field = resource.get("type").and_then(Value::as_str);
        let base_definition = resource.get("baseDefinition").and_then(Value::as_str);
        let derivation = resource.get("derivation").and_then(Value::as_str);

        let txid = self.create_txid().await?;
        let id = Uuid::new_v4();

        // Add id to resource
        let mut result = resource.clone();
        result["id"] = Value::String(id.to_string());

        let row: (Value,) = query_as(
            r#"
            INSERT INTO octofhir.structuredefinition
                (id, url, version, name, status, kind, type, base_definition, derivation, txid, resource)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
            RETURNING resource
            "#,
        )
        .bind(id)
        .bind(&url)
        .bind(&version)
        .bind(&name)
        .bind(status)
        .bind(kind)
        .bind(type_field)
        .bind(base_definition)
        .bind(derivation)
        .bind(txid)
        .bind(&result)
        .fetch_one(&self.pool)
        .await
        .map_err(PostgresError::from)?;

        debug!("Created StructureDefinition: {} ({})", name, url);
        Ok(row.0)
    }

    #[instrument(skip(self, resource))]
    async fn update_structure_definition(
        &self,
        id: &str,
        resource: &Value,
    ) -> Result<Value, StorageError> {
        let uuid = Uuid::parse_str(id)
            .map_err(|_| StorageError::invalid_resource("Invalid UUID format"))?;

        let (url, version, name) = Self::extract_common_fields(resource)?;

        let status = resource
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("draft");

        let kind = resource
            .get("kind")
            .and_then(Value::as_str)
            .ok_or_else(|| StorageError::invalid_resource("Missing required field: kind"))?;

        let type_field = resource.get("type").and_then(Value::as_str);
        let base_definition = resource.get("baseDefinition").and_then(Value::as_str);
        let derivation = resource.get("derivation").and_then(Value::as_str);

        let txid = self.create_txid().await?;

        // Ensure id is in resource
        let mut result = resource.clone();
        result["id"] = Value::String(id.to_string());

        let row: Option<(Value,)> = query_as(
            r#"
            UPDATE octofhir.structuredefinition
            SET url = $2, version = $3, name = $4, status = $5, kind = $6,
                type = $7, base_definition = $8, derivation = $9, txid = $10,
                ts = NOW(), resource = $11
            WHERE id = $1
            RETURNING resource
            "#,
        )
        .bind(uuid)
        .bind(&url)
        .bind(&version)
        .bind(&name)
        .bind(status)
        .bind(kind)
        .bind(type_field)
        .bind(base_definition)
        .bind(derivation)
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
        let uuid = Uuid::parse_str(id)
            .map_err(|_| StorageError::invalid_resource("Invalid UUID format"))?;

        let result = query("DELETE FROM octofhir.structuredefinition WHERE id = $1")
            .bind(uuid)
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
        let rows: Vec<(Value,)> = query_as("SELECT resource FROM octofhir.valueset ORDER BY name")
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
        let row: Option<(Value,)> = if let Some(ver) = version {
            query_as("SELECT resource FROM octofhir.valueset WHERE url = $1 AND version = $2")
                .bind(url)
                .bind(ver)
                .fetch_optional(&self.pool)
                .await
                .map_err(PostgresError::from)?
        } else {
            query_as(
                "SELECT resource FROM octofhir.valueset WHERE url = $1 ORDER BY version DESC NULLS LAST LIMIT 1",
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
        let uuid = Uuid::parse_str(id)
            .map_err(|_| StorageError::invalid_resource("Invalid UUID format"))?;

        let row: Option<(Value,)> =
            query_as("SELECT resource FROM octofhir.valueset WHERE id = $1")
                .bind(uuid)
                .fetch_optional(&self.pool)
                .await
                .map_err(PostgresError::from)?;

        Ok(row.map(|(r,)| r))
    }

    #[instrument(skip(self, resource))]
    async fn create_value_set(&self, resource: &Value) -> Result<Value, StorageError> {
        let (url, version, name) = Self::extract_common_fields(resource)?;

        let status = resource
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("draft");

        let txid = self.create_txid().await?;
        let id = Uuid::new_v4();

        let mut result = resource.clone();
        result["id"] = Value::String(id.to_string());

        let row: (Value,) = query_as(
            r#"
            INSERT INTO octofhir.valueset (id, url, version, name, status, txid, resource)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            RETURNING resource
            "#,
        )
        .bind(id)
        .bind(&url)
        .bind(&version)
        .bind(&name)
        .bind(status)
        .bind(txid)
        .bind(&result)
        .fetch_one(&self.pool)
        .await
        .map_err(PostgresError::from)?;

        debug!("Created ValueSet: {} ({})", name, url);
        Ok(row.0)
    }

    #[instrument(skip(self, resource))]
    async fn update_value_set(&self, id: &str, resource: &Value) -> Result<Value, StorageError> {
        let uuid = Uuid::parse_str(id)
            .map_err(|_| StorageError::invalid_resource("Invalid UUID format"))?;

        let (url, version, name) = Self::extract_common_fields(resource)?;

        let status = resource
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("draft");

        let txid = self.create_txid().await?;

        let mut result = resource.clone();
        result["id"] = Value::String(id.to_string());

        let row: Option<(Value,)> = query_as(
            r#"
            UPDATE octofhir.valueset
            SET url = $2, version = $3, name = $4, status = $5, txid = $6, ts = NOW(), resource = $7
            WHERE id = $1
            RETURNING resource
            "#,
        )
        .bind(uuid)
        .bind(&url)
        .bind(&version)
        .bind(&name)
        .bind(status)
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
        let uuid = Uuid::parse_str(id)
            .map_err(|_| StorageError::invalid_resource("Invalid UUID format"))?;

        let result = query("DELETE FROM octofhir.valueset WHERE id = $1")
            .bind(uuid)
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
        let rows: Vec<(Value,)> =
            query_as("SELECT resource FROM octofhir.codesystem ORDER BY name")
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
        let row: Option<(Value,)> = if let Some(ver) = version {
            query_as("SELECT resource FROM octofhir.codesystem WHERE url = $1 AND version = $2")
                .bind(url)
                .bind(ver)
                .fetch_optional(&self.pool)
                .await
                .map_err(PostgresError::from)?
        } else {
            query_as(
                "SELECT resource FROM octofhir.codesystem WHERE url = $1 ORDER BY version DESC NULLS LAST LIMIT 1",
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
        let uuid = Uuid::parse_str(id)
            .map_err(|_| StorageError::invalid_resource("Invalid UUID format"))?;

        let row: Option<(Value,)> =
            query_as("SELECT resource FROM octofhir.codesystem WHERE id = $1")
                .bind(uuid)
                .fetch_optional(&self.pool)
                .await
                .map_err(PostgresError::from)?;

        Ok(row.map(|(r,)| r))
    }

    #[instrument(skip(self, resource))]
    async fn create_code_system(&self, resource: &Value) -> Result<Value, StorageError> {
        let (url, version, name) = Self::extract_common_fields(resource)?;

        let status = resource
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("draft");

        let content = resource
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or("complete");

        let txid = self.create_txid().await?;
        let id = Uuid::new_v4();

        let mut result = resource.clone();
        result["id"] = Value::String(id.to_string());

        let row: (Value,) = query_as(
            r#"
            INSERT INTO octofhir.codesystem (id, url, version, name, status, content, txid, resource)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            RETURNING resource
            "#,
        )
        .bind(id)
        .bind(&url)
        .bind(&version)
        .bind(&name)
        .bind(status)
        .bind(content)
        .bind(txid)
        .bind(&result)
        .fetch_one(&self.pool)
        .await
        .map_err(PostgresError::from)?;

        debug!("Created CodeSystem: {} ({})", name, url);
        Ok(row.0)
    }

    #[instrument(skip(self, resource))]
    async fn update_code_system(&self, id: &str, resource: &Value) -> Result<Value, StorageError> {
        let uuid = Uuid::parse_str(id)
            .map_err(|_| StorageError::invalid_resource("Invalid UUID format"))?;

        let (url, version, name) = Self::extract_common_fields(resource)?;

        let status = resource
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("draft");

        let content = resource
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or("complete");

        let txid = self.create_txid().await?;

        let mut result = resource.clone();
        result["id"] = Value::String(id.to_string());

        let row: Option<(Value,)> = query_as(
            r#"
            UPDATE octofhir.codesystem
            SET url = $2, version = $3, name = $4, status = $5, content = $6,
                txid = $7, ts = NOW(), resource = $8
            WHERE id = $1
            RETURNING resource
            "#,
        )
        .bind(uuid)
        .bind(&url)
        .bind(&version)
        .bind(&name)
        .bind(status)
        .bind(content)
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
        let uuid = Uuid::parse_str(id)
            .map_err(|_| StorageError::invalid_resource("Invalid UUID format"))?;

        let result = query("DELETE FROM octofhir.codesystem WHERE id = $1")
            .bind(uuid)
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
        let rows: Vec<(Value,)> =
            query_as("SELECT resource FROM octofhir.searchparameter ORDER BY name")
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
        let row: Option<(Value,)> = if let Some(ver) = version {
            query_as(
                "SELECT resource FROM octofhir.searchparameter WHERE url = $1 AND version = $2",
            )
            .bind(url)
            .bind(ver)
            .fetch_optional(&self.pool)
            .await
            .map_err(PostgresError::from)?
        } else {
            query_as(
                "SELECT resource FROM octofhir.searchparameter WHERE url = $1 ORDER BY version DESC NULLS LAST LIMIT 1",
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
        let rows: Vec<(Value,)> = query_as(
            "SELECT resource FROM octofhir.searchparameter WHERE $1 = ANY(base) ORDER BY name",
        )
        .bind(resource_type)
        .fetch_all(&self.pool)
        .await
        .map_err(PostgresError::from)?;

        Ok(rows.into_iter().map(|(r,)| r).collect())
    }

    #[instrument(skip(self))]
    async fn get_search_parameter(&self, id: &str) -> Result<Option<Value>, StorageError> {
        let uuid = Uuid::parse_str(id)
            .map_err(|_| StorageError::invalid_resource("Invalid UUID format"))?;

        let row: Option<(Value,)> =
            query_as("SELECT resource FROM octofhir.searchparameter WHERE id = $1")
                .bind(uuid)
                .fetch_optional(&self.pool)
                .await
                .map_err(PostgresError::from)?;

        Ok(row.map(|(r,)| r))
    }

    #[instrument(skip(self, resource))]
    async fn create_search_parameter(&self, resource: &Value) -> Result<Value, StorageError> {
        let (url, version, name) = Self::extract_common_fields(resource)?;

        let code = resource
            .get("code")
            .and_then(Value::as_str)
            .ok_or_else(|| StorageError::invalid_resource("Missing required field: code"))?;

        let status = resource
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("draft");

        let base: Vec<String> = resource
            .get("base")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(Value::as_str)
                    .map(String::from)
                    .collect()
            })
            .ok_or_else(|| StorageError::invalid_resource("Missing required field: base"))?;

        let sp_type = resource
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| StorageError::invalid_resource("Missing required field: type"))?;

        let expression = resource.get("expression").and_then(Value::as_str);

        let txid = self.create_txid().await?;
        let id = Uuid::new_v4();

        let mut result = resource.clone();
        result["id"] = Value::String(id.to_string());

        let row: (Value,) = query_as(
            r#"
            INSERT INTO octofhir.searchparameter
                (id, url, version, name, code, status, base, type, expression, txid, resource)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
            RETURNING resource
            "#,
        )
        .bind(id)
        .bind(&url)
        .bind(&version)
        .bind(&name)
        .bind(code)
        .bind(status)
        .bind(&base)
        .bind(sp_type)
        .bind(expression)
        .bind(txid)
        .bind(&result)
        .fetch_one(&self.pool)
        .await
        .map_err(PostgresError::from)?;

        debug!("Created SearchParameter: {} ({})", name, url);
        Ok(row.0)
    }

    #[instrument(skip(self, resource))]
    async fn update_search_parameter(
        &self,
        id: &str,
        resource: &Value,
    ) -> Result<Value, StorageError> {
        let uuid = Uuid::parse_str(id)
            .map_err(|_| StorageError::invalid_resource("Invalid UUID format"))?;

        let (url, version, name) = Self::extract_common_fields(resource)?;

        let code = resource
            .get("code")
            .and_then(Value::as_str)
            .ok_or_else(|| StorageError::invalid_resource("Missing required field: code"))?;

        let status = resource
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("draft");

        let base: Vec<String> = resource
            .get("base")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(Value::as_str)
                    .map(String::from)
                    .collect()
            })
            .ok_or_else(|| StorageError::invalid_resource("Missing required field: base"))?;

        let sp_type = resource
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| StorageError::invalid_resource("Missing required field: type"))?;

        let expression = resource.get("expression").and_then(Value::as_str);

        let txid = self.create_txid().await?;

        let mut result = resource.clone();
        result["id"] = Value::String(id.to_string());

        let row: Option<(Value,)> = query_as(
            r#"
            UPDATE octofhir.searchparameter
            SET url = $2, version = $3, name = $4, code = $5, status = $6,
                base = $7, type = $8, expression = $9, txid = $10, ts = NOW(), resource = $11
            WHERE id = $1
            RETURNING resource
            "#,
        )
        .bind(uuid)
        .bind(&url)
        .bind(&version)
        .bind(&name)
        .bind(code)
        .bind(status)
        .bind(&base)
        .bind(sp_type)
        .bind(expression)
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
        let uuid = Uuid::parse_str(id)
            .map_err(|_| StorageError::invalid_resource("Invalid UUID format"))?;

        let result = query("DELETE FROM octofhir.searchparameter WHERE id = $1")
            .bind(uuid)
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
    use super::*;

    #[test]
    fn test_extract_common_fields() {
        let resource = serde_json::json!({
            "resourceType": "StructureDefinition",
            "url": "http://example.org/sd/test",
            "name": "TestSD",
            "version": "1.0.0"
        });

        let (url, version, name) =
            PostgresConformanceStorage::extract_common_fields(&resource).unwrap();

        assert_eq!(url, "http://example.org/sd/test");
        assert_eq!(version, Some("1.0.0".to_string()));
        assert_eq!(name, "TestSD");
    }

    #[test]
    fn test_extract_common_fields_missing_url() {
        let resource = serde_json::json!({
            "resourceType": "StructureDefinition",
            "name": "TestSD"
        });

        let result = PostgresConformanceStorage::extract_common_fields(&resource);
        assert!(result.is_err());
    }
}
