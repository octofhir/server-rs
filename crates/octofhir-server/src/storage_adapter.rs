//! Storage adapter for PostgreSQL backend.
//!
//! This module provides an adapter that wraps `PostgresStorage` and implements
//! the internal `Storage` trait used by handlers, bridging between the
//! `FhirStorage` trait (raw JSON) and the internal trait (ResourceEnvelope).

use async_trait::async_trait;
use octofhir_core::{CoreError, ResourceEnvelope, ResourceType, Result};
use octofhir_db_postgres::PostgresStorage;
use octofhir_storage::{
    FhirStorage, HistoryParams, HistoryResult, StorageError,
    legacy::{
        QueryFilter, QueryResult, SearchQuery, Storage, Transaction, TransactionManager,
        TransactionStats,
    },
};
use serde_json::Value;
use std::sync::Arc;

/// Adapter that wraps `PostgresStorage` and implements the internal `Storage` trait.
pub struct PostgresStorageAdapter {
    inner: Arc<PostgresStorage>,
    stats: TransactionStats,
}

impl PostgresStorageAdapter {
    /// Creates a new adapter wrapping the given PostgreSQL storage.
    pub fn new(storage: PostgresStorage) -> Self {
        Self {
            inner: Arc::new(storage),
            stats: TransactionStats::default(),
        }
    }

    /// Creates a new adapter from an Arc-wrapped PostgreSQL storage.
    pub fn from_arc(storage: Arc<PostgresStorage>) -> Self {
        Self {
            inner: storage,
            stats: TransactionStats::default(),
        }
    }

    /// Convert StorageError to CoreError
    fn map_error(e: StorageError, resource_type: &str, id: &str) -> CoreError {
        match e {
            StorageError::NotFound { .. } => CoreError::resource_not_found(resource_type, id),
            StorageError::AlreadyExists { .. } => CoreError::resource_conflict(resource_type, id),
            StorageError::Deleted { .. } => CoreError::resource_deleted(resource_type, id),
            StorageError::VersionConflict { expected, actual } => CoreError::invalid_resource(
                format!("Version conflict: expected {expected}, got {actual}"),
            ),
            StorageError::InvalidResource { message } => CoreError::invalid_resource(message),
            _ => CoreError::invalid_resource(e.to_string()),
        }
    }

    /// Convert JSON Value to ResourceEnvelope
    fn value_to_envelope(value: &Value) -> Result<ResourceEnvelope> {
        serde_json::from_value(value.clone()).map_err(CoreError::from)
    }

    /// Convert ResourceEnvelope to JSON Value
    fn envelope_to_value(envelope: &ResourceEnvelope) -> Result<Value> {
        serde_json::to_value(envelope).map_err(CoreError::from)
    }

