//! `$sql-describe`: what a `$sql-run` of the same subject would read and
//! return, without reading any data. The web console's SQL on FHIR editor
//! uses it to show a query's dependencies and to give the SQL language
//! server the relations the query may name.
//!
//! System level, not advertised (no canonical, like the console's `$run` and
//! `$sql` on ViewDefinition). It takes the subject parameters and `context` of
//! `$sql-run` and resolves the dependencies the way `$sql-run` does (the same
//! resolver, so the same 404 / 422 answers), then prepares the SQL against
//! empty stand-ins of the dependency tables. It answers a `Parameters`:
//!
//! - `column` (repeating, parts `name` and `type`): the subject's result
//!   columns with their PostgreSQL types;
//! - `dependency` (repeating, SQL subjects): `label`, `canonical`, `kind`
//!   (ViewDefinition, SQLView, ValueSet or ConceptMap), `resolved` (the
//!   canonical with the version it resolved to), its `column`s, and for a
//!   terminology dependency the job's provenance record of it as JSON
//!   (`provenance`);
//! - `check`: why the SQL does not prepare (unknown column, type mismatch,
//!   a construct the sandbox refuses), when it does not.

use std::collections::{HashMap, HashSet};

use async_trait::async_trait;
use octofhir_db_postgres::queries::sql_job::{JobQuery, SqlJobColumn, SqlJobError, check_queries};
use octofhir_search::SqlValue;
use octofhir_sof::guard;
use octofhir_sof::library::SqlLibrary;
use octofhir_sof::plan::{
    Artifact, ContextArtifacts, Edge, ResolvedJob, Resolver, TerminologyKind,
};
use serde_json::{Value, json};
use tokio::sync::OnceCell;

use super::rejection::at;
use super::sql_plan::{DIALECT, JobTables, Subject, classify, load_subject, view_columns};
use super::sql_run::query_role_ready;
use super::term_source::{ServerArtifacts, plan_error};
use super::viewdefinition_run::parameters;
use crate::operations::handler::OperationLevel;
use crate::operations::{OperationContext, OperationError, OperationHandler};
use crate::server::AppState;

/// Time the preparation of a subject's statements may take.
const CHECK_STATEMENT_TIMEOUT_MS: u64 = 10_000;

/// The `$sql-describe` handler.
pub struct SqlDescribeOperation {
    enabled: bool,
    /// Whether the sandbox role is usable; checked on the first SQL subject.
    query_role: OnceCell<bool>,
}

impl SqlDescribeOperation {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            query_role: OnceCell::new(),
        }
    }
}

#[async_trait]
impl OperationHandler for SqlDescribeOperation {
    fn supports_level(&self, level: OperationLevel, _resource_type: Option<&str>) -> bool {
        self.enabled && level == OperationLevel::System
    }

    fn code(&self) -> &str {
        "sql-describe"
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
        match classify(&resource).map_err(|error| at(error, "subject"))? {
            Subject::View(view) => {
                let columns = view_columns(state, &view).await?;
                Ok(parameters_resource(
                    columns
                        .iter()
                        .map(|(name, sql_type)| column_part("column", name, sql_type))
                        .collect(),
                ))
            }
            Subject::Sql(library) => {
                query_role_ready(&self.query_role, state).await?;
                describe_sql(state, context, &library, params).await
            }
        }
    }
}

async fn describe_sql(
    state: &AppState,
    context: &OperationContext,
    library: &SqlLibrary,
    params: &Value,
) -> Result<Value, OperationError> {
    let inline = ContextArtifacts::from_parameters(params).map_err(plan_error)?;
    let source = ServerArtifacts { state, context };
    let mut resolver = Resolver::new(&source, &inline, DIALECT);
    let edges = resolver.subject(library).await.map_err(plan_error)?;
    let job = resolver.finish().map_err(plan_error)?;
    let shapes = JobTables::shapes(state, &job).await?;

    // A SQLView has no columns until its SQL is prepared; every other table
    // artifact carries its own.
    let mut derived: HashMap<String, Vec<(String, String)>> = HashMap::new();
    let mut check = None;
    let sql_views: Vec<&Edge> = edges
        .iter()
        .filter(|edge| matches!(job.artifacts[edge.artifact], Artifact::SqlView { .. }))
        .collect();
    if !sql_views.is_empty() {
        let queries = sql_views
            .iter()
            .map(|edge| {
                Ok(JobQuery {
                    ctes: shapes.ctes(&job, std::slice::from_ref(*edge))?,
                    query: format!("SELECT * FROM \"{}\"", edge.label),
                    params: Vec::new(),
                })
            })
            .collect::<Result<Vec<_>, OperationError>>();
        match queries {
            Ok(queries) => {
                match check_queries(
                    &state.db_pool,
                    &shapes.tables,
                    &queries,
                    CHECK_STATEMENT_TIMEOUT_MS,
                )
                .await
                {
                    Ok(columns) => {
                        for (edge, columns) in sql_views.iter().zip(columns) {
                            derived.insert(edge.label.clone(), typed(columns));
                        }
                    }
                    Err(error) => check = Some(check_message(error)?),
                }
            }
            Err(error) => check = Some(error.to_string()),
        }
    }

    let mut result = Vec::new();
    if check.is_none() {
        match subject_query(library, &shapes, &job, &edges) {
            Ok(query) => match check_queries(
                &state.db_pool,
                &shapes.tables,
                std::slice::from_ref(&query),
                CHECK_STATEMENT_TIMEOUT_MS,
            )
            .await
            {
                Ok(mut columns) => {
                    for (name, sql_type) in typed(columns.pop().unwrap_or_default()) {
                        result.push(column_part("column", &name, &sql_type));
                    }
                }
                Err(error) => check = Some(check_message(error)?),
            },
            Err(message) => check = Some(message),
        }
    }

    for edge in &edges {
        result.push(dependency_part(library, &job, &shapes, edge, &derived));
    }
    if let Some(message) = check {
        result.push(json!({"name": "check", "valueString": message}));
    }
    Ok(parameters_resource(result))
}

