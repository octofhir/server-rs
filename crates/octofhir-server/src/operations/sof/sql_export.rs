//! `$sql-export` (SQL on FHIR v3): export ViewDefinitions, SQLQueries and
//! SQLViews, in any mixture, as one asynchronous job.
//!
//! **Kick-off** (`POST /$sql-export`, `Prefer: respond-async`) does every
//! check a job can fail synchronously: the parameters, each `subject`
//! (exactly one of `subjectCanonical`, `subjectReference`,
//! `subjectResource`; `parameters` bound to a SQL subject's declarations;
//! unique output names), the `patient` / `group` filters, the dependency
//! graph of all subjects with one resolution per canonical and the `context`
//! rules, terminology content, and every query prepared against empty
//! stand-ins of its dependencies (so a missing column is a 422 here, not a
//! failed job). The resolved artifacts are stored with the job: it runs
//! exactly what was checked.
//!
//! **The job** materializes each dependency once and runs every subject in
//! one transaction (`REPEATABLE READ`): one snapshot, so two outputs of a job
//! join without a skew window. Each subject's rows stream into its own files
//! (`ndjson` by default, `csv`, `json`), split into parts past the
//! configured rows per file.
//!
//! **Polling** follows the SQL on FHIR asynchronous delivery: the status URL
//! answers 202 while the job runs and 303 to the result URL once it has
//! finished, successfully or not; the result URL returns the manifest
//! (`Parameters`) or the failure; `DELETE` on the status URL cancels the job,
//! after which the status URL is a 404. See `crate::handlers` for those
//! endpoints.

use std::collections::HashSet;
use std::path::PathBuf;

use async_trait::async_trait;
use axum::http::StatusCode;
use chrono::Utc;
use octofhir_db_postgres::queries::sql_job::{
    JobQuery, Relation, RowSink, SqlExportJob, SqlJobColumn, check_queries, run_sql_export,
};
use serde_json::{Value, json};
use tokio::fs::File;
use tokio::io::{AsyncWriteExt, BufWriter};
use tokio::sync::OnceCell;
use uuid::Uuid;

use octofhir_sof::ColumnInfo;
use octofhir_sof::library::SqlLibrary;
use octofhir_sof::output::{ParquetStreamEncoder, ParquetWriter, RowEncoder, RowFormat};
use octofhir_sof::plan::{Artifact, ContextArtifacts, Edge, ResolvedJob, Resolver};

use super::output::{OutputColumn, PARQUET_MEDIA_TYPE};
use super::rejection::{all_together, at, at_subject_if_unprocessable, rejected, together};
use super::scope::ViewScope;
use super::sql_plan::{DIALECT, JobTables, Subject, classify, load_subject};
use super::sql_run::{
    check_parameters, checked_query, filter_failure, has, is_not_found, job_error, query_role_ready,
};
use super::term_source::{ServerArtifacts, plan_error, provenance_header};
use super::viewdefinition_run::parameters;
use crate::async_jobs::{AsyncJobRequest, AsyncJobStatus};
use crate::config::BulkExportConfig;
use crate::operations::handler::OperationLevel;
use crate::operations::{OperationContext, OperationError, OperationHandler};
use crate::server::AppState;

/// The async job type of `$sql-export`.
pub(crate) const JOB_TYPE: &str = "sql_export";

/// Time one statement of an export job may take.
const EXPORT_STATEMENT_TIMEOUT_MS: u64 = 60 * 60 * 1000;

/// Time the kick-off check of the queries may take.
const CHECK_STATEMENT_TIMEOUT_MS: u64 = 30_000;

/// Rows written between two checks for cancellation.
const CANCEL_CHECK_ROWS: usize = 50_000;

/// The CTE name a ViewDefinition subject's query reads.
const VIEW_SUBJECT: &str = "sof_subject";

/// The parameters of `$sql-export` (SQLExport); `source` is declared but
/// refused, and `_limit` / `resource` are refused as `$sql-run`'s only.
const EXPORT_PARAMETERS: &[&str] = &[
    "subject",
    "context",
    "clientTrackingId",
    "_format",
    "header",
    "patient",
    "group",
    "_since",
    "source",
    "_limit",
    "resource",
];

/// The `$sql-export` handler.
pub struct SqlExportOperation {
    enabled: bool,
    config: BulkExportConfig,
    query_role: OnceCell<bool>,
}

