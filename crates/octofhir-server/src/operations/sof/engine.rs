//! Where a ViewDefinition runs.
//!
//! The server's own data is read as SQL over its PostgreSQL: the view's
//! generated SELECT reads the caller's authorized selection of the resource
//! type (the same admission, grants and patient membership as a search) as a
//! CTE, so it sees exactly the rows a search would return, and nothing is
//! loaded into the server first.
//!
//! The in-memory engine remains the path when SQL cannot express the run:
//! a resource policy that judges each row (only a snapshot can be judged), or
//! a FHIRPath construct outside the SQL lowering (the in-memory engine runs
//! the full FHIRPath function set, e.g. `memberOf()`).

use octofhir_search::BuiltQuery;
use octofhir_sof::{
    ColumnInfo, CompileOptions, CompiledView, GeneratedSql, SqlGenerator, ViewDefinition,
};
use octofhir_storage::SearchParams;
use serde_json::{Map, Value};

use crate::operations::context::AuthorizedSource;
use crate::operations::{OperationContext, OperationError};
use crate::server::AppState;

/// Name of the CTE holding the authorized source rows.
pub(crate) const SOURCE_RELATION: &str = "sof_source";

/// A view lowered to SQL over [`SOURCE_RELATION`].
pub(crate) struct ViewSql {
    generated: GeneratedSql,
    columns: Vec<ColumnInfo>,
}

/// Why a view runs in the in-memory engine instead of SQL; for logs.
pub(crate) type InMemoryReason = &'static str;

impl ViewSql {
    /// Lower `view` to SQL, or say why it needs the in-memory engine.
    pub(crate) fn prepare(
        view: &ViewDefinition,
    ) -> Result<Result<Self, InMemoryReason>, OperationError> {
        match SqlGenerator::new()
            .with_source(SOURCE_RELATION)
            .with_row_filter(None)
            .generate(view)
        {
            Ok(generated) => {
                let columns = generated
                    .columns
                    .iter()
                    .map(|column| ColumnInfo::new(column.alias.clone(), column.col_type))
                    .collect();
                Ok(Ok(Self { generated, columns }))
            }
            // A path the SQL lowering does not cover; FHIRPath semantics are
            // the in-memory engine's to judge.
            Err(octofhir_sof::Error::InvalidPath(_) | octofhir_sof::Error::FhirPath(_)) => {
                Ok(Err("path outside the SQL lowering"))
            }
            Err(error) => Err(OperationError::InvalidParameters(format!(
                "Invalid ViewDefinition: {error}"
            ))),
        }
    }

    pub(crate) fn columns(&self) -> &[ColumnInfo] {
        &self.columns
    }

    /// The view's SELECT over [`SOURCE_RELATION`].
    pub(crate) fn select_sql(&self) -> &str {
        &self.generated.sql
    }

    /// At most `limit` output rows over `source`.
    pub(crate) async fn fetch(
        &self,
        state: &AppState,
        source: &BuiltQuery,
        limit: Option<usize>,
    ) -> Result<Vec<Value>, OperationError> {
        let objects = octofhir_db_postgres::queries::fetch_rows_over_source(
            &state.read_db_pool,
            source,
            SOURCE_RELATION,
            &self.generated.sql,
            limit,
        )
        .await
        .map_err(crate::handlers::map_storage_error)?;
        Ok(self.in_column_order(objects))
    }

    /// The output rows of the next `batch` source rows after `after`, and the
    /// last source id read (`None` once the source is exhausted).
    pub(crate) async fn fetch_page(
        &self,
        state: &AppState,
        source: &BuiltQuery,
        after: Option<&str>,
        batch: usize,
    ) -> Result<(Vec<Value>, Option<String>), OperationError> {
        let page = octofhir_db_postgres::queries::fetch_rows_over_source_page(
            &state.read_db_pool,
            source,
            SOURCE_RELATION,
            &self.generated.sql,
            after,
            batch,
        )
        .await
        .map_err(crate::handlers::map_storage_error)?;
        Ok((self.in_column_order(page.rows), page.last_id))
    }

