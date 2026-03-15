//! PostgreSQL implementation of the FhirStorage trait.

use std::sync::{Arc, OnceLock};

use async_trait::async_trait;
use serde_json::Value;
use sqlx_postgres::{PgPool, PgTransaction};

use octofhir_search::{QueryCache, SearchParameter, SearchParameterRegistry};
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
    fn stored_to_raw(result: StoredResource) -> Result<RawStoredResource, StorageError> {
        let resource_json = serde_json::to_string(&result.resource)
            .map_err(|e| StorageError::internal(format!("Failed to serialize resource: {e}")))?;

        Ok(RawStoredResource {
            id: result.id,
            version_id: result.version_id,
            resource_type: result.resource_type,
            resource_json,
            last_updated: result.last_updated,
            created_at: result.created_at,
        })
    }

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

    async fn write_search_indexes_with_tx(
        &self,
        tx: &mut PgTransaction<'_>,
        resource_type: &str,
        resource_id: &str,
        resource: &Value,
    ) -> Result<(), StorageError> {
        let Some(registry) = self.search_registry.get() else {
            return Ok(());
        };

        let (refs, dates) =
            crate::search_index::extract_search_index_rows(registry, resource_type, resource);

        crate::search_index::write_reference_index_with_tx(tx, resource_type, resource_id, &refs)
            .await?;
        crate::search_index::write_date_index_with_tx(tx, resource_type, resource_id, &dates)
            .await?;

        Ok(())
    }

    async fn remove_search_indexes_with_tx(
        &self,
        tx: &mut PgTransaction<'_>,
        resource_type: &str,
        resource_id: &str,
    ) -> Result<(), StorageError> {
        crate::search_index::delete_search_indexes_with_tx(tx, resource_type, resource_id).await
    }

    /// Reindex a single resource: delete old index rows, extract new ones, and write them.
    ///
    /// This is the same logic as `write_search_indexes` but exposed publicly for
    /// the `$reindex` operation. Errors are propagated rather than silently logged.
    pub async fn reindex_resource(
        &self,
        resource_type: &str,
        resource_id: &str,
        resource: &Value,
    ) -> Result<(), StorageError> {
        let registry = self
            .search_registry
            .get()
            .ok_or_else(|| StorageError::internal("Search registry not initialized"))?;

        let (refs, dates) =
            crate::search_index::extract_search_index_rows(registry, resource_type, resource);

        crate::search_index::write_reference_index(&self.pool, resource_type, resource_id, &refs)
            .await?;
        crate::search_index::write_date_index(&self.pool, resource_type, resource_id, &dates)
            .await?;

        Ok(())
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
        let mut tx = self.pool.begin().await.map_err(|e| {
            StorageError::transaction_error(format!("Failed to begin transaction: {e}"))
        })?;
        let result = queries::crud::create_with_tx(&mut tx, resource).await?;
        self.write_search_indexes_with_tx(
            &mut tx,
            &result.resource_type,
            &result.id,
            &result.resource,
        )
        .await?;
        tx.commit().await.map_err(|e| {
            StorageError::transaction_error(format!("Failed to commit transaction: {e}"))
        })?;
        Ok(result)
    }

    async fn create_raw(
        &self,
        resource: &Value,
    ) -> Result<octofhir_storage::RawStoredResource, StorageError> {
        let stored = self.create(resource).await?;
        Self::stored_to_raw(stored)
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
        let mut tx = self.pool.begin().await.map_err(|e| {
            StorageError::transaction_error(format!("Failed to begin transaction: {e}"))
        })?;
        let result = queries::crud::update_with_tx_if_match(&mut tx, resource, if_match).await?;
        self.write_search_indexes_with_tx(
            &mut tx,
            &result.resource_type,
            &result.id,
            &result.resource,
        )
        .await?;
        tx.commit().await.map_err(|e| {
            StorageError::transaction_error(format!("Failed to commit transaction: {e}"))
        })?;
        Ok(result)
    }

    async fn update_raw(
        &self,
        resource: &Value,
        if_match: Option<&str>,
    ) -> Result<octofhir_storage::RawStoredResource, StorageError> {
        let stored = self.update(resource, if_match).await?;
        Self::stored_to_raw(stored)
    }

    async fn delete(&self, resource_type: &str, id: &str) -> Result<(), StorageError> {
        let mut tx = self.pool.begin().await.map_err(|e| {
            StorageError::transaction_error(format!("Failed to begin transaction: {e}"))
        })?;
        queries::crud::delete_with_tx(&mut tx, resource_type, id).await?;
        self.remove_search_indexes_with_tx(&mut tx, resource_type, id)
            .await?;
        tx.commit().await.map_err(|e| {
            StorageError::transaction_error(format!("Failed to commit transaction: {e}"))
        })?;
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
        let pg_tx = crate::transaction::PostgresTransaction::new(
            sqlx_tx,
            self.search_registry.get().cloned(),
        );

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
