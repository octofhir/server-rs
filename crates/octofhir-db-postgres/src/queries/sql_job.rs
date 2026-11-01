//! Runs the SQL of a SQL on FHIR SQLQuery or SQLView (`$sql-run`).
//!
//! One job is one transaction with one snapshot (`REPEATABLE READ`):
//!
//! 1. Each dependency becomes a temporary table (`pg_temp._sof_dep_N`; the
//!    SQL reads it under its label, a CTE), created as the server's role: a ViewDefinition's generated SQL over the caller's
//!    authorized source, or rows the server computed (a view that runs in
//!    memory). `SELECT` on it is granted to [`SOF_QUERY_ROLE`].
//! 2. The transaction turns read-only, gets a statement timeout, and switches
//!    to [`SOF_QUERY_ROLE`], a role without privileges on any table.
//! 3. The author's SQL runs there (SQLViews as CTEs in front of it), with its
//!    parameters bound, never interpolated. It can read the job's temporary
//!    tables and nothing else; the server checked its syntax first
//!    (functions, relations) so it cannot leave the role either.
//! 4. The transaction rolls back, dropping the temporary tables.

use futures_util::StreamExt;
use serde_json::{Map, Value};
use sqlx_core::column::Column;
use sqlx_core::executor::Executor;
use sqlx_core::query::{Query, query};
use sqlx_core::query_scalar::{QueryScalar, query_scalar};
use sqlx_core::sql_str::{AssertSqlSafe, SqlSafeStr};
use sqlx_core::statement::Statement;
use sqlx_core::type_info::TypeInfo;
use sqlx_postgres::{PgArguments, PgPool, PgTransaction, Postgres};

use octofhir_search::{BuiltQuery, SqlValue};
use octofhir_storage::StorageError;

use crate::error::is_undefined_table;

/// The role a job's SQL runs as: no login, no privileges on any table.
pub const SOF_QUERY_ROLE: &str = "octofhir_sof_query";

/// Make sure [`SOF_QUERY_ROLE`] exists and the server's role may switch to
/// it. Returns `false` (and the reason in the log) when the database user
/// cannot create or grant it; SQL subjects are then refused.
pub async fn ensure_sof_query_role(pool: &PgPool) -> bool {
    let create = format!(
        "DO $$ BEGIN \
           IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = '{SOF_QUERY_ROLE}') THEN \
             CREATE ROLE {SOF_QUERY_ROLE} NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT; \
           END IF; \
         END $$"
    );
    if let Err(error) = query(AssertSqlSafe(create)).execute(pool).await {
        tracing::warn!(%error, role = SOF_QUERY_ROLE, "SQL on FHIR: cannot create the query role; SQLQuery and SQLView subjects are disabled");
        return false;
    }
    // A superuser may SET ROLE without membership; anyone else needs it.
    let member = query_scalar::<_, bool>(AssertSqlSafe(format!(
        "SELECT pg_has_role(current_user, '{SOF_QUERY_ROLE}', 'MEMBER')"
    )))
    .fetch_one(pool)
    .await
    .unwrap_or(false);
    if !member
        && let Err(error) = query(AssertSqlSafe(format!(
            "GRANT {SOF_QUERY_ROLE} TO CURRENT_USER"
        )))
        .execute(pool)
        .await
    {
        tracing::warn!(%error, role = SOF_QUERY_ROLE, "SQL on FHIR: cannot grant the query role; SQLQuery and SQLView subjects are disabled");
        return false;
    }
    true
}

/// Where a dependency's rows come from.
pub enum DependencySource {
    /// A ViewDefinition run as SQL: `select_sql` reads `source_name`, the
    /// caller's authorized rows of its resource type (`source`).
    View {
        source: BuiltQuery,
        source_name: String,
        select_sql: String,
    },
    /// Rows the server computed, as a JSON array of objects.
    Rows(Value),
}