impl SqlExportOperation {
    pub fn new(enabled: bool, config: BulkExportConfig) -> Self {
        Self {
            enabled,
            config,
            query_role: OnceCell::new(),
        }
    }
}

/// The format of the exported files.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ExportFormat {
    Ndjson,
    Csv,
    Json,
    Parquet,
}

impl ExportFormat {
    /// `_format` (default ndjson, whatever `Accept` says).
    fn parse(params: &Value) -> Result<Self, OperationError> {
        let values: Vec<&str> = parameters(params)
            .filter(|param| param["name"] == "_format")
            .filter_map(|param| {
                param["valueCode"]
                    .as_str()
                    .or_else(|| param["valueString"].as_str())
            })
            .collect();
        let format = match values.as_slice() {
            [] => return Ok(Self::Ndjson),
            [format] => format.to_ascii_lowercase(),
            _ => {
                return Err(OperationError::InvalidParameters(
                    "_format must occur at most once".into(),
                ));
            }
        };
        match format.as_str() {
            "ndjson" | "application/x-ndjson" | "application/ndjson" => Ok(Self::Ndjson),
            "csv" | "text/csv" => Ok(Self::Csv),
            "json" | "application/json" => Ok(Self::Json),
            "parquet" | PARQUET_MEDIA_TYPE | "application/parquet" => Ok(Self::Parquet),
            "fhir" => Err(OperationError::InvalidParameters(
                "_format: fhir is offered by $sql-run only; an export produces flat files".into(),
            )),
            other => Err(OperationError::NotSupported(format!(
                "_format {other} is not supported; use ndjson, csv, json or parquet"
            ))),
        }
    }

    fn code(self) -> &'static str {
        match self {
            Self::Ndjson => "ndjson",
            Self::Csv => "csv",
            Self::Json => "json",
            Self::Parquet => "parquet",
        }
    }

    /// How files of this format are encoded.
    fn file_format(self, header: bool) -> FileFormat {
        match self {
            Self::Ndjson => FileFormat::Rows(RowFormat::Ndjson),
            Self::Csv => FileFormat::Rows(RowFormat::Csv { header }),
            Self::Json => FileFormat::Rows(RowFormat::Json),
            Self::Parquet => FileFormat::Parquet,
        }
    }

    fn from_code(code: &str) -> Self {
        match code {
            "csv" => Self::Csv,
            "json" => Self::Json,
            "parquet" => Self::Parquet,
            _ => Self::Ndjson,
        }
    }
}

/// The media type an exported file is served with, by its extension.
pub(crate) fn media_type(filename: &str) -> &'static str {
    if filename.ends_with(".csv") {
        "text/csv"
    } else if filename.ends_with(".parquet") {
        PARQUET_MEDIA_TYPE
    } else if filename.ends_with(".json") {
        "application/json"
    } else {
        "application/x-ndjson"
    }
}

#[async_trait]
impl OperationHandler for SqlExportOperation {
    fn supports_level(&self, level: OperationLevel, _resource_type: Option<&str>) -> bool {
        self.enabled && level == OperationLevel::System
    }

