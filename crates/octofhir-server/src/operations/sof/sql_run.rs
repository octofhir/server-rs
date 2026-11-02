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
use octofhir_api::ApiError;
use octofhir_db_postgres::queries::sql_job::{
    DependencySource, DependencyTable, SqlJob, SqlJobError, ensure_sof_query_role, run_sql_job,
};
use octofhir_search::SqlValue;
use octofhir_sof::ViewDefinition;
use serde_json::Value;
use tokio::sync::OnceCell;

use super::engine::{self, ViewSql};
use super::expansion::{Lookup, Resolved, split_canonical};
use super::output::{OutputColumn, render};
use super::scope::ViewScope;
use super::sql_guard;
use super::sql_library::{Dependency, SqlLibrary, SqlLibraryKind};
use super::term_source::{
    PROVENANCE_KEY, ServerTerminology, TerminologyRelation, concept_map_relation,
    matches_canonical, packaged, provenance_header, stored, value_set_relation,
};
use super::viewdefinition_run::{limit, output_format, parameters, run_view};
use crate::operations::handler::OperationLevel;
use crate::operations::{OperationContext, OperationError, OperationHandler};
use crate::server::AppState;

/// Time a job's statements may take, dependencies included.
const STATEMENT_TIMEOUT_MS: u64 = 30_000;

/// SQLViews depending on SQLViews: deeper than this is refused.
const MAX_DEPENDENCY_DEPTH: usize = 16;

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