    /// Build a WHERE clause from query filters.
    /// Returns (clause_string, parameter_values) where clause_string starts with " AND ..." if filters exist.
    fn build_where_clause(filters: &[QueryFilter]) -> (String, Vec<String>) {
        let mut clause_parts = Vec::new();
        let mut params = Vec::new();

        for filter in filters {
            match filter {
                QueryFilter::Exact { field, value } => match field.as_str() {
                    "_id" => {
                        clause_parts.push(format!("id::text = ${}", params.len() + 1));
                        params.push(value.clone());
                    }
                    other => {
                        clause_parts.push(format!(
                            "resource->>'{}' = ${}",
                            Self::escape_jsonb_key(other),
                            params.len() + 1
                        ));
                        params.push(value.clone());
                    }
                },
                QueryFilter::Contains { field, value } => {
                    clause_parts.push(format!(
                        "resource->>'{}' ILIKE ${}",
                        Self::escape_jsonb_key(field),
                        params.len() + 1
                    ));
                    params.push(format!("%{value}%"));
                }
                QueryFilter::Prefix { field, value } => {
                    clause_parts.push(format!(
                        "resource->>'{}' ILIKE ${}",
                        Self::escape_jsonb_key(field),
                        params.len() + 1
                    ));
                    params.push(format!("{value}%"));
                }
                QueryFilter::Token {
                    field,
                    system,
                    code,
                } => {
                    // Handle simple string fields (like gender) and coded values
                    if system.is_some() {
                        // Full token: system|code
                        clause_parts.push(format!(
                            "(resource->>'{}' ILIKE ${} OR resource->'{}'->'coding' @> ${}::jsonb)",
                            Self::escape_jsonb_key(field),
                            params.len() + 1,
                            Self::escape_jsonb_key(field),
                            params.len() + 2
                        ));
                        params.push(code.clone());
                        params.push(
                            serde_json::json!([{"system": system, "code": code}]).to_string(),
                        );
                    } else {
                        // Code only
                        clause_parts.push(format!(
                            "(resource->>'{}' ILIKE ${} OR resource @> ${}::jsonb)",
                            Self::escape_jsonb_key(field),
                            params.len() + 1,
                            params.len() + 2
                        ));
                        params.push(code.clone());
                        // Search for code in coding array or directly
                        // Note: use (field) to interpolate the variable as the key
                        params.push(
                            serde_json::json!({(field): {"coding": [{"code": code}]}}).to_string(),
                        );
                    }
                }
                QueryFilter::Boolean { field, value } => {
                    clause_parts.push(format!(
                        "(resource->>'{}')::boolean = ${}",
                        Self::escape_jsonb_key(field),
                        params.len() + 1
                    ));
                    params.push(value.to_string());
                }
                QueryFilter::Identifier {
                    field,
                    system,
                    value,
                } => {
                    if let Some(sys) = system {
                        // system|value format
                        clause_parts.push(format!(
                            "resource->'{}' @> ${}::jsonb",
                            Self::escape_jsonb_key(field),
                            params.len() + 1
                        ));
                        params
                            .push(serde_json::json!([{"system": sys, "value": value}]).to_string());
                    } else {
                        // value only
                        clause_parts.push(format!(
                            "resource->'{}' @> ${}::jsonb",
                            Self::escape_jsonb_key(field),
                            params.len() + 1
                        ));
                        params.push(serde_json::json!([{"value": value}]).to_string());
                    }
                }
                QueryFilter::DateRange { field, start, end } => {
                    let field_path = if field == "_lastUpdated" {
                        "ts".to_string()
                    } else {
                        format!(
                            "(resource->>'{}')::timestamp",
                            Self::escape_jsonb_key(field)
                        )
                    };

                    if let Some(s) = start {
                        clause_parts.push(format!("{field_path} >= ${}", params.len() + 1));
                        params.push(s.to_string());
                    }
                    if let Some(e) = end {
                        clause_parts.push(format!("{field_path} <= ${}", params.len() + 1));
                        params.push(e.to_string());
                    }
                }
                QueryFilter::NumberRange { field, min, max } => {
                    let field_path =
                        format!("(resource->>'{}')::numeric", Self::escape_jsonb_key(field));
                    if let Some(m) = min {
                        clause_parts.push(format!("{field_path} >= ${}", params.len() + 1));
                        params.push(m.to_string());
                    }
                    if let Some(m) = max {
                        clause_parts.push(format!("{field_path} <= ${}", params.len() + 1));
                        params.push(m.to_string());
                    }
                }
            }
        }

        if clause_parts.is_empty() {
            (String::new(), params)
        } else {
            (format!(" AND {}", clause_parts.join(" AND ")), params)
        }
    }

    /// Escape a JSONB key to prevent SQL injection.
    fn escape_jsonb_key(key: &str) -> String {
        // Replace single quotes with doubled quotes and remove any dangerous characters
        key.replace('\'', "''")
            .replace('\\', "\\\\")
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-' || *c == '.')
            .collect()
    }

    /// Execute a count query with the given parameters.
    async fn execute_count_query(
        pool: &sqlx_postgres::PgPool,
        sql: &str,
        params: &[String],
    ) -> std::result::Result<i64, CoreError> {
        // Build dynamic query with parameters
        let mut query = sqlx::query_scalar::<_, i64>(sql);
        for param in params {
            query = query.bind(param);
        }

        query
            .fetch_one(pool)
            .await
            .map_err(|e| CoreError::invalid_resource(e.to_string()))
    }

    /// Execute a search query and convert results to ResourceEnvelope.
    async fn execute_search_query(
        pool: &sqlx_postgres::PgPool,
        sql: &str,
        params: &[String],
        limit: i64,
        offset: i64,
    ) -> Result<Vec<ResourceEnvelope>> {
        // Build dynamic query - parameters come in this order: filter params, then limit, offset
        let mut query = sqlx::query_as::<_, (Value,)>(sql);
        for param in params {
            query = query.bind(param);
        }
        query = query.bind(limit).bind(offset);

        let rows: Vec<(Value,)> = query
            .fetch_all(pool)
            .await
            .map_err(|e| CoreError::invalid_resource(e.to_string()))?;

        let mut results = Vec::with_capacity(rows.len());
        for (resource,) in rows {
            match Self::value_to_envelope(&resource) {
                Ok(env) => results.push(env),
                Err(e) => {
                    tracing::warn!("Failed to convert resource to envelope: {}", e);
                }
            }
        }

        Ok(results)
    }
}