    fn code(&self) -> &str {
        "sql-export"
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
        if !self.config.enabled {
            return Err(OperationError::NotSupported(
                "Bulk export is disabled on this server".into(),
            ));
        }
        if context.is_get() {
            return Err(rejected(
                StatusCode::BAD_REQUEST,
                "required",
                None,
                "$sql-export creates a job: use POST",
            ));
        }
        if !context.respond_async() {
            return Err(rejected(
                StatusCode::BAD_REQUEST,
                "required",
                None,
                "$sql-export requires the header Prefer: respond-async",
            ));
        }
        for (name, reason) in [
            ("_limit", "_limit is offered by $sql-run only"),
            (
                "resource",
                "resource is offered by $sql-run only; an export reads the server's data",
            ),
        ] {
            if has(params, name) {
                return Err(rejected(
                    StatusCode::BAD_REQUEST,
                    "invalid",
                    Some(name),
                    reason,
                ));
            }
        }
        if has(params, "source") {
            return Err(rejected(
                StatusCode::BAD_REQUEST,
                "not-supported",
                Some("source"),
                "External data sources (source) are not supported; the export reads this server's data",
            ));
        }
        check_parameters(params, EXPORT_PARAMETERS)?;
        let format = ExportFormat::parse(params).map_err(|error| at(error, "_format"))?;
        let header = csv_header(params).map_err(|error| at(error, "header"))?;
        let client_tracking_id = single_string(params, "clientTrackingId")
            .map_err(|error| at(error, "clientTrackingId"))?;
        let scope = ViewScope::from_parameters(params)?;

        // Subjects first: an unresolvable subject is the more fundamental
        // failure, reported together with an unresolvable filter.
        let subjects = match self.subjects(state, context, params).await {
            Ok(subjects) => subjects,
            Err(error) if is_not_found(&error) => {
                return Err(match filter_failure(state, context, params).await {
                    Some(filter) => together(error, filter),
                    None => error,
                });
            }
            Err(error) => return Err(error),
        };
        scope.search_params(state, context, "Patient").await?;
        let inline = ContextArtifacts::from_parameters(params).map_err(plan_error)?;
        let source = ServerArtifacts { state, context };
        let mut resolver = Resolver::new(&source, &inline, DIALECT);
        let mut planned = Vec::with_capacity(subjects.len());
        for subject in subjects {
            planned.push(match subject.kind {
                SubjectKind::View(resource) => {
                    let artifact = resolver.view_subject(resource);
                    PlannedSubject {
                        name: subject.name,
                        file: subject.file,
                        plan: SubjectPlan::View { artifact },
                    }
                }
                SubjectKind::Sql {
                    library,
                    resource,
                    parameters,
                } => {
                    let dependencies = resolver.subject(&library).await.map_err(plan_error)?;
                    PlannedSubject {
                        name: subject.name,
                        file: subject.file,
                        plan: SubjectPlan::Sql {
                            library: resource,
                            parameters,
                            dependencies,
                        },
                    }
                }
            });
        }
        let job = resolver.finish().map_err(plan_error)?;

        // The caller may read every view's resource type, and every query
        // prepares against its dependencies: checked now, not in the job.
        for artifact in &job.artifacts {
            if let Artifact::View { resource } = artifact
                && let Some(resource_type) = resource["resource"].as_str()
            {
                context
                    .search(
                        state,
                        resource_type,
                        &octofhir_storage::SearchParams::new().with_count(1),
                    )
                    .await?;
            }
        }
        if planned
            .iter()
            .any(|subject| matches!(subject.plan, SubjectPlan::Sql { .. }))
        {
            query_role_ready(&self.query_role, state).await?;
        }
        let shapes = JobTables::shapes(state, &job)
            .await
            .map_err(at_subject_if_unprocessable)?;
        let queries = queries(&job, &shapes, &planned)?;
        check_queries(
            &state.db_pool,
            &shapes.tables,
            &queries,
            CHECK_STATEMENT_TIMEOUT_MS,
        )
        .await
        .map_err(|error| at_subject_if_unprocessable(job_error("$sql-export", error)))?;

        let request_url = context.request_url().to_owned();
        let body = json!({
            "subjects": planned.iter().map(PlannedSubject::to_json).collect::<Vec<_>>(),
            "resolved": job,
            "scope": {"parameter": parameters(params)
                .filter(|param| matches!(param["name"].as_str(), Some("patient" | "group" | "_since")))
                .cloned()
                .collect::<Vec<_>>()},
            "format": format.code(),
            "header": header,
            "clientTrackingId": client_tracking_id,
            "config": {
                "export_path": self.config.export_path,
                "max_rows_per_file": self.config.max_resources_per_file,
            },
        });
        let job_id = state
            .async_job_manager
            .submit_job(AsyncJobRequest {
                request_type: JOB_TYPE.to_owned(),
                method: "POST".to_owned(),
                url: request_url,
                body: Some(body),
                headers: None,
                client_id: context.auth().map(|auth| auth.client_id().to_owned()),
                authorization: context.job_authorization(state)?,
            })
            .await
            .map_err(|error| {
                OperationError::Internal(format!("Failed to submit the export job: {error}"))
            })?;
        tracing::info!(job_id = %job_id, subjects = planned.len(), "SQL on FHIR export job submitted");

        let status_url = format!("{}/_async-status/{job_id}", state.fhir_base_url());
        let mut kickoff = vec![
            json!({"name": "exportId", "valueString": job_id.to_string()}),
            json!({"name": "status", "valueCode": "accepted"}),
            json!({"name": "location", "valueUri": status_url}),
            json!({"name": "cancelUrl", "valueUri": status_url}),
            json!({"name": "_format", "valueCode": format.code()}),
        ];
        if let Some(id) = &client_tracking_id {
            kickoff.insert(1, json!({"name": "clientTrackingId", "valueString": id}));
        }
        Ok(json!({
            "status": "accepted",
            "status_url": status_url,
            "body": {"resourceType": "Parameters", "parameter": kickoff},
        }))
    }
}

