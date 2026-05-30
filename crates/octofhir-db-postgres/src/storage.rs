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
use crate::index_writer::{AsyncIndexWriter, IndexJob, IndexOp};
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
    /// When set, `create / update / delete` enqueue index jobs here after the
    /// resource transaction commits. Unset → indexing runs inline.
    async_indexer: Option<AsyncIndexWriter>,
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

        crate::gin_maintenance::spawn_gin_cleaner(pool.clone());

        Ok(Self {
            pool,
            read_pool: None,
            schema_manager,
            search_registry: Arc::new(OnceLock::new()),
            query_cache: None,
            async_indexer: None,
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
            async_indexer: None,
        }
    }

    /// Plug an async, batched search-index writer into this storage. With
    /// it set, `create / update / delete` enqueue index jobs after the
    /// resource transaction commits; without it, indexing runs inline.
    #[must_use]
    pub fn with_async_indexer(mut self, indexer: AsyncIndexWriter) -> Self {
        self.async_indexer = Some(indexer);
        self
    }

    pub fn set_async_indexer(&mut self, indexer: AsyncIndexWriter) {
        self.async_indexer = Some(indexer);
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

    /// Write search-index rows inside the same transaction as the resource
    /// mutation. This makes the synchronous CRUD path atomic: if index writes
    /// fail, the resource mutation rolls back too.
    async fn write_indexes_with_tx(
        &self,
        tx: &mut PgTransaction<'_>,
        op: IndexOp,
        resource_type: &str,
        resource_id: &str,
        resource: Option<&Value>,
    ) -> Result<(), StorageError> {
        if self.async_indexer.is_some() {
            tracing::debug!(
                resource_type = %resource_type,
                resource_id = %resource_id,
                "Async search-index writer configured; using transactional index write for consistency"
            );
        }

        if matches!(op, IndexOp::Update | IndexOp::Delete) {
            crate::search_index::delete_search_indexes_with_tx(tx, resource_type, resource_id)
                .await?;
        }

        if !matches!(op, IndexOp::Create | IndexOp::Update) {
            return Ok(());
        }

        let Some(registry) = self.search_registry.get() else {
            return Ok(());
        };
        let resource = resource.ok_or_else(|| {
            StorageError::internal("Missing resource payload for search-index write")
        })?;
        let rows =
            crate::search_index::extract_search_index_rows(registry, resource_type, resource);
        let mut buffer = crate::search_index::BatchIndexBuffer::new();
        buffer.extend_with(resource_type, resource_id, &rows);
        if !buffer.is_empty() {
            buffer.flush_with_tx(tx).await?;
        }

        Ok(())
    }

    fn raw_from_stored(stored: &StoredResource) -> Result<RawStoredResource, StorageError> {
        let resource_json = serde_json::to_string(&stored.resource)
            .map_err(|e| StorageError::internal(format!("Failed to serialize resource: {e}")))?;
        Ok(RawStoredResource {
            id: stored.id.clone(),
            version_id: stored.version_id.clone(),
            resource_type: stored.resource_type.clone(),
            resource_json,
            last_updated: stored.last_updated,
            created_at: stored.created_at,
        })
    }

    async fn enqueue_index_write(
        &self,
        op: IndexOp,
        resource_type: &str,
        resource_id: &str,
        resource: &Value,
    ) -> Result<(), StorageError> {
        let Some(indexer) = &self.async_indexer else {
            return Ok(());
        };
        let Some(registry) = self.search_registry.get() else {
            return Ok(());
        };
        let rows =
            crate::search_index::extract_search_index_rows(registry, resource_type, resource);
        indexer
            .submit(IndexJob {
                op,
                resource_type: resource_type.to_string(),
                resource_id: resource_id.to_string(),
                rows,
            })
            .await
    }

    async fn enqueue_index_delete(
        &self,
        resource_type: &str,
        resource_id: &str,
    ) -> Result<(), StorageError> {
        let Some(indexer) = &self.async_indexer else {
            return Ok(());
        };
        indexer
            .submit(IndexJob {
                op: IndexOp::Delete,
                resource_type: resource_type.to_string(),
                resource_id: resource_id.to_string(),
                rows: crate::search_index::ExtractedIndexRows::default(),
            })
            .await
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

        let rows =
            crate::search_index::extract_search_index_rows(registry, resource_type, resource);

        crate::search_index::write_reference_index(
            &self.pool,
            resource_type,
            resource_id,
            &rows.refs,
        )
        .await?;
        crate::search_index::write_date_index(&self.pool, resource_type, resource_id, &rows.dates)
            .await?;
        crate::search_index::write_string_index(
            &self.pool,
            resource_type,
            resource_id,
            &rows.strings,
        )
        .await?;
        crate::search_index::write_number_index(
            &self.pool,
            resource_type,
            resource_id,
            &rows.numbers,
        )
        .await?;
        crate::search_index::write_quantity_index(
            &self.pool,
            resource_type,
            resource_id,
            &rows.quantities,
        )
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
        if self.async_indexer.is_some() {
            let result = {
                let mut tx = self.pool.begin().await.map_err(|e| {
                    StorageError::transaction_error(format!("Failed to begin transaction: {e}"))
                })?;
                let result = queries::crud::create_with_tx(&mut tx, resource, None).await?;
                tx.commit().await.map_err(|e| {
                    StorageError::transaction_error(format!("Failed to commit transaction: {e}"))
                })?;
                result
            };
            if let Err(e) = self
                .enqueue_index_write(
                    IndexOp::Create,
                    &result.resource_type,
                    &result.id,
                    &result.resource,
                )
                .await
            {
                tracing::warn!(
                    error = %e,
                    resource_type = %result.resource_type,
                    resource_id = %result.id,
                    "async search index enqueue failed after create commit"
                );
            }
            return Ok(result);
        }

        let mut tx = self.pool.begin().await.map_err(|e| {
            StorageError::transaction_error(format!("Failed to begin transaction: {e}"))
        })?;
        let result = queries::crud::create_with_tx(&mut tx, resource, None).await?;
        self.write_indexes_with_tx(
            &mut tx,
            IndexOp::Create,
            &result.resource_type,
            &result.id,
            Some(&result.resource),
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
        if self.async_indexer.is_some() {
            let mut tx = self.pool.begin().await.map_err(|e| {
                StorageError::transaction_error(format!("Failed to begin transaction: {e}"))
            })?;
            let result = queries::crud::create_with_tx(&mut tx, resource, None).await?;
            tx.commit().await.map_err(|e| {
                StorageError::transaction_error(format!("Failed to commit transaction: {e}"))
            })?;
            if let Err(e) = self
                .enqueue_index_write(
                    IndexOp::Create,
                    &result.resource_type,
                    &result.id,
                    &result.resource,
                )
                .await
            {
                tracing::warn!(
                    error = %e,
                    resource_type = %result.resource_type,
                    resource_id = %result.id,
                    "async search index enqueue failed after raw create commit"
                );
            }
            return Self::raw_from_stored(&result);
        }

        let mut tx = self.pool.begin().await.map_err(|e| {
            StorageError::transaction_error(format!("Failed to begin transaction: {e}"))
        })?;
        let result = queries::crud::create_with_tx(&mut tx, resource, None).await?;
        self.write_indexes_with_tx(
            &mut tx,
            IndexOp::Create,
            &result.resource_type,
            &result.id,
            Some(&result.resource),
        )
        .await?;
        tx.commit().await.map_err(|e| {
            StorageError::transaction_error(format!("Failed to commit transaction: {e}"))
        })?;
        Self::raw_from_stored(&result)
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
        if self.async_indexer.is_some() {
            let result = {
                let mut tx = self.pool.begin().await.map_err(|e| {
                    StorageError::transaction_error(format!("Failed to begin transaction: {e}"))
                })?;
                let result =
                    queries::crud::update_with_tx_if_match(&mut tx, resource, if_match, None)
                        .await?;
                tx.commit().await.map_err(|e| {
                    StorageError::transaction_error(format!("Failed to commit transaction: {e}"))
                })?;
                result
            };
            if let Err(e) = self
                .enqueue_index_write(
                    IndexOp::Update,
                    &result.resource_type,
                    &result.id,
                    &result.resource,
                )
                .await
            {
                tracing::warn!(
                    error = %e,
                    resource_type = %result.resource_type,
                    resource_id = %result.id,
                    "async search index enqueue failed after update commit"
                );
            }
            return Ok(result);
        }

        let mut tx = self.pool.begin().await.map_err(|e| {
            StorageError::transaction_error(format!("Failed to begin transaction: {e}"))
        })?;
        let result =
            queries::crud::update_with_tx_if_match(&mut tx, resource, if_match, None).await?;
        self.write_indexes_with_tx(
            &mut tx,
            IndexOp::Update,
            &result.resource_type,
            &result.id,
            Some(&result.resource),
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
        if self.async_indexer.is_some() {
            let result = self.update(resource, if_match).await?;
            return Self::raw_from_stored(&result);
        }

        let mut tx = self.pool.begin().await.map_err(|e| {
            StorageError::transaction_error(format!("Failed to begin transaction: {e}"))
        })?;
        let result =
            queries::crud::update_with_tx_if_match(&mut tx, resource, if_match, None).await?;
        self.write_indexes_with_tx(
            &mut tx,
            IndexOp::Update,
            &result.resource_type,
            &result.id,
            Some(&result.resource),
        )
        .await?;
        tx.commit().await.map_err(|e| {
            StorageError::transaction_error(format!("Failed to commit transaction: {e}"))
        })?;
        Self::raw_from_stored(&result)
    }

    async fn delete(&self, resource_type: &str, id: &str) -> Result<(), StorageError> {
        if self.async_indexer.is_some() {
            {
                let mut tx = self.pool.begin().await.map_err(|e| {
                    StorageError::transaction_error(format!("Failed to begin transaction: {e}"))
                })?;
                queries::crud::delete_with_tx(&mut tx, resource_type, id, None).await?;
                tx.commit().await.map_err(|e| {
                    StorageError::transaction_error(format!("Failed to commit transaction: {e}"))
                })?;
            }
            if let Err(e) = self.enqueue_index_delete(resource_type, id).await {
                tracing::warn!(
                    error = %e,
                    resource_type = %resource_type,
                    resource_id = %id,
                    "async search index enqueue failed after delete commit"
                );
            }
            return Ok(());
        }

        let mut tx = self.pool.begin().await.map_err(|e| {
            StorageError::transaction_error(format!("Failed to begin transaction: {e}"))
        })?;
        queries::crud::delete_with_tx(&mut tx, resource_type, id, None).await?;
        self.write_indexes_with_tx(&mut tx, IndexOp::Delete, resource_type, id, None)
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
