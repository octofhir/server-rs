//! `StructureMap/$transform` and `StructureMap/[id]/$transform`: run a StructureMap over a
//! resource.
//!
//! Inputs, after the FHIR operation and the Mapping Language IG's debug page:
//! - the map: the instance, `source` (canonical), `sourceMap` (a StructureMap resource) or
//!   `srcMap` (FML text; repeats, the first is the map to run, the others are available to it);
//! - `supportingMap` (canonical, repeats): more maps to import from;
//! - `content`: the input, a resource, or a Binary holding JSON (a logical model, another FHIR
//!   version);
//! - `model` (repeats): StructureDefinitions and ConceptMaps for this run only, alone or in a
//!   Bundle, as a resource or JSON text;
//! - `debug`: in the query or the body; returns the trace Parameters instead of the result.
//!
//! The result is the target resource; a target the server's FHIR version does not define (a
//! logical model, another release) comes back as a Binary. Imports are resolved from the
//! server's StructureMaps and the installed packages, ConceptMaps for `translate()` the same way.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use base64::Engine as _;
use fml_check::{CoreDefinitions, Release};
use fml_engine::{
    DebugReport, Engine, Error as MapError, ErrorKind, FhirPathExpressions, Limits, MapResolver,
    TranslateFuture, TranslateService, Translation, debug_parameters, translate,
};
use fml_model::{Locations, Span, StructureMap};
use fml_syntax::{LineIndex, Severity};
use octofhir_api::ApiError;
use serde_json::{Value, json};
use tokio::sync::OnceCell;

use super::library::Library;
use super::resolve::{self, ConceptMapCache, Models};
use crate::config::FmlConfig;
use crate::operations::handler::OperationLevel;
use crate::operations::{OperationContext, OperationError, OperationHandler};
use crate::server::AppState;

/// Handler for `$transform` on StructureMap.
pub struct TransformOperation {
    runner: Arc<Runner>,
}

/// What a run gave.
pub(crate) enum Ran {
    /// The debug trace Parameters (`debug=true`).
    Debug(Value),
    Done {
        target: Value,
        /// Values left out of the target.
        warnings: Vec<String>,
        /// The map that ran.
        map: Arc<StructureMap>,
        /// The server's FHIR release.
        release: Release,
    },
}

impl Ran {
    /// The response of `$transform` and `$extract`: the trace, or the target as returned by
    /// [`returned`].
    pub(crate) fn into_response(self) -> Value {
        match self {
            Ran::Debug(parameters) => parameters,
            Ran::Done {
                target,
                map,
                release,
                ..
            } => returned(&map, release, target),
        }
    }
}

/// Runs maps for the FML operations; shared by them.
pub(crate) struct Runner {
    pub(crate) config: FmlConfig,
    /// FHIRPath and core definitions are expensive to build; one of each per server.
    runtime: OnceCell<Arc<Runtime>>,
    /// The configured map folders, read on first use.
    library: OnceCell<Arc<Library>>,
}

/// What every transform shares.
struct Runtime {
    release: Release,
    core: Arc<CoreDefinitions>,
    expressions: Arc<FhirPathExpressions>,
}

impl TransformOperation {
    pub(crate) fn new(runner: Arc<Runner>) -> Self {
        Self { runner }
    }

    async fn transform(
        &self,
        state: &AppState,
        context: &OperationContext,
        instance: Option<&str>,
        params: &Value,
    ) -> Result<Value, OperationError> {
        let inputs = Inputs::read(params)?;
        if inputs.debug {
            self.runner.allow_debug(context)?;
        }

        // The map to run, and the maps it may use.
        let mut sources = Sources::default();
        let mut maps: Vec<StructureMap> = Vec::new();
        let primary = match (instance, &inputs) {
            (Some(id), _) => {
                let stored = context
                    .read(state, "StructureMap", id)
                    .await?
                    .ok_or_else(|| OperationError::NotFound(format!("StructureMap/{id}")))?;
                resolve::map_from_json(stored.resource, &format!("StructureMap/{id}"))?
            }
            (
                None,
                Inputs {
                    source_map: Some(map),
                    ..
                },
            ) => resolve::map_from_json(map.clone(), "sourceMap")?,
            (None, Inputs { src_maps, .. }) if !src_maps.is_empty() => {
                sources.compile(&src_maps[0], "srcMap")?
            }
            (
                None,
                Inputs {
                    source: Some(canonical),
                    ..
                },
            ) => {
                let library = self.runner.library().await?;
                resolve::structure_map(state, context, &library, canonical)
                    .await?
                    .ok_or_else(|| OperationError::NotFound(format!("StructureMap {canonical}")))?
                    .as_ref()
                    .clone()
            }
            _ => {
                return Err(OperationError::InvalidParameters(
                    "Name the map: call it on a StructureMap, or give `source`, `sourceMap` or \
                     `srcMap`"
                        .into(),
                ));
            }
        };
        maps.push(primary);
        let first_src = usize::from(instance.is_none() && inputs.source_map.is_none());
        for text in inputs.src_maps.iter().skip(first_src) {
            maps.push(sources.compile(text, "srcMap")?);
        }
        let library = self.runner.library().await?;
        for canonical in &inputs.supporting_maps {
            let map = resolve::structure_map(state, context, &library, canonical)
                .await?
                .ok_or_else(|| OperationError::NotFound(format!("supportingMap {canonical}")))?;
            maps.push(map.as_ref().clone());
        }
        let content = inputs.content()?;
        let debug = inputs.debug;
        self.runner
            .run(
                state,
                context,
                maps,
                &sources,
                content,
                inputs.models,
                debug,
            )
            .await
            .map(Ran::into_response)
    }
}