/// A subject as named in the request.
struct NamedSubject {
    name: String,
    /// The stem of its files.
    file: String,
    kind: SubjectKind,
}

enum SubjectKind {
    View(Value),
    Sql {
        library: SqlLibrary,
        resource: Value,
        parameters: Option<Value>,
    },
}

/// A subject with its dependencies resolved.
struct PlannedSubject {
    name: String,
    file: String,
    plan: SubjectPlan,
}

enum SubjectPlan {
    View {
        artifact: usize,
    },
    Sql {
        library: Value,
        parameters: Option<Value>,
        dependencies: Vec<Edge>,
    },
}

impl PlannedSubject {
    fn to_json(&self) -> Value {
        match &self.plan {
            SubjectPlan::View { artifact } => json!({
                "name": self.name, "file": self.file, "kind": "view", "artifact": artifact,
            }),
            SubjectPlan::Sql {
                library,
                parameters,
                dependencies,
            } => json!({
                "name": self.name, "file": self.file, "kind": "sql", "library": library,
                "parameters": parameters, "dependencies": dependencies,
            }),
        }
    }

    fn from_json(value: &Value) -> Result<Self, String> {
        let name = value["name"].as_str().ok_or("subject without a name")?;
        let file = value["file"].as_str().ok_or("subject without a file")?;
        let plan = match value["kind"].as_str() {
            Some("view") => SubjectPlan::View {
                artifact: value["artifact"]
                    .as_u64()
                    .and_then(|index| usize::try_from(index).ok())
                    .ok_or("view subject without an artifact")?,
            },
            Some("sql") => SubjectPlan::Sql {
                library: value["library"].clone(),
                parameters: Some(value["parameters"].clone()).filter(|value| !value.is_null()),
                dependencies: serde_json::from_value(value["dependencies"].clone())
                    .map_err(|error| error.to_string())?,
            },
            _ => return Err("subject of an unknown kind".into()),
        };
        Ok(Self {
            name: name.to_owned(),
            file: file.to_owned(),
            plan,
        })
    }
}

impl SqlExportOperation {
    /// The `subject` repetitions (1..*), loaded and named. Problems with
    /// several subjects are reported together.
    async fn subjects(
        &self,
        state: &AppState,
        context: &OperationContext,
        params: &Value,
    ) -> Result<Vec<NamedSubject>, OperationError> {
        let repetitions: Vec<&Value> = parameters(params)
            .filter(|param| param["name"] == "subject")
            .collect();
        if repetitions.is_empty() {
            return Err(rejected(
                StatusCode::BAD_REQUEST,
                "required",
                Some("subject"),
                "subject: at least one subject is required",
            ));
        }
        let mut subjects = Vec::with_capacity(repetitions.len());
        let mut problems = Vec::new();
        for (index, repetition) in repetitions.iter().enumerate() {
            match self.subject(state, context, index, repetition).await {
                Ok(subject) => subjects.push(subject),
                Err(error) => problems.push((format!("subject[{index}]"), error)),
            }
        }
        if !problems.is_empty() {
            return Err(all_together(problems));
        }
        let mut names = HashSet::new();
        for subject in &subjects {
            if !names.insert(subject.name.as_str()) {
                return Err(rejected(
                    StatusCode::BAD_REQUEST,
                    "invalid",
                    Some("subject"),
                    format!(
                        "Two subject repetitions would produce the output name {}; name them apart",
                        subject.name
                    ),
                ));
            }
        }
        let mut files = HashSet::new();
        for subject in &mut subjects {
            let stem = subject.file.clone();
            let mut n = 1;
            while !files.insert(subject.file.clone()) {
                n += 1;
                subject.file = format!("{stem}-{n}");
            }
        }
        Ok(subjects)
    }

