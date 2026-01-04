//! Operation Storage
//!
//! PostgreSQL storage for operations registry.

use async_trait::async_trait;
use octofhir_core::{AppReference, OperationDefinition};
use sqlx_core::error::Error as SqlxError;
use sqlx_core::query::query;
use sqlx_core::row::Row;
use sqlx_postgres::PgPool;

/// Error type for operation storage
#[derive(Debug, thiserror::Error)]
pub enum OperationStorageError {
    #[error("Database error: {0}")]
    Database(#[from] SqlxError),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

/// Trait for operation storage backends
#[async_trait]
pub trait OperationStorage: Send + Sync {
    /// Upsert all operations (insert or update)
    async fn upsert_all(
        &self,
        operations: &[OperationDefinition],
    ) -> Result<(), OperationStorageError>;

    /// Get all operations
    async fn list_all(&self) -> Result<Vec<OperationDefinition>, OperationStorageError>;

    /// Get operations by category
    async fn list_by_category(
        &self,
        category: &str,
    ) -> Result<Vec<OperationDefinition>, OperationStorageError>;

    /// Get operations by module
    async fn list_by_module(
        &self,
        module: &str,
    ) -> Result<Vec<OperationDefinition>, OperationStorageError>;

    /// Get public operations
    async fn list_public(&self) -> Result<Vec<OperationDefinition>, OperationStorageError>;

    /// Get a single operation by ID
    async fn get(&self, id: &str) -> Result<Option<OperationDefinition>, OperationStorageError>;

    /// Check if an operation is public
    async fn is_public(&self, id: &str) -> Result<bool, OperationStorageError>;

    /// Delete operations not in the provided list (cleanup stale operations)
    async fn delete_not_in(&self, ids: &[String]) -> Result<u64, OperationStorageError>;

    /// Update a single operation (partial update)
    async fn update(
        &self,
        id: &str,
        update: OperationUpdate,
    ) -> Result<Option<OperationDefinition>, OperationStorageError>;
}

/// Partial update for an operation
#[derive(Debug, Clone, Default)]
pub struct OperationUpdate {
    /// Update the public flag
    pub public: Option<bool>,
    /// Update the description
    pub description: Option<String>,
}

/// PostgreSQL implementation of operation storage
pub struct PostgresOperationStorage {
    pool: PgPool,
}

impl PostgresOperationStorage {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Convert a row to OperationDefinition
    fn row_to_operation(
        row: &sqlx_postgres::PgRow,
    ) -> Result<OperationDefinition, OperationStorageError> {
        let methods_json: serde_json::Value = row.try_get("methods")?;
        let methods: Vec<String> = serde_json::from_value(methods_json)?;

        // Build AppReference if app_id is present
        let app_id: Option<String> = row.try_get("app_id")?;
        let app_name: Option<String> = row.try_get("app_name")?;
        let app = match (app_id, app_name) {
            (Some(id), Some(name)) => Some(AppReference { id, name }),
            _ => None,
        };

        Ok(OperationDefinition {
            id: row.try_get("id")?,
            name: row.try_get("name")?,
            description: row.try_get("description")?,
            category: row.try_get("category")?,
            methods,
            path_pattern: row.try_get("path_pattern")?,
            public: row.try_get("public")?,
            module: row.try_get("module")?,
            app,
        })
    }
}

#[async_trait]
impl OperationStorage for PostgresOperationStorage {
    async fn upsert_all(
        &self,
        operations: &[OperationDefinition],
    ) -> Result<(), OperationStorageError> {
        // Use a transaction for batch upsert
        let mut tx = self.pool.begin().await?;

        for op in operations {
            let methods_json = serde_json::to_value(&op.methods)?;
            let (app_id, app_name) = match &op.app {
                Some(app) => (Some(app.id.clone()), Some(app.name.clone())),
                None => (None, None),
            };

            query(
                r#"
                INSERT INTO operations (id, name, description, category, methods, path_pattern, public, module, app_id, app_name)
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
                ON CONFLICT (id) DO UPDATE SET
                    name = EXCLUDED.name,
                    description = EXCLUDED.description,
                    category = EXCLUDED.category,
                    methods = EXCLUDED.methods,
                    path_pattern = EXCLUDED.path_pattern,
                    public = EXCLUDED.public,
                    module = EXCLUDED.module,
                    app_id = EXCLUDED.app_id,
                    app_name = EXCLUDED.app_name,
                    updated_at = NOW()
                "#,
            )
            .bind(&op.id)
            .bind(&op.name)
            .bind(&op.description)
            .bind(&op.category)
            .bind(&methods_json)
            .bind(&op.path_pattern)
            .bind(op.public)
            .bind(&op.module)
            .bind(&app_id)
            .bind(&app_name)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    async fn list_all(&self) -> Result<Vec<OperationDefinition>, OperationStorageError> {
        let rows = query(
            r#"
            SELECT id, name, description, category, methods, path_pattern, public, module, app_id, app_name
            FROM operations
            ORDER BY category, id
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        rows.iter().map(Self::row_to_operation).collect()
    }

    async fn list_by_category(
        &self,
        category: &str,
    ) -> Result<Vec<OperationDefinition>, OperationStorageError> {
        let rows = query(
            r#"
            SELECT id, name, description, category, methods, path_pattern, public, module, app_id, app_name
            FROM operations
            WHERE category = $1
            ORDER BY id
            "#,
        )
        .bind(category)
        .fetch_all(&self.pool)
        .await?;

        rows.iter().map(Self::row_to_operation).collect()
    }

    async fn list_by_module(
        &self,
        module: &str,
    ) -> Result<Vec<OperationDefinition>, OperationStorageError> {
        let rows = query(
            r#"
            SELECT id, name, description, category, methods, path_pattern, public, module, app_id, app_name
            FROM operations
            WHERE module = $1
            ORDER BY id
            "#,
        )
        .bind(module)
        .fetch_all(&self.pool)
        .await?;

        rows.iter().map(Self::row_to_operation).collect()
    }

    async fn list_public(&self) -> Result<Vec<OperationDefinition>, OperationStorageError> {
        let rows = query(
            r#"
            SELECT id, name, description, category, methods, path_pattern, public, module, app_id, app_name
            FROM operations
            WHERE public = true
            ORDER BY category, id
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        rows.iter().map(Self::row_to_operation).collect()
    }

    async fn get(&self, id: &str) -> Result<Option<OperationDefinition>, OperationStorageError> {
        let row = query(
            r#"
            SELECT id, name, description, category, methods, path_pattern, public, module, app_id, app_name
            FROM operations
            WHERE id = $1
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some(ref r) => Ok(Some(Self::row_to_operation(r)?)),
            None => Ok(None),
        }
    }

    async fn is_public(&self, id: &str) -> Result<bool, OperationStorageError> {
        let row = query("SELECT public FROM operations WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;

        Ok(row
            .map(|r| r.try_get::<bool, _>("public").unwrap_or(false))
            .unwrap_or(false))
    }

    async fn delete_not_in(&self, ids: &[String]) -> Result<u64, OperationStorageError> {
        let result = query("DELETE FROM operations WHERE id != ALL($1)")
            .bind(ids)
            .execute(&self.pool)
            .await?;

        Ok(result.rows_affected())
    }

    async fn update(
        &self,
        id: &str,
        update: OperationUpdate,
    ) -> Result<Option<OperationDefinition>, OperationStorageError> {
        // Build dynamic update query based on provided fields
        let mut set_clauses = vec!["updated_at = NOW()".to_string()];
        let mut param_index = 2; // $1 is the id

        if update.public.is_some() {
            set_clauses.push(format!("public = ${}", param_index));
            param_index += 1;
        }
        if update.description.is_some() {
            set_clauses.push(format!("description = ${}", param_index));
        }

        let sql = format!(
            "UPDATE operations SET {} WHERE id = $1 RETURNING id, name, description, category, methods, path_pattern, public, module, app_id, app_name",
            set_clauses.join(", ")
        );

        let mut query_builder = query(&sql).bind(id);

        // Bind parameters in order
        if let Some(public) = update.public {
            query_builder = query_builder.bind(public);
        }
        if let Some(ref description) = update.description {
            query_builder = query_builder.bind(description);
        }

        let row = query_builder.fetch_optional(&self.pool).await?;

        match row {
            Some(ref r) => Ok(Some(Self::row_to_operation(r)?)),
            None => Ok(None),
        }
    }
}