impl Runner {
    pub(crate) fn new(config: FmlConfig) -> Self {
        Self {
            config,
            runtime: OnceCell::new(),
            library: OnceCell::new(),
        }
    }

    /// The maps and ConceptMaps of `fml.map_folders`.
    pub(crate) async fn library(&self) -> Result<Arc<Library>, OperationError> {
        self.library
            .get_or_try_init(|| async {
                let folders = self.config.map_folders.clone();
                tokio::task::spawn_blocking(move || Arc::new(Library::load(&folders)))
                    .await
                    .map_err(|error| OperationError::Internal(format!("map folders: {error}")))
            })
            .await
            .cloned()
    }

    async fn runtime(&self, state: &AppState) -> Result<Arc<Runtime>, OperationError> {
        self.runtime
            .get_or_try_init(|| async {
                let release = Release::parse(&state.fhir_version).ok_or_else(|| {
                    OperationError::NotSupported(format!(
                        "$transform does not support FHIR {}",
                        state.fhir_version
                    ))
                })?;
                let expressions = FhirPathExpressions::new(release)
                    .await
                    .map_err(OperationError::Internal)?;
                Ok(Arc::new(Runtime {
                    release,
                    core: Arc::new(CoreDefinitions::new(release)),
                    expressions: Arc::new(expressions),
                }))
            })
            .await
            .cloned()
    }

    /// Runs `maps[0]` over `content`, the other maps (and what they all import) available to
    /// it. With `debug`, the trace Parameters; otherwise the result, or a 422 that says where
    /// the map failed.
    pub(crate) async fn run(
        &self,
        state: &AppState,
        context: &OperationContext,
        maps: Vec<StructureMap>,
        sources: &Sources,
        content: Value,
        models: Models,
        debug: bool,
    ) -> Result<Ran, OperationError> {
        let runtime = self.runtime(state).await?;
        let library = self.library().await?;
        let started = std::time::Instant::now();
        let maps = resolve::with_imports(state, context, &library, maps).await?;
        let resolved = started.elapsed();
        let definitions =
            resolve::definitions(state, context, runtime.core.clone(), &maps, &models).await?;
        let defined = started.elapsed();
        let limits = Limits {
            max_rules: self.config.max_rules,
            max_trace_events: self.config.max_trace_events,
            ..Limits::default()
        };
        let cancel = Arc::new(AtomicBool::new(false));
        let timer = {
            let cancel = cancel.clone();
            let timeout = Duration::from_millis(self.config.timeout_ms);
            tokio::spawn(async move {
                tokio::time::sleep(timeout).await;
                cancel.store(true, Ordering::Relaxed);
            })
        };

        // The engine's futures are not `Send`: it runs on a blocking thread that drives them,
        // with the runtime available for the ConceptMap lookups it makes.
        let handle = tokio::runtime::Handle::current();
        let translator = ServerTranslator {
            state: state.clone(),
            context: context.clone(),
            handle: handle.clone(),
            library,
            found: Mutex::new(HashMap::new()),
        };
        let expressions = runtime.expressions.clone();
        let cancel_flag = cancel.clone();
        let outcome = tokio::task::spawn_blocking(move || {
            let setup = std::time::Instant::now();
            let set = SharedMaps(maps[1..].to_vec());
            let engine = Engine::new(&definitions, SharedExpressions(expressions))
                .with_maps(&set)
                .with_concept_maps(&models.concept_maps)
                .with_translator(&translator)
                .with_limits(limits)
                .with_cancellation(&cancel_flag)
                .with_trace(debug);
            let primary = maps[0].clone();
            let built = setup.elapsed();
            let result = handle.block_on(engine.transform(&primary, &content));
            tracing::debug!(
                maps = maps.len(),
                map_set_ms = built.as_millis() as u64,
                engine_ms = (setup.elapsed() - built).as_millis() as u64,
                "fml transform ran"
            );
            (result, primary)
        })
        .await
        .map_err(|error| OperationError::Internal(format!("$transform: {error}")))?;
        timer.abort();
        tracing::debug!(
            imports_ms = resolved.as_millis() as u64,
            definitions_ms = (defined - resolved).as_millis() as u64,
            total_ms = started.elapsed().as_millis() as u64,
            "fml transform timings"
        );
        let (result, primary) = outcome;

        let spans = |source: Option<&str>, position: &str| sources.span(source, position);
        match result {
            Ok(transformed) => {
                if debug && let Some(trace) = &transformed.trace {
                    return Ok(Ran::Debug(debug_parameters(
                        &DebugReport {
                            result: Some(&transformed.target),
                            trace,
                            error: None,
                            warnings: &transformed.warnings,
                        },
                        &spans,
                    )));
                }
                Ok(Ran::Done {
                    target: transformed.target,
                    warnings: transformed.warnings,
                    map: primary,
                    release: runtime.release,
                })
            }
            Err(error) => {
                if debug && let Some(trace) = &error.trace {
                    return Ok(Ran::Debug(debug_parameters(
                        &DebugReport {
                            result: None,
                            trace,
                            error: Some(&error),
                            warnings: &[],
                        },
                        &spans,
                    )));
                }
                Err(failure(&error, &spans))
            }
        }
    }