    async fn subject(
        &self,
        state: &AppState,
        context: &OperationContext,
        index: usize,
        repetition: &Value,
    ) -> Result<NamedSubject, OperationError> {
        let parts: Vec<&Value> = repetition["part"]
            .as_array()
            .into_iter()
            .flatten()
            .collect();
        let supplied_name = parts
            .iter()
            .find(|part| part["name"] == "name")
            .map(|part| {
                part["valueString"]
                    .as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| {
                        OperationError::InvalidParameters("subject.name must be a string".into())
                    })
            })
            .transpose()?;
        let supplied_parameters: Vec<&Value> = parts
            .iter()
            .copied()
            .filter(|part| part["name"] == "parameters")
            .collect();
        let supplied_parameters = match supplied_parameters.as_slice() {
            [] => None,
            [part] => Some(part["resource"].clone()),
            _ => {
                return Err(OperationError::InvalidParameters(
                    "subject.parameters must occur at most once".into(),
                ));
            }
        };
        let resource = load_subject(state, context, &parts).await?;
        let name = supplied_name
            .or_else(|| resource["name"].as_str().map(str::to_owned))
            .unwrap_or_else(|| format!("subject-{}", index + 1));
        let file = file_stem(&name);
        let kind = match classify(&resource)? {
            Subject::View(_) => {
                if supplied_parameters.is_some() {
                    return Err(OperationError::InvalidParameters(
                        "subject.parameters binds the parameters of a SQLQuery; a ViewDefinition declares none"
                            .into(),
                    ));
                }
                SubjectKind::View(resource)
            }
            Subject::Sql(library) => {
                // Bind now: an unknown or mistyped parameter is a 400 at kick-off.
                checked_query(&library, supplied_parameters.as_ref())?;
                SubjectKind::Sql {
                    library,
                    resource,
                    parameters: supplied_parameters,
                }
            }
        };
        Ok(NamedSubject { name, file, kind })
    }
}

/// A file name stem for an output name: letters, digits, `-`, `_` and `.`.
fn file_stem(name: &str) -> String {
    let stem: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let stem = stem.trim_matches('.').to_owned();
    if stem.is_empty() {
        "output".into()
    } else {
        stem
    }
}

/// `header` (0..1, default true): CSV header row.
fn csv_header(params: &Value) -> Result<bool, OperationError> {
    let values: Vec<&Value> = parameters(params)
        .filter(|param| param["name"] == "header")
        .collect();
    match values.as_slice() {
        [] => Ok(true),
        [param] => param["valueBoolean"]
            .as_bool()
            .or_else(|| match param["valueString"].as_str() {
                Some("true") => Some(true),
                Some("false") => Some(false),
                _ => None,
            })
            .ok_or_else(|| OperationError::InvalidParameters("header must be a boolean".into())),
        _ => Err(OperationError::InvalidParameters(
            "header must occur at most once".into(),
        )),
    }
}

/// A string parameter that occurs at most once.
fn single_string(params: &Value, name: &str) -> Result<Option<String>, OperationError> {
    let values: Vec<&Value> = parameters(params)
        .filter(|param| param["name"] == name)
        .collect();
    match values.as_slice() {
        [] => Ok(None),
        [param] => param["valueString"]
            .as_str()
            .map(|value| Some(value.to_owned()))
            .ok_or_else(|| OperationError::InvalidParameters(format!("{name} must be a string"))),
        _ => Err(OperationError::InvalidParameters(format!(
            "{name} must occur at most once"
        ))),
    }
}

/// One query per subject, over `tables`.
fn queries(
    job: &ResolvedJob,
    tables: &JobTables,
    subjects: &[PlannedSubject],
) -> Result<Vec<JobQuery>, OperationError> {
    subjects
        .iter()
        .map(|subject| match &subject.plan {
            SubjectPlan::View { artifact } => {
                let table = tables.table_of(*artifact).ok_or_else(|| {
                    OperationError::Internal("A view subject has no table".into())
                })?;
                Ok(JobQuery {
                    ctes: vec![(VIEW_SUBJECT.to_owned(), Relation::Table(table))],
                    query: format!("SELECT * FROM \"{VIEW_SUBJECT}\""),
                    params: Vec::new(),
                })
            }
            SubjectPlan::Sql {
                library,
                parameters,
                dependencies,
            } => {
                let library = SqlLibrary::from_json(library, DIALECT)
                    .map_err(|error| OperationError::Internal(error.to_string()))?;
                let (query, params) = checked_query(&library, parameters.as_ref())?;
                Ok(JobQuery {
                    ctes: tables.ctes(job, dependencies)?,
                    query,
                    params,
                })
            }
        })
        .collect()
}

