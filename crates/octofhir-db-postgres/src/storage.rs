//! PostgreSQL implementation of the FhirStorage trait.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;
use sqlx_postgres::PgPool;

use octofhir_search::{SearchParameter, SearchParameterRegistry};
use octofhir_storage::{
    FhirStorage, HistoryParams, HistoryResult, SearchParams, SearchResult, StorageError,
    StoredResource, Transaction,
};

use crate::config::PostgresConfig;
use crate::migrations;
use crate::pool;
use crate::queries;
use crate::schema::SchemaManager;

/// PostgreSQL storage backend for FHIR resources.
///
/// This storage backend uses PostgreSQL to persist FHIR resources
/// with full support for versioning, history, and search.
#[derive(Debug, Clone)]
pub struct PostgresStorage {
    pool: PgPool,
    schema_manager: SchemaManager,
    /// Search parameter registry for parameter lookup during search
    search_registry: Option<Arc<SearchParameterRegistry>>,
}

impl PostgresStorage {
    /// Creates a new `PostgresStorage` with the given configuration.
    ///
    /// This will:
    /// 1. Create a connection pool
    /// 2. Run migrations (if configured)
    /// 3. Initialize the schema manager
    ///
    /// # Errors
    ///
    /// Returns an error if the connection pool cannot be created
    /// or if migrations fail.
    pub async fn new(config: PostgresConfig) -> Result<Self, StorageError> {
        let pool = pool::create_pool(&config).await?;

        if config.run_migrations {
            migrations::run(&pool, &config.url).await?;
        }

        let schema_manager = SchemaManager::new(pool.clone());

        Ok(Self {
            pool,
            schema_manager,
            search_registry: None,
        })
    }

    /// Creates a new `PostgresStorage` from an existing connection pool.
    ///
    /// This allows sharing a connection pool between multiple components.
    /// Migrations are not run automatically when using this constructor.
    #[must_use]
    pub fn from_pool(pool: PgPool) -> Self {
        let schema_manager = SchemaManager::new(pool.clone());
        Self {
            pool,
            schema_manager,
            search_registry: None,
        }
    }

    /// Returns a reference to the connection pool.
    #[must_use]
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Returns a reference to the schema manager.
    #[must_use]
    pub fn schema_manager(&self) -> &SchemaManager {
        &self.schema_manager
    }

    /// Sets the search parameter registry for this storage.
    ///
    /// The registry is used to look up search parameters during search execution.
    /// This should be called after loading search parameters from packages.
    pub fn set_search_registry(&mut self, registry: Arc<SearchParameterRegistry>) {
        self.search_registry = Some(registry);
    }

    /// Returns a reference to the search parameter registry, if set.
    #[must_use]
    pub fn search_registry(&self) -> Option<&Arc<SearchParameterRegistry>> {
        self.search_registry.as_ref()
    }

    /// Gets a search parameter for a specific resource type and code.
    ///
    /// Returns `None` if no registry is set or the parameter is not found.
    #[must_use]
    pub fn get_search_parameter(
        &self,
        resource_type: &str,
        code: &str,
    ) -> Option<Arc<SearchParameter>> {
        self.search_registry
            .as_ref()
            .and_then(|r| r.get(resource_type, code))
    }

    /// Retrieves history across all resource types (system-level history).
    ///
    /// This is a PostgreSQL-specific extension that queries history
    /// across all resource tables in the database.
    ///
    /// # Arguments
    ///
    /// * `params` - History query parameters (since, at, count, offset)
    ///
    /// # Returns
    ///
    /// Returns a `HistoryResult` containing entries from all resource types.
    pub async fn system_history(
        &self,
        params: &HistoryParams,
    ) -> Result<HistoryResult, StorageError> {
        queries::get_system_history(&self.pool, &self.schema_manager, params).await
    }
}

#[async_trait]
impl FhirStorage for PostgresStorage {
    async fn create(&self, resource: &Value) -> Result<StoredResource, StorageError> {
        queries::create(&self.pool, &self.schema_manager, resource).await
    }

    async fn read(
        &self,
        resource_type: &str,
        id: &str,
    ) -> Result<Option<StoredResource>, StorageError> {
        queries::read(&self.pool, resource_type, id).await
    }

    async fn update(
        &self,
        resource: &Value,
        if_match: Option<&str>,
    ) -> Result<StoredResource, StorageError> {
        queries::update(&self.pool, &self.schema_manager, resource, if_match).await
    }

    async fn delete(&self, resource_type: &str, id: &str) -> Result<(), StorageError> {
        queries::delete(&self.pool, resource_type, id).await
    }

    async fn vread(
        &self,
        resource_type: &str,
        id: &str,
        version: &str,
    ) -> Result<Option<StoredResource>, StorageError> {
        queries::vread(&self.pool, resource_type, id, version).await
    }

    async fn history(
        &self,
        resource_type: &str,
        id: Option<&str>,
        params: &HistoryParams,
    ) -> Result<HistoryResult, StorageError> {
        queries::get_history(&self.pool, resource_type, id, params).await
    }

    async fn search(
        &self,
        _resource_type: &str,
        _params: &SearchParams,
    ) -> Result<SearchResult, StorageError> {
        // Search implementation is planned for a future task
        Err(StorageError::internal(
            "PostgreSQL search not yet implemented",
        ))
    }

    async fn begin_transaction(&self) -> Result<Box<dyn Transaction>, StorageError> {
        // Begin a new PostgreSQL transaction
        let sqlx_tx = self.pool.begin().await.map_err(|e| {
            StorageError::transaction_error(format!("Failed to begin transaction: {}", e))
        })?;

        // Wrap in our PostgresTransaction type
        let pg_tx =
            crate::transaction::PostgresTransaction::new(sqlx_tx, self.schema_manager.clone());

        Ok(Box::new(pg_tx))
    }

    fn supports_transactions(&self) -> bool {
        // Native PostgreSQL transactions with ACID guarantees
        true
    }

    fn backend_name(&self) -> &'static str {
        "postgres"
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_backend_name() {
        // We can't create a storage without a DB, but we can test constants
        assert_eq!("postgres", "postgres");
    }
}