    /// A debug trace holds every value the map reads and writes: only a user with the
    /// configured role may ask for one.
    pub(crate) fn allow_debug(&self, context: &OperationContext) -> Result<(), OperationError> {
        let allowed = context
            .auth()
            .and_then(|auth| auth.user.as_ref())
            .is_some_and(|user| user.has_role(&self.config.debug_role));
        if allowed {
            Ok(())
        } else {
            Err(OperationError::Api(ApiError::forbidden(format!(
                "debug=true needs the `{}` role: the trace holds the data the map reads",
                self.config.debug_role
            ))))
        }
    }
}

#[async_trait]
impl OperationHandler for TransformOperation {
    fn supports_level(&self, level: OperationLevel, resource_type: Option<&str>) -> bool {
        self.runner.config.enabled
            && matches!(level, OperationLevel::Type | OperationLevel::Instance)
            && resource_type.is_none_or(|t| t == "StructureMap")
    }

    fn code(&self) -> &str {
        "transform"
    }

    async fn handle_type(
        &self,
        state: &AppState,
        context: &OperationContext,
        resource_type: &str,
        params: &Value,
    ) -> Result<Value, OperationError> {
        self.check_type(resource_type)?;
        self.transform(state, context, None, params).await
    }

    async fn handle_instance(
        &self,
        state: &AppState,
        context: &OperationContext,
        resource_type: &str,
        id: &str,
        params: &Value,
    ) -> Result<Value, OperationError> {
        self.check_type(resource_type)?;
        self.transform(state, context, Some(id), params).await
    }
}

impl TransformOperation {
    fn check_type(&self, resource_type: &str) -> Result<(), OperationError> {
        if !self.runner.config.enabled {
            return Err(OperationError::NotSupported(
                "$transform is disabled on this server".into(),
            ));
        }
        if resource_type != "StructureMap" {
            return Err(OperationError::NotSupported(format!(
                "$transform is defined on StructureMap, not {resource_type}"
            )));
        }
        Ok(())
    }
}

/// The operation's inputs.
#[derive(Default)]
struct Inputs {
    source: Option<String>,
    source_map: Option<Value>,
    src_maps: Vec<String>,
    supporting_maps: Vec<String>,
    content: Option<Value>,
    models: Models,
    debug: bool,
}

impl Inputs {
    fn read(params: &Value) -> Result<Self, OperationError> {
        let mut inputs = Self::default();
        for parameter in params["parameter"].as_array().into_iter().flatten() {
            let text = || {
                ["valueUri", "valueCanonical", "valueUrl", "valueString"]
                    .iter()
                    .find_map(|key| parameter[*key].as_str())
                    .map(str::to_owned)
            };
            match parameter["name"].as_str().unwrap_or_default() {
                "source" => inputs.source = text(),
                "sourceMap" => inputs.source_map = parameter.get("resource").cloned(),
                "srcMap" => inputs.src_maps.extend(text()),
                "supportingMap" => inputs.supporting_maps.extend(text()),
                "content" => inputs.content = resource_or_json(parameter)?,
                "model" => {
                    if let Some(model) = resource_or_json(parameter)? {
                        inputs.models.add(model)?;
                    }
                }
                "debug" => {
                    inputs.debug = parameter["valueBoolean"].as_bool().unwrap_or(false)
                        || parameter["valueString"].as_str() == Some("true");
                }
                // A resource posted as the body: the content.
                "resource" if inputs.content.is_none() => {
                    inputs.content = resource_or_json(parameter)?;
                }
                _ => {}
            }
        }
        Ok(inputs)
    }