impl std::fmt::Debug for PostgresStorageAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PostgresStorageAdapter")
            .field("stats", &self.stats)
            .finish()
    }
}

#[async_trait]
impl Storage for PostgresStorageAdapter {
    async fn get(
        &self,
        resource_type: &ResourceType,
        id: &str,
    ) -> Result<Option<ResourceEnvelope>> {
        let rt_str = resource_type.to_string();
        match self.inner.read(&rt_str, id).await {
            Ok(Some(stored)) => {
                let envelope = Self::value_to_envelope(&stored.resource)?;
                Ok(Some(envelope))
            }
            Ok(None) => Ok(None),
            Err(StorageError::Deleted { .. }) => Err(CoreError::resource_deleted(&rt_str, id)),
            Err(e) => Err(Self::map_error(e, &rt_str, id)),
        }
    }

    async fn insert(
        &self,
        resource_type: &ResourceType,
        resource: ResourceEnvelope,
    ) -> Result<ResourceEnvelope> {
        let rt_str = resource_type.to_string();
        let value = Self::envelope_to_value(&resource)?;
        match self.inner.create(&value).await {
            Ok(stored) => Self::value_to_envelope(&stored.resource),
            Err(e) => Err(Self::map_error(e, &rt_str, &resource.id)),
        }
    }

    async fn update(
        &self,
        resource_type: &ResourceType,
        id: &str,
        resource: ResourceEnvelope,
    ) -> Result<ResourceEnvelope> {
        let rt_str = resource_type.to_string();
        let value = Self::envelope_to_value(&resource)?;
        match self.inner.update(&value, None).await {
            Ok(stored) => Self::value_to_envelope(&stored.resource),
            Err(e) => Err(Self::map_error(e, &rt_str, id)),
        }
    }

    async fn delete(&self, resource_type: &ResourceType, id: &str) -> Result<ResourceEnvelope> {
        let rt_str = resource_type.to_string();

        // First get the current resource to return it
        let current = match self.inner.read(&rt_str, id).await {
            Ok(Some(stored)) => Self::value_to_envelope(&stored.resource)?,
            Ok(None) => {
                // Per FHIR spec: delete of non-existent is idempotent
                return Ok(ResourceEnvelope::new(id.to_string(), resource_type.clone()));
            }
            Err(StorageError::Deleted { .. }) => {
                // Already deleted - return empty envelope
                return Ok(ResourceEnvelope::new(id.to_string(), resource_type.clone()));
            }
            Err(e) => return Err(Self::map_error(e, &rt_str, id)),
        };

        // Now delete it
        match self.inner.delete(&rt_str, id).await {
            Ok(()) => Ok(current),
            Err(StorageError::NotFound { .. }) => {
                // Idempotent delete
                Ok(ResourceEnvelope::new(id.to_string(), resource_type.clone()))
            }
            Err(e) => Err(Self::map_error(e, &rt_str, id)),
        }
    }

    async fn exists(&self, resource_type: &ResourceType, id: &str) -> bool {
        let rt_str = resource_type.to_string();
        matches!(self.inner.read(&rt_str, id).await, Ok(Some(_)))
    }

    async fn count(&self) -> usize {
        // PostgreSQL search is not yet implemented, return 0
        0
    }

