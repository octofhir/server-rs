//! Running a SQL on FHIR job's dependency graph on this server.
//!
//! The graph itself (one resolution per canonical, `context` matching, CTE
//! order) is `octofhir_sof::plan`; this module loads the subjects, and
//! materializes each table artifact once per job (a ViewDefinition over the
//! caller's authorized data, or a relation's rows) and gives every query its
//! dependencies as CTEs named by the labels its SQL uses, each SQLView's SQL
//! checked by [`octofhir_sof::guard`] (see
//! `octofhir_db_postgres::queries::sql_job`).

use std::collections::HashMap;

use axum::http::StatusCode;

use octofhir_api::ApiError;
use octofhir_db_postgres::queries::sql_job::{DependencySource, DependencyTable, Relation};
use octofhir_sof::expansion::{Lookup, split_canonical};
use octofhir_sof::guard;
use octofhir_sof::library::SqlLibrary;
use octofhir_sof::plan::{Artifact, Edge, ResolvedJob, TerminologyKind, closure};
use octofhir_sof::relations::{CONCEPT_MAP_COLUMNS, VALUE_SET_COLUMNS, columns};
use octofhir_sof::{CompiledView, Dialect, ViewDefinition};
use serde_json::Value;

use super::engine::{self, ViewSql};
use super::rejection::{at, rejected};
use super::scope::ViewScope;
use super::term_source::{plan_error, stored_latest};
use crate::operations::{OperationContext, OperationError};
use crate::server::AppState;

/// The SQL dialect this server runs.
pub(crate) const DIALECT: &str = "postgresql";

/// The tables of a job, each artifact materialized once.
pub(crate) struct JobTables {
    pub tables: Vec<DependencyTable>,
    table_of: HashMap<usize, usize>,
}

impl JobTables {
    /// Materialize every table artifact of `job` as the caller sees the data
    /// (`scope` applied to every ViewDefinition).
    pub(crate) async fn build(
        state: &AppState,
        context: &OperationContext,
        scope: &ViewScope,
        job: &ResolvedJob,
    ) -> Result<Self, OperationError> {
        let mut tables = Vec::new();
        let mut table_of = HashMap::new();
        for (index, artifact) in job.artifacts.iter().enumerate() {
            let table = match artifact {
                Artifact::View { resource } => {
                    let view = ViewDefinition::from_json(resource).map_err(|error| {
                        unprocessable(format!("Invalid ViewDefinition: {error}"))
                    })?;
                    view_table(state, context, scope, &view).await?
                }
                Artifact::Terminology { relation, rows } => DependencyTable {
                    name: format!("terminology dependency {index}"),
                    columns: match relation {
                        TerminologyKind::ValueSet => columns(&VALUE_SET_COLUMNS),
                        TerminologyKind::ConceptMap => columns(&CONCEPT_MAP_COLUMNS),
                    },
                    source: DependencySource::Rows(rows.clone()),
                },
                Artifact::SqlView { .. } => continue,
            };
            table_of.insert(index, tables.len());
            tables.push(table);
        }
        Ok(Self { tables, table_of })
    }

    /// Empty stand-ins of every table artifact of `job`, with its columns:
    /// enough to check the job's queries without reading any data.
    pub(crate) async fn shapes(
        state: &AppState,
        job: &ResolvedJob,
    ) -> Result<Self, OperationError> {
        let mut tables = Vec::new();
        let mut table_of = HashMap::new();
        for (index, artifact) in job.artifacts.iter().enumerate() {
            let columns = match artifact {
                Artifact::View { resource } => {
                    let view = ViewDefinition::from_json(resource).map_err(|error| {
                        unprocessable(format!("Invalid ViewDefinition: {error}"))
                    })?;
                    view_columns(state, &view).await?
                }
                Artifact::Terminology { relation, .. } => match relation {
                    TerminologyKind::ValueSet => columns(&VALUE_SET_COLUMNS),
                    TerminologyKind::ConceptMap => columns(&CONCEPT_MAP_COLUMNS),
                },
                Artifact::SqlView { .. } => continue,
            };
            table_of.insert(index, tables.len());
            tables.push(DependencyTable {
                name: format!("dependency {index}"),
                columns,
                source: DependencySource::Rows(Value::Array(Vec::new())),
            });
        }
        Ok(Self { tables, table_of })
    }