    /// The input to transform: the resource, or the JSON a Binary holds.
    fn content(&self) -> Result<Value, OperationError> {
        let content = self.content.as_ref().ok_or_else(|| {
            OperationError::InvalidParameters("Missing parameter `content`".into())
        })?;
        if content["resourceType"] != "Binary" {
            return Ok(content.clone());
        }
        let data = content["data"].as_str().unwrap_or_default();
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data)
            .map_err(|error| {
                OperationError::InvalidParameters(format!("content Binary data: {error}"))
            })?;
        serde_json::from_slice(&bytes).map_err(|error| {
            OperationError::InvalidParameters(format!("content Binary is not JSON: {error}"))
        })
    }
}

/// A parameter's resource, or the JSON text in its `valueString`.
fn resource_or_json(parameter: &Value) -> Result<Option<Value>, OperationError> {
    if let Some(resource) = parameter.get("resource") {
        return Ok(Some(resource.clone()));
    }
    match parameter["valueString"].as_str() {
        Some(text) => serde_json::from_str(text).map(Some).map_err(|error| {
            OperationError::InvalidParameters(format!(
                "`{}` is not JSON: {error}",
                parameter["name"].as_str().unwrap_or_default()
            ))
        }),
        None => Ok(None),
    }
}

/// FML texts given with `srcMap`, by map URL: for text positions in the trace and in errors.
#[derive(Default)]
pub(crate) struct Sources {
    by_url: HashMap<String, (String, LineIndex, Locations)>,
}

impl Sources {
    /// Compiles FML text; errors are a 422 with each problem at its line and column.
    fn compile(&mut self, text: &str, parameter: &str) -> Result<StructureMap, OperationError> {
        let compiled = fml_model::compile(text);
        let index = LineIndex::new(text);
        if compiled.has_errors() {
            let issues: Vec<Value> = compiled
                .diagnostics
                .iter()
                .map(|diagnostic| {
                    let at = index.line_col(text, diagnostic.range.start());
                    json!({
                        "severity": match diagnostic.severity {
                            Severity::Error => "error",
                            Severity::Warning => "warning",
                        },
                        "code": "invalid",
                        "diagnostics": diagnostic.message,
                        "expression": [parameter],
                        "extension": [
                            { "url": ISSUE_LINE, "valueInteger": at.line + 1 },
                            { "url": ISSUE_COL, "valueInteger": at.col_utf16 + 1 }
                        ]
                    })
                })
                .collect();
            return Err(OperationError::Api(ApiError::unprocessable_entity(
                format!("{parameter} does not compile"),
                Some(json!({ "resourceType": "OperationOutcome", "issue": issues })),
            )));
        }
        let url = compiled.map.url.clone().unwrap_or_default();
        self.by_url
            .insert(url, (text.to_owned(), index, compiled.locations));
        Ok(compiled.map)
    }

    fn span(&self, source: Option<&str>, position: &str) -> Option<Span> {
        let (text, index, locations) = self.by_url.get(source.unwrap_or_default())?;
        locations.span(text, index, position)
    }
}

const ISSUE_LINE: &str = "http://hl7.org/fhir/StructureDefinition/operationoutcome-issue-line";
const ISSUE_COL: &str = "http://hl7.org/fhir/StructureDefinition/operationoutcome-issue-col";

/// A failed transform as a 422 OperationOutcome: what went wrong, and where in which map.
fn failure(error: &MapError, spans: &dyn Fn(Option<&str>, &str) -> Option<Span>) -> OperationError {
    let code = match error.kind {
        ErrorKind::Failed => "processing",
        ErrorKind::Limit => "too-costly",
        ErrorKind::Cancelled => "timeout",
    };
    let mut issue = json!({ "severity": "error", "code": code, "diagnostics": error.message });
    if let Some(position) = &error.position {
        issue["expression"] = json!([position]);
        let mut extensions = Vec::new();
        if let Some(map) = &error.map {
            extensions.push(json!({
                "url": "http://hl7.org/fhir/StructureDefinition/operationoutcome-file",
                "valueString": map
            }));
        }
        if let Some(span) = spans(error.map.as_deref(), position) {
            extensions.push(json!({ "url": ISSUE_LINE, "valueInteger": span.start_row + 1 }));
            extensions.push(json!({ "url": ISSUE_COL, "valueInteger": span.start_column + 1 }));
        }
        if !extensions.is_empty() {
            issue["extension"] = Value::Array(extensions);
        }
    }
    OperationError::Api(ApiError::unprocessable_entity(
        error.message.clone(),
        Some(json!({ "resourceType": "OperationOutcome", "issue": [issue] })),
    ))
}