/// A dependency, materialized as the temporary table `name`.
pub struct DependencyTable {
    /// The table name the SQL uses (`relatedArtifact.label`); a validated
    /// SQL identifier.
    pub name: String,
    /// Column names and PostgreSQL types, in order.
    pub columns: Vec<(String, &'static str)>,
    pub source: DependencySource,
}

/// One `$sql-run` job.
pub struct SqlJob {
    pub tables: Vec<DependencyTable>,
    /// SQLViews, as CTEs in front of the query (dependencies first).
    pub views: Vec<(String, String)>,
    /// The checked SQL, with `$n` placeholders.
    pub query: String,
    pub params: Vec<SqlValue>,
    /// Rows to return at most.
    pub limit: usize,
    /// Bytes of row JSON to return at most; more is an error.
    pub max_bytes: usize,
    pub statement_timeout_ms: u64,
}

/// A result column: its name and PostgreSQL type name (`int4`, `text`, ...).
#[derive(Clone, Debug)]
pub struct SqlJobColumn {
    pub name: String,
    pub sql_type: String,
}

pub struct SqlJobResult {
    pub columns: Vec<SqlJobColumn>,
    /// Rows as JSON objects in column order (PostgreSQL `row_to_json`).
    pub rows: Vec<Value>,
}

#[derive(Debug, thiserror::Error)]
pub enum SqlJobError {
    /// The author's SQL failed (syntax, types, a missing column, a timeout).
    #[error("{0}")]
    Query(String),
    /// The result exceeds the byte budget.
    #[error("{0}")]
    TooLarge(String),
    #[error(transparent)]
    Storage(#[from] StorageError),
}

fn quote(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// Run `job`; see the module documentation for the steps.
pub async fn run_sql_job(pool: &PgPool, job: SqlJob) -> Result<SqlJobResult, SqlJobError> {
    let mut tx = pool.begin().await.map_err(storage)?;
    query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
        .execute(&mut *tx)
        .await
        .map_err(storage)?;
    query(AssertSqlSafe(format!(
        "SET LOCAL statement_timeout = {}",
        job.statement_timeout_ms
    )))
    .execute(&mut *tx)
    .await
    .map_err(storage)?;
    for (index, table) in job.tables.iter().enumerate() {
        create_dependency_table(&mut tx, &temporary_table(index), table).await?;
    }
    for statement in [
        "SET LOCAL transaction_read_only = on".to_owned(),
        format!("SET LOCAL ROLE {SOF_QUERY_ROLE}"),
    ] {
        query(AssertSqlSafe(statement))
            .execute(&mut *tx)
            .await
            .map_err(storage)?;
    }

    // The SQL sees each dependency under its label, as a CTE: a temporary
    // table named by the label would shadow the server's table of that name
    // for the generated SQL of the next dependency.
    let list = job
        .tables
        .iter()
        .enumerate()
        .map(|(index, table)| {
            format!(
                "{} AS (SELECT * FROM {})",
                quote(&table.name),
                temporary_table(index)
            )
        })
        .chain(
            job.views
                .iter()
                .map(|(name, sql)| format!("{} AS ({sql})", quote(name))),
        )
        .collect::<Vec<_>>();
    let ctes = if list.is_empty() {
        String::new()
    } else {
        format!("WITH {} ", list.join(", "))
    };
    let select = format!("{ctes}SELECT * FROM ({}) _sof_q", job.query);
    let statement = (&mut *tx)
        .prepare(AssertSqlSafe(select).into_sql_str())
        .await
        .map_err(query_error)?;
    let columns: Vec<SqlJobColumn> = statement
        .columns()
        .iter()
        .map(|column| SqlJobColumn {
            name: column.name().to_owned(),
            sql_type: column.type_info().name().to_ascii_lowercase(),
        })
        .collect();
    for (index, column) in columns.iter().enumerate() {
        if columns[..index]
            .iter()
            .any(|other| other.name == column.name)
        {
            return Err(SqlJobError::Query(format!(
                "Result column {} appears twice; give each column a distinct name",
                column.name
            )));
        }
    }

    let rows_sql = format!(
        "{ctes}SELECT row_to_json(_sof_q)::text FROM ({}) _sof_q",
        job.query
    );
    let mut rows = Vec::new();
    let mut bytes = 0_usize;
    {
        let mut stream = bind_scalar(
            query_scalar::<_, String>(AssertSqlSafe(rows_sql)),
            &job.params,
        )
        .fetch(&mut *tx);
        while rows.len() < job.limit {
            let Some(row) = stream.next().await else {
                break;
            };
            let text = row.map_err(query_error)?;
            bytes = bytes.saturating_add(text.len());
            if bytes > job.max_bytes {
                return Err(SqlJobError::TooLarge(
                    "The query result exceeds the configured byte budget; narrow it or pass _limit"
                        .into(),
                ));
            }
            let object: Map<String, Value> = serde_json::from_str(&text)
                .map_err(|error| StorageError::internal(format!("Row is not JSON: {error}")))?;
            // serde_json maps sort keys; keep the result's column order.
            let mut ordered = Map::with_capacity(columns.len());
            for column in &columns {
                ordered.insert(
                    column.name.clone(),
                    object.get(&column.name).cloned().unwrap_or(Value::Null),
                );
            }
            rows.push(Value::Object(ordered));
        }
    }
    tx.rollback().await.map_err(storage)?;
    Ok(SqlJobResult { columns, rows })
}

/// `CREATE TEMP TABLE` for one dependency, granted to the query role. A view
/// over a resource type with no table yet has no rows.
/// The temporary table holding dependency `index`; never a name the SQL can
/// write (the check admits only labels and CTEs) or the server's SQL reads.
fn temporary_table(index: usize) -> String {
    format!("pg_temp._sof_dep_{index}")
}

async fn create_dependency_table(
    tx: &mut PgTransaction<'_>,
    name: &str,
    table: &DependencyTable,
) -> Result<(), SqlJobError> {
    match &table.source {
        DependencySource::View {
            source,
            source_name,
            select_sql,
        } => {
            query("SAVEPOINT sof_dependency")
                .execute(&mut **tx)
                .await
                .map_err(storage)?;
            let sql = format!(
                "CREATE TEMP TABLE {name} ON COMMIT DROP AS \
                 WITH {source_name} AS ({}) SELECT * FROM ({select_sql}) _sof_dep",
                source.sql
            );
            match bind_query(query(AssertSqlSafe(sql)), &source.params)
                .execute(&mut **tx)
                .await
            {
                Ok(_) => {}
                Err(error) if is_undefined_table(&error) => {
                    query("ROLLBACK TO SAVEPOINT sof_dependency")
                        .execute(&mut **tx)
                        .await
                        .map_err(storage)?;
                    create_from_rows(tx, name, &table.columns, &Value::Array(Vec::new())).await?;
                }
                Err(error) => {
                    return Err(StorageError::internal(format!(
                        "Dependency {} failed: {error}",
                        table.name
                    ))
                    .into());
                }
            }
        }
        DependencySource::Rows(rows) => {
            create_from_rows(tx, name, &table.columns, rows).await?;
        }
    }
    query(AssertSqlSafe(format!(
        "GRANT SELECT ON {name} TO {SOF_QUERY_ROLE}"
    )))
    .execute(&mut **tx)
    .await
    .map_err(storage)?;
    Ok(())
}

async fn create_from_rows(
    tx: &mut PgTransaction<'_>,
    name: &str,
    columns: &[(String, &'static str)],
    rows: &Value,
) -> Result<(), SqlJobError> {
    let definitions = columns
        .iter()
        .map(|(column, sql_type)| format!("{} {sql_type}", quote(column)))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "CREATE TEMP TABLE {name} ON COMMIT DROP AS \
         SELECT * FROM jsonb_to_recordset($1::jsonb) AS _sof_rows({definitions})"
    );
    query(AssertSqlSafe(sql))
        .bind(rows.to_string())
        .execute(&mut **tx)
        .await
        .map_err(storage)?;
    Ok(())
}

/// Bind `params` to `$1..$n` in order.
macro_rules! bind_params {
    ($statement:expr, $params:expr) => {{
        let mut statement = $statement.persistent(false);
        for param in $params {
            statement = match param {
                SqlValue::Text(s) | SqlValue::Json(s) | SqlValue::Timestamp(s) => {
                    statement.bind(s.as_str())
                }
                SqlValue::Integer(i) => statement.bind(*i),
                SqlValue::Float(f) => statement.bind(*f),
                SqlValue::Boolean(b) => statement.bind(*b),
                SqlValue::Null => statement.bind(None::<String>),
            };
        }
        statement
    }};
}

fn bind_scalar<'q, O>(
    statement: QueryScalar<'q, Postgres, O, PgArguments>,
    params: &'q [SqlValue],
) -> QueryScalar<'q, Postgres, O, PgArguments> {
    bind_params!(statement, params)
}

fn bind_query<'q>(
    statement: Query<'q, Postgres, PgArguments>,
    params: &'q [SqlValue],
) -> Query<'q, Postgres, PgArguments> {
    bind_params!(statement, params)
}