    async fn count_by_type(&self, resource_type: &ResourceType) -> usize {
        let table = resource_type.to_string().to_lowercase();
        let sql = format!(r#"SELECT COUNT(*) FROM "{table}" WHERE status != 'deleted'"#);
        match sqlx::query_scalar::<_, i64>(&sql)
            .fetch_one(self.inner.pool())
            .await
        {
            Ok(count) => count as usize,
            Err(_) => 0, // Table might not exist
        }
    }

    async fn search(&self, query: &SearchQuery) -> Result<QueryResult> {
        let table = query.resource_type.to_string().to_lowercase();

        tracing::debug!(filters = ?query.filters, "Building search query");

        // Build WHERE clause from filters
        let (where_clause, params) = Self::build_where_clause(&query.filters);

        // Build sort clause
        let sort_clause = if let Some(field) = &query.sort_field {
            let direction = if query.sort_ascending { "ASC" } else { "DESC" };
            match field.as_str() {
                "_id" => format!("ORDER BY id {direction}"),
                "_lastUpdated" => format!("ORDER BY ts {direction}"),
                other => format!("ORDER BY resource->>'{}' {direction} NULLS LAST", other),
            }
        } else {
            "ORDER BY ts DESC".to_string() // Default: newest first
        };

        // Count query (for total)
        let count_sql =
            format!(r#"SELECT COUNT(*) FROM "{table}" WHERE status != 'deleted'{where_clause}"#,);

        tracing::debug!(sql = %count_sql, params = ?params, "Executing count query");
        let total: i64 =
            match Self::execute_count_query(self.inner.pool(), &count_sql, &params).await {
                Ok(count) => {
                    tracing::debug!(count = count, "Count query succeeded");
                    count
                }
                Err(e) => {
                    tracing::warn!(error = %e, sql = %count_sql, "Count query failed");
                    0 // Table might not exist
                }
            };

        // Data query with pagination
        // LIMIT and OFFSET use parameter positions after filter params
        let limit_pos = params.len() + 1;
        let offset_pos = params.len() + 2;
        let data_sql = format!(
            r#"SELECT resource FROM "{table}" WHERE status != 'deleted'{where_clause} {sort_clause} LIMIT ${limit_pos} OFFSET ${offset_pos}"#,
        );

        let resources = Self::execute_search_query(
            self.inner.pool(),
            &data_sql,
            &params,
            query.count as i64,
            query.offset as i64,
        )
        .await?;

        Ok(QueryResult::new(
            total as usize,
            resources,
            query.offset,
            query.count,
        ))
    }

    async fn search_by_type(
        &self,
        resource_type: &ResourceType,
        filters: Vec<QueryFilter>,
        offset: usize,
        count: usize,
    ) -> Result<QueryResult> {
        let query = SearchQuery {
            resource_type: resource_type.clone(),
            filters,
            offset,
            count,
            sort_field: None,
            sort_ascending: true,
        };
        self.search(&query).await
    }

    async fn history(
        &self,
        resource_type: &str,
        id: Option<&str>,
        params: &HistoryParams,
    ) -> Result<HistoryResult> {
        match self.inner.history(resource_type, id, params).await {
            Ok(result) => Ok(result),
            Err(e) => Err(CoreError::invalid_resource(e.to_string())),
        }
    }

    async fn system_history(&self, params: &HistoryParams) -> Result<HistoryResult> {
        match self.inner.system_history(params).await {
            Ok(result) => Ok(result),
            Err(e) => Err(CoreError::invalid_resource(e.to_string())),
        }
    }

    async fn vread(
        &self,
        resource_type: &ResourceType,
        id: &str,
        version: &str,
    ) -> Result<Option<ResourceEnvelope>> {
        let rt_str = resource_type.to_string();
        match self.inner.vread(&rt_str, id, version).await {
            Ok(Some(stored)) => {
                let envelope = Self::value_to_envelope(&stored.resource)?;
                Ok(Some(envelope))
            }
            Ok(None) => Ok(None),
            Err(StorageError::Deleted { .. }) => Err(CoreError::resource_deleted(&rt_str, id)),
            Err(e) => Err(Self::map_error(e, &rt_str, id)),
        }
    }
}

#[async_trait]
impl TransactionManager for PostgresStorageAdapter {
    async fn begin_transaction(&mut self) -> Result<Transaction> {
        Ok(Transaction::new())
    }

    async fn execute_transaction(&mut self, _transaction: &mut Transaction) -> Result<()> {
        // PostgreSQL transactions not yet fully implemented
        Ok(())
    }

    async fn commit_transaction(&mut self, _transaction: &mut Transaction) -> Result<()> {
        // PostgreSQL transactions not yet fully implemented
        Ok(())
    }

    async fn rollback_transaction(&mut self, _transaction: &mut Transaction) -> Result<()> {
        // PostgreSQL transactions not yet fully implemented
        Ok(())
    }

    async fn abort_transaction(&mut self, _transaction: &mut Transaction) -> Result<()> {
        // PostgreSQL transactions not yet fully implemented
        Ok(())
    }

    fn get_transaction_stats(&self) -> TransactionStats {
        self.stats.clone()
    }
}
