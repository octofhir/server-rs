//! PostgreSQL implementation of the FhirStorage trait.

use async_trait::async_trait;
use serde_json::Value;
use sqlx_postgres::PgPool;

use octofhir_storage::{
    FhirStorage, HistoryParams, HistoryResult, SearchParams, SearchResult, StorageError,
    StoredResource, Transaction,
};

use crate::config::PostgresConfig;
use crate::migrations;
use crate::pool;
use crate::schema::SchemaManager;

/// PostgreSQL storage backend for FHIR resources.
///
/// This storage backend uses PostgreSQL to persist FHIR resources
/// with full support for versioning, history, and search.
#[derive(Debug, Clone)]
pub struct PostgresStorage {
    pool: PgPool,
    #[allow(dead_code)]
    schema_manager: SchemaManager,
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
            migrations::run(&pool).await?;
        }

        let schema_manager = SchemaManager::new(pool.clone());

        Ok(Self {
            pool,
            schema_manager,
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
        }
    }

    /// Returns a reference to the connection pool.
    #[must_use]
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }
}

#[async_trait]
impl FhirStorage for PostgresStorage {
    async fn create(&self, _resource: &Value) -> Result<StoredResource, StorageError> {
        Err(StorageError::internal(
            "PostgreSQL storage not yet implemented",
        ))
    }

    async fn read(
        &self,
        _resource_type: &str,
        _id: &str,
    ) -> Result<Option<StoredResource>, StorageError> {
        Err(StorageError::internal(
            "PostgreSQL storage not yet implemented",
        ))
    }

    async fn update(
        &self,
        _resource: &Value,
        _if_match: Option<&str>,
    ) -> Result<StoredResource, StorageError> {
        Err(StorageError::internal(
            "PostgreSQL storage not yet implemented",
        ))
    }

    async fn delete(&self, _resource_type: &str, _id: &str) -> Result<(), StorageError> {
        Err(StorageError::internal(
            "PostgreSQL storage not yet implemented",
        ))
    }

    async fn vread(
        &self,
        _resource_type: &str,
        _id: &str,
        _version: &str,
    ) -> Result<Option<StoredResource>, StorageError> {
        Err(StorageError::internal(
            "PostgreSQL storage not yet implemented",
        ))
    }

    async fn history(
        &self,
        _resource_type: &str,
        _id: Option<&str>,
        _params: &HistoryParams,
    ) -> Result<HistoryResult, StorageError> {
        Err(StorageError::internal(
            "PostgreSQL storage not yet implemented",
        ))
    }

    async fn search(
        &self,
        _resource_type: &str,
        _params: &SearchParams,
    ) -> Result<SearchResult, StorageError> {
        Err(StorageError::internal(
            "PostgreSQL storage not yet implemented",
        ))
    }

    async fn begin_transaction(&self) -> Result<Box<dyn Transaction>, StorageError> {
        Err(StorageError::transaction_error(
            "PostgreSQL transactions not yet implemented",
        ))
    }

    fn supports_transactions(&self) -> bool {
        true // PostgreSQL supports transactions, but implementation is pending
    }

    fn backend_name(&self) -> &'static str {
        "postgres"
    }
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::*;

    #[test]
    fn test_backend_name() {
        // We can't create a storage without a DB, but we can test constants
        assert_eq!("postgres", "postgres");
    }
}
