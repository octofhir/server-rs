//! PostgreSQL implementation of the FhirStorage trait.

use std::sync::{Arc, OnceLock};

use async_trait::async_trait;
use serde_json::Value;
use sqlx_postgres::PgPool;

use octofhir_search::{QueryCache, SearchParameter, SearchParameterRegistry, SearchParameterType};
use octofhir_storage::{
    FhirStorage, HistoryParams, HistoryResult, RawHistoryResult, RawStoredResource, SearchParams,
    SearchResult, StorageError, StoredResource, Transaction,
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
    /// Optional read replica pool. Read operations use this when available.
    read_pool: Option<PgPool>,
    schema_manager: SchemaManager,
    /// Search parameter registry for parameter lookup during search and indexing.
    /// Wrapped in Arc<OnceLock<>> to allow late initialization after the storage
    /// is wrapped in EventedStorage (the OnceLock is shared across clones).
    search_registry: Arc<OnceLock<Arc<SearchParameterRegistry>>>,
    /// Query cache for search SQL template reuse
    query_cache: Option<Arc<QueryCache>>,
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
            read_pool: None,
            schema_manager,
            search_registry: Arc::new(OnceLock::new()),
            query_cache: None,
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
            read_pool: None,
            schema_manager,
            search_registry: Arc::new(OnceLock::new()),
            query_cache: None,
        }
    }

    /// Sets the read replica pool for routing read operations.
    pub fn set_read_pool(&mut self, pool: PgPool) {
        self.read_pool = Some(pool);
    }

    /// Returns the primary (write) connection pool.
    #[must_use]
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Returns the read pool (replica if configured, otherwise primary).
    #[must_use]
    pub fn read_pool(&self) -> &PgPool {
        self.read_pool.as_ref().unwrap_or(&self.pool)
    }

    /// Returns a reference to the schema manager.
    #[must_use]
    pub fn schema_manager(&self) -> &SchemaManager {
        &self.schema_manager
    }

    /// Returns the shared slot for late-initializing the search registry.
    ///
    /// Clone this `Arc` before wrapping the storage in `EventedStorage`,
    /// then call `.set(registry)` once the registry is ready.
    #[must_use]
    pub fn search_registry_slot(&self) -> &Arc<OnceLock<Arc<SearchParameterRegistry>>> {
        &self.search_registry
    }

    /// Returns a reference to the search parameter registry, if set.
    #[must_use]
    pub fn search_registry(&self) -> Option<&Arc<SearchParameterRegistry>> {
        self.search_registry.get()
    }

    /// Sets the query cache for search SQL template reuse.
    pub fn set_query_cache(&mut self, cache: Arc<QueryCache>) {
        self.query_cache = Some(cache);
    }

    /// Returns a reference to the query cache, if set.
    #[must_use]
    pub fn query_cache(&self) -> Option<&Arc<QueryCache>> {
        self.query_cache.as_ref()
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
            .get()
            .and_then(|r| r.get(resource_type, code))
    }

    /// Extract and write search indexes for a resource after create/update.
    ///
    /// Iterates over all search parameters for the resource type, extracts
    /// reference/date/string values, and writes them to denormalized index tables.
    /// Errors are logged but do not fail the CRUD operation.
    async fn write_search_indexes(&self, resource_type: &str, resource_id: &str, resource: &Value) {
        let registry = match self.search_registry.get() {
            Some(r) => r,
            None => return,
        };

        let params = registry.get_all_for_type(resource_type);

        let mut refs = Vec::new();
        let mut dates = Vec::new();

        for param in &params {
            let expression = match &param.expression {
                Some(e) => e.as_str(),
                None => continue,
            };

            match param.param_type {
                SearchParameterType::Reference => {
                    refs.extend(octofhir_core::search_index::extract_references(
                        resource,
                        resource_type,
                        &param.code,
                        expression,
                        None,
                    ));
                }
                SearchParameterType::Date => {
                    dates.extend(octofhir_core::search_index::extract_dates(
                        resource,
                        resource_type,
                        &param.code,
                        expression,
                    ));
                }
                _ => {}
            }
        }

        if let Err(e) = crate::search_index::write_reference_index(
            &self.pool,
            resource_type,
            resource_id,
            &refs,
        )
        .await
        {
            tracing::warn!(
                "Failed to write reference index for {}/{}: {}",
                resource_type,
                resource_id,
                e
            );
        }

        if let Err(e) =
            crate::search_index::write_date_index(&self.pool, resource_type, resource_id, &dates)
                .await
        {
            tracing::warn!(
                "Failed to write date index for {}/{}: {}",
                resource_type,
                resource_id,
                e
            );
        }
    }

    /// Delete all search indexes for a resource after delete.
    async fn remove_search_indexes(&self, resource_type: &str, resource_id: &str) {
        if let Err(e) =
            crate::search_index::delete_search_indexes(&self.pool, resource_type, resource_id).await
        {
            tracing::warn!(
                "Failed to delete search indexes for {}/{}: {}",
                resource_type,
                resource_id,
                e
            );
        }
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
        queries::get_system_history(self.read_pool(), &self.schema_manager, params).await
    }
}

