//! Execute shared search SQL on a pool or the checked-out Bundle connection.
use chrono::{DateTime, Utc};
use octofhir_search::BuiltQuery;
use octofhir_storage::{IncludeLimits, StorageError};
use sqlx_core::{query_as::query_as, query_scalar::query_scalar, sql_str::AssertSqlSafe};
use sqlx_postgres::{PgConnection, PgPool};
use tokio::sync::Mutex;

use super::{BindAllParams, BindAllParamsRaw, checked_search_count, collect_include_rows};

pub(super) type InlineRow = (String, String, i64, DateTime<Utc>, DateTime<Utc>);
pub(super) type SystemRow = (String, String, i64, DateTime<Utc>, DateTime<Utc>, String);

pub(super) enum SearchSource<'a> {
    Pool(&'a PgPool),
    Connection(Mutex<&'a mut PgConnection>),
}

impl SearchSource<'_> {
    pub(super) async fn explain(
        &self,
        query: &BuiltQuery,
        analyze: bool,
    ) -> Result<serde_json::Value, StorageError> {
        match self {
            Self::Pool(pool) => super::explain_built_search_query_json(pool, query, analyze).await,
            Self::Connection(connection) => {
                let mut guard = connection.lock().await;
                query_scalar(AssertSqlSafe(super::build_explain_sql(&query.sql, analyze)))
                    .bind_all_params(&query.params)
                    .fetch_one(&mut **guard)
                    .await
                    .map_err(|e| StorageError::internal(format!("Search EXPLAIN failed: {e}")))
            }
        }
    }

    pub(super) async fn count(&self, query: &BuiltQuery) -> Result<u32, StorageError> {
        let sql = query_scalar(AssertSqlSafe(query.sql.clone())).bind_all_params(&query.params);
        let count = match self {
            Self::Pool(pool) => sql.fetch_one(*pool).await,
            Self::Connection(connection) => {
                let mut guard = connection.lock().await;
                sql.fetch_one(&mut **guard).await
            }
        }
        .map_err(|e| StorageError::internal(format!("Count query failed: {e}")))?;
        checked_search_count(count)
    }

    pub(super) async fn system_rows(
        &self,
        query: &BuiltQuery,
    ) -> Result<Vec<SystemRow>, StorageError> {
        let sql = query_as(AssertSqlSafe(query.sql.clone())).bind_all_params_raw(&query.params);
        match self {
            Self::Pool(pool) => sql.fetch_all(*pool).await,
            Self::Connection(connection) => {
                let mut guard = connection.lock().await;
                sql.fetch_all(&mut **guard).await
            }
        }
        .map_err(|e| StorageError::internal(format!("System search query failed: {e}")))
    }

    pub(super) async fn inline_rows(
        &self,
        query: &BuiltQuery,
        limits: IncludeLimits,
    ) -> Result<Vec<InlineRow>, StorageError> {
        let sql = query_as(AssertSqlSafe(query.sql.clone())).bind_all_params_raw(&query.params);
        match self {
            Self::Pool(pool) => collect_include_rows(sql.fetch(*pool), limits).await,
            Self::Connection(connection) => {
                let mut guard = connection.lock().await;
                collect_include_rows(sql.fetch(&mut **guard), limits).await
            }
        }
    }
}