    /// jsonb orders object keys; rows keep the view's column order.
    fn in_column_order(&self, objects: Vec<Value>) -> Vec<Value> {
        objects
            .into_iter()
            .map(|object| {
                let mut row = Map::new();
                for column in &self.columns {
                    row.insert(
                        column.name.clone(),
                        object.get(&column.name).cloned().unwrap_or(Value::Null),
                    );
                }
                Value::Object(row)
            })
            .collect()
    }
}

/// The caller's readable rows of `resource_type` matching `params`, as SQL,
/// or why they must be read through the in-memory engine.
pub(crate) async fn authorized_source(
    state: &AppState,
    context: &OperationContext,
    resource_type: &str,
    params: &SearchParams,
) -> Result<Result<BuiltQuery, InMemoryReason>, OperationError> {
    Ok(
        match context
            .authorized_source(state, resource_type, params)
            .await?
        {
            AuthorizedSource::Sql(source) => Ok(source),
            AuthorizedSource::PerResourcePolicy => Err("a resource policy judges each row"),
        },
    )
}

/// Rows of a view run as SQL, as JSON objects in column order.
pub(crate) struct SqlViewRows {
    pub columns: Vec<ColumnInfo>,
    pub rows: Vec<Value>,
}

/// The result of trying the SQL path.
pub(crate) enum SqlRun {
    Rows(SqlViewRows),
    InMemory(InMemoryReason),
}

/// Run `view` as SQL over the caller's authorized rows of its resource type
/// that match `params`, returning at most `limit` rows.
pub(crate) async fn run_view_sql(
    state: &AppState,
    context: &OperationContext,
    view: &ViewDefinition,
    params: &SearchParams,
    limit: Option<usize>,
) -> Result<SqlRun, OperationError> {
    let sql = match ViewSql::prepare(view)? {
        Ok(sql) => sql,
        Err(reason) => return Ok(SqlRun::InMemory(reason)),
    };
    let source = match authorized_source(state, context, &view.resource, params).await? {
        Ok(source) => source,
        Err(reason) => return Ok(SqlRun::InMemory(reason)),
    };
    let rows = sql.fetch(state, &source, limit).await?;
    Ok(SqlRun::Rows(SqlViewRows {
        columns: sql.columns().to_vec(),
        rows,
    }))
}

/// Options for the in-memory engine: the server's terminology layer resolves
/// `memberOf()`, `subsumes()` and `translate()` in view paths.
pub(crate) fn compile_options(state: &AppState) -> CompileOptions {
    match &state.terminology_provider {
        Some(provider) => CompileOptions::with_terminology(provider.clone()),
        None => CompileOptions::default(),
    }
}

/// Page size used when reading source resources for the in-memory engine.
const FETCH_PAGE_SIZE: u32 = 500;