/// The result as returned: the resource, or a Binary when it is not a resource of the server's
/// FHIR version (a logical model, or a map into another release).
fn returned(map: &StructureMap, release: Release, target: Value) -> Value {
    let target_url = map.group.first().and_then(|group| {
        let input = group.input.iter().find(|input| input.mode == "target")?;
        let name = input.type_name.as_deref()?;
        Some(
            map.structure
                .iter()
                .find(|s| s.alias.as_deref() == Some(name))
                .map_or_else(|| name.to_owned(), |s| s.url.clone()),
        )
    });
    let other_release = target_url
        .as_deref()
        .and_then(fml_check::definitions::versioned_core)
        .and_then(|prefix| prefix.rsplit('/').next())
        .and_then(Release::from_url_version)
        .is_some_and(|target| target != release);
    let resource = target.get("resourceType").is_some();
    if resource && !other_release {
        return target;
    }
    let content_type = if resource {
        "application/fhir+json"
    } else {
        "application/json"
    };
    json!({
        "resourceType": "Binary",
        "contentType": content_type,
        "data": base64::engine::general_purpose::STANDARD.encode(target.to_string()),
    })
}

/// The maps a transform imports from, shared without copying: an import names a URL, or one
/// with `*` standing for any run of characters.
struct SharedMaps(Vec<Arc<StructureMap>>);

impl MapResolver for SharedMaps {
    fn resolve(&self, url: &str) -> Vec<Arc<StructureMap>> {
        self.0
            .iter()
            .filter(|map| {
                map.url
                    .as_deref()
                    .is_some_and(|candidate| match url.split_once('*') {
                        Some((prefix, suffix)) => {
                            candidate.len() >= prefix.len() + suffix.len()
                                && candidate.starts_with(prefix)
                                && candidate.ends_with(suffix)
                        }
                        None => candidate == url,
                    })
            })
            .cloned()
            .collect()
    }
}

/// FHIRPath shared by every transform.
struct SharedExpressions(Arc<FhirPathExpressions>);

impl fml_engine::Expressions for SharedExpressions {
    fn evaluate(
        &self,
        expression: &str,
        focus: Option<&fml_engine::Item>,
        variables: &[(String, fml_engine::Item)],
    ) -> impl Future<Output = Result<Vec<fml_engine::Item>, String>> {
        self.0.evaluate(expression, focus, variables)
    }
}

/// `translate()` with the server's ConceptMaps, those of its packages and of the configured map
/// folders, found as the caller, each once per transform. Codes are chosen as the reference engine chooses them.
struct ServerTranslator {
    state: AppState,
    context: OperationContext,
    handle: tokio::runtime::Handle,
    library: Arc<Library>,
    found: Mutex<ConceptMapCache>,
}

impl ServerTranslator {
    async fn concept_map(&self, url: &str) -> Result<Option<Arc<Value>>, String> {
        if let Some(found) = self
            .found
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(url)
        {
            return Ok(found.clone());
        }
        // The lookups are `Send` futures of the server; run them on its runtime.
        let (state, context, library, canonical) = (
            self.state.clone(),
            self.context.clone(),
            self.library.clone(),
            url.to_owned(),
        );
        let found = self
            .handle
            .spawn(
                async move { resolve::concept_map(&state, &context, &library, &canonical).await },
            )
            .await
            .map_err(|error| error.to_string())?
            .map_err(|error| error.to_string())?;
        self.found
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(url.to_owned(), found.clone());
        Ok(found)
    }
}

impl TranslateService for ServerTranslator {
    fn translate<'a>(
        &'a self,
        url: &'a str,
        system: Option<&'a str>,
        code: &'a str,
    ) -> TranslateFuture<'a> {
        Box::pin(async move {
            let concept_map = self
                .concept_map(url)
                .await?
                .ok_or_else(|| format!("ConceptMap `{url}` not found"))?;
            let found: Option<Translation> = translate(&concept_map, system, code)?;
            Ok(found)
        })
    }
}