/// Run a `$sql-export` job (the async job executor calls this).
pub async fn execute_sql_export(
    state: AppState,
    job_id: Uuid,
    body: Value,
) -> Result<Value, String> {
    let started = Utc::now();
    let context = OperationContext::for_job(&state, job_id)
        .await
        .map_err(|error| error.to_string())?;
    let subjects = body["subjects"]
        .as_array()
        .ok_or("The job has no subjects")?
        .iter()
        .map(PlannedSubject::from_json)
        .collect::<Result<Vec<_>, _>>()?;
    let job: ResolvedJob =
        serde_json::from_value(body["resolved"].clone()).map_err(|error| error.to_string())?;
    let scope = ViewScope::from_parameters(&body["scope"]).map_err(|error| error.to_string())?;
    let format = ExportFormat::from_code(body["format"].as_str().unwrap_or("ndjson"));
    let header = body["header"].as_bool().unwrap_or(true);
    let export_path = body["config"]["export_path"]
        .as_str()
        .ok_or("The job has no export path")?;
    let max_rows = body["config"]["max_rows_per_file"]
        .as_u64()
        .and_then(|rows| usize::try_from(rows).ok())
        .filter(|rows| *rows > 0)
        .unwrap_or(100_000);

    let tables = JobTables::build(&state, &context, &scope, &job)
        .await
        .map_err(|error| error.to_string())?;
    let queries = queries(&job, &tables, &subjects).map_err(|error| error.to_string())?;
    let dir = PathBuf::from(export_path).join(job_id.to_string());
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|error| format!("Cannot create the export directory: {error}"))?;
    let mut sink = FileSink {
        state: &state,
        job_id,
        dir: dir.clone(),
        format: format.file_format(header),
        max_rows,
        stems: subjects
            .iter()
            .map(|subject| subject.file.clone())
            .collect(),
        files: vec![Vec::new(); subjects.len()],
        open: None,
        written: 0,
    };
    let run = run_sql_export(
        &state.db_pool,
        SqlExportJob {
            tables: tables.tables,
            queries,
            statement_timeout_ms: EXPORT_STATEMENT_TIMEOUT_MS,
        },
        &mut sink,
    )
    .await;
    if let Err(error) = run {
        let _ = tokio::fs::remove_dir_all(&dir).await;
        return Err(job_error("$sql-export", error).to_string());
    }
    let files = sink.files;
    let ended = Utc::now();

    let base = state.fhir_base_url();
    let mut manifest = vec![json!({"name": "exportId", "valueString": job_id.to_string()})];
    if let Some(id) = body["clientTrackingId"].as_str() {
        manifest.push(json!({"name": "clientTrackingId", "valueString": id}));
    }
    manifest.extend([
        json!({"name": "status", "valueCode": "completed"}),
        json!({"name": "_format", "valueCode": format.code()}),
        json!({"name": "exportStartTime", "valueInstant": instant(started)}),
        json!({"name": "exportEndTime", "valueInstant": instant(ended)}),
        json!({"name": "exportDuration", "valueInteger": (ended - started).num_seconds()}),
    ]);
    for (subject, files) in subjects.iter().zip(&files) {
        let mut parts = vec![json!({"name": "name", "valueString": subject.name})];
        parts.extend(files.iter().map(|file| {
            json!({"name": "location", "valueUri": format!("{base}/_bulk-files/{job_id}/{file}")})
        }));
        manifest.push(json!({"name": "output", "part": parts}));
    }
    tracing::info!(job_id = %job_id, subjects = subjects.len(), "SQL on FHIR export completed");
    let mut result = json!({
        "manifest": {"resourceType": "Parameters", "parameter": manifest},
    });
    if !job.provenance.is_empty() {
        result["terminology"] = Value::String(provenance_header(&job.provenance));
    }
    Ok(result)
}