fn storage(error: sqlx_core::Error) -> SqlJobError {
    SqlJobError::Storage(StorageError::internal(format!("SQL job failed: {error}")))
}

/// An error of the author's SQL, as PostgreSQL words it.
fn query_error(error: sqlx_core::Error) -> SqlJobError {
    match error.as_database_error() {
        Some(database) if database.code().as_deref() == Some("57014") => {
            SqlJobError::Query("The query exceeded the time limit for SQL on FHIR queries".into())
        }
        Some(database) => SqlJobError::Query(database.message().to_owned()),
        None => storage(error),
    }
}

/// Ids of the live `resource_type` resources whose `url` (and `version`) is
/// the given canonical, for a type the search registry has no `url`
/// parameter for (ViewDefinition). The caller reads each id through its
/// authorized read; this lookup grants nothing.
pub async fn ids_by_canonical(
    pool: &PgPool,
    resource_type: &str,
    url: &str,
    version: Option<&str>,
) -> Result<Vec<String>, StorageError> {
    let table = crate::schema::SchemaManager::table_name(resource_type);
    let sql = format!(
        "SELECT id FROM {table} WHERE status <> 'deleted' AND resource->>'url' = $1 \
         AND ($2::text IS NULL OR resource->>'version' = $2) ORDER BY id LIMIT 2"
    );
    match query_scalar::<_, String>(AssertSqlSafe(sql))
        .bind(url)
        .bind(version)
        .fetch_all(pool)
        .await
    {
        Ok(ids) => Ok(ids),
        Err(error) if is_undefined_table(&error) => Ok(Vec::new()),
        Err(error) => Err(StorageError::internal(format!(
            "Canonical lookup failed: {error}"
        ))),
    }
}