    /// The CTEs of a query over `edges`.
    pub(crate) fn ctes(
        &self,
        job: &ResolvedJob,
        edges: &[Edge],
    ) -> Result<Vec<(String, Relation)>, OperationError> {
        closure(job, edges)
            .map_err(plan_error)?
            .into_iter()
            .map(|(label, artifact)| {
                Ok((
                    label,
                    match &job.artifacts[artifact] {
                        Artifact::SqlView {
                            display,
                            sql,
                            dependencies,
                        } => {
                            let relations = dependencies
                                .iter()
                                .map(|dependency| dependency.label.clone())
                                .collect();
                            let checked = guard::check(sql, &relations, &HashMap::new())
                                .map_err(|error| unprocessable(format!("{display}: {error}")))?;
                            Relation::Sql(checked.sql)
                        }
                        _ => Relation::Table(*self.table_of.get(&artifact).ok_or_else(|| {
                            OperationError::Internal("A dependency has no table".into())
                        })?),
                    },
                ))
            })
            .collect()
    }

    /// The table of a ViewDefinition subject.
    pub(crate) fn table_of(&self, artifact: usize) -> Option<usize> {
        self.table_of.get(&artifact).copied()
    }
}

/// The columns of a view with their PostgreSQL types; a view that does not
/// compile is a 422.
pub(crate) async fn view_columns(
    state: &AppState,
    view: &ViewDefinition,
) -> Result<Vec<(String, &'static str)>, OperationError> {
    let columns = match ViewSql::prepare(view)? {
        Ok(sql) => sql.columns().to_vec(),
        Err(_) => CompiledView::compile_with(view, &engine::compile_options(state))
            .await
            .map_err(|error| unprocessable(format!("Invalid ViewDefinition: {error}")))?
            .columns()
            .to_vec(),
    };
    Ok(columns
        .iter()
        .map(|column| {
            (
                column.name.clone(),
                Dialect::Postgres.value_type(column.col_type),
            )
        })
        .collect())
}

/// A ViewDefinition as a table: SQL over the caller's authorized rows when it
/// lowers, else its rows from the in-memory engine.
async fn view_table(
    state: &AppState,
    context: &OperationContext,
    scope: &ViewScope,
    view: &ViewDefinition,
) -> Result<DependencyTable, OperationError> {
    let name = view
        .url
        .clone()
        .or_else(|| view.name.clone())
        .unwrap_or_else(|| view.resource.clone());
    let search = scope.search_params(state, context, &view.resource).await?;
    if let Ok(sql) = ViewSql::prepare(view)?
        && let Ok(source) =
            engine::authorized_source(state, context, &view.resource, &search).await?
    {
        return Ok(DependencyTable {
            name,
            columns: sql
                .columns()
                .iter()
                .map(|column| {
                    (
                        column.name.clone(),
                        Dialect::Postgres.value_type(column.col_type),
                    )
                })
                .collect(),
            source: DependencySource::View {
                source,
                source_name: engine::SOURCE_RELATION.to_owned(),
                select_sql: sql.select_sql().to_owned(),
            },
        });
    }
    let (columns, rows) = engine::execute(state, context, view, None, &search, None).await?;
    Ok(DependencyTable {
        name,
        columns: columns
            .iter()
            .map(|column| {
                (
                    column.name.clone(),
                    Dialect::Postgres.value_type(column.col_type),
                )
            })
            .collect(),
        source: DependencySource::Rows(Value::Array(rows)),
    })
}