/// What `$sql-run` runs.
enum Subject {
    View(ViewDefinition),
    Sql(SqlLibrary),
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
        match subject(state, context, params).await? {
            Subject::View(view) => {
                if has(params, "parameters") {
                    return Err(OperationError::InvalidParameters(
                        "parameters binds the parameters of a SQLQuery; a ViewDefinition declares none"
                            .into(),
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
        let ready = *self
            .query_role
            .get_or_init(|| ensure_sof_query_role(&state.db_pool))
            .await;
        if !ready {
            return Err(OperationError::NotSupported(
                "SQLQuery and SQLView subjects need the database role octofhir_sof_query, which the server's database user cannot create or use"
                    .into(),
            ));
        }

        let format = output_format(params)?;
        let limits = state.config.search.include_limits;
        let row_limit = limit(params)?.map_or(limits.max_resources, |limit| {
            limit.min(limits.max_resources)
        });
        let scope = ViewScope::from_parameters(params)?;
        let values = bind_values(library, params)?;

        let mut plan = Plan {
            state,
            context,
            scope: &scope,
            inline: context_artifacts(params)?,
            labels: HashMap::new(),
            tables: Vec::new(),
            views: Vec::new(),
            terminology: HashMap::new(),
            provenance: Vec::new(),
        };
        plan.resolve(library, &mut Vec::new()).await?;
        let provenance = std::mem::take(&mut plan.provenance);

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
            .map(|name| values.get(name).cloned().unwrap_or(SqlValue::Null))
            .collect();

        let job = SqlJob {
            tables: plan.tables,
            views: plan.views,
            query: checked.sql,
            params: bound,
            limit: row_limit,
            max_bytes: limits.max_bytes,
            statement_timeout_ms: STATEMENT_TIMEOUT_MS,
        };
        let result = run_sql_job(&state.db_pool, job)
            .await
            .map_err(|error| match error {
                SqlJobError::Query(message) => {
                    unprocessable(format!("{}: {message}", library.display))
                }
                SqlJobError::TooLarge(message) => OperationError::InvalidParameters(message),
                SqlJobError::Storage(error) => crate::handlers::map_storage_error(error).into(),
            })?;
        let columns: Vec<OutputColumn> = result
            .columns
            .iter()
            .map(|column| OutputColumn::from_sql(&column.name, &column.sql_type))
            .collect();
        let mut output = render(format, &columns, &result.rows, limits.max_bytes)?;
        if !provenance.is_empty() {
            let record = Value::Array(provenance.clone());
            tracing::info!(
                library = %library.display,
                terminology = %record,
                "SQL on FHIR job terminology"
            );
            if let Some(object) = output.as_object_mut() {
                object.insert(
                    PROVENANCE_KEY.to_owned(),
                    Value::String(provenance_header(&provenance)),
                );
            }
        }
        Ok(output)
    }
}

/// The dependencies of one job, as they are resolved.
struct Plan<'a> {
    state: &'a AppState,
    context: &'a OperationContext,
    scope: &'a ViewScope,
    /// `context` artifacts, preferred over the server's.
    inline: Vec<Value>,
    /// Label → canonical; one table name means one artifact in the job.
    labels: HashMap<String, String>,
    tables: Vec<DependencyTable>,
    /// SQLViews, dependencies before dependents.
    views: Vec<(String, String)>,
    /// Terminology content by artifact (type, url, version): one resolution
    /// per job.
    terminology: HashMap<String, (Vec<(String, &'static str)>, Value)>,
    /// What produced each terminology dependency's content.
    provenance: Vec<Value>,
}

impl Plan<'_> {
    /// Resolve `library`'s dependencies; `path` holds the canonicals of the
    /// SQLViews being resolved, to refuse a cycle.
    fn resolve<'s>(
        &'s mut self,
        library: &'s SqlLibrary,
        path: &'s mut Vec<String>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), OperationError>> + Send + 's>>
    {
        Box::pin(async move {
            if path.len() > MAX_DEPENDENCY_DEPTH {
                return Err(unprocessable(format!(
                    "{}: SQLViews nest deeper than {MAX_DEPENDENCY_DEPTH} levels",
                    library.display
                )));
            }
            for dependency in &library.dependencies {
                match self.labels.get(&dependency.label) {
                    Some(canonical) if *canonical == dependency.canonical => continue,
                    Some(canonical) => {
                        return Err(unprocessable(format!(
                            "Table {} names both {canonical} and {}; use distinct labels",
                            dependency.label, dependency.canonical
                        )));
                    }
                    None => {}
                }
                if path.contains(&dependency.canonical) {
                    return Err(unprocessable(format!(
                        "SQLView {} depends on itself",
                        dependency.canonical
                    )));
                }
                self.labels
                    .insert(dependency.label.clone(), dependency.canonical.clone());
                let resolved = self.artifact(&dependency.canonical).await?;
                let artifact = &resolved.resource;
                // Every reference to one value set or concept map sees one
                // content, however the canonical was written.
                if let Some((columns, rows)) = self.terminology.get(&terminology_key(artifact)) {
                    self.tables.push(DependencyTable {
                        name: dependency.label.clone(),
                        columns: columns.clone(),
                        source: DependencySource::Rows(rows.clone()),
                    });
                    continue;
                }
                match artifact["resourceType"].as_str() {
                    Some("ViewDefinition") => {
                        let view = ViewDefinition::from_json(artifact).map_err(|error| {
                            unprocessable(format!(
                                "Dependency {} is not a valid ViewDefinition: {error}",
                                dependency.canonical
                            ))
                        })?;
                        let table = self.view_table(&dependency.label, &view).await?;
                        self.tables.push(table);
                    }
                    Some("Library") => {
                        let view = SqlLibrary::from_json(artifact).map_err(|error| {
                            unprocessable(format!("Dependency {}: {error}", dependency.canonical))
                        })?;
                        if view.kind != SqlLibraryKind::View {
                            return Err(unprocessable(format!(
                                "Dependency {} is a SQLQuery; a dependency is a ViewDefinition or a SQLView",
                                dependency.canonical
                            )));
                        }
                        path.push(dependency.canonical.clone());
                        self.resolve(&view, path).await?;
                        path.pop();
                        let relations = view
                            .dependencies
                            .iter()
                            .map(|dependency| dependency.label.clone())
                            .collect();
                        let checked = sql_guard::check(&view.sql, &relations, &HashMap::new())
                            .map_err(|error| unprocessable(format!("{}: {error}", view.display)))?;
                        self.views.push((dependency.label.clone(), checked.sql));
                    }
                    Some("ValueSet") => {
                        let source = ServerTerminology {
                            state: self.state,
                            context: self.context,
                            inline: &self.inline,
                        };
                        let relation =
                            value_set_relation(&source, &dependency.label, &resolved).await?;
                        self.add_terminology(dependency, artifact, relation);
                    }
                    Some("ConceptMap") => {
                        let relation = concept_map_relation(&dependency.label, &resolved)?;
                        self.add_terminology(dependency, artifact, relation);
                    }
                    _ => {
                        return Err(unprocessable(format!(
                            "Dependency {} is not a ViewDefinition, SQLView, ValueSet or ConceptMap",
                            dependency.canonical
                        )));
                    }
                }
            }
            Ok(())
        })
    }

    /// The artifact `canonical` (`url` or `url|version`) names: a `context`
    /// artifact, else the server's Library, ViewDefinition, ValueSet or
    /// ConceptMap, else a ValueSet or ConceptMap of an installed package.
    async fn artifact(&self, canonical: &str) -> Result<Resolved, OperationError> {
        let (url, version) = split_canonical(canonical);
        if let Some(found) = self
            .inline
            .iter()
            .find(|artifact| matches_canonical(artifact, url, version))
        {
            return Ok(Resolved {
                resource: found.clone(),
                origin: "context".into(),
            });
        }
        let not_found =
            |reason: String| OperationError::NotFound(format!("Dependency {canonical}: {reason}"));
        for resource_type in ["Library", "ViewDefinition", "ValueSet", "ConceptMap"] {
            match stored(self.state, self.context, resource_type, url, version).await? {
                Lookup::Found(resolved) => return Ok(resolved),
                Lookup::Ambiguous(reason) => return Err(not_found(reason)),
                Lookup::Missing => {}
            }
        }
        for resource_type in ["ValueSet", "ConceptMap"] {
            match packaged(resource_type, url, version).await? {
                Lookup::Found(resolved) => return Ok(resolved),
                Lookup::Ambiguous(reason) => return Err(not_found(reason)),
                Lookup::Missing => {}
            }
        }
        Err(not_found("not found".into()))
    }

    /// A terminology dependency's relation becomes a table of the job.
    fn add_terminology(
        &mut self,
        dependency: &Dependency,
        artifact: &Value,
        relation: TerminologyRelation,
    ) {
        let rows = Value::Array(relation.rows);
        self.terminology.insert(
            terminology_key(artifact),
            (relation.columns.clone(), rows.clone()),
        );
        self.provenance.push(relation.provenance);
        self.tables.push(DependencyTable {
            name: dependency.label.clone(),
            columns: relation.columns,
            source: DependencySource::Rows(rows),
        });
    }

    /// A ViewDefinition as a table: SQL over the caller's authorized rows
    /// when it lowers, else its rows from the in-memory engine.
    async fn view_table(
        &self,
        label: &str,
        view: &ViewDefinition,
    ) -> Result<DependencyTable, OperationError> {
        let search = self
            .scope
            .search_params(self.state, self.context, &view.resource)
            .await?;
        if let Ok(sql) = ViewSql::prepare(view)?
            && let Ok(source) =
                engine::authorized_source(self.state, self.context, &view.resource, &search).await?
        {
            return Ok(DependencyTable {
                name: label.to_owned(),
                columns: sql
                    .columns()
                    .iter()
                    .map(|column| (column.name.clone(), column.col_type.sql_type()))
                    .collect(),
                source: DependencySource::View {
                    source,
                    source_name: engine::SOURCE_RELATION.to_owned(),
                    select_sql: sql.select_sql().to_owned(),
                },
            });
        }
        let (columns, rows) =
            engine::execute(self.state, self.context, view, None, &search, None).await?;
        Ok(DependencyTable {
            name: label.to_owned(),
            columns: columns
                .iter()
                .map(|column| (column.name.clone(), column.col_type.sql_type()))
                .collect(),
            source: DependencySource::Rows(Value::Array(rows)),
        })
    }
}

/// The subject named by exactly one of the three subject parameters.
async fn subject(
    state: &AppState,
    context: &OperationContext,
    params: &Value,
) -> Result<Subject, OperationError> {
    let named: Vec<&Value> = parameters(params)
        .filter(|param| {
            matches!(
                param["name"].as_str(),
                Some("subjectCanonical" | "subjectReference" | "subjectResource")
            )
        })
        .collect();
    let [param] = named.as_slice() else {
        return Err(OperationError::InvalidParameters(
            "Exactly one of subjectCanonical, subjectReference and subjectResource is required"
                .into(),
        ));
    };
    let resource = match param["name"].as_str() {
        Some("subjectResource") => param.get("resource").cloned().ok_or_else(|| {
            OperationError::InvalidParameters("subjectResource requires a resource".into())
        })?,
        Some("subjectCanonical") => {
            let canonical = param["valueCanonical"]
                .as_str()
                .or_else(|| param["valueString"].as_str())
                .or_else(|| param["valueUri"].as_str())
                .ok_or_else(|| {
                    OperationError::InvalidParameters(
                        "subjectCanonical requires a canonical".into(),
                    )
                })?;
            find_canonical(state, context, canonical).await?
        }
        _ => {
            let reference = param["valueReference"]["reference"]
                .as_str()
                .or_else(|| param["valueString"].as_str())
                .ok_or_else(|| {
                    OperationError::InvalidParameters(
                        "subjectReference requires a reference".into(),
                    )
                })?;
            read_reference(state, context, reference).await?
        }
    };
    match resource["resourceType"].as_str() {
        Some("ViewDefinition") => ViewDefinition::from_json(&resource)
            .map(Subject::View)
            .map_err(|error| unprocessable(format!("Invalid ViewDefinition: {error}"))),
        Some("Library") => SqlLibrary::from_json(&resource)
            .map(Subject::Sql)
            .map_err(unprocessable),
        other => Err(unprocessable(format!(
            "The subject is a {}, not a ViewDefinition, SQLQuery or SQLView",
            other.unwrap_or("resource without a type")
        ))),
    }
}

/// `Library/id` or `ViewDefinition/id`, relative or under this server's base.
async fn read_reference(
    state: &AppState,
    context: &OperationContext,
    reference: &str,
) -> Result<Value, OperationError> {
    let base = format!("{}/fhir/", state.base_url.trim_end_matches('/'));
    let relative = reference.strip_prefix(&base).unwrap_or(reference);
    let (resource_type, id) = relative
        .split_once('/')
        .filter(|(resource_type, _)| matches!(*resource_type, "Library" | "ViewDefinition"))
        .ok_or_else(|| {
            OperationError::NotFound(format!(
                "subjectReference {reference} is not a Library or ViewDefinition of this server"
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
/// may read it; none, or more than one version, is a 404.
async fn find_canonical(
    state: &AppState,
    context: &OperationContext,
    canonical: &str,
) -> Result<Value, OperationError> {
    let (url, version) = split_canonical(canonical);
    for resource_type in ["Library", "ViewDefinition"] {
        match stored(state, context, resource_type, url, version).await? {
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

/// The identity of a resolved artifact for the job's terminology cache:
/// type, url and version.
fn terminology_key(artifact: &Value) -> String {
    format!(
        "{}|{}|{}",
        artifact["resourceType"].as_str().unwrap_or_default(),
        artifact["url"].as_str().unwrap_or_default(),
        artifact["version"].as_str().unwrap_or_default()
    )
}

/// `context` (0..*): inline artifacts dependencies may name.
fn context_artifacts(params: &Value) -> Result<Vec<Value>, OperationError> {
    parameters(params)
        .filter(|param| param["name"] == "context")
        .map(|param| {
            param
                .get("resource")
                .filter(|resource| resource["url"].is_string())
                .cloned()
                .ok_or_else(|| {
                    OperationError::InvalidParameters(
                        "context requires a canonical resource with a url".into(),
                    )
                })
        })
        .collect()
}

/// The values of `parameters` for `library`'s declared parameters, checked by
/// name and type; a declared one not supplied is NULL unless required.
fn bind_values(
    library: &SqlLibrary,
    params: &Value,
) -> Result<HashMap<String, SqlValue>, OperationError> {
    let supplied: Vec<&Value> = parameters(params)
        .filter(|param| param["name"] == "parameters")
        .collect();
    let values = match supplied.as_slice() {
        [] => Vec::new(),
        [param] => param["resource"]["parameter"]
            .as_array()
            .cloned()
            .unwrap_or_default(),
        _ => {
            return Err(OperationError::InvalidParameters(
                "parameters must occur at most once".into(),
            ));
        }
    };
    let invalid =
        |message: String| OperationError::InvalidParameters(format!("parameters: {message}"));
    let mut bound = HashMap::new();
    for value in &values {
        let name = value["name"]
            .as_str()
            .ok_or_else(|| invalid("every parameter needs a name".into()))?;
        let declared = library
            .parameters
            .iter()
            .find(|declared| declared.name == name)
            .ok_or_else(|| invalid(format!("{} declares no parameter {name}", library.display)))?;
        let field = declared.value_field();
        let raw = value.get(&field).ok_or_else(|| {
            invalid(format!(
                "{name} is {}, so it takes {field}",
                declared.fhir_type
            ))
        })?;
        let sql_value = match (declared.sql_type(), raw) {
            (Some("integer" | "bigint"), Value::Number(number)) => number
                .as_i64()
                .map(SqlValue::Integer)
                .ok_or_else(|| invalid(format!("{name} is not an integer")))?,
            (Some("boolean"), Value::Bool(flag)) => SqlValue::Boolean(*flag),
            (Some("numeric"), Value::Number(number)) => SqlValue::Text(number.to_string()),
            (Some(_), Value::String(text)) => SqlValue::Text(text.clone()),
            _ => return Err(invalid(format!("{name} has a value of the wrong type"))),
        };
        if bound.insert(name.to_owned(), sql_value).is_some() {
            return Err(invalid(format!("{name} is supplied twice")));
        }
    }
    if let Some(missing) = library
        .parameters
        .iter()
        .find(|declared| declared.required && !bound.contains_key(&declared.name))
    {
        return Err(invalid(format!("{} is required", missing.name)));
    }
    Ok(bound)
}

fn has(params: &Value, name: &str) -> bool {
    parameters(params).any(|param| param["name"] == name)
}

fn unprocessable(message: String) -> OperationError {
    OperationError::Api(ApiError::unprocessable_entity(message, None))
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{Engine, engine::general_purpose::STANDARD};
    use serde_json::json;

    fn query_library() -> SqlLibrary {
        SqlLibrary::from_json(&json!({
            "resourceType": "Library",
            "url": "http://example.org/Library/q",
            "type": {"coding": [{
                "system": "http://hl7.org/fhir/uv/sql-on-fhir/CodeSystem/LibraryTypesCodes",
                "code": "sql-query"}]},
            "parameter": [
                {"name": "from_date", "use": "in", "type": "date", "min": 1},
                {"name": "max_count", "use": "in", "type": "integer"}
            ],
            "content": [{"contentType": "application/sql", "data": STANDARD.encode("SELECT 1")}]
        }))
        .unwrap()
    }

    fn with(values: Value) -> Value {
        json!({"parameter": [{"name": "parameters", "resource": {
            "resourceType": "Parameters", "parameter": values}}]})
    }

    #[test]
    fn parameters_bind_by_name_and_declared_type() {
        let library = query_library();
        let bound = bind_values(
            &library,
            &with(json!([
                {"name": "from_date", "valueDate": "2026-01-01"},
                {"name": "max_count", "valueInteger": 5}
            ])),
        )
        .unwrap();
        assert!(matches!(&bound["from_date"], SqlValue::Text(date) if date == "2026-01-01"));
        assert!(matches!(bound["max_count"], SqlValue::Integer(5)));
    }

    #[test]
    fn unknown_mistyped_and_missing_parameters_are_rejected() {
        let library = query_library();
        for values in [
            json!([{"name": "from_date", "valueDate": "2026-01-01"},
                   {"name": "other", "valueString": "x"}]),
            json!([{"name": "from_date", "valueString": "2026-01-01"}]),
            json!([{"name": "max_count", "valueInteger": 1}]),
        ] {
            assert!(
                matches!(
                    bind_values(&library, &with(values.clone())),
                    Err(OperationError::InvalidParameters(_))
                ),
                "{values}"
            );
        }
    }

    #[test]
    fn canonicals_split_into_url_and_version() {
        assert_eq!(
            split_canonical("http://x/y|1.0"),
            ("http://x/y", Some("1.0"))
        );
        assert_eq!(split_canonical("http://x/y"), ("http://x/y", None));
        let artifact = json!({"url": "http://x/y", "version": "1.0"});
        assert!(matches_canonical(&artifact, "http://x/y", None));
        assert!(matches_canonical(&artifact, "http://x/y", Some("1.0")));
        assert!(!matches_canonical(&artifact, "http://x/y", Some("2.0")));
    }
}