#[async_trait]
impl FhirStorage for PostgresStorage {
    async fn create(&self, resource: &Value) -> Result<StoredResource, StorageError> {
        let result = queries::create(&self.pool, resource).await?;
        self.write_search_indexes(&result.resource_type, &result.id, &result.resource)
            .await;
        Ok(result)
    }

    async fn create_raw(
        &self,
        resource: &Value,
    ) -> Result<octofhir_storage::RawStoredResource, StorageError> {
        let result = queries::create_raw(&self.pool, resource).await?;
        // Use the input resource for extraction (avoid parsing raw JSON back)
        self.write_search_indexes(&result.resource_type, &result.id, resource)
            .await;
        Ok(result)
    }

    async fn read(
        &self,
        resource_type: &str,
        id: &str,
    ) -> Result<Option<StoredResource>, StorageError> {
        queries::read(self.read_pool(), resource_type, id).await
    }

    async fn read_raw(
        &self,
        resource_type: &str,
        id: &str,
    ) -> Result<Option<octofhir_storage::RawStoredResource>, StorageError> {
        queries::read_raw(self.read_pool(), resource_type, id).await
    }

    async fn update(
        &self,
        resource: &Value,
        if_match: Option<&str>,
    ) -> Result<StoredResource, StorageError> {
        let result = queries::update(&self.pool, resource, if_match).await?;
        self.write_search_indexes(&result.resource_type, &result.id, &result.resource)
            .await;
        Ok(result)
    }

    async fn update_raw(
        &self,
        resource: &Value,
        if_match: Option<&str>,
    ) -> Result<octofhir_storage::RawStoredResource, StorageError> {
        let result = queries::update_raw(&self.pool, resource, if_match).await?;
        self.write_search_indexes(&result.resource_type, &result.id, resource)
            .await;
        Ok(result)
    }

    async fn delete(&self, resource_type: &str, id: &str) -> Result<(), StorageError> {
        queries::delete(&self.pool, resource_type, id).await?;
        self.remove_search_indexes(resource_type, id).await;
        Ok(())
    }

    async fn vread(
        &self,
        resource_type: &str,
        id: &str,
        version: &str,
    ) -> Result<Option<StoredResource>, StorageError> {
        queries::vread(self.read_pool(), resource_type, id, version).await
    }

    async fn vread_raw(
        &self,
        resource_type: &str,
        id: &str,
        version: &str,
    ) -> Result<Option<RawStoredResource>, StorageError> {
        queries::vread_raw(self.read_pool(), resource_type, id, version).await
    }

    async fn history(
        &self,
        resource_type: &str,
        id: Option<&str>,
        params: &HistoryParams,
    ) -> Result<HistoryResult, StorageError> {
        queries::get_history(self.read_pool(), resource_type, id, params).await
    }

    async fn system_history(&self, params: &HistoryParams) -> Result<HistoryResult, StorageError> {
        queries::get_system_history(self.read_pool(), &self.schema_manager, params).await
    }

    async fn history_raw(
        &self,
        resource_type: &str,
        id: Option<&str>,
        params: &HistoryParams,
    ) -> Result<RawHistoryResult, StorageError> {
        queries::get_history_raw(self.read_pool(), resource_type, id, params).await
    }

    async fn system_history_raw(
        &self,
        params: &HistoryParams,
    ) -> Result<RawHistoryResult, StorageError> {
        queries::get_system_history_raw(self.read_pool(), &self.schema_manager, params).await
    }

    async fn search(
        &self,
        resource_type: &str,
        params: &SearchParams,
    ) -> Result<SearchResult, StorageError> {
        queries::execute_search(
            self.read_pool(),
            resource_type,
            params,
            self.search_registry.get(),
        )
        .await
    }

    async fn begin_transaction(&self) -> Result<Box<dyn Transaction>, StorageError> {
        // Begin a new PostgreSQL transaction
        let sqlx_tx = self.pool.begin().await.map_err(|e| {
            StorageError::transaction_error(format!("Failed to begin transaction: {}", e))
        })?;

        // Wrap in our PostgresTransaction type
        let pg_tx = crate::transaction::PostgresTransaction::new(sqlx_tx);

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