/// The subject named by exactly one of `subjectCanonical`,
/// `subjectReference` and `subjectResource` among `parts`: none is a 400
/// `required`, more than one a 400 `invalid`, one that cannot be resolved a
/// 404, each naming `subject`.
pub(crate) async fn load_subject(
    state: &AppState,
    context: &OperationContext,
    parts: &[&Value],
) -> Result<Value, OperationError> {
    let named: Vec<&&Value> = parts
        .iter()
        .filter(|param| {
            matches!(
                param["name"].as_str(),
                Some("subjectCanonical" | "subjectReference" | "subjectResource")
            )
        })
        .collect();
    let param = match named.as_slice() {
        [param] => param,
        [] => {
            return Err(rejected(
                StatusCode::BAD_REQUEST,
                "required",
                Some("subject"),
                "A subject is required: one of subjectCanonical, subjectReference and subjectResource",
            ));
        }
        _ => {
            return Err(rejected(
                StatusCode::BAD_REQUEST,
                "invalid",
                Some("subject"),
                "Exactly one of subjectCanonical, subjectReference and subjectResource is allowed",
            ));
        }
    };
    let loaded = match param["name"].as_str() {
        Some("subjectResource") => param.get("resource").cloned().ok_or_else(|| {
            OperationError::InvalidParameters("subjectResource requires a resource".into())
        }),
        Some("subjectCanonical") => match param["valueCanonical"]
            .as_str()
            .or_else(|| param["valueString"].as_str())
            .or_else(|| param["valueUri"].as_str())
        {
            Some(canonical) => find_canonical(state, context, canonical).await,
            None => Err(OperationError::InvalidParameters(
                "subjectCanonical requires a canonical".into(),
            )),
        },
        _ => match param["valueReference"]["reference"]
            .as_str()
            .or_else(|| param["valueString"].as_str())
        {
            Some(reference) => read_reference(state, context, reference).await,
            None => Err(OperationError::InvalidParameters(
                "subjectReference requires a reference".into(),
            )),
        },
    };
    loaded.map_err(|error| at(error, "subject"))
}

/// What a subject is.
pub(crate) enum Subject {
    View(ViewDefinition),
    Sql(SqlLibrary),
}

/// Classify a loaded subject; anything but a ViewDefinition, SQLQuery or
/// SQLView is a 422.
pub(crate) fn classify(resource: &Value) -> Result<Subject, OperationError> {
    match resource["resourceType"].as_str() {
        Some("ViewDefinition") => ViewDefinition::from_json(resource)
            .map(Subject::View)
            .map_err(|error| unprocessable(format!("Invalid ViewDefinition: {error}"))),
        Some("Library") => SqlLibrary::from_json(resource, DIALECT)
            .map(Subject::Sql)
            .map_err(|error| unprocessable(error.to_string())),
        other => Err(unprocessable(format!(
            "The subject is a {}, not a ViewDefinition, SQLQuery or SQLView",
            other.unwrap_or("resource without a type")
        ))),
    }
}

/// The resource a literal reference names on this server (`Type/id`,
/// relative or under this server's base), as the caller may read it; what it
/// is decides later (a resource that is no subject is a 422).
async fn read_reference(
    state: &AppState,
    context: &OperationContext,
    reference: &str,
) -> Result<Value, OperationError> {
    let base = format!("{}/fhir/", state.base_url.trim_end_matches('/'));
    let relative = reference.strip_prefix(&base).unwrap_or(reference);
    let (resource_type, id) = relative
        .split_once('/')
        .filter(|(resource_type, _)| state.resource_type_set.load().contains(*resource_type))
        .ok_or_else(|| {
            OperationError::NotFound(format!(
                "subjectReference {reference} is not a resource of this server"
            ))
        })?;
    octofhir_core::validate_id(id).map_err(|_| {
        OperationError::InvalidParameters(format!("Invalid id in subjectReference {reference}"))
    })?;
    context
        .read(state, resource_type, id)
        .await?
        .map(|stored| stored.resource)
        .ok_or_else(|| OperationError::NotFound(format!("{relative} not found")))
}

/// The server's Library or ViewDefinition `canonical` names, as the caller
/// may read it; without a version the latest one (FHIR canonical
/// resolution); none is a 404.
async fn find_canonical(
    state: &AppState,
    context: &OperationContext,
    canonical: &str,
) -> Result<Value, OperationError> {
    let (url, version) = split_canonical(canonical);
    for resource_type in ["Library", "ViewDefinition"] {
        match stored_latest(state, context, resource_type, url, version).await? {
            Lookup::Found(resolved) => return Ok(resolved.resource),
            Lookup::Ambiguous(reason) => {
                return Err(OperationError::NotFound(format!("Subject {reason}")));
            }
            Lookup::Missing => {}
        }
    }
    Err(OperationError::NotFound(format!(
        "Subject {canonical} not found"
    )))
}

pub(crate) fn unprocessable(message: String) -> OperationError {
    OperationError::Api(ApiError::unprocessable_entity(message, None))
}