/// Run `view` and return its columns and at most `limit` rows, within the
/// configured output budget.
///
/// `inline` resources (the operation's `resource` parameter) are evaluated in
/// memory. Otherwise the view reads the caller's authorized rows of its
/// resource type matching `scope` (the operation's data filters): as SQL when
/// it can, else through `search` and the in-memory engine.
pub(crate) async fn execute(
    state: &AppState,
    context: &OperationContext,
    view: &ViewDefinition,
    inline: Option<Vec<Value>>,
    scope: &SearchParams,
    limit: Option<usize>,
) -> Result<(Vec<ColumnInfo>, Vec<Value>), OperationError> {
    let limits = state.config.search.include_limits;
    if inline.is_none() {
        // One row past the budget tells an over-budget view from a full one.
        let ceiling = limits.max_resources.saturating_add(1);
        let fetch = limit.map_or(ceiling, |limit| limit.min(ceiling));
        match run_view_sql(state, context, view, scope, Some(fetch)).await? {
            SqlRun::Rows(result) => return Ok((result.columns, result.rows)),
            SqlRun::InMemory(reason) => {
                tracing::debug!(reason, resource = %view.resource, "ViewDefinition runs in memory");
            }
        }
    }
    let resources = match inline {
        Some(resources) => resources,
        None => load_source_resources(state, context, &view.resource, scope).await?,
    };
    let source_bytes = resources
        .iter()
        .try_fold(0_usize, |bytes, resource| {
            serde_json::to_vec(resource).map(|json| bytes.saturating_add(json.len()))
        })
        .map_err(|error| OperationError::Internal(error.to_string()))?;
    if resources.len() > limits.max_resources || source_bytes > limits.max_bytes {
        return Err(OperationError::InvalidParameters(
            "View source exceeds the configured resource or byte budget".into(),
        ));
    }
    let compiled = CompiledView::compile_with(view, &compile_options(state))
        .await
        .map_err(|error| {
            OperationError::InvalidParameters(format!("Invalid ViewDefinition: {error}"))
        })?;
    let mut rows = Vec::new();
    let mut output_bytes: usize = 0;
    for resource in &resources {
        if limit.is_some_and(|limit| rows.len() >= limit) {
            break;
        }
        let data = compiled.execute_resource(resource).await.map_err(|error| {
            OperationError::InvalidParameters(format!("Failed to execute ViewDefinition: {error}"))
        })?;
        let result = octofhir_sof::ViewResult {
            row_count: data.len(),
            data,
            columns: compiled.columns().to_vec(),
        };
        for row in result.to_json_array() {
            if limit.is_some_and(|limit| rows.len() >= limit) {
                break;
            }
            output_bytes = output_bytes.saturating_add(
                serde_json::to_vec(&row)
                    .map_err(|error| OperationError::Internal(error.to_string()))?
                    .len(),
            );
            if rows.len() >= limits.max_resources || output_bytes > limits.max_bytes {
                return Err(OperationError::InvalidParameters(
                    "View output exceeds the configured resource or byte budget".into(),
                ));
            }
            rows.push(row);
        }
        tokio::task::yield_now().await;
    }
    Ok((compiled.columns().to_vec(), rows))
}

/// Load the caller's authorized resources of `resource_type` matching
/// `scope`, page by page, within the configured budget.
async fn load_source_resources(
    state: &AppState,
    context: &OperationContext,
    resource_type: &str,
    scope: &SearchParams,
) -> Result<Vec<Value>, OperationError> {
    let mut resources = Vec::new();
    let mut offset: u32 = 0;
    let mut bytes: usize = 0;
    let limits = state.config.search.include_limits;

    loop {
        let mut params = scope.clone().with_sort("_id", false);
        params.count = Some(FETCH_PAGE_SIZE);
        params.offset = Some(offset);

        let page = context.search(state, resource_type, &params).await?;

        let fetched = page.entries.len();
        if page.has_more && fetched == 0 {
            return Err(OperationError::Internal(
                "View source pagination made no progress".into(),
            ));
        }
        for entry in &page.entries {
            bytes = bytes.saturating_add(
                serde_json::to_vec(&entry.resource)
                    .map_err(|error| OperationError::Internal(error.to_string()))?
                    .len(),
            );
        }
        if bytes > limits.max_bytes
            || resources.len().saturating_add(fetched) > limits.max_resources
        {
            return Err(OperationError::InvalidParameters(
                "View source exceeds the configured resource or byte budget".into(),
            ));
        }
        resources.extend(page.entries.into_iter().map(|entry| entry.resource));

        if !page.has_more {
            break;
        }
        offset = offset
            .checked_add(u32::try_from(fetched).map_err(|_| {
                OperationError::InvalidParameters("View source pagination overflow".into())
            })?)
            .ok_or_else(|| {
                OperationError::InvalidParameters("View source pagination overflow".into())
            })?;
    }

    Ok(resources)
}
