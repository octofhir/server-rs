//! `$sql-run` (SQL on FHIR v3): run a ViewDefinition, SQLQuery or SQLView
//! and return its rows in the requested format.
//!
//! System level only. The subject is exactly one of `subjectCanonical`,
//! `subjectReference` and `subjectResource`:
//!
//! - a **ViewDefinition** runs as `$viewdefinition-run` does (server data or
//!   inline `resource`, data filters, formats);
//! - a **SQLQuery** or **SQLView** first resolves its dependencies (the
//!   `context` artifacts supplied inline win over the server's, then the
//!   installed packages): each ViewDefinition becomes a table over the
//!   caller's authorized data (`patient`, `group` and `_since` applied before
//!   the SQL), each SQLView a CTE, each ValueSet or ConceptMap a relation
//!   with the columns "Terminology in SQL" defines, its content resolved once
//!   for the job (see [`super::term_source`]); `parameters` are bound to the
//!   declared `Library.parameter`s; then the SQL runs in a sandbox (see
//!   [`super::sql_guard`] and `octofhir_db_postgres::queries::sql_job`).
//!
//! Errors follow the operation: 400 for invalid or conflicting input, 404 for
//! a subject or dependency that cannot be found or resolves to more than one
//! version, 422 for an artifact that does not conform, a terminology
//! dependency whose content cannot be determined, or SQL that cannot run.

use std::collections::{HashMap, HashSet};

use async_trait::async_trait;
use octofhir_db_postgres::queries::sql_job::{
    JobQuery, SqlJob, SqlJobError, ensure_sof_query_role, run_sql_job,
};
use octofhir_search::SqlValue;
use serde_json::Value;
use tokio::sync::OnceCell;

use octofhir_sof::library::{ParameterValue, SqlLibrary, bind_parameters};
use octofhir_sof::plan::{ContextArtifacts, Resolver};

use super::output::{OutputColumn, render};
use super::scope::ViewScope;
use super::sql_guard;
use super::sql_plan::{DIALECT, JobTables, Subject, classify, load_subject, unprocessable};
use super::term_source::{PROVENANCE_KEY, ServerArtifacts, plan_error, provenance_header};
use super::viewdefinition_run::{limit, output_format, parameters, run_view};
use crate::operations::handler::OperationLevel;
use crate::operations::{OperationContext, OperationError, OperationHandler};
use crate::server::AppState;

/// Time a job's statements may take, dependencies included.
const STATEMENT_TIMEOUT_MS: u64 = 30_000;

/// The `$sql-run` handler.
pub struct SqlRunOperation {
    enabled: bool,
    /// Whether the sandbox role is usable; checked on the first SQL subject.
    query_role: OnceCell<bool>,
}

impl SqlRunOperation {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            query_role: OnceCell::new(),
        }
    }
}

#[async_trait]
impl OperationHandler for SqlRunOperation {
    fn supports_level(&self, level: OperationLevel, _resource_type: Option<&str>) -> bool {
        self.enabled && level == OperationLevel::System
    }

    fn code(&self) -> &str {
        "sql-run"
    }

    async fn handle_system(
        &self,
        state: &AppState,
        context: &OperationContext,
        params: &Value,
    ) -> Result<Value, OperationError> {
        if !self.enabled {
            return Err(OperationError::NotSupported(
                "SQL on FHIR is not enabled. Please set sql_on_fhir.enabled = true in configuration."
                    .into(),
            ));
        }
        let parts: Vec<&Value> = parameters(params).collect();
        let resource = load_subject(state, context, &parts).await?;
        match classify(&resource)? {
            Subject::View(view) => {
                if has(params, "parameters") {
                    return Err(OperationError::InvalidParameters(
                        "parameters binds the parameters of a SQLQuery; a ViewDefinition declares none"
                            .into(),
                    ));
                }
                if has(params, "context") {
                    return Err(OperationError::InvalidParameters(
                        "context: a ViewDefinition subject has no dependencies to match".into(),
                    ));
                }
                run_view(state, context, &view, params).await
            }
            Subject::Sql(library) => self.run_sql(state, context, &library, params).await,
        }
    }
}