/// The subject's SQL as `$sql-run` would prepare it, every parameter bound to
/// null (preparing reads the declared types, not the values).
fn subject_query(
    library: &SqlLibrary,
    shapes: &JobTables,
    job: &ResolvedJob,
    edges: &[Edge],
) -> Result<JobQuery, String> {
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
    let checked = guard::check(&library.sql, &relations, &declared)?;
    let ctes = shapes.ctes(job, edges).map_err(|error| error.to_string())?;
    Ok(JobQuery {
        ctes,
        query: checked.sql,
        params: vec![SqlValue::Null; checked.parameters.len()],
    })
}

/// A preparation failure the author can act on, as the `check` message;
/// anything else fails the request.
fn check_message(error: SqlJobError) -> Result<String, OperationError> {
    match error {
        SqlJobError::Query(message) | SqlJobError::TooLarge(message) => Ok(message),
        SqlJobError::Sink(message) => Err(OperationError::Internal(message)),
        SqlJobError::Storage(error) => Err(crate::handlers::map_storage_error(error).into()),
    }
}

fn typed(columns: Vec<SqlJobColumn>) -> Vec<(String, String)> {
    columns
        .into_iter()
        .map(|column| (column.name, column.sql_type))
        .collect()
}

/// One `dependency` part of the answer.
fn dependency_part(
    library: &SqlLibrary,
    job: &ResolvedJob,
    shapes: &JobTables,
    edge: &Edge,
    derived: &HashMap<String, Vec<(String, String)>>,
) -> Value {
    let canonical = library
        .dependencies
        .iter()
        .find(|dependency| dependency.label == edge.label)
        .map(|dependency| dependency.canonical.clone())
        .unwrap_or_default();
    let artifact = &job.artifacts[edge.artifact];
    let (kind, resolved, provenance) = match artifact {
        Artifact::View { resource } => ("ViewDefinition", versioned(resource), None),
        Artifact::SqlView { display, .. } => ("SQLView", Some(display.clone()), None),
        Artifact::Terminology { relation, .. } => {
            let record = provenance_of(job, &edge.label, &canonical);
            let resolved = record.as_ref().and_then(versioned);
            let kind = match relation {
                TerminologyKind::ValueSet => "ValueSet",
                TerminologyKind::ConceptMap => "ConceptMap",
            };
            (kind, resolved, record)
        }
    };
    let columns: Vec<(String, String)> = match artifact {
        Artifact::SqlView { .. } => derived.get(&edge.label).cloned().unwrap_or_default(),
        _ => shapes
            .table_of(edge.artifact)
            .map(|index| {
                shapes.tables[index]
                    .columns
                    .iter()
                    .map(|(name, sql_type)| (name.clone(), (*sql_type).to_owned()))
                    .collect()
            })
            .unwrap_or_default(),
    };

    let mut part = vec![
        json!({"name": "label", "valueString": edge.label}),
        json!({"name": "canonical", "valueString": canonical}),
        json!({"name": "kind", "valueCode": kind}),
    ];
    if let Some(resolved) = resolved {
        part.push(json!({"name": "resolved", "valueString": resolved}));
    }
    for (name, sql_type) in &columns {
        part.push(column_part("column", name, sql_type));
    }
    if let Some(record) = provenance {
        part.push(json!({"name": "provenance", "valueString": record.to_string()}));
    }
    json!({"name": "dependency", "part": part})
}

/// The provenance record of a terminology dependency: by its label, else by
/// its canonical (one resolution serves every label naming the same one).
fn provenance_of(job: &ResolvedJob, label: &str, canonical: &str) -> Option<Value> {
    let url = canonical.split('|').next().unwrap_or(canonical);
    job.provenance
        .iter()
        .find(|record| record["label"] == label)
        .or_else(|| job.provenance.iter().find(|record| record["url"] == url))
        .cloned()
}

/// `url|version` of a resource or provenance record, `url` alone without a
/// version.
fn versioned(resource: &Value) -> Option<String> {
    let url = resource["url"].as_str()?;
    Some(match resource["version"].as_str() {
        Some(version) => format!("{url}|{version}"),
        None => url.to_owned(),
    })
}

fn column_part(name: &str, column: &str, sql_type: &str) -> Value {
    json!({
        "name": name,
        "part": [
            {"name": "name", "valueString": column},
            {"name": "type", "valueString": sql_type},
        ],
    })
}

fn parameters_resource(parameter: Vec<Value>) -> Value {
    json!({"resourceType": "Parameters", "parameter": parameter})
}