fn instant(time: chrono::DateTime<Utc>) -> String {
    time.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}

/// The download URLs of a finished `$sql-export` job, from its stored result.
pub(crate) fn locations(result: &Value) -> impl Iterator<Item = &str> {
    result["manifest"]["parameter"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|param| param["name"] == "output")
        .flat_map(|output| output["part"].as_array().into_iter().flatten())
        .filter(|part| part["name"] == "location")
        .filter_map(|part| part["valueUri"].as_str())
}

/// The file being written.
struct OpenFile {
    subject: usize,
    writer: BufWriter<File>,
    encoder: Encoder,
    columns: Vec<ColumnInfo>,
}

/// How an export file is encoded.
#[derive(Clone, Copy, Debug)]
enum FileFormat {
    Rows(RowFormat),
    Parquet,
}

impl FileFormat {
    fn extension(self) -> &'static str {
        match self {
            Self::Rows(format) => format.extension(),
            Self::Parquet => "parquet",
        }
    }
}

/// The encoder of one export file.
enum Encoder {
    Rows(RowEncoder),
    Parquet(Box<ParquetStreamEncoder>),
}

impl Encoder {
    /// An encoder and the bytes its file starts with.
    fn new(format: FileFormat, columns: &[ColumnInfo]) -> Result<(Self, Vec<u8>), String> {
        Ok(match format {
            FileFormat::Rows(format) => {
                let names = columns.iter().map(|column| column.name.clone()).collect();
                let encoder = RowEncoder::new(format, names);
                let start = encoder.start().into_bytes();
                (Self::Rows(encoder), start)
            }
            FileFormat::Parquet => {
                let encoder = ParquetStreamEncoder::new(columns.to_vec(), &ParquetWriter::new())
                    .map_err(|error| error.to_string())?;
                (Self::Parquet(Box::new(encoder)), Vec::new())
            }
        })
    }

    fn rows(&self) -> usize {
        match self {
            Self::Rows(encoder) => encoder.rows(),
            Self::Parquet(encoder) => encoder.rows(),
        }
    }

    fn row(&mut self, row: &str) -> Result<Vec<u8>, String> {
        match self {
            Self::Rows(encoder) => encoder.row_json(row).map(String::into_bytes),
            Self::Parquet(encoder) => encoder.row_json(row),
        }
        .map_err(|error| error.to_string())
    }

    fn finish(self) -> Result<Vec<u8>, String> {
        match self {
            Self::Rows(encoder) => Ok(encoder.finish().into_bytes()),
            Self::Parquet(encoder) => encoder.finish().map_err(|error| error.to_string()),
        }
    }
}

/// Writes each subject's rows to its files.
struct FileSink<'a> {
    state: &'a AppState,
    job_id: Uuid,
    dir: PathBuf,
    format: FileFormat,
    max_rows: usize,
    /// File name stem per subject.
    stems: Vec<String>,
    /// File names written per subject.
    files: Vec<Vec<String>>,
    open: Option<OpenFile>,
    /// Rows since the last cancellation check.
    written: usize,
}

impl FileSink<'_> {
    fn extension(&self) -> &'static str {
        self.format.extension()
    }

    /// Open the next file of `subject`. Its first file is `stem.ext`; once a
    /// second is needed the first becomes `stem.part1.ext`.
    async fn open_file(&mut self, subject: usize, columns: Vec<ColumnInfo>) -> Result<(), String> {
        let stem = &self.stems[subject];
        let ext = self.extension();
        let part = self.files[subject].len() + 1;
        if part == 2 {
            let first = format!("{stem}.part1.{ext}");
            tokio::fs::rename(
                self.dir.join(&self.files[subject][0]),
                self.dir.join(&first),
            )
            .await
            .map_err(|error| format!("Cannot rename an export file: {error}"))?;
            self.files[subject][0] = first;
        }
        let name = if part == 1 {
            format!("{stem}.{ext}")
        } else {
            format!("{stem}.part{part}.{ext}")
        };
        let file = File::create(self.dir.join(&name))
            .await
            .map_err(|error| format!("Cannot create an export file: {error}"))?;
        let mut writer = BufWriter::new(file);
        let (encoder, start) = Encoder::new(self.format, &columns)?;
        write(&mut writer, &start).await?;
        self.files[subject].push(name);
        self.open = Some(OpenFile {
            subject,
            writer,
            encoder,
            columns,
        });
        Ok(())
    }

    async fn close_file(&mut self) -> Result<(), String> {
        if let Some(open) = self.open.take() {
            let OpenFile {
                mut writer,
                encoder,
                ..
            } = open;
            write(&mut writer, &encoder.finish()?).await?;
            let mut open_writer = writer;
            open_writer
                .flush()
                .await
                .map_err(|error| format!("Cannot write an export file: {error}"))?;
        }
        Ok(())
    }
}