impl SqlRunOperation {
    async fn run_sql(
        &self,
        state: &AppState,
        context: &OperationContext,
        library: &SqlLibrary,
        params: &Value,
    ) -> Result<Value, OperationError> {
        if has(params, "resource") {
            return Err(OperationError::InvalidParameters(
                "resource supplies data to a ViewDefinition subject; a SQL subject reads its dependencies"
                    .into(),
            ));
        }
        if has(params, "source") {
            return Err(OperationError::NotSupported(
                "External data sources (source) are not supported; the query reads this server's data"
                    .into(),
            ));
        }
        query_role_ready(&self.query_role, state).await?;

        let format = output_format(params)?;
        let limits = state.config.search.include_limits;
        let row_limit = limit(params)?.map_or(limits.max_resources, |limit| {
            limit.min(limits.max_resources)
        });
        let scope = ViewScope::from_parameters(params)?;
        let supplied: Vec<&Value> = parameters(params)
            .filter(|param| param["name"] == "parameters")
            .collect();
        let supplied = match supplied.as_slice() {
            [] => None,
            [param] => Some(&param["resource"]),
            _ => {
                return Err(OperationError::InvalidParameters(
                    "parameters must occur at most once".into(),
                ));
            }
        };
        let (sql, bound) = checked_query(library, supplied)?;

        let inline = ContextArtifacts::from_parameters(params).map_err(plan_error)?;
        let source = ServerArtifacts { state, context };
        let mut resolver = Resolver::new(&source, &inline, DIALECT);
        let edges = resolver.subject(library).await.map_err(plan_error)?;
        let job = resolver.finish().map_err(plan_error)?;
        let tables = JobTables::build(state, context, &scope, &job).await?;
        let ctes = tables.ctes(&job, &edges)?;

        let result = run_sql_job(
            &state.db_pool,
            SqlJob {
                tables: tables.tables,
                query: JobQuery {
                    ctes,
                    query: sql,
                    params: bound,
                },
                limit: row_limit,
                max_bytes: limits.max_bytes,
                statement_timeout_ms: STATEMENT_TIMEOUT_MS,
            },
        )
        .await
        .map_err(|error| job_error(&library.display, error))?;
        let columns: Vec<OutputColumn> = result
            .columns
            .iter()
            .map(|column| OutputColumn::from_sql(&column.name, &column.sql_type))
            .collect();
        let mut output = render(format, &columns, &result.rows, limits.max_bytes)?;
        if !job.provenance.is_empty() {
            let record = Value::Array(job.provenance.clone());
            tracing::info!(
                library = %library.display,
                terminology = %record,
                "SQL on FHIR job terminology"
            );
            if let Some(object) = output.as_object_mut() {
                object.insert(
                    PROVENANCE_KEY.to_owned(),
                    Value::String(provenance_header(&job.provenance)),
                );
            }
        }
        Ok(output)
    }
}

/// Whether the sandbox role is usable (checked once per operation); SQL
/// subjects are refused when it is not.
pub(super) async fn query_role_ready(
    ready: &OnceCell<bool>,
    state: &AppState,
) -> Result<(), OperationError> {
    if *ready
        .get_or_init(|| ensure_sof_query_role(&state.db_pool))
        .await
    {
        Ok(())
    } else {
        Err(OperationError::NotSupported(
            "SQLQuery and SQLView subjects need the database role octofhir_sof_query, which the server's database user cannot create or use"
                .into(),
        ))
    }
}

/// The checked SQL of a SQL subject, with `supplied` (a Parameters
/// resource) bound to its declared parameters.
pub(super) fn checked_query(
    library: &SqlLibrary,
    supplied: Option<&Value>,
) -> Result<(String, Vec<SqlValue>), OperationError> {
    let values = bind_parameters(library, supplied).map_err(|error| match error {
        octofhir_sof::Error::InvalidParameter(message) => {
            OperationError::InvalidParameters(message)
        }
        other => OperationError::InvalidParameters(other.to_string()),
    })?;
    let declared: HashMap<String, &'static str> = library
        .parameters
        .iter()
        .filter_map(|parameter| Some((parameter.name.clone(), parameter.sql_type()?)))
        .collect();
    let relations: HashSet<String> = library
        .dependencies
        .iter()
        .map(|dependency| dependency.label.clone())
        .collect();
    let checked = sql_guard::check(&library.sql, &relations, &declared)
        .map_err(|error| unprocessable(format!("{}: {error}", library.display)))?;
    let bound = checked
        .parameters
        .iter()
        .map(|name| match values.get(name) {
            Some(ParameterValue::Text(text) | ParameterValue::Decimal(text)) => {
                SqlValue::Text(text.clone())
            }
            Some(ParameterValue::Integer(value)) => SqlValue::Integer(*value),
            Some(ParameterValue::Boolean(flag)) => SqlValue::Boolean(*flag),
            Some(ParameterValue::Null) | None => SqlValue::Null,
        })
        .collect();
    Ok((checked.sql, bound))
}

/// A job error as the operation answers it.
pub(super) fn job_error(display: &str, error: SqlJobError) -> OperationError {
    match error {
        SqlJobError::Query(message) => unprocessable(format!("{display}: {message}")),
        SqlJobError::TooLarge(message) => OperationError::InvalidParameters(message),
        SqlJobError::Sink(message) => OperationError::Internal(message),
        SqlJobError::Storage(error) => crate::handlers::map_storage_error(error).into(),
    }
}

pub(super) fn has(params: &Value, name: &str) -> bool {
    parameters(params).any(|param| param["name"] == name)
}