#[async_trait]
impl RowSink for FileSink<'_> {
    async fn begin(&mut self, query: usize, columns: &[SqlJobColumn]) -> Result<(), String> {
        if job_cancelled(self.state, self.job_id).await {
            return Err("The export was cancelled".into());
        }
        let columns = columns
            .iter()
            .map(|column| OutputColumn::from_sql(&column.name, &column.sql_type).info())
            .collect();
        self.open_file(query, columns).await
    }

    async fn row(&mut self, query: usize, row: &str) -> Result<(), String> {
        let needs_new = self
            .open
            .as_ref()
            .is_some_and(|open| open.subject == query && open.encoder.rows() >= self.max_rows);
        if needs_new {
            let columns = self
                .open
                .as_ref()
                .map(|open| open.columns.clone())
                .unwrap_or_default();
            self.close_file().await?;
            self.open_file(query, columns).await?;
        }
        let open = self.open.as_mut().ok_or("No export file is open")?;
        let encoded = open.encoder.row(row)?;
        write(&mut open.writer, &encoded).await?;
        self.written += 1;
        if self.written >= CANCEL_CHECK_ROWS {
            self.written = 0;
            if job_cancelled(self.state, self.job_id).await {
                return Err("The export was cancelled".into());
            }
        }
        Ok(())
    }

    async fn end(&mut self, _query: usize) -> Result<(), String> {
        self.close_file().await
    }
}

async fn job_cancelled(state: &AppState, job_id: Uuid) -> bool {
    state
        .async_job_manager
        .get_job(job_id)
        .await
        .map(|job| job.status == AsyncJobStatus::Cancelled)
        .unwrap_or(true)
}

async fn write(writer: &mut BufWriter<File>, bytes: &[u8]) -> Result<(), String> {
    writer
        .write_all(bytes)
        .await
        .map_err(|error| format!("Cannot write an export file: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_default_to_ndjson_and_refuse_fhir() {
        let with = |code: &str| json!({"parameter": [{"name": "_format", "valueCode": code}]});
        assert_eq!(
            ExportFormat::parse(&json!({"parameter": []})).unwrap(),
            ExportFormat::Ndjson
        );
        assert_eq!(
            ExportFormat::parse(&with("csv")).unwrap(),
            ExportFormat::Csv
        );
        assert_eq!(
            ExportFormat::parse(&with("application/json")).unwrap(),
            ExportFormat::Json
        );
        assert!(matches!(
            ExportFormat::parse(&with("fhir")),
            Err(OperationError::InvalidParameters(_))
        ));
        assert_eq!(
            ExportFormat::parse(&with("parquet")).unwrap(),
            ExportFormat::Parquet
        );
        assert!(matches!(
            ExportFormat::parse(&with("xlsx")),
            Err(OperationError::NotSupported(_))
        ));
    }

    #[test]
    fn output_names_become_safe_file_stems() {
        assert_eq!(file_stem("bp_summary"), "bp_summary");
        assert_eq!(file_stem("../etc/passwd"), "_etc_passwd");
        assert_eq!(file_stem("Blood pressure / day"), "Blood_pressure___day");
        assert_eq!(file_stem("..."), "output");
    }

    #[test]
    fn manifests_list_their_locations() {
        let result = json!({"manifest": {"resourceType": "Parameters", "parameter": [
            {"name": "exportId", "valueString": "1"},
            {"name": "output", "part": [
                {"name": "name", "valueString": "a"},
                {"name": "location", "valueUri": "http://x/a.part1.csv"},
                {"name": "location", "valueUri": "http://x/a.part2.csv"}]}
        ]}});
        assert_eq!(
            locations(&result).collect::<Vec<_>>(),
            ["http://x/a.part1.csv", "http://x/a.part2.csv"]
        );
    }
}
